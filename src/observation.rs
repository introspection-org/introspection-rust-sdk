//! High-level Observation API for instrumenting LLM calls and pipeline steps.
//!
//! Provides [`Observation`] — an RAII span handle that automatically sets
//! gen_ai semantic convention attributes and ends the span on drop.
//!
//! # Example
//!
//! ```rust,no_run
//! use introspection_sdk::{Observation, ObservationConfig, GenerationUpdate};
//! use opentelemetry::trace::TracerProvider;
//! use opentelemetry_sdk::trace::SdkTracerProvider;
//!
//! let provider = SdkTracerProvider::builder().build();
//! let tracer = provider.tracer("my-app");
//!
//! // Wrap an LLM call
//! let mut obs = Observation::start(&tracer, ObservationConfig::generation("chat", "gpt-4o-mini"));
//! // ... make the API call ...
//! obs.update_generation(
//!     GenerationUpdate::new()
//!         .with_response_model("gpt-4o-mini")
//!         .with_response_id("chatcmpl-abc123")
//!         .with_usage(12, 8),
//! );
//! // span ends when `obs` is dropped
//! ```

use opentelemetry::trace::{Span, SpanKind, Status, TraceContextExt, Tracer};
use opentelemetry::{Context, KeyValue};

use crate::messages::{InputMessage, OutputMessage};
use crate::types::attr;

/// The type of observation being recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationType {
    /// A general pipeline step (maps to `SpanKind::Internal`).
    Span,
    /// An LLM generation call (maps to `SpanKind::Client`).
    Generation,
}

/// Token usage information for an LLM call.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

/// Response-side update payload for a generation observation.
#[derive(Debug, Clone, Default)]
pub struct GenerationUpdate {
    pub output: Option<Vec<OutputMessage>>,
    pub usage: Option<Usage>,
    pub response_model: Option<String>,
    pub response_id: Option<String>,
    pub status: Option<Status>,
}

impl GenerationUpdate {
    /// Create an empty update.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the response model name.
    pub fn with_response_model(mut self, model: impl Into<String>) -> Self {
        self.response_model = Some(model.into());
        self
    }

    /// Set the response ID.
    pub fn with_response_id(mut self, id: impl Into<String>) -> Self {
        self.response_id = Some(id.into());
        self
    }

    /// Set output messages using strongly typed [`OutputMessage`] structs.
    pub fn with_output(mut self, output: Vec<OutputMessage>) -> Self {
        self.output = Some(output);
        self
    }

    /// Set token usage with input and output token counts.
    pub fn with_usage(mut self, input_tokens: i64, output_tokens: i64) -> Self {
        self.usage = Some(Usage {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            total_tokens: Some(input_tokens + output_tokens),
        });
        self
    }

    /// Set full usage information.
    pub fn with_full_usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Set a span status override.
    pub fn with_status(mut self, status: Status) -> Self {
        self.status = Some(status);
        self
    }
}

/// Builder for configuring an [`Observation`] before starting it.
#[derive(Debug, Clone)]
pub struct ObservationConfig {
    pub name: String,
    pub observation_type: ObservationType,
    pub model: Option<String>,
    pub system: Option<String>,
    pub operation_name: Option<String>,
    /// Typed input messages for LLM generation observations.
    pub input: Option<Vec<InputMessage>>,
    pub attributes: Vec<KeyValue>,
}

impl ObservationConfig {
    /// Create a config for an LLM generation call.
    ///
    /// `name` is the span name (e.g. `"chat"`), `model` is the model identifier
    /// (e.g. `"gpt-4o-mini"`). The system is auto-inferred from the model name.
    pub fn generation(name: impl Into<String>, model: impl Into<String>) -> Self {
        let model_str = model.into();
        let system = infer_system(&model_str);
        Self {
            name: name.into(),
            observation_type: ObservationType::Generation,
            model: Some(model_str),
            system,
            operation_name: Some("chat".to_string()),
            input: None,
            attributes: Vec::new(),
        }
    }

    /// Create a config for a general pipeline span.
    pub fn span(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            observation_type: ObservationType::Span,
            model: None,
            system: None,
            operation_name: None,
            input: None,
            attributes: Vec::new(),
        }
    }

    /// Set input messages using strongly typed [`InputMessage`] structs.
    pub fn with_input(mut self, input: Vec<InputMessage>) -> Self {
        self.input = Some(input);
        self
    }

    /// Override the auto-inferred system (e.g. `"openai"`, `"anthropic"`).
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Override the default operation name (default is `"chat"` for generations).
    pub fn with_operation_name(mut self, op: impl Into<String>) -> Self {
        self.operation_name = Some(op.into());
        self
    }

    /// Add a custom attribute.
    pub fn with_attribute(mut self, kv: KeyValue) -> Self {
        self.attributes.push(kv);
        self
    }
}

