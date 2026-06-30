//! Server-Sent Events parsing for the task-run stream.
//!
//! Two layers:
//!
//! - [`parse_sse_response`] is the low-level parser: it yields raw
//!   [`SseEvent`] frames (the `event` / `data` / `id` wire shape) verbatim.
//! - [`parse_agui_response`] is the typed layer used by
//!   [`crate::api::TaskRuns::stream`]: it keeps only `ag_ui` frames and
//!   decodes their `data` into a typed [`crate::agui::Event`], dropping
//!   transport frames (`heartbeat`, `done`, `result`).

use bytes::Bytes;
use futures::stream::Stream;
use futures::StreamExt;

use crate::agui::Event;
use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::schemas::SseEvent;

/// The SSE `event:` name carrying an AG-UI protocol event. Every other frame
/// name (`heartbeat`, `done`, `result`) is transport-level and skipped by the
/// typed layer.
pub(crate) const AG_UI_FRAME: &str = "ag_ui";

/// Decode an `ag_ui` frame's `data` payload into a typed [`Event`].
///
/// An unrecognised event `type` decodes to [`Event::Unknown`] (never an
/// error); a structurally invalid payload yields
/// [`IntrospectionAPIError::Decode`]. Shared by the plain typed layer
/// ([`parse_agui_frames`]) and the resumable stream (`crate::api::resumable`),
/// which tracks frame ids itself but reuses this for the decode step.
pub(crate) fn decode_agui_event(data: &str) -> ApiResult<Event> {
    serde_json::from_str::<Event>(data)
        .map_err(|e| IntrospectionAPIError::Decode(format!("failed to decode AG-UI event: {e}")))
}

/// Wrap a byte stream from a `text/event-stream` response in an async
/// [`Stream`] of parsed events.
///
/// The returned stream yields `Result<SseEvent, IntrospectionAPIError>`
/// items. Network drops surface as `Err(IntrospectionAPIError::Transport)`;
/// the stream then ends.
pub fn parse_sse_response(response: reqwest::Response) -> impl Stream<Item = ApiResult<SseEvent>> {
    let byte_stream = response.bytes_stream();
    parse_sse_bytes(byte_stream)
}

