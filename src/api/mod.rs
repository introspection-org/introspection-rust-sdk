//! REST API surface for the Introspection Data Plane (`/v1/tasks`,
//! `/v1/files`, `/v1/shares`).
//!
//! Runner-bound resource namespaces mirror the corresponding JS / Python SDKs:
//!
//! - [`Tasks`] — task lifecycle (list / create / update / archive / delete)
//!   with nested [`TaskRuns`] and a cursor-style [`Tasks::start_prompt`]
//!   sugar that returns a [`RunHandle`].
//! - [`Files`] — OpenAI-style upload / download / list, plus a nested
//!   [`FileVersions`] namespace.
//! - [`Shares`] — read-sharing grants for files and conversations.
//!
//! Everything maps 1:1 to existing DP routes; no new HTTP surface area.
//! Auth reuses the same `INTROSPECTION_TOKEN` bearer used by the OTLP
//! exporter — the SDK is shape-agnostic about API key (`intro_…`) vs
//! short-lived DP JWT.
//!
//! # Configuration
//!
//! Two independent base URLs, with their own env vars and defaults:
//!
//! | Surface | Env var | Default | Config field |
//! | --- | --- | --- | --- |
//! | OTLP collector | `INTROSPECTION_BASE_OTEL_URL` | `https://otel.introspection.dev` | `IntrospectionLogs::builder().base_otel_url(...)` / `SpanProcessorAdvancedOptions::base_otel_url` |
//! | DP REST API | `INTROSPECTION_BASE_API_URL` | `https://api.introspection.dev` | [`AdvancedOptions::base_api_url`] |
//!
//! [`AdvancedOptions::base_api_url`]: crate::AdvancedOptions::base_api_url
//!
//! # Quick start — cursor-style tasks
//!
//! ```rust,no_run
//! use futures::StreamExt;
//! use introspection_sdk::{ClientConfig, IntrospectionClient, RunRequest};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let client = IntrospectionClient::new(ClientConfig::default())?;
//! let runtime = std::env::var("INTROSPECTION_RUNTIME").unwrap_or_else(|_| "customer-agent".into());
//!
//! // Open a runner against the runtime; spawn a task and stream its run.
//! let runner = client.runtime(&runtime).await?.run(RunRequest::default()).await?;
//! let run = runner.tasks().start_prompt("Summarize this repo").await?;
//! let stream = run.stream().await?;
//! tokio::pin!(stream);
//! while let Some(event) = stream.next().await {
//!     // Typed AG-UI events — branch on the variant.
//!     if let introspection_sdk::AgUiEvent::TextMessageContent(e) = event? {
//!         print!("{}", e.delta);
//!     }
//! }
//!
//! // Or collect text frames into a single string:
//! let run = runner.tasks().start_prompt("Say hi").await?;
//! let text = run.text().await?;
//! println!("{text}");
//! # Ok(()) }
//! ```
//!
//! # Quick start — files
//!
//! ```rust,no_run
//! use introspection_sdk::{
//!     ClientConfig, FileCreateText, FileUpload, FileType, IntrospectionClient, RunRequest,
//! };
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let client = IntrospectionClient::new(ClientConfig::default())?;
//! let runtime = std::env::var("INTROSPECTION_RUNTIME").unwrap_or_else(|_| "customer-agent".into());
//! let runner = client.runtime(&runtime).await?.run(RunRequest::default()).await?;
//!
//! // Multipart upload from a local path.
//! let file = runner.files().upload(
//!     FileUpload::from_path("input.jsonl").with_file_type(FileType::Upload),
//! ).await?;
//!
//! // JSON text/markdown upload by content.
//! let note = runner.files().create_text(&FileCreateText {
//!     name: "notes.md".into(),
//!     content: "# Hello".into(),
//!     mime_type: Some("text/markdown".into()),
//! }).await?;
//!
//! // Download into memory.
//! let bytes = runner.files().download(&file.id.to_string()).await?;
//! # let _ = (note, bytes); Ok(()) }
//! ```
//!
//! # Route map
//!
//! | DP route | Rust call |
//! | --- | --- |
//! | `GET    /v1/tasks` | [`Tasks::list`] *(paginator: stream or `next_page`)* |
//! | `POST   /v1/tasks` | [`Tasks::create`] / [`Tasks::start`] / [`Tasks::start_prompt`] |
//! | `GET    /v1/tasks/{id}` | [`Tasks::get`] |
//! | `PATCH  /v1/tasks/{id}` | [`Tasks::update`] |
//! | `DELETE /v1/tasks/{id}` | [`Tasks::delete`] *(403 on dashboard keys)* |
//! | `POST   /v1/tasks/{id}/archive` | [`Tasks::archive`] |
//! | `POST   /v1/tasks/{id}/unarchive` | [`Tasks::unarchive`] |
//! | `POST   /v1/tasks/{id}/runs` | [`TaskRuns::create`] / [`TaskRuns::resume`] |
//! | `GET    /v1/tasks/{id}/runs/{rid}` | [`TaskRuns::get`] |
//! | `POST   /v1/tasks/{id}/runs/{rid}/cancel` | [`TaskRuns::cancel`] / [`TaskRuns::abort`] / [`TaskRuns::drain`] |
//! | `GET    /v1/tasks/{id}/runs/{rid}/stream` | [`TaskRuns::stream`] |
//! | `GET    /v1/files` | [`Files::list`] *(paginator: stream or `next_page`)* |
//! | `POST   /v1/files` (multipart) | [`Files::upload`] |
//! | `POST   /v1/files` (json) | [`Files::create_text`] |
//! | `GET    /v1/files/{id}` | [`Files::get`] |
//! | `PATCH  /v1/files/{id}` | [`Files::update`] |
//! | `DELETE /v1/files/{id}` | [`Files::delete`] |
//! | `GET    /v1/files/{id}/content` | [`Files::download`] / [`Files::download_stream`] |
//! | `GET    /v1/files/{id}/versions` *(paginated)* | [`FileVersions::list`] |
//! | `POST   /v1/files/{id}/versions` | [`FileVersions::create`] |
//! | `GET    /v1/files/{id}/versions/{vid}` | [`FileVersions::get`] |
//! | `GET    /v1/shares` | [`Shares::list`] |
//! | `POST   /v1/shares` | [`Shares::create`] |
//! | `GET    /v1/shares/{id}` | [`Shares::get`] |
//! | `DELETE /v1/shares/{id}` | [`Shares::delete`] |
//!
//! # Streaming
//!
//! [`TaskRuns::stream`] returns `impl Stream<Item = `[`ApiResult`]`<`[`Event`]`>>`,
//! where [`Event`] is the typed AG-UI protocol event (see [`crate::agui`]).
//! The stream yields only protocol events — transport frames (`heartbeat`,
//! `done`, `result`) are handled internally — so callers branch on the
//! [`Event`] variant directly. An unrecognised future event `type` surfaces
//! as [`Event::Unknown`] rather than failing the stream.
//! [`RunHandle::text`] is a convenience that concatenates the `delta` of
//! [`Event::TextMessageContent`] events into a single string.
//!
//! The stream also resumes **transparently** across a mid-turn disconnect
//! (gateway idle-timeout, load-balancer recycle, network blip): it re-attaches
//! with the SSE-standard `Last-Event-ID` so the server replays the frames the
//! client missed, yielding one gap-free [`Event`] sequence (INT-252, see
//! [`resumable`]). Readiness folds in the same way — a not-yet-attachable run
//! answers with `429` + `Retry-After`, honoured as a backoff floor and retried,
//! never surfaced. Use [`TaskRuns::stream_with`] to tune the recovery bounds or
//! opt into an `introspection.reconnect` `CUSTOM` event on each reconnect.
//!
//! The raw frame layer ([`parse_sse_response`] /
//! [`crate::SseEvent`]) remains available for advanced callers who want the
//! untyped `event` / `data` / `id` wire shape.
//!
//! [`Event`]: crate::agui::Event
//! [`Event::Unknown`]: crate::agui::Event::Unknown
//! [`Event::TextMessageContent`]: crate::agui::Event::TextMessageContent
//! [`TaskRuns::stream_with`]: tasks::TaskRuns::stream_with
//!
//! # Errors
//!
//! Every method on `tasks` / `files` / `shares` returns [`ApiResult<T>`], i.e.
//! `Result<T, `[`IntrospectionAPIError`]`>`. The error enum has variants
//! for HTTP failures (status / code / request id / parsed body), transport
//! failures, decode failures, invalid configuration, and local I/O during
//! uploads. The existing OTLP `track` / `feedback` / `identify` paths keep
//! returning [`crate::IntrospectionError`].
//!
//! [`RunHandle::text`]: tasks::RunHandle::text

