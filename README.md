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

## The same Data Plane calls on a client or a runner

`IntrospectionClient` and `Runner` expose one Data Plane surface, enforced by
the `DataPlaneResources` trait: `tasks()` (with `.runs`), `files()`,
`shares()`, `conversations()`, `events()`, `metrics()`, `automations()`,
`issues()` and `connections()`. On the client they act with the client's token; on a runner, with
its session token. Both carry the methods inherently, so a call needs no
import, and generic code can take either:

```rust
use futures::StreamExt;
use introspection_sdk::{DataPlaneResources, IssueListParams, IssueStatus};

async fn open_issue_titles(dp: &impl DataPlaneResources) -> Vec<String> {
    let mut issues = dp.issues().list(&IssueListParams {
        status: vec![IssueStatus::Open],
        ..Default::default()
    });
    let mut titles = Vec::new();
    while let Some(Ok(issue)) = issues.next().await {
        titles.push(issue.title);
    }
    titles
}

open_issue_titles(&client).await;
open_issue_titles(&runner).await;
```

The token's scopes decide which calls succeed. A runner a member opens for
themself (no asserted `identity`, no explicit `scope`) carries
`automations:read` / `automations:write`, `connections:read` /
`connections:write` / `connections:delete` and `issues:read` / `issues:write`
on top of the sandbox set (tasks, files, shares, conversations, events,
metrics). A runner opened for an asserted end customer carries the sandbox set,
or what its `RunRequest::scope` asks for.

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

