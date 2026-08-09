//! Span processor that sends traces to the introspection API.

use opentelemetry::trace::TraceId;
use opentelemetry_otlp::{SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::error::OTelSdkError;
use opentelemetry_sdk::trace::{
    BatchSpanProcessor, SimpleSpanProcessor, SpanData, SpanProcessor as OtelSpanProcessor,
};
use std::collections::{HashMap, VecDeque};
use std::env;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info};

use crate::otel::types;
use crate::VERSION;

/// Create a `reqwest::blocking::Client` on a dedicated thread.
///
/// `BatchSpanProcessor` exports on a background thread without a
/// tokio runtime, so the OTLP exporter needs the blocking reqwest
/// client (gated by `opentelemetry-otlp/reqwest-blocking-client` ->
/// `opentelemetry-http/reqwest-blocking`). The blocking client
/// spawns an internal tokio runtime which panics if constructed
/// inside an existing async runtime — building it on a short-lived
/// thread avoids the "cannot drop a runtime …" issue.
fn new_blocking_http_client(timeout: Duration) -> reqwest::blocking::Client {
    std::thread::spawn(move || {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
    .join()
    .expect("failed to create blocking HTTP client")
}

/// Errors that can occur in the Introspection span processor.
#[derive(Error, Debug)]
pub enum SpanProcessorError {
    #[error("OpenTelemetry error: {0}")]
    OpenTelemetry(String),

    #[error("Token is required")]
    TokenRequired,

    #[error("Base URL is required")]
    BaseUrlRequired,
}

impl From<OTelSdkError> for SpanProcessorError {
    fn from(e: OTelSdkError) -> Self {
        SpanProcessorError::OpenTelemetry(e.to_string())
    }
}

/// Result type for SpanProcessor operations.
pub type SpanProcessorResult<T> = std::result::Result<T, SpanProcessorError>;

/// Advanced options for [`IntrospectionSpanProcessor`].
///
/// Independent from the REST [`crate::AdvancedOptions`] — the span
/// processor talks to the OTLP traces endpoint, which is a different
/// host from the DP REST API.
#[derive(Clone, Debug, Default)]
pub struct SpanProcessorAdvancedOptions {
    /// OTLP collector base URL. If unset, falls back to
    /// `INTROSPECTION_BASE_OTEL_URL`, then to
    /// `https://otel.introspection.dev`.
    pub base_otel_url: Option<String>,

    /// Additional HTTP headers attached to the OTLP export.
    pub additional_headers: Option<HashMap<String, String>>,

    /// Custom span exporter, bypassing the default OTLP one.
    ///
    /// Concrete rather than a trait object because `SpanExporter` is not
    /// dyn-compatible in opentelemetry_sdk 0.32 (it has an async method), so
    /// this cannot accept an in-memory exporter: use it to redirect export to
    /// a different OTLP collector, not to capture spans in a test. Supplying
    /// one waives the token requirement, since nothing then reaches the
    /// Introspection endpoint.
    ///
    /// The processor takes ownership, so the `Arc` must be unshared.
    pub span_exporter: Option<Arc<SpanExporter>>,

    /// Flush interval in milliseconds for the batch processor.
    /// Lower values reduce latency but increase network requests.
    /// Default: 5000
    pub flush_interval_ms: Option<u64>,

    /// Maximum batch size before auto-flush. Set to `1` for sequential
    /// (immediate) export — useful for multi-turn conversations.
    /// Default: uses the OTel SDK default.
    ///
    /// **`Some(1)` costs you an OTLP round trip per span, on the thread that
    /// ended it.** It swaps the background `BatchSpanProcessor` for a
    /// `SimpleSpanProcessor`, whose `on_end` exports synchronously. Reach for
    /// it only when the backend genuinely must ingest span N before span N+1;
    /// otherwise leave it unset and let the batcher export off-thread.
    pub max_batch_size: Option<usize>,
}

/// Configuration for the Introspection span processor.
#[derive(Clone, Debug, Default)]
pub struct SpanProcessorConfig {
    /// Authentication token
    pub token: Option<String>,
    /// Service name (default: "introspection-client")
    pub service_name: Option<String>,
    /// Advanced options for configuration and testing
    pub advanced: Option<SpanProcessorAdvancedOptions>,
}

impl SpanProcessorConfig {
    /// Create a new config with token.
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            ..Default::default()
        }
    }

    /// Set advanced options.
    pub fn advanced(mut self, advanced: SpanProcessorAdvancedOptions) -> Self {
        self.advanced = Some(advanced);
        self
    }

    /// Builder pattern for configuration.
    pub fn builder() -> SpanProcessorConfigBuilder {
        SpanProcessorConfigBuilder::default()
    }
}

