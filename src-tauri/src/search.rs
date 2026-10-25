use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Instant;

use aho_corasick::AhoCorasick;
use serde::Serialize;

use crate::notes::{
    MAX_FILE_BYTES, NoteEntry, canonical_root, ensure_within, list_note_entries_at, modified_ms,
    relative_note_path, worker_count,
};

pub const MAX_SEARCH_HITS: usize = 50;
pub const MAX_HITS_PER_NOTE: usize = 3;
pub const PREVIEW_CHARS: usize = 90;
/// Upper bound on scan workers for the disk path. Past this point the scan is
/// bound by disk and antivirus inspection rather than by cores, so extra
/// threads stop paying off.
pub const MAX_SEARCH_THREADS: usize = 8;
/// Upper bound on scan workers for the in-memory snapshot. There is no disk
/// latency to hide here, so the scan scales with cores until memory bandwidth
/// and scheduling overhead take over (measured on 32 logical cores: 8 threads
/// 24 ms p50, 16 threads 17 ms p50, 32 threads no better and a worse tail).
pub const MAX_SNAPSHOT_THREADS: usize = 16;
/// Hits per streamed batch (RFC-006 §4.5 / ADR-008).
pub const SEARCH_BATCH_SIZE: usize = 16;
/// Default budget for cached note bodies. RFC-006 §4.4: the snapshot is a
/// read-only cache, so memory is capped and overflow only costs speed.
pub const DEFAULT_CACHE_BUDGET_BYTES: usize = 256 * 1024 * 1024;
/// Optional override for the cache budget, in megabytes.
const CACHE_BUDGET_ENV: &str = "MDSTUDIO_SEARCH_CACHE_MB";
/// Warm-up reads notes in batches of this size before touching the snapshot.
const WARM_BATCH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    /// One-based line number in the note.
    pub line: u32,
    /// Context around the match, truncated by characters.
    pub preview: String,
}

/// Where the scanned bodies came from, so the UI can show whether a query was
/// served from the warm snapshot or fell back to the disk (RFC-006 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchSource {
    /// Every body came from the in-memory snapshot.
    Snapshot,
    /// Every body was read from disk (no warm snapshot for this root).
    Disk,
    /// Some bodies came from the snapshot and some from disk (evicted, not yet
    /// warmed, oversized, non-UTF-8).
    Mixed,
}

/// Events streamed to the frontend over a Tauri channel (ADR-008). `Done` is
/// sent exactly once per request and never after a cancellation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SearchEvent {
    Batch {
        hits: Vec<SearchHit>,
    },
    Done {
        total: u32,
        truncated: bool,
        #[serde(rename = "tookMs")]
        took_ms: u32,
        source: SearchSource,
    },
}

/// Newest request id started by the frontend. A worker whose id is older is
/// stale and exits at its next batch boundary.
static ACTIVE_REQUEST: AtomicU64 = AtomicU64::new(0);
/// Newest cancelled request id. Ids are monotonic, so `<=` cancels the target
/// and every older request while leaving newer requests untouched.
static CANCELLED_REQUEST: AtomicU64 = AtomicU64::new(0);

pub fn begin_request(request_id: u64) {
    ACTIVE_REQUEST.fetch_max(request_id, Ordering::SeqCst);
}

pub fn cancel_request(request_id: u64) {
    CANCELLED_REQUEST.fetch_max(request_id, Ordering::SeqCst);
}

fn request_is_cancelled(request_id: u64) -> bool {
    request_id < ACTIVE_REQUEST.load(Ordering::SeqCst)
        || request_id <= CANCELLED_REQUEST.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotState {
    Warming,
    Ready,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotStatus {
    pub state: SnapshotState,
    pub total: usize,
    pub cached: usize,
    pub bytes: usize,
    pub budget_bytes: usize,
    pub degraded_reason: Option<String>,
}

/// A note as the scanner sees it: metadata is always present once the workspace
/// has been listed, while `text` is present only when the snapshot holds it.
struct ScanItem {
    path: Arc<str>,
    title: Arc<str>,
    text: Option<Arc<str>>,
}

/// Scans every Markdown note under `root` and returns up to
/// `MAX_SEARCH_HITS` line hits. RFC-006: the warm path reads bodies from the
/// in-memory snapshot, the cold path walks the workspace. Both call the same
/// scanner, so an entry that is missing from the cache (evicted, not yet
/// warmed, oversized, non-UTF-8) simply falls back to disk without changing
/// results or order. This is the collecting form used by tests and the
/// benchmark; the app itself streams through `search_stream_blocking`.
pub fn search_notes_blocking(root: &str, query: &str) -> Result<Vec<SearchHit>, String> {
    let needle = query.trim();
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let matcher = Matcher::new(needle);
    let root_path = canonical_root(root)?;
    let (items, source) = scan_items_for(&root_path)?;

    parallel_scan(
        &matcher,
        &root_path,
        &items,
        MAX_SEARCH_HITS,
        scan_threads(source),
    )
}

/// Streams the same hits as `search_notes_blocking`: batches of at most
/// `SEARCH_BATCH_SIZE`, then one `Done` carrying the total, truncation flag,
/// elapsed time and data source (RFC-006 §4.5). `emit` returns `false` when the
/// receiver is gone (window closed or cancelled), which stops the scan without
/// sending `Done` and without failing the command.
pub fn search_stream_blocking(
    root: &str,
    query: &str,
    request_id: u64,
    emit: &mut dyn FnMut(SearchEvent) -> bool,
) -> Result<(), String> {
    begin_request(request_id);
    let started = Instant::now();
    let root_path = canonical_root(root)?;
    let needle = query.trim();

    if needle.is_empty() {
        let source = if active_snapshot(&root_path).is_some() {
            SearchSource::Snapshot
        } else {
            SearchSource::Disk
        };
        let _ = emit(SearchEvent::Done {
            total: 0,
            truncated: false,
            took_ms: 0,
            source,
        });
        return Ok(());
    }

    let matcher = Matcher::new(needle);
    let (items, hint) = scan_items_for(&root_path)?;
    let outcome = parallel_scan_stream(
        &matcher,
        &root_path,
        &items,
        MAX_SEARCH_HITS,
        scan_threads(hint),
        Some(request_id),
        emit,
    )?;

    if outcome.stopped {
        // Cancelled, superseded or the receiver went away: no `Done`.
        return Ok(());
    }

    let took_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    let _ = emit(SearchEvent::Done {
        total: outcome.hits as u32,
        truncated: outcome.hits >= MAX_SEARCH_HITS,
        took_ms,
        source: resolve_source(hint, outcome.cached_bodies, outcome.disk_bodies),
    });

    Ok(())
}

/// Snapshot items when the warm cache is ready, otherwise a disk-backed list.
/// In `Degraded` mode cached bodies are revalidated first, so a missed event can
/// only cost a re-read, never a stale hit (RFC-006 §4.6).
fn scan_items_for(root: &Path) -> Result<(Vec<ScanItem>, SearchSource), String> {
    if let Some(snapshot) = active_snapshot(root) {
        validate_degraded(&snapshot);
        if let Some(items) = snapshot.scan_items() {
            return Ok((items, SearchSource::Snapshot));
        }
    }

    let notes = list_note_entries_at(root)?;
    let items = notes
        .iter()
        .map(|note| ScanItem {
            path: Arc::from(note.path.as_str()),
            title: Arc::from(note.title.as_str()),
            text: None,
        })
        .collect();

    Ok((items, SearchSource::Disk))
}

/// Disk-only scan used by tests that need a baseline independent of whatever
/// snapshot happens to be installed.
#[cfg(test)]
fn search_disk_blocking(matcher: &Matcher, root: &Path) -> Result<Vec<SearchHit>, String> {
    let notes = list_note_entries_at(root)?;
    let items: Vec<ScanItem> = notes
        .iter()
        .map(|note| ScanItem {
            path: Arc::from(note.path.as_str()),
            title: Arc::from(note.title.as_str()),
            text: None,
        })
        .collect();

    parallel_scan(matcher, root, &items, MAX_SEARCH_HITS, MAX_SEARCH_THREADS)
}

fn scan_threads(source: SearchSource) -> usize {
    match source {
        SearchSource::Snapshot => MAX_SNAPSHOT_THREADS,
        _ => MAX_SEARCH_THREADS,
    }
}

fn resolve_source(hint: SearchSource, cached: usize, disk: usize) -> SearchSource {
    match (cached, disk) {
        (0, 0) => hint,
        (_, 0) => SearchSource::Snapshot,
        (0, _) => SearchSource::Disk,
        _ => SearchSource::Mixed,
    }
}

/// Collecting wrapper over `parallel_scan_stream`, used by tests and the
/// benchmark. Membership cannot be cancelled here, so no request id is passed.
fn parallel_scan(
    matcher: &Matcher,
    root: &Path,
    items: &[ScanItem],
    limit: usize,
    max_threads: usize,
) -> Result<Vec<SearchHit>, String> {
    let mut collected = Vec::new();
    {
        let mut emit = |event: SearchEvent| {
            if let SearchEvent::Batch { hits } = event {
                collected.extend(hits);
            }
            true
        };
        parallel_scan_stream(matcher, root, items, limit, max_threads, None, &mut emit)?;
    }
    Ok(collected)
}

/// One result range plus where its bodies came from.
struct ScanOutcome {
    hits: Vec<SearchHit>,
    cached_bodies: usize,
    disk_bodies: usize,
}

struct StreamOutcome {
    hits: usize,
    cached_bodies: usize,
    disk_bodies: usize,
    /// The scan stopped early: cancelled, superseded, or the receiver is gone.
    stopped: bool,
}

/// Batch buffer for one streamed search. Hits are grouped as the chunks are
/// joined in path order, so batches keep the same order a serial scan would
/// produce and the global cap only ever drops the tail.
struct StreamState {
    request_id: Option<u64>,
    hits: usize,
    cached_bodies: usize,
    disk_bodies: usize,
    pending: Vec<SearchHit>,
    stopped: bool,
}

impl StreamState {
    fn new(request_id: Option<u64>) -> Self {
        Self {
            request_id,
            hits: 0,
            cached_bodies: 0,
            disk_bodies: 0,
            pending: Vec::with_capacity(SEARCH_BATCH_SIZE),
            stopped: false,
        }
    }

    fn absorb(&mut self, outcome: ScanOutcome, limit: usize) {
        self.cached_bodies += outcome.cached_bodies;
        self.disk_bodies += outcome.disk_bodies;

        let take = outcome.hits.len().min(limit.saturating_sub(self.hits));
        self.hits += take;
        self.pending.extend(outcome.hits.into_iter().take(take));
    }

    /// Sends every full batch plus the remainder. Checking the request here is
    /// what makes cancellation take effect at batch boundaries.
    fn flush(&mut self, emit: &mut dyn FnMut(SearchEvent) -> bool) {
        while self.pending.len() >= SEARCH_BATCH_SIZE {
            if !self.send(SEARCH_BATCH_SIZE, emit) {
                return;
            }
        }
        if !self.pending.is_empty() {
            let remainder = self.pending.len();
            self.send(remainder, emit);
        }
    }

    fn send(&mut self, len: usize, emit: &mut dyn FnMut(SearchEvent) -> bool) -> bool {
        if self.stopped {
            return false;
        }
        if self.request_id.is_some_and(request_is_cancelled) {
            self.stopped = true;
            return false;
        }

        let batch: Vec<SearchHit> = self.pending.drain(..len).collect();
        if !emit(SearchEvent::Batch { hits: batch }) {
            self.stopped = true;
            return false;
        }
        true
    }

    fn finish(self) -> StreamOutcome {
        StreamOutcome {
            hits: self.hits,
            cached_bodies: self.cached_bodies,
            disk_bodies: self.disk_bodies,
            stopped: self.stopped,
        }
    }
}

/// Splits the path-sorted item list into contiguous ranges, scans them in
/// parallel and emits batches as each range joins in order. Chunks are joined
/// in list order, so this reproduces the serial hit order and the global cap
/// only ever drops the tail.
fn parallel_scan_stream(
    matcher: &Matcher,
    root: &Path,
    items: &[ScanItem],
    limit: usize,
    max_threads: usize,
    request_id: Option<u64>,
    emit: &mut dyn FnMut(SearchEvent) -> bool,
) -> Result<StreamOutcome, String> {
    let mut state = StreamState::new(request_id);

    if items.len() < 2 {
        state.absorb(scan_items(matcher, root, items, limit), limit);
        state.flush(emit);
        return Ok(state.finish());
    }

    let workers = worker_count(max_threads).min(items.len());
    let chunk_size = items.len().div_ceil(workers);
    let mut panicked = false;

    thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk_size)
            .map(|chunk| scope.spawn(move || scan_items(matcher, root, chunk, limit)))
            .collect();

        for handle in handles {
            if state.stopped {
                break;
            }
            if state.request_id.is_some_and(request_is_cancelled) {
                state.stopped = true;
                break;
            }
            match handle.join() {
                Ok(outcome) => {
                    state.absorb(outcome, limit);
                    state.flush(emit);
                }
                Err(_) => {
                    panicked = true;
                    break;
                }
            }
        }
    });

    if panicked {
        // A partially scanned workspace must never be reported as a complete
        // result, so a worker panic fails the whole search.
        return Err("search worker thread panicked".to_string());
    }

    Ok(state.finish())
}