#[cfg(feature = "arrow")]
pub mod arrow;
pub mod backoff;
pub mod error;
pub mod files;
pub mod http;
pub mod paginator;
pub mod resumable;
pub mod schemas;
pub mod shares;
pub mod sse;
pub mod tasks;
pub mod telemetry;

#[cfg(feature = "arrow")]
pub use arrow::{ArrowPage, ARROW_STREAM_ACCEPT};
pub use error::{ApiResult, IntrospectionAPIError};
pub use files::{FileUpload, FileVersions, Files, UploadSource};
pub use http::{HttpClient, HttpConfig};
pub use paginator::Paginator;
pub use resumable::{stream_resumable, StreamOptions};
pub use schemas::{
    AgentInfo, Arm, ClusteringRunEvent, ClusteringRunPayload, Conversation, ConversationListParams,
    Dimension, Event, EventListParams, Experiment, ExperimentCreate, ExperimentListParams,
    ExperimentStatus, ExperimentUpdate, FeedbackEvent, FeedbackPayload, File, FileCreateText,
    FileListParams, FileType, FileUpdate, HavingTerm, IntrospectionEventName, JudgementEvent,
    JudgementPayload, MetricFilter, MetricSpec, MetricsConfig, MetricsQuery, MetricsResponse,
    ObservationEvent, ObservationPayload, OrderTerm, Paginated, PaginationParams,
    PatternAssignmentEvent, PatternAssignmentPayload, PatternEvent, PatternPayload, Project,
    ProjectListParams, Recipe, RecipeCreate, RecipeListParams, RecipeUpdate, Repository,
    RepositoryListParams, ResourceShare, ResumeEntry, RunCaller, RunCallerLibrary, RunCallerPage,
    RunRequest, RunnerContext, RunnerDeployment, RunnerIdentity, RunnerSpec, Runtime,
    RuntimeCreate, RuntimeListParams, RuntimeUpdate, ShareCreate, ShareListParams,
    ShareResourceType, SortDirection, SseEvent, StringOrUuid, Task, TaskCancelOptions,
    TaskCancelResponse, TaskCreate, TaskCreateResponse, TaskListParams, TaskMode, TaskPrompt,
    TaskRun, TaskRunCreate, TaskRunKind, TaskRunResponse, TaskRunResume, TaskStatus, TaskUpdate,
    TimeDimension, TypedEvent,
};
pub use shares::Shares;
pub use sse::{parse_agui_response, parse_sse_response};
pub use tasks::{RunHandle, TaskRuns, Tasks};
pub use telemetry::{Conversations, Events, Metrics};
