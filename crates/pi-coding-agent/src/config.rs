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

impl AppConfig {
    pub fn new() -> anyhow::Result<Self> {
        let config_dir = dirs::config_dir()
            .map(|p| p.join(APP_NAME))
            .unwrap_or_else(|| PathBuf::from(".pi"));
        Ok(Self {
            model: default_model_from_env()?,
            max_turns: 32,
            thinking_level: ThinkingLevel::Off,
            config_dir,
        })
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

pub fn resolve_model(id: &str) -> anyhow::Result<Model> {
    match id.to_ascii_lowercase().as_str() {
        "claude-sonnet-4-6" | "claude-sonnet" | "sonnet" => {
            Ok(Model::anthropic_claude_sonnet_4_6())
        }
        "claude-opus-4-7" | "claude-opus" | "opus" => Ok(Model::anthropic_claude_opus_4_7()),
        "gpt-4o" => Ok(Model::openai_gpt_4o()),
        "gpt-4o-mini" => Ok(Model::openai_gpt_4o_mini()),
        "gpt-5" => Ok(Model::openai_gpt_5()),
        "gemini-2.0-flash" | "gemini" | "gemini-flash" => Ok(Model::gemini_2_0_flash()),
        _ => anyhow::bail!("unknown or unsupported explicit model identifier: '{id}'"),
    }
}

pub fn default_model_from_env() -> anyhow::Result<Model> {
    if let Ok(id) = std::env::var("PI_MODEL") {
        return resolve_model(&id);
    }
    if std::env::var("GEMINI_API_KEY").is_ok() {
        Ok(Model::gemini_2_0_flash())
    } else if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        Ok(Model::anthropic_claude_sonnet_4_6())
    } else if std::env::var("OPENAI_API_KEY").is_ok() {
        Ok(Model::openai_gpt_4o_mini())
    } else {
        Ok(Model::anthropic_claude_sonnet_4_6())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_model_gemini_and_known_models() {
        assert_eq!(
            resolve_model("gemini-2.0-flash").unwrap().provider,
            "google"
        );
        assert_eq!(resolve_model("gemini").unwrap().provider, "google");
        assert_eq!(resolve_model("sonnet").unwrap().provider, "anthropic");
        assert_eq!(resolve_model("gpt-4o").unwrap().provider, "openai");
    }

    #[test]
    fn test_resolve_model_unknown_errors() {
        assert!(resolve_model("unknown-model-xyz").is_err());
    }
}