/// One contiguous range of the path-sorted item list. Stops at `limit` hits or
/// when the range is exhausted, exactly like a serial scan would.
fn scan_items(matcher: &Matcher, root: &Path, items: &[ScanItem], limit: usize) -> ScanOutcome {
    let mut hits = Vec::new();
    let mut cached_bodies = 0usize;
    let mut disk_bodies = 0usize;

    for item in items {
        if hits.len() >= limit {
            break;
        }

        let cached;
        let text: &str = match &item.text {
            Some(text) => {
                cached_bodies += 1;
                text
            }
            None => {
                let target = match resolve_search_target(root, &item.path) {
                    Ok(path) => path,
                    Err(_) => continue,
                };
                match fs::read_to_string(&target) {
                    Ok(text) => {
                        cached = text;
                        disk_bodies += 1;
                        &cached
                    }
                    Err(_) => continue,
                }
            }
        };

        collect_note_hits(matcher, &item.path, &item.title, text, &mut hits, limit);
    }

    ScanOutcome {
        hits,
        cached_bodies,
        disk_bodies,
    }
}

fn collect_note_hits(
    matcher: &Matcher,
    path: &str,
    title: &str,
    text: &str,
    hits: &mut Vec<SearchHit>,
    limit: usize,
) {
    match &matcher.kind {
        // ASCII needles scan the whole body once instead of calling the
        // automaton for every line; the automaton's prefilter then works on
        // megabytes at a time instead of ~100-byte slices.
        MatcherKind::Ascii(ascii) => {
            collect_ascii_hits(ascii, matcher.needle_chars, path, title, text, hits, limit)
        }
        MatcherKind::Unicode(_) => collect_line_hits(matcher, path, title, text, hits, limit),
    }
}

fn collect_ascii_hits(
    ascii: &AhoCorasick,
    needle_chars: usize,
    path: &str,
    title: &str,
    text: &str,
    hits: &mut Vec<SearchHit>,
    limit: usize,
) {
    let mut note_hits = 0;
    let mut line: u32 = 0;
    let mut last_line: u32 = 0;
    let mut scanned = 0usize;
    let mut line_start = 0usize;

    for found in ascii.find_iter(text) {
        if note_hits >= MAX_HITS_PER_NOTE || hits.len() >= limit {
            break;
        }

        let offset = found.start();
        let prefix = &text[scanned..offset];
        line += prefix.matches('\n').count() as u32;
        if let Some(position) = prefix.rfind('\n') {
            line_start = scanned + position + 1;
        }
        scanned = offset;

        let line_number = line + 1;
        // One hit per line, matching the per-line scanner's first-match rule.
        if line_number == last_line {
            continue;
        }

        let line_end = text[offset..]
            .find('\n')
            .map(|position| offset + position)
            .unwrap_or(text.len());
        let raw_line = &text[line_start..line_end];
        let trimmed = raw_line.trim();
        let leading = raw_line.len() - raw_line.trim_start().len();
        if offset < line_start + leading {
            continue;
        }

        let char_index = text[line_start + leading..offset].chars().count();
        hits.push(SearchHit {
            path: path.to_string(),
            title: title.to_string(),
            line: line_number,
            preview: build_preview(trimmed, char_index, needle_chars),
        });
        note_hits += 1;
        last_line = line_number;
    }
}

/// Unicode needles fall back to per-line matching: `to_lowercase` can change a
/// string's byte and character length, so a byte offset in the folded text is
/// not a valid offset in the original body.
fn collect_line_hits(
    matcher: &Matcher,
    path: &str,
    title: &str,
    text: &str,
    hits: &mut Vec<SearchHit>,
    limit: usize,
) {
    let mut note_hits = 0;

    for (index, line) in text.lines().enumerate() {
        if note_hits >= MAX_HITS_PER_NOTE || hits.len() >= limit {
            break;
        }
        let trimmed = line.trim();
        if let Some(char_index) = matcher.find(trimmed) {
            hits.push(SearchHit {
                path: path.to_string(),
                title: title.to_string(),
                line: (index + 1) as u32,
                preview: build_preview(trimmed, char_index, matcher.needle_chars),
            });
            note_hits += 1;
        }
    }
}

