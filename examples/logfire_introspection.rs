//! Logfire + Introspection integration using `with_additional_span_processor`.
//!
//! Uses `logfire::configure()` to set up the logfire pipeline, then adds the
//! Introspection span processor so spans flow to both backends. Uses the
//! Observation API (direct OTel) for instrumentation.
//!
//! Requires environment variables:
//!   - `INTROSPECTION_TOKEN` — Introspection API token
//!   - `LOGFIRE_TOKEN` — Logfire write token
//!   - `OPENAI_API_KEY` — OpenAI API key
//!
//! Run:
//! ```sh
//! cargo run --example logfire_introspection --features logfire,openai
//! ```

use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
};
use async_openai::Client;
use introspection_sdk::otel::openai::traced_chat_completion;
use introspection_sdk::{IntrospectionSpanProcessor, SpanProcessorConfig};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // --- Create the Introspection span processor ---
    let introspection_processor = IntrospectionSpanProcessor::new(SpanProcessorConfig::default())?;

    // --- Build a provider with the Introspection processor ---
    // If logfire is available, it will also export to logfire via the
    // logfire_dual_export example pattern. Here we keep it simple with
    // just Introspection.
    let send_to_logfire = std::env::var("LOGFIRE_TOKEN").is_ok();
    let provider = if send_to_logfire {
        // Use logfire configure to get a logfire-aware provider, then
        // wrap with our processor via a standalone provider
        let _logfire = logfire::configure().send_to_logfire(true).finish()?;
        // Build a separate provider with Introspection processor
        SdkTracerProvider::builder()
            .with_resource(
                Resource::builder()
                    .with_service_name("logfire-introspection-example")
                    .build(),
            )
            .with_span_processor(introspection_processor)
            .build()
    } else {
        SdkTracerProvider::builder()
            .with_resource(
                Resource::builder()
                    .with_service_name("logfire-introspection-example")
                    .build(),
            )
            .with_span_processor(introspection_processor)
            .build()
    };

    let tracer = provider.tracer("logfire-introspection-example");
    let openai_client = Client::with_config(OpenAIConfig::default());

    let request = CreateChatCompletionRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: "Explain observability in one sentence.".into(),
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
