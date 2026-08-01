//! Default coding-agent system prompt, plus AGENTS.md loading.

use std::path::Path;

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

/// Build the full system prompt: base instructions concatenated with any
/// project-local AGENTS.md / CLAUDE.md / .pi/instructions.md found while
/// walking up from `cwd`, provided `trust_decision` is `TrustDecision::Trusted`.
pub fn build_system_prompt(
    _config_dir: &Path,
    trust_decision: crate::trust::TrustDecision,
) -> String {
    if trust_decision != crate::trust::TrustDecision::Trusted {
        return BASE_SYSTEM_PROMPT.to_string();
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let project = crate::project::load_project_prompt(&cwd);
    if project.is_empty() {
        BASE_SYSTEM_PROMPT.to_string()
    } else {
        format!("{BASE_SYSTEM_PROMPT}\n----- project instructions -----{project}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::TrustDecision;
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

        // Default / non-trusted decisions must NOT load project instructions
        let prompt_untrusted = build_system_prompt(&dir, TrustDecision::Untrusted);
        assert_eq!(prompt_untrusted, BASE_SYSTEM_PROMPT);

        let prompt_unknown = build_system_prompt(&dir, TrustDecision::Unknown);
        assert_eq!(prompt_unknown, BASE_SYSTEM_PROMPT);

        // When trusted, if run inside directory with AGENTS.md, it should include instructions
        let orig_cwd = std::env::current_dir().unwrap();
        if std::env::set_current_dir(&dir).is_ok() {
            let prompt_trusted = build_system_prompt(&dir, TrustDecision::Trusted);
            assert!(prompt_trusted.contains("secret project instructions"));
            let _ = std::env::set_current_dir(orig_cwd);
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
