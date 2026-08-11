use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use pi_agent::{
    AgentConfig, AgentEvent, AgentSession, AgentTool, AgentToolResult, QueueMode, SessionPhase,
};
use pi_ai::{
    now_ms, AssistantMessage, AssistantMessageEvent, AssistantMessageEventStream, Content, Context,
    FakeProviderFactory, Message, Model, ProviderFactory, StopReason, StreamOptions, Usage,
};
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

struct SteeringTool {
    tool_started: Arc<Notify>,
    tool_release: Arc<Notify>,
}

#[async_trait]
impl AgentTool for SteeringTool {
    fn name(&self) -> &str {
        "test_tool"
    }

    fn description(&self) -> &str {
        "test tool description"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _id: &str,
        _args: serde_json::Value,
    ) -> Result<AgentToolResult, String> {
        self.tool_started.notify_one();
        self.tool_release.notified().await;
        Ok(AgentToolResult::text("tool result"))
    }
}

struct SteeringRecordedCall {
    messages: Vec<Message>,
}

struct SteeringTestProviderFactory {
    calls: Arc<Mutex<Vec<SteeringRecordedCall>>>,
}

#[async_trait]
impl ProviderFactory for SteeringTestProviderFactory {
    async fn stream(
        &self,
        _model: &Model,
        context: &Context,
        _options: &StreamOptions,
    ) -> pi_ai::Result<AssistantMessageEventStream> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(SteeringRecordedCall {
            messages: context.messages.clone(),
        });
        let call_count = calls.len();
        drop(calls);

        let msg = if call_count == 1 {
            AssistantMessage {
                content: vec![Content::ToolCall {
                    id: "call_1".to_string(),
                    name: "test_tool".to_string(),
                    arguments: serde_json::json!({}),
                    thought_signature: None,
                }],
                api: "openai-completions".to_string(),
                provider: "test-provider".to_string(),
                model: "test-model".to_string(),
                response_model: None,
                response_id: None,
                diagnostics: None,
                raw_stop_reason: None,
                usage: Usage::default(),
                stop_reason: StopReason::ToolUse,
                error_message: None,
                timestamp: now_ms(),
            }
        } else {
            test_assistant_msg("final response")
        };

        let stop = msg.stop_reason;

        Ok(Box::pin(async_stream::stream! {
            yield Ok(AssistantMessageEvent::Done {
                reason: stop,
                message: msg,
            });
        }))
    }
}

struct BlockingProviderFactory {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl ProviderFactory for BlockingProviderFactory {
    async fn stream(
        &self,
        _model: &Model,
        _context: &Context,
        _options: &StreamOptions,
    ) -> pi_ai::Result<AssistantMessageEventStream> {
        let entered = self.entered.clone();
        let release = self.release.clone();
        let message = test_assistant_msg("done");
        Ok(Box::pin(async_stream::stream! {
            entered.notify_one();
            release.notified().await;
            yield Ok(AssistantMessageEvent::Done {
                reason: StopReason::Stop,
                message,
            });
        }))
    }
}

struct CancelAwareProviderFactory {
    entered: Arc<Notify>,
}

#[async_trait]
impl ProviderFactory for CancelAwareProviderFactory {
    async fn stream(
        &self,
        _model: &Model,
        _context: &Context,
        options: &StreamOptions,
    ) -> pi_ai::Result<AssistantMessageEventStream> {
        let entered = self.entered.clone();
        let cancel = options.cancel.clone().expect("session cancellation token");
        Ok(Box::pin(async_stream::stream! {
            entered.notify_one();
            cancel.cancelled().await;
            yield Err(pi_ai::Error::Cancelled);
        }))
    }
}

struct SequentialProviderFactory {
    streams: Arc<Mutex<Vec<Vec<AssistantMessageEvent>>>>,
}

#[async_trait]
impl ProviderFactory for SequentialProviderFactory {
    async fn stream(
        &self,
        _model: &Model,
        _context: &Context,
        _options: &StreamOptions,
    ) -> pi_ai::Result<AssistantMessageEventStream> {
        let events = {
            let mut streams = self.streams.lock().unwrap();
            if streams.is_empty() {
                vec![]
            } else {
                streams.remove(0)
            }
        };
        Ok(Box::pin(async_stream::stream! {
            for ev in events {
                yield Ok(ev);
            }
        }))
    }
}

struct NonToolSteeringProviderFactory {
    entered: Arc<Notify>,
    release: Arc<Notify>,
    calls: Arc<Mutex<Vec<Context>>>,
}

#[async_trait]
impl ProviderFactory for NonToolSteeringProviderFactory {
    async fn stream(
        &self,
        _model: &Model,
        context: &Context,
        _options: &StreamOptions,
    ) -> pi_ai::Result<AssistantMessageEventStream> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(context.clone());
        let count = calls.len();
        drop(calls);

