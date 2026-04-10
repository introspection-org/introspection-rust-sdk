//! Introspection Client implementation.
//!
//! Provides an API for tracking events and feedback using OTLP Logs.
//! Uses native OpenTelemetry libraries for batching, export, and baggage propagation.

use crate::types::{
    self, generate_event_id, ClientConfig, FeedbackOptions, IdentifyOptions, PropertyValue,
    TrackOptions,
};

#[cfg(test)]
use crate::types::AdvancedOptions;
use opentelemetry::logs::{LogRecord as _, Logger, LoggerProvider, Severity};
use opentelemetry::{baggage::BaggageExt, Context, Key, KeyValue};
use opentelemetry_otlp::{LogExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{logs::SdkLoggerProvider, Resource};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// SDK version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Errors that can occur in the Introspection client.
#[derive(Error, Debug)]
pub enum IntrospectionError {
    #[error("OpenTelemetry error: {0}")]
    OpenTelemetry(String),

    #[error("Client not initialized")]
    NotInitialized,

    #[error("Client already shut down")]
    AlreadyShutdown,
}

impl From<opentelemetry_sdk::error::OTelSdkError> for IntrospectionError {
    fn from(e: opentelemetry_sdk::error::OTelSdkError) -> Self {
        IntrospectionError::OpenTelemetry(e.to_string())
    }
}

/// Result type for Introspection operations.
pub type Result<T> = std::result::Result<T, IntrospectionError>;

/// Introspection client for tracking events and feedback.
///
/// Uses OpenTelemetry's native LoggerProvider with BatchLogProcessor and OTLP export.
/// Context is managed via OpenTelemetry baggage for distributed propagation.
///
/// # Example
///
/// ```rust,no_run
/// use introspection_sdk::{IntrospectionClient, ClientConfig, FeedbackOptions};
///
/// let client = IntrospectionClient::new(
///     ClientConfig::with_token("your-token")
/// ).unwrap();
///
/// // Track an event
/// client.track("Button Clicked", None);
///
/// // Track feedback with baggage context
/// {
///     let _guard = client.set_user_id("user_123");
///     let _conv_guard = client.set_conversation_id("conv_456");
///     client.feedback("thumbs_up", FeedbackOptions::new());
/// } // Context automatically cleared on drop
///
/// // Shutdown gracefully
/// client.shutdown().unwrap();
/// ```
pub struct IntrospectionClient {
    /// OpenTelemetry logger provider
    logger_provider: SdkLoggerProvider,

    /// OpenTelemetry logger
    logger: opentelemetry_sdk::logs::SdkLogger,

    /// Service name (used in tests)
    #[allow(dead_code)]
    service_name: String,

    /// User traits from identify calls (stored locally)
    #[allow(dead_code)]
    traits: Arc<RwLock<HashMap<String, PropertyValue>>>,
}

impl IntrospectionClient {
    /// Create a new Introspection client.
    ///
    /// Sets up OpenTelemetry LoggerProvider with BatchLogProcessor and OTLP HTTP export.
    ///
    /// # Arguments
    ///
    /// * `config` - Client configuration options
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient};
    ///
    /// // With explicit config
    /// let client = IntrospectionClient::new(
    ///     ClientConfig::builder()
    ///         .token("your-token")
    ///         .service_name("my-service")
    ///         .build()
    ///         .unwrap()
    /// ).unwrap();
    ///
    /// // With advanced options (custom base URL and headers)
    /// let client = IntrospectionClient::new(
    ///     ClientConfig::builder()
    ///         .token("your-token")
    ///         .advanced(AdvancedOptions {
    ///             base_url: Some("http://localhost:8080".to_string()),
    ///             additional_headers: Some([("X-Custom-Header".to_string(), "value".to_string())].into_iter().collect()),
    ///             flush_interval_ms: Some(1000),
    ///             max_batch_size: Some(50),
    ///             ..Default::default()
    ///         })
    ///         .build()
    ///         .unwrap()
    /// ).unwrap();
    ///
    /// // Or use environment variables
    /// let client = IntrospectionClient::new(ClientConfig::default()).unwrap();
    /// ```
    pub fn new(config: ClientConfig) -> Result<Self> {
        let token = config
            .token
            .clone()
            .or_else(|| env::var("INTROSPECTION_TOKEN").ok())
            .unwrap_or_default();

        let service_name = config
            .service_name
            .clone()
            .or_else(|| env::var("INTROSPECTION_SERVICE_NAME").ok())
            .unwrap_or_else(|| types::defaults::SERVICE_NAME.to_string());

        // Get advanced options if provided
        let advanced = config.advanced.unwrap_or_default();

        // Use advanced.base_url if provided, otherwise use env var or default
        let base_url = advanced
            .base_url
            .or_else(|| env::var("INTROSPECTION_BASE_URL").ok())
            .unwrap_or_else(|| types::defaults::BASE_URL.to_string());

        if token.is_empty() {
            warn!("IntrospectionClient: No token provided. Events will not be sent.");
        }

        // Construct endpoint URL
        let endpoint = if base_url.ends_with(types::api_path::LOGS) {
            base_url
        } else {
            format!(
                "{}{}",
                base_url.trim_end_matches('/'),
                types::api_path::LOGS
            )
        };

        info!(
            "IntrospectionClient initialized: service={}, endpoint={}",
            service_name, endpoint
        );

        // Use custom exporter if provided, otherwise create default OTLP exporter
        let exporter = if let Some(custom_exporter_arc) = advanced.log_exporter {
            // Extract LogExporter from Arc - this will fail if there are multiple references
            // For testing purposes, this should be fine as the exporter is typically only
            // referenced once. If this becomes an issue, we may need to clone the exporter
            // or use a different approach.
            Arc::try_unwrap(custom_exporter_arc).map_err(|_| {
                IntrospectionError::OpenTelemetry(
                    "Custom log exporter has multiple references".to_string(),
                )
            })?
        } else {
            // Build headers - start with Authorization, then merge additional headers
            let mut headers =
                HashMap::from([("Authorization".to_string(), format!("Bearer {}", token))]);
            if let Some(additional_headers) = advanced.additional_headers {
                headers.extend(additional_headers);
            }

            // Create OTLP log exporter
            LogExporter::builder()
                .with_http()
                .with_endpoint(&endpoint)
                .with_headers(headers)
                .with_timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| IntrospectionError::OpenTelemetry(e.to_string()))?
        };

        // Note: flush_interval_ms and max_batch_size from advanced options are not
        // directly configurable with with_batch_exporter(). The OpenTelemetry SDK
        // uses default batch settings. To customize these, you would need to use
        // with_log_processor() with a custom BatchLogProcessor, which is more complex.
        // For now, these options are accepted but use SDK defaults.

        // Create resource with service name
        let resource = Resource::builder()
            .with_service_name(service_name.clone())
            .build();

        // Create logger provider with batch processor
        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(exporter)
            .build();

        // Get logger with instrumentation scope name
        // Note: Version is set via the instrumentation scope when creating the logger
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

        // Identity from baggage
        if let Some(uid) = user_id {
            attributes.push((Key::new(types::attr::USER_ID), uid));
        }
        if let Some(aid) = anonymous_id {
            attributes.push((Key::new(types::attr::ANONYMOUS_ID), aid));
        }

        // Gen AI context (explicit params override baggage)
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

        // Properties (with "properties." prefix)
        if let Some(props) = properties {
            for (key, value) in props {
                attributes.push((
                    Key::new(format!("{}{}", types::attr::PROPERTIES_PREFIX, key)),
                    value.to_otel_string(),
                ));
            }
        }

        // Traits (with "context.traits." prefix)
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

    /// Emit a log record using OpenTelemetry.
    /// Timestamp is set explicitly using SystemTime::now() for nanosecond precision.
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
    ///
    /// # Arguments
    ///
    /// * `event_name` - Name of the event (e.g., "Button Clicked")
    /// * `options` - Optional track options with properties
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use introspection_sdk::{IntrospectionClient, ClientConfig, TrackOptions};
    ///
    /// let client = IntrospectionClient::new(ClientConfig::default()).unwrap();
    ///
    /// // Simple track
    /// client.track("Page View", None);
    ///
    /// // With properties
    /// client.track(
    ///     "Button Clicked",
    ///     Some(TrackOptions::new()
    ///         .with_property("button_id", "submit")
    ///         .with_property("page", "checkout")),
    /// );
    /// ```
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
    ///
    /// # Arguments
    ///
    /// * `name` - Feedback name/action (e.g., "thumbs_up", "thumbs_down")
    /// * `options` - Feedback options with comments, conversation context, etc.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use introspection_sdk::{IntrospectionClient, ClientConfig, FeedbackOptions};
    ///
    /// let client = IntrospectionClient::new(ClientConfig::default()).unwrap();
    ///
    /// // Simple feedback
    /// client.feedback("thumbs_up", FeedbackOptions::new());
    ///
    /// // With options
    /// client.feedback(
    ///     "thumbs_down",
    ///     FeedbackOptions::new()
    ///         .with_comments("Answer was off topic")
    ///         .with_conversation_id("conv_123")
    ///         .with_previous_response_id("msg_456"),
    /// );
    /// ```
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

    /// Identify a user and their traits.
    ///
    /// This emits an identify event. Use `set_user_id()` to set baggage context.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user's unique identifier
    /// * `options` - Optional identify options with traits and anonymous_id
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use introspection_sdk::{IntrospectionClient, ClientConfig, IdentifyOptions};
    ///
    /// let client = IntrospectionClient::new(ClientConfig::default()).unwrap();
    ///
    /// // Identify with baggage context
    /// {
    ///     let _guard = client.set_user_id("user_123");
    ///     client.identify(
    ///         "user_123",
    ///         Some(IdentifyOptions::new()
    ///             .with_trait("email", "user@example.com")
    ///             .with_trait("plan", "pro")),
    ///     );
    /// }
    /// ```
    pub fn identify(&self, user_id: &str, options: Option<IdentifyOptions>) {
        let opts = options.unwrap_or_default();

        // Store traits locally
        if !opts.traits.is_empty() {
            if let Ok(mut traits) = self.traits.try_write() {
                traits.extend(opts.traits.clone());
            }
        }

        // Build and emit identify event
        // Note: The user_id should be set via baggage using set_user_id()
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
    // These methods return guards that automatically clean up context on drop
    // ========================================================================

    /// Set user ID in OpenTelemetry baggage.
    ///
    /// Returns a guard that clears the baggage when dropped.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use introspection_sdk::{IntrospectionClient, ClientConfig};
    ///
    /// let client = IntrospectionClient::new(ClientConfig::default()).unwrap();
    ///
    /// {
    ///     let _guard = client.set_user_id("user_123");
    ///     // All events here will have user_id in baggage
    ///     client.track("Event", None);
    /// } // Baggage automatically cleared
    /// ```
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_user_id(&self, user_id: &str) -> BaggageGuard {
        BaggageGuard::new(types::baggage::USER_ID, user_id)
    }

    /// Set anonymous ID in OpenTelemetry baggage.
    ///
    /// Returns a guard that clears the baggage when dropped.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_anonymous_id(&self, anonymous_id: &str) -> BaggageGuard {
        BaggageGuard::new(types::baggage::ANONYMOUS_ID, anonymous_id)
    }

    /// Set conversation ID in OpenTelemetry baggage.
    ///
    /// Returns a guard that clears the baggage when dropped.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_conversation_id(&self, conversation_id: &str) -> BaggageGuard {
        BaggageGuard::new(types::baggage::CONVERSATION_ID, conversation_id)
    }

    /// Set previous response ID in OpenTelemetry baggage.
    ///
    /// Returns a guard that clears the baggage when dropped.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_previous_response_id(&self, previous_response_id: &str) -> BaggageGuard {
        BaggageGuard::new(types::baggage::PREVIOUS_RESPONSE_ID, previous_response_id)
    }

    /// Set agent context in OpenTelemetry baggage.
    ///
    /// Returns a guard that clears the baggage when dropped.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_agent(&self, agent_name: &str, agent_id: Option<&str>) -> BaggageGuard {
        let mut guard = BaggageGuard::new(types::baggage::AGENT_NAME, agent_name);
        if let Some(id) = agent_id {
            guard = guard.with_additional(types::baggage::AGENT_ID, id);
        }
        guard
    }

    /// Set multiple baggage values at once.
    ///
    /// Returns a guard that clears all baggage when dropped.
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
            .map_err(|e| IntrospectionError::OpenTelemetry(e.to_string()))
    }

    /// Shutdown the client gracefully.
    ///
    /// This flushes all pending events and stops the background processor.
    pub fn shutdown(self) -> Result<()> {
        info!("Shutting down IntrospectionClient");
        self.logger_provider
            .shutdown()
            .map_err(|e| IntrospectionError::OpenTelemetry(e.to_string()))
    }
}

