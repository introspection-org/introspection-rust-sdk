//! REST API surface for the Introspection Data Plane (`/v1/tasks`,
//! `/v1/files`).
//!
//! Two parallel namespaces are wired onto [`crate::IntrospectionClient`],
//! both mirroring the corresponding JS / Python SDKs:
//!
//! - [`Tasks`] — task lifecycle (list / create / update / archive / delete)
//!   with nested [`TaskRuns`] and a cursor-style [`Tasks::start_prompt`]
//!   sugar that returns a [`RunHandle`].
//! - [`Files`] — OpenAI-style upload / download / list, plus a nested
//!   [`FileVersions`] namespace.
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
//! let runner = client.runtime_ref(&runtime).await?.run(RunRequest::default()).await?;
//! let run = runner.tasks().start_prompt("Summarize this repo").await?;
//! let stream = run.stream().await?;
//! tokio::pin!(stream);
//! while let Some(event) = stream.next().await {
//!     let event = event?;
//!     println!("[{}] {}", event.event, event.data);
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
//! let runner = client.runtime_ref(&runtime).await?.run(RunRequest::default()).await?;
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
//! | `POST   /v1/tasks/{id}/runs` | [`TaskRuns::create`] |
//! | `GET    /v1/tasks/{id}/runs/{rid}` | [`TaskRuns::get`] |
//! | `POST   /v1/tasks/{id}/runs/{rid}/cancel` | [`TaskRuns::cancel`] |
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
//!
//! # Streaming
//!
//! [`TaskRuns::stream`] returns `impl Stream<Item = `[`ApiResult`]`<`[`SseEvent`]`>>`.
//! The DP proxies frames verbatim from the agents-worker — the SDK does
//! **not** define the event taxonomy. Branch on `event` and parse `data`
//! yourself (typically `serde_json::from_str(&ev.data)`).
//! [`RunHandle::text`] is a convenience that concatenates `data` from
//! `event: text` and `event: message` frames.
//!
//! # Errors
//!
//! Every method on `tasks` / `files` returns [`ApiResult<T>`], i.e.
//! `Result<T, `[`IntrospectionAPIError`]`>`. The error enum has variants
//! for HTTP failures (status / code / request id / parsed body), transport
//! failures, decode failures, invalid configuration, and local I/O during
//! uploads. The existing OTLP `track` / `feedback` / `identify` paths keep
//! returning [`crate::IntrospectionError`].
//!
//! [`RunHandle::text`]: tasks::RunHandle::text

pub mod error;
pub mod files;
pub mod http;
pub mod paginator;
pub mod schemas;
pub mod sse;
pub mod tasks;

pub use error::{ApiResult, IntrospectionAPIError};
pub use files::{FileUpload, FileVersions, Files, UploadSource};
pub use http::{HttpClient, HttpConfig};
pub use paginator::Paginator;
pub use schemas::{
    AgentInfo, Arm, Experiment, ExperimentCreate, ExperimentListParams, ExperimentStatus,
    ExperimentUpdate, File, FileCreateText, FileListParams, FileType, FileUpdate, Paginated,
    PaginationParams, Project, ProjectListParams, Recipe, RecipeCreate, RecipeListParams,
    RecipeUpdate, Repository, RepositoryListParams, RunCaller, RunCallerLibrary, RunCallerPage,
    RunRequest, RunnerContext, RunnerDeployment, RunnerIdentity, RunnerSpec, Runtime,
    RuntimeCreate, RuntimeListParams, RuntimeUpdate, SseEvent, StringOrUuid, Task,
    TaskCancelResponse, TaskCreate, TaskCreateResponse, TaskListParams, TaskMode, TaskPrompt,
    TaskRun, TaskRunCreate, TaskRunResponse, TaskStatus, TaskUpdate,
};
pub use sse::parse_sse_response;
pub use tasks::{RunHandle, TaskRuns, Tasks};
