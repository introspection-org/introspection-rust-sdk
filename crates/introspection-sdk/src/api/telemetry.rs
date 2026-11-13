//! `conversations()` / `events()` / `metrics()` — Data-Plane telemetry reads.
//!
//! These are **Data-Plane-scoped** (DP bearer + `events:read`), part of
//! [`crate::DataPlaneResources`] on both the [`crate::Runner`] and the
//! [`crate::IntrospectionClient`]. Two append-only stores back the reads —
//! `otel_traces` → [`Conversations`] (`GET /v1/conversations`) and `otel_logs`
//! → [`Events`] (`GET /v1/events`) — while all aggregation goes through the
//! bounded [`Metrics`] surface (`POST /v1/metrics`).
//!
//! # Ergonomic window params
//!
//! The list params ([`ConversationListParams`] / [`EventListParams`]) and the
//! [`MetricsQuery`] take ergonomic `order` / `start` / `end` / `lookback`
//! inputs. `lookback` (relative, e.g. `"24h"`) is **mutually exclusive** with
//! `start`/`end`; the conflict is rejected client-side (a typed
//! [`crate::IntrospectionAPIError::InvalidConfig`]) *before* any request is sent. See
//! [`crate::api::schemas`].
//!
//! # Optional Arrow
//!
//! With the `arrow` Cargo feature, `list_arrow` requests the
//! `application/vnd.apache.arrow.stream` response and decodes the Arrow IPC
//! stream, reading pagination metadata from response headers into an
//! `ArrowPage`. The DP answers `406` when the Arrow format is unsupported.
//!
//! Not a doc link: `api::arrow` exists only under that feature, so linking
//! it breaks the build of these docs for everyone who does not enable it.

use std::sync::Arc;

use crate::api::conversation_items::ConversationItems;
use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::genai_span::GenAiSpan;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Conversation, ConversationExportParams, ConversationItemGetParams, ConversationItemListParams,
    ConversationListParams, Event, EventListParams, MetricsQuery, MetricsResponse, Trajectory,
};
use bytes::Bytes;
use futures::{Stream, StreamExt};

/// Base media type of a trajectory-v1 conversation export. The `version`
/// parameter is appended by the caller; a server that does not implement the
/// requested version answers `406` rather than silently serving another shape.
pub const TRAJECTORY_MEDIA_TYPE: &str = "application/vnd.letta.trajectory+json";

/// Accept header selecting the pinned trajectory-v1 export representation.
pub const TRAJECTORY_V1_ACCEPT: &str = "application/vnd.letta.trajectory+json;version=1";

/// Accept header selecting the pinned replay-v1 export: the turn-aware
/// representation a managed Recipe replays a conversation from.
pub const REPLAY_V1_ACCEPT: &str =
    "application/vnd.introspection.conversation-replay+json;version=1";

/// Accept header selecting Harbor's ATIF trajectory. No `version` is pinned:
/// the server serves the ATIF schema version its own release implements, and
/// the body names it in `schema_version`.
pub const ATIF_ACCEPT: &str = "application/vnd.harbor.atif+json";

/// Accept header selecting the JSON-lines export: one GenAI span per line,
/// with no envelope.
pub const JSON_LINES_ACCEPT: &str = "application/x-ndjson";

/// Wire representation selected for a complete conversation export — one of
/// the representations `GET /v1/conversations/{id}/export` negotiates.
///
/// The span export is [`Self::JsonLines`]: `application/x-ndjson`, one GenAI
/// span per line, each line a complete JSON object, so it can be read as it
/// streams. The JSON-envelope export (`application/json`, the whole span list
/// in one document) was removed from this SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConversationExportFormat {
    Trajectory,
    Arrow,
    /// The turn-aware replay export ([`REPLAY_V1_ACCEPT`]).
    Replay,
    /// Harbor's ATIF trajectory ([`ATIF_ACCEPT`]).
    Atif,
    /// JSON lines ([`JSON_LINES_ACCEPT`]): one GenAI span per line, each line
    /// a complete JSON object.
    JsonLines,
}

