use serde::Serialize;
use std::fs;

use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

pub(crate) const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_NOTES: usize = 50_000;
const MAX_DEPTH: usize = 12;
const MAX_TITLE_CHARS: usize = 80;

const MAX_WORKERS: usize = 8;

const IGNORED_DIRS: &[&str] = &["node_modules", "target", "dist", "build"];
const MARKDOWN_EXTS: &[&str] = &["md", "markdown", "mdown"];

type AppResult<T> = Result<T, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEntry {
    /// Path relative to the workspace root, always using forward slashes.
    pub path: String,
    pub name: String,
    pub title: String,
    pub modified_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDocument {
    pub content: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedNote {
    /// Relative path the note can be restored to.
    pub path: String,
    /// In-memory text snapshot for the undo entry. `None` when the file is
    /// over the size limit or is not valid UTF-8.
    pub content: Option<String>,
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| MARKDOWN_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// FNV-1a over the raw bytes. This is a change-detection token, not a
/// cryptographic digest or a security boundary.
fn revision_for_bytes(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub(crate) fn canonical_root(root: &str) -> AppResult<PathBuf> {
    if root.trim().is_empty() {
        return Err("workspace path is empty".to_string());
    }

    let canonical = fs::canonicalize(root)
        .map_err(|error| format!("cannot open workspace: {error}"))?;

    if !canonical.is_dir() {
        return Err("workspace path is not a directory".to_string());
    }

    Ok(canonical)
}

pub(crate) fn relative_note_path(relative: &str) -> AppResult<PathBuf> {
    if relative.trim().is_empty() {
        return Err("note path is empty".to_string());
    }

    let candidate = Path::new(relative);
    if candidate.is_absolute() {
        return Err("absolute note paths are not allowed".to_string());
    }

    let mut clean = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return Err("note path escapes the workspace".to_string()),
        }
    }

    if clean.as_os_str().is_empty() {
        return Err("note path is empty".to_string());
    }
    if !is_markdown(&clean) {
        return Err("only Markdown files can be opened".to_string());
    }

    Ok(clean)
}

pub(crate) fn ensure_within(root: &Path, path: &Path) -> AppResult<()> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err("note path escapes the workspace".to_string())
    }
}

pub(crate) fn resolve_existing(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let relative = relative_note_path(relative)?;
    let candidate = root.join(relative);
    let canonical = fs::canonicalize(&candidate)
        .map_err(|error| format!("cannot open note: {error}"))?;

    ensure_within(root, &canonical)?;

    if !canonical.is_file() {
        return Err("note path is not a file".to_string());
    }

    Ok(canonical)
}

fn resolve_target(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let relative = relative_note_path(relative)?;
    let candidate = root.join(relative);

    if candidate.exists() {
        let canonical = fs::canonicalize(&candidate)
            .map_err(|error| format!("cannot open note: {error}"))?;
        ensure_within(root, &canonical)?;
        if !canonical.is_file() {
            return Err("note path is not a file".to_string());
        }
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "invalid note path".to_string())?;
        let canonical_parent = fs::canonicalize(parent)
            .map_err(|_| "note folder does not exist".to_string())?;
        ensure_within(root, &canonical_parent)?;
        if !canonical_parent.is_dir() {
            return Err("note folder does not exist".to_string());
        }
    }

    Ok(candidate)
}

fn modified_ms(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
}

fn to_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn entry_for(root: &Path, path: &Path) -> Option<NoteEntry> {
    let metadata = fs::metadata(path).ok()?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    let title = path.file_stem()?.to_string_lossy().into_owned();

    Some(NoteEntry {
        path: to_relative(root, path),
        name,
        title,
        modified_ms: modified_ms(&metadata),
    })
}

/// Depth-first enumeration of candidate note paths. Directory listing is cheap
/// (one `read_dir` per folder) and stays serial; the per-note `metadata` call is
/// what costs, so it moves to `resolve_entries` where it can run in parallel.
fn collect_note_paths(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut paths = Vec::new();

    while let Some((dir, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            continue;
        }

        let entries = fs::read_dir(&dir).map_err(|error| format!("cannot read folder: {error}"))?;

        for entry in entries {
            let entry = entry.map_err(|error| format!("cannot read folder entry: {error}"))?;
            let file_type = entry
                .file_type()
                .map_err(|error| format!("cannot read file type: {error}"))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();

            // Symlinks are skipped: they can escape the workspace or create cycles.
            if file_type.is_symlink() {
                continue;
            }

            if file_type.is_dir() {
                if name.starts_with('.')
                    || IGNORED_DIRS
                        .iter()
                        .any(|ignored| name.eq_ignore_ascii_case(ignored))
                {
                    continue;
                }
                pending.push((entry.path(), depth + 1));
                continue;
            }

            if !file_type.is_file() || name.starts_with('.') {
                continue;
            }

            let path = entry.path();
            if !is_markdown(&path) {
                continue;
            }

            if paths.len() >= MAX_NOTES {
                return Err(format!("workspace contains more than {MAX_NOTES} notes"));
            }

            paths.push(path);
        }
    }

    Ok(paths)
}