`feedback` records how a result landed, `track` records a product event
(see [Logging custom events](#logging-custom-events) for `log_event`), and
`identify` attaches who it was. To export your own spans, attach
`IntrospectionSpanProcessor` to an `SdkTracerProvider`; spans in the
OpenTelemetry GenAI semantic conventions are exported as they are.

See [Product signals](https://docs.introspection.dev/sdk/rust/product-signals) for the full surface.

## Logging custom events

`log_event(name, attributes, options)` records an app event under a name you
choose — `ark.feed.entry`, `checkout.completed` — so the platform stores it and
you can read it back by name. `track` is a thin alias of it. Attributes are
stored under `properties.*`, exactly as `track` stores them.

```rust
use std::collections::HashMap;
use introspection_sdk::otel::{
    IntrospectionLogs, LogEventIdentity, LogEventOptions, LogEventSeverity, PropertyValue,
};

let logs = IntrospectionLogs::builder().service_name("ark").build()?; // INTROSPECTION_TOKEN

logs.log_event(
    "ark.feed.entry",
    Some(HashMap::from([
        ("entry_id".to_string(), PropertyValue::from(entry.id.as_str())),
        ("source".to_string(), PropertyValue::from("rss")),
        ("score".to_string(), PropertyValue::from(0.92)),
    ])),
    LogEventOptions::new()
        .with_event_id(format!("feed-entry:{}", entry.id)) // stable id: consumers dedupe on it
        .with_timestamp(entry.published_at)                 // SystemTime; default now
        .with_identity(LogEventIdentity::new().with_user_id(&entry.owner_id)) // overrides the scoped identity
        .with_severity(LogEventSeverity::Info),             // Debug | Info | Warn | Error
)?;

logs.shutdown()?; // flushes
```

- **Names.** Use your own namespace. Names starting with `introspection.`
  (platform events) or `gen_ai.` (OpenTelemetry GenAI conventions) are
  reserved: `log_event` returns `LogEventError::ReservedName` for them and
  `LogEventError::EmptyName` for `""`, and emits nothing. The prefixes are
  exported as `RESERVED_EVENT_NAME_PREFIXES`. `track` delegates to `log_event`,
  so it drops those names too, with a warning, since it returns no error.
- **Idempotency.** Without an event id each call gets a fresh one. When
  delivery can repeat (retries, replayed jobs, an agent re-running a step),
  pass an id derived from the thing being recorded: re-sending the same id is
  how consumers recognise and drop the duplicate.
- **Identity and context.** User / anonymous id and `gen_ai.conversation.id` /
  agent come from the active baggage guards (`set_user_id`,
  `set_conversation_id`, `set_agent`); `LogEventIdentity` overrides per field.

### From a recipe or agent sandbox

A Runtime sandbox is started with `INTROSPECTION_TOKEN` and
`INTROSPECTION_BASE_OTEL_URL` in its environment, so a tool or agent step needs
no configuration — build the logger from the environment and flush before the
step returns:

```rust
use std::collections::HashMap;
use introspection_sdk::otel::{IntrospectionLogs, LogEventOptions, PropertyValue};

fn record_feed_entries(entries: &[FeedEntry]) -> Result<(), Box<dyn std::error::Error>> {
    // Token and collector URL come from the sandbox environment.
    let events = IntrospectionLogs::builder().service_name("ark-recipe").build()?;
    for entry in entries {
        events.log_event(
            "ark.feed.entry",
            Some(HashMap::from([
                ("entry_id".to_string(), PropertyValue::from(entry.id.as_str())),
                ("title".to_string(), PropertyValue::from(entry.title.as_str())),
                ("url".to_string(), PropertyValue::from(entry.url.as_str())),
            ])),
            LogEventOptions::new().with_event_id(format!("ark.feed.entry:{}", entry.id)),
        )?;
    }
    events.shutdown()?; // the sandbox may be torn down after the step
    Ok(())
}
```

### Reading custom events back

Custom events are served by `/v1/events` under the `introspection.track`
family, whose `payload` carries the original `name` and the `properties`. This
SDK build has no typed variant for that family, so request it by its wire name
and read the row from `Event::Unknown`:

```rust
use introspection_sdk::{Event, EventListParams, IntrospectionEventName};

let mut pages = runner.events().list(&EventListParams {
    lookback: Some("24h".into()),
    ..EventListParams::new(IntrospectionEventName::Unknown("introspection.track".into()))
})?;

while let Some(page) = pages.next_page().await? {
    for event in &page.records {
        let Event::Unknown(row) = event else { continue };
        if row["payload"]["name"] != "ark.feed.entry" {
            continue;
        }
        println!("{} {}", row["timestamp"], row["payload"]["properties"]["entry_id"]);
    }
}
```

Filtering by `name` server-side is not available yet — it arrives with a
pending platform change. Until then, filter on `payload.name` client-side as
above.

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

## Schedule automations

An automation runs a prompt as an agent task on a schedule, or once at a set
time. Set `task_id` to post each firing into an existing task instead of
creating a new one. A one-off reminder is a `Manual` automation with a future
`next_trigger_at`:

```rust
use introspection_sdk::{
    AutomationCreateParams, AutomationListParams, AutomationMetadata, AutomationTriggerType,
    AutomationUpdateParams, EventListParams, IntrospectionEventName,
};

let reminder = client.automations().create(&AutomationCreateParams {
    prompt: Some("Follow up on the open invoice.".into()),
    runtime_group_id: Some(runtime_group_id),
    task_id: Some(task_id),
    next_trigger_at: Some("2026-10-06T09:00:00+10:00".into()),
    ..AutomationCreateParams::new("Invoice reminder", AutomationTriggerType::Manual)
}).await?;

// Only the fields you set are sent; `metadata` replaces the stored object.
client.automations().update(reminder.id, &AutomationUpdateParams {
    metadata: Some(AutomationMetadata {
        timezone: Some("Australia/Sydney".into()),
        ..Default::default()
    }),
    ..Default::default()
}).await?;

let run = client.automations().trigger(reminder.id).await?;
println!("{} {:?}", run.status, run.task_id);

// The automations that post into one task, and what each firing did.
let mut automations = client.automations().list(&AutomationListParams {
    task_id: Some(task_id),
    ..Default::default()
});
let fired = client.events().list(&EventListParams {
    automation_id: Some(reminder.id),
    ..EventListParams::new(IntrospectionEventName::AutomationTriggered)
})?;
```

`Automation::kind` is `None` for an automation a person created. A platform
automation carries its kind: `ProjectCheckIn`, `ObservationSynthesis` or
`ObservationClustering`. A kind added later decodes as `AutomationKind::Other`.
A scheduled firing that ran nothing is recorded as an
`introspection.automation.skipped` event with an `AutomationSkipReason`.

`runner.automations()` is the same API on the runner's token. These are Data
Plane routes (`automations:read` / `automations:write`), which a runner a
member opens for themself carries. The API serves them to administrators only
today and answers 403 to anyone else. introspection-cloud#3137 opens them to
members for their own automations that post into one of their own tasks. The `task_id` list filter arrives with that
change, so it is not served yet.

## Connect a member's apps

`connections()` is the apps (Gmail, Slack, …) members connected for
themselves; the agent acts with them in that member's sessions. Creating one
returns a connect page to hand the member. On a runner, `runtime` defaults to
the runner's runtime group; on the client, set it:

```rust
use futures::StreamExt;
use introspection_sdk::{MemberConnectionCreate, MemberConnectionListParams};

let page = runner.connections().create(&MemberConnectionCreate::new("gmail")).await?;
println!("open {} within {}s", page.authorize_url, page.expires_in);

let page = client.connections().create(&MemberConnectionCreate {
    runtime: Some("customer-agent".into()),
    ..MemberConnectionCreate::new("gmail")
}).await?;

let mut connections = runner.connections().list(&MemberConnectionListParams {
    app: Some("gmail".into()),
    ..Default::default()
});
while let Some(connection) = connections.next().await {
    let connection = connection?;
    if !connection.healthy {
        runner.connections().delete(connection.id).await?;
    }
}
```

A caller who is not an administrator only ever lists, reads and removes their
own connections; an administrator can narrow the list with `member_id`. These
are Data Plane routes gated on `connections:read`, `connections:write` and
`connections:delete`. They are distinct from `client.connectors()`, whose
connections an integrator administers for the project.

## Track issues

An issue is a project pursuit with a living brief, a fixed worker task, and
the human requests raised on it:

```rust
use introspection_sdk::{
    IssueCreate, IssueListParams, IssueOwner, IssuePriority, IssueStatus, IssueUpdate,
};

let issue = runner.issues().create(&IssueCreate {
    priority: Some(IssuePriority::High),
    tags: vec!["customer:acme".into()],
    ..IssueCreate::new("Checkout retries double-charge", "Customers on retry see two charges.", task_id)
}).await?;

// Edits are made against the revision you read; a stale one answers 409.
let issue = runner.issues().update(issue.id, &IssueUpdate {
    status: Some(IssueStatus::Closed),
    ..IssueUpdate::new(issue.revision)
}).await?;

let mut mine = runner.issues().list(&IssueListParams {
    owner: vec![IssueOwner::Me],
    status: vec![IssueStatus::Open, IssueStatus::Waiting],
    ..Default::default()
});
```

`update_request` creates or changes one human request on an issue
(`PATCH /v1/issues/{id}` with `{"request": ...}`). List filters that take a
list are sent as repeated keys and ORed; every filter only narrows. These are
Data Plane routes gated on `issues:read`, `issues:write` and `issues:delete`.

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

## Sign in

Each [Application](https://docs.introspection.dev/sdk/authentication) type
offers one way in. The type is chosen when the Application is created, the
types are mutually exclusive, and the grants follow from the type:

| `application_type` | For | Grants | Rust |
| --- | --- | --- | --- |
| `service_account` | Server-to-server (holds a client secret) | `client_credentials` | `service_account_token`, `IntrospectionClient::from_service_account` |
| `jwks` | End users of your own IdP (Supabase, Auth0, Okta, ...) | token exchange of the IdP's JWT | `token_exchange` |
| `spa` | Introspection-hosted login in a browser, optionally brokered | `authorization_code` + PKCE, `refresh_token` | `authorization_code_token` |
| `native` | Mobile and desktop apps that draw their own sign-in screens | `email_code`, `device_code`, `refresh_token` | `EmailCodeAuth` |

An app that needs two ways in registers two Applications. Every token's scopes
are capped by its Application's `allowed_scopes`. A `service_account` token
works on Control Plane routes; the end-user tokens (`jwks`, `spa`, `native`)
belong to a `customer` member and are Data Plane credentials, so Control Plane
routes such as `/v1/runtimes` answer them with `401`. The `native` type's
`device_code` grant is not reachable with an Application yet.

A `native` Application signs its users in with an emailed code:

```rust
use introspection_sdk::auth::{EmailCodeAuth, EmailCodeAuthConfig};
use introspection_sdk::{DataPlaneResources, TaskListParams};

let auth = EmailCodeAuth::new(
    EmailCodeAuthConfig::builder()
        .client_id("intro_app_...")
        .project("acme")
        .build()?,
)?;

auth.send_code("user@example.com").await?;
let session = auth.verify_code("user@example.com", &code_the_user_typed).await?;

let page = auth
    .with_data_plane(|dp| async move {
        dp.tasks().list(&TaskListParams::default()).next_page().await
    })
    .await?;
```

- The client `with_data_plane` hands `op` implements `DataPlaneResources`, so
  every Data Plane namespace (`tasks()`, `issues()`, `connections()`, …) works
  on it. It has no runtime context, so `connections().create` needs `runtime`.

- A returning user's code is six digits. A new user's first code is six
  characters of `A-Z` and `0-9`, so accept letters in the code field.
- `with_data_plane` refreshes the access token before it expires, and once more
  after a `401`. Concurrent callers share one refresh. A refresh the server
  rejects (`invalid_grant`) signs the user out; a network failure does not.
- A sign-in that answers after a sign-out or a newer sign-in fails with
  `IntrospectionAPIError::Superseded` and does not replace the current session.
- `AuthSession` is serializable. Persist it from `auth.changes()` (each refresh
  rotates the refresh token) and restore it with `auth.set_session`.
- `sign_out` clears the local session first, then revokes it on the Control
  Plane.

See [`examples/api/native_email_code.rs`](examples/api/native_email_code.rs).

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
