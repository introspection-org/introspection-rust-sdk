//! Pinned SSE/status exchanges shared with Swift, JavaScript and Python.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig, StreamOptions, TaskRuns};
use introspection_sdk::{AgUiEvent, IntrospectionAPIError};
use serde::Deserialize;
use serde_json::json;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TASK_ID: &str = "55555555-5555-5555-5555-555555555555";

#[derive(Clone, Deserialize)]
struct Scenario {
    name: String,
    streams: Vec<String>,
    statuses: Vec<String>,
    cursors: Vec<String>,
    deltas: Vec<String>,
    error: Option<String>,
    text_error: Option<String>,
    stream_delays_ms: Option<Vec<u64>>,
    timeout_ms: Option<u64>,
}

#[derive(Default)]
struct Seen {
    cursors: Vec<String>,
    reads: usize,
}

async fn setup(scenario: &Scenario) -> (MockServer, TaskRuns, Arc<Mutex<Seen>>) {
    let server = MockServer::start().await;
    let seen = Arc::new(Mutex::new(Seen::default()));
    let recorded = seen.clone();
    let fixture = scenario.clone();
    Mock::given(method("GET"))
        .respond_with(move |request: &Request| {
            let mut seen = recorded.lock().unwrap();
            if request.url.path().ends_with("/stream") {
                let index = seen.cursors.len().min(fixture.streams.len() - 1);
                seen.cursors.push(
                    request
                        .headers
                        .get("last-event-id")
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string(),
                );
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(
                        fixture
                            .stream_delays_ms
                            .as_ref()
                            .map_or(0, |delays| delays[index]),
                    ))
                    .set_body_raw(fixture.streams[index].clone(), "text/event-stream")
            } else {
                assert_eq!(
                    request.url.path(),
                    format!("/v1/tasks/{TASK_ID}/runs/run-1")
                );
                let status = &fixture.statuses[seen.reads];
                seen.reads += 1;
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id":"run-1","task_id":TASK_ID,"status":status}))
            }
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"run":{"id":"run-1","task_id":TASK_ID,"status":"running"}})),
        )
        .mount(&server)
        .await;
    let config = HttpConfig {
        api_url: server.uri(),
        token: "fixture".into(),
        additional_headers: Default::default(),
        timeout: Duration::from_secs(5),
        max_retries: 0,
        retry_base: Duration::from_millis(1),
    };
    let runs = TaskRuns::new(Arc::new(HttpClient::from_parts(
        reqwest::Client::new(),
        config,
    )));
    (server, runs, seen)
}

#[test]
fn fixture_hash() {
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        include_bytes!("fixtures/run-stream-contract.json"),
    );
    let hex: String = digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        hex,
        "f1dfd4501a3466442e1201210fc5c7a17150b03787405f22def762aaf511ea78"
    );
}

fn scenarios() -> Vec<Scenario> {
    serde_json::from_str(include_str!("fixtures/run-stream-contract.json")).unwrap()
}

fn error_kind(error: &IntrospectionAPIError) -> &str {
    match error {
        IntrospectionAPIError::StreamIncomplete(_) => "stream_incomplete",
        IntrospectionAPIError::RunFailed { .. } => "run_failed",
        _ => "unexpected",
    }
}

#[tokio::test]
async fn shared_stream_contract() {
    for fixture in scenarios() {
        let (_server, runs, seen) = setup(&fixture).await;
        let stream = runs.stream_with(
            TASK_ID,
            "run-1",
            StreamOptions {
                max_reconnects: 2,
                backoff: Duration::from_millis(1),
                timeout: Duration::from_millis(fixture.timeout_ms.unwrap_or(300000)),
                ..Default::default()
            },
        );
        futures::pin_mut!(stream);
        let mut deltas = Vec::new();
        let mut failure = None;
        while let Some(event) = stream.next().await {
            match event {
                Ok(AgUiEvent::TextMessageContent(e)) => deltas.push(e.delta),
                Ok(AgUiEvent::TextMessageChunk(e)) => deltas.extend(e.delta),
                Ok(AgUiEvent::RunFinished(e)) => {
                    assert_ne!(e.result, Some(json!({"reason":"stream_close"})))
                }
                Ok(_) => {}
                Err(e) => {
                    failure = Some(error_kind(&e).to_string());
                    break;
                }
            }
        }
        assert_eq!(deltas, fixture.deltas, "{}", fixture.name);
        assert_eq!(failure, fixture.error, "{}", fixture.name);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.cursors, fixture.cursors, "{}", fixture.name);
        assert_eq!(seen.reads, fixture.statuses.len(), "{}", fixture.name);
    }
}

#[tokio::test]
async fn text_outcomes() {
    for fixture in scenarios()
        .into_iter()
        .filter(|s| s.text_error.is_some() || s.name == "text_chunk")
    {
        let (_server, runs, _seen) = setup(&fixture).await;
        let request = Default::default();
        let handle = runs.create(TASK_ID, &request).await.unwrap();
        let result = handle.text().await;
        if let Some(expected) = fixture.text_error {
            assert_eq!(
                error_kind(&result.unwrap_err()),
                expected,
                "{}",
                fixture.name
            );
        } else {
            assert_eq!(result.unwrap(), "chunk");
        }
    }
}
