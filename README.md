<div align="center">
  <a href="https://introspection.dev">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset=".github/images/logo-dark.svg">
      <source media="(prefers-color-scheme: light)" srcset=".github/images/logo-light.svg">
      <img alt="Introspection" src=".github/images/logo-light.svg" width="30%">
    </picture>
  </a>
</div>

<h4 align="center">Build frontier AI systems that self-improve.</h4>

<div align="center">
  <a href="https://introspection.dev"><img src="https://img.shields.io/badge/website-introspection.dev-blue" alt="Website"></a>
  <a href="https://crates.io/crates/introspection-sdk"><img src="https://img.shields.io/crates/v/introspection-sdk?label=%20" alt="crates.io version"></a>
  <a href="https://www.apache.org/licenses/LICENSE-2.0"><img src="https://img.shields.io/badge/license-Apache%202.0-green" alt="License"></a>
  <a href="https://x.com/IntrospectionAI"><img src="https://img.shields.io/twitter/follow/IntrospectionAI" alt="Follow on X"></a>
</div>

<br>

[Introspection](https://introspection.dev) continuously improves your AI systems with production feedback and frontier practices. This is the Rust SDK. Built on OpenTelemetry, providing both a log-based Client API for analytics events and a span-based Observation API for instrumenting LLM calls and pipelines.

## Installation

```bash
cargo add introspection-sdk
```

With optional integrations:

```bash
cargo add introspection-sdk --features openai,logfire
```

See [Feature Flags](#feature-flags) below for the full list.

## Feature Flags

| Feature   | Description                                                        |
| --------- | ------------------------------------------------------------------ |
| `openai`  | `async-openai` integration — `traced_chat_completion` and friends  |
| `logfire`  | Logfire integration for dual-export pipelines                      |
| `testing` | In-memory span exporter and test helpers                           |

## Observation API

The Observation API instruments LLM calls and pipeline steps as OpenTelemetry spans with [gen_ai semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/).

```rust
use introspection_sdk::{Observation, ObservationConfig, GenerationUpdate};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;

let provider = SdkTracerProvider::builder().build();
let tracer = provider.tracer("my-app");

// Instrument an LLM call
let mut obs = Observation::start(
    &tracer,
    ObservationConfig::generation("chat", "gpt-4o-mini"),
);

// ... make the API call ...

obs.update_generation(
    GenerationUpdate::new()
        .with_response_model("gpt-4o-mini")
        .with_response_id("chatcmpl-abc123")
        .with_usage(12, 8),
);
obs.set_ok();
// span ends automatically when `obs` is dropped
```

### Observation Types

- **`ObservationConfig::generation("name", "model")`** — LLM generation call. Sets `gen_ai.request.model`, auto-infers the system (openai, anthropic, etc.), and maps to `SpanKind::Client`.
- **`ObservationConfig::span("name")`** — General pipeline step. Maps to `SpanKind::Internal`.

Both support `.with_input(json)`, `.with_system("openai")`, `.with_operation_name("chat")`, and `.with_attribute(kv)`.

### Nesting

Observations automatically nest via OpenTelemetry context propagation. Child observations created while a parent is alive become children of that parent:

```rust
let parent = Observation::start(&tracer, ObservationConfig::span("pipeline"));

// This becomes a child of "pipeline"
let child = Observation::start(
    &tracer,
    ObservationConfig::generation("chat", "gpt-4o-mini"),
);
drop(child);
drop(parent);
```

## OpenAI Integration

Requires the `openai` feature. Wraps `async-openai` calls with automatic instrumentation.

### `traced_chat_completion`

One-call wrapper for non-streaming completions. Creates a generation span, makes the API call, and records the response (model, id, usage, output).

```rust
use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::types::chat::*;
use introspection_sdk::openai::traced_chat_completion;

let client = Client::with_config(OpenAIConfig::default());
let request = CreateChatCompletionRequest {
    model: "gpt-4o-mini".to_string(),
    messages: vec![ChatCompletionRequestMessage::User(
        ChatCompletionRequestUserMessage {
            content: "Hello!".into(),
            ..Default::default()
        },
    )],
    ..Default::default()
};

let response = traced_chat_completion(&tracer, &client, request).await?;
```

### `traced_chat_completion_stream`

Streaming variant. Returns a `TracedStream` that accumulates content and usage across chunks, finalizing the span when the stream completes or is dropped.

```rust
use futures::StreamExt;
use introspection_sdk::openai::traced_chat_completion_stream;

let mut stream = traced_chat_completion_stream(&tracer, &client, request).await?;
while let Some(result) = stream.next().await {
    let chunk = result?;
    for choice in &chunk.choices {
        if let Some(ref content) = choice.delta.content {
            print!("{content}");
        }
    }
}
```

### `tracing_traced_chat_completion`

For `tracing`-based pipelines (e.g. logfire). Uses `tracing::info_span!` instead of the OTel tracer directly, so spans flow through the `tracing` subscriber.

```rust
use introspection_sdk::openai::tracing_traced_chat_completion;

let response = tracing_traced_chat_completion(&client, request).await?;
```

## Logfire Integration

Logfire routes spans through the `tracing` crate, not the direct OTel API. For dual-export (Logfire + Introspection), build a standalone `SdkTracerProvider` with both span processors:

```rust
use introspection_sdk::{IntrospectionSpanProcessor, SpanProcessorConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;

let introspection_processor = IntrospectionSpanProcessor::new(
    SpanProcessorConfig::with_token("your-introspection-token")
).unwrap();

let provider = SdkTracerProvider::builder()
    .with_span_processor(introspection_processor)
    // .with_span_processor(logfire_processor)
    .build();
```

See `examples/logfire_dual_export.rs` and `examples/logfire_introspection.rs` for complete working examples.

## Environment Variables

The SDK reads from environment variables when values aren't explicitly set:

- `INTROSPECTION_TOKEN` - Authentication token (required)
- `INTROSPECTION_SERVICE_NAME` - Service name (default: "introspection-client")
- `INTROSPECTION_BASE_URL` - API base URL (default: "https://api.nuraline.ai")

**Note:** `base_url` can also be set via `AdvancedOptions` for programmatic configuration.

## OpenTelemetry Integration

### Span Processor

For automatic tracing with logfire or other OpenTelemetry setups:

```rust
use introspection_sdk::{AdvancedOptions, span_processor::{IntrospectionSpanProcessor, SpanProcessorConfig}};
use opentelemetry_sdk::trace::TracerProvider;

// Simple usage
let span_processor = IntrospectionSpanProcessor::new(
    SpanProcessorConfig::with_token("your-token")
).unwrap();

// With advanced options (custom base URL and headers)
let span_processor = IntrospectionSpanProcessor::new(
    SpanProcessorConfig::with_token("your-token")
        .advanced(AdvancedOptions {
            base_url: Some("http://localhost:5418/v1/traces".to_string()),
            additional_headers: None,
            span_exporter: None,
            log_exporter: None, // Not used for span processor, only for client
            ..Default::default()
        })
).unwrap();

let provider = TracerProvider::builder()
    .with_span_processor(span_processor)
    .build();
```

### Native OpenTelemetry Components

This SDK uses native OpenTelemetry components:
- `opentelemetry_sdk::logs::SdkLoggerProvider` - Log provider with batch processing
- `opentelemetry_otlp::LogExporter` - OTLP HTTP exporter
- `opentelemetry::Context` - Context propagation via baggage

This ensures compatibility with other OpenTelemetry instrumentation and distributed tracing.

## Client Usage

### Quick Start

```rust
use introspection_sdk::{IntrospectionClient, ClientConfig, FeedbackOptions};

fn main() {
    // Initialize the client
    let client = IntrospectionClient::new(
        ClientConfig::with_token("your-token")
    ).unwrap();

    // Set context using guards (automatically cleared when guard drops)
    {
        let _user = client.set_user_id("user_123");
        let _conv = client.set_conversation_id("conv_456");

        // Track feedback - main use case
        client.feedback(
            "thumbs_up",
            FeedbackOptions::new()
                .with_comments("Great response!")
                .with_previous_response_id("msg_123"),
        );
    } // Context automatically cleared

    // Shutdown gracefully
    client.shutdown().unwrap();
}
```

### Configuration

#### Using the Builder Pattern

Basic configuration with token and service name:

```rust
use introspection_sdk::{IntrospectionClient, ClientConfig};

let client = IntrospectionClient::new(
    ClientConfig::builder()
        .token("your-token")
        .service_name("my-service")
        .build()
        .unwrap()
).unwrap();
```

#### Using Advanced Options

For advanced configuration including custom base URL, headers, exporters, and batch settings:

```rust
use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient};

// Custom base URL (for testing or custom endpoints)
let client = IntrospectionClient::new(
    ClientConfig::builder()
        .token("your-token")
        .advanced(AdvancedOptions {
            base_url: Some("http://localhost:8080".to_string()),
            ..Default::default()
        })
        .build()
        .unwrap()
).unwrap();

// Full advanced configuration
let client = IntrospectionClient::new(
    ClientConfig::builder()
        .token("your-token")
        .advanced(AdvancedOptions {
            base_url: Some("http://localhost:8080".to_string()),
            additional_headers: Some([("X-Custom-Header".to_string(), "value".to_string())].into_iter().collect()),
            flush_interval_ms: Some(1000), // Flush every 1 second
            max_batch_size: Some(50), // Max 50 logs per batch
            ..Default::default()
        })
        .build()
        .unwrap()
).unwrap();
```

**Note:** `base_url` and `debug` are now part of `AdvancedOptions`. The same `AdvancedOptions` struct is used for both `IntrospectionClient` and `IntrospectionSpanProcessor`. For simple use cases, you can rely on environment variables (`INTROSPECTION_BASE_URL`) or defaults.

#### Using Environment Variables

The client automatically reads from environment variables:

```rust
use introspection_sdk::{IntrospectionClient, ClientConfig};

// Uses environment variables:
// - INTROSPECTION_TOKEN
// - INTROSPECTION_SERVICE_NAME
// - INTROSPECTION_BASE_URL
let client = IntrospectionClient::new(ClientConfig::default()).unwrap();
```

### API Reference

#### feedback

Track feedback on AI responses or messages. This is the main use case.

```rust
use introspection_sdk::FeedbackOptions;

// Simple feedback
client.feedback("thumbs_up", FeedbackOptions::new());

// With comments and context
client.feedback(
    "thumbs_down",
    FeedbackOptions::new()
        .with_comments("Answer was off topic")
        .with_conversation_id("conv_123")
        .with_previous_response_id("msg_456"),
);

// With extra properties
client.feedback(
    "rating",
    FeedbackOptions::new()
        .with_extra("score", 4)
        .with_extra("category", "helpfulness"),
);
```

#### identify

Identify a user and associate traits with them.

```rust
use introspection_sdk::IdentifyOptions;

{
    let _guard = client.set_user_id("user_123");

    client.identify(
        "user_123",
        Some(IdentifyOptions::new()
            .with_trait("email", "user@example.com")
            .with_trait("plan", "pro")),
    );
}
```

#### track

Track a custom event with optional properties.

```rust
use introspection_sdk::TrackOptions;

client.track(
    "Button Clicked",
    Some(TrackOptions::new()
        .with_property("button_id", "submit")
        .with_property("page", "checkout")),
);
```

### Context Management with Baggage

Context is managed via OpenTelemetry baggage, which propagates across distributed systems. The `set_*` methods return guards that automatically clean up when dropped - similar to Python's context managers.

```rust
// Guards automatically clear context when dropped
{
    let _user_guard = client.set_user_id("user_123");
    let _conv_guard = client.set_conversation_id("conv_456");
    let _agent_guard = client.set_agent("support-bot", Some("agent_789"));

    // All events here inherit the baggage context
    client.feedback("thumbs_up", Default::default());
} // Context is cleared here
```

Available context methods:
- `set_user_id(id)` - Set user ID
- `set_anonymous_id(id)` - Set anonymous ID
- `set_conversation_id(id)` - Set conversation ID
- `set_previous_response_id(id)` - Set previous response ID
- `set_agent(name, id)` - Set agent context
- `set_baggage(&[...])` - Set multiple baggage values

### Lifecycle Management

#### Flushing Events

Events are batched and sent periodically by the OpenTelemetry SDK. Force an immediate flush:

```rust
client.flush().unwrap();
```

#### Graceful Shutdown

Always shutdown the client to ensure all events are sent:

```rust
client.shutdown().unwrap();
```

## Documentation

Full documentation is available at [docs.introspection.dev](https://docs.introspection.dev).

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
