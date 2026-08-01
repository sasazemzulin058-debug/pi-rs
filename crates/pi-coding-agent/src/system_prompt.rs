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
/// walking up from `cwd`.
pub fn build_system_prompt(_config_dir: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let explicitly_trusted = std::env::var("PI_TRUST_PROJECT").as_deref() == Ok("1");
    if crate::trust::evaluate_trust(Some(&cwd), explicitly_trusted)
        != crate::trust::TrustDecision::Trusted
    {
        return BASE_SYSTEM_PROMPT.to_string();
    }
    let project = crate::project::load_project_prompt(&cwd);
    if project.is_empty() {
        BASE_SYSTEM_PROMPT.to_string()
    } else {
        format!("{BASE_SYSTEM_PROMPT}\n----- project instructions -----{project}")
    }
}