/// Guard that manages OpenTelemetry baggage context.
///
/// When dropped, the context is restored to its previous state.
/// This mirrors Python's context manager behavior.
pub struct BaggageGuard {
    _context_guard: opentelemetry::ContextGuard,
}

impl BaggageGuard {
    /// Create a new baggage guard with a single key-value pair.
    /// Merges with existing baggage instead of replacing it.
    fn new(key: &str, value: &str) -> Self {
        let current_cx = Context::current();
        let current_baggage = current_cx.baggage();

        // Collect all existing baggage items
        // The iterator returns (Key, (StringValue, BaggageMetadata))
        let mut kvs: Vec<KeyValue> = current_baggage
            .iter()
            .map(|(k, (v, _))| KeyValue::new(k.as_str().to_string(), v.to_string()))
            .collect();

        // Add the new key-value pair (will overwrite if key already exists)
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

        // Collect all existing baggage items
        // The iterator returns (Key, (StringValue, BaggageMetadata))
        let mut kvs: Vec<KeyValue> = current_baggage
            .iter()
            .map(|(k, (v, _))| KeyValue::new(k.as_str().to_string(), v.to_string()))
            .collect();

        // Add the new key-value pairs (will overwrite if keys already exist)
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
        // Drop the old guard and create a new one with the additional baggage
        drop(self);
        Self::new(key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let config = ClientConfig::builder()
            .token("test-token")
            .service_name("test-service")
            .advanced(AdvancedOptions {
                base_url: Some("http://localhost:4318".to_string()),
                ..Default::default()
            })
            .build()
            .unwrap();

        let client = IntrospectionClient::new(config).unwrap();
        assert_eq!(client.service_name, "test-service");
    }

    #[test]
    fn test_baggage_guard_sets_context() {
        let config = ClientConfig::builder().token("test-token").build().unwrap();

        let client = IntrospectionClient::new(config).unwrap();

        // Before setting baggage
        assert_eq!(client.get_user_id(), None);

        {
            let _guard = client.set_user_id("user_123");
            // Inside guard scope
            assert_eq!(client.get_user_id(), Some("user_123".to_string()));
        }

        // After guard is dropped
        assert_eq!(client.get_user_id(), None);
    }

    #[test]
    fn test_nested_baggage_guards() {
        let config = ClientConfig::builder().token("test-token").build().unwrap();

        let client = IntrospectionClient::new(config).unwrap();

        {
            let _user_guard = client.set_user_id("user_123");
            assert_eq!(client.get_user_id(), Some("user_123".to_string()));

            {
                let _conv_guard = client.set_conversation_id("conv_456");
                // Both should be set
                assert_eq!(client.get_user_id(), Some("user_123".to_string()));
                let cx = Context::current();
                assert_eq!(
                    cx.baggage()
                        .get(types::baggage::CONVERSATION_ID)
                        .map(|v| v.to_string()),
                    Some("conv_456".to_string())
                );
            }

            // Conversation should be cleared, user still set
            assert_eq!(client.get_user_id(), Some("user_123".to_string()));
        }

        // Both should be cleared
        assert_eq!(client.get_user_id(), None);
    }
}
