//! Integration tests for Responses API features.
//!
//! These tests hit the real OpenAI API and verify that the traced_responses_create
//! function correctly captures gen_ai attributes for reasoning, encrypted content,
//! and MCP tool calls.
//!
//! Run with:
//! ```sh
//! cargo test --features testing,openai --test responses_api_test -- --ignored
//! ```

use introspection_sdk::otel::openai::traced_responses_create;
use introspection_sdk::otel::testing::{setup_test_provider, span_data_to_json};
use opentelemetry::trace::TracerProvider;

// ---------------------------------------------------------------------------
// Test 1: Encrypted reasoning produces thinking part with signature
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore = "requires OPENAI_API_KEY"]
async fn test_encrypted_reasoning_has_signature() {
    use async_openai::config::OpenAIConfig;
    use async_openai::types::responses::{
        CreateResponse, IncludeEnum, InputParam, Reasoning, ReasoningEffort, ReasoningSummary,
    };
    use async_openai::Client;

    dotenvy::dotenv().ok();

    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");
    let client = Client::with_config(OpenAIConfig::default());

    let response = traced_responses_create(
        &tracer,
        &client,
        CreateResponse {
            model: Some("gpt-5.4".to_string()),
            instructions: Some("Think carefully before answering.".to_string()),
            input: InputParam::Text(
                "If a train travels at 120 km/h for 2.5 hours, then slows to 80 km/h for 1.75 hours, what is the total distance and average speed?"
                    .to_string(),
            ),
            reasoning: Some(Reasoning {
                effort: Some(ReasoningEffort::High),
                summary: Some(ReasoningSummary::Detailed),
            }),
            include: Some(vec![IncludeEnum::ReasoningEncryptedContent]),
            store: Some(false),
            ..Default::default()
        },
    )
    .await
    .expect("API call should succeed");

    assert!(!response.output.is_empty());

    provider.force_flush().unwrap();
    let spans = exporter.get_finished_spans().unwrap();
    assert!(!spans.is_empty(), "Should have at least one span");

    let json = span_data_to_json(&spans[0]);
    insta::assert_json_snapshot!(json, {
        ".trace_id" => "[trace_id]",
        ".span_id" => "[span_id]",
        ".parent_span_id" => "[span_id]",
        ".start_time" => "[timestamp]",
        ".end_time" => "[timestamp]",
        ".attributes[\"gen_ai.response.id\"]" => "[response_id]",
        ".attributes[\"gen_ai.response.model\"]" => "[response_model]",
        ".attributes[\"gen_ai.request.model\"]" => "[request_model]",
        ".attributes[\"gen_ai.usage.input_tokens\"]" => "[input_tokens]",
        ".attributes[\"gen_ai.usage.output_tokens\"]" => "[output_tokens]",
        ".attributes[\"gen_ai.usage.total_tokens\"]" => "[total_tokens]",
        ".attributes[\"gen_ai.output.messages\"]" => "[output_messages]",
        ".attributes[\"gen_ai.input.messages\"]" => "[input_messages]",
        ".attributes[\"gen_ai.conversation.id\"]" => "[conversation_id]",
    });

    // Validate thinking parts with signature in raw output
    let output_raw = json["attributes"]["gen_ai.output.messages"]
        .as_str()
        .unwrap();
    let output_messages: Vec<serde_json::Value> = serde_json::from_str(output_raw).unwrap();
    let empty = vec![];
    let all_parts: Vec<&serde_json::Value> = output_messages
        .iter()
        .flat_map(|m| m["parts"].as_array().unwrap_or(&empty))
        .collect();

    let thinking_parts: Vec<&&serde_json::Value> = all_parts
        .iter()
        .filter(|p| p["type"] == "thinking")
        .collect();
    assert!(!thinking_parts.is_empty(), "Should have thinking parts");
    assert!(
        thinking_parts.iter().any(|p| p.get("signature").is_some()),
        "At least one thinking part should have a signature"
    );

    let text_parts: Vec<&&serde_json::Value> =
        all_parts.iter().filter(|p| p["type"] == "text").collect();
    assert!(!text_parts.is_empty(), "Should have text parts");

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 2: Reasoning with summary (no encryption) produces thinking with content
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore = "requires OPENAI_API_KEY"]
async fn test_reasoning_summary_has_content() {
    use async_openai::config::OpenAIConfig;
    use async_openai::types::responses::{
        CreateResponse, InputParam, Reasoning, ReasoningEffort, ReasoningSummary,
    };
    use async_openai::Client;

    dotenvy::dotenv().ok();

    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");
    let client = Client::with_config(OpenAIConfig::default());

    let _response = traced_responses_create(
        &tracer,
        &client,
        CreateResponse {
            model: Some("gpt-5.4".to_string()),
            instructions: Some("Think step by step. Show your work.".to_string()),
            input: InputParam::Text("What is 17 * 23?".to_string()),
            reasoning: Some(Reasoning {
                effort: Some(ReasoningEffort::High),
                summary: Some(ReasoningSummary::Detailed),
            }),
            ..Default::default()
        },
    )
    .await
    .expect("API call should succeed");

    provider.force_flush().unwrap();
    let spans = exporter.get_finished_spans().unwrap();
    assert!(!spans.is_empty());

    let json = span_data_to_json(&spans[0]);
    insta::assert_json_snapshot!(json, {
        ".trace_id" => "[trace_id]",
        ".span_id" => "[span_id]",
        ".parent_span_id" => "[span_id]",
        ".start_time" => "[timestamp]",
        ".end_time" => "[timestamp]",
        ".attributes[\"gen_ai.response.id\"]" => "[response_id]",
        ".attributes[\"gen_ai.response.model\"]" => "[response_model]",
        ".attributes[\"gen_ai.request.model\"]" => "[request_model]",
        ".attributes[\"gen_ai.usage.input_tokens\"]" => "[input_tokens]",
        ".attributes[\"gen_ai.usage.output_tokens\"]" => "[output_tokens]",
        ".attributes[\"gen_ai.usage.total_tokens\"]" => "[total_tokens]",
        ".attributes[\"gen_ai.output.messages\"]" => "[output_messages]",
        ".attributes[\"gen_ai.input.messages\"]" => "[input_messages]",
        ".attributes[\"gen_ai.conversation.id\"]" => "[conversation_id]",
    });

    // Validate thinking parts have content (summary text)
    let output_raw = json["attributes"]["gen_ai.output.messages"]
        .as_str()
        .unwrap();
    let output_messages: Vec<serde_json::Value> = serde_json::from_str(output_raw).unwrap();
    let empty = vec![];
    let all_parts: Vec<&serde_json::Value> = output_messages
        .iter()
        .flat_map(|m| m["parts"].as_array().unwrap_or(&empty))
        .collect();

    let thinking_parts: Vec<&&serde_json::Value> = all_parts
        .iter()
        .filter(|p| p["type"] == "thinking")
        .collect();
    assert!(!thinking_parts.is_empty(), "Should have thinking parts");
    assert!(
        thinking_parts.iter().any(|p| p.get("content").is_some()),
        "Thinking parts should have content (summary text)"
    );

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 3: MCP tool call produces correct gen_ai attributes
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore = "requires OPENAI_API_KEY"]
async fn test_mcp_deepwiki_produces_spans() {
    use async_openai::config::OpenAIConfig;
    use async_openai::types::mcp::{MCPTool, MCPToolApprovalSetting, MCPToolRequireApproval};
    use async_openai::types::responses::{CreateResponse, InputParam, Tool};
    use async_openai::Client;

    dotenvy::dotenv().ok();

    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");
    let client = Client::with_config(OpenAIConfig::default());

    let _response = traced_responses_create(
        &tracer,
        &client,
        CreateResponse {
            model: Some("gpt-4o".to_string()),
            instructions: Some(
                "Use the DeepWiki MCP tools to answer questions. Be very concise.".to_string(),
            ),
            input: InputParam::Text(
                "What programming language is the openai/openai-agents-python repo written in? One word answer."
                    .to_string(),
            ),
            tools: Some(vec![Tool::Mcp(MCPTool {
                server_label: "deepwiki".to_string(),
                server_url: Some("https://mcp.deepwiki.com/mcp".to_string()),
                require_approval: Some(MCPToolRequireApproval::ApprovalSetting(
                    MCPToolApprovalSetting::Never,
                )),
                ..Default::default()
            })]),
            ..Default::default()
        },
    )
    .await
    .expect("API call should succeed");

    provider.force_flush().unwrap();
    let spans = exporter.get_finished_spans().unwrap();
    assert!(!spans.is_empty());

    let json = span_data_to_json(&spans[0]);
    insta::assert_json_snapshot!(json, {
        ".trace_id" => "[trace_id]",
        ".span_id" => "[span_id]",
        ".parent_span_id" => "[span_id]",
        ".start_time" => "[timestamp]",
        ".end_time" => "[timestamp]",
        ".attributes[\"gen_ai.response.id\"]" => "[response_id]",
        ".attributes[\"gen_ai.response.model\"]" => "[response_model]",
        ".attributes[\"gen_ai.request.model\"]" => "[request_model]",
        ".attributes[\"gen_ai.usage.input_tokens\"]" => "[input_tokens]",
        ".attributes[\"gen_ai.usage.output_tokens\"]" => "[output_tokens]",
        ".attributes[\"gen_ai.usage.total_tokens\"]" => "[total_tokens]",
        ".attributes[\"gen_ai.output.messages\"]" => "[output_messages]",
        ".attributes[\"gen_ai.input.messages\"]" => "[input_messages]",
        ".attributes[\"gen_ai.conversation.id\"]" => "[conversation_id]",
    });

    // Validate MCP tool calls in output
    let output_raw = json["attributes"]["gen_ai.output.messages"]
        .as_str()
        .unwrap();
    let output_messages: Vec<serde_json::Value> = serde_json::from_str(output_raw).unwrap();
    let empty = vec![];
    let mut found_mcp_tool_call = false;
    let mut found_mcp_tool_response = false;

    for msg in &output_messages {
        for part in msg["parts"].as_array().unwrap_or(&empty) {
            if part["type"] == "tool_call"
                && part["name"]
                    .as_str()
                    .is_some_and(|n| n.contains("deepwiki/"))
            {
                found_mcp_tool_call = true;
            }
            if part["type"] == "tool_call_response" {
                found_mcp_tool_response = true;
            }
        }
    }

    assert!(found_mcp_tool_call, "Should have MCP tool_call part");
    assert!(
        found_mcp_tool_response,
        "Should have MCP tool_call_response part"
    );

    provider.shutdown().unwrap();
}