/// Turns candidate paths into entries. Notes whose metadata cannot be read are
/// skipped, and the work is split across threads because a stat per note is the
/// dominant syscall cost on large workspaces.
fn resolve_entries(root: &Path, paths: &[PathBuf]) -> Vec<NoteEntry> {
    if paths.len() < 2 {
        return paths
            .iter()
            .filter_map(|path| entry_for(root, path))
            .collect();
    }

    let workers = worker_count(MAX_WORKERS).min(paths.len());
    let chunk_size = paths.len().div_ceil(workers);
    let mut notes = Vec::with_capacity(paths.len());

    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk_size)
            .map(|slice| {
                scope.spawn(move || {
                    slice
                        .iter()
                        .filter_map(|path| entry_for(root, path))
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        for handle in handles {
            if let Ok(mut chunk) = handle.join() {
                notes.append(&mut chunk);
            }
        }
    });

    notes
}

/// Shared thread-count rule for scan work: enough to hide stat/read latency,
/// capped so extra threads do not just contend for one disk.
pub(crate) fn worker_count(max: usize) -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(max)
}

fn strip_markdown_extension(input: &str) -> &str {
    let lower = input.to_ascii_lowercase();
    for extension in MARKDOWN_EXTS {
        let suffix = format!(".{extension}");
        if lower.ends_with(&suffix) {
            return &input[..input.len() - suffix.len()];
        }
    }
    input
}

fn sanitize_title(input: &str) -> String {
    let trimmed = input.trim();
    let stem = strip_markdown_extension(trimmed);

    let mut output = String::new();
    for character in stem.chars() {
        let mapped = if character.is_control()
            || matches!(character, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            || character.is_whitespace()
        {
            '-'
        } else {
            character
        };

        if mapped == '-' {
            if !output.ends_with('-') {
                output.push('-');
            }
        } else {
            output.push(mapped);
        }
    }

    let output: String = output
        .trim_matches(|character: char| character == '-' || character == '.' || character.is_whitespace())
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    let output = output.trim_end_matches(['-', '.', ' ']).to_string();

    if output.is_empty() {
        return "untitled".to_string();
    }

    let reserved = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
        "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if reserved.contains(&output.to_ascii_lowercase().as_str()) {
        format!("{output}-note")
    } else {
        output
    }
}

/// Lists a workspace without touching the search snapshot; also the entry point
/// used by the benchmark and by the cold search path.
pub(crate) fn list_note_entries_at(root: &Path) -> AppResult<Vec<NoteEntry>> {
    let paths = collect_note_paths(root)?;
    let mut notes = resolve_entries(root, &paths);
    // Precompute the lowercase key once per note instead of allocating twice
    // on every comparison.
    notes.sort_by_cached_key(|note| (note.path.to_ascii_lowercase(), note.path.clone()));
    Ok(notes)
}

#[tauri::command]
pub fn list_notes(root: String) -> AppResult<Vec<NoteEntry>> {
    let notes = list_note_entries_at(&canonical_root(&root)?)?;
    // Opening or refreshing a workspace starts the background warm-up; the UI
    // never waits for it (RFC-006 §4.4).
    crate::search::start_warm(&root, &notes);
    Ok(notes)
}

#[tauri::command]
pub fn snapshot_status() -> Option<crate::search::SnapshotStatus> {
    crate::search::snapshot_status()
}

