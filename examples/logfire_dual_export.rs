//! Dual export example: sends gen_ai traces to both Logfire and Introspection.
//!
//! Builds a standalone `SdkTracerProvider` with two span processors —
//! one for Introspection and one for Logfire — then uses the SDK's
//! `traced_chat_completion` (backed by the `Observation` API) to
//! instrument the OpenAI call.
//!
//! Requires environment variables:
//!   - `INTROSPECTION_TOKEN` — Introspection API token
//!   - `LOGFIRE_TOKEN` — Logfire write token
//!   - `OPENAI_API_KEY` — OpenAI API key
//!
//! Run:
//! ```sh
//! cargo run --example logfire_dual_export --features logfire,openai
//! ```

use std::collections::HashMap;
use std::time::Duration;

use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
};
use async_openai::Client;
use introspection_sdk::openai::traced_chat_completion;
use introspection_sdk::{IntrospectionSpanProcessor, SpanProcessorConfig};
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::trace::{BatchSpanProcessor, SdkTracerProvider};
use opentelemetry_sdk::Resource;

fn new_blocking_http_client() -> reqwest::blocking::Client {
    std::thread::spawn(|| {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
    .join()
    .expect("failed to create blocking HTTP client")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok(); // Load .env if present

    // Introspection span processor (reads INTROSPECTION_TOKEN from env)
    let introspection_processor = IntrospectionSpanProcessor::new(SpanProcessorConfig::default())?;

    // Logfire OTLP exporter
    let logfire_token = std::env::var("LOGFIRE_TOKEN").expect("LOGFIRE_TOKEN must be set");
    let mut logfire_headers = HashMap::new();
    logfire_headers.insert(
        "Authorization".to_string(),
        format!("Bearer {}", logfire_token),
    );

    let logfire_exporter = SpanExporter::builder()
        .with_http()
        .with_http_client(new_blocking_http_client())
        .with_endpoint("https://logfire-us.pydantic.dev/v1/traces")
        .with_headers(logfire_headers)
        .with_timeout(Duration::from_secs(30))
        .build()?;
    let logfire_processor = BatchSpanProcessor::builder(logfire_exporter).build();

    // Single provider with both processors
    let provider = SdkTracerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_service_name("logfire-dual-export-example")
                .build(),
        )
        .with_span_processor(introspection_processor)
        .with_span_processor(logfire_processor)
        .build();

    let tracer = provider.tracer("my-app");
    let openai_client = Client::with_config(OpenAIConfig::default());

    let request = CreateChatCompletionRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: "Explain the concept of dual exporting in observability in one sentence."
                    .into(),
                ..Default::default()
            },
        )],
        ..Default::default()
    };

    let response = traced_chat_completion(&tracer, &openai_client, request).await?;
    let content = response.choices[0].message.content.as_deref().unwrap_or("");
    println!("Response: {content}");

    if let Err(e) = provider.shutdown() {
        eprintln!("Warning: shutdown error: {e}");
    }

    Ok(())
}
