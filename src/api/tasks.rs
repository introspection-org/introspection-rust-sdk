//! `client.tasks.*` — task lifecycle and cursor-style run streaming.

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::Stream;
use futures::StreamExt;

use crate::agui::Event;
use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::resumable::{stream_resumable, StreamOptions};
use crate::api::schemas::{
    Task, TaskCancelOptions, TaskCancelResponse, TaskCreate, TaskCreateResponse, TaskListParams,
    TaskMode, TaskRun, TaskRunCreate, TaskRunResponse, TaskRunResume, TaskUpdate,
};

/// Handle returned by [`Tasks::start`] and [`TaskRuns::create`].
///
/// Mirrors the Cursor SDK shape: [`Self::stream`] iterates typed AG-UI
/// [`Event`]s, [`Self::text`] collects the assistant's text into a string,
/// [`Self::cancel`] cancels the run.
#[derive(Clone)]
pub struct RunHandle {
    /// The task this run belongs to. `None` when constructed from
    /// `runs.create(...)` (which only returns the run).
    pub task: Option<Task>,
    /// The run itself.
    pub run: TaskRun,
    runs: TaskRuns,
}

impl RunHandle {
    pub(crate) fn new(task: Option<Task>, run: TaskRun, runs: TaskRuns) -> Self {
        Self { task, run, runs }
    }

    /// Stream typed AG-UI [`Event`]s from `/v1/tasks/{id}/runs/{rid}/stream`.
    ///
    /// The stream resumes **transparently** across a mid-turn disconnect
    /// (gateway idle-timeout, load-balancer recycle, network blip): see
    /// [`TaskRuns::stream`]. Use [`Self::stream_with`] to tune the recovery
    /// bounds or opt into reconnect events.
    pub async fn stream(&self) -> ApiResult<impl Stream<Item = ApiResult<Event>>> {
        self.runs
            .stream(&self.run.task_id.to_string(), &self.run.id)
            .await
    }

    /// [`Self::stream`] with explicit recovery bounds ([`StreamOptions`]).
    pub fn stream_with(&self, opts: StreamOptions) -> impl Stream<Item = ApiResult<Event>> {
        self.runs
            .stream_with(&self.run.task_id.to_string(), &self.run.id, opts)
    }

    /// Consume this handle and return a pinned stream of AG-UI [`Event`]s.
    ///
    /// Unlike [`Self::stream`] the returned stream is `Pin<Box<…>>` so
    /// callers can iterate without `tokio::pin!`.
    pub async fn into_stream(
        self,
    ) -> ApiResult<Pin<Box<dyn Stream<Item = ApiResult<Event>> + Send>>> {
        let s = self.stream().await?;
        Ok(Box::pin(s))
    }

    /// Cancel the run.
    pub async fn cancel(&self) -> ApiResult<TaskCancelResponse> {
        self.runs
            .cancel(&self.run.task_id.to_string(), &self.run.id)
            .await
    }

    /// Cancel the run with explicit abort or drain options.
    pub async fn cancel_with(&self, options: &TaskCancelOptions) -> ApiResult<TaskCancelResponse> {
        self.runs
            .cancel_with(&self.run.task_id.to_string(), &self.run.id, options)
            .await
    }

    /// Convenience: concatenate the assistant's streamed text — the `delta`
    /// of every [`Event::TextMessageContent`] — into a single string. Returns
    /// an error on the first transport / decode failure.
    pub async fn text(&self) -> ApiResult<String> {
        let mut out = String::new();
        let stream = self.stream().await?;
        tokio::pin!(stream);
        while let Some(ev) = stream.next().await {
            if let Event::TextMessageContent(e) = ev? {
                out.push_str(&e.delta);
            }
        }
        Ok(out)
    }
}

/// `client.tasks.runs.*`.
#[derive(Clone)]
pub struct TaskRuns {
    http: Arc<HttpClient>,
}

