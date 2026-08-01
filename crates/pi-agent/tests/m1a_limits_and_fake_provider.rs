use futures::StreamExt;
use pi_agent::{run_agent, AgentConfig, RuntimeLimits};
use pi_ai::{
    now_ms, AssistantMessage, AssistantMessageEvent, Content, FakeProviderFactory, Message, Model,
    ProviderFactory, StopReason, Usage,
};
use std::sync::Arc;

fn assert_same_events(left: &[AssistantMessageEvent], right: &[AssistantMessageEvent]) {
    assert_eq!(left.len(), right.len());
    for (l, r) in left.iter().zip(right.iter()) {
        match (l, r) {
            (
                AssistantMessageEvent::Done {
                    reason: l_reason,
                    message: l_msg,
                },
                AssistantMessageEvent::Done {
                    reason: r_reason,
                    message: r_msg,
                },
            ) => {
                assert_eq!(l_reason, r_reason);
                assert_eq!(l_msg.api, r_msg.api);
                assert_eq!(l_msg.provider, r_msg.provider);
                assert_eq!(l_msg.model, r_msg.model);
                assert_eq!(l_msg.stop_reason, r_msg.stop_reason);
                assert_eq!(l_msg.content.len(), r_msg.content.len());
            }
            _ => panic!("unhandled or mismatched event variant"),
        }
    }
}

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

    assert_same_events(&collected1, &events);
    assert_same_events(&collected2, &events);
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

#[tokio::test]
async fn test_agent_loop_tool_execution() {
    use pi_agent::tools::write::WriteTool;
    use pi_agent::types::AgentTool;

    let test_dir = std::env::temp_dir().join(format!("pi_agent_test_{}", std::process::id()));
    let test_path = test_dir.join("test_write.txt");
    let test_path_str = test_path.to_string_lossy().to_string();

    let model = test_model();
    let events = vec![
        AssistantMessageEvent::Done {
            reason: StopReason::ToolUse,
            message: test_assistant_message(
                vec![Content::ToolCall {
                    id: "call_1".into(),
                    name: "write".into(),
                    arguments: serde_json::json!({
                        "path": test_path_str,
                        "content": "hello world"
                    }),
                }],
                StopReason::ToolUse,
            ),
        },
        AssistantMessageEvent::Done {
            reason: StopReason::Stop,
            message: test_assistant_message(vec![], StopReason::Stop),
        },
    ];
    let factory = Arc::new(FakeProviderFactory::new(events));
    let tool: Arc<dyn AgentTool> = Arc::new(WriteTool);
    let cfg = AgentConfig::new(model, "system")
        .with_provider_factory(factory)
        .with_tools(vec![tool]);

    let result = run_agent(&cfg, Message::user_text("hello"), None).await;
    assert!(result.is_ok());
    let run = result.unwrap();

    let tool_result_found = run.messages.iter().any(|msg| match msg {
        Message::ToolResult(tr) => {
            tr.tool_call_id == "call_1" && tr.tool_name == "write" && !tr.is_error
        }
        _ => false,
    });
    assert!(tool_result_found);

    if test_dir.exists() {
        let _ = std::fs::remove_dir_all(&test_dir);
    }
}

#[tokio::test]
async fn test_serial_tool_loop_transcript_and_provider_calls() {
    use pi_agent::tools::write::WriteTool;
    use pi_agent::types::AgentTool;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingFakeProviderFactory {
        inner: FakeProviderFactory,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ProviderFactory for CountingFakeProviderFactory {
        async fn stream(
            &self,
            model: &Model,
            context: &pi_ai::Context,
            options: &pi_ai::StreamOptions,
        ) -> pi_ai::Result<pi_ai::AssistantMessageEventStream> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.stream(model, context, options).await
        }
    }

    let test_dir = std::env::temp_dir().join(format!("pi_agent_serial_test_{}", std::process::id()));
    let test_path = test_dir.join("serial_write.txt");
    let test_path_str = test_path.to_string_lossy().to_string();

    let model = test_model();
    let events = vec![
        AssistantMessageEvent::Done {
            reason: StopReason::ToolUse,
            message: test_assistant_message(
                vec![Content::ToolCall {
                    id: "call_1".into(),
                    name: "write".into(),
                    arguments: serde_json::json!({
                        "path": test_path_str,
                        "content": "serial step 1"
                    }),
                }],
                StopReason::ToolUse,
            ),
        },
        AssistantMessageEvent::Done {
            reason: StopReason::Stop,
            message: test_assistant_message(
                vec![Content::text("done serial tool call")],
                StopReason::Stop,
            ),
        },
    ];

    let counting_factory = Arc::new(CountingFakeProviderFactory {
        inner: FakeProviderFactory::new(events),
        calls: AtomicUsize::new(0),
    });

    let tool: Arc<dyn AgentTool> = Arc::new(WriteTool);
    let cfg = AgentConfig::new(model, "system")
        .with_provider_factory(counting_factory.clone())
        .with_tools(vec![tool]);

    let user_msg = Message::user_text("run serial tool");
    let result = run_agent(&cfg, user_msg, None).await;
    assert!(result.is_ok());
    let run = result.unwrap();

    assert_eq!(counting_factory.calls.load(Ordering::SeqCst), 2);
    assert_eq!(run.messages.len(), 4);

    match &run.messages[0] {
        Message::User(u) => assert_eq!(u.content, vec![Content::text("run serial tool")]),
        _ => panic!("expected User message at index 0"),
    }
    match &run.messages[1] {
        Message::Assistant(a) => {
            assert_eq!(a.stop_reason, StopReason::ToolUse);
            assert!(matches!(&a.content[0], Content::ToolCall { id, name, .. } if id == "call_1" && name == "write"));
        }
        _ => panic!("expected Assistant message at index 1"),
    }
    match &run.messages[2] {
        Message::ToolResult(tr) => {
            assert_eq!(tr.tool_call_id, "call_1");
            assert_eq!(tr.tool_name, "write");
            assert!(!tr.is_error);
        }
        _ => panic!("expected ToolResult message at index 2"),
    }
    match &run.messages[3] {
        Message::Assistant(a) => {
            assert_eq!(a.stop_reason, StopReason::Stop);
            assert_eq!(a.content, vec![Content::text("done serial tool call")]);
        }
        _ => panic!("expected Assistant final message at index 3"),
    }

    if test_dir.exists() {
        let _ = std::fs::remove_dir_all(&test_dir);
    }
}
