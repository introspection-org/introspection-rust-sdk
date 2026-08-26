//! # Introspection SDK for Rust
//!
//! Rust client for [Introspection](https://introspection.dev). Three
//! independent surfaces, mix-and-match as needed:
//!
//! 1. [`IntrospectionClient`] — REST surface (`runtimes`, `experiments`,
//!    `Runner`, `tasks`, `files`, `shares`, and runner telemetry reads). Always available, no OpenTelemetry
//!    dependency. No feature flag required. [`auth`] mints the token it
//!    takes when you have OAuth credentials rather than an API key.
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
//! let runtime = client.runtime("customer-agent").await?;
//! // runtime.run(Default::default()).await?;
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
//! | `INTROSPECTION_SERVICE_NAME`    | Service name (logs/traces)                  |
//! | `INTROSPECTION_BASE_API_URL`    | REST API host (default `api.introspection.dev`) |
//! | `INTROSPECTION_BASE_OTEL_URL`   | OTLP collector host (default `otel.introspection.dev`) |
//! | `INTROSPECTION_DEV_TARGET`      | Development only: route this process's tasks to your own `introspection dev` server. Rides every request as a header. No default — see [`dev_target`] |

pub mod agui;
pub mod api;
pub mod auth;
pub mod client;
pub mod dev_target;
// Always compiled — `otel::messages` carries the gen_ai semantic-convention
// message vocabulary that the REST-only conversations read returns. The OTLP
// exporter surfaces under it stay gated on the `otel` feature from inside.
pub mod otel;
pub mod resources;
pub mod runner;
pub mod types;

// Re-export wire types + low-level REST API surface (always available)
pub use api::{
    AgentInfo, AnnotationEvent, AnnotationPayload, Arm, ClusteringRunEvent, ClusteringRunPayload,
    Connection, ConnectionAuthorizationPending, ConnectionBrokerSubjectType,
    ConnectionCreateParams, ConnectionCreateSubjectType, ConnectionMissionConstraints,
    ConnectionStatus, ConnectionSubjectType, ConnectionToken, ConnectionTokenParams,
    ConnectionTokenResult, Connector, ConnectorAuthMode, ConnectorAuthorization,
    ConnectorAuthorizeParams, ConnectorCreateParams, ConnectorListParams, ConnectorStatus,
    ConnectorUpdateParams, Conversation, ConversationAgent, ConversationCost,
    ConversationExportFormat, ConversationExportParams, ConversationItemGetParams,
    ConversationItemInclude, ConversationItemListParams, ConversationItemPaginator,
    ConversationItems, ConversationListParams, ConversationMetrics, ConversationResolution,
    ConversationSentiment, ConversationStatus, ConversationUsage, Conversations, Dimension, Event,
    EventListParams, Events, Experiment, ExperimentGoal, ExperimentGoalComponent,
    ExperimentGoalDirection, ExperimentGoalGuard, ExperimentListParams, ExperimentStatus,
    FeedbackEvent, FeedbackPayload, File, FileCreateText, FileListParams, FileType, FileUpdate,
    FileUpload, FileVersions, Files, GenAiAgent, GenAiAttributes, GenAiInput, GenAiOutput,
    GenAiRequest, GenAiResponse, GenAiSpan, GenAiSpanList, GenAiTool, GenAiToolCall, GenAiUsage,
    HavingTerm, IdRef, IntrospectionAPIError, IntrospectionAttributes, IntrospectionConversation,
    IntrospectionEventName, IntrospectionRecipe, IntrospectionRuntime, JudgeGoalComponent,
    JudgementEvent, JudgementPayload, MetricFilter, MetricSpec, Metrics, MetricsConfig,
    MetricsQuery, MetricsResponse, NameRef, ObservationEvent, ObservationPayload, OrderTerm,
    Paginated, PaginationParams, Paginator, PatternAssignmentEvent, PatternAssignmentPayload,
    PatternEvent, PatternPayload, Recipe, RecipeListParams, ResourceShare, ResumeEntry, RunCaller,
    RunCallerLibrary, RunCallerPage, RunHandle, RunRequest, RunnerContext, RunnerDeployment,
    RunnerIdentity, RunnerSpec, Runtime, RuntimeListParams, RuntimeLlmMode, ShareCreate,
    ShareListParams, ShareResourceType, Shares, SortDirection, SpanAttributes, SpanStatus,
    SseEvent, StreamOptions, StringOrUuid, Task, TaskCancelOptions, TaskCancelResponse, TaskCreate,
    TaskCreateResponse, TaskFileRef, TaskKind, TaskListParams, TaskPrompt, TaskRepoRequest,
    TaskRun, TaskRunCreate, TaskRunKind, TaskRunResponse, TaskRunResume, TaskRuns, TaskStatus,
    TaskUpdate, Tasks, TelemetryGoalComponent, TimeDimension, TokenCount, Trajectory, TypedEvent,
    UploadSource,
};
#[cfg(feature = "arrow")]
pub use api::{ArrowPage, ARROW_STREAM_ACCEPT};
// AG-UI protocol event surface yielded by the task-run stream. The full
// taxonomy lives in `crate::agui`; these aliases give the common types a
// discoverable name at the crate root (`Event` alone would be ambiguous).
pub use agui::{Event as AgUiEvent, EventType as AgUiEventType};
pub use auth::{
    authorization_code_token, service_account_token, token_exchange, AuthorizationCodeParams,
    OAuthToken, ServiceAccountTokenParams, TokenExchangeParams,
};
pub use client::{IntrospectionClient, IntrospectionError, Result, VERSION};
pub use resources::annotations::{
    AnnotationEventOptions, AnnotationListParams, AnnotationMutation, AnnotationState,
    AnnotationTarget, ProjectLabel, ProjectLabelCreate, ProjectLabelListParams, ProjectLabelUpdate,
};
pub use resources::{
    Annotations, Connections, Connectors, ExperimentHandle, Experiments, ProjectLabels, Recipes,
    RuntimeHandle, Runtimes,
};
pub use runner::{Runner, RunnerSource};
pub use types::{AdvancedOptions, ClientConfig, ClientConfigBuilder};

// OTel surfaces — gated behind the `otel` feature, re-exported from
// `crate::otel` for top-level access.
#[cfg(feature = "otel")]
pub use otel::{
    BaggageGuard, FeedbackOptions, IdentifyOptions, IntrospectionLogs, IntrospectionLogsConfig,
    IntrospectionLogsConfigBuilder, IntrospectionLogsError, IntrospectionSpanProcessor,
    PropertyValue, SpanProcessorAdvancedOptions, SpanProcessorConfig, SpanProcessorConfigBuilder,
    SpanProcessorError, SpanProcessorResult, TrackOptions,
};

// Always available: the conversations read returns these message types inside
// `attributes.gen_ai.{input,output}.messages`, with or without the `otel`
// feature.
pub use otel::messages::{
    ContentPart, InputMessage, OutputMessage, TextPart, ThinkingPart, ToolCallRequestPart,
    ToolCallResponsePart,
};
