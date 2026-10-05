//! [`IntrospectionLogs`] — independent OTLP Logs exporter for the
//! Introspection SDK.
//!
//! Owns its own [`SdkLoggerProvider`] and exposes the
//! `log_event(...)` / `track(...)` / `feedback(...)` / `identify(...)`
//! analytics surface
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

use opentelemetry::logs::{LogRecord as _, Logger, LoggerProvider};
use opentelemetry::{baggage::BaggageExt, Context, InstrumentationScope, Key, KeyValue};
use opentelemetry_otlp::{LogExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{logs::SdkLoggerProvider, Resource};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::otel::types::{
    self, generate_event_id, reserved_event_name_prefix, FeedbackOptions, IdentifyOptions,
    LogEventError, LogEventIdentity, LogEventOptions, LogEventSeverity, PropertyValue,
    TrackOptions,
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

/// The instrumentation scope stamped on every emitted record.
///
/// The version matters as much as the name: it is how a record is traced back
/// to the release that produced it. `logger(name)` leaves it unset, so every
/// event this crate sent was unattributable to a version.
fn sdk_scope() -> InstrumentationScope {
    InstrumentationScope::builder(types::logger_name::SDK)
        .with_version(crate::VERSION)
        .build()
}

/// Independent OTLP Logs exporter — owns its own [`SdkLoggerProvider`]
/// and emits `log_event` / `track` / `feedback` / `identify` events with
/// OpenTelemetry baggage-managed context.
///
/// Construct via [`IntrospectionLogs::builder`].
pub struct IntrospectionLogs {
    /// OpenTelemetry logger provider
    logger_provider: SdkLoggerProvider,

    /// OpenTelemetry logger
    logger: opentelemetry_sdk::logs::SdkLogger,
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

    /// OTLP batch flush interval in milliseconds. `None` uses the
    /// OpenTelemetry default.
    pub flush_interval_ms: Option<u64>,

    /// OTLP max export batch size. `None` uses the OpenTelemetry default.
    pub max_batch_size: Option<usize>,

    /// Maximum records buffered before new ones are dropped. `None` uses the
    /// OpenTelemetry default (2048). Bounds the queue, not one export — see
    /// [`crate::otel::SpanProcessorAdvancedOptions::max_queue_size`].
    ///
    pub max_queue_size: Option<usize>,

    /// Deadline for one OTLP export, in milliseconds. `None` uses
    /// [`crate::otel::types::defaults::EXPORT_TIMEOUT_MS`] (30000).
    ///
    /// Applied to the exporter's HTTP request, which is the only place a
    /// per-export deadline takes effect. `BatchConfig::max_export_timeout` is
    /// dead code in `opentelemetry_sdk` 0.32 unless the experimental
    /// async-runtime processor is enabled — it is populated from
    /// `OTEL_BLRP_EXPORT_TIMEOUT` and then never read — so this option, not
    /// that env var, is what bounds an export.
    ///
    /// Ignored when a caller supplies their own `log_exporter`.
    pub export_timeout_ms: Option<u64>,
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
            let mut headers = HashMap::from([
                (
                    "User-Agent".to_string(),
                    format!("introspection-sdk/{}", crate::VERSION),
                ),
                ("Authorization".to_string(), format!("Bearer {}", token)),
            ]);
            if let Some(additional_headers) = &config.additional_headers {
                headers.extend(additional_headers.clone());
            }

            LogExporter::builder()
                .with_http()
                .with_endpoint(&endpoint)
                .with_headers(headers)
                .with_timeout(Duration::from_millis(
                    config
                        .export_timeout_ms
                        .unwrap_or(types::defaults::EXPORT_TIMEOUT_MS),
                ))
                .build()
                .map_err(|e| IntrospectionLogsError::OpenTelemetry(e.to_string()))?
        };

        let resource = Resource::builder()
            .with_service_name(service_name.clone())
            .build();

        // Defaulted rather than left to the OTel defaults (1000ms / 512), so
        // analytics events leave a Rust process on the same cadence as every
        // other SDK and as this crate's own span processor.
        let mut batch_config = opentelemetry_sdk::logs::BatchConfigBuilder::default()
            .with_scheduled_delay(Duration::from_millis(
                config
                    .flush_interval_ms
                    .unwrap_or(types::defaults::FLUSH_INTERVAL_MS),
            ))
            .with_max_export_batch_size(
                config
                    .max_batch_size
                    .unwrap_or(types::defaults::MAX_BATCH_SIZE),
            );
        // Applied only when set, so an unset option leaves the OpenTelemetry
        // default in charge. The batch size and flush interval above are
        // defaulted deliberately; this one has no Introspection-specific
        // value to prefer, so the OpenTelemetry default is the right one.
        if let Some(queue_size) = config.max_queue_size {
            batch_config = batch_config.with_max_queue_size(queue_size);
        }
        let processor = opentelemetry_sdk::logs::BatchLogProcessor::builder(exporter)
            .with_batch_config(batch_config.build())
            .build();

        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_log_processor(processor)
            .build();

        let logger = logger_provider.logger_with_scope(sdk_scope());

        Ok(Self {
            logger_provider,
            logger,
        })
    }

    /// Build an instance around an arbitrary exporter.
    ///
    /// The public `log_exporter` option is the concrete OTLP type, so an
    /// in-memory exporter cannot be supplied through it; without this the
    /// emitted records could not be asserted on at all.
    #[cfg(test)]
    fn with_exporter(
        exporter: impl opentelemetry_sdk::logs::LogExporter + 'static,
        service_name: &str,
    ) -> Self {
        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(
                Resource::builder()
                    .with_service_name(service_name.to_string())
                    .build(),
            )
            .with_simple_exporter(exporter)
            .build();
        let logger = logger_provider.logger_with_scope(sdk_scope());
        Self {
            logger_provider,
            logger,
        }
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
    #[allow(clippy::too_many_arguments)]
    fn build_attributes(
        &self,
        event_name: &str,
        properties: Option<&HashMap<String, PropertyValue>>,
        traits: Option<&HashMap<String, PropertyValue>>,
        conversation_id: Option<&str>,
        previous_response_id: Option<&str>,
        event_id: Option<&str>,
        identity: Option<&LogEventIdentity>,
    ) -> Vec<(Key, opentelemetry::logs::AnyValue)> {
        let cx = Context::current();
        let (ctx_user_id, ctx_anonymous_id) = Self::get_identity_from_context(&cx);
        let user_id = identity.and_then(|i| i.user_id.clone()).or(ctx_user_id);
        let anonymous_id = identity
            .and_then(|i| i.anonymous_id.clone())
            .or(ctx_anonymous_id);
        let (ctx_conversation_id, ctx_previous_response_id, agent_name, agent_id) =
            Self::get_gen_ai_from_context(&cx);

        let mut attributes: Vec<(Key, opentelemetry::logs::AnyValue)> = vec![
            (
                Key::new(types::attr::EVENT_NAME),
                event_name.to_string().into(),
            ),
            (
                Key::new(types::attr::EVENT_ID),
                event_id
                    .map(|s| s.to_string())
                    .unwrap_or_else(generate_event_id)
                    .into(),
            ),
        ];

        if let Some(uid) = user_id {
            attributes.push((Key::new(types::attr::USER_ID), uid.into()));
        }
        if let Some(aid) = anonymous_id {
            attributes.push((Key::new(types::attr::ANONYMOUS_ID), aid.into()));
        }

        let final_conversation_id = conversation_id
            .map(|s| s.to_string())
            .or(ctx_conversation_id);
        let final_previous_response_id = previous_response_id
            .map(|s| s.to_string())
            .or(ctx_previous_response_id);

        if let Some(conv_id) = final_conversation_id {
            attributes.push((Key::new(types::attr::CONVERSATION_ID), conv_id.into()));
        }
        if let Some(resp_id) = final_previous_response_id {
            attributes.push((Key::new(types::attr::PREVIOUS_RESPONSE_ID), resp_id.into()));
        }
        if let Some(name) = agent_name {
            attributes.push((Key::new(types::attr::AGENT_NAME), name.into()));
        }
        if let Some(id) = agent_id {
            attributes.push((Key::new(types::attr::AGENT_ID), id.into()));
        }

        // A null value is an absent value: the key is omitted rather than
        // shipped as the string "null". `PropertyValue::Json` is reachable
        // with `serde_json::Value::Null` inside it, and stringifying that
        // produced a `properties.x = "null"` attribute that reads as the
        // four-character string on the way back out.
        for (prefix, source) in [
            (types::attr::PROPERTIES_PREFIX, properties),
            (types::attr::TRAITS_PREFIX, traits),
        ] {
            let Some(map) = source else { continue };
            for (key, value) in map {
                if value.is_null() {
                    continue;
                }
                attributes.push((Key::new(format!("{prefix}{key}")), value.to_otel_value()));
            }
        }

        attributes
    }

    /// Emit an INFO log record stamped now.
    fn emit(&self, attributes: Vec<(Key, opentelemetry::logs::AnyValue)>) {
        self.emit_at(attributes, SystemTime::now(), LogEventSeverity::Info);
    }

    /// Emit a log record via OpenTelemetry.
    fn emit_at(
        &self,
        attributes: Vec<(Key, opentelemetry::logs::AnyValue)>,
        timestamp: SystemTime,
        severity: LogEventSeverity,
    ) {
        let mut record = self.logger.create_log_record();
        record.set_timestamp(timestamp);
        record.set_severity_number(severity.to_otel());
        record.set_severity_text(severity.as_str());
        for (key, value) in attributes {
            record.add_attribute(key, value);
        }
        self.logger.emit(record);
    }

    /// Log an app event under any custom name, e.g. `"ark.feed.entry"`.
    ///
    /// `attributes` land under `properties.*`, which is where the platform's
    /// `introspection.track` read projection finds them; read the events back
    /// from `runner.events()` with `event_name=introspection.track`. Identity
    /// and gen_ai context are taken from the active baggage unless
    /// [`LogEventOptions::identity`] overrides them, field by field. A
    /// property whose value is JSON `null` is omitted.
    ///
    /// Pass a stable [`LogEventOptions::event_id`] when delivery may repeat:
    /// consumers dedupe on it, so re-sending the same id is safe.
    ///
    /// # Errors
    ///
    /// [`LogEventError::EmptyName`] for `""`, and
    /// [`LogEventError::ReservedName`] for a name under one of
    /// [`types::RESERVED_EVENT_NAME_PREFIXES`] (`introspection.`, `gen_ai.`).
    /// Nothing is emitted in either case.
    ///
    /// ```rust,no_run
    /// use std::collections::HashMap;
    /// use introspection_sdk::otel::{IntrospectionLogs, LogEventOptions, PropertyValue};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let logs = IntrospectionLogs::builder().service_name("ark").build()?;
    /// logs.log_event(
    ///     "checkout.completed",
    ///     Some(HashMap::from([
    ///         ("order_id".to_string(), PropertyValue::from("o_1")),
    ///         ("total".to_string(), PropertyValue::from(42)),
    ///     ])),
    ///     LogEventOptions::new().with_event_id("checkout:o_1"),
    /// )?;
    /// logs.shutdown()?;
    /// # Ok(()) }
    /// ```
    pub fn log_event(
        &self,
        name: &str,
        attributes: Option<HashMap<String, PropertyValue>>,
        options: LogEventOptions,
    ) -> std::result::Result<(), LogEventError> {
        if name.is_empty() {
            return Err(LogEventError::EmptyName);
        }
        if let Some(prefix) = reserved_event_name_prefix(name) {
            return Err(LogEventError::ReservedName {
                name: name.to_string(),
                prefix,
            });
        }
        let record_attributes = self.build_attributes(
            name,
            attributes.as_ref(),
            None,
            None,
            None,
            options.event_id.as_deref(),
            options.identity.as_ref(),
        );
        self.emit_at(
            record_attributes,
            options.timestamp.unwrap_or_else(SystemTime::now),
            options.severity,
        );
        debug!("Logged event: {}", name);
        Ok(())
    }

    /// Track a custom event. A thin alias of [`Self::log_event`] kept for the
    /// Segment-style call shape.
    ///
    /// A name [`Self::log_event`] rejects (empty, or under `introspection.` /
    /// `gen_ai.`) is dropped with a `warn!`, since this signature has no error
    /// to return; call [`Self::log_event`] to handle the rejection.
    pub fn track(&self, event_name: &str, options: Option<TrackOptions>) {
        let opts = options.unwrap_or_default();
        let log_options = LogEventOptions {
            event_id: opts.event_id,
            ..LogEventOptions::default()
        };
        if let Err(e) = self.log_event(event_name, Some(opts.properties), log_options) {
            warn!("track: event dropped: {e}");
        }
    }

    /// Track feedback on a message or response.
    pub fn feedback(&self, name: &str, options: FeedbackOptions) {
        // The caller's extras go in first: `name` and `comments` are named
        // arguments, so a `with_property("name", ...)` must not silently
        // replace the feedback name this call is about.
        let mut properties: HashMap<String, PropertyValue> = options.extra.clone();
        properties.insert("name".to_string(), PropertyValue::String(name.to_string()));
        if let Some(comments) = &options.comments {
            properties.insert(
                "comments".to_string(),
                PropertyValue::String(comments.clone()),
            );
        }

        let attributes = self.build_attributes(
            types::event_name::FEEDBACK,
            Some(&properties),
            None,
            options.conversation_id.as_deref(),
            options.previous_response_id.as_deref(),
            options.event_id.as_deref(),
            None,
        );
        self.emit(attributes);
        debug!("Feedback: {}", name);
    }

    /// Identify a user and emit an identify event with their traits.
    pub fn identify(&self, user_id: &str, options: Option<IdentifyOptions>) {
        let opts = options.unwrap_or_default();

        // The identity attributes are read off the context, so the ids this
        // call is about have to be on it while the record is built. Without
        // this the event carries neither the user id nor the anonymous id.
        let mut values: Vec<(&str, &str)> = vec![(types::baggage::USER_ID, user_id)];
        if let Some(anon) = opts.anonymous_id.as_deref() {
            values.push((types::baggage::ANONYMOUS_ID, anon));
        }
        let _guard = BaggageGuard::new_multi(&values);

        let attributes = self.build_attributes(
            types::event_name::IDENTIFY,
            None,
            Some(&opts.traits),
            None,
            None,
            opts.event_id.as_deref(),
            None,
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

    /// Scope a conversation, minting an id when the caller has none.
    ///
    /// Without it a caller starting a fresh conversation had to invent an
    /// id, and an invented one does not
    /// match the `intro_conv_<hex>` shape the rest of the platform mints.
    ///
    /// Returns the id alongside the guard, since a caller that did not
    /// supply one still needs to know what it got — to correlate feedback
    /// against, or to hand to another process.
    ///
    /// ```rust,no_run
    /// # use introspection_sdk::otel::IntrospectionLogs;
    /// # let logs = IntrospectionLogs::builder().token("t").build().unwrap();
    /// let (conversation_id, _scope) = logs.conversation(None);
    /// logs.track("Turn Completed", None);
    /// # let _ = conversation_id;
    /// ```
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn conversation(&self, conversation_id: Option<&str>) -> (String, BaggageGuard) {
        let id = conversation_id
            .map(str::to_string)
            .unwrap_or_else(types::new_conversation_id);
        let guard = BaggageGuard::new(types::baggage::CONVERSATION_ID, &id);
        (id, guard)
    }

    /// Set previous response ID in OpenTelemetry baggage.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_previous_response_id(&self, previous_response_id: &str) -> BaggageGuard {
        BaggageGuard::new(types::baggage::PREVIOUS_RESPONSE_ID, previous_response_id)
    }

    /// Set agent context in OpenTelemetry baggage.
    #[must_use = "the returned guard must be held to maintain the baggage context"]
    pub fn set_agent(&self, agent_name: &str, agent_id: Option<&str>) -> BaggageGuard {
        // One guard for both keys. Layering a second guard over the first
        // would drop it, restoring the context that carried the name.
        match agent_id {
            Some(id) => BaggageGuard::new_multi(&[
                (types::baggage::AGENT_NAME, agent_name),
                (types::baggage::AGENT_ID, id),
            ]),
            None => BaggageGuard::new(types::baggage::AGENT_NAME, agent_name),
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logs_creation() {
        IntrospectionLogs::builder()
            .token("test-token")
            .service_name("test-service")
            .base_otel_url("http://localhost:4318")
            .build()
            .unwrap();
    }

    fn emitted(
        exporter: &opentelemetry_sdk::logs::InMemoryLogExporter,
    ) -> Vec<std::collections::HashMap<String, String>> {
        exporter
            .get_emitted_logs()
            .unwrap()
            .into_iter()
            .map(|log| {
                log.record
                    .attributes_iter()
                    .map(|(k, v)| (k.to_string(), format!("{v:?}")))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_positional_feedback_name_survives_an_extra_property_of_the_same_name() {
        let exporter = opentelemetry_sdk::logs::InMemoryLogExporter::default();
        let logs = IntrospectionLogs::with_exporter(exporter.clone(), "unit-tests");

        logs.feedback(
            "thumbs_up",
            FeedbackOptions::new()
                .with_comments("great")
                .with_extra("name", "not the feedback name")
                .with_extra("comments", "not the comments"),
        );
        let _ = logs.flush();

        let records = emitted(&exporter);
        let attrs = &records[0];
        assert!(
            attrs["properties.name"].contains("thumbs_up"),
            "got {}",
            attrs["properties.name"]
        );
        assert!(
            attrs["properties.comments"].contains("great"),
            "got {}",
            attrs["properties.comments"]
        );
    }

    #[test]
    fn every_record_carries_the_sdk_name_and_version_as_its_scope() {
        // The scope rides every log record and is how ingest attributes an
        // event to a client and a release. `logger(name)` left the version
        // unset, so nothing this crate emitted could be tied to a version.
        let exporter = opentelemetry_sdk::logs::InMemoryLogExporter::default();
        let logs = IntrospectionLogs::with_exporter(exporter.clone(), "unit-tests");
        logs.track("E", None);
        let _ = logs.flush();

        let emitted = exporter.get_emitted_logs().unwrap();
        let scope = &emitted[0].instrumentation;
        assert_eq!(scope.name(), "introspection-sdk");
        assert_eq!(scope.version(), Some(crate::VERSION));

        // The language is not in the scope name on purpose -- it rides the
        // resource, which is where semconv puts it. Assert that, since the
        // scope name's brevity depends on it.
        let resource = &emitted[0].resource;
        assert_eq!(
            resource
                .get(&opentelemetry::Key::from_static_str(
                    "telemetry.sdk.language"
                ))
                .map(|v| v.to_string())
                .as_deref(),
            Some("rust")
        );
    }

    #[test]
    fn test_numeric_and_bool_properties_keep_their_type_on_the_wire() {
        use opentelemetry::logs::AnyValue;

        let exporter = opentelemetry_sdk::logs::InMemoryLogExporter::default();
        let logs = IntrospectionLogs::with_exporter(exporter.clone(), "unit-tests");

        logs.track(
            "Rated",
            Some(
                TrackOptions::new()
                    .with_property("rating", 5)
                    .with_property("ratio", 0.5)
                    .with_property("ok", true)
                    .with_property("label", "good"),
            ),
        );
        let _ = logs.flush();

        let logs_out = exporter.get_emitted_logs().unwrap();
        let record = &logs_out[0].record;
        let attrs: std::collections::HashMap<String, AnyValue> = record
            .attributes_iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();

        // Stringifying these shipped AnyValue::String where the backend expects
        // an int/double/bool for the same key.
        assert!(matches!(attrs["properties.rating"], AnyValue::Int(5)));
        assert!(matches!(attrs["properties.ratio"], AnyValue::Double(_)));
        assert!(matches!(attrs["properties.ok"], AnyValue::Boolean(true)));
        assert!(matches!(attrs["properties.label"], AnyValue::String(_)));
    }

    #[test]
    fn test_track_emits_event_name_and_properties() {
        let exporter = opentelemetry_sdk::logs::InMemoryLogExporter::default();
        let logs = IntrospectionLogs::with_exporter(exporter.clone(), "unit-tests");

        logs.track(
            "Button Clicked",
            Some(TrackOptions::new().with_property("button_id", "submit")),
        );

        let records = emitted(&exporter);
        assert_eq!(records.len(), 1);
        assert!(records[0][types::attr::EVENT_NAME].contains("Button Clicked"));
        assert!(records[0].contains_key(types::attr::EVENT_ID));
        assert!(
            records[0][&format!("{}button_id", types::attr::PROPERTIES_PREFIX)].contains("submit")
        );
    }

    /// A null property is absent, not the four-character string `"null"`.
    ///
    /// `PropertyValue::Json` accepts `serde_json::Value::Null`, and
    /// stringifying it put `properties.note = "null"` on the wire — a value a
    /// consumer reads back as text, indistinguishable from a caller who meant
    /// the word. Null traits are dropped the same way.
    #[test]
    fn a_null_property_is_omitted_rather_than_stringified() {
        let exporter = opentelemetry_sdk::logs::InMemoryLogExporter::default();
        let logs = IntrospectionLogs::with_exporter(exporter.clone(), "unit-tests");

        logs.track(
            "Button Clicked",
            Some(
                TrackOptions::new()
                    .with_property("note", serde_json::Value::Null)
                    .with_property("kept", "yes"),
            ),
        );
        logs.identify(
            "user_42",
            Some(
                IdentifyOptions::new()
                    .with_trait("plan", serde_json::Value::Null)
                    .with_trait("email", "a@b.c"),
            ),
        );

        let records = emitted(&exporter);
        let props = format!("{}note", types::attr::PROPERTIES_PREFIX);
        let kept = format!("{}kept", types::attr::PROPERTIES_PREFIX);
        assert!(
            !records[0].contains_key(&props),
            "null property was emitted"
        );
        assert!(
            records[0].contains_key(&kept),
            "sibling property was dropped"
        );

        let plan = format!("{}plan", types::attr::TRAITS_PREFIX);
        let email = format!("{}email", types::attr::TRAITS_PREFIX);
        assert!(!records[1].contains_key(&plan), "null trait was emitted");
        assert!(records[1].contains_key(&email), "sibling trait was dropped");
    }

    #[test]
    fn test_identify_puts_both_ids_on_the_record() {
        // The identity attributes are read off the context, so identify() has
        // to place its own ids there before building the record.
        let exporter = opentelemetry_sdk::logs::InMemoryLogExporter::default();
        let logs = IntrospectionLogs::with_exporter(exporter.clone(), "unit-tests");

        logs.identify(
            "user_42",
            Some(
                IdentifyOptions::new()
                    .with_anonymous_id("anon_7")
                    .with_trait("plan", "pro"),
            ),
        );

        let records = emitted(&exporter);
        assert_eq!(records.len(), 1);
        assert!(records[0][types::attr::EVENT_NAME].contains(types::event_name::IDENTIFY));
        assert!(records[0][types::attr::USER_ID].contains("user_42"));
        assert!(records[0][types::attr::ANONYMOUS_ID].contains("anon_7"));
        assert!(records[0][&format!("{}plan", types::attr::TRAITS_PREFIX)].contains("pro"));
    }

    #[test]
    fn test_feedback_carries_the_conversation_and_agent_scope() {
        let exporter = opentelemetry_sdk::logs::InMemoryLogExporter::default();
        let logs = IntrospectionLogs::with_exporter(exporter.clone(), "unit-tests");

        {
            let _agent = logs.set_agent("planner", Some("ag_1"));
            logs.feedback(
                "thumbs_up",
                FeedbackOptions::new().with_conversation_id("conv_7"),
            );
        }

        let records = emitted(&exporter);
        assert_eq!(records.len(), 1);
        assert!(records[0][types::attr::EVENT_NAME].contains(types::event_name::FEEDBACK));
        assert!(records[0][types::attr::CONVERSATION_ID].contains("conv_7"));
        assert!(records[0][types::attr::AGENT_NAME].contains("planner"));
        assert!(records[0][types::attr::AGENT_ID].contains("ag_1"));
    }

    mod log_event {
        use super::*;
        use opentelemetry::logs::{AnyValue, Severity};
        use opentelemetry_sdk::logs::InMemoryLogExporter;
        use std::time::{Duration, UNIX_EPOCH};

        fn logs() -> (IntrospectionLogs, InMemoryLogExporter) {
            let exporter = InMemoryLogExporter::default();
            let logs = IntrospectionLogs::with_exporter(exporter.clone(), "unit-tests");
            (logs, exporter)
        }

        fn props(pairs: &[(&str, PropertyValue)]) -> Option<HashMap<String, PropertyValue>> {
            Some(
                pairs
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            )
        }

        fn attr(attrs: &HashMap<String, String>, key: &str) -> String {
            attrs
                .get(key)
                .unwrap_or_else(|| panic!("missing {key} in {attrs:?}"))
                .clone()
        }

        #[test]
        fn emits_the_custom_name_with_attributes_under_properties() {
            let (logs, exporter) = logs();
            logs.log_event(
                "ark.feed.entry",
                props(&[
                    ("entry_id", "e_1".into()),
                    ("score", 0.9.into()),
                    ("tags", serde_json::json!(["a", "b"]).into()),
                    ("skipped", serde_json::Value::Null.into()),
                ]),
                LogEventOptions::new(),
            )
            .unwrap();

            let out = exporter.get_emitted_logs().unwrap();
            assert_eq!(out.len(), 1);
            let record = &out[0].record;
            let attrs: HashMap<String, AnyValue> = record
                .attributes_iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            assert_eq!(
                attrs[types::attr::EVENT_NAME],
                AnyValue::String("ark.feed.entry".into())
            );
            assert_eq!(attrs["properties.entry_id"], AnyValue::String("e_1".into()));
            assert_eq!(attrs["properties.score"], AnyValue::Double(0.9));
            assert_eq!(
                attrs["properties.tags"],
                AnyValue::String(r#"["a","b"]"#.into())
            );
            assert!(!attrs.contains_key("properties.skipped"));
            assert!(
                matches!(&attrs[types::attr::EVENT_ID], AnyValue::String(id) if id.as_str().starts_with("intro_event_"))
            );
            assert_eq!(record.severity_number(), Some(Severity::Info));
            assert_eq!(record.severity_text(), Some("INFO"));
        }

        #[test]
        fn passes_a_caller_supplied_event_id_through_for_dedup() {
            let (logs, exporter) = logs();
            for _ in 0..2 {
                logs.log_event(
                    "ark.feed.entry",
                    props(&[("entry_id", "e_1".into())]),
                    LogEventOptions::new().with_event_id("feed-entry:e_1"),
                )
                .unwrap();
            }
            let ids: Vec<String> = emitted(&exporter)
                .iter()
                .map(|a| attr(a, types::attr::EVENT_ID))
                .collect();
            assert_eq!(ids.len(), 2);
            assert!(
                ids.iter().all(|id| id.contains("feed-entry:e_1")),
                "{ids:?}"
            );
        }

        #[test]
        fn honours_timestamp_and_severity() {
            let (logs, exporter) = logs();
            let at = UNIX_EPOCH + Duration::from_millis(1_767_323_045_678);
            logs.log_event(
                "ark.sync.failed",
                None,
                LogEventOptions::new()
                    .with_timestamp(at)
                    .with_severity(LogEventSeverity::Error),
            )
            .unwrap();
            let out = exporter.get_emitted_logs().unwrap();
            let record = &out[0].record;
            assert_eq!(record.timestamp(), Some(at));
            assert_eq!(record.severity_number(), Some(Severity::Error));
            assert_eq!(record.severity_text(), Some("ERROR"));
        }

        #[test]
        fn maps_every_severity() {
            let (logs, exporter) = logs();
            for severity in [
                LogEventSeverity::Debug,
                LogEventSeverity::Info,
                LogEventSeverity::Warn,
                LogEventSeverity::Error,
            ] {
                logs.log_event(
                    "ark.s",
                    None,
                    LogEventOptions::new().with_severity(severity),
                )
                .unwrap();
            }
            let got: Vec<_> = exporter
                .get_emitted_logs()
                .unwrap()
                .iter()
                .map(|l| (l.record.severity_number(), l.record.severity_text()))
                .collect();
            assert_eq!(
                got,
                vec![
                    (Some(Severity::Debug), Some("DEBUG")),
                    (Some(Severity::Info), Some("INFO")),
                    (Some(Severity::Warn), Some("WARN")),
                    (Some(Severity::Error), Some("ERROR")),
                ]
            );
        }

        #[test]
        fn lets_the_identity_option_override_the_context_field_by_field() {
            let (logs, exporter) = logs();
            {
                let _user = logs.set_user_id("ctx_user");
                logs.log_event(
                    "ark.a",
                    None,
                    LogEventOptions::new().with_identity(
                        LogEventIdentity::new()
                            .with_user_id("explicit_user")
                            .with_anonymous_id("anon_1"),
                    ),
                )
                .unwrap();
                logs.log_event(
                    "ark.b",
                    None,
                    LogEventOptions::new()
                        .with_identity(LogEventIdentity::new().with_anonymous_id("anon_2")),
                )
                .unwrap();
            }
            let records = emitted(&exporter);
            assert!(attr(&records[0], types::attr::USER_ID).contains("explicit_user"));
            assert!(attr(&records[0], types::attr::ANONYMOUS_ID).contains("anon_1"));
            // An omitted field still falls back to the scoped identity.
            assert!(attr(&records[1], types::attr::USER_ID).contains("ctx_user"));
            assert!(attr(&records[1], types::attr::ANONYMOUS_ID).contains("anon_2"));
        }

        #[test]
        fn rejects_reserved_names() {
            let (logs, exporter) = logs();
            for (name, prefix) in [
                ("introspection.track", "introspection."),
                ("introspection.feedback", "introspection."),
                ("gen_ai.client.inference", "gen_ai."),
            ] {
                let err = logs
                    .log_event(name, None, LogEventOptions::new())
                    .unwrap_err();
                assert_eq!(
                    err,
                    LogEventError::ReservedName {
                        name: name.to_string(),
                        prefix,
                    }
                );
                assert!(err.to_string().contains("reserved"), "{err}");
            }
            assert!(exporter.get_emitted_logs().unwrap().is_empty());
        }

        #[test]
        fn rejects_an_empty_name() {
            let (logs, exporter) = logs();
            let err = logs
                .log_event("", None, LogEventOptions::new())
                .unwrap_err();
            assert_eq!(err, LogEventError::EmptyName);
            assert!(err.to_string().contains("non-empty"));
            assert!(exporter.get_emitted_logs().unwrap().is_empty());
        }

        #[test]
        fn allows_names_that_merely_contain_a_reserved_word() {
            let (logs, exporter) = logs();
            logs.log_event("my.introspection.event", None, LogEventOptions::new())
                .unwrap();
            logs.log_event("gen_ai_usage", None, LogEventOptions::new())
                .unwrap();
            assert_eq!(exporter.get_emitted_logs().unwrap().len(), 2);
        }

        #[test]
        fn names_the_reserved_prefixes() {
            assert_eq!(
                types::RESERVED_EVENT_NAME_PREFIXES,
                ["introspection.", "gen_ai."]
            );
        }

        #[test]
        fn track_emits_what_log_event_emits() {
            let (logs, exporter) = logs();
            logs.track(
                "Button Clicked",
                Some(
                    TrackOptions::new()
                        .with_property("buttonId", "submit")
                        .with_event_id("e1"),
                ),
            );
            logs.log_event(
                "Button Clicked",
                props(&[("buttonId", "submit".into())]),
                LogEventOptions::new().with_event_id("e1"),
            )
            .unwrap();

            let out = exporter.get_emitted_logs().unwrap();
            assert_eq!(out.len(), 2);
            let shape = |i: usize| {
                let r = &out[i].record;
                let mut attrs: Vec<(String, String)> = r
                    .attributes_iter()
                    .map(|(k, v)| (k.to_string(), format!("{v:?}")))
                    .collect();
                attrs.sort();
                (attrs, r.severity_number(), r.severity_text())
            };
            assert_eq!(shape(0), shape(1));
            let attrs = &emitted(&exporter)[0];
            assert!(attr(attrs, types::attr::EVENT_NAME).contains("Button Clicked"));
            assert!(attr(attrs, types::attr::EVENT_ID).contains("e1"));
            assert!(attr(attrs, "properties.buttonId").contains("submit"));
        }

        #[test]
        fn track_drops_a_reserved_name() {
            let (logs, exporter) = logs();
            logs.track("introspection.feedback", None);
            logs.track("", None);
            assert!(exporter.get_emitted_logs().unwrap().is_empty());
        }
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

    #[test]
    fn conversation_mints_an_id_in_the_shape_the_other_sdks_mint() {
        let logs = IntrospectionLogs::builder()
            .token("test-token")
            .build()
            .unwrap();

        let (id, _scope) = logs.conversation(None);
        // `intro_conv_` + 32 hex, the shape the backend
        // produce and the same one the span processor falls back to.
        assert!(id.starts_with("intro_conv_"), "got {id}");
        let hex = &id["intro_conv_".len()..];
        assert_eq!(hex.len(), 32);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));

        let cx = Context::current();
        assert_eq!(
            cx.baggage()
                .get(types::baggage::CONVERSATION_ID)
                .map(|v| v.to_string()),
            Some(id)
        );
    }

    #[test]
    fn conversation_honours_an_id_the_caller_already_has() {
        let logs = IntrospectionLogs::builder()
            .token("test-token")
            .build()
            .unwrap();

        let (id, _scope) = logs.conversation(Some("conv_from_upstream"));
        assert_eq!(id, "conv_from_upstream");
        let cx = Context::current();
        assert_eq!(
            cx.baggage()
                .get(types::baggage::CONVERSATION_ID)
                .map(|v| v.to_string()),
            Some("conv_from_upstream".to_string())
        );
    }

    #[test]
    fn the_conversation_scope_is_released_with_its_guard() {
        let logs = IntrospectionLogs::builder()
            .token("test-token")
            .build()
            .unwrap();

        {
            let (_id, _scope) = logs.conversation(None);
            assert!(Context::current()
                .baggage()
                .get(types::baggage::CONVERSATION_ID)
                .is_some());
        }
        assert!(Context::current()
            .baggage()
            .get(types::baggage::CONVERSATION_ID)
            .is_none());
    }
}