        let entered = self.entered.clone();
        let release = self.release.clone();

        let msg = if count == 1 {
            test_assistant_msg("non-tool response 1")
        } else {
            test_assistant_msg("steered response 2")
        };

        Ok(Box::pin(async_stream::stream! {
            if count == 1 {
                entered.notify_one();
                release.notified().await;
            }
            yield Ok(AssistantMessageEvent::Done {
                reason: StopReason::Stop,
                message: msg,
            });
        }))
    }
}

struct ResumableCancelProviderFactory {
    entered: Arc<Notify>,
    run_count: Arc<Mutex<usize>>,
    second_token_was_cancelled: Arc<Mutex<Option<bool>>>,
}

#[async_trait]
impl ProviderFactory for ResumableCancelProviderFactory {
    async fn stream(
        &self,
        _model: &Model,
        _context: &Context,
        options: &StreamOptions,
    ) -> pi_ai::Result<AssistantMessageEventStream> {
        let mut count = self.run_count.lock().unwrap();
        *count += 1;
        let current_run = *count;
        drop(count);

        let cancel = options.cancel.clone().expect("session cancellation token");
        if current_run == 1 {
            let entered = self.entered.clone();
            Ok(Box::pin(async_stream::stream! {
                entered.notify_one();
                cancel.cancelled().await;
                yield Err(pi_ai::Error::Cancelled);
            }))
        } else {
            *self.second_token_was_cancelled.lock().unwrap() = Some(cancel.is_cancelled());
            let msg = test_assistant_msg("success after abort");
            Ok(Box::pin(async_stream::stream! {
                yield Ok(AssistantMessageEvent::Done {
                    reason: StopReason::Stop,
                    message: msg,
                });
            }))
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

fn test_assistant_msg(text: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![Content::text(text)],
        api: "openai-completions".to_string(),
        provider: "test-provider".to_string(),
        model: "test-model".to_string(),
        response_model: None,
        response_id: None,
        diagnostics: None,
        raw_stop_reason: None,
        usage: Usage::default(),
        stop_reason: StopReason::Stop,
        error_message: None,
        timestamp: now_ms(),
    }
}

#[tokio::test]
async fn active_run_steering() {
    let tool_started = Arc::new(Notify::new());
    let tool_release = Arc::new(Notify::new());
    let tool = Arc::new(SteeringTool {
        tool_started: tool_started.clone(),
        tool_release: tool_release.clone(),
    });

    let calls = Arc::new(Mutex::new(Vec::new()));
    let factory = Arc::new(SteeringTestProviderFactory {
        calls: calls.clone(),
    });

    let cfg = AgentConfig::new(test_model(), "system")
        .with_tools(vec![tool])
        .with_provider_factory(factory);

    let session = AgentSession::new(cfg, vec![]);
    session
        .queue_followup(Message::user_text("initial prompt"))
        .unwrap();

    let session_clone = session.clone();
    let run_handle = tokio::spawn(async move { session_clone.run(None).await });

    tool_started.notified().await;
    session
        .queue_steering(Message::user_text("steer msg"))
        .unwrap();
    tool_release.notify_one();

    run_handle.await.unwrap().unwrap();

    let recorded = calls.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    let second_call_messages = &recorded[1].messages;
    assert!(second_call_messages.iter().any(|m| match m {
        Message::User { content, .. } => content.iter().any(|c| match c {
            Content::Text { text, .. } => text == "steer msg",
            _ => false,
        }),
        _ => false,
    }));
}

#[tokio::test]
async fn independent_queue_modes() {
    let session = AgentSession::new(AgentConfig::new(test_model(), "sys"), vec![]);
    session.set_steering_mode(QueueMode::All).unwrap();
    session.set_followup_mode(QueueMode::OneAtATime).unwrap();

    session.queue_steering(Message::user_text("s1")).unwrap();
    session.queue_steering(Message::user_text("s2")).unwrap();
    session.queue_followup(Message::user_text("f1")).unwrap();
    session.queue_followup(Message::user_text("f2")).unwrap();

    let mut st = session.state();
    let steered = st.take_steering();
    assert_eq!(steered.len(), 2);

    let followed = st.take_followups();
    assert_eq!(followed.len(), 1);
    assert_eq!(st.input_queue.len(), 1);
}

#[tokio::test]
async fn queue_modes_default_one_at_a_time() {
    let session = AgentSession::new(AgentConfig::new(test_model(), "sys"), vec![]);
    let st = session.state();
    assert_eq!(st.steering_mode, QueueMode::OneAtATime);
    assert_eq!(st.followup_mode, QueueMode::OneAtATime);
}

#[tokio::test]
async fn reusable_after_normal_run() {
    let events1 = vec![AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_msg("reply1"),
    }];
    let events2 = vec![AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_msg("reply2"),
    }];

