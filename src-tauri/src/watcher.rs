use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::notes::{WorkspaceDelta, canonical_root, is_ignored_segment, is_markdown};
use crate::search::{self, ReloadOutcome};

/// Quiet period before a batch of filesystem events is applied. Long enough to
/// collapse an editor's write/rename/attribute burst, short enough that an
/// external edit is visible well inside the 500 ms budget (RFC-006 §4.6).
pub const DEBOUNCE: Duration = Duration::from_millis(300);

/// Consecutive note reads that do not match disk before the snapshot is marked
/// `Degraded` and queries start revalidating by mtime+size (ADR-009).
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Callback the watcher uses to push note-list changes to the UI.
pub type WorkspaceEmitter = Arc<dyn Fn(WorkspaceDelta) + Send + Sync>;

/// Event name the frontend subscribes to (see `noteRepository.ts`).
pub const WORKSPACE_EVENT: &str = "workspace:changed";

struct WatchHandle {
    root: PathBuf,
    /// Shared with the debounce thread so a later `watch_workspace` call for the
    /// same root can attach the UI emitter to the running thread.
    emitter: Arc<Mutex<Option<WorkspaceEmitter>>>,
    /// Dropping the notify watcher stops its backend thread, which drops the
    /// event sender and ends the debounce thread.
    _watcher: RecommendedWatcher,
}

static WATCHER: OnceLock<Mutex<Option<WatchHandle>>> = OnceLock::new();
/// Installed by `run()` once Tauri is up; keeps this module free of Tauri types
/// so the library can still be tested without a WebView2 runtime.
static EMITTER: OnceLock<WorkspaceEmitter> = OnceLock::new();

pub fn set_emitter(emitter: WorkspaceEmitter) {
    let _ = EMITTER.set(emitter);
}

/// The emitter installed by the app, if any.
pub fn global_emitter() -> Option<WorkspaceEmitter> {
    EMITTER.get().cloned()
}

fn watcher_slot() -> &'static Mutex<Option<WatchHandle>> {
    WATCHER.get_or_init(|| Mutex::new(None))
}

/// Starts the recursive watcher for `root`, reusing it when the workspace has
/// not changed. Watching is best-effort: the caller turns an error into a
/// `Degraded` snapshot instead of failing the workspace open (ADR-009).
pub fn watch_workspace(root: &Path, emitter: Option<WorkspaceEmitter>) -> Result<(), String> {
    // The snapshot is keyed by the canonical root, so events must be resolved
    // against the same path.
    let root = canonical_root(&root.to_string_lossy())?;
    {
        let mut slot = watcher_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(handle) = slot.as_mut().filter(|handle| handle.root == root) {
            *handle
                .emitter
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = emitter;
            return Ok(());
        }
    }

    stop_watching();

    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let _ = sender.send(event);
    })
    .map_err(|error| format!("cannot start file watcher: {error}"))?;

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|error| format!("cannot watch workspace: {error}"))?;

    let watch_root = root.clone();
    let emitter_slot = Arc::new(Mutex::new(emitter));
    let thread_slot = Arc::clone(&emitter_slot);
    std::thread::Builder::new()
        .name("search-watch".to_string())
        .spawn(move || debounce_loop(watch_root, receiver, thread_slot))
        .map_err(|error| format!("cannot start file watcher thread: {error}"))?;

    let mut slot = watcher_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = Some(WatchHandle {
        root,
        emitter: emitter_slot,
        _watcher: watcher,
    });
    Ok(())
}

/// Drops the watcher, if any. Called when the workspace changes and by tests.
pub fn stop_watching() {
    let mut slot = watcher_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = None;
}

/// True when the watcher is bound to `root`.
pub fn is_watching(root: &Path) -> bool {
    let Ok(root) = canonical_root(&root.to_string_lossy()) else {
        return false;
    };
    let slot = watcher_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.as_ref().is_some_and(|handle| handle.root == root)
}

