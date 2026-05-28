//! OpenTelemetry-flavoured type definitions for the Introspection SDK.
//!
//! These types are gated behind the `otel` Cargo feature and feed
//! [`crate::otel::IntrospectionLogs`] (track/feedback/identify) and
//! [`crate::otel::IntrospectionSpanProcessor`]. The REST-only types
//! ([`crate::AdvancedOptions`], [`crate::ClientConfig`]) live in
//! [`crate::types`] and are always available.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Options for the `track` method on [`crate::otel::IntrospectionLogs`].
///
/// # Example
///
/// ```rust
/// use introspection_sdk::otel::TrackOptions;
///
/// let options = TrackOptions::new()
///     .with_property("button_id", "submit")
///     .with_property("page", "checkout");
/// ```
#[derive(Debug, Clone, Default)]
pub struct TrackOptions {
    /// Event properties
    pub properties: HashMap<String, PropertyValue>,

    /// Custom event ID (auto-generated if not provided)
    pub event_id: Option<String>,
}

impl TrackOptions {
    /// Create new empty track options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a property to the event.
    pub fn with_property(
        mut self,
        key: impl Into<String>,
        value: impl Into<PropertyValue>,
    ) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }

    /// Set multiple properties at once.
    pub fn with_properties(mut self, properties: HashMap<String, PropertyValue>) -> Self {
        self.properties.extend(properties);
        self
    }

    /// Set a custom event ID.
    pub fn with_event_id(mut self, event_id: impl Into<String>) -> Self {
        self.event_id = Some(event_id.into());
        self
    }
}

/// Options for the `feedback` method on [`crate::otel::IntrospectionLogs`].
///
/// # Example
///
/// ```rust
/// use introspection_sdk::otel::FeedbackOptions;
///
/// let options = FeedbackOptions::new()
///     .with_comments("Great response!")
///     .with_conversation_id("conv_123");
/// ```
#[derive(Debug, Clone, Default)]
pub struct FeedbackOptions {
    /// User's comments (e.g., "Answer was off topic")
    pub comments: Option<String>,

    /// Conversation/session ID (falls back to baggage context)
    pub conversation_id: Option<String>,

    /// ID of the response being given feedback on
    pub previous_response_id: Option<String>,

    /// Custom event ID (auto-generated if not provided)
    pub event_id: Option<String>,

    /// Additional custom properties
    pub extra: HashMap<String, PropertyValue>,
}

impl FeedbackOptions {
    /// Create new empty feedback options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set user comments.
    pub fn with_comments(mut self, comments: impl Into<String>) -> Self {
        self.comments = Some(comments.into());
        self
    }

    /// Set conversation ID.
    pub fn with_conversation_id(mut self, conversation_id: impl Into<String>) -> Self {
        self.conversation_id = Some(conversation_id.into());
        self
    }

    /// Set previous response ID.
    pub fn with_previous_response_id(mut self, previous_response_id: impl Into<String>) -> Self {
        self.previous_response_id = Some(previous_response_id.into());
        self
    }

    /// Set a custom event ID.
    pub fn with_event_id(mut self, event_id: impl Into<String>) -> Self {
        self.event_id = Some(event_id.into());
        self
    }

    /// Add an extra property.
    pub fn with_extra(mut self, key: impl Into<String>, value: impl Into<PropertyValue>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }
}

/// Options for the `identify` method on [`crate::otel::IntrospectionLogs`].
///
/// # Example
///
/// ```rust
/// use introspection_sdk::otel::IdentifyOptions;
///
/// let options = IdentifyOptions::new()
///     .with_trait("email", "user@example.com")
///     .with_trait("plan", "pro");
/// ```
#[derive(Debug, Clone, Default)]
pub struct IdentifyOptions {
    /// User traits (email, name, plan, etc.)
    pub traits: HashMap<String, PropertyValue>,

    /// Anonymous ID to associate with the user
    pub anonymous_id: Option<String>,

    /// Custom event ID (auto-generated if not provided)
    pub event_id: Option<String>,
}