/// Paths produced by `walk` are already relative, normalized, and symlink-free.
/// Re-running `canonicalize` for every note only repeats syscalls, so the search
/// path validates lexically and rejects symlinks with a single `lstat`.
fn resolve_search_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    resolve_search_target_with_metadata(root, relative).map(|(path, _)| path)
}

/// Same validation, but hands back the metadata the check already read so the
/// warm path does not need a second `stat` for the mtime stamp.
fn resolve_search_target_with_metadata(
    root: &Path,
    relative: &str,
) -> Result<(PathBuf, fs::Metadata), String> {
    let cleaned = relative_note_path(relative)?;
    let candidate = root.join(cleaned);
    ensure_within(root, &candidate)?;

    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|error| format!("cannot read note metadata: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("note path is a symlink".to_string());
    }
    if !metadata.is_file() {
        return Err("note path is not a file".to_string());
    }

    Ok((candidate, metadata))
}

// ---------------------------------------------------------------------------
// L2 snapshot: a read-only cache of note bodies, keyed by workspace.
// ---------------------------------------------------------------------------

struct SnapshotNote {
    path: Arc<str>,
    title: Arc<str>,
    /// `None` means "not cached"; the scanner then reads this note from disk.
    text: Option<Arc<str>>,
    /// Bytes of the cached body, `0` when nothing is cached. Compared against
    /// the on-disk length when the watcher is degraded (RFC-006 §4.6).
    size: usize,
    /// Modification time (ms since the epoch) of the file this body came from;
    /// `0` means unknown, and validation then compares the length only.
    modified_ms: u64,
    /// Monotonic use counter, `0` when the note holds no text.
    last_used: u64,
    /// Whether warm-up still has to attempt this note.
    needs_warm: bool,
}

struct SnapshotInner {
    root: PathBuf,
    state: SnapshotState,
    degraded_reason: Option<String>,
    /// Path-sorted, mirroring `list_notes`, so hit order is independent of cache.
    notes: Vec<SnapshotNote>,
    index: HashMap<String, usize>,
    /// `last_used` -> path. Eviction pops the front (oldest).
    lru: BTreeMap<u64, String>,
    bytes: usize,
    budget: usize,
    tick: u64,
    /// True while a warm pass owns this snapshot, so refreshes do not start a
    /// second pass over the same notes.
    warming: bool,
}

pub struct Snapshot {
    inner: Mutex<SnapshotInner>,
}

impl Snapshot {
    fn new(root: PathBuf, entries: &[NoteEntry], budget: usize) -> Self {
        let notes = entries
            .iter()
            .map(|entry| SnapshotNote {
                path: Arc::from(entry.path.as_str()),
                title: Arc::from(entry.title.as_str()),
                text: None,
                size: 0,
                modified_ms: entry.modified_ms.unwrap_or(0),
                last_used: 0,
                needs_warm: true,
            })
            .collect::<Vec<_>>();
        let index = notes
            .iter()
            .enumerate()
            .map(|(position, note)| (note.path.to_string(), position))
            .collect();

        Self {
            inner: Mutex::new(SnapshotInner {
                root,
                state: SnapshotState::Warming,
                degraded_reason: None,
                notes,
                index,
                lru: BTreeMap::new(),
                bytes: 0,
                budget,
                tick: 0,
                warming: true,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SnapshotInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn status(&self) -> SnapshotStatus {
        let inner = self.lock();
        SnapshotStatus {
            state: inner.state,
            total: inner.notes.len(),
            cached: inner
                .notes
                .iter()
                .filter(|note| note.text.is_some())
                .count(),
            bytes: inner.bytes,
            budget_bytes: inner.budget,
            degraded_reason: inner.degraded_reason.clone(),
        }
    }

    /// A stable copy of the note list for scanning. Copies only `Arc`s, so the
    /// lock is released before the scan and the bodies are never duplicated.
    /// `Degraded` still serves cached bodies because callers revalidate them by
    /// mtime+size first (RFC-006 §4.6); `Warming` falls back to disk.
    fn scan_items(&self) -> Option<Vec<ScanItem>> {
        let inner = self.lock();
        if !matches!(inner.state, SnapshotState::Ready | SnapshotState::Degraded) {
            return None;
        }

        Some(
            inner
                .notes
                .iter()
                .map(|note| ScanItem {
                    path: Arc::clone(&note.path),
                    title: Arc::clone(&note.title),
                    text: note.text.clone(),
                })
                .collect(),
        )
    }

    /// Claims up to `WARM_BATCH` notes that still need a read. Marking them
    /// here (rather than after the read) keeps a failed note from being retried
    /// forever; a refresh with fresh entries adds it back as new.
    fn pending_batch(&self) -> Vec<Arc<str>> {
        let mut inner = self.lock();
        let mut batch = Vec::with_capacity(WARM_BATCH);
        for note in inner.notes.iter_mut() {
            if batch.len() == WARM_BATCH {
                break;
            }
            if note.needs_warm {
                note.needs_warm = false;
                batch.push(Arc::clone(&note.path));
            }
        }
        batch
    }
}

impl SnapshotInner {
    /// Fills in the cache for a freshly read note without ever exceeding the
    /// budget. Warm-up uses this so a workspace larger than the budget caches a
    /// prefix instead of reading gigabytes to keep the tail.
    fn warm_text(&mut self, path: &str, text: Arc<str>, modified_ms: u64) {
        let Some(&index) = self.index.get(path) else {
            return;
        };
        if text.len() > self.budget || self.bytes + text.len() > self.budget {
            return;
        }
        self.replace_text(index, text, modified_ms);
    }

    /// Caches a body written by this app. Evicts least-recently-used text to
    /// stay inside the budget (RFC-006 §4.4), which only costs speed later.
    fn cache_text(&mut self, path: &str, text: Arc<str>, modified_ms: u64) {
        let Some(&index) = self.index.get(path) else {
            return;
        };
        if text.len() > self.budget {
            self.drop_text(index);
            return;
        }
        self.replace_text(index, text, modified_ms);
        self.evict_to_budget();
    }

    fn replace_text(&mut self, index: usize, text: Arc<str>, modified_ms: u64) {
        let path = self.notes[index].path.to_string();
        self.drop_text(index);

        self.tick += 1;
        let note = &mut self.notes[index];
        note.last_used = self.tick;
        note.size = text.len();
        note.modified_ms = modified_ms;
        self.bytes += text.len();
        note.text = Some(text);
        self.lru.insert(self.tick, path);
    }

    fn drop_text(&mut self, index: usize) {
        let note = &mut self.notes[index];
        if let Some(old) = note.text.take() {
            self.bytes = self.bytes.saturating_sub(old.len());
        }
        note.size = 0;
        if note.last_used != 0 {
            self.lru.remove(&note.last_used);
            note.last_used = 0;
        }
    }

    fn evict_to_budget(&mut self) {
        while self.bytes > self.budget {
            let Some((_, victim)) = self.lru.pop_first() else {
                break;
            };
            let Some(&index) = self.index.get(&victim) else {
                continue;
            };
            self.drop_text(index);
        }
    }

    /// Inserts a note at its sorted position, keeping the ordering key used by
    /// `list_notes` (case-insensitive path, then exact path).
    fn insert_note(
        &mut self,
        path: String,
        title: String,
        text: Option<Arc<str>>,
        modified_ms: u64,
    ) {
        let key = path.to_ascii_lowercase();
        let position = self.notes.partition_point(|note| {
            let note_key = note.path.to_ascii_lowercase();
            match note_key.cmp(&key) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => note.path.as_ref() < path.as_str(),
            }
        });

        let mut note = SnapshotNote {
            path: Arc::from(path.as_str()),
            title: Arc::from(title.as_str()),
            text: None,
            size: 0,
            modified_ms,
            last_used: 0,
            needs_warm: text.is_none(),
        };

        if let Some(text) = text {
            if text.len() <= self.budget {
                self.tick += 1;
                note.last_used = self.tick;
                note.size = text.len();
                self.bytes += text.len();
                self.lru.insert(note.last_used, path.clone());
                note.text = Some(text);
            }
        }

        self.notes.insert(position, note);
        for offset in position..self.notes.len() {
            let path = self.notes[offset].path.to_string();
            self.index.insert(path, offset);
        }

        self.evict_to_budget();
    }

    /// Returns whether the path was present, so callers can tell a real removal
    /// from a stale event about a path that was never listed.
    fn remove_path(&mut self, path: &str) -> bool {
        let Some(position) = self.index.remove(path) else {
            return false;
        };

        self.drop_text(position);
        self.notes.remove(position);
        for offset in position..self.notes.len() {
            self.index
                .insert(self.notes[offset].path.to_string(), offset);
        }
        true
    }

    /// Reconciles the note list with a fresh `list_notes` result. Existing
    /// bodies are kept by path; new paths are queued for warm-up; removed paths
    /// drop their text. Rebuilding is O(n) but only runs on workspace refresh.
    fn reconcile(&mut self, entries: &[NoteEntry]) {
        let mut previous: HashMap<String, SnapshotNote> = HashMap::with_capacity(self.notes.len());
        for note in std::mem::take(&mut self.notes) {
            previous.insert(note.path.to_string(), note);
        }

        self.index.clear();
        self.lru.clear();
        self.bytes = 0;
        self.notes = Vec::with_capacity(entries.len());

        for entry in entries {
            let cached = previous.remove(&entry.path);
            let stamp = entry.modified_ms.unwrap_or(0);
            // A body whose recorded mtime no longer matches the fresh listing is
            // stale. Dropping it here is what makes a manual refresh reconcile
            // external edits even when the watcher never reported them.
            let text = cached.as_ref().and_then(|note| {
                if note.text.is_some() && note.modified_ms == stamp {
                    note.text.clone()
                } else {
                    None
                }
            });
            let has_text = text.is_some();
            let last_used = cached.as_ref().map(|note| note.last_used).unwrap_or(0);
            let needs_warm = match (&text, cached.as_ref()) {
                (Some(_), Some(note)) => note.needs_warm,
                _ => true,
            };

            if let Some(text) = &text {
                self.bytes += text.len();
                self.lru.insert(last_used, entry.path.clone());
            }

            self.index.insert(entry.path.clone(), self.notes.len());
            self.notes.push(SnapshotNote {
                path: Arc::from(entry.path.as_str()),
                title: Arc::from(entry.title.as_str()),
                size: text.as_ref().map(|body| body.len()).unwrap_or(0),
                text,
                modified_ms: stamp,
                last_used: if has_text { last_used } else { 0 },
                needs_warm,
            });
        }

        self.evict_to_budget();
    }
}

static SNAPSHOT: OnceLock<Mutex<Option<Arc<Snapshot>>>> = OnceLock::new();

fn snapshot_slot() -> &'static Mutex<Option<Arc<Snapshot>>> {
    SNAPSHOT.get_or_init(|| Mutex::new(None))
}

fn active_snapshot(root: &Path) -> Option<Arc<Snapshot>> {
    let slot = snapshot_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let snapshot = slot.as_ref()?;
    if snapshot.lock().root != root {
        return None;
    }
    Some(Arc::clone(snapshot))
}

fn is_current(snapshot: &Arc<Snapshot>) -> bool {
    let slot = snapshot_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, snapshot))
}

