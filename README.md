<div align="center">
  <a href="https://introspection.dev">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset=".github/images/logo-dark.svg">
      <source media="(prefers-color-scheme: light)" srcset=".github/images/logo-light.svg">
      <img alt="Introspection" src=".github/images/logo-light.svg" width="30%">
    </picture>
  </a>
</div>

<h4 align="center">The infrastructure for long-horizon vertical agents.</h4>

<div align="center">
  <a href="https://introspection.dev"><img src="https://img.shields.io/badge/website-introspection.dev-blue" alt="Website"></a>
  <a href="https://crates.io/crates/introspection-sdk"><img src="https://img.shields.io/crates/v/introspection-sdk?label=%20" alt="crates.io version"></a>
  <a href="https://www.apache.org/licenses/LICENSE-2.0"><img src="https://img.shields.io/badge/license-Apache%202.0-green" alt="License"></a>
  <a href="https://x.com/IntrospectionAI"><img src="https://img.shields.io/twitter/follow/IntrospectionAI" alt="Follow on X"></a>
</div>

<br>

[Introspection](https://introspection.dev) is the infrastructure for
long-horizon vertical agents, powered by Pi. Define an agent as a
[Recipe](https://pi.recipes) — agents, skills, policies, and evals in plain
source you own in Git — deploy it to a governed per-customer Runtime, and
improve it in production with conversations, observations, judges, and
experiments.

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
| [`IntrospectionSpanProcessor`](#3-introspectionspanprocessor--traces) | Traces: span processor (OTLP traces) | `otel` |

They share no state. Construct the ones you want, configure independently, mix and match.

## Installation

Default install — `IntrospectionClient` only (no OpenTelemetry pulled in):

```toml
[dependencies]
introspection-sdk = "0.13"
```

With logs/traces export:

```toml
[dependencies]
introspection-sdk = { version = "0.13", features = ["otel"] }
```

### Feature flags

| Feature   | Description                                                          |
| --------- | -------------------------------------------------------------------- |
| `otel`    | Enables `IntrospectionLogs` and `IntrospectionSpanProcessor`         |
| `arrow`   | Arrow IPC decode for the telemetry reads (`list_arrow` / `export_arrow`) |
| `testing` | In-memory span exporter and test helpers (implies `otel`)            |

## Three surfaces

### 1. `IntrospectionClient` — Introspection API (runtimes, tasks, files)

The main Introspection API surface. No OpenTelemetry dependency; just
HTTPS calls to read and run runtimes, manage experiments, tasks, and files,
and drive the `Runner` SSE stream.

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
// matching the AG-UI protocol's own taxonomy. Transport frames
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

#### Authenticating without an API key

`introspection_sdk::auth` wraps the Control Plane's `POST /v1/oauth/token`
grants, so server code (CI jobs, hosted-login backends, federation brokers)
does not hand-roll a form-encoded token POST. All three return the same
`OAuthToken`, which carries the `dp_url` the CP resolved for the token's
project — hand that to a browser client so it needs no separate Data Plane
configuration.

```rust
use introspection_sdk::{auth::ServiceAccountTokenParams, IntrospectionClient};

// client_credentials — the headless counterpart to a long-lived API key.
let client = IntrospectionClient::from_service_account(
    ServiceAccountTokenParams::builder()
        .client_id(std::env::var("INTRO_SA_CLIENT_ID")?)
        .client_secret(std::env::var("INTRO_SA_CLIENT_SECRET")?)
        .project(std::env::var("INTRO_PROJECT")?)
        .build()?,
    None,
).await?;
```

`auth::token_exchange` (RFC 8693) trades an end user's partner-IdP token for a
project-scoped token for a federated `customer` member, and
`auth::authorization_code_token` (RFC 6749 / PKCE) completes a hosted-login
callback. No refresh token is issued: re-mint once `expires_in` lapses.

#### Conversations are GenAI spans

Every conversation read — the summary list and the item list/detail — returns
the same object: `GenAiSpan`, an OpenTelemetry span with identity and timing at
the top level and everything else under `attributes`, keyed by its
[GenAI semantic-convention][semconv] name. `gen_ai.request.model` is reached as
`gen_ai.request.model` because that is what the SDK wrote when it created the
span — there is no private dialect of renamed columns to learn.

Two properties are worth knowing before you write against it:

- **The tree is open.** Every attribute node carries an `extra` map, so an
  attribute this SDK release never heard of still arrives and still round-trips.
  The server returns the tree as stored, not as an allow-list.
- **Absent means absent.** Nothing serializes as `null` — a value that is not
  present is a key that is not there. A real `0` is still a `0`.

A summary is the same envelope carrying the latest turn only, with conversation
rollups under `gen_ai.usage.*` (token totals) and `introspection.conversation.*`
(counts with no semantic-convention name). One parser for both reads.

```rust
use introspection_sdk::ConversationListParams;

let conversations = runner.conversations();
let mut pages = conversations.list(&ConversationListParams::default())?;
let page = pages.next_page().await?.unwrap();
for summary in &page.records {
    println!("{:?} {:?}", summary.conversation_id(), summary.request_model());
}
```

[semconv]: https://github.com/open-telemetry/semantic-conventions-genai

#### Conversation items

`runner.conversations().items.list(...)` returns a
`ConversationItemPaginator`. Use it as an async `Stream` to iterate spans
across pages, or call `next_page()` when the OpenAI-style page metadata
(`first_id`, `last_id`, `has_more`, and opaque `next`) is needed:

```rust
use futures::StreamExt;
use introspection_sdk::ConversationItemListParams;

let conversations = runner.conversations();
let mut items = conversations.items.list(
    "conversation-id",
    &ConversationItemListParams {
        limit: Some(100),
        ..Default::default()
    },
)?;

while let Some(item) = items.next().await {
    let span = item?;
    // Reach for attributes by their semantic-convention name, or use the
    // accessors for the common ones.
    println!("{:?} {:?}", span.span_id, span.operation_name());
    for message in span.input_messages() {
        println!("  {}", message.role);
    }
}

// Or preserve page metadata explicitly:
let mut pages = conversations.items.list(
    "conversation-id",
    &ConversationItemListParams::default(),
)?;
while let Some(page) = pages.next_page().await? {
    println!("first={:?} last={:?} next={:?}",
        page.first_id, page.last_id, page.next);
}
```

Pass a returned `next` token into `ConversationItemListParams::next` to resume
from a checkpoint. `first_id` and `last_id` are informational span IDs, not
pagination inputs.

`conversations.items.get(...)` fetches a single item carrying the **full input
history** for that span — unconditionally, with no `include` to remember. That
is the read to fork or resume a conversation from. The only remaining `include`
values are `events` and `resource_attributes`.

#### Complete conversation exports

The Data Plane walks complete exports in 1,000-row storage pages. Typed helpers
parse JSON, trajectory, and (with the `arrow` feature) Arrow responses;
`export_stream` forwards raw chunks without retaining the full response in SDK
memory:

```rust
use futures::TryStreamExt;
use introspection_sdk::{ConversationExportFormat, ConversationExportParams};

let conversations = runner.conversations();
let params = ConversationExportParams::default();
let mut bytes = conversations
    .export_stream("conversation-id", ConversationExportFormat::Trajectory, &params)
    .await?;
while let Some(chunk) = bytes.try_next().await? {
    destination.write_all(&chunk).await?;
}

let spans = conversations.export_json("conversation-id", &params).await?;
let trajectory = conversations.export_trajectory("conversation-id", &params).await?;
```

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

The `introspection.reconnect` marker rides the protocol's `CUSTOM` channel, so
it needs no transport-specific handling.

#### Retries (429 / 5xx)

Unary calls auto-retry on transient, retryable statuses with a capped-exponential
backoff (the server's `Retry-After` is honoured as a floor when present; absent,
it's pure exponential — the retry happens either way):

- **`429 Too Many Requests`** — retried for **every** method (the request was
  rejected, not processed, so re-sending is safe even for writes). Covers
  `tasks.get` (status polling), lists, create, cancel, delete, file metadata.
- **`502` / `503` / `504`** — retried for **GET only** (idempotent reads), since
  re-sending a non-idempotent write on a transient gateway error isn't safe.

`Retry-After` is understood in both RFC 9110 forms — delta-seconds and an
HTTP-date.

Retries are bounded (`HttpConfig::max_retries`, default 2); once the budget is
spent the status surfaces as a normal `IntrospectionAPIError::Http { status, .. }`
so the caller can inspect it and back off further. The error carries what the
response said: `status()`, `code()` (the DP's machine-readable code — a `401`
with `runner_expired` means refresh the runner, not rotate the key),
`request_id()`, `body()`, and `retry_after()` for scheduling your own retry
with the server's number. Streaming has its own resume budget (above);
multipart uploads are not auto-retried.

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

When you are starting a conversation rather than continuing one, take
the id from `conversation`, which mints one in the same
`intro_conv_<hex>` shape the rest of the platform uses:

```rust,no_run
let (conversation_id, _scope) = logs.conversation(None);
logs.track("Turn Completed", None);
// `conversation_id` is what to record feedback against later.
```

### 3. `IntrospectionSpanProcessor` — Traces

A standalone `SpanProcessor` you attach to your own
`SdkTracerProvider`. Sends spans to the Introspection OTLP collector
via HTTP. Composes with any other span processors on the same provider.

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

## Environment variables

```shell
# Introspection API (IntrospectionClient)
export INTROSPECTION_TOKEN="intro_xxx"
export INTROSPECTION_BASE_API_URL="https://api.introspection.dev"   # optional

# Development only: route this process's tasks to your own `introspection dev`
# server when several developers share one Runtime. No default — see below.
export INTROSPECTION_DEV_TARGET="roland"                            # optional

# OTel (IntrospectionLogs + IntrospectionSpanProcessor)
export INTROSPECTION_BASE_OTEL_URL="https://otel.introspection.dev" # optional
export INTROSPECTION_SERVICE_NAME="my-service"                      # optional
```

All env values can be overridden programmatically via the matching
builder method or advanced-options struct.

### Sharing a Runtime with another developer

When two people run `introspection dev` against one Runtime, a task created by
a shared application credential carries no developer, so the platform cannot
tell their machines apart. Name one:

```shell
# introspection dev prints the line to copy
export INTROSPECTION_DEV_TARGET="roland"
```

The SDK reads it and sends it as a request header on every call, so this
process's tasks reach that dev server — prompts, working tree, and local MCP
servers. There is no default: a target names *someone else's* machine, and
guessing it from the local username would be right on a laptop and quietly
wrong in a shared development deployment. Set it explicitly, or leave it unset
and keep today's behaviour.

It travels as a header, not on `caller`. `caller` stays what it is documented
to be: descriptive metadata you attach to a session that the platform never
acts on. Nothing changes outside the development environment, where the value
is ignored.

## Documentation

Full documentation is available at [docs.introspection.dev](https://docs.introspection.dev).

## License

Apache-2.0