    let factory = Arc::new(SequentialProviderFactory {
        streams: Arc::new(Mutex::new(vec![events1, events2])),
    });
    let cfg = AgentConfig::new(test_model(), "sys").with_provider_factory(factory);

    let session = AgentSession::new(cfg, vec![]);
    session.queue_followup(Message::user_text("p1")).unwrap();
    session.run(None).await.unwrap();

    assert_eq!(session.state().phase, SessionPhase::Idle);

    session.queue_followup(Message::user_text("p2")).unwrap();
    session.run(None).await.unwrap();

    assert_eq!(session.state().phase, SessionPhase::Idle);
    assert_eq!(session.state().messages.len(), 4);
    assert!(session.state().messages.iter().any(|m| match m {
        Message::Assistant(a) => a.content.iter().any(|c| match c {
            Content::Text { text, .. } => text == "reply2",
            _ => false,
        }),
        _ => false,
    }));
}

#[tokio::test]
async fn reusable_after_abort() {
    let entered = Arc::new(Notify::new());
    let run_count = Arc::new(Mutex::new(0));
    let second_token_was_cancelled = Arc::new(Mutex::new(None));
    let factory = Arc::new(ResumableCancelProviderFactory {
        entered: entered.clone(),
        run_count: run_count.clone(),
        second_token_was_cancelled: second_token_was_cancelled.clone(),
    });
    let cfg = AgentConfig::new(test_model(), "system").with_provider_factory(factory);
    let session = AgentSession::new(cfg, vec![]);
    session
        .queue_followup(Message::user_text("cancel me"))
        .unwrap();

    let session_clone = session.clone();
    let running = tokio::spawn(async move { session_clone.run(None).await });

    timeout(TEST_TIMEOUT, entered.notified())
        .await
        .expect("cancel-aware provider did not start");
    session.cancel();

    let run_res = timeout(TEST_TIMEOUT, running)
        .await
        .expect("cancelled run did not finish")
        .expect("join failed");
    let err = run_res.expect_err("cancelled run should return error");
    assert!(err.to_string().contains("request cancelled"));

    assert_eq!(session.state().phase, SessionPhase::Idle);

    session.queue_followup(Message::user_text("run 2")).unwrap();
    timeout(TEST_TIMEOUT, session.run(None))
        .await
        .expect("reused session run did not finish")
        .expect("run 2 failed");

    assert_eq!(session.state().phase, SessionPhase::Idle);
    assert_eq!(*run_count.lock().unwrap(), 2);
    assert_eq!(
        *second_token_was_cancelled.lock().unwrap(),
        Some(false),
        "second run received cancelled token"
    );
    assert!(session.state().messages.iter().any(|m| match m {
        Message::Assistant(a) => a.content.iter().any(|c| match c {
            Content::Text { text, .. } => text == "success after abort",
            _ => false,
        }),
        _ => false,
    }));
}

