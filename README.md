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

[Introspection](https://introspection.dev) is the infrastructure for
long-horizon vertical agents, powered by Pi. Define an agent as a
[Recipe](https://pi.recipes) — agents, skills, policies, and evals in plain
source you own in Git — deploy it to a governed per-customer Runtime, and
improve it in production with conversations, observations, judges, and
experiments.

This is the Rust SDK: run tasks against a deployed runtime, stream their
output, and record what users thought of the result.

## Install

```shell
cargo add introspection-sdk
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

See [Tasks and streaming](https://docs.introspection.dev/sdk/rust/tasks-and-streaming) for reconnects,
interrupts, and cancellation.

## Record feedback

Enable the `otel` feature, then attach the outcome to the conversation the
agent produced:

```shell
cargo add introspection-sdk --features otel
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

See [Product signals](https://docs.introspection.dev/sdk/rust/product-signals) for the full surface.

## Read what happened

A finished task leaves a durable conversation. Add immutable, filter-only
metadata when creating the task, then use the same keys to find it later:

```rust
use std::collections::HashMap;
use introspection_sdk::{ConversationListParams, TaskCreate};

runner.tasks().create(&TaskCreate {
    prompt: Some("Handle this checkout".into()),
    conversation_metadata: Some(HashMap::from([
        ("flow".into(), "checkout".into()),
        ("tenant".into(), "acme".into()),
    ])),
    ..Default::default()
}).await?;

let conversations = runner.conversations();
let mut pages = conversations.list(&ConversationListParams {
    metadata: Some(HashMap::from([("flow".into(), "checkout".into())])),
    ..Default::default()
})?;

while let Some(page) = pages.next_page().await? {
    for summary in &page.records {
        println!("{} {} tokens", summary.id, summary.usage.total_tokens);
    }
}
```

The runner also exposes `files()`, `shares()`, `events()`, and `metrics()`.

Every list-params struct carries a `filters: Option<HashMap<String, serde_json::Value>>`
passthrough for a query parameter this SDK build predates: each pair goes on
the wire verbatim (an array as a repeated key), so a new server-side filter
is usable the day it ships, without waiting for a typed field here.

## Curate traces with human review

Annotations are append-only events on an OTel trace/span. Each write changes
exactly one dimension; label and reviewer vectors are complete snapshots, so
an empty vector clears that dimension.

```rust
use introspection_sdk::{
    AdvancedOptions, AnnotationEventOptions, AnnotationMutation,
    AnnotationTarget, ClientConfig, IntrospectionClient,
};

let client = IntrospectionClient::new(
    ClientConfig::with_token(member_access_token).advanced(AdvancedOptions {
        base_api_url: Some("https://api.introspection.dev".into()),
        dp_url: Some("https://dp.example".into()),
        cp_session: Some(encoded_member_session),
        ..Default::default()
    }),
)?;

client.annotations().create(
    AnnotationTarget {
        trace_id: "0af7651916cd43dd8448eb211c80319c".into(),
        span_id: "b7ad6b7169203331".into(),
    },
    AnnotationMutation::ReviewerEmails(vec!["expert@example.com".into()]),
    AnnotationEventOptions::default(),
).await?;
```

Reusable labels live under `client.project_labels()`. Their slug and color are
immutable after creation; only the optional description can be updated.

See [Production evidence](https://docs.introspection.dev/sdk/rust/production-evidence) for transcripts,
typed events, and metrics queries, [Files and shares](https://docs.introspection.dev/sdk/rust/files-and-shares)
for durable inputs and grants, and [`examples/`](examples/) for end-to-end
programs.

## Read a repository's files

`client.repositories()` resolves a repository on the Control Plane;
`contents(id)` reads its files through the Data Plane (`repositories:read`).

```rust
use futures::StreamExt;
use introspection_sdk::{ContentsQuery, RepositoryContent, RepositoryListParams};

let repository = client.repositories().list(&RepositoryListParams {
    project: Some("acme".into()),
    slug: Some("support-triage".into()),
    ..Default::default()
}).await?.remove(0);
let contents = client.repositories().contents(repository.id);

// Every entry of a directory, following the cursor; errors if the path is a file.
let mut entries = contents.list(&ContentsQuery { path: "agents".into(), ..Default::default() })?;
while let Some(entry) = entries.next().await {
    let entry = entry?;
    println!("{} {}", entry.entry_type.as_str(), entry.path);
}

// One file (or a directory's first page), at a branch, tag or commit.
let query = ContentsQuery { r#ref: Some("main".into()), ..Default::default() };
if let RepositoryContent::File(file) = contents.get("agents/agent.yaml", &query).await? {
    println!("{} ({})", file.content, file.encoding);
}
```

`commits(id, query)` walks a repository's history, newest first, and
`commit(id, sha)` reads one commit with its changed files and unified diff.

```rust
use introspection_sdk::CommitsQuery;

let query = CommitsQuery { path: Some("agents/agent.yaml".into()), ..Default::default() };
let mut commits = client.repositories().commits(repository.id, &query)?;
while let Some(commit) = commits.next().await {
    let commit = commit?;
    println!("{} {}", &commit.sha[..7], commit.message.lines().next().unwrap_or(""));
}

let detail = client.repositories().commit(repository.id, "main").await?;
for file in &detail.files {
    println!("{} {} +{} -{}", file.status.as_str(), file.filename, file.additions, file.deletions);
}
```

## Environment variables

```shell
export INTROSPECTION_TOKEN="intro_xxx"
export INTROSPECTION_SERVICE_NAME="my-service"   # optional
```

## Documentation

- [Rust quickstart](https://docs.introspection.dev/sdk/rust/quickstart)
- [Tasks and streaming](https://docs.introspection.dev/sdk/rust/tasks-and-streaming)
- [Files and shares](https://docs.introspection.dev/sdk/rust/files-and-shares)
- [Production evidence](https://docs.introspection.dev/sdk/rust/production-evidence)
- [Product signals](https://docs.introspection.dev/sdk/rust/product-signals)
- [Platform operations](https://docs.introspection.dev/sdk/rust/platform-operations)
- [Rust SDK reference](https://docs.introspection.dev/sdk/rust/reference)
- [Authentication](https://docs.introspection.dev/sdk/authentication)

## License

Apache-2.0
