//! `pi-rs` — interactive coding agent CLI.

mod config;
mod file_config;
mod interactive;
mod permission;
mod print_mode;
mod project;
// ponytail: session import/JSONL APIs are library seams pending CLI wiring; remove once wired.
#[allow(dead_code)]
mod session;
mod system_prompt;
// ponytail: Termux helpers are exercised by unit tests before CLI startup wiring.
#[allow(dead_code)]
mod termux;
// ponytail: trust evaluation is staged before interactive persistence is wired.
#[allow(dead_code)]
mod trust;

#[cfg(test)]
mod tests {
    use super::project::load_project_prompt;
    use super::termux::{is_termux, termux_shell, termux_tmpdir};
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-rs-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_load_project_prompt_nested_ordering() {
        let root = temp_dir("project-prompt-nested");
        let sub = root.join("child").join("nested");
        fs::create_dir_all(&sub).unwrap();

        fs::write(root.join("AGENTS.md"), "root agents").unwrap();
        fs::write(root.join("CLAUDE.md"), "root claude").unwrap();
        fs::write(sub.join("AGENTS.md"), "child agents").unwrap();

        let prompt = load_project_prompt(&sub);

        let child_agents_pos = prompt.find("child agents").unwrap();
        let root_agents_pos = prompt.find("root agents").unwrap();
        let root_claude_pos = prompt.find("root claude").unwrap();

        assert!(
            child_agents_pos < root_agents_pos,
            "child AGENTS.md should come before root AGENTS.md"
        );
        assert!(
            root_agents_pos < root_claude_pos,
            "root AGENTS.md should come before root CLAUDE.md"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_termux_env_detection() {
        let termux_detected = is_termux();
        let sh_path = termux_shell();
        let tmp_path = termux_tmpdir();

        if std::env::var("PREFIX")
            .map(|p| p.starts_with("/data/data/com.termux/files/usr"))
            .unwrap_or(false)
        {
            assert!(termux_detected);
            assert!(sh_path.to_string_lossy().contains("com.termux"));
        } else {
            assert!(!termux_detected);
        }

        assert!(!sh_path.as_os_str().is_empty());
        assert!(!tmp_path.as_os_str().is_empty());
    }
}

use std::sync::Arc;

use clap::{Parser, Subcommand};

use crate::config::{parse_thinking_level, AppConfig};
use crate::permission::{CliPermission, Mode};

#[derive(Parser, Debug)]
#[command(name = "pi-rs", version, about = "Pi coding agent (Rust port)")]
struct Cli {
    /// One-shot prompt — run agent to completion and exit.
    #[arg(short, long, alias = "print")]
    prompt: Option<String>,

    /// Model identifier. Overrides PI_MODEL.
    #[arg(short = 'm', long, env = "PI_MODEL")]
    model: Option<String>,

    /// Maximum agent turns before stopping.
    #[arg(long)]
    max_turns: Option<u32>,

    /// Skip permission prompts (DANGEROUS — bash/write/edit run without confirm).
    #[arg(long)]
    yolo: bool,

    /// In print mode (`-p`), emit JSON-lines on stdout instead of human text.
    #[arg(long)]
    json: bool,

    /// Resume a saved session by id.
    #[arg(long)]
    resume: Option<String>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Manage saved sessions.
    Sessions {
        #[command(subcommand)]
        action: SessionAction,
    },
}

#[derive(Subcommand, Debug)]
enum SessionAction {
    /// List saved sessions.
    List,
    /// Show a single session as pretty JSON.
    Show { id: String },
    /// Delete a session by id.
    Delete { id: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // Load `$XDG_CONFIG_HOME/pi-rs/config.toml` (best-effort) before parsing
    // argv so `PI_MODEL` can be seeded from the file before the `env`
    // attribute on `Cli::model` resolves it.
    let file_cfg = file_config::load();
    if let Some(m) = &file_cfg.model {
        if std::env::var_os("PI_MODEL").is_none() {
            std::env::set_var("PI_MODEL", m);
        }
    }

    let cli = Cli::parse();
    if let Some(m) = &cli.model {
        std::env::set_var("PI_MODEL", m);
    }

    // CLI flags / env win; the file fills holes.
    let max_turns = cli.max_turns.or(file_cfg.max_turns).unwrap_or(32);
    let thinking_level = file_cfg
        .thinking_level
        .as_deref()
        .and_then(parse_thinking_level)
        .unwrap_or_default();
    let yolo = cli.yolo || file_cfg.yolo;
    let json = cli.json || file_cfg.json;

    let app = AppConfig {
        max_turns,
        thinking_level,
        ..AppConfig::default()
    };

    if let Some(Cmd::Sessions { action }) = cli.cmd {
        return run_sessions_cmd(&app, action);
    }

    let permission: Arc<dyn pi_agent::PermissionPolicy> = if yolo {
        Arc::new(CliPermission::new(Mode::Yolo))
    } else {
        Arc::new(CliPermission::new(Mode::Interactive))
    };

    let cwd = std::env::current_dir().ok();
    let explicitly_trusted = std::env::var("PI_TRUST_PROJECT").as_deref() == Ok("1");
    let trust_decision = if explicitly_trusted {
        crate::trust::TrustDecision::Trusted
    } else {
        crate::trust::evaluate_trust(cwd.as_deref(), cli.prompt.is_none())
    };

    match (cli.prompt, cli.resume) {
        (Some(p), _) => print_mode::run_print(&app, p, permission, json, trust_decision).await,
        (None, resume_id) => {
            let initial = match resume_id {
                Some(id) => match session::load(&app.config_dir, &id) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        eprintln!("warning: failed to load session {id}: {e}");
                        None
                    }
                },
                None => None,
            };
            interactive::run_interactive(&app, permission, initial, trust_decision).await
        }
    }
}

fn run_sessions_cmd(app: &AppConfig, action: SessionAction) -> anyhow::Result<()> {
    match action {
        SessionAction::List => {
            let summaries = session::list(&app.config_dir)?;
            if summaries.is_empty() {
                eprintln!("(no saved sessions)");
                return Ok(());
            }
            for s in summaries {
                let first = s
                    .first_message
                    .replace('\n', " ")
                    .chars()
                    .take(70)
                    .collect::<String>();
                println!("{}\t{}\t{}\t{}", s.id, s.model, s.turns, first);
            }
            Ok(())
        }
        SessionAction::Show { id } => {
            let s = session::load(&app.config_dir, &id)?;
            println!("{}", serde_json::to_string_pretty(&s)?);
            Ok(())
        }
        SessionAction::Delete { id } => {
            let path = session::sessions_dir(&app.config_dir).join(format!("{id}.json"));
            std::fs::remove_file(&path)?;
            eprintln!("deleted {}", path.display());
            Ok(())
        }
    }
}
