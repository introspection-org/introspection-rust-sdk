//! Multi-turn tool calling example using the Observation API.
//!
//! Demonstrates a two-turn conversation where the model requests a tool call,
//! the tool is executed locally, and the result is sent back for a final response.
//!
//! Requires environment variables:
//!   - `INTROSPECTION_TOKEN` — Introspection API token
//!   - `OPENAI_API_KEY` — OpenAI API key
//!
//! Run:
//! ```sh
//! cargo run --example openai_tool_call --features openai
//! ```

use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestMessage, ChatCompletionRequestToolMessage,
    ChatCompletionRequestToolMessageContent, ChatCompletionRequestUserMessage, ChatCompletionTool,
    ChatCompletionTools, CreateChatCompletionRequest, FunctionObject,
};
use async_openai::Client;
use introspection_sdk::otel::openai::traced_chat_completion;
use introspection_sdk::{
    IntrospectionSpanProcessor, Observation, ObservationConfig, SpanProcessorConfig,
};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

/// Simulated weather function.
fn get_weather(location: &str, _unit: &str) -> String {
    format!("The weather in {location} is 18°C with partly cloudy skies.")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok(); // Load .env if present

    // --- Provider setup (sync context — keeps reqwest's internal runtime
    //     from being dropped inside an async executor) ---
    let introspection_processor = IntrospectionSpanProcessor::new(SpanProcessorConfig::default())?;

    let provider = SdkTracerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_service_name("openai-tool-call-example")
                .build(),
        )
        .with_span_processor(introspection_processor)
        .build();

    tokio::runtime::Runtime::new()?.block_on(async {
        let tracer = provider.tracer("openai-tool-call-example");
        let openai_client = Client::with_config(OpenAIConfig::default());

        // --- Define the tool ---
        let weather_tool = ChatCompletionTools::Function(ChatCompletionTool {
            function: FunctionObject {
                name: "get_weather".to_string(),
                description: Some("Get the current weather for a location".to_string()),
                parameters: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": { "type": "string", "description": "City name" },
                        "unit": { "type": "string", "enum": ["celsius", "fahrenheit"] }
                    },
                    "required": ["location"]
                })),
                strict: None,
            },
        });

        // --- Wrap entire pipeline in a parent observation ---
        let _pipeline = Observation::start(&tracer, ObservationConfig::span("tool-call-pipeline"));

        // --- Turn 1: send request with tool definitions ---
        let request1 = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: "What's the weather in San Francisco?".into(),
                    ..Default::default()
                },
            )],
            tools: Some(vec![weather_tool]),
            ..Default::default()
        };

        let response1 = traced_chat_completion(&tracer, &openai_client, request1).await?;

        // --- Extract and execute tool call ---
        let choice = &response1.choices[0];
        let tool_calls = choice
            .message
            .tool_calls
            .as_ref()
            .expect("model should request a tool call");

        let tool_call = match &tool_calls[0] {
            ChatCompletionMessageToolCalls::Function(tc) => tc,
            _ => panic!("expected a function tool call"),
        };

        let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)?;
        let location = args["location"].as_str().unwrap_or("unknown");
        let unit = args["unit"].as_str().unwrap_or("celsius");
        let tool_result = get_weather(location, unit);
        println!(
            "Tool call: {}({}) -> {tool_result}",
            tool_call.function.name, tool_call.function.arguments
        );

        // --- Turn 2: send tool result back ---
        let request2 = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![
                // Original user message
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: "What's the weather in San Francisco?".into(),
                    ..Default::default()
                }),
                // Assistant's tool call response
                ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
                    tool_calls: Some(tool_calls.clone()),
                    ..Default::default()
                }),
                // Tool result
                ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
                    content: ChatCompletionRequestToolMessageContent::Text(tool_result),
                    tool_call_id: tool_call.id.clone(),
                }),
            ],
            ..Default::default()
        };

        let response2 = traced_chat_completion(&tracer, &openai_client, request2).await?;
        let final_content = response2.choices[0]
            .message
            .content
            .as_deref()
            .unwrap_or("");
        println!("Final response: {final_content}");

        Ok::<(), Box<dyn std::error::Error>>(())
    })?;

    // --- Shutdown (sync context — safe to drop the blocking client's runtime) ---
    if let Err(e) = provider.shutdown() {
        eprintln!("Warning: shutdown error: {e}");
    }
    Ok(())
}