fn cache_budget() -> usize {
    let megabytes = std::env::var(CACHE_BUDGET_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_CACHE_BUDGET_BYTES / (1024 * 1024));
    megabytes.saturating_mul(1024 * 1024)
}

/// Installs or reconciles the snapshot for `root` and warms it on a background
/// thread. Called after every `list_notes`, so opening a workspace starts the
/// warm-up and later refreshes only queue newly seen notes.
pub fn start_warm(root: &str, entries: &[NoteEntry]) {
    let Ok(root_path) = canonical_root(root) else {
        return;
    };

    let Some(snapshot) = prepare(&root_path, entries) else {
        return;
    };

    // `prepare` already claimed the warm flag for this pass.
    let _ = thread::Builder::new()
        .name("search-warm".to_string())
        .spawn(move || warm_snapshot(&snapshot));
}

/// Starts a warm pass unless one already owns the snapshot, so a validation
/// refill cannot race the initial warm-up.
fn spawn_warm(snapshot: Arc<Snapshot>) {
    {
        let mut inner = snapshot.lock();
        if inner.warming {
            return;
        }
        inner.warming = true;
    }

    let _ = thread::Builder::new()
        .name("search-warm".to_string())
        .spawn(move || warm_snapshot(&snapshot));
}

fn prepare(root: &Path, entries: &[NoteEntry]) -> Option<Arc<Snapshot>> {
    let mut slot = snapshot_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(existing) = slot.as_ref() {
        let mut inner = existing.lock();
        if inner.root == root {
            if inner.warming {
                return None;
            }
            inner.reconcile(entries);
            if inner.state == SnapshotState::Degraded {
                // A workspace that could not be read before may have recovered.
                inner.degraded_reason = None;
                inner.state = SnapshotState::Ready;
            }
            if !inner.notes.iter().any(|note| note.needs_warm) {
                return None;
            }
            inner.warming = true;
            return Some(Arc::clone(existing));
        }
    }

    let snapshot = Arc::new(Snapshot::new(root.to_path_buf(), entries, cache_budget()));
    *slot = Some(Arc::clone(&snapshot));
    Some(snapshot)
}

fn warm_snapshot(snapshot: &Arc<Snapshot>) {
    let root = snapshot.lock().root.clone();
    let mut attempted = 0usize;
    let mut loaded = 0usize;

    loop {
        if !is_current(snapshot) {
            return;
        }

        {
            // Once the remaining budget cannot hold even the largest note, no
            // future read can be cached; stop instead of streaming the whole
            // workspace for nothing.
            let mut inner = snapshot.lock();
            if inner.budget > 0
                && inner.budget.saturating_sub(inner.bytes) < MAX_FILE_BYTES as usize
            {
                inner
                    .notes
                    .iter_mut()
                    .for_each(|note| note.needs_warm = false);
                break;
            }
        }

        let batch = snapshot.pending_batch();
        if batch.is_empty() {
            break;
        }

        let mut texts: Vec<(String, Arc<str>, u64)> = Vec::with_capacity(batch.len());
        for path in &batch {
            attempted += 1;
            let Ok((target, metadata)) = resolve_search_target_with_metadata(&root, path) else {
                continue;
            };
            let Ok(text) = fs::read_to_string(&target) else {
                continue;
            };
            texts.push((
                path.to_string(),
                Arc::from(text),
                modified_ms(&metadata).unwrap_or(0),
            ));
        }
        loaded += texts.len();

        let mut inner = snapshot.lock();
        for (path, text, stamp) in texts {
            inner.warm_text(&path, text, stamp);
        }
    }

    let mut inner = snapshot.lock();
    if inner.state == SnapshotState::Warming {
        inner.state = if inner.budget > 0 && attempted > 0 && loaded == 0 {
            inner.degraded_reason = Some("workspace notes could not be read".to_string());
            SnapshotState::Degraded
        } else {
            SnapshotState::Ready
        };
    }
    inner.warming = false;
}

/// Synchronous warm-up, used by the benchmark and tests.
pub fn warm_snapshot_blocking(root: &str) -> Result<SnapshotStatus, String> {
    let root_path = canonical_root(root)?;
    let entries = list_note_entries_at(&root_path)?;
    let snapshot = Arc::new(Snapshot::new(root_path, &entries, cache_budget()));
    {
        let mut slot = snapshot_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(Arc::clone(&snapshot));
    }
    warm_snapshot(&snapshot);
    Ok(snapshot.status())
}

/// Lists a workspace and reports `(notes, json_bytes)`: what the frontend
/// receives when a workspace opens (RFC-006 §4.7). The benchmark times it.
pub fn list_notes_report(root: &str) -> Result<(usize, usize), String> {
    let root_path = canonical_root(root)?;
    let entries = list_note_entries_at(&root_path)?;
    let bytes = serde_json::to_string(&entries)
        .map_err(|error| error.to_string())?
        .len();
    Ok((entries.len(), bytes))
}

/// Returns the current snapshot status, or `None` when no workspace has been
/// opened yet.
pub fn snapshot_status() -> Option<SnapshotStatus> {
    let slot = snapshot_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.as_ref().map(|snapshot| snapshot.status())
}

fn title_for(relative: &str) -> String {
    Path::new(relative)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| relative.to_string())
}

/// Snapshot keys are exactly the relative paths produced by `list_notes`
/// (forward slashes, no `.` segments), so a caller-supplied path is normalized
/// the same way before it can match — or be inserted as a duplicate.
fn cache_key(relative: &str) -> Option<String> {
    relative_note_path(relative).ok()?;
    normalized_relative(relative)
}