impl ConversationExportFormat {
    /// The `Accept` header that negotiates this representation.
    pub fn accept(self) -> &'static str {
        match self {
            Self::Trajectory => TRAJECTORY_V1_ACCEPT,
            Self::Arrow => "application/vnd.apache.arrow.stream",
            Self::Replay => REPLAY_V1_ACCEPT,
            Self::Atif => ATIF_ACCEPT,
            Self::JsonLines => JSON_LINES_ACCEPT,
        }
    }
}

/// Decode a JSON-lines body read from `route`: one `T` per line, each
/// yielded once its newline arrives, wherever the chunk boundaries fall.
pub(crate) fn parse_json_lines<S, T>(stream: S, route: String) -> impl Stream<Item = ApiResult<T>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>>,
    T: crate::api::http::ResponseBody,
{
    async_stream::stream! {
        // The bytes of the line still waiting for its newline.
        let mut pending: Vec<u8> = Vec::new();
        let mut stream = Box::pin(stream);

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    yield Err(IntrospectionAPIError::from(e));
                    return;
                }
            };
            let mut rest = &bytes[..];
            while let Some(newline) = rest.iter().position(|b| *b == b'\n') {
                pending.extend_from_slice(&rest[..newline]);
                rest = &rest[newline + 1..];
                let line = std::mem::take(&mut pending);
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let decoded = crate::api::http::decode_slice(&line, &route);
                let failed = decoded.is_err();
                yield decoded;
                if failed {
                    return;
                }
            }
            pending.extend_from_slice(rest);
        }
        // A body cut off mid-line fails here rather than ending early.
        if !pending.iter().all(u8::is_ascii_whitespace) {
            yield crate::api::http::decode_slice(&pending, &route);
        }
    }
}

#[cfg(feature = "arrow")]
use crate::api::arrow::{decode_arrow_response, ArrowPage, ARROW_STREAM_ACCEPT};

/// The response header that carries the opaque cursor to pass as `next`: an
/// oldest-first conversation export's resume cursor (for
/// [`ConversationExportParams::next`]), and an Arrow list page's next-page
/// cursor.
pub const NEXT_CURSOR_HEADER: &str = "x-next-cursor";

/// What [`Conversations::export_resumable`] answered.
///
/// The cursor is opaque: it embeds a nanosecond timestamp, but pass it back
/// exactly as it was received rather than parsing it.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConversationExport<S> {
    /// `200`: the export body, streamed as it arrives. `next` is the
    /// `X-Next-Cursor` cursor, which the server answers for an
    /// oldest-first export (or any request with `next`); a newest-first export
    /// carries none.
    Body {
        /// The response bytes, passed through unbuffered.
        body: S,
        /// The cursor to resume after this export.
        next: Option<String>,
    },
    /// `204 No Content`: nothing is newer than the `next` that was sent.
    /// `next` echoes that cursor, to pass again on the following attempt.
    NothingNewer {
        /// The same cursor, echoed by the server.
        next: String,
    },
}

impl<S> ConversationExport<S> {
    /// The cursor to resume after this export, whichever way it answered.
    pub fn next(&self) -> Option<&str> {
        match self {
            Self::Body { next, .. } => next.as_deref(),
            Self::NothingNewer { next } => Some(next),
        }
    }
}

/// `runner.conversations.*` — `GET /v1/conversations` (append-only
/// `otel_traces`).
#[derive(Clone)]
pub struct Conversations {
    http: Arc<HttpClient>,
    /// Items of a conversation (`/v1/conversations/{id}/items`).
    pub items: ConversationItems,
}

