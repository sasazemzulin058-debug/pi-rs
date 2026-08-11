//! Local loopback integration test for Anthropic SSE streaming in pi-ai.
//! Tests response frames split across arbitrary byte boundaries, including
//! text, thinking, and tool call delta fragmentation.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use futures::StreamExt;
use pi_ai::providers::anthropic::AnthropicProvider;
use pi_ai::providers::Provider;
use pi_ai::{AssistantMessageEvent, Content, Context, Message, Model, StopReason, StreamOptions};
use serde_json::json;

async fn stream_error_for_body(body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        stream.write_all(body.as_bytes()).unwrap();
        stream.flush().unwrap();
    });

    let base_url = format!("http://127.0.0.1:{port}");
    let model = Model {
        id: "claude-3-5-sonnet-20241022".into(),
        name: "Claude 3.5 Sonnet".into(),
        provider: "anthropic".into(),
        api: "anthropic-messages".into(),
        base_url: base_url.clone(),
        reasoning: false,
        context_window: 200000,
        max_tokens: 8192,
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
    let mut events = AnthropicProvider::new()
        .stream(&model, &context, &options)
        .await
        .unwrap();
    let mut error = None;
    while let Some(event) = events.next().await {
        if let Err(err) = event {
            error = Some(err.to_string());
            break;
        }
    }
    server_handle.join().unwrap();
    error.expect("stream should fail closed")
}

#[tokio::test]
async fn test_anthropic_sse_fragmented_loopback() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();

        let mut req_buf = Vec::new();
        let mut buf = [0u8; 1024];
        let mut header_len = 0;
        loop {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            req_buf.extend_from_slice(&buf[..n]);
            if let Some(pos) = req_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                header_len = pos + 4;
                break;
            }
        }

        let headers_str = String::from_utf8_lossy(&req_buf[..header_len]);
        let mut content_length = 0;
        for line in headers_str.lines() {
            if line.to_lowercase().starts_with("content-length:") {
                if let Some(val) = line.split(':').nth(1) {
                    content_length = val.trim().parse::<usize>().unwrap_or(0);
                }
            }
        }

        let mut body_read = req_buf.len() - header_len;
        while body_read < content_length {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            body_read += n;
        }

        let response_headers = "HTTP/1.1 200 OK\r\n\
Content-Type: text/event-stream\r\n\
Cache-Control: no-cache\r\n\
Connection: keep-alive\r\n\r\n";

        stream.write_all(response_headers.as_bytes()).unwrap();
        stream.flush().unwrap();

        let msg_start = json!({
            "type": "message_start",
            "message": {
                "id": "msg_123",
                "model": "claude-3-5-sonnet-20241022",
                "usage": {
                    "input_tokens": 15,
                    "cache_read_input_tokens": 5,
                    "cache_creation_input_tokens": 2
                }
            }
        })
        .to_string();

        let block0_start = json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "thinking" }
        })
        .to_string();

        let block0_delta = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "thinking_delta", "thinking": "Let me calculate " }
        })
        .to_string();

        let block0_stop = json!({
            "type": "content_block_stop",
            "index": 0
        })
        .to_string();

        let block1_start = json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": { "type": "text" }
        })
        .to_string();

        let block1_delta = json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": { "type": "text_delta", "text": "Result is " }
        })
        .to_string();

        let block1_stop = json!({
            "type": "content_block_stop",
            "index": 1
        })
        .to_string();

        let block2_start = json!({
            "type": "content_block_start",
            "index": 2,
            "content_block": { "type": "tool_use", "id": "call_999", "name": "calculator" }
        })
        .to_string();

        let block2_delta = json!({
            "type": "content_block_delta",
            "index": 2,
            "delta": { "type": "input_json_delta", "partial_json": "{\"x\": 42}" }
        })
        .to_string();

        let block2_stop = json!({
            "type": "content_block_stop",
            "index": 2
        })
        .to_string();

        let msg_delta = json!({
            "type": "message_delta",
            "delta": { "stop_reason": "tool_use" },
            "usage": { "output_tokens": 25 }
        })
        .to_string();

        let msg_stop = json!({
            "type": "message_stop"
        })
        .to_string();

        let sse_body = format!(
            "event: message_start\ndata: {msg_start}\n\n\
event: content_block_start\ndata: {block0_start}\n\n\
event: content_block_delta\ndata: {block0_delta}\n\n\
event: content_block_stop\ndata: {block0_stop}\n\n\
event: content_block_start\ndata: {block1_start}\n\n\
event: content_block_delta\ndata: {block1_delta}\n\n\
event: content_block_stop\ndata: {block1_stop}\n\n\
event: content_block_start\ndata: {block2_start}\n\n\
event: content_block_delta\ndata: {block2_delta}\n\n\
event: content_block_stop\ndata: {block2_stop}\n\n\
event: message_delta\ndata: {msg_delta}\n\n\
event: message_stop\ndata: {msg_stop}\n\n"
        );

        let bytes = sse_body.as_bytes();
        let chunk_sizes = [8, 15, 4, 30, 10, 45, 12, 6, 35, 9, bytes.len()];
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
        id: "claude-3-5-sonnet-20241022".into(),
        name: "Claude 3.5 Sonnet".into(),
        provider: "anthropic".into(),
        api: "anthropic-messages".into(),
        base_url: base_url.clone(),
        reasoning: true,
        context_window: 200000,
        max_tokens: 8192,
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

    let provider = AnthropicProvider::new();
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

    assert!(matches!(events.first(), Some(AssistantMessageEvent::Start)));

    let last = events.last().expect("Events should not be empty");
    if let AssistantMessageEvent::Done { reason, message } = last {
        assert_eq!(*reason, StopReason::ToolUse);
        assert_eq!(message.model, "claude-3-5-sonnet-20241022");
        assert_eq!(message.usage.input, 15);
        assert_eq!(message.usage.cache_read, 5);
        assert_eq!(message.usage.cache_write, 2);
        assert_eq!(message.usage.output, 25);

        assert_eq!(message.content.len(), 3);
        match &message.content[0] {
            Content::Thinking { thinking, .. } => assert_eq!(thinking, "Let me calculate "),
            _ => panic!("Expected thinking content at index 0"),
        }
        match &message.content[1] {
            Content::Text { text, .. } => assert_eq!(text, "Result is "),
            _ => panic!("Expected text content at index 1"),
        }
        match &message.content[2] {
            Content::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "call_999");
                assert_eq!(name, "calculator");
                assert_eq!(arguments, &json!({"x": 42}));
            }
            _ => panic!("Expected tool call content at index 2"),
        }
    } else {
        panic!("Last event was not Done");
    }
}