/// RAII handle for an active observation span.
///
/// The span is ended automatically when this handle is dropped.
/// The type parameter `S` is the concrete span type from the tracer —
/// it is inferred automatically when calling [`Observation::start`].
///
/// The observation is registered in the OTel context so that child
/// observations created within the same scope are automatically nested.
///
/// Use [`Observation::start`] to create one.
pub struct Observation<S: Span> {
    span: S,
    _context_guard: opentelemetry::ContextGuard,
    ended: bool,
}

impl<S: Span> Observation<S> {
    /// Start a new observation span.
    ///
    /// The span is created with attributes derived from `config`.
    /// The type parameter is inferred from the tracer.
    pub fn start<T: Tracer<Span = S>>(tracer: &T, config: ObservationConfig) -> Self {
        let kind = match config.observation_type {
            ObservationType::Generation => SpanKind::Client,
            ObservationType::Span => SpanKind::Internal,
        };

        let mut span = tracer
            .span_builder(config.name)
            .with_kind(kind)
            .start(tracer);

        // Set request-side attributes for generations
        if config.observation_type == ObservationType::Generation {
            span.set_attribute(KeyValue::new("openinference.span.kind", "LLM"));
            span.set_attribute(KeyValue::new("gen_ai.provider.name", "openai"));
            if let Some(ref model) = config.model {
                span.set_attribute(KeyValue::new(attr::GEN_AI_REQUEST_MODEL, model.clone()));
            }
            if let Some(ref system) = config.system {
                span.set_attribute(KeyValue::new(attr::GEN_AI_SYSTEM, system.clone()));
            }
            if let Some(ref op) = config.operation_name {
                span.set_attribute(KeyValue::new(attr::GEN_AI_OPERATION_NAME, op.clone()));
            }
            if let Some(ref input) = config.input {
                span.set_attribute(KeyValue::new(
                    attr::GEN_AI_INPUT_MESSAGES,
                    serde_json::to_string(input).unwrap_or_default(),
                ));
            }
        }

        // Apply any extra attributes
        for kv in config.attributes {
            span.set_attribute(kv);
        }

        // Register span context so child observations are nested automatically.
        // We use with_remote_span_context to propagate the trace/span IDs
        // without moving the span out of our ownership.
        let span_context = span.span_context().clone();
        let cx = Context::current().with_remote_span_context(span_context);
        let guard = cx.attach();

        Self {
            span,
            _context_guard: guard,
            ended: false,
        }
    }

    /// Update this generation observation with response-side data.
    ///
    /// Sets `gen_ai.response.*` and `gen_ai.usage.*` attributes.
    pub fn update_generation(&mut self, update: GenerationUpdate) {
        if let Some(model) = update.response_model {
            self.span
                .set_attribute(KeyValue::new(attr::GEN_AI_RESPONSE_MODEL, model));
        }
        if let Some(id) = update.response_id {
            self.span
                .set_attribute(KeyValue::new(attr::GEN_AI_RESPONSE_ID, id));
        }
        if let Some(output) = update.output {
            self.span.set_attribute(KeyValue::new(
                attr::GEN_AI_OUTPUT_MESSAGES,
                serde_json::to_string(&output).unwrap_or_default(),
            ));
        }
        if let Some(usage) = update.usage {
            if let Some(input_tokens) = usage.input_tokens {
                self.span
                    .set_attribute(KeyValue::new(attr::GEN_AI_USAGE_INPUT_TOKENS, input_tokens));
            }
            if let Some(output_tokens) = usage.output_tokens {
                self.span.set_attribute(KeyValue::new(
                    attr::GEN_AI_USAGE_OUTPUT_TOKENS,
                    output_tokens,
                ));
            }
            if let Some(total_tokens) = usage.total_tokens {
                self.span
                    .set_attribute(KeyValue::new(attr::GEN_AI_USAGE_TOTAL_TOKENS, total_tokens));
            }
        }
        if let Some(status) = update.status {
            self.span.set_status(status);
        }
    }

    /// Shorthand: set the output messages.
    pub fn set_output(&mut self, output: Vec<OutputMessage>) {
        self.span.set_attribute(KeyValue::new(
            attr::GEN_AI_OUTPUT_MESSAGES,
            serde_json::to_string(&output).unwrap_or_default(),
        ));
    }

