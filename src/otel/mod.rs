//! OpenTelemetry-powered surfaces for the Introspection SDK.
//!
//! Everything here except [`messages`] is compiled only with the `otel` Cargo
//! feature. [`messages`] is always available because it is the gen_ai
//! semantic-convention message vocabulary, which the REST-only conversations
//! read ([`crate::api::genai_span`]) returns — the same types on the way out
//! over OTLP and on the way back in over `/v1/conversations`, which is the
//! point of a shared vocabulary.
//!
//! The feature-gated part hosts two independent surfaces that customers can
//! mix-and-match:
//!
//! * [`IntrospectionLogs`] — owns an [`opentelemetry_sdk::logs::SdkLoggerProvider`]
//!   and exports `track` / `feedback` / `identify` events over OTLP HTTP.
//! * [`IntrospectionSpanProcessor`] — a [`opentelemetry_sdk::trace::SpanProcessor`]
//!   you attach to your own `SdkTracerProvider` to forward spans over OTLP HTTP.
//!
//! These two surfaces share no state. They are also fully independent
//! from [`crate::IntrospectionClient`] (the always-on REST surface).
//!
//! Higher-level helpers — [`messages`] and [`observation`] — also live
//! under this module.

#[cfg(feature = "otel")]
pub mod logs;
pub mod messages;
#[cfg(feature = "otel")]
pub mod observation;
#[cfg(feature = "otel")]
pub mod span_processor;
#[cfg(feature = "otel")]
pub mod types;

// `testing` for downstream consumers; `test` so the in-crate tests build
// under any feature set (the in-memory exporters come from the
// `opentelemetry_sdk` dev-dependency, which enables its `testing` feature).
#[cfg(any(feature = "testing", all(test, feature = "otel")))]
pub mod testing;

#[cfg(feature = "otel")]
pub use logs::{
    BaggageGuard, IntrospectionLogs, IntrospectionLogsConfig, IntrospectionLogsConfigBuilder,
    IntrospectionLogsError,
};
pub use messages::{
    ContentPart, InputMessage, OutputMessage, TextPart, ThinkingPart, ToolCallRequestPart,
    ToolCallResponsePart,
};
#[cfg(feature = "otel")]
pub use observation::{GenerationUpdate, Observation, ObservationConfig, ObservationType, Usage};
#[cfg(feature = "otel")]
pub use span_processor::{
    IntrospectionSpanProcessor, SpanProcessorAdvancedOptions, SpanProcessorConfig,
    SpanProcessorConfigBuilder, SpanProcessorError, SpanProcessorResult,
};
#[cfg(feature = "otel")]
pub use types::{
    api_path, attr, baggage, defaults, event_name, generate_event_id, logger_name, severity,
    FeedbackOptions, IdentifyOptions, PropertyValue, TrackOptions,
};
