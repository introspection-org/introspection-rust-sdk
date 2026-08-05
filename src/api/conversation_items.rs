//! `runner.conversations().items` — paginated conversation span reads.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Stream;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::genai_span::{GenAiSpan, GenAiSpanList};
use crate::api::http::HttpClient;
use crate::api::schemas::{ConversationItemGetParams, ConversationItemListParams};

type PageFuture = Pin<Box<dyn Future<Output = ApiResult<GenAiSpanList>> + Send>>;

/// `runner.conversations().items` read-only namespace.
#[derive(Clone)]
pub struct ConversationItems {
    http: Arc<HttpClient>,
}

impl ConversationItems {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/conversations/{conversation_id}/items`.
    ///
    /// The returned paginator can be used as a [`Stream`] of
    /// [`GenAiSpan`]s or page-by-page with
    /// [`ConversationItemPaginator::next_page`]. Pagination passes each
    /// response's opaque `next` token back unchanged.
    pub fn list(
        &self,
        conversation_id: &str,
        params: &ConversationItemListParams,
    ) -> ApiResult<ConversationItemPaginator> {
        if let Some(limit) = params.limit {
            if !(1..=1000).contains(&limit) {
                return Err(IntrospectionAPIError::InvalidConfig(
                    "conversation item limit must be between 1 and 1000".into(),
                ));
            }
        }
        if let Some(days) = params.lookback_days {
            if !(1..=365).contains(&days) {
                return Err(IntrospectionAPIError::InvalidConfig(
                    "conversation item lookback_days must be between 1 and 365".into(),
                ));
            }
        }
        let path = format!("/v1/conversations/{}/items", urlencode(conversation_id));
        Ok(ConversationItemPaginator::new(
            self.http.clone(),
            path,
            list_query(params),
            params.next.clone(),
        ))
    }

    /// `GET /v1/conversations/{conversation_id}/items/{item_id}`.
    ///
    /// The detail response carries the **full input history** for that span —
    /// unconditionally, with no `include` to remember — while list responses
    /// carry only the turn-local input delta. This is the read to fork or
    /// resume a conversation from.
    pub async fn get(
        &self,
        conversation_id: &str,
        item_id: &str,
        params: &ConversationItemGetParams,
    ) -> ApiResult<GenAiSpan> {
        let path = format!(
            "/v1/conversations/{}/items/{}",
            urlencode(conversation_id),
            urlencode(item_id)
        );
        self.http.get_json(&path, &get_query(params)).await
    }
}

/// Async paginator for the OpenAI-style [`GenAiSpanList`] envelope.
///
/// `first_id` and `last_id` remain available through [`Self::next_page`] but
/// are not used for pagination.
pub struct ConversationItemPaginator {
    http: Arc<HttpClient>,
    path: String,
    base_query: Vec<(String, String)>,
    next_cursor: Option<String>,
    started: bool,
    exhausted: bool,
    buffer: VecDeque<GenAiSpan>,
    pending: Option<PageFuture>,
}

impl ConversationItemPaginator {
    fn new(
        http: Arc<HttpClient>,
        path: String,
        base_query: Vec<(String, String)>,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            http,
            path,
            base_query,
            next_cursor,
            started: false,
            exhausted: false,
            buffer: VecDeque::new(),
            pending: None,
        }
    }

    /// Fetch one page, returning `None` after the page with `has_more=false`.
    pub async fn next_page(&mut self) -> ApiResult<Option<GenAiSpanList>> {
        if self.exhausted && self.started {
            return Ok(None);
        }
        self.started = true;
        let page: GenAiSpanList = self
            .http
            .get_json(&self.path, &self.query_for_current_cursor())
            .await?;
        if let Err(error) = self.advance(&page) {
            self.exhausted = true;
            return Err(error);
        }
        Ok(Some(page))
    }

    /// Collect items to exhaustion, bounded by `max_pages` (`0` is unbounded).
    pub async fn collect_all(&mut self, max_pages: usize) -> ApiResult<Vec<GenAiSpan>> {
        let mut out = Vec::new();
        let mut pages = 0usize;
        while let Some(page) = self.next_page().await? {
            out.extend(page.data);
            pages += 1;
            if max_pages != 0 && pages >= max_pages {
                break;
            }
        }
        Ok(out)
    }

    fn advance(&mut self, page: &GenAiSpanList) -> ApiResult<()> {
        if !page.has_more {
            self.next_cursor = None;
            self.exhausted = true;
            return Ok(());
        }
        let next = page.next.clone().ok_or_else(|| {
            IntrospectionAPIError::Decode(
                "conversation items response has_more without next".into(),
            )
        })?;
        if self.next_cursor.as_deref() == Some(next.as_str()) {
            return Err(IntrospectionAPIError::Decode(
                "conversation items response repeated its next cursor".into(),
            ));
        }
        self.next_cursor = Some(next);
        Ok(())
    }

    fn query_for_current_cursor(&self) -> Vec<(String, String)> {
        let mut query = self.base_query.clone();
        query.retain(|(key, _)| key != "next");
        if let Some(next) = &self.next_cursor {
            query.push(("next".into(), next.clone()));
        }
        query
    }
}

impl Stream for ConversationItemPaginator {
    type Item = ApiResult<GenAiSpan>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(item) = this.buffer.pop_front() {
                return Poll::Ready(Some(Ok(item)));
            }
            if let Some(future) = this.pending.as_mut() {
                match future.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(page)) => {
                        this.pending = None;
                        if let Err(error) = this.advance(&page) {
                            this.exhausted = true;
                            return Poll::Ready(Some(Err(error)));
                        }
                        this.buffer.extend(page.data);
                        continue;
                    }
                    Poll::Ready(Err(error)) => {
                        this.pending = None;
                        this.exhausted = true;
                        return Poll::Ready(Some(Err(error)));
                    }
                }
            }
            if this.exhausted && this.started {
                return Poll::Ready(None);
            }
            this.started = true;
            let http = this.http.clone();
            let path = this.path.clone();
            let query = this.query_for_current_cursor();
            this.pending = Some(Box::pin(async move { http.get_json(&path, &query).await }));
        }
    }
}

fn list_query(params: &ConversationItemListParams) -> Vec<(String, String)> {
    let mut query = Vec::new();
    push_opt(&mut query, "limit", params.limit);
    push_opt_ref(&mut query, "next", params.next.as_deref());
    push_opt_ref(&mut query, "start_date", params.start_date.as_deref());
    push_opt_ref(&mut query, "end_date", params.end_date.as_deref());
    for include in &params.include {
        query.push(("include".into(), include.as_str().into()));
    }
    push_opt_ref(&mut query, "agent_name", params.agent_name.as_deref());
    push_opt_ref(&mut query, "agent_id", params.agent_id.as_deref());
    push_opt_ref(&mut query, "service_name", params.service_name.as_deref());
    push_opt_ref(
        &mut query,
        "operation_name",
        params.operation_name.as_deref(),
    );
    push_opt(&mut query, "lookback_days", params.lookback_days);
    if let Some(share_id) = params.share_id {
        query.push(("share_id".into(), share_id.to_string()));
    }
    query
}

fn get_query(params: &ConversationItemGetParams) -> Vec<(String, String)> {
    let mut query = Vec::new();
    for include in &params.include {
        query.push(("include".into(), include.as_str().into()));
    }
    if let Some(share_id) = params.share_id {
        query.push(("share_id".into(), share_id.to_string()));
    }
    query
}

fn push_opt<T: ToString>(query: &mut Vec<(String, String)>, key: &str, value: Option<T>) {
    if let Some(value) = value {
        query.push((key.into(), value.to_string()));
    }
}

fn push_opt_ref(query: &mut Vec<(String, String)>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        query.push((key.into(), value.into()));
    }
}

fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
