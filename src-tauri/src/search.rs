use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use aho_corasick::AhoCorasick;
use serde::Serialize;

use crate::notes::{
    NoteEntry, canonical_root, ensure_within, list_notes, relative_note_path, worker_count,
};

pub const MAX_SEARCH_HITS: usize = 50;
pub const MAX_HITS_PER_NOTE: usize = 3;
pub const PREVIEW_CHARS: usize = 90;
/// Upper bound on scan workers. Past this point the scan is bound by disk and
/// antivirus inspection rather than by cores, so extra threads stop paying off.
pub const MAX_SEARCH_THREADS: usize = 8;

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

/// Scans every Markdown note under `root` and returns up to
/// `MAX_SEARCH_HITS` line hits. RFC-006 L0: the ASCII path matches through a
/// case-insensitive automaton, so unmatched lines are never lowercased.
/// RFC-006 L1: the path-sorted note list is split into contiguous ranges that
/// are scanned in parallel.
pub fn search_notes_blocking(root: &str, query: &str) -> Result<Vec<SearchHit>, String> {
    let needle = query.trim();
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let matcher = Matcher::new(needle);
    let root_path = canonical_root(root)?;
    let notes = list_notes(root.to_string())?;

    if notes.len() < 2 {
        return Ok(scan_notes(&matcher, &root_path, &notes, MAX_SEARCH_HITS));
    }

    let workers = search_workers().min(notes.len());
    let chunk_size = notes.len().div_ceil(workers);
    let mut chunk_hits: Vec<Vec<SearchHit>> = Vec::with_capacity(workers);
    let mut panicked = false;

    // Chunks are contiguous slices of the same path-sorted list and joins run in
    // chunk order, so concatenating them reproduces the serial hit order and the
    // global cap only ever drops the tail. Each chunk keeps at most the global
    // cap of its own hits, which is all a truncated prefix can consume.
    thread::scope(|scope| {
        let matcher = &matcher;
        let root_path = &root_path;
        let handles: Vec<_> = notes
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || scan_notes(matcher, root_path, chunk, MAX_SEARCH_HITS))
            })
            .collect();

        for handle in handles {
            match handle.join() {
                Ok(hits) => chunk_hits.push(hits),
                Err(_) => panicked = true,
            }
        }
    });

    if panicked {
        return Err("search worker thread panicked".to_string());
    }

    let mut hits = Vec::with_capacity(MAX_SEARCH_HITS);
    for chunk in chunk_hits {
        let remaining = MAX_SEARCH_HITS.saturating_sub(hits.len());
        if remaining == 0 {
            break;
        }
        hits.extend(chunk.into_iter().take(remaining));
    }

    Ok(hits)
}

/// One contiguous range of the path-sorted note list. Stops at `limit` hits or
/// when the range is exhausted, exactly like a serial scan would.
fn scan_notes(matcher: &Matcher, root: &Path, notes: &[NoteEntry], limit: usize) -> Vec<SearchHit> {
    let mut hits = Vec::new();

    for note in notes {
        if hits.len() >= limit {
            break;
        }

        let target = match resolve_search_target(root, &note.path) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let text = match fs::read_to_string(&target) {
            Ok(text) => text,
            Err(_) => continue,
        };

        let mut note_hits = 0;
        for (index, line) in text.lines().enumerate() {
            if note_hits >= MAX_HITS_PER_NOTE || hits.len() >= limit {
                break;
            }
            let trimmed = line.trim();
            if let Some(char_index) = matcher.find(trimmed) {
                hits.push(SearchHit {
                    path: note.path.clone(),
                    title: note.title.clone(),
                    line: (index + 1) as u32,
                    preview: build_preview(trimmed, char_index, matcher.needle_chars),
                });
                note_hits += 1;
            }
        }
    }

    hits
}

fn search_workers() -> usize {
    worker_count(MAX_SEARCH_THREADS)
}

/// Paths produced by `walk` are already relative, normalized, and symlink-free.
/// Re-running `canonicalize` for every note only repeats syscalls, so the search
/// path validates lexically and rejects symlinks with a single `lstat`.
fn resolve_search_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
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

    Ok(candidate)
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
    let start = char_index
        .saturating_sub(PREVIEW_CHARS / 3)
        .min(total);
    let end = (char_index + needle_chars + PREVIEW_CHARS).max(start).min(total);

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
        assert!(preview.starts_with('…'), "preview should start with ellipsis: {preview}");
        assert!(preview.ends_with('…'), "preview should end with ellipsis: {preview}");
        assert!(preview.contains("needle"));
    }

    #[test]
    fn search_target_rejects_traversal_absolute_and_non_markdown() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "ok.md", "ok");

        let traversal = resolve_search_target(root, "../outside.md").unwrap_err();
        assert!(traversal.contains("escapes"), "{traversal}");

        let absolute = resolve_search_target(root, &root.join("ok.md").to_string_lossy()).unwrap_err();
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
        let notes = list_notes(root_string.clone()).unwrap();

        for query in ["needle", "rare token"] {
            let matcher = Matcher::new(query);
            let serial = scan_notes(&matcher, &root_path, &notes, MAX_SEARCH_HITS);
            let parallel = search_notes_blocking(&root_string, query).unwrap();

            assert_eq!(parallel, serial, "query {query:?} diverged from serial scan");
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
        assert_eq!(paths, sorted, "hit order should follow the path-sorted scan order");
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
        write_file(root, "bulk/target.md", "# Target\n\nthe needle lives here\n");

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
}
