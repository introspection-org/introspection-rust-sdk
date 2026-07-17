<div align="center">
  <a href="https://introspection.dev">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset=".github/images/logo-dark.svg">
      <source media="(prefers-color-scheme: light)" srcset=".github/images/logo-light.svg">
      <img alt="Introspection" src=".github/images/logo-light.svg" width="30%">
    </picture>
  </a>
</div>

<h4 align="center">Deploy vertical agents that improve in production.</h4>

<div align="center">
  <a href="https://introspection.dev"><img src="https://img.shields.io/badge/website-introspection.dev-blue" alt="Website"></a>
  <a href="https://crates.io/crates/introspection-sdk"><img src="https://img.shields.io/crates/v/introspection-sdk?label=%20" alt="crates.io version"></a>
  <a href="https://www.apache.org/licenses/LICENSE-2.0"><img src="https://img.shields.io/badge/license-Apache%202.0-green" alt="License"></a>
  <a href="https://x.com/IntrospectionAI"><img src="https://img.shields.io/twitter/follow/IntrospectionAI" alt="Follow on X"></a>
</div>

<br>

[Introspection](https://introspection.dev) is the managed cloud for vertical
agents, powered by Pi. Define an agent as a recipe, deploy it to a
commit-pinned runtime, and improve it in production with conversations,
patterns, judges, and experiments.

This is the native Rust client for driving Introspection runtimes and tasks,
alongside optional analytics and OpenTelemetry surfaces. Use
`IntrospectionClient` to open a runner against a deployed runtime, start a task,
and stream its output. See the [platform SDK overview](https://docs.introspection.dev/sdk)
for the wider product workflow and the JavaScript, Python, browser, and CLI
clients.

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

The `async-openai` adapter is experimental:

```toml
[dependencies]
introspection-sdk = { version = "0.1", features = ["openai"] }
```

### Feature flags

| Feature   | Description                                                        |
| --------- | ------------------------------------------------------------------ |
| `otel`    | Enables `IntrospectionLogs` and `IntrospectionSpanProcessor`       |
| `openai`  | Experimental `async-openai` support (implies `otel`)            |
| `testing` | In-memory span exporter and test helpers (implies `otel`)          |

## Three surfaces

### 1. `IntrospectionClient` — Introspection API (runtimes, tasks, files)

The main Introspection API surface. No OpenTelemetry dependency; just
HTTPS calls to manage runtimes, experiments, tasks, and files, and to
drive the `Runner` SSE stream.

```rust
// cargo add introspection-sdk
use introspection_sdk::{AgUiEvent, ClientConfig, IntrospectionClient, RunRequest};
use futures::StreamExt;

let client = IntrospectionClient::new(ClientConfig::default())?;
let runner = client.runtime("customer-agent").await?
    .run(RunRequest {
        agent_name: Some("support-agent".into()),
        scope: Some("customer:acme".into()),
        ..Default::default()
    }).await?;

let mut events = runner.tasks()
    .start_prompt("Say hello in one sentence.").await?
    .into_stream().await?;

// `stream()` yields typed AG-UI protocol events (see `introspection_sdk::agui`),
// matching the JS (`@ag-ui/core`) and Python SDKs. Transport frames
// (heartbeats) are handled internally; an unknown future event type surfaces
// as `AgUiEvent::Unknown` rather than failing the stream.
while let Some(event) = events.next().await {
    if let AgUiEvent::TextMessageContent(e) = event? {
        print!("{}", e.delta);
    }
}
```

`RunRequest` also accepts `identity`, `caller`, and `ttl_seconds`. The resolved
runner context includes the runtime or experiment selection, runtime group,
flat recipe revision fields, agent name, identity, and caller.

Existing bodyless `handle.cancel().await` remains supported and aborts
immediately. Pass typed options with
`handle.cancel_with(&TaskCancelOptions::Abort).await` for explicit abort or
`handle.cancel_with(&TaskCancelOptions::Drain { ... }).await` for graceful
teardown. `TaskCancelOptions::default()` is abort. Interrupted runs resume with
`runner.tasks().runs.resume(...)`. Rust runners also expose `runner.shares()`
for file and conversation sharing grants.

See [`examples/api/runtimes.rs`](examples/api/runtimes.rs) for a longer
end-to-end walkthrough.

#### Resilient streaming

`stream()` resumes **transparently** across a mid-turn disconnect — gateway
idle-timeout, load-balancer recycle, network blip. On a drop it re-attaches with
the SSE-standard `Last-Event-ID` so the server replays the frames the client
missed, yielding one gap-free `Stream` of `AgUiEvent`. There is no
consumer-visible change: the loop above just keeps working, ending when the turn
finishes and yielding a terminal `Err` only once recovery is exhausted. Readiness
folds in the same way — while a run is not yet attachable the server answers with
`429` + `Retry-After`, which the stream honours as a backoff floor and retries,
never surfaced to the caller.

Use `stream_with` to tune the recovery bounds, or to opt into an
`introspection.reconnect` `CUSTOM` event on each reconnect / readiness wait (off
by default — the stream is otherwise fully transparent):

```rust
use introspection_sdk::{AgUiEvent, StreamOptions};
use introspection_sdk::agui::introspection::RECONNECT_EVENT_NAME;
use std::time::Duration;

let stream = runner.tasks().runs.stream_with(
    &task_id,
    &run_id,
    StreamOptions {
        max_reconnects: 5,
        timeout: Duration::from_secs(300),
        emit_reconnect_events: true,
        ..Default::default()
    },
);
futures::pin_mut!(stream);
while let Some(event) = stream.next().await {
    match event? {
        AgUiEvent::TextMessageContent(e) => print!("{}", e.delta),
        AgUiEvent::Custom(c) if c.name == RECONNECT_EVENT_NAME => {
            eprintln!("reconnecting… ({})", c.value["reason"]);
        }
        _ => {}
    }
}
```

The same `introspection.reconnect` marker rides the `CUSTOM` channel in the JS
and Python SDKs, so it is expressible identically across all three.

#### Retries (429 / 5xx)

Unary calls auto-retry on transient, retryable statuses with a capped-exponential
backoff (the server's `Retry-After` is honoured as a floor when present; absent,
it's pure exponential — the retry happens either way):

- **`429 Too Many Requests`** — retried for **every** method (the request was
  rejected, not processed, so re-sending is safe even for writes). Covers
  `tasks.get` (status polling), lists, create, cancel, delete, file metadata.
- **`502` / `503` / `504`** — retried for **GET only** (idempotent reads), since
  re-sending a non-idempotent write on a transient gateway error isn't safe.

Retries are bounded (`HttpConfig::max_retries`, default 2); once the budget is
spent the status surfaces as a normal `IntrospectionAPIError::Http { status, .. }`
so the caller can inspect it and back off further. Streaming has its own resume
budget (above); multipart uploads are not auto-retried.

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

> **Experimental integration.**

Requires the `openai` feature. Wraps `async-openai` calls with span
instrumentation.

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