/// Builder for SpanProcessorConfig.
#[derive(Default)]
pub struct SpanProcessorConfigBuilder {
    token: Option<String>,
    service_name: Option<String>,
    advanced: Option<SpanProcessorAdvancedOptions>,
}

impl SpanProcessorConfigBuilder {
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn service_name(mut self, service_name: impl Into<String>) -> Self {
        self.service_name = Some(service_name.into());
        self
    }

    pub fn advanced(mut self, advanced: SpanProcessorAdvancedOptions) -> Self {
        self.advanced = Some(advanced);
        self
    }

    pub fn build(self) -> SpanProcessorConfig {
        SpanProcessorConfig {
            token: self.token,
            service_name: self.service_name,
            advanced: self.advanced,
        }
    }
}

/// Inner processor type — either batch (default) or simple (for sequential export).
#[derive(Debug)]
enum InnerProcessor {
    Batch(BatchSpanProcessor),
    Simple(SimpleSpanProcessor<SpanExporter>),
}

/// Span processor that sends traces to the introspection API.
///
/// This wraps OpenTelemetry's BatchSpanProcessor (default) or SimpleSpanProcessor
/// (when `max_batch_size = Some(1)`) and configures it to send traces to the
/// introspection backend via OTLP.
///
/// Set `max_batch_size` to `1` to export each span individually on end, ensuring
/// sequential processing by the backend. This is useful for multi-turn conversations
/// where each turn must be ingested before the next arrives — at the price of a
/// synchronous OTLP round trip inside `on_end`, on whichever thread closed the
/// span. Leave it unset unless you need that ordering.
///
/// # Example
///
/// ```rust,no_run
/// use introspection_sdk::otel::{
///     IntrospectionSpanProcessor, SpanProcessorAdvancedOptions, SpanProcessorConfig,
/// };
/// use opentelemetry_sdk::trace::SdkTracerProvider;
///
/// // Simple usage
/// let span_processor = IntrospectionSpanProcessor::new(
///     SpanProcessorConfig::with_token("your-token")
/// ).unwrap();
///
/// // Sequential export for multi-turn conversations
/// let span_processor = IntrospectionSpanProcessor::new(
///     SpanProcessorConfig::with_token("your-token")
///         .advanced(SpanProcessorAdvancedOptions {
///             max_batch_size: Some(1),
///             ..Default::default()
///         })
/// ).unwrap();
///
/// let provider = SdkTracerProvider::builder()
///     .with_span_processor(span_processor)
///     .build();
/// ```
#[derive(Debug)]
pub struct IntrospectionSpanProcessor {
    inner: InnerProcessor,
    service_name: Option<String>,
    /// Conversation id minted per trace, for spans that arrive without one.
    /// Bounded: see [`MAX_TRACKED_TRACES`].
    trace_conversations: Mutex<(HashMap<TraceId, String>, VecDeque<TraceId>)>,
}

/// How many traces the conversation-id fallback remembers.
///
/// The map would otherwise grow for the process's lifetime, one entry per
/// trace. Matching the other SDKs' bound keeps the eviction point the same
/// everywhere.
const MAX_TRACKED_TRACES: usize = 4096;

/// A span carrying any of these is LLM-relevant and gets exported. Everything
/// else on the provider (HTTP clients, framework spans, database drivers)
/// reaches this processor too and must not be shipped to Introspection.
const GEN_AI_MARKERS: [&str; 5] = [
    types::attr::GEN_AI_PROVIDER_NAME,
    types::attr::GEN_AI_OPERATION_NAME,
    types::attr::GEN_AI_REQUEST_MODEL,
    types::attr::GEN_AI_INPUT_MESSAGES,
    types::attr::GEN_AI_OUTPUT_MESSAGES,
];