#[tauri::command]
pub fn read_note(root: String, path: String) -> AppResult<NoteDocument> {
    let root = canonical_root(&root)?;
    let target = resolve_existing(&root, &path)?;
    let size = fs::metadata(&target)
        .map_err(|error| format!("cannot read note metadata: {error}"))?
        .len();

    if size > MAX_FILE_BYTES {
        return Err(format!(
            "note is too large ({} bytes, limit {MAX_FILE_BYTES})",
            size
        ));
    }

    let bytes = fs::read(&target)
        .map_err(|error| format!("note is not valid UTF-8 or cannot be read: {error}"))?;
    let content = String::from_utf8(bytes.clone())
        .map_err(|error| format!("note is not valid UTF-8 or cannot be read: {error}"))?;

    Ok(NoteDocument {
        content,
        revision: revision_for_bytes(&bytes),
    })
}

#[tauri::command]
pub fn write_note(
    root: String,
    path: String,
    content: String,
    expected_revision: String,
) -> AppResult<String> {
    if content.len() as u64 > MAX_FILE_BYTES {
        return Err(format!(
            "note is too large ({} bytes, limit {MAX_FILE_BYTES})",
            content.len()
        ));
    }

    let root = canonical_root(&root)?;
    let target = resolve_target(&root, &path)?;

    if !target.exists() {
        return Err("conflict: note no longer exists".to_string());
    }

    let current = fs::read(&target)
        .map_err(|error| format!("cannot read note before writing: {error}"))?;
    let current_revision = revision_for_bytes(&current);
    if current_revision != expected_revision {
        return Err("conflict: note was modified outside MD Studio".to_string());
    }

    fs::write(&target, content.as_bytes()).map_err(|error| format!("cannot write note: {error}"))?;
    crate::search::cache_upsert(&root, &path, &content);

    Ok(revision_for_bytes(content.as_bytes()))
}

#[tauri::command]
pub fn create_note(root: String, title: String) -> AppResult<NoteEntry> {
    let root = canonical_root(&root)?;
    let stem = sanitize_title(&title);
    let mut suffix = 1;
    let mut relative = format!("{stem}.md");

    while root.join(&relative).exists() {
        suffix += 1;
        if suffix > 999 {
            return Err("too many notes share this title".to_string());
        }
        relative = format!("{stem}-{suffix}.md");
    }

    let relative = relative_note_path(&relative)?;
    let target = root.join(&relative);

    fs::write(&target, b"").map_err(|error| format!("cannot create note: {error}"))?;

    let entry = entry_for(&root, &target).ok_or_else(|| "cannot read the new note".to_string())?;
    crate::search::cache_upsert(&root, &entry.path, "");
    Ok(entry)
}

#[tauri::command]
pub fn delete_note(root: String, path: String) -> AppResult<DeletedNote> {
    delete_note_with(root, path, |target| {
        trash::delete(target).map_err(|error| error.to_string())
    })
}

/// Splits the delete flow from the platform trash call so tests can verify the
/// snapshot and failure semantics without touching the real recycle bin.
fn delete_note_with<F>(root: String, path: String, trash_fn: F) -> AppResult<DeletedNote>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let root = canonical_root(&root)?;
    let target = resolve_existing(&root, &path)?;
    let size = fs::metadata(&target)
        .map_err(|error| format!("cannot read note metadata: {error}"))?
        .len();

    // Read before trashing so a read failure aborts without deleting anything.
    let content = if size <= MAX_FILE_BYTES {
        let bytes = fs::read(&target)
            .map_err(|error| format!("cannot read note before deleting: {error}"))?;
        String::from_utf8(bytes).ok()
    } else {
        None
    };

    trash_fn(&target).map_err(|error| format!("cannot move note to trash: {error}"))?;

    let deleted = DeletedNote {
        path: to_relative(&root, &target),
        content,
    };
    crate::search::cache_remove(&root, &deleted.path);
    Ok(deleted)
}

#[tauri::command]
pub fn restore_note(root: String, path: String, content: String) -> AppResult<NoteEntry> {
    if content.len() as u64 > MAX_FILE_BYTES {
        return Err(format!(
            "note is too large ({} bytes, limit {MAX_FILE_BYTES})",
            content.len()
        ));
    }

    let root = canonical_root(&root)?;
    let target = resolve_target(&root, &path)?;

    if target.exists() {
        return Err("note path is already occupied".to_string());
    }

    fs::write(&target, content.as_bytes())
        .map_err(|error| format!("cannot restore note: {error}"))?;

    let entry =
        entry_for(&root, &target).ok_or_else(|| "cannot read the restored note".to_string())?;
    crate::search::cache_upsert(&root, &entry.path, &content);
    Ok(entry)
}

