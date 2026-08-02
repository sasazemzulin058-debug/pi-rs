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
async fn test_agent_retries_context_overflow_once() {
    let model = test_model();
    let done = AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_message(vec![Content::text("ok")], StopReason::Stop),
    };
    let factory = Arc::new(FakeProviderFactory::new_sequence(vec![
        vec![Err(pi_ai::Error::ProviderError {
            status: 400,
            body: "context_length_exceeded".into(),
        })],
        vec![Ok(done)],
    ]));
    let cfg = AgentConfig::new(model, "system").with_provider_factory(factory);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    // We need messages length > 2 so that compact_messages returns true,
    // otherwise the agent loop fails with context overflow instead of retrying,
    // because it detects compaction did not reduce the history.
    let messages = vec![
        Message::user_text("system-like or user initial query"),
        Message::Assistant(test_assistant_message(
            vec![Content::text("assistant response 1")],
            StopReason::Stop,
        )),
        Message::user_text("user response 2"),
    ];
    let result = pi_agent::run_agent_with_history(&cfg, messages, Some(tx))
        .await
        .unwrap();
    assert!(result
        .messages
        .iter()
        .any(|m| matches!(m, Message::Assistant(_))));
    assert!(matches!(
        rx.recv().await,
        Some(pi_agent::AgentEvent::UserMessage { .. })
    ));
    let mut compacted = false;
    while let Ok(event) = rx.try_recv() {
        compacted |= matches!(event, pi_agent::AgentEvent::AutoCompacted);
    }
    assert!(compacted);
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

    let test_dir =
        std::env::temp_dir().join(format!("pi_agent_serial_test_{}", std::process::id()));
    let test_path = test_dir.join("serial_write.txt");
    let test_path_str = test_path.to_string_lossy().to_string();

    let model = test_model();
    let events = vec![AssistantMessageEvent::Done {
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
    }];

    let counting_factory = Arc::new(CountingFakeProviderFactory {
        inner: FakeProviderFactory::new(events),
        calls: AtomicUsize::new(0),
    });

    let tool: Arc<dyn AgentTool> = Arc::new(WriteTool);
    let cfg = AgentConfig::new(model, "system")
        .with_provider_factory(counting_factory.clone())
        .with_tools(vec![tool])
        .with_max_turns(2);

    let user_msg = Message::user_text("run serial tool");
    let result = run_agent(&cfg, user_msg, None).await;
    assert!(result.is_ok());
    let run = result.unwrap();

    assert_eq!(counting_factory.calls.load(Ordering::SeqCst), 2);
    assert_eq!(run.messages.len(), 5);

    match &run.messages[0] {
        Message::User { content, .. } => {
            assert_eq!(content, &vec![Content::text("run serial tool")]);
        }
        _ => panic!("expected User message at index 0"),
    }
    match &run.messages[1] {
        Message::Assistant(a) => {
            assert_eq!(a.stop_reason, StopReason::ToolUse);
            assert!(
                matches!(&a.content[0], Content::ToolCall { id, name, .. } if id == "call_1" && name == "write")
            );
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
    assert!(run.stopped_at_turn_limit);
    assert!(matches!(run.messages[3], Message::Assistant(_)));
    assert!(matches!(run.messages[4], Message::ToolResult(_)));
    if test_dir.exists() {
        let _ = std::fs::remove_dir_all(&test_dir);
    }
}

#[tokio::test]
async fn test_compaction_retains_proper_tool_group() {
    let model = test_model();

    // Construct a long history that includes:
    // 0: User initial message
    // 1: Assistant message with tool call
    // 2: ToolResult message
    // 3: User message
    // 4: Assistant message with tool call
    // 5: ToolResult message (this is the last message)
    let messages = vec![
        Message::User {
            content: vec![Content::text("initial user message")],
            timestamp: now_ms(),
        },
        Message::Assistant(test_assistant_message(
            vec![Content::ToolCall {
                id: "call_old".into(),
                name: "write".into(),
                arguments: serde_json::json!({}),
            }],
            StopReason::ToolUse,
        )),
        Message::ToolResult(pi_ai::ToolResultMessage {
            tool_call_id: "call_old".into(),
            tool_name: "write".into(),
            content: vec![Content::text("old tool result")],
            is_error: false,
            timestamp: now_ms(),
        }),
        Message::User {
            content: vec![Content::text("follow up user message")],
            timestamp: now_ms(),
        },
        Message::Assistant(test_assistant_message(
            vec![Content::ToolCall {
                id: "call_last".into(),
                name: "write".into(),
                arguments: serde_json::json!({}),
            }],
            StopReason::ToolUse,
        )),
        Message::ToolResult(pi_ai::ToolResultMessage {
            tool_call_id: "call_last".into(),
            tool_name: "write".into(),
            content: vec![Content::text("last tool result")],
            is_error: false,
            timestamp: now_ms(),
        }),
    ];

    let done = AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_message(vec![Content::text("ok")], StopReason::Stop),
    };

    // First attempt fails with context overflow. Compaction runs.
    // Compaction must retain message 0 (initial), and the last tool group (4 and 5).
    // Specifically:
    // Message 0: User ("initial user message")
    // Message 4: Assistant with "call_last"
    // Message 5: ToolResult for "call_last"
    // Other messages (1, 2, 3) must be discarded.
    let factory = Arc::new(FakeProviderFactory::new_sequence(vec![
        vec![Err(pi_ai::Error::ProviderError {
            status: 400,
            body: "context_length_exceeded".into(),
        })],
        vec![Ok(done)],
    ]));
    let cfg = AgentConfig::new(model, "system").with_provider_factory(factory);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let result = pi_agent::run_agent_with_history(&cfg, messages, Some(tx))
        .await
        .unwrap();

    // Verify history compaction reduced the history correctly
    // The final result should contain compacted messages + the new assistant message "ok".
    // Compacted messages should be: [initial_user (0), assistant_last (4), tool_result_last (5)] -> 3 messages.
    // Plus the new assistant "ok" message -> 4 messages total.
    assert_eq!(result.messages.len(), 4);

    match &result.messages[0] {
        Message::User { content, .. } => {
            assert_eq!(content[0].as_text().unwrap(), "initial user message")
        }
        _ => panic!("Expected initial user message at index 0"),
    }
    match &result.messages[1] {
        Message::Assistant(a) => {
            assert!(matches!(&a.content[0], Content::ToolCall { id, .. } if id == "call_last"));
        }
        _ => panic!("Expected last assistant tool call at index 1"),
    }
    match &result.messages[2] {
        Message::ToolResult(tr) => {
            assert_eq!(tr.tool_call_id, "call_last");
        }
        _ => panic!("Expected last tool result at index 2"),
    }
    match &result.messages[3] {
        Message::Assistant(a) => {
            assert_eq!(a.content[0].as_text().unwrap(), "ok");
        }
        _ => panic!("Expected final assistant message at index 3"),
    }

    let mut compacted = false;
    while let Ok(event) = rx.try_recv() {
        compacted |= matches!(event, pi_agent::AgentEvent::AutoCompacted);
    }
    assert!(compacted);
}

