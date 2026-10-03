//! Resume run streams from content cursors, including output before the first attach.
//! Only a settling RUN_FINISHED or RUN_ERROR confirms completion. Nonterminal EOF
//! checks the run-scoped status and retries within the recovery budget. Replay
//! gaps remain visible to consumers; RunHandle::text rejects incomplete output.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_stream::stream;
use futures::stream::Stream;
use futures::StreamExt;
use serde_json::json;

use crate::agui::{introspection::reconnect_event, Event};
use crate::api::backoff::{backoff_delay, retry_after_from};
use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::schemas::{TaskRun, TaskStatus};
use crate::api::sse::{decode_agui_event, parse_sse_response, AG_UI_FRAME};

/// Options controlling the resilient run stream ([`stream_resumable`]).
#[derive(Debug, Clone)]
pub struct StreamOptions {
    /// Maximum consecutive reconnects with no forward progress before the
    /// stream gives up and yields a terminal `Err`. Reset whenever a reconnect
    /// delivers a new content cursor.
    pub max_reconnects: u32,
    /// Base step for the capped-exponential reconnect/readiness backoff.
    /// `Retry-After` is the floor on a `429`.
    pub backoff: Duration,
    /// Wall-clock deadline after which no further recovery is attempted.
    pub timeout: Duration,
    /// Emit an opt-in `introspection.reconnect` AG-UI `CUSTOM` event into the
    /// stream on each reconnect / readiness wait. Default `false` — the stream
    /// is otherwise fully transparent. Consumers branch on
    /// `Event::Custom(e) if e.name == agui::introspection::RECONNECT_EVENT_NAME`.
    pub emit_reconnect_events: bool,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            max_reconnects: 5,
            backoff: Duration::from_millis(500),
            timeout: Duration::from_secs(300),
            emit_reconnect_events: false,
        }
    }
}