    /// Shorthand: set token usage counts.
    pub fn set_usage(&mut self, input_tokens: i64, output_tokens: i64) {
        self.span
            .set_attribute(KeyValue::new(attr::GEN_AI_USAGE_INPUT_TOKENS, input_tokens));
        self.span.set_attribute(KeyValue::new(
            attr::GEN_AI_USAGE_OUTPUT_TOKENS,
            output_tokens,
        ));
        self.span.set_attribute(KeyValue::new(
            attr::GEN_AI_USAGE_TOTAL_TOKENS,
            input_tokens + output_tokens,
        ));
    }

    /// Set an arbitrary attribute on the span.
    pub fn set_attribute(&mut self, kv: KeyValue) {
        self.span.set_attribute(kv);
    }

    /// Mark the span as errored.
    pub fn set_error(&mut self, description: impl Into<String>) {
        self.span.set_status(Status::error(description.into()));
    }

    /// Mark the span as OK.
    pub fn set_ok(&mut self) {
        self.span.set_status(Status::Ok);
    }

    /// Explicitly end the span. Also happens automatically on drop.
    pub fn end(mut self) {
        if !self.ended {
            self.span.end();
            self.ended = true;
        }
    }
}

impl<S: Span> Drop for Observation<S> {
    fn drop(&mut self) {
        if !self.ended {
            self.span.end();
            self.ended = true;
        }
    }
}

/// Infer the gen_ai system from a model name.
///
/// Returns `Some("openai")` for GPT models, `Some("anthropic")` for Claude, etc.
pub fn infer_system(model: &str) -> Option<String> {
    let lower = model.to_lowercase();
    if lower.starts_with("gpt") || lower.starts_with("o1") || lower.starts_with("o3") {
        Some("openai".to_string())
    } else if lower.starts_with("claude") {
        Some("anthropic".to_string())
    } else if lower.starts_with("gemini") {
        Some("google".to_string())
    } else if lower.starts_with("mistral") || lower.starts_with("mixtral") {
        Some("mistral".to_string())
    } else if lower.starts_with("llama") {
        Some("meta".to_string())
    } else if lower.starts_with("command") {
        Some("cohere".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_system_openai() {
        assert_eq!(infer_system("gpt-4o-mini"), Some("openai".to_string()));
        assert_eq!(infer_system("gpt-4"), Some("openai".to_string()));
        assert_eq!(infer_system("o1-preview"), Some("openai".to_string()));
        assert_eq!(infer_system("o3-mini"), Some("openai".to_string()));
    }

    #[test]
    fn test_infer_system_anthropic() {
        assert_eq!(infer_system("claude-3-opus"), Some("anthropic".to_string()));
        assert_eq!(
            infer_system("claude-3.5-sonnet"),
            Some("anthropic".to_string())
        );
    }

    #[test]
    fn test_infer_system_google() {
        assert_eq!(infer_system("gemini-1.5-pro"), Some("google".to_string()));
    }

    #[test]
    fn test_infer_system_unknown() {
        assert_eq!(infer_system("some-custom-model"), None);
    }

    #[test]
    fn test_observation_config_generation() {
        let config = ObservationConfig::generation("chat", "gpt-4o-mini")
            .with_input(vec![crate::messages::InputMessage::user("hi")]);
        assert_eq!(config.observation_type, ObservationType::Generation);
        assert_eq!(config.model, Some("gpt-4o-mini".to_string()));
        assert_eq!(config.system, Some("openai".to_string()));
        assert_eq!(config.operation_name, Some("chat".to_string()));
        assert!(config.input.is_some());
    }

    #[test]
    fn test_observation_config_span() {
        let config = ObservationConfig::span("my-step");
        assert_eq!(config.observation_type, ObservationType::Span);
        assert_eq!(config.model, None);
        assert_eq!(config.system, None);
    }

    #[test]
    fn test_generation_update_builder() {
        let update = GenerationUpdate::new()
            .with_response_model("gpt-4o-mini")
            .with_response_id("chatcmpl-123")
            .with_usage(10, 5)
            .with_output(vec![crate::messages::OutputMessage::assistant("hello")]);

        assert_eq!(update.response_model, Some("gpt-4o-mini".to_string()));
        assert_eq!(update.response_id, Some("chatcmpl-123".to_string()));
        assert!(update.output.is_some());
        let usage = update.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(5));
        assert_eq!(usage.total_tokens, Some(15));
    }
}
