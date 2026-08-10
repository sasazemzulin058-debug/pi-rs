use std::sync::Arc;

use async_trait::async_trait;
use pi_agent::{AgentConfig, AgentEvent, AgentSession, QueueMode, SessionPhase};
use pi_ai::{
    now_ms, AssistantMessage, AssistantMessageEvent, AssistantMessageEventStream, Content, Context,
    FakeProviderFactory, Message, Model, ProviderFactory, StopReason, StreamOptions, Usage,
};
use tokio::sync::{mpsc, Notify};

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
        usage: Usage::default(),
        stop_reason: StopReason::Stop,
        error_message: None,
        timestamp: now_ms(),
    }
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
async fn test_cancel_before_run_settles_once() {
    let model = test_model();
    let factory = Arc::new(FakeProviderFactory::new(vec![]));
    let cfg = AgentConfig::new(model, "system").with_provider_factory(factory);

    let session = AgentSession::new(cfg, vec![Message::user_text("prompt")]);
    session.cancel();

    let (tx, mut rx) = mpsc::unbounded_channel();
    session.run(Some(tx.clone())).await.unwrap();
    assert!(session.is_settled());

    // Calling run post-settlement or tick post-settlement
    let err = session.run(Some(tx)).await.unwrap_err();
    assert!(err.to_string().contains("already settled"));

    let settlements: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AgentEvent::Settlement { cancelled, .. } => Some(cancelled),
            _ => None,
        })
        .collect();
    assert_eq!(settlements, vec![true]);
}

#[tokio::test]
async fn test_cancel_between_ticks_settles_once() {
    let events = vec![AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_msg("done"),
    }];
    let cfg = AgentConfig::new(test_model(), "system")
        .with_provider_factory(Arc::new(FakeProviderFactory::new(events)));
    let session = AgentSession::new(cfg, vec![]);
    session.set_queue_mode(QueueMode::OneAtATime).unwrap();
    session.queue_followup(Message::user_text("first")).unwrap();
    session
        .queue_followup(Message::user_text("second"))
        .unwrap();

    assert!(session.tick(None).await.unwrap());
    assert_eq!(session.state().input_queue.len(), 1);
    session.cancel();

    let (tx, mut rx) = mpsc::unbounded_channel();
    session.run(Some(tx)).await.unwrap();

    let state = session.state();
    assert!(state.cancelled);
    assert!(state.settled);
    assert_eq!(state.phase, SessionPhase::Settled);
    assert_eq!(state.input_queue.len(), 1);
    let settlements: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AgentEvent::Settlement { cancelled, .. } => Some(cancelled),
            _ => None,
        })
        .collect();
    assert_eq!(settlements, vec![true]);
}

#[tokio::test]
async fn test_natural_settlement_once() {
    let model = test_model();
    let events = vec![AssistantMessageEvent::Done {
        reason: StopReason::Stop,
        message: test_assistant_msg("done"),
    }];
    let factory = Arc::new(FakeProviderFactory::new(events));
    let cfg = AgentConfig::new(model, "system").with_provider_factory(factory);

    let session = AgentSession::new(cfg, vec![]);
    session.queue_followup(Message::user_text("hi")).unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel();
    session.run(Some(tx.clone())).await.unwrap();
    assert!(session.is_settled());

    // Second run fails post-settlement error
    let err = session.run(Some(tx)).await.unwrap_err();
    assert!(err.to_string().contains("already settled"));

    let mut settlements = 0;
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev, AgentEvent::Settlement { .. }) {
            settlements += 1;
        }
    }
    assert_eq!(settlements, 1);
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
async fn test_queue_rejected_after_atomic_settlement() {
    let cfg = AgentConfig::new(test_model(), "system")
        .with_provider_factory(Arc::new(FakeProviderFactory::new(vec![])));
    let session = AgentSession::new(cfg, vec![]);

    session.run(None).await.unwrap();
    assert_eq!(session.state().phase, SessionPhase::Settled);
    assert!(session
        .queue_followup(Message::user_text("late"))
        .unwrap_err()
        .to_string()
        .contains("already settled"));
    assert!(session
        .queue_steering(Message::user_text("late steering"))
        .unwrap_err()
        .to_string()
        .contains("already settled"));
    assert!(session
        .set_queue_mode(QueueMode::OneAtATime)
        .unwrap_err()
        .to_string()
        .contains("already settled"));
    assert!(session.state().input_queue.is_empty());
    assert!(session.state().steering_queue.is_empty());
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

    let (tx, mut rx) = mpsc::unbounded_channel();
    let running = {
        let session = session.clone();
        tokio::spawn(async move { session.run(Some(tx)).await })
    };
    entered.notified().await;
    session.cancel();

    let err = running.await.unwrap().unwrap_err();
    assert!(err.to_string().contains("request cancelled"));
    let state = session.state();
    assert!(state.cancelled);
    assert!(state.settled);
    assert_eq!(state.phase, SessionPhase::Settled);
    let settlements: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AgentEvent::Settlement { cancelled, .. } => Some(cancelled),
            _ => None,
        })
        .collect();
    assert_eq!(settlements, vec![true]);
}
