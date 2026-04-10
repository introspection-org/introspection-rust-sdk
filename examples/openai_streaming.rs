//! Streaming chat completion example using the Observation API.
//!
//! Demonstrates a two-turn conversation:
//!   - Turn 1: non-streaming call via `traced_chat_completion` (auto-instrumented)
//!   - Turn 2: streaming call via `traced_chat_completion_stream` (auto-instrumented)
//!
//! Requires environment variables:
//!   - `INTROSPECTION_TOKEN` — Introspection API token
//!   - `OPENAI_API_KEY` — OpenAI API key
//!
//! Run:
//! ```sh
//! cargo run --example openai_streaming --features openai
//! ```

use std::io::Write;

use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
    ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
};
use async_openai::Client;
use futures::StreamExt;
use introspection_sdk::openai::{traced_chat_completion, traced_chat_completion_stream};
use introspection_sdk::{
    IntrospectionSpanProcessor, Observation, ObservationConfig, SpanProcessorConfig,
};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok(); // Load .env if present

    // --- Provider setup ---
    let introspection_processor = IntrospectionSpanProcessor::new(SpanProcessorConfig::default())?;

    let provider = SdkTracerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_service_name("openai-streaming-example")
                .build(),
        )
        .with_span_processor(introspection_processor)
        .build();

    let tracer = provider.tracer("openai-streaming-example");
    let openai_client = Client::with_config(OpenAIConfig::default());
    let model = "gpt-4o-mini";

    // --- Wrap entire pipeline in a parent observation ---
    let _pipeline = Observation::start(&tracer, ObservationConfig::span("streaming-pipeline"));

    // --- Turn 1: non-streaming call (auto-instrumented) ---
    let request1 = CreateChatCompletionRequest {
        model: model.to_string(),
        messages: vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: "You are a helpful assistant. Say hello briefly.".into(),
                ..Default::default()
            },
        )],
        ..Default::default()
    };

    let response1 = traced_chat_completion(&tracer, &openai_client, request1).await?;
    let assistant_reply = response1.choices[0]
        .message
        .content
        .as_deref()
        .unwrap_or("");
    println!("Turn 1 (non-streaming): {assistant_reply}");

    // --- Turn 2: streaming call (auto-instrumented) ---
    let stream_request = CreateChatCompletionRequest {
        model: model.to_string(),
        messages: vec![
            ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                content: "You are a helpful assistant. Say hello briefly.".into(),
                ..Default::default()
            }),
            ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
                content: Some(assistant_reply.to_string().into()),
                ..Default::default()
            }),
            ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                content: "Now tell me a short joke about programming.".into(),
                ..Default::default()
            }),
        ],
        ..Default::default()
    };

    let mut stream = traced_chat_completion_stream(&tracer, &openai_client, stream_request).await?;

    print!("Turn 2 (streaming): ");
    while let Some(result) = stream.next().await {
        let chunk = result?;
        for choice in &chunk.choices {
            if let Some(ref content) = choice.delta.content {
                print!("{content}");
                std::io::stdout().flush()?;
            }
        }
    }
    println!(); // newline after streaming output

    // --- Shutdown ---
    if let Err(e) = provider.shutdown() {
        eprintln!("Warning: shutdown error: {e}");
    }
    Ok(())
}
