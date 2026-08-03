//! Local loopback integration test for OpenAI SSE streaming in pi-ai.
//! Tests response frames split across arbitrary byte boundaries, including
//! text and tool call delta fragmentation.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use futures::StreamExt;
use pi_ai::providers::openai::OpenAiProvider;
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
    let model = Model::openai_compat("openai", "gpt-4o", base_url.clone(), 128_000, 4096);
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
    let mut events = OpenAiProvider::new()
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
async fn test_openai_sse_fragmented_loopback() {
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

        let chunk1 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "delta": {
                    "role": "assistant",
                    "content": "Hello "
                }
            }]
        })
        .to_string();

        let chunk2 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "delta": {
                    "content": "world!"
                }
            }]
        })
        .to_string();

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
        })
        .to_string();

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
        })
        .to_string();

        let chunk5 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        })
        .to_string();

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

#[tokio::test]
async fn test_openai_sse_requires_done_marker() {
    let chunk = json!({
        "choices": [{"delta": {"content": "partial"}, "finish_reason": "stop"}]
    });
    let error = stream_error_for_body(format!("data: {chunk}\n\n")).await;
    assert!(error.contains("before [DONE]"), "unexpected error: {error}");
}

#[tokio::test]
async fn test_openai_sse_rejects_incomplete_tool_calls() {
    for (id, name, arguments) in [
        ("", "calculator", "{}"),
        ("call_1", "", "{}"),
        ("call_1", "calculator", ""),
    ] {
        let chunk = json!({
            "choices": [{
                "delta": {"tool_calls": [{
                    "index": 0,
                    "id": id,
                    "function": {"name": name, "arguments": arguments}
                }]},
                "finish_reason": "tool_calls"
            }]
        });
        let error = stream_error_for_body(format!("data: {chunk}\n\ndata: [DONE]\n\n")).await;
        assert!(
            error.contains("missing an id or function name")
                || error.contains("empty tool call arguments"),
            "unexpected error: {error}"
        );
    }
}

#[tokio::test]
async fn test_openai_sse_malformed_json_fails_closed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).unwrap();

        let response_headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
        stream.write_all(response_headers.as_bytes()).unwrap();
        stream.flush().unwrap();

        let sse_body = "data: {invalid json}\n\n";
        stream.write_all(sse_body.as_bytes()).unwrap();
        stream.flush().unwrap();
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

    let mut found_err = false;
    while let Some(ev_res) = stream.next().await {
        if let Err(e) = ev_res {
            assert!(e.to_string().contains("malformed sse data"));
            found_err = true;
            break;
        }
    }

    server_handle.join().unwrap();
    assert!(
        found_err,
        "Expected invalid response error on malformed SSE data"
    );
}

#[tokio::test]
async fn test_openai_sse_malformed_tool_args_fails_closed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).unwrap();

        let response_headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
        stream.write_all(response_headers.as_bytes()).unwrap();
        stream.flush().unwrap();

        let chunk = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_bad",
                        "function": {
                            "name": "calculator",
                            "arguments": "{bad json"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();

        let sse_body = format!("data: {chunk}\n\ndata: [DONE]\n\n");
        stream.write_all(sse_body.as_bytes()).unwrap();
        stream.flush().unwrap();
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

    let mut found_err = false;
    while let Some(ev_res) = stream.next().await {
        if let Err(e) = ev_res {
            assert!(e.to_string().contains("malformed tool call arguments"));
            found_err = true;
            break;
        }
    }

    server_handle.join().unwrap();
    assert!(
        found_err,
        "Expected invalid response error on malformed tool args"
    );
}
