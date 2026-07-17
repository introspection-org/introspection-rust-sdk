//! Integration tests for the REST API surface (`client.tasks` /
//! `client.files`) backed by `wiremock`.
//!
//! These tests don't touch OpenTelemetry — they construct the API
//! namespaces directly via [`HttpClient::from_parts`] so we can swap the
//! real DP for a `wiremock::MockServer`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{
    ConversationListParams, Conversations, EventListParams, Events, FileCreateText, FileListParams,
    FileUpdate, FileUpload, FileVersions, Files, HttpClient, HttpConfig, IntrospectionAPIError,
    MetricSpec, Metrics, MetricsQuery, PaginationParams, SortDirection, TaskCreate, TaskListParams,
    TaskMode, TaskRunCreate, TaskRuns, TaskUpdate, Tasks,
};
use introspection_sdk::AgUiEvent;
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn build_http(server: &MockServer) -> Arc<HttpClient> {
    let cfg = HttpConfig {
        api_url: server.uri(),
        token: "intro_test".to_string(),
        additional_headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        max_retries: 2,
        retry_base: Duration::from_millis(1),
    };
    Arc::new(HttpClient::from_parts(reqwest::Client::new(), cfg))
}

#[tokio::test]
async fn tasks_list_returns_paginated_envelope() {
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server));

    Mock::given(method("GET"))
        .and(path("/v1/tasks"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [],
            "count": 0,
            "total_count": 0,
            "next": null,
        })))
        .mount(&server)
        .await;

    let mut paginator = tasks.list(&TaskListParams {
        limit: Some(50),
        ..Default::default()
    });
    let page = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page.count, 0);
    assert!(page.records.is_empty());
    assert!(paginator.next_page().await.unwrap().is_none());
}

