//! # Introspection SDK for Rust
//!
//! Rust client for [Introspection](https://introspection.dev). Three
//! independent surfaces, mix-and-match as needed:
//!
//! 1. [`IntrospectionClient`] — REST surface (`runtimes`, `experiments`,
//!    `Runner`, `tasks`, `files`). Always available, no OpenTelemetry
//!    dependency. No feature flag required.
//! 2. `otel::IntrospectionLogs` — OTLP **logs** exporter for
//!    `track` / `feedback` / `identify` analytics events. Owns its own
//!    `SdkLoggerProvider`. Requires the `otel` Cargo feature.
//! 3. `otel::IntrospectionSpanProcessor` — OTLP **trace** exporter
//!    you attach to your own `SdkTracerProvider`. Requires the `otel`
//!    feature.
//!
//! The three surfaces share no state — construct only what you need.
//!
//! ## REST quick start
//!
//! ```rust,no_run
//! use introspection_sdk::{ClientConfig, IntrospectionClient};
//!
//! # async fn main_() -> Result<(), Box<dyn std::error::Error>> {
//! let client = IntrospectionClient::new(ClientConfig::with_token("your-token"))?;
//! let runtimes = client.runtimes();
//! // runtimes.create(...).await?;
//! # Ok(()) }
//! ```
//!
//! ## Analytics (logs) quick start
//!
//! Requires the `otel` feature.
//!
//! ```rust,no_run
//! # #[cfg(feature = "otel")] {
//! use introspection_sdk::otel::{FeedbackOptions, IntrospectionLogs, TrackOptions};
//!
//! let logs = IntrospectionLogs::builder()
//!     .token("your-token")
//!     .service_name("my-service")
//!     .build()
//!     .unwrap();
//!
//! logs.track(
//!     "Button Clicked",
//!     Some(TrackOptions::new().with_property("button_id", "submit")),
//! );
//!
//! {
//!     let _user = logs.set_user_id("user_123");
//!     let _conv = logs.set_conversation_id("conv_456");
//!     logs.feedback(
//!         "thumbs_up",
//!         FeedbackOptions::new().with_comments("Great response!"),
//!     );
//! } // Context cleared when guards drop
//!
//! logs.shutdown().unwrap();
//! # }
//! ```
//!
//! ## Traces quick start
//!
//! Requires the `otel` feature.
//!
//! ```rust,no_run
//! # #[cfg(feature = "otel")] {
//! use introspection_sdk::otel::{IntrospectionSpanProcessor, SpanProcessorConfig};
//! use opentelemetry_sdk::trace::SdkTracerProvider;
//!
//! let processor = IntrospectionSpanProcessor::new(
//!     SpanProcessorConfig::with_token("your-token"),
//! ).unwrap();
//!
//! let provider = SdkTracerProvider::builder()
//!     .with_span_processor(processor)
//!     .build();
//! # let _ = provider;
//! # }
//! ```
//!
//! ## Environment variables
//!
//! | Variable                        | Purpose                                     |
//! |---------------------------------|---------------------------------------------|
//! | `INTROSPECTION_TOKEN`           | Auth token (all surfaces)                   |
//! | `INTROSPECTION_PROJECT_ID`      | Default project UUID (REST client)          |
//! | `INTROSPECTION_SERVICE_NAME`    | Service name (logs/traces)                  |
//! | `INTROSPECTION_BASE_API_URL`    | REST API host (default `api.introspection.dev`) |
//! | `INTROSPECTION_BASE_OTEL_URL`   | OTLP collector host (default `otel.introspection.dev`) |

pub mod api;
pub mod client;
#[cfg(feature = "otel")]
pub mod otel;
pub mod resources;
pub mod runner;
pub mod types;

// Re-export wire types + low-level REST API surface (always available)
pub use api::{
    Arm, Experiment, ExperimentCreate, ExperimentListParams, ExperimentStatus, ExperimentUpdate,
    File, FileCreateText, FileListParams, FileType, FileUpdate, FileUpload, FileVersions, Files,
    IntrospectionAPIError, Paginated, PaginationParams, Paginator, Project, ProjectListParams,
    Recipe, RecipeCreate, RecipeListParams, RecipeUpdate, Repository, RepositoryListParams,
    RunCaller, RunCallerLibrary, RunCallerPage, RunHandle, RunRequest, RunnerContext,
    RunnerDeployment, RunnerIdentity, RunnerSpec, Runtime, RuntimeCreate, RuntimeListParams,
    RuntimeUpdate, SseEvent, Task, TaskCancelResponse, TaskCreate, TaskCreateResponse,
    TaskListParams, TaskMode, TaskPrompt, TaskRun, TaskRunCreate, TaskRunResponse, TaskRuns,
    TaskStatus, TaskUpdate, Tasks, UploadSource,
};
pub use client::{IntrospectionClient, IntrospectionError, Result, VERSION};
pub use resources::{
    ExperimentHandle, Experiments, Projects, RecipePin, Recipes, Repositories, RuntimeHandle,
    Runtimes,
};
pub use runner::{Runner, RunnerSource};
pub use types::{AdvancedOptions, ClientConfig, ClientConfigBuilder};

// OTel surfaces — gated behind the `otel` feature, re-exported from
// `crate::otel` for top-level access.
#[cfg(feature = "otel")]
pub use otel::{
    BaggageGuard, FeedbackOptions, GenerationUpdate, IdentifyOptions, IntrospectionLogs,
    IntrospectionLogsConfig, IntrospectionLogsConfigBuilder, IntrospectionLogsError,
    IntrospectionSpanProcessor, Observation, ObservationConfig, ObservationType, PropertyValue,
    SpanProcessorAdvancedOptions, SpanProcessorConfig, SpanProcessorConfigBuilder,
    SpanProcessorError, SpanProcessorResult, TrackOptions, Usage,
};

#[cfg(feature = "otel")]
pub use otel::messages::{
    ContentPart, InputMessage, OutputMessage, TextPart, ThinkingPart, ToolCallRequestPart,
    ToolCallResponsePart,
};

#[cfg(feature = "openai")]
pub use otel::openai::TracedStream;