impl TaskRuns {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `POST /v1/tasks/{id}/runs` — create a new run on an existing task.
    pub async fn create(&self, task_id: &str, body: &TaskRunCreate) -> ApiResult<RunHandle> {
        let path = format!("/v1/tasks/{}/runs", urlencode(task_id));
        let res: TaskRunResponse = self.http.post_json(&path, body).await?;
        Ok(RunHandle::new(None, res.run, self.clone()))
    }

    /// Resume an AG-UI interrupt by posting its typed resume entries.
    pub async fn resume(&self, task_id: &str, body: &TaskRunResume) -> ApiResult<RunHandle> {
        let path = format!("/v1/tasks/{}/runs", urlencode(task_id));
        let res: TaskRunResponse = self.http.post_json(&path, body).await?;
        Ok(RunHandle::new(None, res.run, self.clone()))
    }

    /// `GET /v1/tasks/{id}/runs/{rid}`.
    pub async fn get(&self, task_id: &str, run_id: &str) -> ApiResult<TaskRun> {
        let path = format!(
            "/v1/tasks/{}/runs/{}",
            urlencode(task_id),
            urlencode(run_id)
        );
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/tasks/{id}/runs/{rid}/cancel`.
    pub async fn cancel(&self, task_id: &str, run_id: &str) -> ApiResult<TaskCancelResponse> {
        let path = format!(
            "/v1/tasks/{}/runs/{}/cancel",
            urlencode(task_id),
            urlencode(run_id)
        );
        // POST with no body, JSON response — use the same multipart-less JSON
        // surface by sending an empty `()` body.
        self.http.post_json(&path, &serde_json::json!({})).await
    }

    /// Cancel a run with explicit abort or drain options.
    pub async fn cancel_with(
        &self,
        task_id: &str,
        run_id: &str,
        options: &TaskCancelOptions,
    ) -> ApiResult<TaskCancelResponse> {
        let path = format!(
            "/v1/tasks/{}/runs/{}/cancel",
            urlencode(task_id),
            urlencode(run_id)
        );
        self.http.post_json(&path, options).await
    }

    /// `GET /v1/tasks/{id}/runs/{rid}/stream` — async iterable of typed
    /// AG-UI [`Event`]s.
    ///
    /// The stream resumes **transparently** across a mid-turn disconnect
    /// (gateway idle-timeout, load-balancer recycle, network blip): it
    /// re-attaches with the SSE-standard `Last-Event-ID` so the server replays
    /// the frames the client missed, yielding a single gap-free sequence
    /// (INT-252, see [`crate::api::resumable`]). It ends when the turn finishes
    /// and yields a terminal `Err` only once recovery is exhausted — no
    /// consumer-visible change from a plain stream. Use [`Self::stream_with`]
    /// to tune the recovery bounds or opt into reconnect events.
    pub async fn stream(
        &self,
        task_id: &str,
        run_id: &str,
    ) -> ApiResult<impl Stream<Item = ApiResult<Event>>> {
        Ok(self.stream_with(task_id, run_id, StreamOptions::default()))
    }

    /// [`Self::stream`] with explicit recovery bounds ([`StreamOptions`]).
    pub fn stream_with(
        &self,
        task_id: &str,
        run_id: &str,
        opts: StreamOptions,
    ) -> impl Stream<Item = ApiResult<Event>> {
        stream_resumable(self.http.clone(), task_id, run_id, opts)
    }
}

/// `client.tasks.*`.
#[derive(Clone)]
pub struct Tasks {
    http: Arc<HttpClient>,
    /// Nested `runs` namespace.
    pub runs: TaskRuns,
}

impl Tasks {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        let runs = TaskRuns::new(http.clone());
        Self { http, runs }
    }

    /// `GET /v1/tasks` — paginator over the list endpoint.
    ///
    /// The returned [`Paginator<Task>`] implements
    /// [`futures::Stream`]`<Item = `[`ApiResult`]`<`[`Task`]`>>` (auto-paginates) and
    /// also exposes [`Paginator::next_page`] for page-at-a-time access
    /// (use this when you need the `total_count` envelope).
    pub fn list(&self, params: &TaskListParams) -> Paginator<Task> {
        Paginator::new(self.http.clone(), "/v1/tasks", params)
            .expect("TaskListParams must serialize to a JSON object")
    }

    /// `POST /v1/tasks` — create a task (and its initial run).
    pub async fn create(&self, body: &TaskCreate) -> ApiResult<TaskCreateResponse> {
        self.http.post_json("/v1/tasks", body).await
    }

    /// `GET /v1/tasks/{id}`.
    pub async fn get(&self, task_id: &str) -> ApiResult<Task> {
        let path = format!("/v1/tasks/{}", urlencode(task_id));
        self.http.get_json(&path, &()).await
    }

    /// `PATCH /v1/tasks/{id}`.
    pub async fn update(&self, task_id: &str, body: &TaskUpdate) -> ApiResult<Task> {
        let path = format!("/v1/tasks/{}", urlencode(task_id));
        self.http.patch_json(&path, body).await
    }

    /// `DELETE /v1/tasks/{id}` — soft-delete a task.
    ///
    /// Requires the `tasks:delete` scope. Dashboard-minted API keys do
    /// **not** grant this scope by default (per the cloud PR #678 scope
    /// model), so calls will return
    /// [`crate::IntrospectionAPIError::Http`] with `status == 403` unless
    /// the caller holds a wildcard or explicitly elevated key.
    pub async fn delete(&self, task_id: &str) -> ApiResult<()> {
        let path = format!("/v1/tasks/{}", urlencode(task_id));
        self.http.delete_empty(&path).await
    }

    /// `POST /v1/tasks/{id}/archive`.
    pub async fn archive(&self, task_id: &str) -> ApiResult<()> {
        let path = format!("/v1/tasks/{}/archive", urlencode(task_id));
        self.http.post_empty(&path).await
    }

    /// `POST /v1/tasks/{id}/unarchive`.
    pub async fn unarchive(&self, task_id: &str) -> ApiResult<()> {
        let path = format!("/v1/tasks/{}/unarchive", urlencode(task_id));
        self.http.post_empty(&path).await
    }

    /// Cursor-style sugar: `POST /v1/tasks` with a prompt + return a
    /// [`RunHandle`] on the initial run.
    ///
    /// ```rust,no_run
    /// # use introspection_sdk::{ClientConfig, IntrospectionClient, RunRequest};
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = IntrospectionClient::new(ClientConfig::default())?;
    /// let runtime = std::env::var("INTROSPECTION_RUNTIME").unwrap_or_else(|_| "customer-agent".into());
    /// let runner = client.runtime(&runtime).await?.run(RunRequest::default()).await?;
    /// let run = runner.tasks().start_prompt("Summarize this repo").await?;
    /// let text = run.text().await?;
    /// println!("{text}");
    /// # Ok(()) }
    /// ```
    pub async fn start_prompt(&self, prompt: impl Into<String>) -> ApiResult<RunHandle> {
        self.start(&TaskCreate {
            prompt: Some(prompt.into()),
            mode: Some(TaskMode::Agent),
            ..Default::default()
        })
        .await
    }

    /// Cursor-style sugar with a full [`TaskCreate`] body.
    pub async fn start(&self, body: &TaskCreate) -> ApiResult<RunHandle> {
        let res = self.create(body).await?;
        Ok(RunHandle::new(Some(res.task), res.run, self.runs.clone()))
    }
}

/// RFC 3986 path-segment percent encoding. We avoid pulling in
/// `percent-encoding` for one call site and inline a minimal helper here.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_passes_safe_chars() {
        assert_eq!(urlencode("abc-123_.~"), "abc-123_.~");
    }

    #[test]
    fn urlencode_escapes_slash_and_space() {
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
    }
}
