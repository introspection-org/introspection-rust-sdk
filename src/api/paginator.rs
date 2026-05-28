//! `Paginator<T>` — single-method pagination for DP list endpoints that
//! return `PaginatedResponse[T]`.
//!
//! Use as a [`Stream`] (auto-paginates item by item) or call
//! [`Paginator::next_page`] for page-at-a-time access (when you need the
//! `total_count` envelope or want to checkpoint cursors yourself).

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Stream;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::schemas::Paginated;

type PageFut<T> = Pin<Box<dyn Future<Output = ApiResult<Paginated<T>>> + Send>>;

/// Async paginator over a DP list endpoint.
///
/// # As a [`Stream`]
///
/// Auto-paginates page by page and yields one record at a time. This is
/// the typical usage:
///
/// ```rust,no_run
/// use futures::StreamExt;
/// use introspection_sdk::{IntrospectionClient, ClientConfig, RunRequest, TaskListParams};
/// use uuid::Uuid;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let client = IntrospectionClient::new(ClientConfig::default())?;
/// let runtime_id: Uuid = std::env::var("INTROSPECTION_RUNTIME_ID")?.parse()?;
/// let runner = client.runtime(runtime_id).run(RunRequest::default()).await?;
/// let tasks = runner.tasks();
/// let mut paginator = tasks.list(&TaskListParams::default());
/// while let Some(task) = paginator.next().await {
///     println!("{}", task?.id);
/// }
/// # Ok(()) }
/// ```
///
/// # Page at a time
///
/// Call [`Paginator::next_page`] to fetch a single page envelope (records
/// + `count` + `total_count` + opaque `next` cursor):
///
/// ```rust,no_run
/// use introspection_sdk::{IntrospectionClient, ClientConfig, RunRequest, TaskListParams};
/// use uuid::Uuid;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let client = IntrospectionClient::new(ClientConfig::default())?;
/// let runtime_id: Uuid = std::env::var("INTROSPECTION_RUNTIME_ID")?.parse()?;
/// let runner = client.runtime(runtime_id).run(RunRequest::default()).await?;
/// let tasks = runner.tasks();
/// let mut paginator = tasks.list(&TaskListParams::default());
/// while let Some(page) = paginator.next_page().await? {
///     println!("count={} total={:?}", page.count, page.total_count);
///     for task in page.records {
///         println!("  {}", task.id);
///     }
/// }
/// # Ok(()) }
/// ```
///
/// Mixing the two modes on the same instance is well-defined (records
/// pulled by `next_page` are not re-yielded by the `Stream` and vice-versa)
/// but generally surprising — pick one.
pub struct Paginator<T> {
    http: Arc<HttpClient>,
    path: String,
    base_params: serde_json::Value,
    next_cursor: Option<String>,
    started: bool,
    exhausted: bool,
    buffer: VecDeque<T>,
    pending: Option<PageFut<T>>,
}

impl<T> Paginator<T>
where
    T: DeserializeOwned + Send + 'static,
{
    pub(crate) fn new<P: Serialize>(
        http: Arc<HttpClient>,
        path: impl Into<String>,
        params: &P,
    ) -> ApiResult<Self> {
        let value = serde_json::to_value(params).map_err(|e| {
            IntrospectionAPIError::Decode(format!("failed to encode list params: {e}"))
        })?;
        Ok(Self {
            http,
            path: path.into(),
            base_params: value,
            next_cursor: None,
            started: false,
            exhausted: false,
            buffer: VecDeque::new(),
            pending: None,
        })
    }

    /// Fetch the next page synchronously (as far as `next_page` itself is
    /// concerned — this still awaits an HTTP round-trip). Returns
    /// `Ok(None)` once the paginator has walked off the end.
    pub async fn next_page(&mut self) -> ApiResult<Option<Paginated<T>>> {
        if self.exhausted && self.started {
            return Ok(None);
        }
        self.started = true;
        let params = self.params_for_current_cursor();
        let page: Paginated<T> = self.http.get_json(&self.path, &params).await?;
        self.next_cursor = page.next.clone();
        if page.next.is_none() {
            self.exhausted = true;
        }
        Ok(Some(page))
    }

    fn params_for_current_cursor(&self) -> serde_json::Value {
        let mut params = self.base_params.clone();
        if let Some(ref c) = self.next_cursor {
            if let Some(obj) = params.as_object_mut() {
                obj.insert("next".to_string(), serde_json::Value::String(c.clone()));
            }
        }
        params
    }
}

impl<T> Stream for Paginator<T>
where
    T: DeserializeOwned + Send + Unpin + 'static,
{
    type Item = ApiResult<T>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(item) = this.buffer.pop_front() {
                return Poll::Ready(Some(Ok(item)));
            }
            if let Some(fut) = this.pending.as_mut() {
                match fut.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(page)) => {
                        this.pending = None;
                        this.next_cursor = page.next.clone();
                        if page.next.is_none() {
                            this.exhausted = true;
                        }
                        for r in page.records {
                            this.buffer.push_back(r);
                        }
                        continue;
                    }
                    Poll::Ready(Err(e)) => {
                        this.pending = None;
                        this.exhausted = true;
                        return Poll::Ready(Some(Err(e)));
                    }
                }
            }
            if this.exhausted && this.started {
                return Poll::Ready(None);
            }
            this.started = true;
            let http = this.http.clone();
            let path = this.path.clone();
            let params = this.params_for_current_cursor();
            let fut: PageFut<T> = Box::pin(async move {
                http.get_json::<serde_json::Value, Paginated<T>>(&path, &params)
                    .await
            });
            this.pending = Some(fut);
        }
    }
}
