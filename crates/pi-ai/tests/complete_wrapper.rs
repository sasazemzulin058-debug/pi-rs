use async_trait::async_trait;
use pi_ai::providers::FakeProviderFactory;
use pi_ai::stream::AssistantMessageEventStream;
use pi_ai::types::{
    AssistantMessage, AssistantMessageEvent, Content, Context, Cost, Model, StopReason,
    StreamOptions, Usage,
};
use pi_ai::{complete, Error, ProviderFactory};

fn test_model() -> Model {
    Model {
        id: "test-model".into(),
        name: "Test Model".into(),
        api: "anthropic-messages".into(),
        provider: "anthropic".into(),
        base_url: "https://api.anthropic.com".into(),
        reasoning: false,
        context_window: 200_000,
        max_tokens: 8192,
        pricing: pi_ai::types::ModelPricing::default(),
    }
}

#[tokio::test]
async fn test_complete_success() {
    let expected_msg = AssistantMessage {
        content: vec![Content::Text {
            text: "Hello world".into(),
            text_signature: None,
        }],
        api: "anthropic-messages".into(),
        provider: "anthropic".into(),
        model: "test-model".into(),
        response_model: Some("claude-3-5-sonnet".into()),
        response_id: Some("resp_123".into()),
        diagnostics: Some(serde_json::json!({"system_fingerprint": "fp_456"})),
        raw_stop_reason: Some("end_turn".into()),
        usage: Usage {
            input: 10,
            output: 20,
            cache_read: 5,
            cache_write: 2,
            total_tokens: 37,
            cache_write_1h: None,
            reasoning: None,
            cost: Cost {
                input: 0.01,
                output: 0.02,
                cache_read: 0.005,
                cache_write: 0.002,
                total: 0.037,
            },
        },
        stop_reason: StopReason::Stop,
        error_message: None,
        timestamp: 1700000000000,
    };

    let factory = FakeProviderFactory::new(vec![
        AssistantMessageEvent::Start,
        AssistantMessageEvent::TextDelta {
            content_index: 0,
            delta: "Hello world".into(),
        },
        AssistantMessageEvent::Done {
            reason: StopReason::Stop,
            message: expected_msg.clone(),
        },
    ]);

    let model = test_model();
    let ctx = Context::default();
    let opts = StreamOptions::default();

    let res = factory.complete(&model, &ctx, &opts).await.unwrap();
    assert_eq!(
        serde_json::to_value(&res).unwrap(),
        serde_json::to_value(&expected_msg).unwrap()
    );
}

#[tokio::test]
async fn test_complete_returns_terminal_error_message() {
    let err_msg = AssistantMessage {
        content: vec![Content::Text {
            text: "partial response before rate limit".into(),
            text_signature: Some("sig_error_123".into()),
        }],
        api: "anthropic-messages".into(),
        provider: "anthropic".into(),
        model: "test-model".into(),
        response_model: Some("claude-3-5-sonnet".into()),
        response_id: Some("resp_err_789".into()),
        diagnostics: Some(serde_json::json!({"system_fingerprint": "fp_err"})),
        raw_stop_reason: Some("rate_limit".into()),
        usage: Usage {
            input: 5,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total_tokens: 5,
            cache_write_1h: None,
            reasoning: None,
            cost: Cost {
                input: 0.005,
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
                total: 0.005,
            },
        },
        stop_reason: StopReason::Error,
        error_message: Some("Rate limit exceeded".into()),
        timestamp: 1700000001000,
    };

    let factory = FakeProviderFactory::new(vec![AssistantMessageEvent::Error {
        reason: StopReason::Error,
        error: err_msg.clone(),
    }]);

    let model = test_model();
    let ctx = Context::default();
    let opts = StreamOptions::default();

    let res = factory.complete(&model, &ctx, &opts).await.unwrap();
    assert_eq!(
        serde_json::to_value(&res).unwrap(),
        serde_json::to_value(&err_msg).unwrap()
    );
}

struct StreamErrorFactory;

#[async_trait]
impl ProviderFactory for StreamErrorFactory {
    async fn stream(
        &self,
        _model: &Model,
        _context: &Context,
        _options: &StreamOptions,
    ) -> pi_ai::Result<AssistantMessageEventStream> {
        Ok(Box::pin(async_stream::stream! {
            yield Err(Error::ProviderError {
                status: 429,
                body: "rate limit from stream".into(),
            });
        }))
    }
}

#[tokio::test]
async fn test_complete_propagates_stream_item_error() {
    let factory = StreamErrorFactory;
    let model = test_model();
    let ctx = Context::default();
    let opts = StreamOptions::default();

    let err = factory.complete(&model, &ctx, &opts).await.unwrap_err();
    match err {
        Error::ProviderError { status, body } => {
            assert_eq!(status, 429);
            assert_eq!(body, "rate limit from stream");
        }
        other => panic!("Expected ProviderError status 429, got {:?}", other),
    }
}

#[tokio::test]
async fn test_complete_missing_done_event() {
    let factory = FakeProviderFactory::new(vec![]);

    let model = test_model();
    let ctx = Context::default();
    let opts = StreamOptions::default();

    let err = factory.complete(&model, &ctx, &opts).await.unwrap_err();
    match err {
        Error::InvalidResponse(msg) => assert!(msg.contains("Stream ended without Done event")),
        other => panic!("Expected InvalidResponse, got {:?}", other),
    }
}

#[tokio::test]
async fn test_complete_public_wrapper_dispatch() {
    // Testing free function complete() compile dispatch check
    let model = Model {
        id: "unknown".into(),
        name: "Unknown".into(),
        api: "invalid-provider".into(),
        provider: "invalid".into(),
        base_url: "http://localhost".into(),
        reasoning: false,
        context_window: 1000,
        max_tokens: 1000,
        pricing: pi_ai::types::ModelPricing::default(),
    };
    let ctx = Context::default();
    let opts = StreamOptions::default();

    let err = complete(&model, &ctx, &opts).await.unwrap_err();
    match err {
        Error::UnsupportedProvider(p) => assert_eq!(p, "invalid-provider"),
        other => panic!("Expected UnsupportedProvider, got {:?}", other),
    }
}
