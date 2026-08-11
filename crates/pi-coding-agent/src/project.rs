//! Project-prompt loading: walks up from canonical project root, gathers `AGENTS.md`,
//! `CLAUDE.md`, and `.pi/instructions.md` files, and concatenates them under
//! visible separators.

use crate::trust::CanonicalProjectRoot;
use std::fs;
use std::path::{Path, PathBuf};

const FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", ".pi/instructions.md"];

fn find_shadowed_context_file(canonical_project_root: &Path) -> Option<PathBuf> {
    let mut found_gitfile = None;
    for ancestor in canonical_project_root.ancestors() {
        let git_candidate = ancestor.join(".git");
        if let Ok(meta) = fs::symlink_metadata(&git_candidate) {
            if meta.is_dir() {
                return None;
            } else if meta.is_file() && !meta.file_type().is_symlink() {
                found_gitfile = Some((ancestor.to_path_buf(), git_candidate));
                break;
            } else {
                return None;
            }
        }
    }

    let (worktree_root, gitfile_path) = found_gitfile?;
    let content = fs::read_to_string(&gitfile_path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() != 1 {
        return None;
    }
    // Git's file grammar is exactly `gitdir: <path>`; do not accept
    // whitespace-normalized variants or trailing fields.
    let line = lines[0];
    let gitdir_str = line.strip_prefix("gitdir: ")?;
    if gitdir_str.is_empty() || gitdir_str.trim() != gitdir_str {
        return None;
    }

    let raw_gitdir = Path::new(gitdir_str);
    let gitdir = if raw_gitdir.is_absolute() {
        raw_gitdir.to_path_buf()
    } else {
        worktree_root.join(raw_gitdir)
    };
    let canonical_gitdir = gitdir.canonicalize().ok()?;

    let commondir_file = canonical_gitdir.join("commondir");
    let commondir_content = fs::read_to_string(&commondir_file).ok()?;
    let commondir_lines: Vec<&str> = commondir_content.lines().collect();
    if commondir_lines.len() != 1 {
        return None;
    }
    let raw_commondir_str = commondir_lines[0].trim();
    if raw_commondir_str.is_empty() {
        return None;
    }

    let raw_commondir = Path::new(raw_commondir_str);
    let common_git = if raw_commondir.is_absolute() {
        raw_commondir.to_path_buf()
    } else {
        canonical_gitdir.join(raw_commondir)
    };
    let canonical_common_git = common_git.canonicalize().ok()?;

    let main_repo_root = canonical_common_git.parent()?.canonicalize().ok()?;
    let expected_main_git = main_repo_root.join(".git").canonicalize().ok()?;
    if canonical_common_git != expected_main_git {
        return None;
    }

    let canonical_worktree_root = worktree_root.canonicalize().ok()?;
    if canonical_worktree_root == main_repo_root
        || !canonical_worktree_root.starts_with(&main_repo_root)
    {
        return None;
    }

    let has_wt_agents = worktree_root.join("AGENTS.md").is_file();
    let has_wt_claude = worktree_root.join("CLAUDE.md").is_file();

    if has_wt_agents {
        Some(main_repo_root.join("AGENTS.md"))
    } else if has_wt_claude {
        Some(main_repo_root.join("CLAUDE.md"))
    } else {
        None
    }
}

/// Load every project prompt fragment from trusted canonical project root and ancestors, joined with separators.
/// Rejects resource symlinks and symlinked directories escaping owning ancestor at discovery time;
/// this is not race-proof against concurrent filesystem replacement.
/// Returns an empty string if there are no fragments.
pub fn load_project_prompt(project_root: &CanonicalProjectRoot) -> String {
    let mut found: Vec<(PathBuf, String)> = Vec::new();
    let mut seen_canonical_paths: Vec<PathBuf> = Vec::new();

    let shadowed_file =
        find_shadowed_context_file(project_root.path()).and_then(|p| p.canonicalize().ok());

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

            if let Some(ref shadowed) = shadowed_file {
                if canonical_file == *shadowed {
                    continue;
                }
            }

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