#[tokio::test]
async fn test_anthropic_sse_malformed_json_fails_closed() {
    let error =
        stream_error_for_body("event: message_start\ndata: {invalid json}\n\n".into()).await;
    assert!(
        error.contains("malformed sse data"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn test_anthropic_sse_malformed_tool_args_fails_closed() {
    let block_start = json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": { "type": "tool_use", "id": "call_1", "name": "calc" }
    })
    .to_string();
    let block_delta = json!({
        "type": "content_block_delta",
        "index": 0,
        "delta": { "type": "input_json_delta", "partial_json": "{bad json" }
    })
    .to_string();
    let block_stop = json!({
        "type": "content_block_stop",
        "index": 0
    })
    .to_string();

    let body = format!(
        "event: content_block_start\ndata: {block_start}\n\n\
event: content_block_delta\ndata: {block_delta}\n\n\
event: content_block_stop\ndata: {block_stop}\n\n"
    );
    let error = stream_error_for_body(body).await;
    assert!(
        error.contains("malformed tool call arguments"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn test_anthropic_sse_eof_before_message_stop_fails_closed() {
    let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n";
    let error = stream_error_for_body(body.into()).await;
    assert!(
        error.contains("ended before message_stop"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn test_anthropic_sse_eof_during_tool_args_fails_closed() {
    let block_start = json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": { "type": "tool_use", "id": "call_1", "name": "calc" }
    });
    let block_delta = json!({
        "type": "content_block_delta",
        "index": 0,
        "delta": { "type": "input_json_delta", "partial_json": "{\\\"x\\\":" }
    });
    let body = format!(
        "event: content_block_start\\ndata: {}\\n\\nevent: content_block_delta\\ndata: {}\\n\\n",
        block_start, block_delta
    );
    let error = stream_error_for_body(body).await;
    assert!(
        error.contains("before message_stop") || error.contains("before content block closure"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn test_anthropic_sse_unknown_block_delta_fails_closed() {
    let block_start = json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": { "type": "text" }
    });
    let block_delta = json!({
        "type": "content_block_delta",
        "index": 0,
        "delta": { "type": "future_delta", "value": "x" }
    });
    let body = format!(
        "event: content_block_start\ndata: {}\n\nevent: content_block_delta\ndata: {}\n\n",
        block_start, block_delta
    );
    let error = stream_error_for_body(body).await;
    assert!(error.contains("unknown type"), "unexpected error: {error}");
}

#[tokio::test]
async fn test_anthropic_sse_missing_tool_id_or_name_fails_closed() {
    let block_start = json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": { "type": "tool_use", "id": "", "name": "calc" }
    })
    .to_string();

    let body = format!("event: content_block_start\ndata: {block_start}\n\n");
    let error = stream_error_for_body(body).await;
    assert!(
        error.contains("missing an id or function name"),
        "unexpected error: {error}"
    );
}
