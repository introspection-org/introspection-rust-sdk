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

[Introspection](https://introspection.dev) continuously improves your AI systems with production feedback and frontier practices. This is the Rust SDK.

The SDK exposes **three independent surfaces** — wire up only what you need:

| Surface | What it does | Cargo feature |
| --- | --- | --- |
| [`IntrospectionClient`](#1-introspectionclient--introspection-api-runtimes-tasks-files) | Introspection API: runtimes, experiments, runner, tasks, files | _none_ (default) |
| [`IntrospectionLogs`](#2-introspectionlogs--analytics-events-track-feedback-identify) | Analytics events: `track` / `feedback` / `identify` (OTLP logs) | `otel` |
| [`IntrospectionSpanProcessor`](#3-introspectionspanprocessor--traces-span-processors--instrumentors) | Traces: span processors + LLM SDK instrumentors (OTLP traces) | `otel` |

They share no state. Construct the ones you want, configure independently, mix and match.

## Installation

Default install — `IntrospectionClient` only (no OpenTelemetry pulled in):

```toml
[dependencies]
introspection-sdk = "0.1"
```

With logs/traces export:

```toml
[dependencies]
introspection-sdk = { version = "0.1", features = ["otel"] }
```

With the `async-openai` adapter for instrumented LLM calls:

```toml
[dependencies]
introspection-sdk = { version = "0.1", features = ["openai"] }
```

### Feature flags

| Feature   | Description                                                        |
| --------- | ------------------------------------------------------------------ |
| `otel`    | Enables `IntrospectionLogs` and `IntrospectionSpanProcessor`       |
| `openai`  | `async-openai` integration — `traced_chat_completion` and friends (implies `otel`) |
| `testing` | In-memory span exporter and test helpers (implies `otel`)          |

## Three surfaces

### 1. `IntrospectionClient` — Introspection API (runtimes, tasks, files)

The main Introspection API surface. No OpenTelemetry dependency; just
HTTPS calls to manage runtimes, experiments, tasks, and files, and to
drive the `Runner` SSE stream.

```rust
// cargo add introspection-sdk
use introspection_sdk::{ClientConfig, IntrospectionClient, RunRequest};
use futures::StreamExt;

let client = IntrospectionClient::new(ClientConfig::default())?;
let runner = client.runtime_by_name("customer-agent").await?
    .run(RunRequest::default()).await?;

let mut events = runner.tasks()
    .start_prompt("Say hello in one sentence.").await?
    .into_stream().await?;

while let Some(event) = events.next().await {
    let event = event?;
    println!("[{}] {}", event.event, event.data);
}
```

See [`examples/api/runtimes.rs`](examples/api/runtimes.rs) for a longer
end-to-end walkthrough.

### 2. `IntrospectionLogs` — Analytics events (track, feedback, identify)

Owns its own `SdkLoggerProvider` and emits `track` / `feedback` /
`identify` events as OTLP logs. Fully independent of
`IntrospectionClient` — pass a token / service name / OTLP base URL
straight to the builder.

Requires the `otel` feature.

```rust
use introspection_sdk::otel::{FeedbackOptions, IntrospectionLogs, TrackOptions};

let logs = IntrospectionLogs::builder()
    .token("your-token")
    .service_name("my-service")
    // Optional: override the OTLP collector URL.
    // .base_otel_url("https://otel.introspection.dev")
    .build()
    .unwrap();

// Custom event
logs.track(
    "Button Clicked",
    Some(TrackOptions::new().with_property("button_id", "submit")),
);

// Feedback with baggage-managed context
{
    let _user = logs.set_user_id("user_123");
    let _conv = logs.set_conversation_id("conv_456");

    logs.feedback(
        "thumbs_up",
        FeedbackOptions::new().with_comments("Great response!"),
    );
} // Context cleared automatically when guards drop

logs.shutdown().unwrap();
```

Available baggage guards: `set_user_id`, `set_anonymous_id`,
`set_conversation_id`, `set_previous_response_id`, `set_agent`,
`set_baggage`. Each returns an RAII guard that clears the value when
dropped.

### 3. `IntrospectionSpanProcessor` — Traces (span processors + instrumentors)

A standalone `SpanProcessor` you attach to your own
`SdkTracerProvider`. Sends spans to the Introspection OTLP collector
via HTTP. Composes with Logfire and any other span processors.

Requires the `otel` feature.

```rust
use introspection_sdk::otel::{
    IntrospectionSpanProcessor, SpanProcessorAdvancedOptions, SpanProcessorConfig,
};
use opentelemetry_sdk::trace::SdkTracerProvider;

let processor = IntrospectionSpanProcessor::new(
    SpanProcessorConfig::with_token("your-token"),
).unwrap();

let provider = SdkTracerProvider::builder()
    .with_span_processor(processor)
    // .with_span_processor(other_processor)
    .build();
```

`SpanProcessorAdvancedOptions` lets you override the OTLP collector URL
(`base_otel_url`), add HTTP headers, or inject a custom `SpanExporter`
for tests.

## Higher-level helpers (otel feature)

### Observation API

Instruments LLM calls and pipeline steps as OpenTelemetry spans with
[gen_ai semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/).

```rust
use introspection_sdk::otel::{GenerationUpdate, Observation, ObservationConfig};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;

let provider = SdkTracerProvider::builder().build();
let tracer = provider.tracer("my-app");

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
// span ends when `obs` is dropped
```

Observations nest automatically via OpenTelemetry context propagation.

### `async-openai` integration

Requires the `openai` feature. Wraps `async-openai` chat and responses
calls with automatic span instrumentation.

```rust
use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::types::chat::*;
use introspection_sdk::otel::openai::traced_chat_completion;

let client = Client::with_config(OpenAIConfig::default());
let request = CreateChatCompletionRequest {
    model: "gpt-4o-mini".to_string(),
    messages: vec![/* … */],
    ..Default::default()
};

let response = traced_chat_completion(&tracer, &client, request).await?;
```

Streaming variant `traced_chat_completion_stream` and the
`tracing`-based `tracing_traced_chat_completion` are also available.

## Environment variables

```shell
# Introspection API (IntrospectionClient)
export INTROSPECTION_TOKEN="intro_xxx"
export INTROSPECTION_BASE_API_URL="https://api.introspection.dev"   # optional

# OTel (IntrospectionLogs + IntrospectionSpanProcessor)
export INTROSPECTION_BASE_OTEL_URL="https://otel.introspection.dev" # optional
export INTROSPECTION_SERVICE_NAME="my-service"                      # optional
```

All env values can be overridden programmatically via the matching
builder method or advanced-options struct.

## Documentation

Full documentation is available at [docs.introspection.dev](https://docs.introspection.dev).

## License

Apache-2.0