#[tauri::command]
pub fn rename_note(root: String, path: String, new_title: String) -> AppResult<NoteEntry> {
    let root = canonical_root(&root)?;
    let source = resolve_existing(&root, &path)?;
    let relative = relative_note_path(&path)?;
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .filter(|ext| MARKDOWN_EXTS.contains(&ext.as_str()))
        .unwrap_or_else(|| "md".to_string());
    let stem = sanitize_title(&new_title);
    let target_relative = parent.join(format!("{stem}.{extension}"));
    let target_relative_string = to_relative(&root, &root.join(&target_relative));
    relative_note_path(&target_relative_string)?;
    let target = root.join(&target_relative);

    if target.exists() {
        let canonical_target = fs::canonicalize(&target)
            .map_err(|error| format!("cannot inspect target note: {error}"))?;
        if canonical_target == source {
            return entry_for(&root, &source)
                .ok_or_else(|| "cannot read the renamed note".to_string());
        }
        return Err("a note with that name already exists".to_string());
    }

    fs::rename(&source, &target).map_err(|error| format!("cannot rename note: {error}"))?;

    let entry =
        entry_for(&root, &target).ok_or_else(|| "cannot read the renamed note".to_string())?;
    crate::search::cache_rename(&root, &path, &entry.path);
    Ok(entry)
}

/// Streams hits over `on_event` (ADR-008). The scan runs on a blocking thread,
/// and `Done` is sent before this returns, so the caller can await the promise
/// and still have seen every event.
#[tauri::command]
pub async fn search_notes(
    root: String,
    query: String,
    request_id: u64,
    on_event: tauri::ipc::Channel<crate::search::SearchEvent>,
) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = |event: crate::search::SearchEvent| on_event.send(event).is_ok();
        crate::search::search_stream_blocking(&root, &query, request_id, &mut emit)
    })
    .await
    .map_err(|error| format!("search task failed: {error}"))?
}