/// Baggage key -> the span attribute it becomes. The baggage keys use the
/// `identify()` underscore form; the span attributes use the dotted semconv
/// form. The gen_ai keys are identical on both sides but are listed here so
/// the projection stays in one table.
const BAGGAGE_TO_ATTRIBUTE: [(&str, &str); 5] = [
    (
        types::baggage::CONVERSATION_ID,
        types::attr::CONVERSATION_ID,
    ),
    (types::baggage::AGENT_NAME, types::attr::AGENT_NAME),
    (types::baggage::AGENT_ID, types::attr::AGENT_ID),
    (types::baggage::USER_ID, types::attr::USER_ID),
    (types::baggage::ANONYMOUS_ID, types::attr::ANONYMOUS_ID),
];

/// Deprecated pre-1.27 provider key, superseded by `gen_ai.provider.name`.
const DEPRECATED_SYSTEM_KEY: &str = "gen_ai.system";

/// Whether a span carries enough LLM data to be worth exporting.
fn should_export(span: &SpanData) -> bool {
    span.attributes
        .iter()
        .any(|kv| GEN_AI_MARKERS.contains(&kv.key.as_str()))
}

impl IntrospectionSpanProcessor {
    /// Create a new IntrospectionSpanProcessor with the given configuration.
    pub fn new(config: SpanProcessorConfig) -> SpanProcessorResult<Self> {
        let advanced = config.advanced.unwrap_or_default();

        // A caller-supplied exporter never reaches the Introspection endpoint,
        // so there is nothing to authenticate; requiring a token there would
        // make the in-memory testing path need a dummy one. Matches the JS
        // SDK, which documents the same exemption.
        let has_custom_exporter = advanced.span_exporter.is_some();
        let token = match config
            .token
            .or_else(|| env::var("INTROSPECTION_TOKEN").ok())
        {
            Some(token) => token,
            None if has_custom_exporter => String::new(),
            None => return Err(SpanProcessorError::TokenRequired),
        };

        // Stays `None` when the caller did not ask for one. Defaulting here
        // and then applying it in `set_resource` would rewrite the
        // `service.name` of a provider the caller had already labelled: a
        // process that built its provider as "checkout-api" would see every
        // LLM span arrive at Introspection as "introspection-client". Python
        // and JS both only merge when the option is set.
        let service_name = config
            .service_name
            .or_else(|| env::var("INTROSPECTION_SERVICE_NAME").ok());

        // Building a stand-in here instead of using the caller's exporter
        // would silently discard it and export to the stand-in's endpoint.
        let exporter_for_processor: SpanExporter =
            if let Some(custom_exporter) = advanced.span_exporter {
                Arc::try_unwrap(custom_exporter).map_err(|_| {
                    SpanProcessorError::OpenTelemetry(
                        "Custom span exporter has multiple references".to_string(),
                    )
                })?
            } else {
                let base_url = advanced
                    .base_otel_url
                    .or_else(|| env::var("INTROSPECTION_BASE_OTEL_URL").ok())
                    .unwrap_or_else(|| types::defaults::BASE_OTEL_URL.to_string());

                let endpoint = if base_url.ends_with(types::api_path::TRACES) {
                    base_url.clone()
                } else {
                    format!(
                        "{}{}",
                        base_url.trim_end_matches('/'),
                        types::api_path::TRACES
                    )
                };

                info!(
                    "IntrospectionSpanProcessor initialized: service={}, endpoint={}",
                    service_name.as_deref().unwrap_or("<provider default>"),
                    endpoint
                );

                let mut headers = HashMap::new();
                headers.insert(
                    "User-Agent".to_string(),
                    format!("introspection-sdk/{}", VERSION),
                );
                headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                if let Some(additional) = advanced.additional_headers {
                    headers.extend(additional);
                }

                let http_client = new_blocking_http_client(Duration::from_secs(30));

                SpanExporter::builder()
                    .with_http()
                    .with_http_client(http_client)
                    .with_endpoint(&endpoint)
                    .with_headers(headers)
                    .with_timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|e| SpanProcessorError::OpenTelemetry(e.to_string()))?
            };

