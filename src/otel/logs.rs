//! [`IntrospectionLogs`] — independent OTLP Logs exporter for the
//! Introspection SDK.
//!
//! Owns its own [`SdkLoggerProvider`] and exposes the
//! `track(...)` / `feedback(...)` / `identify(...)` analytics surface
//! plus `set_user_id` / `set_anonymous_id` / `set_conversation_id` /
//! `set_previous_response_id` / `set_agent` baggage guards.
//!
//! This struct does **not** borrow from or depend on
//! [`crate::IntrospectionClient`] (the REST surface). Construct it
//! independently when you want analytics events:
//!
//! ```rust,no_run
//! use introspection_sdk::otel::IntrospectionLogs;
//!
//! let logs = IntrospectionLogs::builder()
//!     .token("your-token")
//!     .service_name("my-service")
//!     .build()
//!     .unwrap();
//!
//! logs.track("Button Clicked", None);
//! logs.shutdown().unwrap();
//! ```

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use opentelemetry::logs::{LogRecord as _, Logger, LoggerProvider, Severity};
use opentelemetry::{baggage::BaggageExt, Context, Key, KeyValue};
use opentelemetry_otlp::{LogExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{logs::SdkLoggerProvider, Resource};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::otel::types::{
    self, generate_event_id, FeedbackOptions, IdentifyOptions, PropertyValue, TrackOptions,
};

/// Errors that can be returned by [`IntrospectionLogs`].
#[derive(Error, Debug)]
pub enum IntrospectionLogsError {
    #[error("OpenTelemetry error: {0}")]
    OpenTelemetry(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

impl From<opentelemetry_sdk::error::OTelSdkError> for IntrospectionLogsError {
    fn from(e: opentelemetry_sdk::error::OTelSdkError) -> Self {
        IntrospectionLogsError::OpenTelemetry(e.to_string())
    }
}

impl From<derive_builder::UninitializedFieldError> for IntrospectionLogsError {
    fn from(e: derive_builder::UninitializedFieldError) -> Self {
        IntrospectionLogsError::Config(e.to_string())
    }
}

/// Result type for [`IntrospectionLogs`] operations.
pub type Result<T> = std::result::Result<T, IntrospectionLogsError>;

/// Independent OTLP Logs exporter — owns its own [`SdkLoggerProvider`]
/// and emits `track` / `feedback` / `identify` events with
/// OpenTelemetry baggage-managed context.
///
/// Construct via [`IntrospectionLogs::builder`].
pub struct IntrospectionLogs {
    /// OpenTelemetry logger provider
    logger_provider: SdkLoggerProvider,

    /// OpenTelemetry logger
    logger: opentelemetry_sdk::logs::SdkLogger,

    /// Service name (used in tests).
    #[allow(dead_code)]
    service_name: String,

    /// User traits from identify calls (stored locally).
    #[allow(dead_code)]
    traits: Arc<RwLock<HashMap<String, PropertyValue>>>,
}

/// Builder-friendly config for [`IntrospectionLogs`]. Use
/// [`IntrospectionLogs::builder`] rather than constructing this
/// directly.
#[derive(Default, Clone, Debug, derive_builder::Builder)]
#[builder(
    setter(into, strip_option),
    default,
    pattern = "owned",
    build_fn(name = "build_config", private, error = "IntrospectionLogsError")
)]
pub struct IntrospectionLogsConfig {
    /// Authentication token (env: `INTROSPECTION_TOKEN`).
    pub token: Option<String>,

    /// Service name (env: `INTROSPECTION_SERVICE_NAME`,
    /// default: `"introspection-client"`).
    pub service_name: Option<String>,

    /// OTLP collector base URL (env: `INTROSPECTION_BASE_OTEL_URL`,
    /// default: `https://otel.introspection.dev`).
    pub base_otel_url: Option<String>,

    /// Additional HTTP headers to attach to OTLP exports.
    pub additional_headers: Option<HashMap<String, String>>,

    /// Custom log exporter — bypasses the default OTLP HTTP exporter.
    /// Primarily used for testing.
    pub log_exporter: Option<Arc<LogExporter>>,
}

impl IntrospectionLogsConfigBuilder {
    /// Finalize the builder and construct an [`IntrospectionLogs`].
    pub fn build(self) -> Result<IntrospectionLogs> {
        IntrospectionLogs::from_config(self.build_config()?)
    }
}

impl IntrospectionLogs {
    /// Start building an [`IntrospectionLogs`] instance.
    pub fn builder() -> IntrospectionLogsConfigBuilder {
        IntrospectionLogsConfigBuilder::default()
    }

    /// Construct from a fully-resolved [`IntrospectionLogsConfig`].
    /// Most callers should prefer [`IntrospectionLogs::builder`].
    pub fn from_config(config: IntrospectionLogsConfig) -> Result<Self> {
        let token = config
            .token
            .or_else(|| env::var("INTROSPECTION_TOKEN").ok())
            .unwrap_or_default();

        let service_name = config
            .service_name
            .or_else(|| env::var("INTROSPECTION_SERVICE_NAME").ok())
            .unwrap_or_else(|| crate::types::defaults::SERVICE_NAME.to_string());

        let base_otel_url = config
            .base_otel_url
            .or_else(|| env::var("INTROSPECTION_BASE_OTEL_URL").ok())
            .unwrap_or_else(|| types::defaults::BASE_OTEL_URL.to_string());

        if token.is_empty() {
            warn!("IntrospectionLogs: No token provided. Events will not be sent.");
        }

        // Construct endpoint URL
        let endpoint = if base_otel_url.ends_with(types::api_path::LOGS) {
            base_otel_url
        } else {
            format!(
                "{}{}",
                base_otel_url.trim_end_matches('/'),
                types::api_path::LOGS
            )
        };

        info!(
            "IntrospectionLogs initialized: service={}, endpoint={}",
            service_name, endpoint
        );

        // Use custom exporter if provided, otherwise create default OTLP exporter
        let exporter = if let Some(custom_exporter_arc) = config.log_exporter {
            Arc::try_unwrap(custom_exporter_arc).map_err(|_| {
                IntrospectionLogsError::OpenTelemetry(
                    "Custom log exporter has multiple references".to_string(),
                )
            })?
        } else {
            let mut headers =
                HashMap::from([("Authorization".to_string(), format!("Bearer {}", token))]);
            if let Some(additional_headers) = &config.additional_headers {
                headers.extend(additional_headers.clone());
            }

            LogExporter::builder()
                .with_http()
                .with_endpoint(&endpoint)
                .with_headers(headers)
                .with_timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| IntrospectionLogsError::OpenTelemetry(e.to_string()))?
        };

        let resource = Resource::builder()
            .with_service_name(service_name.clone())
            .build();

        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(exporter)
            .build();

        let logger = logger_provider.logger(types::logger_name::RUST_SDK);

        Ok(Self {
            logger_provider,
            logger,
            service_name,
            traits: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get identity context from OpenTelemetry baggage.
    fn get_identity_from_context(cx: &Context) -> (Option<String>, Option<String>) {
        let baggage = cx.baggage();
        let user_id = baggage.get(types::baggage::USER_ID).map(|v| v.to_string());
        let anonymous_id = baggage
            .get(types::baggage::ANONYMOUS_ID)
            .map(|v| v.to_string());
        (user_id, anonymous_id)
    }

    /// Get gen_ai context from OpenTelemetry baggage.
    fn get_gen_ai_from_context(
        cx: &Context,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        let baggage = cx.baggage();
        let conversation_id = baggage
            .get(types::baggage::CONVERSATION_ID)
            .map(|v| v.to_string());
        let previous_response_id = baggage
            .get(types::baggage::PREVIOUS_RESPONSE_ID)
            .map(|v| v.to_string());
        let agent_name = baggage
            .get(types::baggage::AGENT_NAME)
            .map(|v| v.to_string());
        let agent_id = baggage.get(types::baggage::AGENT_ID).map(|v| v.to_string());
        (conversation_id, previous_response_id, agent_name, agent_id)
    }

    /// Build attributes for a log record.
    fn build_attributes(
        &self,
        event_name: &str,
        properties: Option<&HashMap<String, PropertyValue>>,
        traits: Option<&HashMap<String, PropertyValue>>,
        conversation_id: Option<&str>,
        previous_response_id: Option<&str>,
        event_id: Option<&str>,
    ) -> Vec<(Key, String)> {
        let cx = Context::current();
        let (user_id, anonymous_id) = Self::get_identity_from_context(&cx);
        let (ctx_conversation_id, ctx_previous_response_id, agent_name, agent_id) =
            Self::get_gen_ai_from_context(&cx);

        let mut attributes: Vec<(Key, String)> = vec![
            (Key::new(types::attr::EVENT_NAME), event_name.to_string()),
            (
                Key::new(types::attr::EVENT_ID),
                event_id
                    .map(|s| s.to_string())
                    .unwrap_or_else(generate_event_id),
            ),
        ];

        if let Some(uid) = user_id {
            attributes.push((Key::new(types::attr::USER_ID), uid));
        }
        if let Some(aid) = anonymous_id {
            attributes.push((Key::new(types::attr::ANONYMOUS_ID), aid));
        }

        let final_conversation_id = conversation_id
            .map(|s| s.to_string())
            .or(ctx_conversation_id);
        let final_previous_response_id = previous_response_id
            .map(|s| s.to_string())
            .or(ctx_previous_response_id);

        if let Some(conv_id) = final_conversation_id {
            attributes.push((Key::new(types::attr::CONVERSATION_ID), conv_id));
        }
        if let Some(resp_id) = final_previous_response_id {
            attributes.push((Key::new(types::attr::PREVIOUS_RESPONSE_ID), resp_id));
        }
        if let Some(name) = agent_name {
            attributes.push((Key::new(types::attr::AGENT_NAME), name));
        }
        if let Some(id) = agent_id {
            attributes.push((Key::new(types::attr::AGENT_ID), id));
        }

        if let Some(props) = properties {
            for (key, value) in props {
                attributes.push((
                    Key::new(format!("{}{}", types::attr::PROPERTIES_PREFIX, key)),
                    value.to_otel_string(),
                ));
            }
        }

        if let Some(t) = traits {
            for (key, value) in t {
                attributes.push((
                    Key::new(format!("{}{}", types::attr::TRAITS_PREFIX, key)),
                    value.to_otel_string(),
                ));
            }
        }

        attributes
    }

    /// Emit a log record via OpenTelemetry.
    fn emit(&self, attributes: Vec<(Key, String)>) {
        let mut record = self.logger.create_log_record();
        record.set_timestamp(SystemTime::now());
        record.set_severity_number(Severity::Info);
        record.set_severity_text(types::severity::INFO);
        for (key, value) in attributes {
            record.add_attribute(key, value);
        }
        self.logger.emit(record);
    }

    /// Track a custom event.
    pub fn track(&self, event_name: &str, options: Option<TrackOptions>) {
        let opts = options.unwrap_or_default();
        let attributes = self.build_attributes(
            event_name,
            Some(&opts.properties),
            None,
            None,
            None,
            opts.event_id.as_deref(),
        );
        self.emit(attributes);
        debug!("Tracked: {}", event_name);
    }

    /// Track feedback on a message or response.
    pub fn feedback(&self, name: &str, options: FeedbackOptions) {
        let mut properties: HashMap<String, PropertyValue> = HashMap::new();
        properties.insert("name".to_string(), PropertyValue::String(name.to_string()));
        if let Some(comments) = &options.comments {
            properties.insert(
                "comments".to_string(),
                PropertyValue::String(comments.clone()),
            );
        }
        properties.extend(options.extra.clone());

        let attributes = self.build_attributes(
            types::event_name::FEEDBACK,
            Some(&properties),
            None,
            options.conversation_id.as_deref(),
            options.previous_response_id.as_deref(),
            options.event_id.as_deref(),
        );
        self.emit(attributes);
        debug!("Feedback: {}", name);
    }

    /// Identify a user and emit an identify event with their traits.
    pub fn identify(&self, user_id: &str, options: Option<IdentifyOptions>) {
        let opts = options.unwrap_or_default();

        if !opts.traits.is_empty() {
            if let Ok(mut traits) = self.traits.try_write() {
                traits.extend(opts.traits.clone());
            }
        }

        let attributes = self.build_attributes(
            types::event_name::IDENTIFY,
            None,
            Some(&opts.traits),
            None,
            None,
            opts.event_id.as_deref(),
        );
        self.emit(attributes);
        debug!("Identified: {}", user_id);
    }

    // ========================================================================
    // Baggage Context Management
    // ========================================================================

    /// Set user ID in OpenTelemetry baggage.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_user_id(&self, user_id: &str) -> BaggageGuard {
        BaggageGuard::new(types::baggage::USER_ID, user_id)
    }

    /// Set anonymous ID in OpenTelemetry baggage.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_anonymous_id(&self, anonymous_id: &str) -> BaggageGuard {
        BaggageGuard::new(types::baggage::ANONYMOUS_ID, anonymous_id)
    }

    /// Set conversation ID in OpenTelemetry baggage.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_conversation_id(&self, conversation_id: &str) -> BaggageGuard {
        BaggageGuard::new(types::baggage::CONVERSATION_ID, conversation_id)
    }

    /// Set previous response ID in OpenTelemetry baggage.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_previous_response_id(&self, previous_response_id: &str) -> BaggageGuard {
        BaggageGuard::new(types::baggage::PREVIOUS_RESPONSE_ID, previous_response_id)
    }

    /// Set agent context in OpenTelemetry baggage.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_agent(&self, agent_name: &str, agent_id: Option<&str>) -> BaggageGuard {
        let mut guard = BaggageGuard::new(types::baggage::AGENT_NAME, agent_name);
        if let Some(id) = agent_id {
            guard = guard.with_additional(types::baggage::AGENT_ID, id);
        }
        guard
    }

    /// Set multiple baggage values at once.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_baggage(&self, values: &[(&str, &str)]) -> BaggageGuard {
        BaggageGuard::new_multi(values)
    }

    /// Get the current user ID from baggage.
    pub fn get_user_id(&self) -> Option<String> {
        let cx = Context::current();
        cx.baggage()
            .get(types::baggage::USER_ID)
            .map(|v| v.to_string())
    }

    /// Get the current anonymous ID from baggage.
    pub fn get_anonymous_id(&self) -> Option<String> {
        let cx = Context::current();
        cx.baggage()
            .get(types::baggage::ANONYMOUS_ID)
            .map(|v| v.to_string())
    }

    /// Flush all pending events.
    pub fn flush(&self) -> Result<()> {
        self.logger_provider
            .force_flush()
            .map_err(|e| IntrospectionLogsError::OpenTelemetry(e.to_string()))
    }

    /// Shutdown the logger gracefully, flushing pending events.
    pub fn shutdown(self) -> Result<()> {
        info!("Shutting down IntrospectionLogs");
        self.logger_provider
            .shutdown()
            .map_err(|e| IntrospectionLogsError::OpenTelemetry(e.to_string()))
    }
}