#[tokio::test]
async fn idle_cancel_is_noop() {
    let events = vec![AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_msg("ok"),
    }];
    let factory = Arc::new(FakeProviderFactory::new(events));
    let cfg = AgentConfig::new(test_model(), "sys").with_provider_factory(factory);
    let session = AgentSession::new(cfg, vec![]);

    session.cancel();
    assert_eq!(session.state().phase, SessionPhase::Idle);

    session.queue_followup(Message::user_text("hello")).unwrap();
    session.run(None).await.unwrap();
    assert_eq!(session.state().phase, SessionPhase::Idle);
    assert_eq!(session.state().messages.len(), 2);
}

#[tokio::test]
async fn test_queue_order_and_one_at_a_time() {
    let model = test_model();
    let events = vec![AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_msg("reply"),
    }];
    let factory = Arc::new(FakeProviderFactory::new(events));
    let cfg = AgentConfig::new(model, "system").with_provider_factory(factory);

    let session = AgentSession::new(cfg, vec![Message::user_text("prompt")]);
    session.set_queue_mode(QueueMode::OneAtATime).unwrap();
    session.queue_followup(Message::user_text("f1")).unwrap();
    session.queue_followup(Message::user_text("f2")).unwrap();
    session.queue_steering(Message::user_text("s1")).unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel();
    let ran = session.tick(Some(tx)).await.unwrap();
    assert!(ran);

    // Steering queue prioritized: s1 consumed first, f1 and f2 remain in input queue
    assert_eq!(session.state().input_queue.len(), 2);
    assert_eq!(session.state().steering_queue.len(), 0);

    let mut received_events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        received_events.push(ev);
    }
    assert!(received_events.iter().any(|e| matches!(
        e,
        AgentEvent::PhaseChange {
            phase: SessionPhase::Steering
        }
    )));
}

#[tokio::test]
async fn active_run_non_tool_steering() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let factory = Arc::new(NonToolSteeringProviderFactory {
        entered: entered.clone(),
        release: release.clone(),
        calls: calls.clone(),
    });

    let cfg = AgentConfig::new(test_model(), "sys").with_provider_factory(factory);
    let session = AgentSession::new(cfg, vec![]);
    session.queue_followup(Message::user_text("start")).unwrap();

    let session_clone = session.clone();
    let running = tokio::spawn(async move { session_clone.run(None).await });

    timeout(TEST_TIMEOUT, entered.notified())
        .await
        .expect("first non-tool provider did not start");

    session
        .queue_steering(Message::user_text("steer non tool"))
        .unwrap();

    release.notify_one();

    timeout(TEST_TIMEOUT, running)
        .await
        .expect("non-tool steering run did not finish")
        .expect("join failed")
        .expect("run failed");

    assert_eq!(session.state().phase, SessionPhase::Idle);
    assert!(session.state().steering_queue.is_empty());

    let recorded = calls.lock().unwrap();
    assert_eq!(recorded.len(), 2, "provider should be called twice");

    let second_call_messages = &recorded[1].messages;
    let steering_count_in_context = second_call_messages
        .iter()
        .filter(|m| match m {
            Message::User { content, .. } => content.iter().any(|c| match c {
                Content::Text { text, .. } => text == "steer non tool",
                _ => false,
            }),
            _ => false,
        })
        .count();
    assert_eq!(
        steering_count_in_context, 1,
        "second provider context should include steering message exactly once"
    );

    let steering_count_in_session = session
        .state()
        .messages
        .iter()
        .filter(|m| match m {
            Message::User { content, .. } => content.iter().any(|c| match c {
                Content::Text { text, .. } => text == "steer non tool",
                _ => false,
            }),
            _ => false,
        })
        .count();
    assert_eq!(
        steering_count_in_session, 1,
        "session messages should contain steering message exactly once"
    );
}