impl IdentifyOptions {
    /// Create new empty identify options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a user trait.
    pub fn with_trait(mut self, key: impl Into<String>, value: impl Into<PropertyValue>) -> Self {
        self.traits.insert(key.into(), value.into());
        self
    }

    /// Set multiple traits at once.
    pub fn with_traits(mut self, traits: HashMap<String, PropertyValue>) -> Self {
        self.traits.extend(traits);
        self
    }

    /// Set the anonymous ID.
    pub fn with_anonymous_id(mut self, anonymous_id: impl Into<String>) -> Self {
        self.anonymous_id = Some(anonymous_id.into());
        self
    }

    /// Set a custom event ID.
    pub fn with_event_id(mut self, event_id: impl Into<String>) -> Self {
        self.event_id = Some(event_id.into());
        self
    }
}

/// A property value that can be a string, number, boolean, or JSON object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Json(serde_json::Value),
}

impl From<&str> for PropertyValue {
    fn from(s: &str) -> Self {
        PropertyValue::String(s.to_string())
    }
}

impl From<String> for PropertyValue {
    fn from(s: String) -> Self {
        PropertyValue::String(s)
    }
}

impl From<i32> for PropertyValue {
    fn from(n: i32) -> Self {
        PropertyValue::Int(n as i64)
    }
}

impl From<i64> for PropertyValue {
    fn from(n: i64) -> Self {
        PropertyValue::Int(n)
    }
}

impl From<f64> for PropertyValue {
    fn from(n: f64) -> Self {
        PropertyValue::Float(n)
    }
}

impl From<bool> for PropertyValue {
    fn from(b: bool) -> Self {
        PropertyValue::Bool(b)
    }
}

impl From<serde_json::Value> for PropertyValue {
    fn from(v: serde_json::Value) -> Self {
        PropertyValue::Json(v)
    }
}

impl PropertyValue {
    /// Convert to a string representation for OpenTelemetry attributes.
    pub fn to_otel_string(&self) -> String {
        match self {
            PropertyValue::String(s) => s.clone(),
            PropertyValue::Int(n) => n.to_string(),
            PropertyValue::Float(n) => n.to_string(),
            PropertyValue::Bool(b) => b.to_string(),
            PropertyValue::Json(v) => v.to_string(),
        }
    }
}

/// Generate a unique event ID.
///
/// Format: `intro_event_<timestamp>-<random8>`
pub fn generate_event_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let timestamp_hex = format!("{:x}", timestamp);
    let random_part = &Uuid::new_v4().to_string()[..8];
    format!("intro_event_{}-{}", timestamp_hex, random_part)
}

/// Standard log attribute keys used by the Introspection SDK.
/// These follow OpenTelemetry semantic conventions where applicable.
pub mod attr {
    // Core event fields
    pub const EVENT_NAME: &str = "event.name";
    pub const EVENT_ID: &str = "event.id";

    // Identity
    pub const USER_ID: &str = "identity.user.id";
    pub const ANONYMOUS_ID: &str = "identity.anonymous.id";

    // Gen AI (OTel semantic conventions)
    pub const CONVERSATION_ID: &str = "gen_ai.conversation.id";
    pub const PREVIOUS_RESPONSE_ID: &str = "gen_ai.request.previous_response_id";
    pub const AGENT_NAME: &str = "gen_ai.agent.name";
    pub const AGENT_ID: &str = "gen_ai.agent.id";