fn is_relevant(event: &Event) -> bool {
    // Access and metadata-only events cannot change a body; everything else may.
    matches!(
        event.kind,
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

/// Coalesces events until the workspace has been quiet for `DEBOUNCE`, then
/// applies them. Exits when the watcher (and therefore the sender) is dropped.
fn debounce_loop(
    root: PathBuf,
    receiver: Receiver<notify::Result<Event>>,
    emitter: Arc<Mutex<Option<WorkspaceEmitter>>>,
) {
    let mut pending: BTreeSet<PathBuf> = BTreeSet::new();
    let mut deadline: Option<Instant> = None;

    loop {
        let wait = deadline
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or(DEBOUNCE);

        match receiver.recv_timeout(wait) {
            Ok(Ok(event)) => {
                if !is_relevant(&event) {
                    continue;
                }
                pending.extend(event.paths);
                deadline = Some(Instant::now() + DEBOUNCE);
            }
            Ok(Err(error)) => {
                // A backend overflow or watch error means events were lost, so
                // the snapshot can no longer be trusted without validation.
                search::mark_degraded(&root, &format!("file watching failed: {error}"));
            }
            Err(RecvTimeoutError::Timeout) => {
                if deadline.is_none() {
                    continue;
                }
                let delta = flush(&root, std::mem::take(&mut pending));
                deadline = None;

                if !delta.is_empty() {
                    let emitter = emitter
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone();
                    if let Some(emitter) = emitter {
                        emitter(delta);
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Reduces every event to "what is on disk now": a path that still exists is
/// re-read into the snapshot, a path that is gone is dropped. That single rule
/// covers create, modify, remove, in-place saves and atomic temp-file renames.
fn flush(root: &Path, paths: BTreeSet<PathBuf>) -> WorkspaceDelta {
    let mut delta = WorkspaceDelta::default();
    let mut failures = 0u32;

    for path in paths {
        let Some(relative) = relative_to(root, &path) else {
            continue;
        };
        if relative.split('/').any(is_ignored_segment) {
            continue;
        }

        if is_markdown(&path) {
            match search::cache_reload(root, &relative) {
                ReloadOutcome::Inserted(entry) => {
                    delta.added.push(entry);
                    failures = 0;
                }
                ReloadOutcome::Removed => {
                    delta.removed.push(relative);
                    failures = 0;
                }
                ReloadOutcome::Failed => failures += 1,
                ReloadOutcome::Updated | ReloadOutcome::Unchanged => failures = 0,
            }
            continue;
        }

        // A removed folder takes its notes with it; a created one needs nothing
        // because its files produce their own events.
        if !path.exists() {
            delta
                .removed
                .extend(search::cache_remove_subtree(root, &relative));
        }
    }

    if failures >= MAX_CONSECUTIVE_FAILURES {
        search::mark_degraded(
            root,
            "workspace notes could not be read after external changes",
        );
    }

    delta
}

fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let stripped = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = stripped
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SnapshotState;
    use std::fs;
    use tempfile::tempdir;

    /// Stops the watcher even when an assertion fails, so one failing test
    /// cannot leave a backend thread pointing at a deleted temp directory.
    struct WatchGuard;

    impl Drop for WatchGuard {
        fn drop(&mut self) {
            stop_watching();
        }
    }

    fn write_file(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn search(root: &Path, query: &str) -> Vec<String> {
        search::search_notes_blocking(&root.to_string_lossy(), query)
            .unwrap()
            .into_iter()
            .map(|hit| hit.path)
            .collect()
    }

    fn wait_until(timeout: Duration, mut check: impl FnMut() -> bool) -> Duration {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if check() {
                return started.elapsed();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("condition not met within {timeout:?}");
    }

    /// The snapshot and the watcher are both keyed by the canonical root.
    fn canonical(root: &Path) -> PathBuf {
        canonical_root(&root.to_string_lossy()).unwrap()
    }

    fn open(root: &Path) -> WatchGuard {
        search::warm_snapshot_blocking(&root.to_string_lossy()).unwrap();
        watch_workspace(root, None).unwrap();
        WatchGuard
    }

    #[test]
    fn external_write_becomes_visible_after_debounce() {
        let _guard = search::test_guard();
        let temp = tempdir().unwrap();
        let root_buf = canonical(temp.path());
        let root: &Path = &root_buf;
        write_file(root, "note.md", "alpha\n");
        let _watch = open(root);

        assert!(search(root, "beta").is_empty());
        fs::write(root.join("note.md"), "beta now lives here\n").unwrap();

        let elapsed = wait_until(Duration::from_secs(3), || search(root, "beta").len() == 1);
        assert!(
            elapsed < Duration::from_millis(1_500),
            "external edit took {elapsed:?}; 500 ms on an idle machine, this bound only catches a missing flush"
        );
        assert!(search(root, "alpha").is_empty(), "stale body still served");
    }

    #[test]
    fn external_delete_removes_the_note_from_the_snapshot() {
        let _guard = search::test_guard();
        let temp = tempdir().unwrap();
        let root_buf = canonical(temp.path());
        let root: &Path = &root_buf;
        write_file(root, "keep.md", "shared needle\n");
        write_file(root, "sub/doomed.md", "shared needle\n");
        let _watch = open(root);

        assert_eq!(search(root, "needle").len(), 2);
        fs::remove_file(root.join("sub/doomed.md")).unwrap();

        wait_until(Duration::from_secs(3), || {
            let status = search::snapshot_status().unwrap();
            status.total == 1 && search(root, "needle") == vec!["keep.md".to_string()]
        });
    }

    #[test]
    fn external_folder_delete_drops_every_note_under_it() {
        let _guard = search::test_guard();
        let temp = tempdir().unwrap();
        let root_buf = canonical(temp.path());
        let root: &Path = &root_buf;
        write_file(root, "keep.md", "shared needle\n");
        write_file(root, "folder/a.md", "shared needle\n");
        write_file(root, "folder/nested/b.md", "shared needle\n");
        let _watch = open(root);

        assert_eq!(search(root, "needle").len(), 3);
        fs::remove_dir_all(root.join("folder")).unwrap();

        wait_until(Duration::from_secs(3), || {
            search::snapshot_status().unwrap().total == 1 && search(root, "needle").len() == 1
        });
    }

    #[test]
    fn atomic_save_through_a_temp_file_is_seen() {
        let _guard = search::test_guard();
        let temp = tempdir().unwrap();
        let root_buf = canonical(temp.path());
        let root: &Path = &root_buf;
        write_file(root, "note.md", "before the swap\n");
        let _watch = open(root);

        // Editors on Windows write a sibling temp file and rename it over the
        // target; the intermediary name is not Markdown and must be ignored.
        write_file(root, ".note.md.tmp", "gamma survived the rename\n");
        fs::rename(root.join(".note.md.tmp"), root.join("note.md")).unwrap();

        let elapsed = wait_until(Duration::from_secs(3), || search(root, "gamma").len() == 1);
        assert!(elapsed < Duration::from_millis(1_500), "{elapsed:?}");
        assert!(search(root, "before").is_empty());
    }

    #[test]
    fn ignored_folders_never_enter_the_snapshot() {
        let _guard = search::test_guard();
        let temp = tempdir().unwrap();
        let root_buf = canonical(temp.path());
        let root: &Path = &root_buf;
        write_file(root, "note.md", "plain body\n");
        let _watch = open(root);

        write_file(
            root,
            "node_modules/pkg/hidden.md",
            "needle in a build folder\n",
        );
        write_file(root, ".git/hidden.md", "needle in a hidden folder\n");

        // Give the debounce window time to pass; nothing may be added.
        std::thread::sleep(DEBOUNCE + Duration::from_millis(150));
        assert!(search(root, "needle").is_empty());
        assert_eq!(search::snapshot_status().unwrap().total, 1);
    }

    #[test]
    fn degraded_snapshot_revalidates_before_returning_hits() {
        let _guard = search::test_guard();
        stop_watching();

        let temp = tempdir().unwrap();
        let root_buf = canonical(temp.path());
        let root: &Path = &root_buf;
        write_file(root, "note.md", "before\n");
        search::warm_snapshot_blocking(&root.to_string_lossy()).unwrap();

        search::mark_degraded(root, "watcher disabled");
        assert_eq!(
            search::snapshot_status().unwrap().state,
            SnapshotState::Degraded
        );

        // No watcher: only the mtime+size check can notice this edit.
        fs::write(root.join("note.md"), "after the marker\n").unwrap();

        let hits = search::search_notes_blocking(&root.to_string_lossy(), "marker").unwrap();
        assert_eq!(hits.len(), 1);
        assert!(search(root, "before").is_empty());

        // The stale body was dropped, so the cache reloads it in the background.
        wait_until(Duration::from_secs(3), || {
            let status = search::snapshot_status().unwrap();
            status.cached == 1 && status.bytes == "after the marker\n".len()
        });
    }

    #[test]
    fn watching_is_reused_and_rebound_across_workspaces() {
        let _guard = search::test_guard();
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        write_file(first.path(), "a.md", "needle one\n");
        write_file(second.path(), "b.md", "needle two\n");

        let _watch = open(first.path());
        assert!(is_watching(first.path()));

        search::warm_snapshot_blocking(&second.path().to_string_lossy()).unwrap();
        watch_workspace(second.path(), None).unwrap();
        assert!(is_watching(second.path()));
        assert!(!is_watching(first.path()));

        fs::write(second.path().join("b.md"), "rebound needle\n").unwrap();
        wait_until(Duration::from_secs(3), || {
            search(second.path(), "rebound").len() == 1
        });
    }

    #[test]
    fn tree_delta_reports_creates_and_renames_but_not_edits() {
        let _guard = search::test_guard();
        let temp = tempdir().unwrap();
        let root_buf = canonical(temp.path());
        let root: &Path = &root_buf;
        write_file(root, "keep.md", "needle\n");
        search::warm_snapshot_blocking(&root.to_string_lossy()).unwrap();

        let (sender, receiver) = mpsc::channel();
        let emitter: WorkspaceEmitter = Arc::new(move |delta: WorkspaceDelta| {
            let _ = sender.send(delta);
        });
        watch_workspace(root, Some(emitter)).unwrap();
        let _watch = WatchGuard;

        // Editing a listed note changes its body, not the tree.
        fs::write(root.join("keep.md"), "needle again\n").unwrap();
        let edit = receiver.recv_timeout(DEBOUNCE + Duration::from_millis(300));
        assert!(
            edit.is_err(),
            "an in-place edit should not emit a tree delta: {edit:?}"
        );

        write_file(root, "fresh.md", "new body\n");
        let delta = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(delta.added.len(), 1, "{delta:?}");
        assert_eq!(delta.added[0].path, "fresh.md");
        assert_eq!(delta.added[0].name, "fresh.md");
        assert_eq!(delta.added[0].title, "fresh");
        assert!(delta.added[0].modified_ms.is_some());
        assert!(delta.removed.is_empty());

        // A rename arrives as a removal of the old path plus an addition.
        fs::rename(root.join("fresh.md"), root.join("renamed.md")).unwrap();
        let mut added = Vec::new();
        let mut removed = Vec::new();
        wait_until(Duration::from_secs(3), || {
            while let Ok(delta) = receiver.try_recv() {
                added.extend(delta.added.into_iter().map(|entry| entry.path));
                removed.extend(delta.removed);
            }
            added.iter().any(|path| path == "renamed.md")
                && removed.iter().any(|path| path == "fresh.md")
        });
        assert!(!added.iter().any(|path| path == "keep.md"));
    }
}
