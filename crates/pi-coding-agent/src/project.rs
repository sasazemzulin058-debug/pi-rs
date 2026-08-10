//! Project-prompt loading: walks up from canonical project root, gathers `AGENTS.md`,
//! `CLAUDE.md`, and `.pi/instructions.md` files, and concatenates them under
//! visible separators.

use crate::trust::CanonicalProjectRoot;
use std::fs;
use std::path::{Path, PathBuf};

const FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", ".pi/instructions.md"];

/// Load every project prompt fragment from trusted canonical project root and ancestors, joined with separators.
/// Rejects resource symlinks and symlinked directories escaping owning ancestor at discovery time;
/// this is not race-proof against concurrent filesystem replacement.
/// Returns an empty string if there are no fragments.
pub fn load_project_prompt(project_root: &CanonicalProjectRoot) -> String {
    let mut found: Vec<(PathBuf, String)> = Vec::new();
    let mut seen_canonical_paths: Vec<PathBuf> = Vec::new();

    for ancestor in project_root.path().ancestors() {
        let Ok(canonical_ancestor) = ancestor.canonicalize() else {
            continue;
        };

        for name in FILES {
            let candidate_path = ancestor.join(name);

            // Symlink check on candidate and parent dirs
            if is_or_contains_symlink(&candidate_path, &canonical_ancestor) {
                continue;
            }

            let Ok(meta) = fs::symlink_metadata(&candidate_path) else {
                continue;
            };

            if !meta.is_file() || meta.file_type().is_symlink() {
                continue;
            }

            let Ok(canonical_file) = candidate_path.canonicalize() else {
                continue;
            };

            if !canonical_file.starts_with(&canonical_ancestor) {
                continue;
            }

            if seen_canonical_paths.contains(&canonical_file) {
                continue;
            }

            if let Ok(content) = fs::read_to_string(&candidate_path) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    seen_canonical_paths.push(canonical_file);
                    found.push((candidate_path, trimmed.to_string()));
                }
            }
        }
    }

    if found.is_empty() {
        return String::new();
    }
    let mut buf = String::new();
    for (path, content) in found {
        buf.push_str(&format!("\n\n----- {} -----\n", path.display()));
        buf.push_str(&content);
    }
    buf
}

fn is_or_contains_symlink(path: &Path, canonical_ancestor: &Path) -> bool {
    let mut curr = path.to_path_buf();
    while curr.starts_with(canonical_ancestor) {
        if let Ok(meta) = fs::symlink_metadata(&curr) {
            if meta.file_type().is_symlink() {
                return true;
            }
        }
        if !curr.pop() || curr == canonical_ancestor {
            break;
        }
    }
    false
}