/// Guard that manages OpenTelemetry baggage context.
///
/// When dropped, the context is restored to its previous state.
pub struct BaggageGuard {
    _context_guard: opentelemetry::ContextGuard,
}

impl BaggageGuard {
    /// Create a new baggage guard with a single key-value pair.
    /// Merges with existing baggage instead of replacing it.
    fn new(key: &str, value: &str) -> Self {
        let current_cx = Context::current();
        let current_baggage = current_cx.baggage();

        let mut kvs: Vec<KeyValue> = current_baggage
            .iter()
            .map(|(k, (v, _))| KeyValue::new(k.as_str().to_string(), v.to_string()))
            .collect();

        kvs.push(KeyValue::new(key.to_string(), value.to_string()));

        let cx = current_cx.with_baggage(kvs);
        let guard = cx.attach();
        Self {
            _context_guard: guard,
        }
    }

    /// Create a new baggage guard with multiple key-value pairs.
    /// Merges with existing baggage instead of replacing it.
    fn new_multi(values: &[(&str, &str)]) -> Self {
        let current_cx = Context::current();
        let current_baggage = current_cx.baggage();

        let mut kvs: Vec<KeyValue> = current_baggage
            .iter()
            .map(|(k, (v, _))| KeyValue::new(k.as_str().to_string(), v.to_string()))
            .collect();

        for (k, v) in values {
            kvs.push(KeyValue::new(k.to_string(), v.to_string()));
        }

        let cx = current_cx.with_baggage(kvs);
        let guard = cx.attach();
        Self {
            _context_guard: guard,
        }
    }