impl Conversations {
    /// How many pages [`Self::retrieve`] walks looking for the latest turn
    /// when the caller did not name one.
    pub const TURN_SCAN_MAX_PAGES: usize = 2;

    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self {
            items: ConversationItems::new(http.clone()),
            http,
        }
    }

    /// `GET /v1/conversations` — cursor paginator (JSON).
    ///
    /// The returned [`Paginator<Conversation>`] auto-paginates as a
    /// [`futures::Stream`] and also exposes [`Paginator::next_page`] /
    /// [`Paginator::collect_all`]. Returns [`IntrospectionAPIError::InvalidConfig`]
    /// up front for an out-of-range `limit` or a `lookback`/`start`/`end`
    /// conflict.
    ///
    /// [`IntrospectionAPIError::InvalidConfig`]: crate::api::error::IntrospectionAPIError::InvalidConfig
    pub fn list(&self, params: &ConversationListParams) -> ApiResult<Paginator<Conversation>> {
        let wire = params.to_wire()?;
        Paginator::new(self.http.clone(), "/v1/conversations", &wire)
    }

    /// Fetch one conversation summary with its complete agent index.
    pub async fn get(&self, conversation_id: &str) -> ApiResult<Conversation> {
        self.http
            .get_json(
                &format!(
                    "/v1/conversations/{}",
                    crate::api::encoding::encode(conversation_id)
                ),
                &(),
            )
            .await
    }

    fn export_path(conversation_id: &str) -> String {
        format!(
            "/v1/conversations/{}/export",
            crate::api::encoding::encode(conversation_id)
        )
    }

    /// `GET /v1/conversations/{id}/export` as JSON lines
    /// (`application/x-ndjson`) — one complete conversation, a GenAI span at
    /// a time.
    ///
    /// The body is one span per line, each line a complete JSON object. Each
    /// span is decoded as its line arrives, so the export is never held
    /// whole. This replaces `export_json`, the JSON-envelope export, which
    /// was removed.
    pub async fn export_spans(
        &self,
        conversation_id: &str,
        params: &ConversationExportParams,
    ) -> ApiResult<impl Stream<Item = ApiResult<GenAiSpan>>> {
        let path = Self::export_path(conversation_id);
        let response = self
            .http
            .get_raw(&path, &params.to_wire(), Some(JSON_LINES_ACCEPT))
            .await?;
        Ok(parse_json_lines(response.bytes_stream(), path))
    }

    /// `GET /v1/conversations/{id}/export` in a representation that is one
    /// JSON document, returned as the server sent it.
    ///
    /// For the representations this SDK does not model — the replay and ATIF
    /// exports — and for callers that forward an export unchanged. The typed
    /// reads are [`Self::export_spans`] and [`Self::export_trajectory`].
    /// [`ConversationExportFormat::Arrow`] and
    /// [`ConversationExportFormat::JsonLines`] are not one JSON document, so
    /// they are refused with [`IntrospectionAPIError::InvalidConfig`] before
    /// any request.
    pub async fn export_json_value(
        &self,
        conversation_id: &str,
        format: ConversationExportFormat,
        params: &ConversationExportParams,
    ) -> ApiResult<serde_json::Value> {
        if format == ConversationExportFormat::Arrow {
            return Err(IntrospectionAPIError::InvalidConfig(
                "the Arrow export is not JSON; use export_arrow or export_stream".to_string(),
            ));
        }
        if format == ConversationExportFormat::JsonLines {
            return Err(IntrospectionAPIError::InvalidConfig(
                "the JSON-lines export is not one JSON document; use export_spans or export_stream"
                    .to_string(),
            ));
        }
        let response = self
            .http
            .get_raw(
                &Self::export_path(conversation_id),
                &params.to_wire(),
                Some(format.accept()),
            )
            .await?;
        crate::api::http::decode_json(response).await
    }

    /// Stream a complete export's response bytes, in any `format`, without
    /// buffering them in the SDK. With
    /// [`ConversationExportFormat::JsonLines`] the bytes are JSON lines: one
    /// GenAI span per line, each line a complete JSON object. The server walks storage in bounded pages; this stream passes
    /// each response chunk through as it arrives.
    ///
    /// This drops the response headers, so it cannot return the resume cursor
    /// of an oldest-first export, and a resumed export with nothing newer
    /// (`204`) is an empty stream. Use [`Self::export_resumable`] for those.
    pub async fn export_stream(
        &self,
        conversation_id: &str,
        format: ConversationExportFormat,
        params: &ConversationExportParams,
    ) -> ApiResult<impl Stream<Item = ApiResult<Bytes>>> {
        let response = self
            .http
            .get_raw(
                &Self::export_path(conversation_id),
                &params.to_wire(),
                Some(format.accept()),
            )
            .await?;
        Ok(response
            .bytes_stream()
            .map(|chunk| chunk.map_err(IntrospectionAPIError::from)))
    }

    /// [`Self::export_stream`] with the resume cursor: `GET
    /// /v1/conversations/{id}/export` in any `format`, returning the
    /// `X-Next-Cursor` header alongside the streamed body.
    ///
    /// Set [`ConversationExportParams::direction`] to
    /// [`SortDirection::Asc`](crate::api::SortDirection::Asc) for an
    /// oldest-first export, and pass the returned cursor as
    /// [`ConversationExportParams::next`] to resume after it. The header is
    /// read before the body streams. A resumed export with nothing newer
    /// answers `204`, returned as [`ConversationExport::NothingNewer`] with the
    /// cursor the server echoed (or, if it echoed none, the one that was sent).
    pub async fn export_resumable(
        &self,
        conversation_id: &str,
        format: ConversationExportFormat,
        params: &ConversationExportParams,
    ) -> ApiResult<ConversationExport<impl Stream<Item = ApiResult<Bytes>>>> {
        let response = self
            .http
            .get_raw(
                &Self::export_path(conversation_id),
                &params.to_wire(),
                Some(format.accept()),
            )
            .await?;
        let next = match response.headers().get(NEXT_CURSOR_HEADER) {
            Some(value) => Some(
                value
                    .to_str()
                    .map_err(|_| {
                        IntrospectionAPIError::Decode(format!(
                            "the export's {NEXT_CURSOR_HEADER} header is not visible ASCII"
                        ))
                    })?
                    .to_string(),
            ),
            None => None,
        };
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            let next = next.or_else(|| params.next.clone()).ok_or_else(|| {
                IntrospectionAPIError::Decode(format!(
                    "the export answered 204 without an {NEXT_CURSOR_HEADER} cursor"
                ))
            })?;
            return Ok(ConversationExport::NothingNewer { next });
        }
        let body = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(IntrospectionAPIError::from));
        Ok(ConversationExport::Body { body, next })
    }

    /// `GET /v1/conversations` with `Accept: application/vnd.apache.arrow.stream`
    /// — one Arrow page. Pagination metadata comes from response headers
    /// (`X-Next-Cursor` is load-bearing for the next page). Requires the
    /// `arrow` feature.
    #[cfg(feature = "arrow")]
    pub async fn list_arrow(&self, params: &ConversationListParams) -> ApiResult<ArrowPage> {
        let wire = params.to_wire()?;
        let res = self
            .http
            .get_raw("/v1/conversations", &wire, Some(ARROW_STREAM_ACCEPT))
            .await?;
        decode_arrow_response(res).await
    }
    /// `GET /v1/conversations/{id}/export` — one complete conversation as
    /// trajectory-v1.
    ///
    /// This is not [`ConversationItems::list`] in another coat. The export is
    /// assembled server-side over the whole conversation, so it carries no
    /// cursor and no page bound; `params` filters what gets assembled.
    ///
    /// The trajectory is a projection derived on read from the stored GenAI
    /// messages, so a conversation that cannot be represented as trajectory-v1
    /// answers `422` rather than returning a partial export, and one with no
    /// exportable records answers `404`.
    pub async fn export_trajectory(
        &self,
        conversation_id: &str,
        params: &ConversationExportParams,
    ) -> ApiResult<Trajectory> {
        let res = self
            .http
            .get_raw(
                &Self::export_path(conversation_id),
                &params.to_wire(),
                Some(TRAJECTORY_V1_ACCEPT),
            )
            .await?;
        crate::api::http::decode_json(res).await
    }

    /// Load the state of a conversation as of one item.
    ///
    /// Returns the span itself: the items read already carries the full input
    /// history, system instructions, and tool definitions, so composing a
    /// second type from it would just be copying fields into different names.
    ///
    /// When `item_id` is `None` the latest LLM turn is used — the first item
    /// (the route is descending-only) whose `gen_ai.operation.name` is
    /// `"chat"`, falling back to the first item that produced any output.
    /// Returns `Ok(None)` when the conversation has no items.
    ///
    /// For the full per-turn transcript instead, walk
    /// [`ConversationItems::list`], which is newest-first.
    pub async fn retrieve(
        &self,
        conversation_id: &str,
        item_id: Option<&str>,
    ) -> ApiResult<Option<GenAiSpan>> {
        let target_id = match item_id {
            Some(id) => Some(id.to_string()),
            None => self.find_latest_turn_id(conversation_id).await?,
        };
        let Some(target_id) = target_id else {
            return Ok(None);
        };
        let span = self
            .items
            .get(
                conversation_id,
                &target_id,
                &ConversationItemGetParams::default(),
            )
            .await?;
        Ok(Some(span))
    }

    /// Scan items for the most recent LLM turn. The route sorts descending
    /// and rejects a cursor that disagrees, so the first match is the latest.
    ///
    /// Bounded to [`Self::TURN_SCAN_MAX_PAGES`] pages: the latest turn is at
    /// the head of a descending sort, so walking further only refines a
    /// fallback. Without the bound a conversation whose items are all
    /// non-`chat` paged to exhaustion on every convenience read.
    async fn find_latest_turn_id(&self, conversation_id: &str) -> ApiResult<Option<String>> {
        let mut pages = self
            .items
            .list(conversation_id, &ConversationItemListParams::default())?;
        let mut fallback: Option<String> = None;
        let mut scanned = 0usize;
        while let Some(page) = pages.next_page().await? {
            for item in page.data {
                if item.operation_name() == Some("chat") {
                    return Ok(item.span_id);
                }
                if fallback.is_none() && !item.output_messages().is_empty() {
                    fallback = item.span_id.clone();
                }
            }
            scanned += 1;
            if scanned >= Self::TURN_SCAN_MAX_PAGES {
                break;
            }
        }
        Ok(fallback)
    }

    /// `GET /v1/conversations/{id}/export` as a single Arrow page.
    ///
    /// Unlike [`Conversations::list_arrow`], this is one page for the whole
    /// conversation rather than the first of a cursor walk: the export route
    /// assembles the complete conversation server-side and streams it in one
    /// response. Requires the `arrow` feature.
    #[cfg(feature = "arrow")]
    pub async fn export_arrow(
        &self,
        conversation_id: &str,
        params: &ConversationExportParams,
    ) -> ApiResult<ArrowPage> {
        let res = self
            .http
            .get_raw(
                &Self::export_path(conversation_id),
                &params.to_wire(),
                Some(ARROW_STREAM_ACCEPT),
            )
            .await?;
        decode_arrow_response(res).await
    }
}

