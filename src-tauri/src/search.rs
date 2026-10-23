use std::fs;
use std::path::{Path, PathBuf};

use aho_corasick::AhoCorasick;
use serde::Serialize;

use crate::notes::{canonical_root, ensure_within, list_notes, relative_note_path};

pub const MAX_SEARCH_HITS: usize = 50;
pub const MAX_HITS_PER_NOTE: usize = 3;
pub const PREVIEW_CHARS: usize = 90;

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
pub fn search_notes_blocking(root: &str, query: &str) -> Result<Vec<SearchHit>, String> {
    let needle = query.trim();
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let matcher = Matcher::new(needle);
    let root_path = canonical_root(root)?;
    let notes = list_notes(root.to_string())?;
    let mut hits = Vec::new();

    for note in notes {
        if hits.len() >= MAX_SEARCH_HITS {
            break;
        }

        let target = match resolve_search_target(&root_path, &note.path) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let text = match fs::read_to_string(&target) {
            Ok(text) => text,
            Err(_) => continue,
        };

        let mut note_hits = 0;
        for (index, line) in text.lines().enumerate() {
            if note_hits >= MAX_HITS_PER_NOTE || hits.len() >= MAX_SEARCH_HITS {
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

    Ok(hits)
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
