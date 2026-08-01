//! Local loopback integration test for OpenAI SSE streaming in pi-ai.
//! Tests response frames split across arbitrary byte boundaries, including
//! text and tool call delta fragmentation.

use std::io::Write;
use std::net::TcpListener;
use std::thread;

use futures::StreamExt;
use pi_ai::providers::openai::OpenAiProvider;
use pi_ai::providers::Provider;
use pi_ai::{
    AssistantMessageEvent, Content, Context, Message, Model, StopReason, StreamOptions,
};
use serde_json::json;

#[tokio::test]
async fn test_openai_sse_fragmented_loopback() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();

        let response_headers = "HTTP/1.1 200 OK\r\n\
Content-Type: text/event-stream\r\n\
Cache-Control: no-cache\r\n\
Connection: keep-alive\r\n\r\n";

        stream.write_all(response_headers.as_bytes()).unwrap();
        stream.flush().unwrap();

        let chunk1 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "delta": {
                    "role": "assistant",
                    "content": "Hello "
                }
            }]
        }).to_string();

        let chunk2 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "delta": {
                    "content": "world!"
                }
            }]
        }).to_string();

        let chunk3 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc123",
                        "function": {
                            "name": "calculator",
                            "arguments": "{\"expr\": "
                        }
                    }]
                }
            }]
        }).to_string();

        let chunk4 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "arguments": "\"2 + 2\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }).to_string();

        let chunk5 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        }).to_string();

        let sse_body = format!(
            "data: {chunk1}\n\n\
data: {chunk2}\n\n\
data: {chunk3}\n\n\
data: {chunk4}\n\n\
data: {chunk5}\n\n\
data: [DONE]\n\n"
        );

        // Split raw SSE body byte stream arbitrarily across multiple writes to simulate TCP fragmentation
        let bytes = sse_body.as_bytes();
        let chunk_sizes = [5, 12, 3, 25, 7, 50, 15, 8, 40, 10, bytes.len()];
        let mut offset = 0;

        for &size in &chunk_sizes {
            if offset >= bytes.len() {
                break;
            }
            let end = (offset + size).min(bytes.len());
            stream.write_all(&bytes[offset..end]).unwrap();
            stream.flush().unwrap();
            thread::sleep(std::time::Duration::from_millis(5));
            offset = end;
        }
    });

    let base_url = format!("http://127.0.0.1:{}", port);
    let model = Model {
        id: "gpt-4o".into(),
        name: "GPT-4o".into(),
        provider: "openai".into(),
        api: "openai-completions".into(),
        base_url: base_url.clone(),
        reasoning: false,
        context_window: 128000,
        max_tokens: 4096,
        pricing: Default::default(),
    };

    let context = Context {
        system_prompt: None,
        messages: vec![Message::user_text("test")],
        tools: vec![],
    };

    let options = StreamOptions {
        api_key: Some("test-key".into()),
        base_url: Some(base_url),
        ..Default::default()
    };

    let provider = OpenAiProvider::new();
    let mut stream = provider
        .stream(&model, &context, &options)
        .await
        .expect("Stream initialization failed");

    let mut events = Vec::new();
    while let Some(ev_res) = stream.next().await {
        let ev = ev_res.expect("Stream event failed");
        events.push(ev);
    }

    server_handle.join().unwrap();

    // Verify Start event
    assert!(matches!(events.first(), Some(AssistantMessageEvent::Start)));

    // Verify Done event and reconstructed message
    let last = events.last().expect("Events should not be empty");
    if let AssistantMessageEvent::Done { reason, message } = last {
        assert_eq!(*reason, StopReason::ToolUse);
        assert_eq!(message.model, "gpt-4o");
        assert_eq!(message.usage.input, 10);
        assert_eq!(message.usage.output, 20);
        assert_eq!(message.usage.total_tokens, 30);

        assert_eq!(message.content.len(), 2);
        match &message.content[0] {
            Content::Text { text } => assert_eq!(text, "Hello world!"),
            _ => panic!("Expected text content at index 0"),
        }
        match &message.content[1] {
            Content::ToolCall {
                id,
                name,
                arguments,
            } => {
                assert_eq!(id, "call_abc123");
                assert_eq!(name, "calculator");
                assert_eq!(arguments, &json!({"expr": "2 + 2"}));
            }
            _ => panic!("Expected tool call content at index 1"),
        }
    } else {
        panic!("Last event was not Done");
    }
}
