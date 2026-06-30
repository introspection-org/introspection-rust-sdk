//! Integration tests for transparent stream resume (INT-252).
//!
//! Drives the resilient run stream (`tasks.runs.stream_with`) against a tiny
//! raw-TCP mock DP. wiremock can't model a connection severed mid-body (its
//! hyper server rejects a short Content-Length), so a hand-rolled listener
//! gives byte-level control: a "severed" attach writes the valid frames, then
//! drops the socket under an over-stated `Content-Length` so `reqwest` raises a
//! transport error *after* delivering the frames — exactly a network drop. The
//! mock also records the `Last-Event-ID` header seen on each attach.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::agui::introspection::RECONNECT_EVENT_NAME;
use introspection_sdk::api::{HttpClient, HttpConfig, StreamOptions, TaskRuns};
use introspection_sdk::AgUiEvent;

const TASK_ID: &str = "00000000-0000-0000-0000-000000000001";
const RUN_ID: &str = "run_001";

fn content(id: &str, delta: &str) -> String {
    format!(
        "id: {id}\nevent: ag_ui\ndata: {{\"type\":\"TEXT_MESSAGE_CONTENT\",\
         \"messageId\":\"m\",\"delta\":\"{delta}\"}}\n\n"
    )
}

const FINISH: &str =
    "id: c-0\nevent: ag_ui\ndata: {\"type\":\"RUN_FINISHED\",\"threadId\":\"t\",\"runId\":\"run_001\"}\n\n";

type SeenLog = Arc<Mutex<Vec<Option<String>>>>;

/// Spawn a raw-TCP mock that serves `script` (one entry per attach, clamped at
/// the last) and returns its base URL plus the `Last-Event-ID` log.
fn spawn_mock(script: Vec<(&'static str, String)>) -> (String, SeenLog) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: SeenLog = Arc::new(Mutex::new(Vec::new()));
    let seen_thread = seen.clone();

    thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            let mut sock = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            let mut buf = [0u8; 8192];
            let n = sock.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let last_event_id = req.lines().find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("last-event-id:")
                    .map(|_| l[l.find(':').unwrap() + 1..].trim().to_string())
            });
            seen_thread.lock().unwrap().push(last_event_id);

            let (kind, body) = script[i.min(script.len() - 1)].clone();
            match kind {
                "clean" => {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes());
                    let _ = sock.write_all(body.as_bytes());
                }
                "severed" => {
                    // Advertise more than we send, then drop → reqwest sees the
                    // body end early and errors, after the frames are delivered.
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len() + 64
                    );
                    let _ = sock.write_all(head.as_bytes());
                    let _ = sock.write_all(body.as_bytes());
                    let _ = sock.flush();
                    // socket dropped here → connection reset mid-body
                }
                "429" => {
                    let json = b"{\"status\":\"provisioning\"}";
                    let head = format!(
                        "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\n\
                         Retry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        json.len()
                    );
                    let _ = sock.write_all(head.as_bytes());
                    let _ = sock.write_all(json);
                }
                _ => unreachable!(),
            }
            // sock dropped → connection closed
        }
    });

    (format!("http://{addr}"), seen)
}

fn http_for(url: &str) -> Arc<HttpClient> {
    let cfg = HttpConfig {
        api_url: url.to_string(),
        token: "intro_test".to_string(),
        additional_headers: std::collections::HashMap::new(),
        timeout: Duration::from_secs(5),
        // Streaming uses its own resume budget (get_stream_raw), not the unary
        // 429 retry; these values are irrelevant to these tests.
        max_retries: 0,
        retry_base: Duration::from_millis(1),
    };
    Arc::new(HttpClient::from_parts(reqwest::Client::new(), cfg))
}

fn opts() -> StreamOptions {
    StreamOptions {
        backoff: Duration::from_millis(1),
        ..Default::default()
    }
}

/// Collect every typed event the stream yields (errors surface as `Err`).
async fn collect_events(
    runs: &TaskRuns,
    o: StreamOptions,
) -> Result<Vec<AgUiEvent>, introspection_sdk::IntrospectionAPIError> {
    let stream = runs.stream_with(TASK_ID, RUN_ID, o);
    futures::pin_mut!(stream);
    let mut out = Vec::new();
    while let Some(item) = stream.next().await {
        out.push(item?);
    }
    Ok(out)
}

/// Text deltas (the assistant's streamed output), in order.
fn deltas(events: &[AgUiEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgUiEvent::TextMessageContent(c) => Some(c.delta.clone()),
            _ => None,
        })
        .collect()
}

/// The `introspection.reconnect` CUSTOM markers, in order.
fn reconnects(events: &[AgUiEvent]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter_map(|e| match e {
            AgUiEvent::Custom(c) if c.name == RECONNECT_EVENT_NAME => Some(c.value.clone()),
            _ => None,
        })
        .collect()
}

async fn collect_deltas(
    runs: &TaskRuns,
    o: StreamOptions,
) -> Result<Vec<String>, introspection_sdk::IntrospectionAPIError> {
    Ok(deltas(&collect_events(runs, o).await?))
}

#[tokio::test]
async fn clean_completion_single_attach() {
    let (url, seen) = spawn_mock(vec![(
        "clean",
        format!("{}{}{}", content("1", "a"), content("2", "b"), FINISH),
    )]);
    let runs = TaskRuns::new(http_for(&url));

    let deltas = collect_deltas(&runs, opts()).await.unwrap();
    assert_eq!(deltas, vec!["a", "b"]);
    assert_eq!(*seen.lock().unwrap(), vec![None]); // no resume header on first attach
}