#[tokio::test]
async fn test_rejects_overlapping_run_and_tick() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let factory = Arc::new(BlockingProviderFactory {
        entered: entered.clone(),
        release: release.clone(),
    });
    let cfg = AgentConfig::new(test_model(), "system").with_provider_factory(factory);
    let session = AgentSession::new(cfg, vec![]);
    session.queue_followup(Message::user_text("first")).unwrap();

    let running = {
        let session = session.clone();
        tokio::spawn(async move { session.run(None).await })
    };
    entered.notified().await;

    let err = session.tick(None).await.unwrap_err();
    assert!(err.to_string().contains("run already active"));
    release.notify_one();
    running.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_cancel_interrupts_active_provider() {
    let entered = Arc::new(Notify::new());
    let factory = Arc::new(CancelAwareProviderFactory {
        entered: entered.clone(),
    });
    let cfg = AgentConfig::new(test_model(), "system").with_provider_factory(factory);
    let session = AgentSession::new(cfg, vec![]);
    session
        .queue_followup(Message::user_text("cancel me"))
        .unwrap();

    let session_clone = session.clone();
    let running = tokio::spawn(async move { session_clone.run(None).await });
    entered.notified().await;
    session.cancel();

    let err = running.await.unwrap().unwrap_err();
    assert!(err.to_string().contains("request cancelled"));
    assert_eq!(session.state().phase, SessionPhase::Idle);
}

#[tokio::test]
async fn test_thinking_level_mutation_shared_and_snapshotted() {
    struct OptionsCapturingFactory {
        options: Arc<Mutex<Vec<StreamOptions>>>,
    }

    #[async_trait]
    impl ProviderFactory for OptionsCapturingFactory {
        async fn stream(
            &self,
            _model: &Model,
            _context: &Context,
            options: &StreamOptions,
        ) -> pi_ai::Result<AssistantMessageEventStream> {
            self.options.lock().unwrap().push(options.clone());
            let msg = AssistantMessage {
                content: vec![Content::Text {
                    text: "done".into(),
                    text_signature: None,
                }],
                api: "openai-completions".to_string(),
                provider: "test-provider".to_string(),
                model: "test-model".to_string(),
                response_model: None,
                response_id: None,
                diagnostics: None,
                raw_stop_reason: None,
                error_message: None,
                timestamp: now_ms(),
                usage: Usage::default(),
                stop_reason: StopReason::Stop,
            };
            Ok(Box::pin(async_stream::stream! {
                yield Ok(AssistantMessageEvent::Start);
                yield Ok(AssistantMessageEvent::Done {
                    reason: StopReason::Stop,
                    message: msg,
                });
            }))
        }
    }

    let captured = Arc::new(Mutex::new(Vec::new()));
    let factory = Arc::new(OptionsCapturingFactory {
        options: captured.clone(),
    });
    let cfg = AgentConfig::new(test_model(), "system").with_provider_factory(factory);
    let session = AgentSession::new(cfg, vec![]);
    let session_clone = session.clone();

    assert_eq!(session.thinking_level(), pi_ai::ThinkingLevel::Off);

    session.set_thinking_level(pi_ai::ThinkingLevel::High);
    assert_eq!(session_clone.thinking_level(), pi_ai::ThinkingLevel::High);

    session.queue_followup(Message::user_text("run 1")).unwrap();
    session.run(None).await.unwrap();

    let opts1 = captured.lock().unwrap().clone();
    assert_eq!(opts1.len(), 1);
    assert_eq!(opts1[0].reasoning, Some(pi_ai::ThinkingLevel::High));

    session.set_thinking_level(pi_ai::ThinkingLevel::Low);
    session.queue_followup(Message::user_text("run 2")).unwrap();
    session.run(None).await.unwrap();

    let opts2 = captured.lock().unwrap().clone();
    assert_eq!(opts2.len(), 2);
    assert_eq!(opts2[1].reasoning, Some(pi_ai::ThinkingLevel::Low));
}
