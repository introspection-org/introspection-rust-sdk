//! Minimal Server-Sent Events parser over a [`reqwest::Response`] byte
//! stream.
//!
//! The DP does not define the event taxonomy — frames are proxied verbatim
//! from the agents-worker, so we yield raw [`SseEvent`] structs and let the
//! caller branch on `event` / `serde_json::from_str(&ev.data)`.

use bytes::Bytes;
use futures::stream::Stream;
use futures::StreamExt;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::schemas::SseEvent;

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
}