#[tokio::test]
async fn mid_turn_drop_reattaches_with_last_event_id() {
    let (url, seen) = spawn_mock(vec![
        (
            "severed",
            format!("{}{}", content("1", "a"), content("2", "b")),
        ),
        ("clean", format!("{}{}", content("3", "c"), FINISH)),
    ]);
    let runs = TaskRuns::new(http_for(&url));

    let deltas = collect_deltas(&runs, opts()).await.unwrap();
    assert_eq!(deltas, vec!["a", "b", "c"]); // gap-free
                                             // Reconnect resumes from the last numeric content-frame id seen.
    assert_eq!(*seen.lock().unwrap(), vec![None, Some("2".to_string())]);
}

#[tokio::test]
async fn mid_turn_drop_emits_opt_in_reconnect_event() {
    // Same real socket-drop as above, but with reconnect events opted in: the
    // consumer sees an `introspection.reconnect` CUSTOM event for the recovery,
    // interleaved gap-free with the text deltas.
    let (url, _seen) = spawn_mock(vec![
        (
            "severed",
            format!("{}{}", content("1", "a"), content("2", "b")),
        ),
        ("clean", format!("{}{}", content("3", "c"), FINISH)),
    ]);
    let runs = TaskRuns::new(http_for(&url));

    let events = collect_events(
        &runs,
        StreamOptions {
            backoff: Duration::from_millis(1),
            emit_reconnect_events: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(deltas(&events), vec!["a", "b", "c"]); // still gap-free
    let markers = reconnects(&events);
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0]["reason"], "severed");
    assert_eq!(markers[0]["last_event_id"], "2");
}

#[tokio::test]
async fn transparent_by_default_no_reconnect_event() {
    let (url, _seen) = spawn_mock(vec![
        (
            "severed",
            format!("{}{}", content("1", "a"), content("2", "b")),
        ),
        ("clean", format!("{}{}", content("3", "c"), FINISH)),
    ]);
    let runs = TaskRuns::new(http_for(&url));

    let events = collect_events(&runs, opts()).await.unwrap();
    assert_eq!(deltas(&events), vec!["a", "b", "c"]);
    assert!(reconnects(&events).is_empty()); // opt-in: off by default
}

#[tokio::test]
async fn resume_cursor_ignores_control_ids() {
    let heartbeat = "id: c-9\nevent: heartbeat\ndata: {\"runId\":\"run_001\"}\n\n";
    let (url, seen) = spawn_mock(vec![
        ("severed", format!("{}{}", content("5", "a"), heartbeat)),
        ("clean", format!("{}{}", content("6", "b"), FINISH)),
    ]);
    let runs = TaskRuns::new(http_for(&url));

    let deltas = collect_deltas(&runs, opts()).await.unwrap();
    assert_eq!(deltas, vec!["a", "b"]);
    assert_eq!(*seen.lock().unwrap(), vec![None, Some("5".to_string())]); // "c-9" not a cursor
}

#[tokio::test]
async fn readiness_429_backs_off_then_attaches() {
    let (url, _seen) = spawn_mock(vec![
        ("429", String::new()),
        ("429", String::new()),
        ("clean", format!("{}{}", content("1", "a"), FINISH)),
    ]);
    let runs = TaskRuns::new(http_for(&url));

    let deltas = collect_deltas(&runs, opts()).await.unwrap();
    assert_eq!(deltas, vec!["a"]); // 429 never surfaced
}

#[tokio::test]
async fn readiness_429_emits_opt_in_reconnect_event_with_phase() {
    let (url, _seen) = spawn_mock(vec![
        ("429", String::new()),
        ("clean", format!("{}{}", content("1", "a"), FINISH)),
    ]);
    let runs = TaskRuns::new(http_for(&url));

    let events = collect_events(
        &runs,
        StreamOptions {
            backoff: Duration::from_millis(1),
            emit_reconnect_events: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(deltas(&events), vec!["a"]);
    let markers = reconnects(&events);
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0]["reason"], "readiness");
    // The `429` body's `status` is surfaced as the readiness phase.
    assert_eq!(markers[0]["phase"], "provisioning");
}

#[tokio::test]
async fn exhausts_reconnects_yields_err() {
    let (url, _seen) = spawn_mock(vec![("severed", String::new())]);
    let runs = TaskRuns::new(http_for(&url));

    let o = StreamOptions {
        backoff: Duration::from_millis(1),
        max_reconnects: 2,
        ..Default::default()
    };
    assert!(collect_deltas(&runs, o).await.is_err());
}

#[tokio::test]
async fn forward_progress_resets_budget() {
    let (url, _seen) = spawn_mock(vec![
        ("severed", content("1", "a")),
        ("severed", content("2", "b")),
        ("severed", content("3", "c")),
        ("clean", format!("{}{}", content("4", "d"), FINISH)),
    ]);
    let runs = TaskRuns::new(http_for(&url));

    let o = StreamOptions {
        backoff: Duration::from_millis(1),
        max_reconnects: 1,
        ..Default::default()
    };
    let deltas = collect_deltas(&runs, o).await.unwrap();
    assert_eq!(deltas, vec!["a", "b", "c", "d"]);
}
