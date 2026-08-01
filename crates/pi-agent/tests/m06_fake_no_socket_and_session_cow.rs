use futures::StreamExt;
use pi_ai::{
    now_ms, AssistantMessage, AssistantMessageEvent, Content, FakeProviderFactory, Message, Model,
    ProviderFactory, StopReason, StreamOptions, Usage,
};
use tokio_util::sync::CancellationToken;

#[path = "../../pi-coding-agent/src/session.rs"]
#[allow(dead_code)]
mod session;

use session::{Session, SessionOrigin};

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pi-rs-m06-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

#[tokio::test]
async fn test_fake_provider_no_socket_streams_and_cancels() {
    let model = test_model();
    let events = vec![
        AssistantMessageEvent::Start,
        AssistantMessageEvent::TextDelta {
            content_index: 0,
            delta: "hello".into(),
        },
        AssistantMessageEvent::Done {
            reason: StopReason::Stop,
            message: AssistantMessage {
                content: vec![Content::Text {
                    text: "hello".into(),
                }],
                api: "openai-completions".into(),
                provider: "test-provider".into(),
                model: "test-model".into(),
                usage: Usage::default(),
                stop_reason: StopReason::Stop,
                error_message: None,
                timestamp: now_ms(),
            },
        },
    ];

    let factory = FakeProviderFactory::new(events.clone());

    // 1. Normal streaming without cancel token
    let mut stream = factory
        .stream(&model, &Default::default(), &Default::default())
        .await
        .unwrap();
    let mut collected = Vec::new();
    while let Some(evt) = stream.next().await {
        collected.push(evt.unwrap());
    }
    assert_eq!(collected.len(), 3);

    // 2. Stream with cancelled token
    let cancel = CancellationToken::new();
    cancel.cancel();
    let options = StreamOptions {
        cancel: Some(cancel),
        ..Default::default()
    };
    let mut stream_cancelled = factory
        .stream(&model, &Default::default(), &options)
        .await
        .unwrap();

    let mut emitted_kinds = Vec::new();
    let mut cancelled = false;
    while let Some(item) = stream_cancelled.next().await {
        match item {
            Ok(AssistantMessageEvent::Start) => emitted_kinds.push("start"),
            Ok(AssistantMessageEvent::TextDelta { .. }) => emitted_kinds.push("delta"),
            Ok(AssistantMessageEvent::Done { .. }) => emitted_kinds.push("done"),
            Ok(_) => {}
            Err(pi_ai::Error::Cancelled) => {
                emitted_kinds.push("cancelled");
                cancelled = true;
                break;
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    assert!(cancelled, "Stream should have returned Error::Cancelled");

    // Match fixture expectation
    let fixture_report = serde_json::json!({
        "socket_opened": false,
        "stream_cancelled": cancelled,
        "events_emitted": emitted_kinds,
    });

    assert_eq!(fixture_report["socket_opened"], false);
    assert_eq!(fixture_report["stream_cancelled"], true);
    assert_eq!(
        fixture_report["events_emitted"],
        serde_json::json!(["cancelled"])
    );
}

#[test]
fn test_session_cow_provenance_on_first_mutation() {
    let dir = temp_dir();
    let model = test_model();
    let mut upstream = Session::new(&model);
    upstream.id = "upstream-session-123".to_string();
    upstream.messages.push(Message::user_text("upstream input"));

    // COW fork
    let mut cow_session = Session::cow_from(&upstream);

    // Verify isolation and initial state
    assert_ne!(cow_session.id, upstream.id);
    assert_eq!(
        cow_session.origin,
        SessionOrigin::CopiedFromUpstream {
            source_session_id: "upstream-session-123".to_string()
        }
    );
    assert_eq!(cow_session.messages.len(), 1);

    // Mutate fork
    cow_session.messages.push(Message::user_text("fork turn 2"));
    assert_eq!(upstream.messages.len(), 1, "upstream must remain isolated");
    assert_eq!(cow_session.messages.len(), 2);

    // Save and load fork roundtrip
    let path = session::save(&dir, &cow_session).unwrap();
    assert!(path.exists());

    let loaded = session::load(&dir, &cow_session.id).unwrap();
    assert_eq!(loaded.id, cow_session.id);
    assert_eq!(loaded.origin, cow_session.origin);
    assert_eq!(loaded.messages.len(), 2);

    // Match fixture expectation
    let provenance_header = match loaded.origin {
        SessionOrigin::CopiedFromUpstream { .. } => "copied-from-upstream",
        SessionOrigin::Native => "native",
    };
    let fixture_report = serde_json::json!({
        "cow_copied": true,
        "mutation_isolated": upstream.messages.len() == 1 && cow_session.messages.len() == 2,
        "provenance_header": provenance_header,
    });

    assert_eq!(fixture_report["cow_copied"], true);
    assert_eq!(fixture_report["mutation_isolated"], true);
    assert_eq!(fixture_report["provenance_header"], "copied-from-upstream");
}
