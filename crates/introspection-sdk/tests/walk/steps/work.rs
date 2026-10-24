//! Steps 9 to 13: a file in, a task on the runtime, the evidence it left,
//! and the telemetry reads over it.

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt as _;
use introspection_sdk::{
    AccessTagCreate, AnnotationEventOptions, AnnotationListParams, AnnotationMutation,
    AnnotationTarget, ApprovalListParams, ConversationContains, ConversationExportFormat,
    ConversationExportParams, ConversationItemGetParams, ConversationItemListParams,
    ConversationListParams, EventListParams, File, FileCreateText, FileListParams, FileUpdate,
    FileUpload, FileVersionListParams, IntrospectionClient, IntrospectionEventName,
    MemberConnectionCreate, MemberConnectionListParams, MemberListParams, MetricFilter, MetricSpec,
    MetricsQuery, MissionListParams, RunRequest, ShareCreate, ShareListParams, ShareResourceType,
    TagCreate, TagKind, TagListParams, TagUpdate, Task, TaskCreate, TaskFileRef, TaskListParams,
    TaskPrompt, TaskRunCreate, TaskStatus, TaskUpdate,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::steps::expect_not_found;
use crate::support::{assert_stable, settled, span_event_id, walk};
use crate::{set, Walk};

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
        })
        .await
        .expect("upload the input file");
    assert_stable!(input, @r#"
    {
      "created_at": "[timestamp]",
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
      "parent_id": null,
      "project_id": "[uuid]",
      "size_bytes": "[number]",
      "storage_path": "[uuid]/[uuid]/files/[uuid]/walk-input.txt",
      "storage_version_id": null,
      "tags": [],
      "task_id": null,
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
            .with_content_type("text/plain"),
        )
        .await
        .expect("add a version");
    let versions = sa
        .files()
        .versions
        .list(&id, &FileVersionListParams::default())
        .collect_all(0)
        .await
        .expect("list versions");
    assert!(versions.iter().any(|v| v.id == version.id));
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
    let include_versions = || {
        Some(HashMap::from([(
            "include_versions".to_string(),
            json!(true),
        )]))
    };
    let found = |files: Vec<File>| files.iter().any(|f| f.id == input.id);
    let by_metadata = FileListParams {
        metadata: Some(HashMap::from([
            ("suite".to_string(), "walk".to_string()),
            ("stage".to_string(), "attached".to_string()),
        ])),
        filters: include_versions(),
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
        filters: include_versions(),
        ..Default::default()
    };
    let listed = sa
        .files()
        .list(&by_tag)
        .collect_all(0)
        .await
        .expect("list by tag");
    assert!(found(listed), "the tag filter finds the file");
    // todo: no `name_contains` on `FileListParams`.
    let mut by_name = include_versions().expect("filters");
    by_name.insert("name_contains".into(), json!("WALK-INPUT"));
    let by_name = FileListParams {
        filters: Some(by_name),
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
    let runtime = set(&w.runtime, "the runtime").id;
    let runner = sa
        .runtimes()
        .handle(runtime)
        .run(RunRequest {
            agent_name: Some(w.settings.agent.clone()),
            ..Default::default()
        })
        .await
        .expect("open a runner on the runtime");
    let started = runner
        .tasks()
        .start(&TaskCreate {
            prompt: Some(w.settings.prompt()),
            agent_name: Some(w.settings.agent.clone()),
            files: Some(vec![TaskFileRef {
                id: set(&w.input_file, "the input file").id.to_string(),
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

/// A second turn through `/runs`, a third one cancelled; list, retitle,
/// archive and unarchive.
async fn more_turns(w: &Walk, id: &str) {
    let runner = set(&w.runner, "the runner");
    let id = id.to_string();
    let second = TaskRunCreate {
        prompt: Some(TaskPrompt {
            text: w.settings.second_turn(),
            images: None,
        }),
        ..Default::default()
    };
    let turn = runner
        .tasks()
        .runs
        .create(&id, &second)
        .await
        .expect("a second turn");
    turn.text().await.expect("stream the second turn");
    settled_task(w, "task after its second turn", &id, turn_over).await;
    let third = runner
        .tasks()
        .runs
        .create(&id, &second)
        .await
        .expect("a third turn");
    third.cancel().await.expect("cancel the third turn");
    settled_task(w, "task after a cancelled turn", &id, |t| {
        turn_over(t) || t.status == TaskStatus::Cancelled
    })
    .await;

    let listed = runner
        .tasks()
        .list(&TaskListParams::default())
        .collect_all(0)
        .await
        .expect("list tasks");
    assert!(listed.iter().any(|t| t.id.to_string() == id));
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
        filters: Some(HashMap::from([("task_id".to_string(), json!(task))])),
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
      "parent_id": null,
      "project_id": "[uuid]",
      "size_bytes": "[number]",
      "storage_path": "[uuid]/[uuid]/files/[uuid]/greeting.txt",
      "storage_version_id": null,
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
        "failed_tool_use_count": "[number]",
        "has_errors": false,
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
    // todo: `ConversationMetrics` has no `llm_call_count` or
    // `failed_llm_call_count`, which the JS walk bounds.
    assert!(conversation.metrics.tool_use_count > 0);
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
    let rows: usize = table.batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(rows, exported.data.len());
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
    assert_eq!(
        streamed["data"].as_array().map(Vec::len),
        Some(exported.data.len())
    );
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

pub async fn telemetry_reads(w: &mut Walk) {
    let sa = w.sa();
    let member = w.member();
    let runtime = set(&w.runtime, "the runtime").id;
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
      "color": null,
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
    let output = set(&w.output_file, "the output file").id;
    let share = sa
        .shares()
        .create(&ShareCreate {
            granted_member_id: Some(owners.first().expect("a member").id),
            ..ShareCreate::new(ShareResourceType::File, output.to_string())
        })
        .await
        .expect("share the output file");
    assert_stable!(share, @r#"
    {
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "granted_member_id": "[uuid]",
      "granted_tag": null,
      "id": "[uuid]",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "resource_id": "[uuid]",
      "resource_type": "file",
      "updated_at": "[timestamp]",
      "url": "[url]",
      "visible_from": null
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

    annotations(w, &sa, &member).await;
    dp_reads_empty(&sa, &member).await;
}

async fn annotations(w: &Walk, sa: &IntrospectionClient, member: &IntrospectionClient) {
    let conversation_id = set(&w.conversation_id, "the conversation").clone();
    // The annotation targets the conversation's first item (its span).
    let page = first_items(sa, &conversation_id, &ConversationItemListParams::default()).await;
    let item = page.data.first().expect("an item");
    let target = AnnotationTarget {
        trace_id: item.trace_id.clone(),
        span_id: item.span_id.clone().expect("a span id"),
    };
    let event = |label: &str| AnnotationEventOptions {
        event_id: Some(span_event_id(&target.trace_id, &target.span_id, label)),
    };
    // An event id derived from the span, so the request replays; the SDK
    // would mint one.
    member
        .annotations()
        .create(
            target.clone(),
            AnnotationMutation::Labels(vec!["walk".into()]),
            event("labels"),
        )
        .await
        .expect("label the span");
    // Reviewers by email resolve through the Control Plane's member list,
    // which a member's Data Plane token cannot read; the walk clears them,
    // which needs no lookup, and reaches the check made before any request.
    member
        .annotations()
        .create(
            target.clone(),
            AnnotationMutation::ReviewerEmails(vec![]),
            event("reviewers"),
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
    assert_eq!(
        table.batches.iter().map(|b| b.num_rows()).sum::<usize>(),
        events.len()
    );
    // todo: no SDK method for the annotation facets or navigation reads.
    let raw = set(&w.member, "the member token").dp_raw();
    let _: Value = raw
        .get(&format!(
            "/v1/annotations/facets?conversation_id={conversation_id}"
        ))
        .await
        .expect("annotation facets");
    let _: Value = raw
        .get(&format!(
            "/v1/annotations/navigation?trace_id={}&span_id={}",
            target.trace_id, target.span_id
        ))
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
