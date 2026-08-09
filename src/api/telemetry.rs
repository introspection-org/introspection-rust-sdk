//! `runner.conversations` / `runner.events` / `runner.metrics` — Data-Plane
//! telemetry reads.
//!
//! These are **Data-Plane-scoped**: they hang off the [`crate::Runner`] (DP
//! bearer + `events:read`), never the CP-scoped top-level
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
//! [`IntrospectionAPIError::InvalidConfig`]) *before* any request is sent. See
//! [`crate::api::schemas`].
//!
//! # Optional Arrow
//!
//! With the `arrow` Cargo feature, `list_arrow` requests the
//! `application/vnd.apache.arrow.stream` response and decodes the Arrow IPC
//! stream, reading pagination metadata from response headers into an
//! [`ArrowPage`](crate::api::arrow::ArrowPage). The DP answers `406` when the
//! Arrow format is unsupported.

use std::sync::Arc;

use bytes::Bytes;
use futures::{Stream, StreamExt};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

use crate::api::conversation_items::ConversationItems;
use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::genai_span::{GenAiSpan, GenAiSpanList};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Conversation, ConversationExportParams, ConversationItemGetParams, ConversationItemListParams,
    ConversationListParams, Event, EventListParams, MetricsQuery, MetricsResponse, Trajectory,
};

/// Base media type of a trajectory-v1 conversation export. The `version`
/// parameter is appended by the caller; a server that does not implement the
/// requested version answers `406` rather than silently serving another shape.
pub const TRAJECTORY_MEDIA_TYPE: &str = "application/vnd.letta.trajectory+json";

/// Accept header selecting the pinned trajectory-v1 export representation.
pub const TRAJECTORY_V1_ACCEPT: &str = "application/vnd.letta.trajectory+json;version=1";

/// Wire representation selected for a complete conversation export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationExportFormat {
    Json,
    Trajectory,
    Arrow,
}

impl ConversationExportFormat {
    fn accept(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Trajectory => TRAJECTORY_V1_ACCEPT,
            Self::Arrow => "application/vnd.apache.arrow.stream",
        }
    }
}

#[cfg(feature = "arrow")]
use crate::api::arrow::{decode_arrow_response, ArrowPage, ARROW_STREAM_ACCEPT};

/// `runner.conversations.*` — `GET /v1/conversations` (append-only
/// `otel_traces`).
#[derive(Clone)]
pub struct Conversations {
    http: Arc<HttpClient>,
    /// Items of a conversation (`/v1/conversations/{id}/items`).
    pub items: ConversationItems,
}

impl Conversations {
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
                    utf8_percent_encode(conversation_id, PATH_SEGMENT_ENCODE_SET)
                ),
                &(),
            )
            .await
    }

    fn export_path(conversation_id: &str) -> String {
        format!(
            "/v1/conversations/{}/export",
            utf8_percent_encode(conversation_id, PATH_SEGMENT_ENCODE_SET)
        )
    }

    /// `GET /v1/conversations/{id}/export` — one complete conversation as
    /// the standard GenAI-span list.
    pub async fn export_json(
        &self,
        conversation_id: &str,
        params: &ConversationExportParams,
    ) -> ApiResult<GenAiSpanList> {
        let response = self
            .http
            .get_raw(
                &Self::export_path(conversation_id),
                params,
                Some("application/json"),
            )
            .await?;
        crate::api::http::decode_json(response).await
    }

    /// Stream a complete export's response bytes without buffering them in
    /// the SDK. The server walks storage in bounded pages; this stream passes
    /// each response chunk through as it arrives.
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
                params,
                Some(format.accept()),
            )
            .await?;
        Ok(response
            .bytes_stream()
            .map(|chunk| chunk.map_err(IntrospectionAPIError::from)))
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
                params,
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
    async fn find_latest_turn_id(&self, conversation_id: &str) -> ApiResult<Option<String>> {
        let mut pages = self
            .items
            .list(conversation_id, &ConversationItemListParams::default())?;
        let mut fallback: Option<String> = None;
        while let Some(page) = pages.next_page().await? {
            for item in page.data {
                if item.operation_name() == Some("chat") {
                    return Ok(item.span_id);
                }
                if fallback.is_none() && !item.output_messages().is_empty() {
                    fallback = item.span_id.clone();
                }
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
                params,
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
    /// [`EventListParams::event_name`] is **required** (compile-enforced) —
    /// exactly one of the six canonical families per request, so every page
    /// is homogeneous and each record deserializes into the matching typed
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
        let path = format!(
            "/v1/events/{}",
            utf8_percent_encode(event_id, PATH_SEGMENT_ENCODE_SET)
        );
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
}
