//! OpenTelemetry-powered surfaces for the Introspection SDK.
//!
//! This module is only compiled with the `otel` Cargo feature. It hosts
//! two independent surfaces that customers can mix-and-match:
//!
//! * [`IntrospectionLogs`] — owns an [`opentelemetry_sdk::logs::SdkLoggerProvider`]
//!   and exports `track` / `feedback` / `identify` events over OTLP HTTP.
//! * [`IntrospectionSpanProcessor`] — a [`opentelemetry_sdk::trace::SpanProcessor`]
//!   you attach to your own `SdkTracerProvider` to forward spans over OTLP HTTP.
//!
//! These two surfaces share no state. They are also fully independent
//! from [`crate::IntrospectionClient`] (the always-on REST surface).
//!
//! Higher-level helpers — [`messages`], [`observation`], and the
//! `async-openai` adapter at [`openai`] (gated on the `openai` feature) —
//! also live under this module.

pub mod logs;
pub mod messages;
pub mod observation;
pub mod span_processor;
pub mod types;

#[cfg(feature = "openai")]
pub mod openai;

#[cfg(any(feature = "testing", all(test, feature = "otel")))]
pub mod testing;

pub use logs::{
    BaggageGuard, IntrospectionLogs, IntrospectionLogsConfig, IntrospectionLogsConfigBuilder,
    IntrospectionLogsError,
};
pub use messages::{
    ContentPart, InputMessage, OutputMessage, TextPart, ThinkingPart, ToolCallRequestPart,
    ToolCallResponsePart,
};
pub use observation::{GenerationUpdate, Observation, ObservationConfig, ObservationType, Usage};
pub use span_processor::{
    IntrospectionSpanProcessor, SpanProcessorAdvancedOptions, SpanProcessorConfig,
    SpanProcessorConfigBuilder, SpanProcessorError, SpanProcessorResult,
};
pub use types::{
    api_path, attr, baggage, defaults, event_name, generate_event_id, logger_name, severity,
    FeedbackOptions, IdentifyOptions, PropertyValue, TrackOptions,
};

#[cfg(feature = "openai")]
pub use openai::TracedStream;