/// Forward-slash form of any workspace-relative path. Unlike `cache_key` this
/// accepts directories, so a removed folder can drop its notes.
fn normalized_relative(relative: &str) -> Option<String> {
    let candidate = Path::new(relative);
    if candidate.is_absolute() {
        return None;
    }
    let parts: Vec<String> = candidate
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

/// Last-write stamp used to decide whether a cached body still matches disk.
fn modified_stamp(path: &Path) -> u64 {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| modified_ms(&metadata))
        .unwrap_or(0)
}

/// Mirrors a successful in-app write into the snapshot. The cache never decides
/// what to save (ADR-005: `write_note` still compares revisions on disk); it
/// only avoids serving the body we just replaced.
pub(crate) fn cache_upsert(root: &Path, relative: &str, text: &str) {
    let Some(snapshot) = active_snapshot(root) else {
        return;
    };
    let Some(key) = cache_key(relative) else {
        return;
    };
    let stamp = modified_stamp(&root.join(&key));
    let mut inner = snapshot.lock();
    let title = title_for(&key);

    match inner.index.get(&key).copied() {
        Some(index) => {
            inner.notes[index].title = Arc::from(title.as_str());
            inner.cache_text(&key, Arc::from(text), stamp);
        }
        None => inner.insert_note(key, title, Some(Arc::from(text)), stamp),
    }
}

pub(crate) fn cache_remove(root: &Path, relative: &str) {
    let Some(snapshot) = active_snapshot(root) else {
        return;
    };
    let Some(key) = cache_key(relative) else {
        return;
    };
    snapshot.lock().remove_path(&key);
}

pub(crate) fn cache_rename(root: &Path, from: &str, to: &str) {
    let Some(snapshot) = active_snapshot(root) else {
        return;
    };
    let (Some(from), Some(to)) = (cache_key(from), cache_key(to)) else {
        return;
    };
    let stamp = modified_stamp(&root.join(&to));
    let mut inner = snapshot.lock();
    let Some(position) = inner.index.get(&from).copied() else {
        return;
    };

    let text = inner.notes[position].text.clone();
    inner.drop_text(position);
    inner.notes.remove(position);
    for offset in position..inner.notes.len() {
        let path = inner.notes[offset].path.to_string();
        inner.index.insert(path, offset);
    }

    inner.insert_note(to.clone(), title_for(&to), text, stamp);
}

/// Result of mirroring one externally changed path into the snapshot. The
/// variants that change the note list carry what the UI needs to patch its tree
/// without re-listing the whole workspace (ADR-009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReloadOutcome {
    /// The path appeared in the workspace; this entry should be inserted.
    Inserted(NoteEntry),
    /// An existing entry got a new body or timestamp.
    Updated,
    /// The path no longer exists and the entry was dropped.
    Removed,
    /// The event did not concern a listed note (temp files, unknown paths).
    Unchanged,
    /// The path exists but its body could not be read; any cached body is gone.
    Failed,
}

/// Builds the tree-facing entry for a snapshot key.
fn entry_for_key(key: &str, modified_ms: u64) -> NoteEntry {
    let name = key.rsplit('/').next().unwrap_or(key).to_string();
    NoteEntry {
        path: key.to_string(),
        name,
        title: title_for(key),
        modified_ms: if modified_ms == 0 {
            None
        } else {
            Some(modified_ms)
        },
    }
}

/// Re-reads one note and mirrors it into the snapshot. This is the watcher's
/// single entry point (ADR-009): create, modify, remove and rename all reduce to
/// "look at disk now", which also handles editors that save atomically through a
/// temporary file and a rename.
pub(crate) fn cache_reload(root: &Path, relative: &str) -> ReloadOutcome {
    let Some(snapshot) = active_snapshot(root) else {
        return ReloadOutcome::Unchanged;
    };
    let Some(key) = cache_key(relative) else {
        return ReloadOutcome::Unchanged;
    };

    let (target, metadata) = match resolve_search_target_with_metadata(root, &key) {
        Ok(found) => found,
        Err(_) => {
            return if snapshot.lock().remove_path(&key) {
                ReloadOutcome::Removed
            } else {
                ReloadOutcome::Unchanged
            };
        }
    };
    let stamp = modified_ms(&metadata).unwrap_or(0);

    // Too large or not valid UTF-8: keep the entry so the note stays listed, but
    // drop any body, which sends the next query to disk for it.
    let text = if metadata.len() <= MAX_FILE_BYTES {
        fs::read_to_string(&target).ok()
    } else {
        None
    };

    let entry = entry_for_key(&key, stamp);
    let title = entry.title.clone();
    let mut inner = snapshot.lock();
    let existing = inner.index.get(&key).copied();
    match (&text, existing) {
        (Some(text), Some(index)) => {
            inner.notes[index].title = Arc::from(title.as_str());
            inner.cache_text(&key, Arc::from(text.as_str()), stamp);
            ReloadOutcome::Updated
        }
        (Some(text), None) => {
            inner.insert_note(key, title, Some(Arc::from(text.as_str())), stamp);
            ReloadOutcome::Inserted(entry)
        }
        (None, Some(index)) => {
            inner.notes[index].modified_ms = stamp;
            inner.drop_text(index);
            ReloadOutcome::Failed
        }
        (None, None) => {
            inner.insert_note(key, title, None, stamp);
            ReloadOutcome::Inserted(entry)
        }
    }
}

/// Drops every note under a removed directory and returns the dropped paths so
/// the tree can drop them too.
pub(crate) fn cache_remove_subtree(root: &Path, relative: &str) -> Vec<String> {
    let Some(snapshot) = active_snapshot(root) else {
        return Vec::new();
    };
    let Some(key) = normalized_relative(relative) else {
        return Vec::new();
    };

    let prefix = format!("{key}/");
    let mut inner = snapshot.lock();
    let victims: Vec<String> = inner
        .notes
        .iter()
        .filter(|note| note.path.as_ref() == key || note.path.starts_with(&prefix))
        .map(|note| note.path.to_string())
        .collect();

    for path in &victims {
        inner.remove_path(path);
    }
    victims
}

/// Public form of `mark_degraded` for callers holding a user-supplied root
/// (the benchmark measures the degradation path this way).
pub fn degrade_snapshot(root: &str, reason: &str) -> Result<(), String> {
    let root = canonical_root(root)?;
    mark_degraded(&root, reason);
    Ok(())
}

/// Marks the snapshot as serving possibly stale bodies (ADR-009). Queries keep
/// using the cache but revalidate every cached note first.
pub(crate) fn mark_degraded(root: &Path, reason: &str) {
    let Some(snapshot) = active_snapshot(root) else {
        return;
    };
    let mut inner = snapshot.lock();
    inner.state = SnapshotState::Degraded;
    inner.degraded_reason = Some(reason.to_string());
}

/// Revalidates cached bodies with mtime+size before a degraded query. Mismatched
/// notes lose their body (so they are re-read from disk) and a background warm
/// pass refills them; matching notes keep the cache path.
fn validate_degraded(snapshot: &Arc<Snapshot>) {
    let (root, state) = {
        let inner = snapshot.lock();
        (inner.root.clone(), inner.state)
    };
    if state != SnapshotState::Degraded {
        return;
    }

    let candidates: Vec<(String, usize, u64)> = {
        let inner = snapshot.lock();
        inner
            .notes
            .iter()
            .filter(|note| note.text.is_some())
            .map(|note| (note.path.to_string(), note.size, note.modified_ms))
            .collect()
    };
    if candidates.is_empty() {
        return;
    }

    let stale = stale_bodies(&root, &candidates);
    if stale.is_empty() {
        return;
    }

    {
        let mut inner = snapshot.lock();
        if inner.state != SnapshotState::Degraded {
            return;
        }
        for path in stale {
            if let Some(&index) = inner.index.get(&path) {
                inner.drop_text(index);
                inner.notes[index].needs_warm = true;
            }
        }
    }

    spawn_warm(Arc::clone(snapshot));
}

