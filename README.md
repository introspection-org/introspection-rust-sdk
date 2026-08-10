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

This is the Rust SDK: run tasks against a deployed runtime, stream their
output, and record what users thought of the result.

## Install

```toml
[dependencies]
introspection-sdk = "0.13"
```

| Feature | Adds |
| --- | --- |
| `otel` | `IntrospectionLogs` and `IntrospectionSpanProcessor` |
| `arrow` | Arrow IPC decode for the telemetry reads |
| `testing` | In-memory span exporter and test helpers (implies `otel`) |

## Run a task

```rust
use futures::StreamExt;
use introspection_sdk::{AgUiEvent, ClientConfig, IntrospectionClient, RunRequest};

let client = IntrospectionClient::new(ClientConfig::default())?; // token from INTROSPECTION_TOKEN
let runner = client
    .runtime("customer-agent")
    .await?
    .run(RunRequest::default())
    .await?;

let mut events = runner
    .tasks()
    .start_prompt("Say hello in one sentence.")
    .await?
    .into_stream()
    .await?;

while let Some(event) = events.next().await {
    if let AgUiEvent::TextMessageContent(e) = event? {
        print!("{}", e.delta);
    }
}
```

Or wait for the finished answer instead of streaming:

```rust
let handle = runner.tasks().start_prompt("Summarize my open tickets.").await?;
println!("{}", handle.text().await?);
```

Continue the same task with a follow-up run:

```rust
use introspection_sdk::{TaskPrompt, TaskRunCreate, TaskRunKind};

let follow_up = runner.tasks().runs.create(
    &task_id,
    &TaskRunCreate {
        kind: Some(TaskRunKind::Prompt),
        prompt: Some(TaskPrompt {
            text: "Now draft the reply.".into(),
            images: None,
        }),
        ..Default::default()
    },
).await?;
println!("{}", follow_up.text().await?);
```

Unknown future AG-UI event types surface as `AgUiEvent::Unknown` rather than
ending the stream.

## Record feedback

Enable the `otel` feature, then attach the outcome to the conversation the
agent produced:

```toml
introspection-sdk = { version = "0.13", features = ["otel"] }
```

```rust
use introspection_sdk::otel::{FeedbackOptions, IntrospectionLogs, TrackOptions};

let logs = IntrospectionLogs::builder()
    .service_name("support-api")
    .build()?;

logs.track("case_closed", Some(TrackOptions::new().with_property("source", "web")));

{
    let _user = logs.set_user_id("user_123");
    let _conversation = logs.set_conversation_id(&conversation_id);

    logs.feedback(
        "thumbs_up",
        FeedbackOptions::new().with_comments("The answer solved it"),
    );
} // guards clear the context when they drop

logs.shutdown()?;
```

`feedback` records how a result landed, `track` records a product event, and
`identify` attaches who it was. To export your own spans, attach
`IntrospectionSpanProcessor` to an `SdkTracerProvider`; spans in the
OpenTelemetry GenAI semantic conventions are exported as they are.

## Read what happened

A finished task leaves a durable conversation:

```rust
use introspection_sdk::ConversationListParams;

let conversations = runner.conversations();
let mut pages = conversations.list(&ConversationListParams::default())?;

while let Some(page) = pages.next_page().await? {
    for summary in &page.records {
        println!("{} {} tokens", summary.id, summary.usage.total_tokens);
    }
}
```

The runner also exposes `files()`, `shares()`, `events()`, and `metrics()`. See
[`examples/`](examples/) for end-to-end programs.

## Environment variables

```shell
export INTROSPECTION_TOKEN="intro_xxx"
export INTROSPECTION_SERVICE_NAME="my-service"   # optional
```

## Documentation

Full documentation is available at [docs.introspection.dev](https://docs.introspection.dev).

## License

Apache-2.0
