//! Transparent stream resume for the task-run stream (INT-252).
//!
//! See `docs/design/sdk-resumable-streams.md` in `introspection-cloud`.
//!
//! A turn is consumed over a long-lived SSE stream that can be severed before
//! the turn settles (gateway idle-timeout, load-balancer recycle, network
//! blip). Rather than surface that as a turn failure — losing every event
//! between the drop and a manual retry — the run stream reconnects
//! **transparently**: it tracks the last content-frame id and re-attaches with
//! the SSE-standard `Last-Event-ID` header, so the server replays the frames
//! the client missed and the `Stream` yields a single gap-free sequence of
//! typed AG-UI [`Event`]s. There is **no consumer-visible change**: the stream
//! ends when the turn finishes and yields a terminal `Err` only once recovery
//! is exhausted, exactly like a plain stream.
//!
//! Readiness folds in the same way: a not-yet-attachable run answers the attach
//! with `429` + `Retry-After`, which is honoured as a backoff floor and retried
//! — never surfaced to the caller.
//!
//! Resume is otherwise invisible. Callers that *want* to observe it (to show a
//! "reconnecting…" affordance or record telemetry) opt into
//! [`StreamOptions::emit_reconnect_events`], which injects an
//! `introspection.reconnect` AG-UI `CUSTOM` event
//! ([`crate::agui::introspection`]) into the stream on each reconnect /
//! readiness wait — the same marker channel the JS / Python SDKs use, so it is
//! expressible identically across all three.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_stream::stream;
use futures::stream::Stream;
use futures::StreamExt;
use serde_json::json;

use crate::agui::{introspection::reconnect_event, Event};
use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::sse::{decode_agui_event, parse_sse_response, AG_UI_FRAME};

/// Cap on the reconnect/readiness backoff.
const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Options controlling the resilient run stream ([`stream_resumable`]).
#[derive(Debug, Clone)]
pub struct StreamOptions {
    /// Maximum consecutive reconnects with no forward progress before the
    /// stream gives up and yields a terminal `Err`. Reset whenever a reconnect
    /// delivers a new event.
    pub max_reconnects: u32,
    /// Base step for the capped-exponential reconnect/readiness backoff.
    /// `Retry-After` is the floor on a `429`.
    pub backoff: Duration,
    /// Overall wall-clock deadline for the whole turn.
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

/// `Retry-After` as the floor of a capped-exponential step (`base * 2^n`).
fn retry_backoff(n: u32, retry_after: Option<Duration>, base: Duration) -> Duration {
    let factor = 1u64.checked_shl(n.min(20)).unwrap_or(u64::MAX);
    let exp_ms = (base.as_millis() as u64).saturating_mul(factor);
    let exp = Duration::from_millis(exp_ms).min(MAX_BACKOFF);
    retry_after.map(|ra| ra.max(exp)).unwrap_or(exp)
}

/// A numeric SSE `id:` is a resumable content-frame cursor; control frames
/// (RUN_* lifecycle, heartbeats) carry a non-numeric `c-…` id that is not.
fn content_cursor(id: &Option<String>) -> bool {
    matches!(id, Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

/// Consume a run's SSE stream as a single gap-free sequence of typed AG-UI
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
        urlencode(task_id),
        urlencode(run_id),
    );

    stream! {
        let start = Instant::now();
        let mut last_event_id: Option<String> = None;
        let mut reconnects: u32 = 0;

        loop {
            match http
                .get_stream_raw(&path, Some("text/event-stream"), last_event_id.as_deref())
                .await
            {
                Ok(res) if res.status().as_u16() == 429 => {
                    // Not attachable yet — a readiness wait, not a failed attempt.
                    let retry_after = res
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.trim().parse::<f64>().ok())
                        .map(Duration::from_secs_f64);
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
                                retry_backoff(reconnects, retry_after, opts.backoff).min(rem),
                            )
                            .await;
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
                                if content_cursor(&frame.id) {
                                    last_event_id = frame.id.clone();
                                }
                                // Transport frames (heartbeat / done / result)
                                // carry no AG-UI payload — skip them.
                                if frame.event != AG_UI_FRAME {
                                    continue;
                                }
                                match decode_agui_event(&frame.data) {
                                    Ok(event) => {
                                        progressed = true;
                                        yield Ok(event);
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
                    match severed {
                        // Clean EOF: the DP closed the stream on turn completion.
                        None => return,
                        Some(e) => {
                            // Forward progress resets the budget; a reconnect
                            // that delivers nothing counts down.
                            reconnects = if progressed { 0 } else { reconnects + 1 };
                            if reconnects > opts.max_reconnects
                                || start.elapsed() >= opts.timeout
                            {
                                yield Err(e);
                                return;
                            }
                            if opts.emit_reconnect_events {
                                yield Ok(reconnect_event(json!({
                                    "reason": "severed",
                                    "attempt": reconnects,
                                    "last_event_id": last_event_id,
                                })));
                            }
                            let rem = opts.timeout.saturating_sub(start.elapsed());
                            tokio::time::sleep(
                                retry_backoff(reconnects, None, opts.backoff).min(rem),
                            )
                            .await;
                            continue;
                        }
                    }
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
                    tokio::time::sleep(retry_backoff(reconnects, None, opts.backoff)).await;
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
    IntrospectionAPIError::Decode(
        "run stream did not become attachable before the timeout".to_string(),
    )
}

/// RFC 3986 path-segment percent encoding (mirrors `tasks::urlencode`).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