/// Stats every cached body and returns the paths whose size or mtime changed.
/// Parallel because 10k serial `stat` calls cost ~690 ms on Windows (measured),
/// which would erase the cache's advantage in the very mode that needs it.
fn stale_bodies(root: &Path, candidates: &[(String, usize, u64)]) -> Vec<String> {
    fn check(root: &Path, slice: &[(String, usize, u64)]) -> Vec<String> {
        let mut stale = Vec::new();
        for (path, size, stamp) in slice {
            let fresh = match resolve_search_target_with_metadata(root, path) {
                Ok((_, metadata)) => {
                    metadata.len() == *size as u64
                        && (*stamp == 0 || modified_ms(&metadata).unwrap_or(0) == *stamp)
                }
                Err(_) => false,
            };
            if !fresh {
                stale.push(path.clone());
            }
        }
        stale
    }

    if candidates.len() < 2 {
        return check(root, candidates);
    }

    let workers = worker_count(MAX_SNAPSHOT_THREADS).min(candidates.len());
    let chunk_size = candidates.len().div_ceil(workers);
    let mut stale = Vec::new();

    thread::scope(|scope| {
        let handles: Vec<_> = candidates
            .chunks(chunk_size)
            .map(|chunk| scope.spawn(move || check(root, chunk)))
            .collect();

        for handle in handles {
            if let Ok(mut local) = handle.join() {
                stale.append(&mut local);
            }
        }
    });

    stale
}

#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: Mutex<()> = Mutex::new(());
    GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Matcher {
    kind: MatcherKind,
    needle_chars: usize,
}

enum MatcherKind {
    Ascii(AhoCorasick),
    Unicode(String),
}

impl Matcher {
    fn new(needle: &str) -> Self {
        if needle.is_ascii() {
            let ascii = AhoCorasick::builder()
                .ascii_case_insensitive(true)
                .build([needle])
                .expect("a single literal pattern always builds");
            Self {
                kind: MatcherKind::Ascii(ascii),
                needle_chars: needle.len(),
            }
        } else {
            let lower = needle.to_lowercase();
            let needle_chars = lower.chars().count();
            Self {
                kind: MatcherKind::Unicode(lower),
                needle_chars,
            }
        }
    }

    /// Returns the character index of the first match, or `None`.
    fn find(&self, haystack: &str) -> Option<usize> {
        match &self.kind {
            MatcherKind::Ascii(ascii) => {
                let found = ascii.find(haystack.as_bytes())?;
                Some(haystack[..found.start()].chars().count())
            }
            MatcherKind::Unicode(needle_lower) => {
                let folded = haystack.to_lowercase();
                let byte_index = folded.find(needle_lower)?;
                Some(folded[..byte_index].chars().count())
            }
        }
    }
}

/// Builds a context window around `char_index` without copying the whole line
/// into a `Vec<char>`. Only called for lines that actually matched.
fn build_preview(trimmed: &str, char_index: usize, needle_chars: usize) -> String {
    let total = trimmed.chars().count();
    let start = char_index.saturating_sub(PREVIEW_CHARS / 3).min(total);
    let end = (char_index + needle_chars + PREVIEW_CHARS)
        .max(start)
        .min(total);

    let start_byte = byte_at_char(trimmed, start);
    let end_byte = byte_at_char(trimmed, end);
    let mut preview = trimmed[start_byte..end_byte].to_string();

    if start > 0 {
        preview.insert(0, '…');
    }
    if end < total {
        preview.push('…');
    }

    preview
}

