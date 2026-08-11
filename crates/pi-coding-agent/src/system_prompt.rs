//! Default coding-agent system prompt, plus AGENTS.md loading.

// ponytail: ceiling is path-based nested module staging to prevent main.rs edit conflicts; upgrade path is crate-root mod resources in main.rs when runtime consumer lands.
#[path = "resources.rs"]
pub mod resources;

use std::path::{Path, PathBuf};

pub const BASE_SYSTEM_PROMPT: &str = r#"You are pi, an interactive coding assistant running in a terminal.

You have access to tools for reading and modifying files, listing directories, searching with grep and glob, running shell commands via bash, fetching URLs, and tracking todos. Use them to investigate the user's repository and make focused, correct changes.

Guidelines:
- Prefer reading files before editing them; never invent code that you have not verified.
- Make small, focused diffs. Do not introduce unrelated refactors.
- After making changes, summarize what you did briefly and accurately.
- For shell-only tasks (build, test, run), use the bash tool with sensible timeouts.
- When asked an open-ended question, prefer concise answers grounded in actual files.

You operate inside the user's working directory; relative paths resolve from there.
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPromptResult {
    pub prompt: String,
    pub system_prompt_source: Option<PathBuf>,
    pub append_system_prompt_sources: Vec<PathBuf>,
}

fn read_existing_utf8_file(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    Some(content)
}

/// Build the full system prompt result with tracked file sources.
pub fn build_system_prompt_with_sources(
    config_dir: &Path,
    trust_context: &(
        crate::trust::TrustDecision,
        Option<crate::trust::CanonicalProjectRoot>,
    ),
) -> SystemPromptResult {
    let (decision, root) = trust_context;
    let is_trusted = *decision == crate::trust::TrustDecision::Trusted;

    let project_root_path = match (is_trusted, root) {
        (true, Some(r)) => Some(r.path()),
        _ => None,
    };

    // 1. Base system prompt discovery
    let (base_prompt, system_prompt_source) = if let Some(proj) = project_root_path {
        let proj_sys = proj.join(".pi").join("SYSTEM.md");
        if let Some(content) = read_existing_utf8_file(&proj_sys) {
            (content, Some(proj_sys))
        } else {
            let global_sys = config_dir.join("SYSTEM.md");
            if let Some(content) = read_existing_utf8_file(&global_sys) {
                (content, Some(global_sys))
            } else {
                (BASE_SYSTEM_PROMPT.to_string(), None)
            }
        }
    } else {
        let global_sys = config_dir.join("SYSTEM.md");
        if let Some(content) = read_existing_utf8_file(&global_sys) {
            (content, Some(global_sys))
        } else {
            (BASE_SYSTEM_PROMPT.to_string(), None)
        }
    };

    // 2. Append system prompt discovery
    let (append_prompt, append_sources) = if let Some(proj) = project_root_path {
        let proj_app = proj.join(".pi").join("APPEND_SYSTEM.md");
        if let Some(content) = read_existing_utf8_file(&proj_app) {
            (Some(content), vec![proj_app])
        } else {
            let global_app = config_dir.join("APPEND_SYSTEM.md");
            if let Some(content) = read_existing_utf8_file(&global_app) {
                (Some(content), vec![global_app])
            } else {
                (None, Vec::new())
            }
        }
    } else {
        let global_app = config_dir.join("APPEND_SYSTEM.md");
        if let Some(content) = read_existing_utf8_file(&global_app) {
            (Some(content), vec![global_app])
        } else {
            (None, Vec::new())
        }
    };

    let mut prompt = base_prompt;

    if is_trusted {
        if let Some(ref canonical_root) = root {
            let project = crate::project::load_project_prompt(canonical_root);
            if !project.is_empty() {
                prompt.push_str("\n----- project instructions -----");
                prompt.push_str(&project);
            }
        }
    }

    if let Some(app) = append_prompt {
        prompt.push_str("\n----- append instructions -----\n");
        prompt.push_str(&app);
    }

    SystemPromptResult {
        prompt,
        system_prompt_source,
        append_system_prompt_sources: append_sources,
    }
}

/// Build the full system prompt: base instructions concatenated with any
/// project-local AGENTS.md / CLAUDE.md / .pi/instructions.md found while
/// walking up from canonical project root, provided `trust_context` is `(TrustDecision::Trusted, Some(root))`.
pub fn build_system_prompt(
    config_dir: &Path,
    trust_context: &(
        crate::trust::TrustDecision,
        Option<crate::trust::CanonicalProjectRoot>,
    ),
) -> String {
    build_system_prompt_with_sources(config_dir, trust_context).prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::{CanonicalProjectRoot, TrustDecision};
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-rs-sysprompt-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_build_system_prompt_trust_boundary() {
        let dir = temp_dir("trust-boundary");
        fs::write(dir.join("AGENTS.md"), "secret project instructions").unwrap();

        let root = CanonicalProjectRoot::new(&dir).unwrap();

        // Default / non-trusted decisions must NOT load project instructions
        let prompt_untrusted =
            build_system_prompt(&dir, &(TrustDecision::Untrusted, Some(root.clone())));
        assert_eq!(prompt_untrusted, BASE_SYSTEM_PROMPT);

        let prompt_unknown =
            build_system_prompt(&dir, &(TrustDecision::Unknown, Some(root.clone())));
        assert_eq!(prompt_unknown, BASE_SYSTEM_PROMPT);

        let prompt_trusted = build_system_prompt(&dir, &(TrustDecision::Trusted, Some(root)));
        assert!(prompt_trusted.contains("secret project instructions"));

        let _ = fs::remove_dir_all(&dir);
    }
}