/// A numeric SSE `id:` is a resumable content-frame cursor; control frames
/// (RUN_* lifecycle, heartbeats) carry a non-numeric `c-…` id that is not.
fn content_cursor(id: &Option<String>) -> bool {
    matches!(id, Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

/// Consume a run's SSE stream as a resumable sequence of typed AG-UI
/// [`Event`]s, reconnecting transparently on a mid-turn disconnect via
/// `Last-Event-ID`. See the module docs. Network drops are recovered
/// internally; a terminal `Err` is yielded only when recovery is exhausted or
/// the attach fails unrecoverably.
pub fn stream_resumable(
    http: Arc<HttpClient>,
    task_id: &str,
    run_id: &str,
    opts: StreamOptions,
) -> impl Stream<Item = ApiResult<Event>> {
    let path = format!(
        "/v1/tasks/{}/runs/{}/stream",
        crate::api::encoding::encode(task_id),
        crate::api::encoding::encode(run_id),
    );

    stream! {
        let start = Instant::now();
        let mut last_event_id = Some("0".to_string());
        let mut reconnects: u32 = 0;
        // Readiness waits are counted separately from reconnects: they are
        // not failed attempts, but the delay still has to grow or a DP that
        // sends no Retry-After is polled at a flat ~250ms for the whole
        // timeout window.
        let mut readiness_waits: u32 = 0;

        loop {
            match http
                .get_stream_raw(&path, Some("text/event-stream"), last_event_id.as_deref())
                .await
            {
                Ok(res) if res.status().as_u16() == 429 => {
                    // Not attachable yet — a readiness wait, not a failed attempt.
                    let retry_after = retry_after_from(res.headers());
                    let phase = readiness_phase(res).await;
                    let remaining = opts.timeout.checked_sub(start.elapsed());
                    match remaining {
                        None => {
                            yield Err(timeout_error());
                            return;
                        }
                        Some(rem) => {
                            if opts.emit_reconnect_events {
                                yield Ok(reconnect_event(json!({
                                    "reason": "readiness",
                                    "attempt": reconnects,
                                    "last_event_id": last_event_id,
                                    "phase": phase,
                                    "retry_after_ms": retry_after.map(|d| d.as_millis() as u64),
                                })));
                            }
                            tokio::time::sleep(
                                backoff_delay(readiness_waits, opts.backoff, retry_after)
                                    .min(rem),
                            )
                            .await;
                            readiness_waits = readiness_waits.saturating_add(1);
                            continue;
                        }
                    }
                }
                Ok(res) if res.status().is_success() => {
                    let sse = parse_sse_response(res);
                    futures::pin_mut!(sse);
                    let mut progressed = false;
                    let mut severed: Option<IntrospectionAPIError> = None;
                    while let Some(item) = sse.next().await {
                        match item {
                            Ok(frame) => {
                                // Transport frames (heartbeat / done / result)
                                // carry no AG-UI payload — skip them.
                                if frame.event != AG_UI_FRAME {
                                    continue;
                                }
                                match decode_agui_event(&frame.data) {
                                    Ok(event) => {
                                        let control = matches!(event, Event::RunStarted(_) | Event::RunFinished(_) | Event::RunError(_));
                                        if !control && content_cursor(&frame.id) {
                                            if let Some(cursor) = frame.id.as_ref().and_then(|id| id.parse::<u64>().ok()) {
                                                let previous = last_event_id.as_ref().and_then(|id| id.parse::<u64>().ok()).unwrap_or(0);
                                                if cursor <= previous { continue; }
                                                last_event_id = frame.id.clone();
                                                progressed = true;
                                            }
                                        }
                                        if matches!(&event, Event::RunFinished(e) if e.result.as_ref().and_then(|v| v.get("reason")).and_then(|v| v.as_str()) == Some("stream_close")) { continue; }
                                        let settled = matches!(event, Event::RunFinished(_) | Event::RunError(_));
                                        yield Ok(event);
                                        if settled { return; }
                                    }
                                    Err(e) => {
                                        // A malformed payload is terminal, like
                                        // a plain typed stream.
                                        yield Err(e);
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                severed = Some(e);
                                break;
                            }
                        }
                    }
                    if severed.is_none() {
                        let state = http.get_json::<_, TaskRun>(path.trim_end_matches("/stream"), &()).await.ok();
                        if let Some(state) = state {
                            if matches!(state.status, TaskStatus::Failed | TaskStatus::Cancelled) {
                                yield Err(IntrospectionAPIError::RunFailed { message: format!("The run ended with status {}", state.status.as_str()), code: Some("run_failed".into()) });
                                return;
                            }
                            if matches!(state.status, TaskStatus::Idle | TaskStatus::Completed | TaskStatus::AwaitingUser) {
                                yield Err(IntrospectionAPIError::StreamIncomplete("The run settled without a complete stream; read the conversation transcript".into()));
                                return;
                            }
                        }
                    }
                    reconnects = if progressed { 0 } else { reconnects + 1 };
                    if reconnects > opts.max_reconnects || start.elapsed() >= opts.timeout {
                        yield Err(severed.unwrap_or_else(|| IntrospectionAPIError::StreamIncomplete("The stream ended before the run settled".into())));
                        return;
                    }
                    if opts.emit_reconnect_events {
                        yield Ok(reconnect_event(json!({
                            "reason": if severed.is_some() { "severed" } else { "stream_close" },
                            "attempt": reconnects,
                            "last_event_id": last_event_id,
                        })));
                    }
                    let rem = opts.timeout.saturating_sub(start.elapsed());
                    tokio::time::sleep(backoff_delay(reconnects, opts.backoff, None).min(rem)).await;
                    continue;
                }
                // Other non-2xx — surface it (won't fix on retry).
                Ok(res) => {
                    let status = res.status().as_u16();
                    let request_id = res
                        .headers()
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    let body = res.text().await.ok().filter(|t| !t.is_empty());
                    yield Err(IntrospectionAPIError::http(
                        status,
                        format!("run stream attach failed (status={status})"),
                        request_id,
                        body.map(serde_json::Value::String),
                    ));
                    return;
                }
                // Transport error before any response.
                Err(e) => {
                    reconnects += 1;
                    if reconnects > opts.max_reconnects || start.elapsed() >= opts.timeout {
                        yield Err(e);
                        return;
                    }
                    if opts.emit_reconnect_events {
                        yield Ok(reconnect_event(json!({
                            "reason": "connect_error",
                            "attempt": reconnects,
                            "last_event_id": last_event_id,
                        })));
                    }
                    tokio::time::sleep(backoff_delay(reconnects, opts.backoff, None)).await;
                    continue;
                }
            }
        }
    }
}

/// Extract the readiness `status` phase from a `429` body (best effort).
async fn readiness_phase(res: reqwest::Response) -> Option<String> {
    let body = res.text().await.ok()?;
    let value: serde_json::Value = serde_json::from_str(&body).ok()?;
    value
        .get("status")
        .and_then(|s| s.as_str())
        .map(str::to_string)
}

fn timeout_error() -> IntrospectionAPIError {
    IntrospectionAPIError::Timeout(
        "run stream did not become attachable before the timeout".to_string(),
    )
}