#[tokio::test]
async fn test_second_overflow_terminates_without_another_retry() {
    let model = test_model();

    // We start with 3 messages so first compaction is allowed.
    let messages = vec![
        Message::user_text("system-like or user initial query"),
        Message::Assistant(test_assistant_message(
            vec![Content::text("assistant response 1")],
            StopReason::Stop,
        )),
        Message::user_text("user response 2"),
    ];

    // Sequence of provider streams:
    // 1st stream fails with overflow -> triggers compaction -> messages size reduced.
    // 2nd stream fails with overflow -> tries to compact, but it either fails to reduce further or compaction_retried is already true.
    // In any case, it must terminate with error rather than retrying indefinitely.
    let factory = Arc::new(FakeProviderFactory::new_sequence(vec![
        vec![Err(pi_ai::Error::ProviderError {
            status: 400,
            body: "context_length_exceeded".into(),
        })],
        vec![Err(pi_ai::Error::ProviderError {
            status: 400,
            body: "context_length_exceeded".into(),
        })],
    ]));
    let cfg = AgentConfig::new(model, "system").with_provider_factory(factory);

    let result = pi_agent::run_agent_with_history(&cfg, messages, None).await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        err.to_string().contains("context_length_exceeded")
            || err.to_string().contains("provider returned an error")
    );
}

