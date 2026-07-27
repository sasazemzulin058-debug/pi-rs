use std::path::PathBuf;

use pi_ai::{Model, ThinkingLevel};

pub const APP_NAME: &str = "pi-rs";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub model: Model,
    pub max_turns: u32,
    pub thinking_level: ThinkingLevel,
    pub config_dir: PathBuf,
}

impl Default for AppConfig {
    fn default() -> Self {
        let config_dir = dirs::config_dir()
            .map(|p| p.join(APP_NAME))
            .unwrap_or_else(|| PathBuf::from(".pi"));
        Self {
            model: default_model_from_env(),
            max_turns: 32,
            thinking_level: ThinkingLevel::Off,
            config_dir,
        }
    }
}

/// Parse a `thinking_level` string from the file config into a
/// [`ThinkingLevel`]. Unknown values return `None`.
pub fn parse_thinking_level(s: &str) -> Option<ThinkingLevel> {
    match s.to_ascii_lowercase().as_str() {
        "off" => Some(ThinkingLevel::Off),
        "minimal" => Some(ThinkingLevel::Minimal),
        "low" => Some(ThinkingLevel::Low),
        "medium" => Some(ThinkingLevel::Medium),
        "high" => Some(ThinkingLevel::High),
        "xhigh" => Some(ThinkingLevel::Xhigh),
        _ => None,
    }
}

pub fn default_model_from_env() -> Model {
    if let Ok(id) = std::env::var("PI_MODEL") {
        match id.as_str() {
            "claude-sonnet-4-6" | "claude-sonnet" | "sonnet" => {
                return Model::anthropic_claude_sonnet_4_6();
            }
            "claude-opus-4-7" | "claude-opus" | "opus" => {
                return Model::anthropic_claude_opus_4_7();
            }
            "gpt-4o" => return Model::openai_gpt_4o(),
            "gpt-4o-mini" => return Model::openai_gpt_4o_mini(),
            _ => {}
        }
    }
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        Model::anthropic_claude_sonnet_4_6()
    } else if std::env::var("OPENAI_API_KEY").is_ok() {
        Model::openai_gpt_4o_mini()
    } else {
        Model::anthropic_claude_sonnet_4_6()
    }
}
