//! Steps 9 to 13: a file in, a task on the runtime, the evidence it left,
//! and the telemetry reads over it.

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt as _;
use introspection_sdk::{
    AccessTagCreate, AnnotationEventOptions, AnnotationListParams, AnnotationMutation,
    AnnotationNavigationParams, AnnotationTarget, Annotations, ApprovalListParams,
    ConversationContains, ConversationExportFormat, ConversationExportParams,
    ConversationItemGetParams, ConversationItemListParams, ConversationListParams, EventListParams,
    File, FileCreateText, FileListParams, FileUpdate, FileUpload, FileVersionCreateText,
    FileVersionListParams, IntrospectionClient, IntrospectionEventName, Lane,
    MemberConnectionCreate, MemberConnectionListParams, MemberListParams, MetricFilter, MetricSpec,
    MetricsQuery, MissionListParams, RunFrame, RunRequest, ShareCreate, ShareListParams,
    ShareResourceType, StreamOptions, TagCreate, TagKind, TagListParams, TagUpdate, Task,
    TaskCreate, TaskFileRef, TaskListParams, TaskPrompt, TaskRunCreate, TaskStatus, TaskUpdate,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::steps::expect_not_found;
use crate::support::{assert_stable, settled, span_event_id, walk};
use crate::{set, Walk};

/// How long the input file lives: a day outlasts the walk.
const FILE_TTL_SECONDS: u64 = 24 * 60 * 60;

fn walk_metadata(extra: &[(&str, &str)]) -> HashMap<String, Value> {
    std::iter::once(("suite", "walk"))
        .chain(extra.iter().copied())
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect()
}

pub async fn files_in(w: &mut Walk) {
    let sa = w.sa();
    let input = sa
        .files()
        .create_text(&FileCreateText {
            name: walk::INPUT_FILE_NAME.into(),
            content: walk::INPUT_FILE_TEXT.into(),
            mime_type: Some("text/plain".into()),
            metadata: Some(walk_metadata(&[])),
            tags: None,
            ttl_seconds: Some(FILE_TTL_SECONDS),
        })
        .await
        .expect("upload the input file");
    assert_stable!(input, @r#"
    {
      "created_at": "[timestamp]",
      "expires_at": "[timestamp]",
      "file_type": "filesystem",
      "id": "[uuid]",
      "member_id": "[uuid]",
      "metadata": {
        "blob_checksum": "[digest]",
        "original_filename": "walk-input.txt",
        "suite": "walk"
      },
      "mime_type": "text/plain",
      "name": "walk-input.txt",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "size_bytes": "[number]",
      "storage_path": "[uuid]/[uuid]/files/[uuid]/walk-input.txt",
      "tags": [],
      "updated_at": "[timestamp]",
      "version": 1
    }
    "#);
    let id = input.id.to_string();
    let read = sa.files().get(&id).await.expect("read the file");
    assert_eq!(read.name, walk::INPUT_FILE_NAME);
    let body = sa.files().download(&id).await.expect("download the file");
    assert_eq!(body.as_ref(), walk::INPUT_FILE_TEXT.as_bytes());
    let version = sa
        .files()
        .versions
        .create(
            &id,
            FileUpload::from_bytes(
                format!("{} (v2)", walk::INPUT_FILE_TEXT).into_bytes(),
                walk::INPUT_FILE_NAME,
            )
            .with_content_type("text/plain")
            .with_ttl_seconds(FILE_TTL_SECONDS),
        )
        .await
        .expect("add a version");
    let text_version = sa
        .files()
        .versions
        // A version extends the chain's head, which is now the second version.
        .create_text(
            &version.id.to_string(),
            &FileVersionCreateText {
                content: format!("{} (v3)", walk::INPUT_FILE_TEXT),
                mime_type: Some("text/plain".into()),
                metadata: Some(walk_metadata(&[])),
                ttl_seconds: Some(FILE_TTL_SECONDS),
                ..Default::default()
            },
        )
        .await
        .expect("add a text version");
    assert_stable!(text_version, @r#"
    {
      "created_at": "[timestamp]",
      "expires_at": "[timestamp]",
      "file_type": "filesystem",
      "id": "[uuid]",
      "member_id": "[uuid]",
      "metadata": {
        "blob_checksum": "[digest]",
        "original_filename": "walk-input.txt",
        "suite": "walk"
      },
      "mime_type": "text/plain",
      "name": "walk-input.txt",
      "org_id": "[uuid]",
      "parent_id": "[uuid]",
      "project_id": "[uuid]",
      "size_bytes": "[number]",
      "storage_path": "[uuid]/[uuid]/files/[uuid]/walk-input.txt",
      "tags": [],
      "updated_at": "[timestamp]",
      "version": 3
    }
    "#);
    let versions = sa
        .files()
        .versions
        .list(&id, &FileVersionListParams::default())
        .collect_all(0)
        .await
        .expect("list versions");
    assert!(versions.iter().any(|v| v.id == version.id));
    assert!(versions.iter().any(|v| v.id == text_version.id));
    sa.files()
        .versions
        .get(&id, &version.id.to_string())
        .await
        .expect("read the version");
    file_filters(&sa, &input).await;
    w.input_file = Some(input);
}

/// The file found by its metadata, its tag and a substring of its name.
async fn file_filters(sa: &IntrospectionClient, input: &File) {
    let id = input.id.to_string();
    let updated = sa
        .files()
        .update(
            &id,
            &FileUpdate {
                metadata: Some(walk_metadata(&[("stage", "attached")])),
                ..Default::default()
            },
        )
        .await
        .expect("update the file's metadata");
    let stage = updated.metadata.as_ref().and_then(|m| m.get("stage"));
    assert_eq!(stage, Some(&json!("attached")));
    // A file with versions is listed only with `include_versions`.
    let found = |files: Vec<File>| files.iter().any(|f| f.id == input.id);
    let by_metadata = FileListParams {
        metadata: Some(HashMap::from([
            ("suite".to_string(), "walk".to_string()),
            ("stage".to_string(), "attached".to_string()),
        ])),
        include_versions: Some(true),
        ..Default::default()
    };
    let listed = sa
        .files()
        .list(&by_metadata)
        .collect_all(0)
        .await
        .expect("list by metadata");
    assert!(found(listed), "the metadata filter finds the file");
    sa.files()
        .update(
            &id,
            &FileUpdate {
                tags: Some(vec!["suite:walk".into()]),
                ..Default::default()
            },
        )
        .await
        .expect("tag the file");
    let by_tag = FileListParams {
        tag: Some("suite:walk".into()),
        include_versions: Some(true),
        ..Default::default()
    };
    let listed = sa
        .files()
        .list(&by_tag)
        .collect_all(0)
        .await
        .expect("list by tag");
    assert!(found(listed), "the tag filter finds the file");
    let by_name = FileListParams {
        include_versions: Some(true),
        name_contains: Some("WALK-INPUT".into()),
        ..Default::default()
    };
    let listed = sa
        .files()
        .list(&by_name)
        .collect_all(0)
        .await
        .expect("list by name");
    assert!(found(listed), "the substring filter finds the file");
}

/// A turn has ended once the row settles; the second turn needs it idle.
fn turn_over(t: &Task) -> bool {
    matches!(
        t.status,
        TaskStatus::Idle | TaskStatus::AwaitingUser | TaskStatus::Completed
    )
}

async fn settled_task(w: &Walk, label: &str, id: &str, done: impl Fn(&Task) -> bool) -> Task {
    let tasks = set(&w.runner, "the runner").tasks();
    let quiet = w.sa_quiet().tasks();
    settled(
        w.mode,
        label,
        Duration::from_secs(120),
        || tasks.get(id),
        || quiet.get(id),
        done,
    )
    .await
}

pub async fn task(w: &mut Walk) {
    let sa = w.sa();
    let runtime = set(&w.runtime, "the runtime");
    // Every `/run` option the CLI sends: the project as a check, the service
    // account's own environment, this Runtime version's Recipe, and bindings
    // that must materialize.
    let recipe_id = runtime.recipe_id.expect("the runtime pins a recipe");
    let runner = sa
        .runtimes()
        .handle(runtime.id)
        .with_project(w.project_id())
        .run(RunRequest {
            agent_name: Some(w.settings.agent.clone()),
            environment: Some(Lane::Production),
            recipe_id: Some(recipe_id),
            bindings_required: Some(true),
            ..Default::default()
        })
        .await
        .expect("open a runner on the runtime");
    assert_eq!(runner.context().recipe_id, recipe_id);
    // What a caller with its own HTTP client needs to reach the DP.
    assert!(!runner.session_token().is_empty());
    assert!(runner.deployment().endpoint.starts_with("http"));
    let started = runner
        .tasks()
        .start(&TaskCreate {
            prompt: Some(w.settings.prompt()),
            agent_name: Some(w.settings.agent.clone()),
            files: Some(vec![TaskFileRef {
                id: set(&w.input_file, "the input file").id,
                ..Default::default()
            }]),
            metadata: Some(walk_metadata(&[])),
            idle_timeout_seconds: Some(180),
            ..Default::default()
        })
        .await
        .expect("start the task");
    let created = started.task.clone().expect("the create returns the task");
    let reply = started.text().await.expect("stream the turn");
    assert!(!reply.is_empty(), "the turn replied");
    w.runner = Some(runner);
    let id = created.id.to_string();
    let task = settled_task(w, "task after its first turn", &id, turn_over).await;
    // `metadata` is the platform's (the launch path, the active run) and
    // differs between a warm pod and a baked image; the walk's own key is
    // asserted and the rest left out of the snapshot.
    let metadata = task.metadata.clone().unwrap_or_default();
    assert_eq!(metadata.get("suite"), Some(&json!("walk")));
    assert_stable!(task, { ".metadata" => "[platform's]" }, @r#"
    {
      "created_at": "[timestamp]",
      "display_index": "[number]",
      "id": "[uuid]",
      "is_archived": false,
      "kind": "agent",
      "last_user_message_at": "[timestamp]",
      "member_id": "[uuid]",
      "metadata": "[platform's]",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "runtime_group_id": "[uuid]",
      "runtime_id": "[uuid]",
      "started_at": "[timestamp]",
      "status": "idle",
      "tags": [],
      "title": "Call the `echo` tool from the declared `hello-mcp` MCP server with the message `hello from the public MCP`. Write the echoed value exactly, with no quotes or additional formatting, to `/workspace/outputs/greeting.txt`. Then run `echo $WALK_MARKER` in…",
      "updated_at": "[timestamp]"
    }
    "#);
    w.conversation_id = Some(
        metadata
            .get("conversation_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| id.clone()),
    );
    let runner = set(&w.runner, "the runner");
    let run = runner
        .tasks()
        .runs
        .get(&id, &started.run.id)
        .await
        .expect("read the run");
    assert_eq!(run.task_id, task.id);
    more_turns(w, &id).await;
    w.task = Some(task);
}

/// Every frame of one attach, failing on an error the stream yields.
async fn read_frames(
    frames: impl futures::Stream<Item = introspection_sdk::api::ApiResult<RunFrame>>,
) -> Vec<RunFrame> {
    let mut out = Vec::new();
    futures::pin_mut!(frames);
    while let Some(frame) = frames.next().await {
        out.push(frame.expect("a run frame"));
    }
    out
}

/// The numeric content cursor a frame carries, if it is a content frame.
fn cursor_of(frame: &RunFrame) -> Option<u64> {
    frame.frame.as_ref()?.id.as_deref()?.parse().ok()
}

/// The second turn, sent twice under one `delivery_id`, read as raw frames
/// without the server's start hold, then reattached from its first cursor.
async fn second_turn(w: &Walk, id: &str, prompt: &TaskPrompt) {
    let runner = set(&w.runner, "the runner");
    // A fixed id, so replay sends the body the recording holds; the receipt
    // is per task, and every walk makes a new task.
    let second = TaskRunCreate {
        delivery_id: Some("walk-second-turn".into()),
        prompt: Some(prompt.clone()),
        ..Default::default()
    };
    let turn = runner
        .tasks()
        .runs
        .create(id, &second)
        .await
        .expect("a second turn");
    // The CLI's retry of a request whose outcome it never saw: the same
    // delivery answers with the run it already accepted.
    let retried = runner
        .tasks()
        .runs
        .create(id, &second)
        .await
        .expect("retry the second turn");
    assert_eq!(retried.run.id, turn.run.id, "a retried delivery is one run");
    let frames = read_frames(turn.stream_frames(StreamOptions {
        wait_for_start: Some(false),
        ..Default::default()
    }))
    .await;
    assert!(
        frames
            .iter()
            .all(|f| f.frame.as_ref().is_some_and(|f| f.event == "ag_ui")),
        "every event keeps its ag_ui frame"
    );
    assert!(matches!(
        frames.last().map(|f| &f.event),
        Some(introspection_sdk::AgUiEvent::RunFinished(_))
    ));
    let first = frames
        .iter()
        .find_map(cursor_of)
        .expect("the turn streamed content");
    settled_task(w, "task after its second turn", id, turn_over).await;
    // Reattach past the first content frame: the replay starts after it.
    let replay = read_frames(turn.stream_frames(StreamOptions {
        since: first,
        wait_for_start: Some(false),
        ..Default::default()
    }))
    .await;
    assert!(
        replay.iter().filter_map(cursor_of).all(|c| c > first),
        "a reattach from a cursor replays only what follows it"
    );
}

/// A second turn through `/runs`, sent twice under one `delivery_id` and read
/// as raw frames, then again from a cursor; a third one cancelled; list by
/// runtime, retitle, archive and unarchive.
async fn more_turns(w: &Walk, id: &str) {
    let runner = set(&w.runner, "the runner");
    let id = id.to_string();
    let prompt = TaskPrompt {
        text: w.settings.second_turn(),
        images: None,
    };
    second_turn(w, &id, &prompt).await;
    let third = runner
        .tasks()
        .runs
        .create(
            &id,
            &TaskRunCreate {
                prompt: Some(prompt),
                ..Default::default()
            },
        )
        .await
        .expect("a third turn");
    third.cancel().await.expect("cancel the third turn");
    let settled = settled_task(w, "task after a cancelled turn", &id, |t| {
        turn_over(t) || t.status == TaskStatus::Cancelled
    })
    .await;

    // The runtime the task was stamped with, which the filter compares.
    let runtime_id = settled.runtime_id.expect("the task names its runtime");
    let listed = runner
        .tasks()
        .list(&TaskListParams {
            runtime_id: Some(runtime_id),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list the runtime's tasks");
    assert!(listed.iter().any(|t| t.id.to_string() == id));
    assert!(listed.iter().all(|t| t.runtime_id == Some(runtime_id)));
    let retitled = runner
        .tasks()
        .update(
            &id,
            &TaskUpdate {
                title: Some("The walk's task".into()),
                ..Default::default()
            },
        )
        .await
        .expect("retitle the task");
    assert_eq!(retitled.title.as_deref(), Some("The walk's task"));
    runner.tasks().archive(&id).await.expect("archive the task");
    runner
        .tasks()
        .unarchive(&id)
        .await
        .expect("unarchive the task");
}

pub async fn list_files(
    client: &IntrospectionClient,
    params: &FileListParams,
) -> introspection_sdk::api::ApiResult<Vec<File>> {
    client.files().list(params).collect_all(0).await
}

pub async fn evidence(w: &mut Walk) {
    let sa = w.sa();
    let quiet = w.sa_quiet();
    let task = set(&w.task, "the task").id;
    let conversation_id = set(&w.conversation_id, "the conversation").clone();
    let by_task = FileListParams {
        task_id: Some(task),
        ..Default::default()
    };
    let outputs = settled(
        w.mode,
        "task output files",
        Duration::from_secs(120),
        || list_files(&sa, &by_task),
        || list_files(&quiet, &by_task),
        |files: &Vec<File>| {
            files
                .iter()
                .any(|f| f.name.ends_with(walk::OUTPUT_FILE_NAME))
        },
    )
    .await;
    let output = outputs
        .into_iter()
        .find(|f| f.name.ends_with(walk::OUTPUT_FILE_NAME))
        .expect("the output file");
    assert_stable!(output, @r#"
    {
      "created_at": "[timestamp]",
      "file_type": "filesystem",
      "generated_output_task_id": "[uuid]",
      "id": "[uuid]",
      "member_id": "[uuid]",
      "metadata": {
        "blob_checksum": "[digest]",
        "conversation_id": "[uuid]",
        "original_filename": "greeting.txt",
        "relative_path": "greeting.txt",
        "task_id": "[uuid]"
      },
      "mime_type": "text/plain",
      "name": "tasks/[uuid]/outputs/greeting.txt",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "size_bytes": "[number]",
      "storage_path": "[uuid]/[uuid]/files/[uuid]/greeting.txt",
      "tags": [],
      "task_id": "[uuid]",
      "updated_at": "[timestamp]",
      "version": 1
    }
    "#);
    let content = sa
        .files()
        .download(&output.id.to_string())
        .await
        .expect("download the output");
    assert_eq!(String::from_utf8_lossy(&content).trim(), walk::ECHO_RESULT);
    w.output_file = Some(output);

    let conversation = sa
        .conversations()
        .get(&conversation_id)
        .await
        .expect("read the conversation");
    assert_stable!(conversation, @r#"
    {
      "agents": [
        {
          "depth": 0,
          "id": "[redacted]",
          "name": "scripted"
        }
      ],
      "cost": {
        "usd": 0.0
      },
      "created_at": "[timestamp]",
      "environment": "production",
      "id": "[uuid]",
      "metrics": {
        "duration_ms": "[number]",
        "failed_llm_call_count": "[number]",
        "failed_tool_use_count": "[number]",
        "has_errors": false,
        "llm_call_count": "[number]",
        "max_turn": "[number]",
        "span_count": "[number]",
        "tool_use_count": "[number]",
        "trace_count": "[number]"
      },
      "object": "conversation",
      "owner": {
        "key": "[redacted]"
      },
      "recipe": {
        "git_commit_sha": "[sha]"
      },
      "runtime": {
        "group_id": "[uuid]",
        "id": "[uuid]"
      },
      "service_name": "hello-mcp",
      "task_title": "The walk's task",
      "updated_at": "[timestamp]",
      "usage": {
        "output_tokens": "[number]",
        "total_input_tokens": "[number]",
        "total_tokens": "[number]"
      }
    }
    "#);
    assert!(conversation.metrics.tool_use_count > 0);
    assert!(conversation.metrics.llm_call_count > 0);
    assert!(conversation.metrics.failed_llm_call_count <= conversation.metrics.llm_call_count);
    assert!(conversation.metrics.trace_count >= 2);
    assert!(conversation.usage.total_tokens > 0);
    conversation_reads(&sa, &conversation_id, w).await;
}

async fn first_items(
    sa: &IntrospectionClient,
    conversation_id: &str,
    params: &ConversationItemListParams,
) -> introspection_sdk::GenAiSpanList {
    sa.conversations()
        .items
        .list(conversation_id, params)
        .expect("an items paginator")
        .next_page()
        .await
        .expect("list the conversation's items")
        .expect("a page")
}

async fn conversation_reads(sa: &IntrospectionClient, conversation_id: &str, w: &Walk) {
    let group = set(&w.runtime, "the runtime").runtime_group_id;
    let page = first_items(sa, conversation_id, &ConversationItemListParams::default()).await;
    let items = serde_json::to_string(&page).expect("items JSON");
    assert!(items.contains("echo"), "the hello-mcp echo tool was called");
    assert!(
        items.contains(walk::ECHO_RESULT),
        "the echo result reached the conversation"
    );
    assert!(
        items.contains(walk::VARIABLE_VALUE),
        "the variable reached the sandbox"
    );
    let first = page.data.first().expect("an item");
    sa.conversations()
        .items
        .get(
            conversation_id,
            first.span_id.as_deref().expect("a span id"),
            &ConversationItemGetParams::default(),
        )
        .await
        .expect("read one item");
    let by_group = ConversationListParams {
        runtime_group_id: group,
        ..Default::default()
    };
    let listed = sa
        .conversations()
        .list(&by_group)
        .expect("a conversations paginator")
        .collect_all(0)
        .await
        .expect("list conversations");
    assert!(listed.iter().any(|c| c.id == conversation_id));
    let export = ConversationExportParams::default();
    sa.conversations()
        .export_trajectory(conversation_id, &export)
        .await
        .expect("the trajectory export");
    exports(sa, conversation_id, &by_group, page.data.len()).await;
    item_pages(sa, conversation_id).await;
}

/// The other export formats and the Arrow list.
async fn exports(
    sa: &IntrospectionClient,
    conversation_id: &str,
    by_group: &ConversationListParams,
    items: usize,
) {
    let export = ConversationExportParams::default();
    let latest = sa
        .conversations()
        .retrieve(conversation_id, None)
        .await
        .expect("the latest turn");
    assert!(
        latest.and_then(|s| s.span_id).is_some(),
        "the latest LLM turn"
    );
    let exported = sa
        .conversations()
        .export_json(conversation_id, &export)
        .await
        .expect("export JSON");
    assert!(exported.data.len() >= items);
    let table = sa
        .conversations()
        .export_arrow(conversation_id, &export)
        .await
        .expect("export Arrow");
    // Spans can still be arriving between two exports, so each is held to
    // the items the step already read rather than to the other.
    let rows: usize = table.batches.iter().map(|b| b.num_rows()).sum();
    assert!(rows >= items);
    let mut stream = Box::pin(
        sa.conversations()
            .export_stream(conversation_id, ConversationExportFormat::Json, &export)
            .await
            .expect("export as a stream"),
    );
    let mut streamed = Vec::new();
    while let Some(chunk) = stream.next().await {
        streamed.extend_from_slice(&chunk.expect("a chunk"));
    }
    let streamed: Value = serde_json::from_slice(&streamed).expect("the streamed export is JSON");
    assert!(streamed["data"].as_array().map_or(0, Vec::len) >= items);
    // The representations the SDK does not model come back as the server's
    // JSON.
    for format in [
        ConversationExportFormat::Replay,
        ConversationExportFormat::Atif,
    ] {
        let body = sa
            .conversations()
            .export_json_value(conversation_id, format, &export)
            .await
            .unwrap_or_else(|e| panic!("the {format:?} export: {e}"));
        assert!(body.is_object(), "the {format:?} export is a JSON object");
    }
    let listed = sa
        .conversations()
        .list_arrow(by_group)
        .await
        .expect("list as Arrow");
    assert!(listed.batches.iter().map(|b| b.num_rows()).sum::<usize>() > 0);
}

/// A paged item list, and items selected by what their messages say.
async fn item_pages(sa: &IntrospectionClient, conversation_id: &str) {
    let one = ConversationItemListParams {
        limit: Some(1),
        ..Default::default()
    };
    let mut pages = sa
        .conversations()
        .items
        .list(conversation_id, &one)
        .expect("an items paginator");
    pages.next_page().await.expect("a first page");
    assert!(
        pages.next_page().await.expect("a second page").is_some(),
        "a second page of items"
    );
    // `contains` selects items by the plaintext summary of their messages.
    let tool_calls = ConversationItemListParams {
        contains: vec![ConversationContains::OutputToolCall("*".into())],
        ..Default::default()
    };
    let tool_calls = first_items(sa, conversation_id, &tool_calls).await;
    assert!(
        serde_json::to_string(&tool_calls)
            .expect("JSON")
            .contains("echo"),
        "a tool-call item"
    );
    let user_turns = ConversationItemListParams {
        contains: vec![ConversationContains::InputUser],
        ..Default::default()
    };
    assert!(
        !first_items(sa, conversation_id, &user_turns)
            .await
            .data
            .is_empty(),
        "a user turn"
    );
}

/// One count through the typed query and the same count as a raw document.
async fn metrics(sa: &IntrospectionClient, runtime: Uuid) {
    // A fixed window, so the request replays; the walk's conversation is
    // within it by construction.
    sa.metrics()
        .query(&MetricsQuery {
            view: "conversations".into(),
            metrics: vec![MetricSpec {
                measure: None,
                aggregation: "count".into(),
            }],
            filters: Some(vec![MetricFilter {
                field: "runtime_id".into(),
                operator: "eq".into(),
                value: json!(runtime),
            }]),
            start: Some("2026-01-01T00:00:00Z".into()),
            end: Some("2027-01-01T00:00:00Z".into()),
            ..Default::default()
        })
        .await
        .expect("a metrics response");
    // The same count as a document the SDK forwards unmodelled, the way the
    // CLI's `metrics query` does; its own window keeps the two requests apart.
    let raw = sa
        .metrics()
        .query_raw(&json!({
            "view": "conversations",
            "metrics": [{"aggregation": "count"}],
            "filters": [{"field": "runtime_id", "operator": "eq", "value": runtime}],
            "from_timestamp": "2026-01-01T00:00:00Z",
            "to_timestamp": "2026-12-31T00:00:00Z",
        }))
        .await
        .expect("a raw metrics response");
    assert!(raw["data"].is_array(), "{raw}");
}

pub async fn telemetry_reads(w: &mut Walk) {
    let sa = w.sa();
    let member = w.member();
    let runtime = set(&w.runtime, "the runtime").id;
    metrics(&sa, runtime).await;

    // Access tags are an administrator's to write: the owner's token, not
    // the service account, which acts as the project's agent member.
    let tag = member
        .tags()
        .create(TagCreate::Access(AccessTagCreate {
            name: walk::TAG_NAME.into(),
            description: Some("the walk's tag".into()),
        }))
        .await
        .expect("create the access tag");
    assert_stable!(tag, @r#"
    {
      "created_at": "[timestamp]",
      "description": "the walk's tag",
      "kind": "access",
      "name": "walk",
      "updated_at": "[timestamp]"
    }
    "#);
    member
        .tags()
        .get(TagKind::Access, walk::TAG_NAME)
        .await
        .expect("read the tag");
    member
        .tags()
        .update(
            TagKind::Access,
            walk::TAG_NAME,
            TagUpdate::description("the walk's tag, updated"),
        )
        .await
        .expect("update the tag");
    let tags = member
        .tags()
        .list(&TagListParams::new(TagKind::Access))
        .collect_all(0)
        .await
        .expect("list tags");
    assert!(tags.iter().any(|t| t.name == walk::TAG_NAME));

    // A tag share's grantor must hold the tag, so the file is shared to the
    // owner member directly.
    let owners = w
        .owner()
        .members()
        .list(&MemberListParams::default())
        .collect_all(0)
        .await
        .expect("list members");
    let owner_id = owners.first().expect("a member").id;
    let output = set(&w.output_file, "the output file").id;
    let share = sa
        .shares()
        .create(&ShareCreate {
            granted_member_id: Some(owner_id),
            ..ShareCreate::new(ShareResourceType::File, output.to_string())
        })
        .await
        .expect("share the output file");
    assert_stable!(share, @r#"
    {
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "granted_member_id": "[uuid]",
      "id": "[uuid]",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "resource_id": "[uuid]",
      "resource_type": "file",
      "updated_at": "[timestamp]",
      "url": "[url]"
    }
    "#);
    sa.shares()
        .get(&share.id.to_string())
        .await
        .expect("read the share");
    let by_file = ShareListParams {
        resource_type: Some(ShareResourceType::File),
        ..Default::default()
    };
    let shares = sa
        .shares()
        .list(&by_file)
        .collect_all(0)
        .await
        .expect("list shares");
    assert!(shares.iter().any(|s| s.id == share.id));

    annotations(w, &sa, &member, owner_id).await;
    dp_reads_empty(&sa, &member).await;
}

async fn annotations(
    w: &Walk,
    sa: &IntrospectionClient,
    member: &IntrospectionClient,
    reviewer: Uuid,
) {
    let conversation_id = set(&w.conversation_id, "the conversation").clone();
    // The annotation targets the conversation's first item (its span).
    let page = first_items(sa, &conversation_id, &ConversationItemListParams::default()).await;
    let item = page.data.first().expect("an item");
    let target = AnnotationTarget {
        trace_id: item.trace_id.clone(),
        span_id: item.span_id.clone().expect("a span id"),
    };
    let event = |label: &str, comment: &str| AnnotationEventOptions {
        event_id: Some(span_event_id(&target.trace_id, &target.span_id, label)),
        comment: Some(comment.into()),
    };
    // An event id derived from the span, so the request replays; the SDK
    // would mint one.
    let labelled = event("labels", "labelled by the walk");
    let label_event = labelled.event_id;
    let created = member
        .annotations()
        .create(
            target.clone(),
            AnnotationMutation::Labels(vec!["walk".into()]),
            labelled,
        )
        .await
        .expect("label the span");
    assert_eq!(Some(created.event_id), label_event);
    // Reviewers by member id need no Control Plane lookup, so a Data Plane
    // client alone assigns them.
    let dp_only = Annotations::data_plane(set(&w.member, "the member token").dp_http());
    let assigned = dp_only
        .create(
            target.clone(),
            AnnotationMutation::ReviewerIds(vec![reviewer]),
            event("reviewer-ids", "assigned by the walk"),
        )
        .await
        .expect("assign a reviewer by id");
    assert_eq!(Some(assigned.event_id), event("reviewer-ids", "").event_id);
    assert!(
        dp_only
            .create(
                target.clone(),
                AnnotationMutation::ReviewerEmails(vec!["expert@example.com".into()]),
                AnnotationEventOptions::default(),
            )
            .await
            .is_err(),
        "an email needs the Control Plane, refused before any request"
    );
    // Reviewers by email resolve through the Control Plane's member list,
    // which a member's Data Plane token cannot read; the walk clears them,
    // which needs no lookup, and reaches the check made before any request.
    member
        .annotations()
        .create(
            target.clone(),
            AnnotationMutation::ReviewerEmails(vec![]),
            event("reviewers", "reviewers cleared by the walk"),
        )
        .await
        .expect("clear the reviewers");
    let blank = member
        .annotations()
        .create(
            target.clone(),
            AnnotationMutation::ReviewerEmails(vec![" ".into()]),
            AnnotationEventOptions::default(),
        )
        .await;
    assert!(
        blank.is_err(),
        "a blank reviewer email is refused before any request"
    );
    let by_conversation = AnnotationListParams {
        conversation_id: Some(conversation_id.clone()),
        ..Default::default()
    };
    // An annotation is appended through the OTLP self-ingest path; on the
    // local stack it stays `visible: false`, so the list may read empty.
    let listed = member
        .annotations()
        .list(&by_conversation)
        .await
        .expect("an annotations paginator")
        .collect_all(0)
        .await
        .expect("list annotations");
    if w.settings.target != "compose" {
        assert!(listed
            .first()
            .is_some_and(|a| a.labels.contains(&"walk".to_string())));
    }
    annotation_reads(w, sa, &conversation_id, &target).await;
}

/// The annotation as events (JSON and Arrow), its facets and navigation.
async fn annotation_reads(
    w: &Walk,
    sa: &IntrospectionClient,
    conversation_id: &str,
    target: &AnnotationTarget,
) {
    let params = EventListParams {
        conversation_id: Some(conversation_id.to_string()),
        ..EventListParams::new(IntrospectionEventName::Annotation)
    };
    let events = sa
        .events()
        .list(&params)
        .expect("an events paginator")
        .collect_all(0)
        .await
        .expect("list events");
    if let Some(first) = events.first() {
        let id = serde_json::to_value(first).expect("event JSON")["id"]
            .as_str()
            .expect("an id")
            .to_string();
        sa.events().get(&id).await.expect("read the event");
    }
    let table = sa
        .events()
        .list_arrow(&params)
        .await
        .expect("events as Arrow");
    // The schema rides the stream even when the page has no rows.
    assert!(table.schema.is_some(), "the Arrow page carries its schema");
    assert_eq!(
        table.batches.iter().map(|b| b.num_rows()).sum::<usize>(),
        events.len()
    );
    let member = w.member();
    let annotations = member.annotations();
    annotations.facets().await.expect("annotation facets");
    annotations
        .navigation(target, &AnnotationNavigationParams::default())
        .await
        .expect("annotation navigation");
}

/// The AAuth surfaces and the member's app connections, read empty.
async fn dp_reads_empty(sa: &IntrospectionClient, member: &IntrospectionClient) {
    let missions = sa
        .aauth()
        .missions()
        .list(&MissionListParams::default())
        .collect_all(0)
        .await
        .expect("missions");
    assert!(missions.is_empty());
    let approvals = sa
        .aauth()
        .approvals()
        .list(&ApprovalListParams::default())
        .collect_all(0)
        .await
        .expect("approvals");
    assert!(approvals.is_empty());
    // `/v1/connections` is the apps a member connects through an app
    // (Pipedream) connector, which needs that provider's credentials; with
    // none on the project every route answers 404.
    let connections = member.connections();
    expect_not_found(
        connections
            .list(&MemberConnectionListParams::default())
            .collect_all(0)
            .await,
    );
    let id: Uuid = "019a1000-0000-7000-8000-0000000000c1"
        .parse()
        .expect("a uuid");
    expect_not_found(connections.get(id).await);
    expect_not_found(connections.delete(id).await);
    expect_not_found(
        connections
            .create(&MemberConnectionCreate {
                app: "gmail".into(),
                runtime: Some(walk::RUNTIME_SLUG.into()),
            })
            .await,
    );
    // A client with no runtime of its own refuses before any request.
    let refused = connections
        .create(&MemberConnectionCreate {
            app: "gmail".into(),
            runtime: None,
        })
        .await;
    assert!(refused.is_err_and(|e| e.to_string().contains("runtime")));
}
