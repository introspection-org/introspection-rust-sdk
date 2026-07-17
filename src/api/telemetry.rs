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

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Conversation, ConversationListParams, Event, EventListParams, MetricsQuery, MetricsResponse,
};

#[cfg(feature = "arrow")]
use crate::api::arrow::{decode_arrow_response, ArrowPage, ARROW_STREAM_ACCEPT};

/// `runner.conversations.*` — `GET /v1/conversations` (append-only
/// `otel_traces`).
#[derive(Clone)]
pub struct Conversations {
    http: Arc<HttpClient>,
}

impl Conversations {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
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
