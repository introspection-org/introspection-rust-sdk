//! Server-Sent Events parsing for the task-run stream.
//!
//! [`parse_sse_response`] is the low-level parser: it yields raw [`SseEvent`]
//! frames (the `event` / `data` / `id` wire shape) verbatim.
//! `decode_agui_event` (crate-internal) lifts one `ag_ui` frame's `data`
//! into a typed [`crate::agui::Event`]. The resumable stream
//! (`crate::api::resumable`)
//! composes the two, skipping transport frames (`heartbeat`, `done`,
//! `result`) as it tracks frame ids for resumption.

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
/// [`IntrospectionAPIError::Decode`]. Used by the resumable stream
/// (`crate::api::resumable`), which tracks frame ids itself and reuses this
/// for the decode step.
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
        // Bytes received that did not form a complete codepoint. A chunk
        // boundary lands wherever TCP put it, so any multi-byte character --
        // an emoji, an accent, any CJK text -- can straddle two chunks.
        let mut pending: Vec<u8> = Vec::new();
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
            // SSE is required to be UTF-8, but only the *stream* is; an
            // individual chunk can end mid-character. Decode as far as the
            // last complete codepoint and carry the remainder forward.
            pending.extend_from_slice(&bytes);
            let decoded_upto = match std::str::from_utf8(&pending) {
                Ok(s) => {
                    buf.push_str(s);
                    pending.len()
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    // `error_len() == None` means the bytes after `valid_up_to`
                    // are a truncated-but-legal prefix: more of the character
                    // is still in flight. Anything else is genuinely invalid.
                    if e.error_len().is_some() {
                        yield Err(IntrospectionAPIError::Decode(
                            "SSE stream emitted non-UTF-8 bytes".to_string(),
                        ));
                        return;
                    }
                    // Safe: `valid_up_to` is the length of the valid prefix.
                    buf.push_str(std::str::from_utf8(&pending[..valid]).expect("valid prefix"));
                    valid
                }
            };
            pending.drain(..decoded_upto);

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

    /// Parse a stream that has been chopped into the given byte-sized chunks,
    /// the way TCP actually delivers one.
    fn parse_chunks(chunks: Vec<Vec<u8>>) -> Vec<ApiResult<SseEvent>> {
        let chunks: Vec<Result<Bytes, reqwest::Error>> =
            chunks.into_iter().map(|c| Ok(Bytes::from(c))).collect();
        let s = stream::iter(chunks);
        let parsed = parse_sse_bytes(s);
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let mut out = Vec::new();
                tokio::pin!(parsed);
                while let Some(ev) = parsed.next().await {
                    out.push(ev);
                }
                out
            })
    }

    #[test]
    fn survives_a_codepoint_split_across_chunks() {
        // "data: caf\u{e9} \u{1f389}\n\n" cut mid-character twice: once inside the
        // two-byte e-acute, once inside the four-byte emoji.
        let full = "data: caf\u{e9} \u{1f389}\n\n".as_bytes().to_vec();
        let e_acute_mid = full.iter().position(|b| *b == 0xC3).unwrap() + 1;
        let emoji_mid = full.iter().position(|b| *b == 0xF0).unwrap() + 2;
        let events = parse_chunks(vec![
            full[..e_acute_mid].to_vec(),
            full[e_acute_mid..emoji_mid].to_vec(),
            full[emoji_mid..].to_vec(),
        ]);
        let events: Vec<_> = events.into_iter().map(|e| e.unwrap()).collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "caf\u{e9} \u{1f389}");
    }

    #[test]
    fn still_rejects_genuinely_invalid_utf8() {
        // 0xFF can never begin a UTF-8 sequence, so this is not a truncation.
        let events = parse_chunks(vec![b"data: ".to_vec(), vec![0xFF], b"\n\n".to_vec()]);
        assert!(matches!(
            events.first(),
            Some(Err(IntrospectionAPIError::Decode(_)))
        ));
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

    // --- typed decode (`decode_agui_event`) ---
    //
    // Frame filtering and error propagation live in the resumable stream and
    // are covered against a live mock server in tests/resumable_test.rs.

    #[test]
    fn decodes_a_known_ag_ui_event() {
        let ev = decode_agui_event(
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"run-1:text:0","delta":"hello"}"#,
        )
        .unwrap();
        match ev {
            Event::TextMessageContent(e) => {
                assert_eq!(e.message_id, "run-1:text:0");
                assert_eq!(e.delta, "hello");
            }
            other => panic!("expected TextMessageContent, got {other:?}"),
        }
    }

    #[test]
    fn an_unrecognised_event_type_decodes_to_unknown_rather_than_erroring() {
        // A future protocol addition must never sever a live stream.
        let ev = decode_agui_event(r#"{"type":"SOME_FUTURE_EVENT","x":1}"#).unwrap();
        assert!(matches!(ev, Event::Unknown));
    }

    #[test]
    fn a_structurally_invalid_payload_is_a_decode_error() {
        let err = decode_agui_event("not json").unwrap_err();
        assert!(matches!(err, IntrospectionAPIError::Decode(_)));
    }
}