    /// Add additional baggage to this guard.
    /// Merges with existing baggage instead of replacing it.
    fn with_additional(self, key: &str, value: &str) -> Self {
        drop(self);
        Self::new(key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logs_creation() {
        let logs = IntrospectionLogs::builder()
            .token("test-token")
            .service_name("test-service")
            .base_otel_url("http://localhost:4318")
            .build()
            .unwrap();
        assert_eq!(logs.service_name, "test-service");
    }

    #[test]
    fn test_baggage_guard_sets_context() {
        let logs = IntrospectionLogs::builder()
            .token("test-token")
            .build()
            .unwrap();

        assert_eq!(logs.get_user_id(), None);

        {
            let _guard = logs.set_user_id("user_123");
            assert_eq!(logs.get_user_id(), Some("user_123".to_string()));
        }

        assert_eq!(logs.get_user_id(), None);
    }

    #[test]
    fn test_nested_baggage_guards() {
        let logs = IntrospectionLogs::builder()
            .token("test-token")
            .build()
            .unwrap();

        {
            let _user_guard = logs.set_user_id("user_123");
            assert_eq!(logs.get_user_id(), Some("user_123".to_string()));

            {
                let _conv_guard = logs.set_conversation_id("conv_456");
                assert_eq!(logs.get_user_id(), Some("user_123".to_string()));
                let cx = Context::current();
                assert_eq!(
                    cx.baggage()
                        .get(types::baggage::CONVERSATION_ID)
                        .map(|v| v.to_string()),
                    Some("conv_456".to_string())
                );
            }

            assert_eq!(logs.get_user_id(), Some("user_123".to_string()));
        }

        assert_eq!(logs.get_user_id(), None);
    }
}