    // Gen AI span attributes (OTel semantic conventions for LLM observability)
    pub const GEN_AI_SYSTEM: &str = "gen_ai.system";
    pub const GEN_AI_OPERATION_NAME: &str = "gen_ai.operation.name";
    pub const GEN_AI_REQUEST_MODEL: &str = "gen_ai.request.model";
    pub const GEN_AI_RESPONSE_MODEL: &str = "gen_ai.response.model";
    pub const GEN_AI_RESPONSE_ID: &str = "gen_ai.response.id";
    pub const GEN_AI_USAGE_INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";
    pub const GEN_AI_USAGE_OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
    pub const GEN_AI_USAGE_TOTAL_TOKENS: &str = "gen_ai.usage.total_tokens";
    pub const GEN_AI_INPUT_MESSAGES: &str = "gen_ai.input.messages";
    pub const GEN_AI_OUTPUT_MESSAGES: &str = "gen_ai.output.messages";
    pub const GEN_AI_SYSTEM_INSTRUCTIONS: &str = "gen_ai.system_instructions";
    pub const GEN_AI_TOOL_DEFINITIONS: &str = "gen_ai.tool.definitions";

    // Prefixes for dynamic keys
    pub const PROPERTIES_PREFIX: &str = "properties.";
    pub const TRAITS_PREFIX: &str = "context.traits.";
}

/// Baggage keys used for context propagation.
/// Note: Identity keys use underscores instead of dots for baggage compatibility.
pub mod baggage {
    pub const USER_ID: &str = "identity.user_id";
    pub const ANONYMOUS_ID: &str = "identity.anonymous_id";
    pub const CONVERSATION_ID: &str = "gen_ai.conversation.id";
    pub const PREVIOUS_RESPONSE_ID: &str = "gen_ai.request.previous_response_id";
    pub const AGENT_NAME: &str = "gen_ai.agent.name";
    pub const AGENT_ID: &str = "gen_ai.agent.id";
}

/// Standard event names used by the Introspection SDK.
pub mod event_name {
    pub const IDENTIFY: &str = "identify";
    pub const FEEDBACK: &str = "introspection.feedback";
}

/// OTel-related default configuration values.
pub mod defaults {
    /// Default OTLP collector base URL.
    pub const BASE_OTEL_URL: &str = "https://otel.introspection.dev";
    pub const FLUSH_INTERVAL_MS: u64 = 5000;
    pub const MAX_BATCH_SIZE: usize = 100;
}

/// Log severity text constants.
pub mod severity {
    pub const INFO: &str = "INFO";
}

/// Logger names for OpenTelemetry instrumentation scope.
pub mod logger_name {
    pub const RUST_SDK: &str = "introspection-sdk-rust";
}

/// API endpoint paths.
pub mod api_path {
    pub const LOGS: &str = "/v1/logs";
    pub const TRACES: &str = "/v1/traces";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_options_builder() {
        let options = TrackOptions::new()
            .with_property("key", "value")
            .with_event_id("custom_id");

        assert!(options.properties.contains_key("key"));
        assert_eq!(options.event_id, Some("custom_id".to_string()));
    }

    #[test]
    fn test_feedback_options_builder() {
        let options = FeedbackOptions::new()
            .with_comments("Great!")
            .with_conversation_id("conv_123")
            .with_extra("rating", 5);

        assert_eq!(options.comments, Some("Great!".to_string()));
        assert_eq!(options.conversation_id, Some("conv_123".to_string()));
        assert!(options.extra.contains_key("rating"));
    }

    #[test]
    fn test_generate_event_id() {
        let id = generate_event_id();
        assert!(id.starts_with("intro_event_"));
    }

    #[test]
    fn test_property_value_conversions() {
        let s: PropertyValue = "hello".into();
        assert!(matches!(s, PropertyValue::String(_)));

        let n: PropertyValue = 42i64.into();
        assert!(matches!(n, PropertyValue::Int(42)));

        let b: PropertyValue = true.into();
        assert!(matches!(b, PropertyValue::Bool(true)));
    }

    #[test]
    fn test_property_value_to_otel_string() {
        assert_eq!(
            PropertyValue::String("hello".to_string()).to_otel_string(),
            "hello"
        );
        assert_eq!(PropertyValue::Int(42).to_otel_string(), "42");
        assert_eq!(PropertyValue::Float(99.99).to_otel_string(), "99.99");
        assert_eq!(PropertyValue::Bool(true).to_otel_string(), "true");
    }
}