fn byte_at_char(input: &str, chars: usize) -> usize {
    input
        .char_indices()
        .nth(chars)
        .map(|(byte, _)| byte)
        .unwrap_or(input.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    fn write_file(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn entry(path: &str) -> NoteEntry {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let title = name
            .rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(&name)
            .to_string();
        NoteEntry {
            path: path.to_string(),
            name,
            title,
            modified_ms: None,
        }
    }

    fn test_snapshot(root: &Path, paths: &[&str], budget: usize) -> Arc<Snapshot> {
        let entries: Vec<NoteEntry> = paths.iter().map(|path| entry(path)).collect();
        Arc::new(Snapshot::new(root.to_path_buf(), &entries, budget))
    }

    fn cached_paths(inner: &SnapshotInner) -> Vec<String> {
        inner
            .notes
            .iter()
            .filter(|note| note.text.is_some())
            .map(|note| note.path.to_string())
            .collect()
    }

    #[test]
    fn search_notes_finds_content_with_line_and_preview() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(
            root,
            "a.md",
            "# Hello\n\nThe autosave loop is here.\nsecond AUTOSAVE",
        );
        write_file(root, "sub/b.md", "中文搜索关键字\n");
        write_file(root, "name-only.md", "no matching body");
        let root_string = root.to_string_lossy().into_owned();

        let hits = search_notes_blocking(&root_string, "AUTOSAVE").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "a.md");
        assert_eq!(hits[0].line, 3);
        assert!(hits[0].preview.contains("autosave"));
        assert_eq!(hits[1].line, 4);

        let chinese = search_notes_blocking(&root_string, "关键字").unwrap();
        assert_eq!(chinese.len(), 1);
        assert_eq!(chinese[0].path, "sub/b.md");
        assert_eq!(chinese[0].line, 1);

        let filename_only = search_notes_blocking(&root_string, "name-only").unwrap();
        assert!(filename_only.is_empty());

        let blank = search_notes_blocking(&root_string, "   ").unwrap();
        assert!(blank.is_empty());
    }

    #[test]
    fn search_is_ascii_case_insensitive_without_touching_case() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "note.md", "AutoSave and AUTOSAVE and autosave\n");
        let root_string = root.to_string_lossy().into_owned();

        let hits = search_notes_blocking(&root_string, "aUtOsAvE").unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].preview.contains("AutoSave"));
    }

    #[test]
    fn search_preview_truncates_long_lines_with_ellipsis() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let long_prefix = "x".repeat(200);
        let long_suffix = "y".repeat(200);
        write_file(
            root,
            "long.md",
            &format!("{long_prefix} needle {long_suffix}\n"),
        );
        let root_string = root.to_string_lossy().into_owned();

        let hits = search_notes_blocking(&root_string, "needle").unwrap();
        assert_eq!(hits.len(), 1);
        let preview = &hits[0].preview;
        assert!(
            preview.starts_with('…'),
            "preview should start with ellipsis: {preview}"
        );
        assert!(
            preview.ends_with('…'),
            "preview should end with ellipsis: {preview}"
        );
        assert!(preview.contains("needle"));
    }

    #[test]
    fn search_target_rejects_traversal_absolute_and_non_markdown() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "ok.md", "ok");

        let traversal = resolve_search_target(root, "../outside.md").unwrap_err();
        assert!(traversal.contains("escapes"), "{traversal}");

        let absolute =
            resolve_search_target(root, &root.join("ok.md").to_string_lossy()).unwrap_err();
        assert!(absolute.contains("absolute"), "{absolute}");

        let non_markdown = resolve_search_target(root, "ok.txt").unwrap_err();
        assert!(non_markdown.contains("Markdown"), "{non_markdown}");
    }

    #[test]
    fn parallel_scan_matches_serial_scan_hit_for_hit() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        // 60 notes x 5 matching lines is well past the global cap of 50, so the
        // comparison also proves truncation only ever drops the tail.
        for index in 0..60 {
            let mut body = format!("# Note {index}\n\n");
            for line in 0..5 {
                body.push_str(&format!("needle on line {line}\n"));
            }
            if index % 13 == 0 {
                body.push_str("the rare token lives here\n");
            }
            write_file(
                root,
                &format!("folder-{}/note-{index:04}.md", index % 4),
                &body,
            );
        }

        let root_string = root.to_string_lossy().into_owned();
        let root_path = canonical_root(&root_string).unwrap();
        let notes = list_note_entries_at(&root_path).unwrap();
        let items: Vec<ScanItem> = notes
            .iter()
            .map(|note| ScanItem {
                path: Arc::from(note.path.as_str()),
                title: Arc::from(note.title.as_str()),
                text: None,
            })
            .collect();

        for query in ["needle", "rare token"] {
            let matcher = Matcher::new(query);
            let serial = scan_items(&matcher, &root_path, &items, MAX_SEARCH_HITS).hits;
            let parallel = search_notes_blocking(&root_string, query).unwrap();

            assert_eq!(
                parallel, serial,
                "query {query:?} diverged from serial scan"
            );
        }

        let hits = search_notes_blocking(&root_string, "needle").unwrap();
        assert_eq!(hits.len(), MAX_SEARCH_HITS);

        let mut per_note: std::collections::BTreeMap<&str, usize> = Default::default();
        for hit in &hits {
            *per_note.entry(hit.path.as_str()).or_default() += 1;
        }
        assert!(
            per_note.values().all(|count| *count <= MAX_HITS_PER_NOTE),
            "per-note cap regressed: {per_note:?}"
        );

        let paths: Vec<&str> = hits.iter().map(|hit| hit.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(
            paths, sorted,
            "hit order should follow the path-sorted scan order"
        );
    }

    #[test]
    fn search_notes_handles_a_large_workspace_within_budget() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let body = "line one\nline two\nline three\nline four\n".repeat(8);

        for index in 0..1_000 {
            write_file(
                root,
                &format!("bulk/note-{index:04}.md"),
                &format!("# Note {index}\n\n{body}"),
            );
        }
        write_file(
            root,
            "bulk/target.md",
            "# Target\n\nthe needle lives here\n",
        );

        let started = Instant::now();
        let hits = search_notes_blocking(&root.to_string_lossy(), "needle").unwrap();
        let elapsed = started.elapsed();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "bulk/target.md");
        assert_eq!(hits[0].line, 3);
        assert!(
            elapsed < Duration::from_secs(10),
            "large workspace search took {elapsed:?}, budget is 10s"
        );
        println!("large workspace search: 1001 notes in {elapsed:?}");
    }

    #[test]
    fn whole_text_scan_matches_the_per_line_scan() {
        let bodies = [
            "needle at the start\nsecond line\n",
            "  needle after indent\nneedle twice\nneedle three\nneedle four\n",
            "crlf needle\r\nnext needle\r\n",
            "no match here\nneedle\n",
            "中文 needle 中文\nneedle 中文 needle\n",
            "trailing space needle   \n",
            "",
            "needle",
        ];
        let matcher = Matcher::new("needle");
        let ascii = match &matcher.kind {
            MatcherKind::Ascii(ascii) => ascii,
            _ => unreachable!(),
        };

        for body in bodies {
            let mut whole = Vec::new();
            collect_ascii_hits(
                ascii,
                matcher.needle_chars,
                "note.md",
                "note",
                body,
                &mut whole,
                50,
            );

            let mut per_line = Vec::new();
            collect_line_hits(&matcher, "note.md", "note", body, &mut per_line, 50);

            assert_eq!(whole, per_line, "body {:?} diverged", body);
        }
    }

    #[test]
    fn snapshot_evicts_least_recently_used_when_over_budget() {
        let temp = tempdir().unwrap();
        let snapshot = test_snapshot(temp.path(), &["a.md", "b.md", "c.md", "d.md"], 10);
        let mut inner = snapshot.lock();

        inner.cache_text("a.md", Arc::from("aaaa"), 1);
        inner.cache_text("b.md", Arc::from("bbbb"), 2);
        assert_eq!(inner.bytes, 8);

        inner.cache_text("c.md", Arc::from("cccc"), 3);
        assert_eq!(cached_paths(&inner), vec!["b.md", "c.md"]);
        assert_eq!(inner.bytes, 8);

        inner.cache_text("d.md", Arc::from("dddd"), 4);
        assert_eq!(cached_paths(&inner), vec!["c.md", "d.md"]);
        assert_eq!(inner.bytes, 8);

        // A note larger than the whole budget is never cached.
        inner.cache_text("a.md", Arc::from("x".repeat(11)), 5);
        assert_eq!(cached_paths(&inner), vec!["c.md", "d.md"]);
        assert!(inner.bytes <= inner.budget);
    }

    fn install_snapshot(snapshot: &Arc<Snapshot>) {
        let mut slot = snapshot_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(Arc::clone(snapshot));
        snapshot.lock().state = SnapshotState::Ready;
    }

    #[test]
    fn snapshot_budget_never_changes_the_result() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        let body = "needle on every line to make the body long enough\n".repeat(4);
        for index in 0..6 {
            write_file(
                root,
                &format!("note-{index}.md"),
                &format!("# {index}\n{body}"),
            );
        }

        let root_string = root.to_string_lossy().into_owned();
        let root_path = canonical_root(&root_string).unwrap();
        let entries = list_note_entries_at(&root_path).unwrap();
        let on_disk = search_disk_blocking(&Matcher::new("needle"), &root_path).unwrap();
        assert_eq!(on_disk.len(), 18, "6 notes x 3 hits per note");

        // 0 bytes and a budget that fits only part of the bodies both leave
        // some (or all) notes to be read from disk mid-scan.
        for budget in [0, 700] {
            let snapshot = Arc::new(Snapshot::new(root_path.clone(), &entries, budget));
            {
                let mut inner = snapshot.lock();
                for note in entries.iter() {
                    let text = fs::read_to_string(root_path.join(&note.path)).unwrap();
                    inner.warm_text(&note.path, Arc::from(text), 1);
                }
            }
            install_snapshot(&snapshot);

            let cached = snapshot.status().cached;
            assert!(
                cached < entries.len(),
                "budget {budget} should not fit everything"
            );
            if budget > 0 {
                assert!(cached > 0, "partial budget should still cache something");
            }

            let warm = search_notes_blocking(&root_string, "needle").unwrap();
            assert_eq!(warm, on_disk, "budget {budget} changed the result set");
        }
    }

    #[test]
    fn snapshot_scan_matches_disk_scan_with_a_full_cache() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        for index in 0..20 {
            write_file(
                root,
                &format!("folder-{}/note-{index:02}.md", index % 3),
                &format!("# Note {index}\n\nneedle here\nno match\n"),
            );
        }
        let root_string = root.to_string_lossy().into_owned();
        let root_path = canonical_root(&root_string).unwrap();
        let entries = list_note_entries_at(&root_path).unwrap();

        let snapshot = Snapshot::new(root_path.clone(), &entries, usize::MAX);
        for note in entries.iter() {
            let text = fs::read_to_string(root_path.join(&note.path)).unwrap();
            snapshot.lock().warm_text(&note.path, Arc::from(text), 1);
        }
        assert_eq!(snapshot.status().cached, entries.len());

        let snapshot = Arc::new(snapshot);
        install_snapshot(&snapshot);
        assert_eq!(snapshot.scan_items().unwrap().len(), entries.len());

        let warm = search_notes_blocking(&root_string, "needle").unwrap();
        let cold = search_disk_blocking(&Matcher::new("needle"), &root_path).unwrap();
        assert_eq!(warm, cold);
    }

    #[test]
    fn warm_snapshot_reports_ready_with_cached_counts() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "a.md", "alpha needle\n");
        write_file(root, "sub/b.md", "beta needle\n");
        write_file(root, "skip.txt", "not markdown needle\n");

        let status = warm_snapshot_blocking(&root.to_string_lossy()).unwrap();
        assert_eq!(status.state, SnapshotState::Ready);
        assert_eq!(status.total, 2);
        assert_eq!(status.cached, 2);
        assert!(status.bytes > 0);

        let hits = search_notes_blocking(&root.to_string_lossy(), "needle").unwrap();
        assert_eq!(hits.len(), 2);

        let status = snapshot_status().unwrap();
        assert_eq!(status.total, 2);
    }

    #[test]
    fn snapshot_updates_follow_rename_and_delete() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "old.md", "needle body\n");
        write_file(root, "gone.md", "needle body\n");

        let root_string = root.to_string_lossy().into_owned();
        let root_path = canonical_root(&root_string).unwrap();
        let entries = list_note_entries_at(&root_path).unwrap();
        let paths: Vec<&str> = entries.iter().map(|note| note.path.as_str()).collect();
        let snapshot = test_snapshot(&root_path, &paths, 1024);
        {
            let mut inner = snapshot.lock();
            for note in entries.iter() {
                let text = fs::read_to_string(root_path.join(&note.path)).unwrap();
                inner.warm_text(&note.path, Arc::from(text), 1);
            }
        }
        install_snapshot(&snapshot);

        cache_rename(&root_path, "old.md", "renamed.md");
        cache_remove(&root_path, "gone.md");

        let status = snapshot.status();
        assert_eq!(status.total, 1);
        assert_eq!(status.cached, 1);

        let hits = search_notes_blocking(&root_string, "needle").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "renamed.md");
    }

    #[test]
    fn snapshot_keys_normalize_caller_paths() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "sub/note.md", "body\n");

        let root_string = root.to_string_lossy().into_owned();
        let root_path = canonical_root(&root_string).unwrap();
        warm_snapshot_blocking(&root_string).unwrap();

        cache_upsert(&root_path, "./sub/note.md", "fresh");

        let snapshot = active_snapshot(&root_path).unwrap();
        assert_eq!(snapshot.status().total, 1, "must not insert a duplicate");
        let hits = search_notes_blocking(&root_string, "fresh").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "sub/note.md");
    }

    #[test]
    fn refresh_reconciles_added_and_removed_notes() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "a.md", "alpha needle\n");

        let root_string = root.to_string_lossy().into_owned();
        let root_path = canonical_root(&root_string).unwrap();

        let entries = list_note_entries_at(&root_path).unwrap();
        let snapshot = prepare(&root_path, &entries).unwrap();
        warm_snapshot(&snapshot);
        assert_eq!(snapshot.status().total, 1);
        assert_eq!(snapshot.status().cached, 1);

        // A note created outside `list_notes` (e.g. another tool) arrives with
        // the next refresh and is queued for warm-up.
        write_file(root, "b.md", "beta needle\n");
        let entries = list_note_entries_at(&root_path).unwrap();
        let snapshot = prepare(&root_path, &entries).unwrap();
        assert!(Arc::ptr_eq(
            &snapshot,
            &active_snapshot(&root_path).unwrap()
        ));
        warm_snapshot(&snapshot);

        let status = snapshot.status();
        assert_eq!(status.total, 2);
        assert_eq!(status.cached, 2);

        // Removal is picked up by the same reconciliation.
        fs::remove_file(root.join("a.md")).unwrap();
        let entries = list_note_entries_at(&root_path).unwrap();
        assert!(
            prepare(&root_path, &entries).is_none(),
            "nothing left to warm"
        );
        assert_eq!(snapshot.status().total, 1);

        let hits = search_notes_blocking(&root_string, "needle").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "b.md");
    }

    #[test]
    fn in_app_writes_are_visible_to_the_snapshot() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "note.md", "old body\n");

        let root_string = root.to_string_lossy().into_owned();
        let status = warm_snapshot_blocking(&root_string).unwrap();
        assert_eq!(status.cached, 1);
        assert!(
            search_notes_blocking(&root_string, "fresh")
                .unwrap()
                .is_empty()
        );

        let document = crate::notes::read_note(root_string.clone(), "note.md".to_string()).unwrap();
        crate::notes::write_note(
            root_string.clone(),
            "note.md".to_string(),
            "fresh token\n".to_string(),
            document.revision,
        )
        .unwrap();

        let hits = search_notes_blocking(&root_string, "fresh").unwrap();
        assert_eq!(hits.len(), 1, "snapshot should mirror the write");
        assert_eq!(hits[0].path, "note.md");
    }

    /// Request ids are global and monotonic, so tests that run in any order
    /// must never reuse one: a stale id is cancelled by design.
    fn next_request_id() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1_000);
        NEXT.fetch_add(1, Ordering::SeqCst)
    }

    /// Six notes with five matching lines each: 18 hits, because the per-note
    /// cap stops at three. That is more than one batch but well under the
    /// global cap of 50.
    fn streaming_workspace(root: &Path) {
        for index in 0..6 {
            let mut body = format!("# Note {index}\n\n");
            for line in 0..5 {
                body.push_str(&format!("needle on line {line}\n"));
            }
            write_file(root, &format!("note-{index}.md"), &body);
        }
    }

    fn batch_sizes(events: &[SearchEvent]) -> Vec<usize> {
        events
            .iter()
            .filter_map(|event| match event {
                SearchEvent::Batch { hits } => Some(hits.len()),
                SearchEvent::Done { .. } => None,
            })
            .collect()
    }

    fn streamed_hits(events: &[SearchEvent]) -> Vec<SearchHit> {
        events
            .iter()
            .filter_map(|event| match event {
                SearchEvent::Batch { hits } => Some(hits.clone()),
                SearchEvent::Done { .. } => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn stream_sends_capped_batches_then_one_done() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        streaming_workspace(root);
        let root_string = root.to_string_lossy().into_owned();

        let mut events: Vec<SearchEvent> = Vec::new();
        let mut emit = |event: SearchEvent| {
            events.push(event);
            true
        };
        search_stream_blocking(&root_string, "needle", next_request_id(), &mut emit).unwrap();

        let sizes = batch_sizes(&events);
        assert!(
            sizes.iter().all(|size| *size <= SEARCH_BATCH_SIZE),
            "batch exceeded the cap: {sizes:?}"
        );
        assert_eq!(sizes.iter().sum::<usize>(), 18);
        assert_eq!(
            streamed_hits(&events),
            search_notes_blocking(&root_string, "needle").unwrap()
        );

        match events.last() {
            Some(SearchEvent::Done {
                total,
                truncated,
                source,
                ..
            }) => {
                assert_eq!(*total, 18);
                assert!(!*truncated, "18 hits are below the global cap");
                assert_eq!(*source, SearchSource::Disk, "no snapshot was warmed");
            }
            other => panic!("last event should be Done, got {other:?}"),
        }
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, SearchEvent::Done { .. }))
                .count(),
            1,
            "Done must be sent exactly once"
        );
    }

    #[test]
    fn stream_reports_truncation_when_the_global_cap_is_hit() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        for index in 0..30 {
            let mut body = format!("# Note {index}\n\n");
            for line in 0..5 {
                body.push_str(&format!("needle on line {line}\n"));
            }
            write_file(root, &format!("note-{index:02}.md"), &body);
        }
        let root_string = root.to_string_lossy().into_owned();

        let mut events: Vec<SearchEvent> = Vec::new();
        let mut emit = |event: SearchEvent| {
            events.push(event);
            true
        };
        search_stream_blocking(&root_string, "needle", next_request_id(), &mut emit).unwrap();

        assert_eq!(streamed_hits(&events).len(), MAX_SEARCH_HITS);
        match events.last() {
            Some(SearchEvent::Done {
                total, truncated, ..
            }) => {
                assert_eq!(*total, MAX_SEARCH_HITS as u32);
                assert!(*truncated, "hitting the cap must set truncated");
            }
            other => panic!("last event should be Done, got {other:?}"),
        }
    }

    #[test]
    fn stream_stops_without_done_when_cancelled_mid_scan() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        streaming_workspace(root);
        let root_string = root.to_string_lossy().into_owned();
        let request_id = next_request_id();

        let mut events: Vec<SearchEvent> = Vec::new();
        let mut emit = |event: SearchEvent| {
            if matches!(event, SearchEvent::Batch { .. }) {
                // Simulates the user clearing the query after the first batch.
                cancel_request(request_id);
            }
            events.push(event);
            true
        };
        search_stream_blocking(&root_string, "needle", request_id, &mut emit).unwrap();

        assert_eq!(
            events.len(),
            1,
            "cancellation must stop the scan: {events:?}"
        );
        assert!(
            matches!(events[0], SearchEvent::Batch { .. }),
            "no Done may follow a cancelled request"
        );
    }

    #[test]
    fn stream_reports_snapshot_source_when_the_cache_is_ready() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        streaming_workspace(root);
        let root_string = root.to_string_lossy().into_owned();
        let status = warm_snapshot_blocking(&root_string).unwrap();
        assert_eq!(status.state, SnapshotState::Ready);
        assert_eq!(status.cached, 6);

        let mut events: Vec<SearchEvent> = Vec::new();
        let mut emit = |event: SearchEvent| {
            events.push(event);
            true
        };
        search_stream_blocking(&root_string, "needle", next_request_id(), &mut emit).unwrap();

        match events.last() {
            Some(SearchEvent::Done { source, total, .. }) => {
                assert_eq!(*total, 18);
                assert_eq!(*source, SearchSource::Snapshot);
            }
            other => panic!("last event should be Done, got {other:?}"),
        }
    }

    #[test]
    fn search_events_match_the_frontend_contract() {
        let batch = serde_json::to_value(SearchEvent::Batch {
            hits: vec![SearchHit {
                path: "a.md".to_string(),
                title: "a".to_string(),
                line: 3,
                preview: "needle".to_string(),
            }],
        })
        .unwrap();
        assert_eq!(batch["kind"], "batch");
        assert_eq!(batch["hits"][0]["line"], 3);

        let done = serde_json::to_value(SearchEvent::Done {
            total: 2,
            truncated: false,
            took_ms: 7,
            source: SearchSource::Snapshot,
        })
        .unwrap();
        assert_eq!(done["kind"], "done");
        assert_eq!(done["tookMs"], 7);
        assert_eq!(done["source"], "snapshot");
        assert_eq!(done["truncated"], false);
    }

    #[test]
    fn stream_delivers_an_empty_done_for_a_blank_query() {
        let _guard = test_guard();
        let temp = tempdir().unwrap();
        let root_string = temp.path().to_string_lossy().into_owned();

        let mut events: Vec<SearchEvent> = Vec::new();
        let mut emit = |event: SearchEvent| {
            events.push(event);
            true
        };
        search_stream_blocking(&root_string, "   ", next_request_id(), &mut emit).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            SearchEvent::Done {
                total, truncated, ..
            } => {
                assert_eq!(*total, 0);
                assert!(!*truncated);
            }
            other => panic!("blank query should emit only Done, got {other:?}"),
        }
    }
}