/// Cancels a streamed search. Batches already in flight may still arrive; the
/// frontend drops them by request id (ADR-008).
#[tauri::command]
pub fn search_cancel(request_id: u64) {
    crate::search::cancel_request(request_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_file(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn list_notes_returns_nested_markdown_and_skips_hidden() {
        // `list_notes` installs a search snapshot in a process-global slot, so
        // this test must not overlap other tests that do the same.
        let _guard = crate::search::test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "alpha.md", "a");
        write_file(root, "sub/beta.markdown", "b");
        write_file(root, "sub/ignore.txt", "x");
        write_file(root, ".hidden/secret.md", "s");
        fs::create_dir_all(root.join("node_modules")).unwrap();
        write_file(root, "node_modules/pkg.md", "p");

        let notes = list_notes(root.to_string_lossy().into_owned()).unwrap();
        let paths: Vec<_> = notes.iter().map(|note| note.path.as_str()).collect();

        assert_eq!(paths, vec!["alpha.md", "sub/beta.markdown"]);
        assert_eq!(notes[1].title, "beta");
        assert!(notes[0].modified_ms.is_some());
    }

    #[test]
    fn list_notes_sorts_case_insensitively() {
        let _guard = crate::search::test_guard();
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "Banana.md", "b");
        write_file(root, "apple.md", "a");

        let notes = list_notes(root.to_string_lossy().into_owned()).unwrap();
        let paths: Vec<_> = notes.iter().map(|note| note.path.as_str()).collect();
        assert_eq!(paths, vec!["apple.md", "Banana.md"]);
    }

    #[test]
    fn read_and_write_roundtrip() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let root_string = root.to_string_lossy().into_owned();
        write_file(root, "note.md", "# old");

        let original = read_note(root_string.clone(), "note.md".to_string()).unwrap();
        assert_eq!(original.content, "# old");
        assert_eq!(original.revision.len(), 16);

        let new_revision = write_note(
            root_string.clone(),
            "note.md".to_string(),
            "# new\n\n正文".to_string(),
            original.revision,
        )
        .unwrap();

        let document = read_note(root_string, "note.md".to_string()).unwrap();
        assert_eq!(document.content, "# new\n\n正文");
        assert_eq!(document.revision, new_revision);
    }

    #[test]
    fn revision_is_stable_and_changes_with_content() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let root_string = root.to_string_lossy().into_owned();
        write_file(root, "revision.md", "same");

        let first = read_note(root_string.clone(), "revision.md".to_string()).unwrap();
        let second = read_note(root_string.clone(), "revision.md".to_string()).unwrap();
        assert_eq!(first.revision, second.revision);

        write_file(root, "revision.md", "different");
        let third = read_note(root_string, "revision.md".to_string()).unwrap();
        assert_ne!(first.revision, third.revision);
    }

    #[test]
    fn write_note_detects_conflict_after_external_modification() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let root_string = root.to_string_lossy().into_owned();
        write_file(root, "conflict.md", "disk one");

        let document = read_note(root_string.clone(), "conflict.md".to_string()).unwrap();
        write_file(root, "conflict.md", "external edit");

        let error = write_note(
            root_string,
            "conflict.md".to_string(),
            "local edit".to_string(),
            document.revision,
        )
        .unwrap_err();

        assert!(error.starts_with("conflict:"));
        assert_eq!(
            fs::read_to_string(root.join("conflict.md")).unwrap(),
            "external edit"
        );
    }

    #[test]
    fn write_note_conflicts_when_note_is_missing() {
        let temp = tempdir().unwrap();
        let root = temp.path().to_string_lossy().into_owned();
        let error = write_note(
            root,
            "missing.md".to_string(),
            "local".to_string(),
            "0000000000000000".to_string(),
        )
        .unwrap_err();
        assert!(error.starts_with("conflict:"));
    }

    #[test]
    fn rejects_parent_directory_traversal() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "ok.md", "ok");

        let error = read_note(
            root.to_string_lossy().into_owned(),
            "../secret.md".to_string(),
        )
        .unwrap_err();
        assert!(error.contains("escapes"));

        let error = write_note(
            root.to_string_lossy().into_owned(),
            "../evil.md".to_string(),
            "x".to_string(),
            "0000000000000000".to_string(),
        )
        .unwrap_err();
        assert!(error.contains("escapes"));
    }

    #[test]
    fn rejects_absolute_paths_and_non_markdown() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "ok.md", "ok");

        let absolute = root.join("ok.md").to_string_lossy().into_owned();
        let error = read_note(root.to_string_lossy().into_owned(), absolute).unwrap_err();
        assert!(error.contains("absolute"));

        let error = read_note(
            root.to_string_lossy().into_owned(),
            "ok.txt".to_string(),
        )
        .unwrap_err();
        assert!(error.contains("Markdown"));
    }

    #[test]
    fn create_note_sanitizes_and_deduplicates() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let root_string = root.to_string_lossy().into_owned();

        let first = create_note(root_string.clone(), "Hello: World?".to_string()).unwrap();
        let second = create_note(root_string.clone(), "Hello: World?".to_string()).unwrap();

        assert_eq!(first.path, "Hello-World.md");
        assert_eq!(second.path, "Hello-World-2.md");
        assert!(root.join("Hello-World.md").exists());
    }

    #[test]
    fn create_note_falls_back_for_empty_titles() {
        let temp = tempdir().unwrap();
        let root = temp.path().to_string_lossy().into_owned();
        let note = create_note(root, "   ".to_string()).unwrap();
        assert_eq!(note.path, "untitled.md");
    }

    #[test]
    fn rejects_oversized_content() {
        let temp = tempdir().unwrap();
        let root = temp.path().to_string_lossy().into_owned();
        let oversized = "x".repeat(MAX_FILE_BYTES as usize + 1);
        let error = write_note(
            root,
            "big.md".to_string(),
            oversized,
            "0000000000000000".to_string(),
        )
        .unwrap_err();
        assert!(error.contains("too large"));
    }

    #[test]
    fn delete_note_removes_file_and_rejects_traversal() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let root_string = root.to_string_lossy().into_owned();
        write_file(root, "delete-me.md", "gone");

        delete_note_with(
            root_string.clone(),
            "delete-me.md".to_string(),
            |target| fs::remove_file(target).map_err(|error| error.to_string()),
        )
        .unwrap();
        assert!(!root.join("delete-me.md").exists());

        let error = delete_note(root_string, "../outside.md".to_string()).unwrap_err();
        assert!(error.contains("escapes"));
    }

    #[test]
    fn delete_returns_snapshot_and_moves_note_out_of_the_workspace() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "sub/gone.md", "# body");

        let deleted = delete_note_with(
            root.to_string_lossy().into_owned(),
            "sub/gone.md".to_string(),
            |target| fs::remove_file(target).map_err(|error| error.to_string()),
        )
        .unwrap();

        assert_eq!(deleted.path, "sub/gone.md");
        assert_eq!(deleted.content.as_deref(), Some("# body"));
        assert!(!root.join("sub/gone.md").exists());
    }

    #[test]
    fn delete_keeps_the_file_when_the_trash_call_fails() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "keep.md", "precious");

        let error = delete_note_with(
            root.to_string_lossy().into_owned(),
            "keep.md".to_string(),
            |_| Err("recycle bin unavailable".to_string()),
        )
        .unwrap_err();

        assert!(error.contains("cannot move note to trash"));
        assert_eq!(
            fs::read_to_string(root.join("keep.md")).unwrap(),
            "precious"
        );
    }

    #[test]
    fn delete_returns_no_snapshot_for_invalid_utf8() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("binary.md"), [0xff, 0xfe, 0xfd]).unwrap();

        let deleted = delete_note_with(
            root.to_string_lossy().into_owned(),
            "binary.md".to_string(),
            |target| fs::remove_file(target).map_err(|error| error.to_string()),
        )
        .unwrap();

        assert_eq!(deleted.content, None);
        assert!(!root.join("binary.md").exists());
    }

    #[test]
    fn restore_note_writes_the_snapshot_back() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let root_string = root.to_string_lossy().into_owned();
        write_file(root, "undo.md", "# undo me\n\n正文");

        let deleted = delete_note_with(
            root_string.clone(),
            "undo.md".to_string(),
            |target| fs::remove_file(target).map_err(|error| error.to_string()),
        )
        .unwrap();

        let restored = restore_note(
            root_string.clone(),
            deleted.path,
            deleted.content.unwrap(),
        )
        .unwrap();

        assert_eq!(restored.path, "undo.md");
        assert_eq!(
            fs::read_to_string(root.join("undo.md")).unwrap(),
            "# undo me\n\n正文"
        );
        let document = read_note(root_string, "undo.md".to_string()).unwrap();
        assert_eq!(document.content, "# undo me\n\n正文");
    }

    #[test]
    fn restore_note_refuses_to_overwrite_an_existing_file() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "taken.md", "already here");

        let error = restore_note(
            root.to_string_lossy().into_owned(),
            "taken.md".to_string(),
            "restored".to_string(),
        )
        .unwrap_err();

        assert!(error.contains("already occupied"));
        assert_eq!(
            fs::read_to_string(root.join("taken.md")).unwrap(),
            "already here"
        );
    }

    #[test]
    fn restore_note_rejects_traversal_and_oversized_content() {
        let temp = tempdir().unwrap();
        let root = temp.path().to_string_lossy().into_owned();

        let error = restore_note(root.clone(), "../escape.md".to_string(), "x".to_string())
            .unwrap_err();
        assert!(error.contains("escapes"));

        let oversized = "x".repeat(MAX_FILE_BYTES as usize + 1);
        let error = restore_note(root, "big.md".to_string(), oversized).unwrap_err();
        assert!(error.contains("too large"));
    }

    #[test]
    fn rename_note_preserves_directory_and_extension() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let root_string = root.to_string_lossy().into_owned();
        write_file(root, "sub/old.markdown", "# body");

        let renamed = rename_note(
            root_string.clone(),
            "sub/old.markdown".to_string(),
            "New Name: v2".to_string(),
        )
        .unwrap();

        assert_eq!(renamed.path, "sub/New-Name-v2.markdown");
        assert_eq!(
            fs::read_to_string(root.join("sub/New-Name-v2.markdown")).unwrap(),
            "# body"
        );
        assert!(!root.join("sub/old.markdown").exists());

        let same = rename_note(
            root_string,
            "sub/New-Name-v2.markdown".to_string(),
            "New Name: v2".to_string(),
        )
        .unwrap();
        assert_eq!(same.path, "sub/New-Name-v2.markdown");
    }

    #[test]
    fn rename_note_rejects_existing_target() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let root_string = root.to_string_lossy().into_owned();
        write_file(root, "a.md", "a");
        write_file(root, "b.md", "b");

        let error = rename_note(root_string, "a.md".to_string(), "b".to_string()).unwrap_err();

        assert!(error.contains("already exists"));
        assert!(root.join("a.md").exists());
        assert_eq!(fs::read_to_string(root.join("b.md")).unwrap(), "b");
    }

    #[test]
    fn create_note_strips_markdown_extension() {
        let temp = tempdir().unwrap();
        let root = temp.path().to_string_lossy().into_owned();
        let note = create_note(root, "readme.markdown".to_string()).unwrap();
        assert_eq!(note.path, "readme.md");
    }

}