/// `runner.events.*` — `GET /v1/events` (append-only `otel_logs`).
#[derive(Clone)]
pub struct Events {
    http: Arc<HttpClient>,
}

impl Events {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/events` — cursor paginator (JSON).
    ///
    /// [`EventListParams::event_name`] is **required** (compile-enforced).
    /// [`EventListParams::extra_event_names`] adds families that share its
    /// row shape and visibility class; the server merges them into one page
    /// and refuses (`422`) a set that does not. Each record deserializes into the matching typed
    /// [`Event`] variant (envelope + nested typed payload). Rows whose
    /// `event_name` this SDK build doesn't recognise surface as
    /// [`Event::Unknown`] rather than failing the page. Per-family filters
    /// (e.g. observation `pattern_id` / `lens` / `include_superseded`,
    /// pattern `lens` / `status`) pass through
    /// [`EventListParams::filters`] verbatim.
    ///
    /// [`Event::Unknown`]: crate::api::schemas::Event::Unknown
    pub fn list(&self, params: &EventListParams) -> ApiResult<Paginator<Event>> {
        let wire = params.to_wire()?;
        Paginator::new(self.http.clone(), "/v1/events", &wire)
    }

    /// `GET /v1/events/{id}` — read one event by id, across every family.
    ///
    /// Unlike [`Events::list`], no `event_name` is supplied, so the family
    /// is not known in advance. A family this SDK build predates
    /// deserializes as [`Event::Unknown`] rather than erroring, so a new
    /// server-side family is not mistaken for a missing event. Returns a
    /// `404` [`IntrospectionAPIError`] when the id does not resolve within
    /// the caller's tenant scope.
    ///
    /// [`Event::Unknown`]: crate::api::schemas::Event::Unknown
    /// [`IntrospectionAPIError`]: crate::api::error::IntrospectionAPIError
    pub async fn get(&self, event_id: &str) -> ApiResult<Event> {
        #[derive(serde::Serialize)]
        struct Q {}
        let path = format!("/v1/events/{}", crate::api::encoding::encode(event_id));
        self.http.get_json(&path, &Q {}).await
    }

    /// `GET /v1/events` as an Arrow stream — one Arrow page. Because the
    /// response is always single-family, the envelope arrives as constant
    /// typed columns and the family payload as one typed Arrow `struct`
    /// column (no JSON-blob fallback). Requires the `arrow` feature.
    #[cfg(feature = "arrow")]
    pub async fn list_arrow(&self, params: &EventListParams) -> ApiResult<ArrowPage> {
        let wire = params.to_wire()?;
        let res = self
            .http
            .get_raw("/v1/events", &wire, Some(ARROW_STREAM_ACCEPT))
            .await?;
        decode_arrow_response(res).await
    }
}

/// `runner.metrics.*` — the bounded `POST /v1/metrics` analytics surface.
#[derive(Clone)]
pub struct Metrics {
    http: Arc<HttpClient>,
}

impl Metrics {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `POST /v1/metrics` — run one bounded aggregation query.
    ///
    /// Validates the ergonomic `lookback`/`start`/`end` window client-side
    /// (mapping to the wire `from_timestamp`/`to_timestamp`) before sending;
    /// the DP enforces the allow-listed views / measures / dimensions and the
    /// hard limits.
    pub async fn query(&self, query: &MetricsQuery) -> ApiResult<MetricsResponse> {
        let wire = query.to_wire()?;
        self.http.post_json("/v1/metrics", &wire).await
    }

    /// `POST /v1/metrics` with the request body sent unchanged and the
    /// response returned as the server sent it.
    ///
    /// For callers that forward a metrics document they did not build, such
    /// as a query read from a file: the Data Plane owns the metrics grammar,
    /// so a request or response shape newer than this SDK build passes
    /// through instead of failing to (de)serialize. The only local check is
    /// that `request` is a JSON object, which every metrics request is; any
    /// other value is an [`IntrospectionAPIError::InvalidConfig`] sent nowhere.
    pub async fn query_raw(&self, request: &serde_json::Value) -> ApiResult<serde_json::Value> {
        if !request.is_object() {
            return Err(IntrospectionAPIError::InvalidConfig(
                "a metrics request must be a JSON object".to_string(),
            ));
        }
        self.http.post_json("/v1/metrics", request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::TryStreamExt;

    fn line(span_id: &str) -> String {
        format!(r#"{{"trace_id":"t","span_id":"{span_id}","start_time":"2026-01-01T00:00:00Z"}}"#)
    }

    async fn spans(chunks: &[&str]) -> ApiResult<Vec<GenAiSpan>> {
        let chunks: Vec<_> = chunks
            .iter()
            .map(|c| Ok::<_, reqwest::Error>(Bytes::copy_from_slice(c.as_bytes())))
            .collect();
        parse_json_lines(futures::stream::iter(chunks), "/v1/x".to_string())
            .try_collect()
            .await
    }

    fn ids(spans: &[GenAiSpan]) -> Vec<&str> {
        spans.iter().filter_map(|s| s.span_id.as_deref()).collect()
    }

    #[tokio::test]
    async fn json_lines_join_a_line_split_across_chunks() {
        // The second line is cut twice, once inside a two-byte character.
        let body = format!("{}\n{}\n\n{}\n", line("a"), line("\u{e9}b"), line("c"));
        let first = body.find('\u{e9}').unwrap() + 1;
        let bytes = body.as_bytes();
        let chunks: Vec<_> = [
            &bytes[..first - 20],
            &bytes[first - 20..first],
            &bytes[first..],
        ]
        .into_iter()
        .map(|c| Ok::<_, reqwest::Error>(Bytes::copy_from_slice(c)))
        .collect();

        let read: Vec<GenAiSpan> =
            parse_json_lines(futures::stream::iter(chunks), "/v1/x".to_string())
                .try_collect()
                .await
                .unwrap();

        assert_eq!(ids(&read), ["a", "\u{e9}b", "c"]);
    }

    #[tokio::test]
    async fn json_lines_read_a_last_line_without_its_newline() {
        let read = spans(&[&format!("{}\r\n{}", line("a"), line("b"))])
            .await
            .unwrap();

        assert_eq!(ids(&read), ["a", "b"]);
    }

    #[tokio::test]
    async fn json_lines_fail_on_a_body_cut_off_mid_line() {
        let error = spans(&[&format!("{}\n{{\"span_id\":", line("a"))])
            .await
            .unwrap_err();

        assert!(matches!(error, IntrospectionAPIError::Decode(_)), "{error}");
    }
}
