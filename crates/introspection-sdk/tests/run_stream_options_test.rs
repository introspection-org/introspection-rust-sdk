//! `StreamOptions::since` / `wait_for_start`, the raw-frame stream, the typed
//! `429` / `410` errors, and the task request fields the CLI sends.
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig, StreamOptions, TaskRuns};
use introspection_sdk::{
    AgUiEvent, IntrospectionAPIError, RunFrame, TaskListParams, TaskPrompt, TaskRunCreate,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TASK_ID: &str = "55555555-5555-5555-5555-555555555555";
const STREAM: &str = "/v1/tasks/55555555-5555-5555-5555-555555555555/runs/run-1/stream";

const FRAMES: &str = "id: 8\nevent: ag_ui\ndata: {\"type\":\"TEXT_MESSAGE_CONTENT\",\"messageId\":\"m\",\"delta\":\"hi\"}\n\n\
event: heartbeat\ndata: {}\n\n\
id: c-1\nevent: ag_ui\ndata: {\"type\":\"RUN_FINISHED\",\"threadId\":\"t\",\"runId\":\"run-1\"}\n\n";

fn runs(server: &MockServer) -> TaskRuns {
    let config = HttpConfig {
        api_url: server.uri(),
        token: "fixture".into(),
        additional_headers: Default::default(),
        timeout: Duration::from_secs(5),
        max_retries: 0,
        retry_base: Duration::from_millis(1),
    };
    TaskRuns::new(Arc::new(HttpClient::from_parts(
        reqwest::Client::new(),
        config,
    )))
}

fn from_cursor_seven() -> StreamOptions {
    StreamOptions {
        backoff: Duration::from_millis(1),
        since: 7,
        wait_for_start: Some(false),
        ..Default::default()
    }
}

// Keep the SDK error intact: the tests assert on its variant.
#[allow(clippy::result_large_err)]
async fn frames(
    runs: &TaskRuns,
    opts: StreamOptions,
) -> Result<Vec<RunFrame>, IntrospectionAPIError> {
    let stream = runs.stream_frames(TASK_ID, "run-1", opts);
    futures::pin_mut!(stream);
    let mut out = Vec::new();
    while let Some(item) = stream.next().await {
        out.push(item?);
    }
    Ok(out)
}

#[tokio::test]
async fn since_and_wait_for_start_reach_the_first_attach() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(STREAM))
        .and(query_param("wait_for_start", "0"))
        .and(header("last-event-id", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(FRAMES, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let got = frames(&runs(&server), from_cursor_seven()).await.unwrap();

    // The heartbeat is transport and is skipped; each event keeps its frame.
    assert_eq!(got.len(), 2);
    let first = got[0].frame.as_ref().expect("a server frame");
    assert_eq!(first.id.as_deref(), Some("8"));
    assert_eq!(first.event, "ag_ui");
    assert!(first.data.contains("\"delta\":\"hi\""));
    assert!(matches!(&got[0].event, AgUiEvent::TextMessageContent(e) if e.delta == "hi"));
    assert_eq!(got[1].frame.as_ref().unwrap().id.as_deref(), Some("c-1"));
    assert!(matches!(got[1].event, AgUiEvent::RunFinished(_)));
}

#[tokio::test]
async fn the_server_default_sends_no_wait_for_start() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(STREAM))
        .and(header("last-event-id", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(FRAMES, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let stream = runs(&server).stream_with(TASK_ID, "run-1", StreamOptions::default());
    futures::pin_mut!(stream);
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].url.query(), None);
}

#[tokio::test]
async fn a_410_is_stream_history_unavailable_at_the_cursor() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(STREAM))
        .respond_with(ResponseTemplate::new(410).set_body_json(json!({"detail": "history gone"})))
        .mount(&server)
        .await;

    let error = frames(&runs(&server), from_cursor_seven())
        .await
        .unwrap_err();

    match &error {
        IntrospectionAPIError::StreamHistoryUnavailable { cursor, message } => {
            assert_eq!(*cursor, 7);
            assert!(message.contains("history gone"), "{message}");
        }
        other => panic!("expected StreamHistoryUnavailable, got {other:?}"),
    }
    assert_eq!(error.status(), Some(410));
    assert_eq!(error.code(), Some("stream_history_unavailable"));
}

#[tokio::test]
async fn a_429_past_the_window_is_run_not_ready_with_its_phase() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(STREAM))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({"status": "provisioning"})),
        )
        .mount(&server)
        .await;

    let error = frames(
        &runs(&server),
        StreamOptions {
            timeout: Duration::from_millis(50),
            ..from_cursor_seven()
        },
    )
    .await
    .unwrap_err();

    match &error {
        IntrospectionAPIError::RunNotReady { phase, retry_after } => {
            assert_eq!(phase.as_deref(), Some("provisioning"));
            assert_eq!(*retry_after, Some(Duration::ZERO));
        }
        other => panic!("expected RunNotReady, got {other:?}"),
    }
    assert_eq!(error.status(), Some(429));
    assert_eq!(error.code(), Some("run_not_ready"));
}

#[tokio::test]
async fn a_reconnect_event_has_no_server_frame() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(STREAM))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({"status": "starting"})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(STREAM))
        .respond_with(ResponseTemplate::new(200).set_body_raw(FRAMES, "text/event-stream"))
        .mount(&server)
        .await;

    let got = frames(
        &runs(&server),
        StreamOptions {
            emit_reconnect_events: true,
            ..from_cursor_seven()
        },
    )
    .await
    .unwrap();

    assert!(got[0].frame.is_none());
    assert!(matches!(got[0].event, AgUiEvent::Custom(_)));
    assert!(got[1..].iter().all(|f| f.frame.is_some()));
}

#[test]
fn a_run_create_carries_its_delivery_id() {
    let body = TaskRunCreate {
        delivery_id: Some("018f0000-0000-7000-8000-000000000001".into()),
        prompt: Some(TaskPrompt {
            text: "again".into(),
            images: None,
        }),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&body).unwrap(),
        json!({
            "delivery_id": "018f0000-0000-7000-8000-000000000001",
            "prompt": {"text": "again"},
        })
    );
    assert_eq!(
        serde_json::to_value(TaskRunCreate::default()).unwrap(),
        json!({})
    );
}

#[tokio::test]
async fn a_task_list_filters_by_runtime_id() {
    let server = MockServer::start().await;
    let runtime = Uuid::parse_str("018f0000-0000-7000-8000-0000000000aa").unwrap();
    Mock::given(method("GET"))
        .and(path("/v1/tasks"))
        .and(query_param("runtime_id", runtime.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"records": [], "count": 0})))
        .expect(1)
        .mount(&server)
        .await;
    let tasks = introspection_sdk::api::Tasks::new(Arc::new(HttpClient::from_parts(
        reqwest::Client::new(),
        HttpConfig {
            api_url: server.uri(),
            token: "fixture".into(),
            additional_headers: Default::default(),
            timeout: Duration::from_secs(5),
            max_retries: 0,
            retry_base: Duration::from_millis(1),
        },
    )));

    let listed = tasks
        .list(&TaskListParams {
            runtime_id: Some(runtime),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .unwrap();
    assert!(listed.is_empty());
}
