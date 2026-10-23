use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_NOTES: usize = 10_000;
const MAX_DEPTH: usize = 12;
const MAX_TITLE_CHARS: usize = 80;

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

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| MARKDOWN_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn canonical_root(root: &str) -> AppResult<PathBuf> {
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

fn relative_note_path(relative: &str) -> AppResult<PathBuf> {
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

fn ensure_within(root: &Path, path: &Path) -> AppResult<()> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err("note path escapes the workspace".to_string())
    }
}

fn resolve_existing(root: &Path, relative: &str) -> AppResult<PathBuf> {
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

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<NoteEntry>) -> AppResult<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }

    let entries = fs::read_dir(dir).map_err(|error| format!("cannot read folder: {error}"))?;

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
            if name.starts_with('.') || IGNORED_DIRS.iter().any(|dir| name.eq_ignore_ascii_case(dir)) {
                continue;
            }
            walk(root, &entry.path(), depth + 1, out)?;
            continue;
        }

        if !file_type.is_file() || name.starts_with('.') {
            continue;
        }

        let path = entry.path();
        if !is_markdown(&path) {
            continue;
        }

        if out.len() >= MAX_NOTES {
            return Err(format!("workspace contains more than {MAX_NOTES} notes"));
        }

        if let Some(note) = entry_for(root, &path) {
            out.push(note);
        }
    }

    Ok(())
}

fn sanitize_title(input: &str) -> String {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();
    let stem = if lower.ends_with(".md") {
        &trimmed[..trimmed.len() - 3]
    } else {
        trimmed
    };

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

#[tauri::command]
pub fn list_notes(root: String) -> AppResult<Vec<NoteEntry>> {
    let root = canonical_root(&root)?;
    let mut notes = Vec::new();
    walk(&root, &root, 0, &mut notes)?;
    notes.sort_by(|left, right| {
        left.path
            .to_ascii_lowercase()
            .cmp(&right.path.to_ascii_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(notes)
}

#[tauri::command]
pub fn read_note(root: String, path: String) -> AppResult<String> {
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

    fs::read_to_string(&target)
        .map_err(|error| format!("note is not valid UTF-8 or cannot be read: {error}"))
}

#[tauri::command]
pub fn write_note(root: String, path: String, content: String) -> AppResult<()> {
    if content.len() as u64 > MAX_FILE_BYTES {
        return Err(format!(
            "note is too large ({} bytes, limit {MAX_FILE_BYTES})",
            content.len()
        ));
    }

    let root = canonical_root(&root)?;
    let target = resolve_target(&root, &path)?;

    fs::write(&target, content.as_bytes()).map_err(|error| format!("cannot write note: {error}"))
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

    entry_for(&root, &target).ok_or_else(|| "cannot read the new note".to_string())
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
    fn read_and_write_roundtrip() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_file(root, "note.md", "# old");

        write_note(
            root.to_string_lossy().into_owned(),
            "note.md".to_string(),
            "# new\n\n正文".to_string(),
        )
        .unwrap();

        let content = read_note(
            root.to_string_lossy().into_owned(),
            "note.md".to_string(),
        )
        .unwrap();
        assert_eq!(content, "# new\n\n正文");
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
        let error = write_note(root, "big.md".to_string(), oversized).unwrap_err();
        assert!(error.contains("too large"));
    }
}