        // `max_batch_size = 1` selects SimpleSpanProcessor, which exports each
        // span immediately on end() -- useful for multi-turn conversations
        // where each turn must be ingested before the next arrives.
        //
        // It is opt-in only. `SimpleSpanProcessor::on_end` blocks the calling
        // thread on an HTTPS POST behind a mutex, so inferring it from a
        // token prefix silently turned every `span.end()` on a dev or staging
        // token -- including ones on a tokio worker -- into blocking I/O the
        // caller never asked for.
        let max_batch_size = advanced.max_batch_size;
        let flush_interval = Duration::from_millis(
            advanced
                .flush_interval_ms
                .unwrap_or(types::defaults::FLUSH_INTERVAL_MS),
        );
        let inner = if max_batch_size == Some(1) {
            InnerProcessor::Simple(SimpleSpanProcessor::new(exporter_for_processor))
        } else {
            let mut batch_config = opentelemetry_sdk::trace::BatchConfigBuilder::default()
                .with_scheduled_delay(flush_interval);
            if let Some(batch_size) = max_batch_size {
                batch_config = batch_config.with_max_export_batch_size(batch_size);
            }
            InnerProcessor::Batch(
                BatchSpanProcessor::builder(exporter_for_processor)
                    .with_batch_config(batch_config.build())
                    .build(),
            )
        };

        Ok(Self {
            inner,
            service_name,
            trace_conversations: Mutex::new((HashMap::new(), VecDeque::new())),
        })
    }

    /// The resource the inner processor should use, given the provider's.
    ///
    /// `service_name` is a documented constructor argument, and the provider's
    /// resource is the only place it can land -- SpanData carries no resource
    /// of its own. It is applied only when the caller set it: an unset option
    /// must leave a provider the caller already labelled alone.
    fn resource_for(&self, provider: &opentelemetry_sdk::Resource) -> opentelemetry_sdk::Resource {
        let Some(name) = &self.service_name else {
            return provider.clone();
        };
        let mut builder = opentelemetry_sdk::Resource::builder().with_service_name(name.clone());
        for kv in provider.iter() {
            if kv.0.as_str() != "service.name" {
                builder = builder
                    .with_attribute(opentelemetry::KeyValue::new(kv.0.clone(), kv.1.clone()));
            }
        }
        builder.build()
    }

    /// The conversation id minted for `trace_id`, creating one on first sight.
    fn conversation_id_for_trace(&self, trace_id: TraceId) -> String {
        let mut guard = self
            .trace_conversations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (ids, order) = &mut *guard;
        if let Some(existing) = ids.get(&trace_id) {
            return existing.clone();
        }
        let conversation_id = format!("intro_conv_{}", uuid::Uuid::new_v4().simple());
        ids.insert(trace_id, conversation_id.clone());
        order.push_back(trace_id);
        while order.len() > MAX_TRACKED_TRACES {
            if let Some(oldest) = order.pop_front() {
                ids.remove(&oldest);
            }
        }
        conversation_id
    }

    /// Stamp baggage-derived attributes onto a span, resolve its conversation
    /// id, and drop the deprecated provider key.
    ///
    /// Baggage wins over a pre-stamped attribute so per-call identity set with
    /// [`crate::otel::IntrospectionLogs`]'s guards overrides whatever the
    /// emitter defaulted to. Without this the guards scope the emitted events
    /// and leave the spans anonymous.
    fn enrich(&self, span: &mut SpanData) {
        use opentelemetry::baggage::BaggageExt;

        let cx = opentelemetry::Context::current();
        let baggage = cx.baggage();

        span.attributes
            .retain(|kv| kv.key.as_str() != DEPRECATED_SYSTEM_KEY);

        for (baggage_key, attribute_key) in BAGGAGE_TO_ATTRIBUTE {
            let Some(value) = baggage.get(baggage_key) else {
                continue;
            };
            let value = value.to_string();
            span.attributes
                .retain(|kv| kv.key.as_str() != attribute_key);
            span.attributes
                .push(opentelemetry::KeyValue::new(attribute_key, value));
        }

        // Baggage won above, and an id the emitter set itself is still on the
        // span, so anything left without one has no conversation at all. Mint
        // a stable id per trace: without it the spans of a single run reach
        // Introspection unable to be grouped, which is what the other SDKs
        // avoid by doing exactly this.
        let has_conversation = span
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == types::attr::CONVERSATION_ID);
        if !has_conversation {
            let conversation_id = self.conversation_id_for_trace(span.span_context.trace_id());
            span.attributes.push(opentelemetry::KeyValue::new(
                types::attr::CONVERSATION_ID,
                conversation_id,
            ));
        }

        // An emitter that recorded messages without naming the operation is
        // doing a chat completion.
        let has_operation = span
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == types::attr::GEN_AI_OPERATION_NAME);
        let has_messages = span.attributes.iter().any(|kv| {
            kv.key.as_str() == types::attr::GEN_AI_INPUT_MESSAGES
                || kv.key.as_str() == types::attr::GEN_AI_OUTPUT_MESSAGES
        });
        if !has_operation && has_messages {
            span.attributes.push(opentelemetry::KeyValue::new(
                types::attr::GEN_AI_OPERATION_NAME,
                "chat",
            ));
        }
    }
}

