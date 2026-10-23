use serde::Serialize;

use crate::notes::{canonical_root, list_notes, resolve_existing};

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
/// `MAX_SEARCH_HITS` line hits. This is the pre-RFC-006 implementation,
/// kept unchanged so benchmarks can capture a baseline before L0/L1/L2.
pub fn search_notes_blocking(root: &str, query: &str) -> Result<Vec<SearchHit>, String> {
    let needle = query.trim();
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let needle_lower = needle.to_lowercase();
    let root_path = canonical_root(root)?;
    let notes = list_notes(root.to_string())?;
    let mut hits = Vec::new();

    for note in notes {
        if hits.len() >= MAX_SEARCH_HITS {
            break;
        }

        let target = match resolve_existing(&root_path, &note.path) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let text = match std::fs::read_to_string(&target) {
            Ok(text) => text,
            Err(_) => continue,
        };

        let mut note_hits = 0;
        for (index, line) in text.lines().enumerate() {
            if note_hits >= MAX_HITS_PER_NOTE || hits.len() >= MAX_SEARCH_HITS {
                break;
            }
            if line.to_lowercase().contains(&needle_lower) {
                hits.push(SearchHit {
                    path: note.path.clone(),
                    title: note.title.clone(),
                    line: (index + 1) as u32,
                    preview: build_preview(line, &needle_lower),
                });
                note_hits += 1;
            }
        }
    }

    Ok(hits)
}

fn build_preview(line: &str, needle_lower: &str) -> String {
    let trimmed = line.trim();
    let lowered = trimmed.to_lowercase();
    let byte_index = lowered.find(needle_lower).unwrap_or(0);
    let chars: Vec<char> = trimmed.chars().collect();
    let char_index = lowered[..byte_index].chars().count().min(chars.len());
    let needle_len = needle_lower.chars().count();
    let start = char_index
        .saturating_sub(PREVIEW_CHARS / 3)
        .min(chars.len());
    let end = (char_index + needle_len + PREVIEW_CHARS)
        .max(start)
        .min(chars.len());
    let mut preview: String = chars[start..end].iter().collect();

    if start > 0 {
        preview.insert(0, '…');
    }
    if end < chars.len() {
        preview.push('…');
    }

    preview
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