fn parse_sse_bytes<S>(stream: S) -> impl Stream<Item = ApiResult<SseEvent>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>>,
{
    async_stream::stream! {
        let mut buf = String::new();
        let mut cur = SseEvent::empty();
        let mut has_content = false;
        let mut stream = Box::pin(stream);

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    yield Err(IntrospectionAPIError::from(e));
                    return;
                }
            };
            // SSE is required to be UTF-8.
            match std::str::from_utf8(&bytes) {
                Ok(s) => buf.push_str(s),
                Err(_) => {
                    yield Err(IntrospectionAPIError::Decode(
                        "SSE stream emitted non-UTF-8 bytes".to_string(),
                    ));
                    return;
                }
            }

            while let Some(nl) = buf.find('\n') {
                let mut line = buf[..nl].to_string();
                buf.drain(..=nl);
                if line.ends_with('\r') {
                    line.pop();
                }

                if line.is_empty() {
                    if has_content {
                        yield Ok(cur);
                        cur = SseEvent::empty();
                        has_content = false;
                    }
                    continue;
                }
                if line.starts_with(':') {
                    continue;
                }
                let (field, raw_value) = match line.find(':') {
                    Some(i) => (&line[..i], &line[i + 1..]),
                    None => (line.as_str(), ""),
                };
                let value = raw_value.strip_prefix(' ').unwrap_or(raw_value);
                match field {
                    "event" => {
                        cur.event = value.to_string();
                        has_content = true;
                    }
                    "data" => {
                        if cur.data.is_empty() {
                            cur.data.push_str(value);
                        } else {
                            cur.data.push('\n');
                            cur.data.push_str(value);
                        }
                        has_content = true;
                    }
                    "id" => {
                        cur.id = Some(value.to_string());
                        has_content = true;
                    }
                    "retry" => {
                        if let Ok(n) = value.parse::<u64>() {
                            cur.retry = Some(n);
                            has_content = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        if has_content {
            yield Ok(cur);
        }
    }
}

/// Adapt a raw `text/event-stream` [`reqwest::Response`] into a typed
/// [`Event`] stream.
///
/// Only `ag_ui` frames are surfaced; transport frames (`heartbeat`, `done`,
/// `result`) are dropped. A frame whose `data` fails to decode into an
/// [`Event`] yields [`IntrospectionAPIError::Decode`] and ends the stream —
/// the same terminal behaviour a transport drop has. An unrecognised event
/// `type` decodes to [`Event::Unknown`] rather than erroring, so a future
/// protocol addition never severs the stream.
pub fn parse_agui_response(response: reqwest::Response) -> impl Stream<Item = ApiResult<Event>> {
    parse_agui_frames(parse_sse_response(response))
}

/// Lift a raw [`SseEvent`] stream into a typed [`Event`] stream. Split out
/// from [`parse_agui_response`] so it can be unit-tested without a live HTTP
/// response.
fn parse_agui_frames<S>(frames: S) -> impl Stream<Item = ApiResult<Event>>
where
    S: Stream<Item = ApiResult<SseEvent>>,
{
    async_stream::stream! {
        let mut frames = Box::pin(frames);
        while let Some(frame) = frames.next().await {
            let frame = match frame {
                Ok(f) => f,
                Err(e) => {
                    yield Err(e);
                    return;
                }
            };
            // Transport frames (heartbeat / done / result) carry no AG-UI
            // payload — skip them.
            if frame.event != AG_UI_FRAME {
                continue;
            }
            match decode_agui_event(&frame.data) {
                Ok(event) => yield Ok(event),
                Err(e) => {
                    yield Err(e);
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::stream;
    use futures::StreamExt;

    fn parse_str(input: &str) -> Vec<SseEvent> {
        // Build a single-chunk stream of bytes that never errors.
        let chunks: Vec<Result<Bytes, reqwest::Error>> = vec![Ok(Bytes::from(input.to_string()))];
        let s = stream::iter(chunks);
        let parsed = parse_sse_bytes(s);
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let mut out = Vec::new();
                tokio::pin!(parsed);
                while let Some(ev) = parsed.next().await {
                    out.push(ev.unwrap());
                }
                out
            })
    }

    #[test]
    fn parses_simple_message() {
        let events = parse_str("data: hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "message");
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn joins_multiline_data() {
        let events = parse_str("data: line1\ndata: line2\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2");
    }

    #[test]
    fn switches_event_name() {
        let events = parse_str("event: text\ndata: hi\n\nevent: done\ndata: bye\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "text");
        assert_eq!(events[0].data, "hi");
        assert_eq!(events[1].event, "done");
        assert_eq!(events[1].data, "bye");
    }

    #[test]
    fn ignores_comments() {
        let events = parse_str(":heartbeat\ndata: hi\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hi");
    }

    #[test]
    fn handles_crlf() {
        let events = parse_str("data: hi\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hi");
    }

    #[test]
    fn captures_id_and_retry() {
        let events = parse_str("id: 42\nretry: 1500\ndata: hi\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id.as_deref(), Some("42"));
        assert_eq!(events[0].retry, Some(1500));
    }

    // --- typed AG-UI layer (`parse_agui_frames`) ---

    fn agui_frame(event: &str, data: &str) -> ApiResult<SseEvent> {
        Ok(SseEvent {
            event: event.to_string(),
            data: data.to_string(),
            id: None,
            retry: None,
        })
    }

    fn collect_agui(frames: Vec<ApiResult<SseEvent>>) -> Vec<ApiResult<Event>> {
        let s = parse_agui_frames(stream::iter(frames));
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let mut out = Vec::new();
                tokio::pin!(s);
                while let Some(ev) = s.next().await {
                    out.push(ev);
                }
                out
            })
    }

    #[test]
    fn typed_layer_decodes_ag_ui_frames() {
        let out = collect_agui(vec![agui_frame(
            "ag_ui",
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"run-1:text:0","delta":"hello"}"#,
        )]);
        assert_eq!(out.len(), 1);
        match out.into_iter().next().unwrap().unwrap() {
            Event::TextMessageContent(e) => {
                assert_eq!(e.message_id, "run-1:text:0");
                assert_eq!(e.delta, "hello");
            }
            other => panic!("expected TextMessageContent, got {other:?}"),
        }
    }

    #[test]
    fn typed_layer_skips_transport_frames() {
        let out = collect_agui(vec![
            agui_frame("heartbeat", r#"{"runId":"r1"}"#),
            agui_frame(
                "ag_ui",
                r#"{"type":"RUN_STARTED","threadId":"t1","runId":"r1"}"#,
            ),
            agui_frame("done", "{}"),
        ]);
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out.into_iter().next().unwrap().unwrap(),
            Event::RunStarted(_)
        ));
    }

    #[test]
    fn typed_layer_propagates_transport_error() {
        let out = collect_agui(vec![
            agui_frame(
                "ag_ui",
                r#"{"type":"RUN_STARTED","threadId":"t1","runId":"r1"}"#,
            ),
            Err(IntrospectionAPIError::Decode("boom".to_string())),
        ]);
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0], Ok(Event::RunStarted(_))));
        assert!(matches!(out[1], Err(IntrospectionAPIError::Decode(_))));
    }

    #[test]
    fn typed_layer_unknown_event_does_not_error() {
        let out = collect_agui(vec![agui_frame(
            "ag_ui",
            r#"{"type":"SOME_FUTURE_EVENT","x":1}"#,
        )]);
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out.into_iter().next().unwrap().unwrap(),
            Event::Unknown
        ));
    }
}