impl OtelSpanProcessor for IntrospectionSpanProcessor {
    fn set_resource(&mut self, resource: &opentelemetry_sdk::Resource) {
        let resource = self.resource_for(resource);
        match &mut self.inner {
            InnerProcessor::Batch(p) => p.set_resource(&resource),
            InnerProcessor::Simple(p) => p.set_resource(&resource),
        }
    }

    fn on_start(&self, span: &mut opentelemetry_sdk::trace::Span, cx: &opentelemetry::Context) {
        debug!("Starting introspection span");
        match &self.inner {
            InnerProcessor::Batch(p) => p.on_start(span, cx),
            InnerProcessor::Simple(p) => p.on_start(span, cx),
        }
    }

    fn on_end(&self, mut span: SpanData) {
        debug!("Ending introspection span");

        if !should_export(&span) {
            // An infrastructure span: HTTP, routing, database. Exporting it
            // would ship unrelated traffic to Introspection.
            return;
        }

        self.enrich(&mut span);

        match &self.inner {
            InnerProcessor::Batch(p) => p.on_end(span),
            InnerProcessor::Simple(p) => p.on_end(span),
        }
    }

    fn shutdown(&self) -> Result<(), OTelSdkError> {
        info!("Shutting down introspection span processor");
        match &self.inner {
            InnerProcessor::Batch(p) => p.shutdown(),
            InnerProcessor::Simple(p) => p.shutdown(),
        }
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> Result<(), OTelSdkError> {
        info!("Shutting down introspection span processor with timeout");
        match &self.inner {
            InnerProcessor::Batch(p) => p.shutdown_with_timeout(timeout),
            InnerProcessor::Simple(p) => p.shutdown_with_timeout(timeout),
        }
    }

    fn force_flush(&self) -> Result<(), OTelSdkError> {
        info!("Flushing introspection span processor");
        match &self.inner {
            InnerProcessor::Batch(p) => p.force_flush(),
            InnerProcessor::Simple(p) => p.force_flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{Span, SpanKind, Status, Tracer, TracerProvider};
    use opentelemetry::KeyValue;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use std::sync::Mutex;

    /// Mutex to serialize tests that manipulate environment variables.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_span_processor_creation_with_token() {
        let processor =
            IntrospectionSpanProcessor::new(SpanProcessorConfig::with_token("test-token")).unwrap();

        assert!(processor.force_flush().is_ok());
    }

    #[test]
    fn test_span_processor_creation_with_advanced_options() {
        let mut custom_headers = HashMap::new();
        custom_headers.insert("X-Custom-Header".to_string(), "custom-value".to_string());

        let processor = IntrospectionSpanProcessor::new(
            SpanProcessorConfig::with_token("test-token").advanced(SpanProcessorAdvancedOptions {
                base_otel_url: Some("http://localhost:5418".to_string()),
                additional_headers: Some(custom_headers),
                span_exporter: None,
                flush_interval_ms: None,
                max_batch_size: None,
            }),
        )
        .unwrap();

        assert!(processor.force_flush().is_ok());
    }

    #[test]
    fn test_span_processor_with_custom_exporter() {
        let test_exporter = Arc::new(
            SpanExporter::builder()
                .with_http()
                .with_endpoint("http://localhost:9999/v1/traces")
                .with_timeout(Duration::from_secs(1))
                .build()
                .unwrap(),
        );

        // No token: a caller-supplied exporter never reaches the
        // Introspection endpoint, so there is nothing to authenticate.
        let processor = IntrospectionSpanProcessor::new(SpanProcessorConfig::default().advanced(
            SpanProcessorAdvancedOptions {
                span_exporter: Some(test_exporter),
                ..Default::default()
            },
        ))
        .expect("a custom exporter waives the token requirement");

        assert!(processor.force_flush().is_ok());
    }

    #[test]
    fn test_custom_exporter_must_not_be_shared() {
        // The processor takes ownership of the exporter. A caller holding a
        // second reference gets a loud error rather than a processor that
        // quietly exports somewhere else.
        let exporter = Arc::new(
            SpanExporter::builder()
                .with_http()
                .with_endpoint("http://localhost:9999/v1/traces")
                .with_timeout(Duration::from_secs(1))
                .build()
                .unwrap(),
        );
        let _second_reference = Arc::clone(&exporter);

        let err = IntrospectionSpanProcessor::new(
            SpanProcessorConfig::with_token("test-token").advanced(SpanProcessorAdvancedOptions {
                span_exporter: Some(exporter),
                ..Default::default()
            }),
        )
        .expect_err("a shared exporter cannot be moved into the processor");

        assert!(
            matches!(err, SpanProcessorError::OpenTelemetry(ref m) if m.contains("multiple references")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn test_span_processor_processes_spans() {
        let processor = IntrospectionSpanProcessor::new(
            SpanProcessorConfig::with_token("test-token").advanced(SpanProcessorAdvancedOptions {
                base_otel_url: Some("http://localhost:9999".to_string()),
                ..Default::default()
            }),
        )
        .unwrap();

        let provider = SdkTracerProvider::builder()
            .with_span_processor(processor)
            .build();

        let tracer = provider.tracer("test-tracer");

        let mut span = tracer
            .span_builder("test-span")
            .with_kind(SpanKind::Server)
            .start(&tracer);
        span.set_status(Status::Ok);
        span.set_attribute(KeyValue::new("test.key", "test.value"));
        span.end();

        let _ = provider.force_flush();

        provider.shutdown().unwrap();
    }

    #[test]
    fn test_span_processor_shutdown() {
        let processor =
            IntrospectionSpanProcessor::new(SpanProcessorConfig::with_token("test-token")).unwrap();

        assert!(processor.shutdown().is_ok());
    }

    #[test]
    fn test_span_processor_shutdown_with_timeout() {
        let processor =
            IntrospectionSpanProcessor::new(SpanProcessorConfig::with_token("test-token")).unwrap();

        assert!(processor
            .shutdown_with_timeout(Duration::from_secs(1))
            .is_ok());
    }

    #[test]
    fn test_span_processor_requires_token() {
        let _lock = ENV_MUTEX.lock().unwrap();

        let old_token = std::env::var("INTROSPECTION_TOKEN").ok();
        std::env::remove_var("INTROSPECTION_TOKEN");

        let config = SpanProcessorConfig {
            token: None,
            service_name: None,
            advanced: None,
        };

        let result = IntrospectionSpanProcessor::new(config);
        assert!(
            result.is_err(),
            "Expected TokenRequired error when no token is provided."
        );
        assert!(matches!(
            result.unwrap_err(),
            SpanProcessorError::TokenRequired
        ));

        if let Some(token) = old_token {
            std::env::set_var("INTROSPECTION_TOKEN", token);
        }
    }

    #[test]
    fn test_span_processor_with_explicit_token() {
        let config = SpanProcessorConfig {
            token: Some("explicit-token".to_string()),
            service_name: None,
            advanced: None,
        };

        let processor = IntrospectionSpanProcessor::new(config);
        assert!(processor.is_ok());
    }

    #[test]
    fn test_span_processor_uses_env_token() {
        let _lock = ENV_MUTEX.lock().unwrap();

        let old_token = std::env::var("INTROSPECTION_TOKEN").ok();
        std::env::set_var("INTROSPECTION_TOKEN", "env-token");

        let processor = IntrospectionSpanProcessor::new(SpanProcessorConfig::default());
        assert!(processor.is_ok());

        if let Some(token) = old_token {
            std::env::set_var("INTROSPECTION_TOKEN", token);
        } else {
            std::env::remove_var("INTROSPECTION_TOKEN");
        }
    }

    /// Build a minimal `SpanData` carrying `attrs`.
    fn span_with(attrs: Vec<KeyValue>) -> SpanData {
        let (provider, _exporter) = crate::otel::testing::setup_test_provider();
        let tracer = provider.tracer("test");
        let mut span = tracer.span_builder("s").start(&tracer);
        for kv in attrs {
            span.set_attribute(kv);
        }
        span.end();
        let (_p, exporter) = (provider, _exporter);
        exporter.get_finished_spans().unwrap().pop().unwrap()
    }

    fn attr<'a>(span: &'a SpanData, key: &str) -> Option<&'a opentelemetry::Value> {
        span.attributes
            .iter()
            .find(|kv| kv.key.as_str() == key)
            .map(|kv| &kv.value)
    }

    #[test]
    fn test_infrastructure_spans_are_not_exported() {
        // The processor sits on the global provider, where every HTTP,
        // routing, and database span in the process also arrives.
        let infra = span_with(vec![
            KeyValue::new("http.request.method", "GET"),
            KeyValue::new("http.response.status_code", 200i64),
        ]);
        assert!(!should_export(&infra));

        for marker in GEN_AI_MARKERS {
            let llm = span_with(vec![KeyValue::new(marker, "x")]);
            assert!(
                should_export(&llm),
                "{marker} should mark a span for export"
            );
        }
    }

    /// Build a processor whose exporter goes nowhere; these tests only look
    /// at what `enrich` did to the span.
    fn enriching_processor() -> IntrospectionSpanProcessor {
        IntrospectionSpanProcessor::new(
            SpanProcessorConfig::builder()
                .service_name("enrich-test")
                .advanced(SpanProcessorAdvancedOptions {
                    span_exporter: Some(Arc::new(
                        SpanExporter::builder()
                            .with_http()
                            .with_endpoint("http://localhost:19876/v1/traces")
                            .build()
                            .unwrap(),
                    )),
                    ..Default::default()
                })
                .build(),
        )
        .unwrap()
    }

    /// Build a processor with the given `service_name` and a nowhere exporter.
    fn processor_named(service_name: Option<&str>) -> IntrospectionSpanProcessor {
        let mut config = SpanProcessorConfig::builder().advanced(SpanProcessorAdvancedOptions {
            span_exporter: Some(Arc::new(
                SpanExporter::builder()
                    .with_http()
                    .with_endpoint("http://localhost:19876/v1/traces")
                    .build()
                    .unwrap(),
            )),
            ..Default::default()
        });
        if let Some(name) = service_name {
            config = config.service_name(name);
        } else {
            // The constructor also reads this, and a set value would make the
            // unset case indistinguishable.
            unsafe { env::remove_var("INTROSPECTION_SERVICE_NAME") };
        }
        IntrospectionSpanProcessor::new(config.token("t").build()).unwrap()
    }

    fn service_name_of(resource: &opentelemetry_sdk::Resource) -> Option<String> {
        resource
            .iter()
            .find(|kv| kv.0.as_str() == "service.name")
            .map(|kv| kv.1.to_string())
    }

    #[test]
    fn test_an_unset_service_name_leaves_the_providers_resource_alone() {
        let caller = opentelemetry_sdk::Resource::builder()
            .with_service_name("checkout-api")
            .with_attribute(KeyValue::new("deployment.environment", "prod"))
            .build();

        // Unset: rewriting the label the caller chose would land every LLM
        // span at Introspection under the SDK's own default name.
        let unset = processor_named(None).resource_for(&caller);
        assert_eq!(service_name_of(&unset).as_deref(), Some("checkout-api"));

        // Set: the documented option still wins, and the caller's other
        // resource attributes survive.
        let named = processor_named(Some("explicit")).resource_for(&caller);
        assert_eq!(service_name_of(&named).as_deref(), Some("explicit"));
        assert!(named
            .iter()
            .any(|kv| kv.0.as_str() == "deployment.environment"));
    }

    #[test]
    fn test_conversation_id_falls_back_to_one_id_per_trace() {
        let processor = enriching_processor();

        // No baggage, no conversation attribute: the spans of one run would
        // otherwise reach Introspection with nothing to group them by.
        let mut first = span_with(vec![KeyValue::new(
            types::attr::GEN_AI_REQUEST_MODEL,
            "claude",
        )]);
        processor.enrich(&mut first);
        let minted = attr(&first, types::attr::CONVERSATION_ID)
            .map(|v| v.to_string())
            .expect("a conversation id");
        assert!(minted.starts_with("intro_conv_"), "got {minted}");

        // Same trace -> same id.
        let mut same_trace = first.clone();
        same_trace
            .attributes
            .retain(|kv| kv.key.as_str() != types::attr::CONVERSATION_ID);
        processor.enrich(&mut same_trace);
        assert_eq!(
            attr(&same_trace, types::attr::CONVERSATION_ID).map(|v| v.to_string()),
            Some(minted.clone())
        );

        // A different trace is a different conversation.
        let mut other = span_with(vec![KeyValue::new(
            types::attr::GEN_AI_REQUEST_MODEL,
            "claude",
        )]);
        processor.enrich(&mut other);
        assert_ne!(
            attr(&other, types::attr::CONVERSATION_ID).map(|v| v.to_string()),
            Some(minted)
        );
    }

    #[test]
    fn test_an_emitters_own_conversation_id_survives() {
        let processor = enriching_processor();
        let mut span = span_with(vec![
            KeyValue::new(types::attr::GEN_AI_REQUEST_MODEL, "claude"),
            KeyValue::new(types::attr::CONVERSATION_ID, "conv_from_emitter"),
        ]);
        processor.enrich(&mut span);
        assert_eq!(
            attr(&span, types::attr::CONVERSATION_ID).map(|v| v.to_string()),
            Some("conv_from_emitter".to_string())
        );
        // Exactly one: a minted id appended alongside the emitter's would be
        // ambiguous on the wire even though the first one still reads back.
        assert_eq!(
            span.attributes
                .iter()
                .filter(|kv| kv.key.as_str() == types::attr::CONVERSATION_ID)
                .count(),
            1
        );
    }

    #[test]
    fn test_enrich_projects_baggage_and_strips_the_deprecated_key() {
        use opentelemetry::baggage::BaggageExt;

        let processor = IntrospectionSpanProcessor::new(
            SpanProcessorConfig::builder()
                .service_name("enrich-test")
                .advanced(SpanProcessorAdvancedOptions {
                    span_exporter: Some(Arc::new(
                        SpanExporter::builder()
                            .with_http()
                            .with_endpoint("http://localhost:19876/v1/traces")
                            .build()
                            .unwrap(),
                    )),
                    ..Default::default()
                })
                .build(),
        )
        .unwrap();

        let mut span = span_with(vec![
            KeyValue::new(types::attr::GEN_AI_PROVIDER_NAME, "anthropic"),
            KeyValue::new(DEPRECATED_SYSTEM_KEY, "anthropic"),
            KeyValue::new(types::attr::GEN_AI_INPUT_MESSAGES, "[]"),
            // A default the emitter stamped; per-call baggage must win.
            KeyValue::new(types::attr::AGENT_NAME, "on-span"),
        ]);

        let cx = opentelemetry::Context::current().with_baggage(vec![
            KeyValue::new(types::baggage::CONVERSATION_ID, "conv_1"),
            KeyValue::new(types::baggage::AGENT_NAME, "from-baggage"),
            KeyValue::new(types::baggage::AGENT_ID, "agent_9"),
            KeyValue::new(types::baggage::USER_ID, "user_42"),
            KeyValue::new(types::baggage::ANONYMOUS_ID, "anon_7"),
        ]);
        let _guard = cx.attach();

        processor.enrich(&mut span);

        assert!(attr(&span, DEPRECATED_SYSTEM_KEY).is_none());
        assert_eq!(
            attr(&span, types::attr::CONVERSATION_ID).map(|v| v.to_string()),
            Some("conv_1".to_string())
        );
        assert_eq!(
            attr(&span, types::attr::AGENT_NAME).map(|v| v.to_string()),
            Some("from-baggage".to_string())
        );
        assert_eq!(
            attr(&span, types::attr::AGENT_ID).map(|v| v.to_string()),
            Some("agent_9".to_string())
        );
        // identify() has to reach the trace, not just the events.
        assert_eq!(
            attr(&span, types::attr::USER_ID).map(|v| v.to_string()),
            Some("user_42".to_string())
        );
        assert_eq!(
            attr(&span, types::attr::ANONYMOUS_ID).map(|v| v.to_string()),
            Some("anon_7".to_string())
        );
        // Messages present, operation unnamed: it is a chat completion.
        assert_eq!(
            attr(&span, types::attr::GEN_AI_OPERATION_NAME).map(|v| v.to_string()),
            Some("chat".to_string())
        );
        // Exactly one agent.name survived the override.
        assert_eq!(
            span.attributes
                .iter()
                .filter(|kv| kv.key.as_str() == types::attr::AGENT_NAME)
                .count(),
            1
        );
    }
}
