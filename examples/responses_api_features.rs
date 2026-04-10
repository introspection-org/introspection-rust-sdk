//! OpenAI Responses API Features Example
//!
//! Demonstrates Introspection tracing with OpenAI Responses API features:
//! web search, reasoning with detailed summaries, encrypted reasoning, and
//! remote MCP tools via DeepWiki.
//!
//! Requires environment variables:
//!   - `INTROSPECTION_TOKEN` — Introspection API token
//!   - `OPENAI_API_KEY` — OpenAI API key
//!
//! Run:
//! ```sh
//! cargo run --example responses_api_features --features openai
//! ```

use async_openai::config::OpenAIConfig;
use async_openai::types::mcp::{MCPTool, MCPToolApprovalSetting, MCPToolRequireApproval};
use async_openai::types::responses::{
    CreateResponse, IncludeEnum, InputParam, OutputItem, Reasoning, ReasoningEffort,
    ReasoningSummary, Tool, WebSearchTool,
};
use async_openai::Client;
use introspection_sdk::openai::traced_responses_create;
use introspection_sdk::{IntrospectionSpanProcessor, SpanProcessorConfig};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // --- Introspection setup ---
    let processor = IntrospectionSpanProcessor::new(SpanProcessorConfig::default())?;
    let provider = SdkTracerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_service_name("responses-api-features-example")
                .build(),
        )
        .with_span_processor(processor)
        .build();
    let tracer = provider.tracer("responses-api-features-example");
    let client = Client::with_config(OpenAIConfig::default());

    // === 1. Web Search (gpt-4o) ===
    println!("=== 1. Web Search Agent (gpt-4o) ===");

    let r1 = traced_responses_create(
        &tracer,
        &client,
        CreateResponse {
            model: Some("gpt-4o".to_string()),
            instructions: Some(
                "You MUST use web search. Always search the web first before answering.".into(),
            ),
            input: InputParam::Text("What is the latest SpaceX launch in 2026?".into()),
            tools: Some(vec![Tool::WebSearch(WebSearchTool::default())]),
            ..Default::default()
        },
    )
    .await?;
    print_response_text(&r1.output);
    println!();

    // === 2. Reasoning with Detailed Summary (gpt-5.4) ===
    println!("=== 2. Reasoning with Detailed Summary (gpt-5.4) ===");

    let r2 = traced_responses_create(
        &tracer,
        &client,
        CreateResponse {
            model: Some("gpt-5.4".to_string()),
            instructions: Some("Think step by step. Show your work.".into()),
            input: InputParam::Text(
                "A farmer has 17 chickens and 23 cows. Each chicken eats 0.5kg of feed per day \
                 and each cow eats 15kg. If feed costs $0.40/kg, how much does the farmer spend per week?"
                    .into(),
            ),
            reasoning: Some(Reasoning {
                effort: Some(ReasoningEffort::High),
                summary: Some(ReasoningSummary::Detailed),
            }),
            ..Default::default()
        },
    )
    .await?;
    print_response_text(&r2.output);
    println!();

    // === 3. Encrypted Reasoning + Detailed Summary (gpt-5.4) ===
    println!("=== 3. Encrypted Reasoning + Detailed Summary (gpt-5.4) ===");

    let r3 = traced_responses_create(
        &tracer,
        &client,
        CreateResponse {
            model: Some("gpt-5.4".to_string()),
            instructions: Some("Think carefully before answering.".into()),
            input: InputParam::Text(
                "If a train travels at 120 km/h for 2.5 hours, then slows to 80 km/h for 1.75 hours, \
                 what is the total distance and average speed?"
                    .into(),
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
    .await?;
    print_response_text(&r3.output);
    println!();

    // === 4. MCP Tools - DeepWiki (gpt-4o) ===
    println!("=== 4. MCP Tools - DeepWiki (gpt-4o) ===");

    let r4 = traced_responses_create(
        &tracer,
        &client,
        CreateResponse {
            model: Some("gpt-4o".to_string()),
            instructions: Some(
                "Use the DeepWiki MCP tools to answer questions about code repositories.".into(),
            ),
            input: InputParam::Text(
                "How does the Agent class work in the openai/openai-agents-python repo?".into(),
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
    .await?;
    print_response_text(&r4.output);

    // --- Shutdown ---
    if let Err(e) = provider.shutdown() {
        eprintln!("Warning: shutdown error: {e}");
    }
    println!("\n✓ All examples completed and traces exported.");
    Ok(())
}

fn print_response_text(output: &[OutputItem]) {
    for item in output {
        if let OutputItem::Message(msg) = item {
            for content in &msg.content {
                let v = serde_json::to_value(content).unwrap_or_default();
                if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
                    let preview = if text.len() > 200 { &text[..200] } else { text };
                    println!("Response: {preview}...");
                }
            }
        }
    }
}