#[tokio::test]
async fn test_streaming_deltas_retry_reset_protocol() {
    let model = test_model();

    let messages = vec![
        Message::user_text("system-like or user initial query"),
        Message::Assistant(test_assistant_message(
            vec![Content::text("assistant response 1")],
            StopReason::Stop,
        )),
        Message::user_text("user response 2"),
    ];

    let done = AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_message(vec![Content::text("ok")], StopReason::Stop),
    };

    // Stream 1 emits normal TextDelta immediately, then fails with context overflow.
    // Stream 2 emits TextDelta then Done.
    // Must observe: TextDelta("overflow delta"), RetryReset, AutoCompacted, TextDelta("good delta").
    let factory = Arc::new(FakeProviderFactory::new_sequence(vec![
        vec![
            Ok(AssistantMessageEvent::TextDelta {
                content_index: 0,
                delta: "overflow delta ".into(),
            }),
            Err(pi_ai::Error::ProviderError {
                status: 400,
                body: "context_length_exceeded".into(),
            }),
        ],
        vec![
            Ok(AssistantMessageEvent::TextDelta {
                content_index: 0,
                delta: "good delta".into(),
            }),
            Ok(done),
        ],
    ]));
    let cfg = AgentConfig::new(model, "system").with_provider_factory(factory);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let result = pi_agent::run_agent_with_history(&cfg, messages, Some(tx))
        .await
        .unwrap();
    assert_eq!(result.messages.len(), 2);

    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    let delta_reset_indices: Vec<(usize, String)> = events
        .iter()
        .enumerate()
        .filter_map(|(i, ev)| match ev {
            pi_agent::AgentEvent::TextDelta { delta } => Some((i, format!("delta:{delta}"))),
            pi_agent::AgentEvent::RetryReset => Some((i, "reset".into())),
            pi_agent::AgentEvent::AutoCompacted => Some((i, "compacted".into())),
            _ => None,
        })
        .collect();

    assert_eq!(
        delta_reset_indices
            .iter()
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>(),
        vec![
            "delta:overflow delta ",
            "reset",
            "compacted",
            "delta:good delta"
        ]
    );
}

#[tokio::test]
async fn test_compacted_messages_provider_validity() {
    use pi_ai::providers;

    let anthropic_model = Model::anthropic_claude_sonnet_4_6();

    let raw_messages = vec![
        Message::user_text("initial instruction"),
        Message::Assistant(test_assistant_message(
            vec![Content::text("reply 1")],
            StopReason::Stop,
        )),
        Message::user_text("followup query"),
    ];

    let done = AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_message(vec![Content::text("ok")], StopReason::Stop),
    };

    let factory = Arc::new(FakeProviderFactory::new_sequence(vec![
        vec![Err(pi_ai::Error::ProviderError {
            status: 400,
            body: "context_length_exceeded".into(),
        })],
        vec![Ok(done)],
    ]));
    let cfg = AgentConfig::new(test_model(), "system").with_provider_factory(factory);

    let result = pi_agent::run_agent_with_history(&cfg, raw_messages, None)
        .await
        .unwrap();

    let ctx = pi_ai::Context {
        system_prompt: Some("system prompt".into()),
        messages: result.messages,
        tools: vec![],
    };
    let opts = pi_ai::StreamOptions::default();

    // Verify Anthropic serializer accepts compacted history
    let anthropic_body = providers::anthropic::build_request_body(&anthropic_model, &ctx, &opts);
    let ant_msgs = anthropic_body["messages"].as_array().unwrap();
    // Compacted history merged user(0) + user(2) into 1 User message, plus final assistant response = 2 total
    assert_eq!(ant_msgs.len(), 2);
    assert_eq!(ant_msgs[0]["role"], "user");
    assert_eq!(ant_msgs[1]["role"], "assistant");
}
