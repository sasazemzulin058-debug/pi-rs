//! Integration test for Anthropic prompt-cache markers.
//!
//! Asserts that the Anthropic request body emits `cache_control` on the
//! system prompt block, the last tool definition, and the last text block
//! of the last user message when `StreamOptions::cache_retention` is
//! non-`None`. With the default (`None`), no markers are emitted and the
//! system prompt stays a plain string for backward compatibility.

use pi_ai::providers::anthropic::build_request_body;
use pi_ai::{CacheRetention, Content, Context, Message, Model, StreamOptions, Tool};
use serde_json::json;

fn ctx_with_one_of_each() -> Context {
    Context {
        system_prompt: Some("you are pi.".into()),
        messages: vec![
            Message::User {
                content: vec![Content::text("hello")],
                timestamp: 0,
            },
            Message::user_text("second turn"),
        ],
        tools: vec![
            Tool {
                name: "first".into(),
                description: "first tool".into(),
                parameters: json!({"type": "object", "properties": {}}),
            },
            Tool {
                name: "last".into(),
                description: "last tool".into(),
                parameters: json!({"type": "object", "properties": {}}),
            },
        ],
    }
}

#[test]
fn cache_retention_short_marks_system_tools_and_last_user() {
    let model = Model::anthropic_claude_sonnet_4_6();
    let ctx = ctx_with_one_of_each();
    let opt = StreamOptions {
        cache_retention: CacheRetention::Short,
        ..Default::default()
    };
    let body = build_request_body(&model, &ctx, &opt);

    // System prompt is now an array, first block has cache_control: ephemeral.
    let sys = &body["system"];
    assert!(sys.is_array(), "system should be an array form");
    assert_eq!(sys[0]["type"], "text");
    assert_eq!(sys[0]["text"], "you are pi.");
    assert_eq!(sys[0]["cache_control"]["type"], "ephemeral");
    assert!(sys[0]["cache_control"].get("ttl").is_none());

    // Tools: last entry has cache_control, earlier entries do not.
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert!(tools[0].get("cache_control").is_none());
    assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");

    // Last user message's last text block has cache_control.
    let messages = body["messages"].as_array().unwrap();
    let last_user = messages
        .iter()
        .rfind(|m| m["role"] == "user")
        .expect("last user");
    let blocks = last_user["content"].as_array().unwrap();
    let last_text = blocks
        .iter()
        .rfind(|b| b["type"] == "text")
        .expect("text block");
    assert_eq!(last_text["cache_control"]["type"], "ephemeral");

    // Earlier user messages should not be marked.
    let first_user = messages
        .iter()
        .find(|m| m["role"] == "user")
        .expect("first user");
    let first_blocks = first_user["content"].as_array().unwrap();
    assert!(first_blocks[0].get("cache_control").is_none());
}

#[test]
fn cache_retention_long_adds_ttl_1h() {
    let model = Model::anthropic_claude_sonnet_4_6();
    let ctx = ctx_with_one_of_each();
    let opt = StreamOptions {
        cache_retention: CacheRetention::Long,
        ..Default::default()
    };
    let body = build_request_body(&model, &ctx, &opt);
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["system"][0]["cache_control"]["ttl"], "1h");
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.last().unwrap()["cache_control"]["ttl"], "1h");
}

#[test]
fn cache_retention_none_keeps_legacy_shape() {
    let model = Model::anthropic_claude_sonnet_4_6();
    let ctx = ctx_with_one_of_each();
    let opt = StreamOptions::default();
    assert_eq!(opt.cache_retention, CacheRetention::None);
    let body = build_request_body(&model, &ctx, &opt);
    // System stays a plain string.
    assert_eq!(body["system"], json!("you are pi."));
    // Tools have no cache_control.
    for t in body["tools"].as_array().unwrap() {
        assert!(t.get("cache_control").is_none());
    }
    // Last user message text has no cache_control.
    let messages = body["messages"].as_array().unwrap();
    let last_user = messages.iter().rfind(|m| m["role"] == "user").unwrap();
    for b in last_user["content"].as_array().unwrap() {
        assert!(b.get("cache_control").is_none());
    }
}

