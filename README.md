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

## Label your customers

Asserting an identity on `run` mints (or finds) the `customer` member for that
end user. Attach `metadata` to it there, then read members back and filter by
it:

```rust
use std::collections::HashMap;
use futures::StreamExt;
use introspection_sdk::{MemberListParams, MemberUpdateParams, RunRequest, RunnerIdentity};

let runner = client.runtime("customer-agent").await?.run(RunRequest {
    identity: Some(RunnerIdentity {
        user_id: Some("u_123".into()),
        metadata: Some(HashMap::from([("plan".into(), "enterprise".into())])),
        ..Default::default()
    }),
    ..Default::default()
}).await?;

let mut members = client.members().list(&MemberListParams {
    metadata: Some(HashMap::from([("plan".into(), "enterprise".into())])),
    ..Default::default()
});
while let Some(member) = members.next().await {
    let member = member?;
    println!("{:?} {:?}", member.external_user_id, member.metadata);
}

client.members().update(member_id, &MemberUpdateParams {
    metadata: Some(HashMap::new()), // replaces the whole map; empty clears it
    ..Default::default()
}).await?;
```

Metadata grants nothing. An assertion merges its keys into an existing
member's metadata: asserted keys overwrite keys with the same name and other
keys stay. An absent or empty map changes nothing. `tags` are different
because they grant access. A member can read and write every file and task
whose tags intersect its own, so a `RunnerIdentity` sets tags only on a member
it creates, and setting tags through `members()` requires `members:manage`.
The list filters are `tag` (one tag) and `metadata` (up to 16 pairs, all of
which must match). The members routes are Control Plane routes, so they need
an org credential with `members:read` / `members:write` / `members:manage`.

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

`merge(id, &RepositoryMergeCreate)` merges a branch or commit into a branch,
like GitHub's merges API (`repositories:write`). `None` means `base` already
contains `head`; a conflict is a `409` error and leaves the branch unchanged.

```rust
use introspection_sdk::RepositoryMergeCreate;

let merge = RepositoryMergeCreate {
    base: "main".into(),
    head: "feature/triage".into(),
    commit_message: None, // "Merge feature/triage into main"
};
match client.repositories().merge(repository.id, &merge).await? {
    Some(commit) => println!("merged as {}", commit.sha),
    None => println!("already up to date"),
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

## Stream recovery

Run streams request replay from cursor `0`, including output produced before the
first connection. Only a settling `RUN_FINISHED` or `RUN_ERROR` confirms completion;
`RUN_FINISHED` with `result.reason = "stream_close"` is suppressed. A nonterminal
EOF checks the specific run's status and reconnects within the recovery budget.
Each new content cursor renews both the timeout window and the reconnect budget.
Lifecycle events, heartbeats and duplicate content renew neither. The timeout is
checked when recovery is needed; it does not interrupt an open connection.

Every reconnect resumes from the last content cursor (`Last-Event-ID`). When
that cursor is older than the server's replay buffer, the stream continues with
one AG-UI `MESSAGES_SNAPSHOT` holding the run's messages so far; its id becomes
the new cursor, and the text helper takes its assistant text in place of what it
had read. When the server holds neither the frames nor a snapshot, it answers
`410` and the stream ends with an incomplete-output error. Runtime images that
predate the snapshot send `CUSTOM resume_gap` instead; raw streams pass it
through. The text helper raises an incomplete-output error instead of returning
partial text, including on `resume_gap`; it also raises on run failure or
cancellation. If the status read
says the run settled but the stream never confirmed completion, it raises an
incomplete-output error. Recover final output from the conversation transcript
when needed; the SDK does not automatically hydrate it or require an additional
`conversations:read` scope just to stream. A long stream can therefore
reconnect after its original timeout as long as content has continued to advance.

Use a concrete run ID when consuming one turn. `runs/current` is a moving alias: a
reconnect or status read may resolve to the next turn if another run has started.

The in-process fake sandbox (`mock://`) supplies replies through the conversation
transcript, not SSE. Its attach-only `stream_close` cannot satisfy `.text()`; use
transcript reads for fake-sandbox tests, or a real runtime for `.text()` tests.

The shared `run-stream-contract.json` fixtures pin these behaviors across Swift,
JavaScript, Rust and Python. Each test suite pins the fixture SHA-256; intentional
contract changes must update all four copies and their expected hashes together.

Rust exposes `IntrospectionAPIError::StreamIncomplete` and `IntrospectionAPIError::RunFailed`.

## License

Apache-2.0