#[tokio::test]
async fn tasks_create_returns_task_and_run() {
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server));

    Mock::given(method("POST"))
        .and(path("/v1/tasks"))
        .and(body_json(json!({
            "prompt": "hi",
            "mode": "agent",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "task": {
                "id": "00000000-0000-0000-0000-000000000001",
                "org_id": "00000000-0000-0000-0000-00000000aaaa",
                "project_id": "00000000-0000-0000-0000-00000000bbbb",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "mode": "agent",
                "status": "pending",
                "is_archived": false,
            },
            "run": {
                "id": "run_001",
                "task_id": "00000000-0000-0000-0000-000000000001",
                "status": "queued",
            }
        })))
        .mount(&server)
        .await;

    let handle = tasks
        .start(&TaskCreate {
            prompt: Some("hi".into()),
            mode: Some(TaskMode::Agent),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(handle.run.id, "run_001");
    assert!(handle.task.is_some());
}

#[tokio::test]
async fn tasks_update_patches_title() {
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server));

    Mock::given(method("PATCH"))
        .and(path("/v1/tasks/abc"))
        .and(body_json(json!({"title": "renamed"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "org_id": "00000000-0000-0000-0000-00000000aaaa",
            "project_id": "00000000-0000-0000-0000-00000000bbbb",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "title": "renamed",
            "mode": "agent",
            "status": "running",
            "is_archived": false,
        })))
        .mount(&server)
        .await;

    let task = tasks
        .update(
            "abc",
            &TaskUpdate {
                title: Some("renamed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(task.title.as_deref(), Some("renamed"));
}

#[tokio::test]
async fn tasks_archive_unarchive_are_post_empty() {
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server));

    Mock::given(method("POST"))
        .and(path("/v1/tasks/abc/archive"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/tasks/abc/unarchive"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    tasks.archive("abc").await.unwrap();
    tasks.unarchive("abc").await.unwrap();
}

#[tokio::test]
async fn tasks_delete_surfaces_403_as_api_error() {
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server));

    Mock::given(method("DELETE"))
        .and(path("/v1/tasks/abc"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("content-type", "application/json")
                .insert_header("x-request-id", "req_42")
                .set_body_json(json!({"detail": "scope tasks:delete required"})),
        )
        .mount(&server)
        .await;

    let err = tasks.delete("abc").await.unwrap_err();
    assert_eq!(err.status(), Some(403));
    assert_eq!(err.request_id(), Some("req_42"));
}

#[tokio::test]
async fn run_handle_streams_typed_agui_events() {
    let server = MockServer::start().await;
    let http = build_http(&server);
    let runs = TaskRuns::new(http);

    // `ag_ui` frames carry typed AG-UI events; the heartbeat is a transport
    // frame the typed layer drops. Message ids are the worker's non-UUID
    // `{run_id}:text:0` shape.
    let body = "\
event: ag_ui\ndata: {\"type\":\"TEXT_MESSAGE_CONTENT\",\"messageId\":\"run_001:text:0\",\"delta\":\"hello \"}\n\n\
event: heartbeat\ndata: {\"runId\":\"run_001\"}\n\n\
event: ag_ui\ndata: {\"type\":\"TEXT_MESSAGE_CONTENT\",\"messageId\":\"run_001:text:0\",\"delta\":\"world\"}\n\n";
    Mock::given(method("GET"))
        .and(path(
            "/v1/tasks/00000000-0000-0000-0000-000000000001/runs/run_001/stream",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let stream = runs
        .stream("00000000-0000-0000-0000-000000000001", "run_001")
        .await
        .unwrap();
    let events: Vec<_> = stream.collect().await;
    // Two typed events; the heartbeat transport frame is not surfaced.
    assert_eq!(events.len(), 2);
    let deltas: Vec<String> = events
        .iter()
        .map(|ev| match ev.as_ref().unwrap() {
            AgUiEvent::TextMessageContent(e) => e.delta.clone(),
            other => panic!("expected TextMessageContent, got {other:?}"),
        })
        .collect();
    assert_eq!(deltas, ["hello ", "world"]);
}

#[tokio::test]
async fn task_runs_create_then_cancel() {
    let server = MockServer::start().await;
    let runs = TaskRuns::new(build_http(&server));

    Mock::given(method("POST"))
        .and(path("/v1/tasks/abc/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run": {
                "id": "run_2",
                "task_id": "00000000-0000-0000-0000-000000000001",
                "status": "queued",
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(
            "/v1/tasks/00000000-0000-0000-0000-000000000001/runs/run_2/cancel",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "run_2"})))
        .mount(&server)
        .await;

    let handle = runs
        .create(
            "abc",
            &TaskRunCreate {
                message: Some("go".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let cancel = handle.cancel().await.unwrap();
    assert_eq!(cancel.id, "run_2");
}

#[tokio::test]
async fn files_list_returns_paginated() {
    let server = MockServer::start().await;
    let files = Files::new(build_http(&server));
    Mock::given(method("GET"))
        .and(path("/v1/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [],
            "count": 0,
            "next": null,
        })))
        .mount(&server)
        .await;
    let mut paginator = files.list(&FileListParams::default());
    let page = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page.count, 0);
}

#[tokio::test]
async fn files_create_text_sends_json() {
    let server = MockServer::start().await;
    let files = Files::new(build_http(&server));

    Mock::given(method("POST"))
        .and(path("/v1/files"))
        .and(body_json(json!({
            "name": "note.md",
            "content": "hello",
            "mime_type": "text/markdown",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(file_response("note.md")))
        .mount(&server)
        .await;

    let file = files
        .create_text(&FileCreateText {
            name: "note.md".into(),
            content: "hello".into(),
            mime_type: Some("text/markdown".into()),
        })
        .await
        .unwrap();
    assert_eq!(file.name, "note.md");
}

#[tokio::test]
async fn files_upload_sends_multipart() {
    let server = MockServer::start().await;
    let files = Files::new(build_http(&server));

    Mock::given(method("POST"))
        .and(path("/v1/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(file_response("payload.bin")))
        .mount(&server)
        .await;

    let file = files
        .upload(FileUpload::from_bytes(b"hello".to_vec(), "payload.bin"))
        .await
        .unwrap();
    assert_eq!(file.name, "payload.bin");
}

#[tokio::test]
async fn files_update_and_delete() {
    let server = MockServer::start().await;
    let files = Files::new(build_http(&server));

    Mock::given(method("PATCH"))
        .and(path("/v1/files/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(file_response("renamed.bin")))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/files/abc"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let file = files
        .update(
            "abc",
            &FileUpdate {
                name: Some("renamed.bin".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(file.name, "renamed.bin");
    files.delete("abc").await.unwrap();
}

#[tokio::test]
async fn files_download_bytes() {
    let server = MockServer::start().await;
    let files = Files::new(build_http(&server));

    Mock::given(method("GET"))
        .and(path("/v1/files/abc/content"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .set_body_bytes(b"hello world".as_ref()),
        )
        .mount(&server)
        .await;

    let bytes = files.download("abc").await.unwrap();
    assert_eq!(bytes.as_ref(), b"hello world");
}

#[tokio::test]
async fn file_versions_list_and_get() {
    let server = MockServer::start().await;
    let versions = FileVersions::new(build_http(&server));

    Mock::given(method("GET"))
        .and(path("/v1/files/abc/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [file_response("v1.bin")],
            "count": 1,
            "total_count": 1,
            "next": null,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/files/abc/versions/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(file_response("v1.bin")))
        .mount(&server)
        .await;

    let mut paginator = versions.list("abc", &PaginationParams::default());
    let page = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page.records.len(), 1);
    let one = versions.get("abc", "v1").await.unwrap();
    assert_eq!(one.name, "v1.bin");
}

#[tokio::test]
async fn validation_error_is_translated() {
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server));

    Mock::given(method("POST"))
        .and(path("/v1/tasks"))
        .respond_with(
            ResponseTemplate::new(422)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "detail": [
                        {"msg": "field required", "loc": ["body", "prompt"], "type": "value_error.missing"}
                    ]
                })),
        )
        .mount(&server)
        .await;

    let err = tasks.create(&TaskCreate::default()).await.unwrap_err();
    assert!(matches!(
        err,
        IntrospectionAPIError::Http { status: 422, .. }
    ));
    assert!(err.to_string().contains("field required"));
}

#[tokio::test]
async fn conversations_list_maps_window_params_and_paginates() {
    let server = MockServer::start().await;
    let conversations = Conversations::new(build_http(&server));

    // First page: the ergonomic `order`/`start`/`end` land on the wire as
    // `direction`/`start_date`/`end_date`; a `next` cursor drives page 2.
    Mock::given(method("GET"))
        .and(path("/v1/conversations"))
        .and(query_param("direction", "asc"))
        .and(query_param("start_date", "2026-01-01T00:00:00Z"))
        .and(query_param("end_date", "2026-02-01T00:00:00Z"))
        .and(query_param("limit", "10"))
        .and(query_param_is_missing("next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{"conversation_id": "c1", "trace_id": "t1", "custom_attr": 7}],
            "count": 1,
            "next": "cursor_2",
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/conversations"))
        .and(query_param("next", "cursor_2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{"conversation_id": "c2"}],
            "count": 1,
            "next": null,
        })))
        .mount(&server)
        .await;

    let mut paginator = conversations
        .list(&ConversationListParams {
            limit: Some(10),
            order: Some(SortDirection::Asc),
            start: Some("2026-01-01T00:00:00Z".into()),
            end: Some("2026-02-01T00:00:00Z".into()),
            ..Default::default()
        })
        .expect("params validate");

    // Page to exhaustion via `next`, bounded.
    let all = paginator.collect_all(10).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].conversation_id.as_deref(), Some("c1"));
    // Unknown telemetry attributes ride along in `extra`.
    assert_eq!(all[0].extra.get("custom_attr"), Some(&json!(7)));
    assert_eq!(all[1].conversation_id.as_deref(), Some("c2"));
}

#[tokio::test]
async fn conversations_list_rejects_lookback_with_start_before_send() {
    let server = MockServer::start().await;
    let conversations = Conversations::new(build_http(&server));

    let result = conversations.list(&ConversationListParams {
        lookback: Some("24h".into()),
        start: Some("2026-01-01T00:00:00Z".into()),
        ..Default::default()
    });
    assert!(matches!(
        result.err(),
        Some(IntrospectionAPIError::InvalidConfig(_))
    ));
    // No request was ever sent — validation is client-side.
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn events_list_sends_grain_and_pattern_id() {
    let server = MockServer::start().await;
    let events = Events::new(build_http(&server));

    Mock::given(method("GET"))
        .and(path("/v1/events"))
        .and(query_param("grain", "introspection.observation"))
        .and(query_param("pattern_id", "pat_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{"event_id": "e1", "name": "obs"}],
            "count": 1,
            "next": null,
        })))
        .mount(&server)
        .await;

    let mut paginator = events
        .list(&EventListParams {
            grain: Some("introspection.observation".into()),
            pattern_id: Some("pat_1".into()),
            ..Default::default()
        })
        .expect("params validate");
    let page = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].event_id.as_deref(), Some("e1"));
}

#[tokio::test]
async fn metrics_query_posts_bounded_body() {
    let server = MockServer::start().await;
    let metrics = Metrics::new(build_http(&server));

    Mock::given(method("POST"))
        .and(path("/v1/metrics"))
        .and(body_json(json!({
            "view": "spans",
            "metrics": [{"measure": "duration_ns", "aggregation": "p95"}],
            "from_timestamp": "2026-06-01T00:00:00Z",
            "to_timestamp": "2026-07-01T00:00:00Z",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"p95": 1234}],
            "meta": {"approximate": true},
        })))
        .mount(&server)
        .await;

    let res = metrics
        .query(&MetricsQuery {
            view: "spans".into(),
            metrics: vec![MetricSpec {
                measure: "duration_ns".into(),
                aggregation: "p95".into(),
            }],
            start: Some("2026-06-01T00:00:00Z".into()),
            end: Some("2026-07-01T00:00:00Z".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(res.data.len(), 1);
    assert_eq!(res.data[0]["p95"], 1234);
}

#[cfg(feature = "arrow")]
#[tokio::test]
async fn conversations_list_arrow_decodes_stream_and_headers() {
    use arrow::array::{Int64Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use arrow::ipc::writer::StreamWriter;
    use arrow::record_batch::RecordBatch;
    use std::sync::Arc as StdArc;

    let server = MockServer::start().await;
    let conversations = Conversations::new(build_http(&server));

    // Build a small Arrow IPC stream server-side to hand back.
    let schema = StdArc::new(Schema::new(vec![
        Field::new("conversation_id", DataType::Utf8, false),
        Field::new("span_count", DataType::Int64, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            StdArc::new(StringArray::from(vec!["c1", "c2"])),
            StdArc::new(Int64Array::from(vec![3, 5])),
        ],
    )
    .unwrap();
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut buf, &schema).unwrap();
        writer.write(&batch).unwrap();
        writer.finish().unwrap();
    }

    Mock::given(method("GET"))
        .and(path("/v1/conversations"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/vnd.apache.arrow.stream")
                .insert_header("x-next-cursor", "cursor_9")
                .insert_header("x-result-count", "2")
                .insert_header("x-truncated", "true")
                .set_body_bytes(buf),
        )
        .mount(&server)
        .await;

    let page = conversations
        .list_arrow(&ConversationListParams::default())
        .await
        .unwrap();
    assert_eq!(page.num_rows(), 2);
    assert_eq!(page.next.as_deref(), Some("cursor_9"));
    assert_eq!(page.count, Some(2));
    assert!(page.truncated);
}

fn file_response(name: &str) -> serde_json::Value {
    json!({
        "id": "00000000-0000-0000-0000-000000000abc",
        "org_id": "00000000-0000-0000-0000-00000000aaaa",
        "project_id": "00000000-0000-0000-0000-00000000bbbb",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "name": name,
        "file_type": "other",
        "storage_path": format!("/storage/{name}"),
        "mime_type": "application/octet-stream",
        "size_bytes": 5,
    })
}

fn task_response(status: &str) -> serde_json::Value {
    json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "org_id": "00000000-0000-0000-0000-00000000aaaa",
        "project_id": "00000000-0000-0000-0000-00000000bbbb",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "mode": "agent",
        "status": status,
        "is_archived": false,
    })
}

#[tokio::test]
async fn task_get_retries_on_429_then_succeeds() {
    // A status poll that trips the rate limit is retried transparently: the
    // 429 is served once (higher priority, single use), then the 200 wins.
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server));

    Mock::given(method("GET"))
        .and(path("/v1/tasks/abc"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_json(json!({"detail": "rate limited"})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/tasks/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_response("running")))
        .mount(&server)
        .await;

    let task = tasks.get("abc").await.unwrap();
    assert_eq!(task.id.to_string(), "00000000-0000-0000-0000-000000000001");
}

#[tokio::test]
async fn task_get_surfaces_429_after_exhausting_retries() {
    // A persistent 429 is retried up to the budget, then surfaced as a typed
    // HTTP error rather than looping forever.
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server)); // max_retries = 2

    Mock::given(method("GET"))
        .and(path("/v1/tasks/def"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_json(json!({"detail": "rate limited"})),
        )
        .mount(&server)
        .await;

    let err = tasks.get("def").await.unwrap_err();
    assert!(matches!(
        err,
        IntrospectionAPIError::Http { status: 429, .. }
    ));
}

#[tokio::test]
async fn get_retries_on_503_then_succeeds() {
    // A transient 503 on a GET (idempotent) is retried: 503 served once
    // (higher priority, single use), then the 200 wins.
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server));

    Mock::given(method("GET"))
        .and(path("/v1/tasks/abc"))
        .respond_with(
            ResponseTemplate::new(503).set_body_json(json!({"detail": "sandbox unavailable"})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/tasks/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_response("running")))
        .mount(&server)
        .await;

    let task = tasks.get("abc").await.unwrap();
    assert_eq!(task.id.to_string(), "00000000-0000-0000-0000-000000000001");
}

#[tokio::test]
async fn write_does_not_retry_on_503() {
    // A 503 on a non-idempotent write (POST /v1/tasks) is surfaced immediately,
    // never re-sent.
    let server = MockServer::start().await;
    let tasks = Tasks::new(build_http(&server)); // max_retries = 2

    Mock::given(method("POST"))
        .and(path("/v1/tasks"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({"detail": "unavailable"})))
        .mount(&server)
        .await;

    let err = tasks.create(&TaskCreate::default()).await.unwrap_err();
    assert!(matches!(
        err,
        IntrospectionAPIError::Http { status: 503, .. }
    ));
    // Exactly one attempt — no retry for a write.
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