#[test]
fn test_compacted_messages_serializer_validity_all_providers() {
    use pi_ai::providers::{anthropic, google, openai, openai_responses};
    use serde_json::json;

    let tool_call_id = "call_abc123";
    let compacted_messages = vec![
        Message::user_text("initial instruction"),
        Message::Assistant(pi_ai::AssistantMessage {
            content: vec![
                Content::text("I will call a tool."),
                Content::ToolCall {
                    id: tool_call_id.to_string(),
                    name: "read_file".to_string(),
                    arguments: json!({"path": "foo.txt"}),
                },
            ],
            api: "test".into(),
            provider: "test".into(),
            model: "test".into(),
            usage: Default::default(),
            stop_reason: pi_ai::StopReason::ToolUse,
            error_message: None,
            timestamp: 0,
        }),
        Message::ToolResult(pi_ai::ToolResultMessage {
            tool_call_id: tool_call_id.to_string(),
            tool_name: "read_file".to_string(),
            content: vec![Content::text("file content")],
            is_error: false,
            timestamp: 0,
        }),
    ];

    let ctx = Context {
        system_prompt: Some("system prompt".into()),
        messages: compacted_messages,
        tools: vec![],
    };
    let options = StreamOptions::default();

    // Anthropic
    let anth_model = Model::anthropic_claude_sonnet_4_6();
    let anth_body = anthropic::build_request_body(&anth_model, &ctx, &options);
    let anth_msgs = anth_body["messages"].as_array().unwrap();
    assert_eq!(anth_msgs.len(), 3);
    assert_eq!(anth_msgs[1]["role"], "assistant");
    assert_eq!(anth_msgs[1]["content"][1]["type"], "tool_use");
    assert_eq!(anth_msgs[1]["content"][1]["id"], tool_call_id);
    assert_eq!(anth_msgs[2]["role"], "user");
    assert_eq!(anth_msgs[2]["content"][0]["type"], "tool_result");
    assert_eq!(anth_msgs[2]["content"][0]["tool_use_id"], tool_call_id);

    // Google Gemini
    let g_body = google::build_request_body(&ctx, &options);
    let g_contents = g_body["contents"].as_array().unwrap();
    assert_eq!(g_contents.len(), 3);
    assert_eq!(g_contents[1]["role"], "model");
    assert_eq!(
        g_contents[1]["parts"][1]["functionCall"]["name"],
        "read_file"
    );
    assert_eq!(g_contents[2]["role"], "user");
    assert_eq!(
        g_contents[2]["parts"][0]["functionResponse"]["name"],
        "read_file"
    );

    // OpenAI Chat
    let oa_model = Model::openai_compat(
        "openai",
        "gpt-4o",
        "https://api.openai.com/v1",
        128000,
        4096,
    );
    let oa_body = openai::build_request_body(&oa_model, &ctx, &options);
    let oa_msgs = oa_body["messages"].as_array().unwrap();
    // System + 3 messages = 4
    assert_eq!(oa_msgs.len(), 4);
    assert_eq!(oa_msgs[0]["role"], "system");
    assert_eq!(oa_msgs[2]["role"], "assistant");
    assert_eq!(oa_msgs[2]["tool_calls"][0]["id"], tool_call_id);
    assert_eq!(oa_msgs[3]["role"], "tool");
    assert_eq!(oa_msgs[3]["tool_call_id"], tool_call_id);

    // OpenAI Responses
    let oar_body = openai_responses::build_request_body(&oa_model, &ctx, &options);
    let oar_input = oar_body["input"].as_array().unwrap();
    // User message, Assistant text message, Assistant function_call item, Tool result item = 4
    assert_eq!(oar_input.len(), 4);
    assert_eq!(oar_input[2]["type"], "function_call");
    assert_eq!(oar_input[2]["call_id"], tool_call_id);
    assert_eq!(oar_input[3]["type"], "function_call_output");
    assert_eq!(oar_input[3]["call_id"], tool_call_id);
}
