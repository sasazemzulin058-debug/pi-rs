use futures::StreamExt;
use pi_agent::{run_agent, AgentConfig, RuntimeLimits};
use pi_ai::{
    now_ms, AssistantMessage, AssistantMessageEvent, Content, FakeProviderFactory, Message,
    Model, StopReason, Usage,
};
use std::sync::Arc;

fn test_model() -> Model {
    Model::openai_compat(
        "test-provider",
        "test-model",
        "https://api.test.com/v1",
        128_000,
        4096,
    )
}

fn test_assistant_message(content: Vec<Content>, stop_reason: StopReason) -> AssistantMessage {
    AssistantMessage {
        content,
        api: "openai-completions".to_string(),
        provider: "test-provider".to_string(),
        model: "test-model".to_string(),
        usage: Usage::default(),
        stop_reason,
        error_message: None,
        timestamp: now_ms(),
    }
}

#[test]
fn test_runtime_limits_default_and_override() {
    let model = test_model();
    let cfg = AgentConfig::new(model.clone(), "system");
    assert_eq!(cfg.runtime_limits.max_turns, 32);
    assert_eq!(RuntimeLimits::default().max_turns, 32);

    let cfg_override = cfg.with_max_turns(5);
    assert_eq!(cfg_override.runtime_limits.max_turns, 5);

    let cfg_zero = AgentConfig::new(model, "system").with_max_turns(0);
    assert_eq!(cfg_zero.runtime_limits.max_turns, 0);
}

#[tokio::test]
async fn test_repeatable_fake_provider_stream() {
    let model = test_model();
    let events = vec![AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_message(vec![], StopReason::Stop),
    }];
    let factory = FakeProviderFactory::new(events.clone());

    let mut stream1 = factory
        .stream(&model, &Default::default(), &Default::default())
        .await
        .unwrap();
    let mut collected1 = Vec::new();
    while let Some(evt) = stream1.next().await {
        collected1.push(evt.unwrap());
    }

    let mut stream2 = factory
        .stream(&model, &Default::default(), &Default::default())
        .await
        .unwrap();
    let mut collected2 = Vec::new();
    while let Some(evt) = stream2.next().await {
        collected2.push(evt.unwrap());
    }

    assert_eq!(collected1, events);
    assert_eq!(collected2, events);
    assert_eq!(collected1, collected2);
}

#[tokio::test]
async fn test_agent_loop_turn_limit_enforcement() {
    let model = test_model();
    let events = vec![AssistantMessageEvent::Done {
        reason: StopReason::ToolUse,
        message: test_assistant_message(
            vec![Content::ToolCall {
                id: "call_1".into(),
                name: "dummy".into(),
                arguments: serde_json::json!({}),
            }],
            StopReason::ToolUse,
        ),
    }];
    let factory = Arc::new(FakeProviderFactory::new(events));
    let cfg = AgentConfig::new(model, "system")
        .with_provider_factory(factory)
        .with_max_turns(2);

    let result = run_agent(&cfg, Message::user_text("hello"), None).await;
    assert!(result.is_ok());
    let run = result.unwrap();
    assert!(run.stopped_at_turn_limit);
}

#[tokio::test]
async fn test_agent_loop_zero_turn_limit() {
    let model = test_model();
    let events = vec![AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_message(vec![], StopReason::Stop),
    }];
    let factory = Arc::new(FakeProviderFactory::new(events));
    let cfg = AgentConfig::new(model, "system")
        .with_provider_factory(factory)
        .with_max_turns(0);

    let result = run_agent(&cfg, Message::user_text("hello"), None).await;
    assert!(result.is_ok());
    let run = result.unwrap();
    assert!(run.stopped_at_turn_limit);
}
