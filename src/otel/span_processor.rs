//! Span processor that sends traces to the introspection API.

use opentelemetry_otlp::{SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::error::OTelSdkError;
use opentelemetry_sdk::trace::{
    BatchSpanProcessor, SimpleSpanProcessor, SpanData, SpanProcessor as OtelSpanProcessor,
};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info};

use crate::otel::types;
use crate::VERSION;

/// Create a `reqwest::blocking::Client` on a dedicated thread.
///
/// The blocking client spawns an internal tokio runtime, which panics
/// if constructed inside an existing async runtime.  Building it on a
/// short-lived thread avoids the "cannot drop a runtime …" issue.
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

    /// Custom span exporter (bypasses default OTLP exporter) — primarily
    /// for tests.
    pub span_exporter: Option<Arc<SpanExporter>>,

    /// Flush interval in milliseconds for the batch processor.
    /// Lower values reduce latency but increase network requests.
    /// Default: 5000
    pub flush_interval_ms: Option<u64>,

    /// Maximum batch size before auto-flush. Set to `1` for sequential
    /// (immediate) export — useful for multi-turn conversations.
    /// Default: uses the OTel SDK default.
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
/// where each turn must be ingested before the next arrives.
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
    _exporter: Arc<SpanExporter>,
}

impl IntrospectionSpanProcessor {
    /// Create a new IntrospectionSpanProcessor with the given configuration.
    pub fn new(config: SpanProcessorConfig) -> SpanProcessorResult<Self> {
        let advanced = config.advanced.unwrap_or_default();

        let token = config
            .token
            .or_else(|| env::var("INTROSPECTION_TOKEN").ok())
            .ok_or(SpanProcessorError::TokenRequired)?;

        let service_name = config
            .service_name
            .or_else(|| env::var("INTROSPECTION_SERVICE_NAME").ok())
            .unwrap_or_else(|| crate::types::defaults::SERVICE_NAME.to_string());

        // Note: BatchSpanProcessor::builder takes ownership of SpanExporter (not Arc)
        // So we need to handle custom exporters differently - we can't extract from Arc
        // For custom exporters, we'll create a dummy one for the processor since the real one
        // is stored in _exporter for reference
        let (exporter, exporter_for_processor): (Arc<SpanExporter>, SpanExporter) =
            if let Some(custom_exporter) = advanced.span_exporter {
                let dummy_exporter = SpanExporter::builder()
                    .with_http()
                    .with_http_client(new_blocking_http_client(Duration::from_secs(30)))
                    .with_endpoint("http://localhost/v1/traces")
                    .with_timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|e| SpanProcessorError::OpenTelemetry(e.to_string()))?;
                (custom_exporter.clone(), dummy_exporter)
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
                    service_name, endpoint
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

                let headers_clone = headers.clone();

                let http_client = new_blocking_http_client(Duration::from_secs(30));

                let exporter_for_processor = SpanExporter::builder()
                    .with_http()
                    .with_http_client(http_client)
                    .with_endpoint(&endpoint)
                    .with_headers(headers)
                    .with_timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|e| SpanProcessorError::OpenTelemetry(e.to_string()))?;

                let exporter = Arc::new(
                    SpanExporter::builder()
                        .with_http()
                        .with_http_client(new_blocking_http_client(Duration::from_secs(30)))
                        .with_endpoint(&endpoint)
                        .with_headers(headers_clone)
                        .with_timeout(Duration::from_secs(30))
                        .build()
                        .map_err(|e| SpanProcessorError::OpenTelemetry(e.to_string()))?,
                );

                (exporter, exporter_for_processor)
            };

        // Use SimpleSpanProcessor for sequential export when max_batch_size=1.
        // This ensures each span is exported immediately on end(), which is
        // required for multi-turn conversations where each turn must be
        // ingested before the next arrives.
        // Default to sequential export for dev/staging tokens.
        let max_batch_size = advanced.max_batch_size.or_else(|| {
            if token.starts_with("intro_dev") || token.starts_with("intro_staging") {
                Some(1)
            } else {
                None
            }
        });
        let flush_interval = Duration::from_millis(advanced.flush_interval_ms.unwrap_or(5000));
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
            _exporter: exporter,
        })
    }
}

impl OtelSpanProcessor for IntrospectionSpanProcessor {
    fn set_resource(&mut self, resource: &opentelemetry_sdk::Resource) {
        match &mut self.inner {
            InnerProcessor::Batch(p) => p.set_resource(resource),
            InnerProcessor::Simple(p) => p.set_resource(resource),
        }
    }

    fn on_start(&self, span: &mut opentelemetry_sdk::trace::Span, cx: &opentelemetry::Context) {
        debug!("Starting introspection span");
        match &self.inner {
            InnerProcessor::Batch(p) => p.on_start(span, cx),
            InnerProcessor::Simple(p) => p.on_start(span, cx),
        }
    }

    fn on_end(&self, span: SpanData) {
        debug!("Ending introspection span");
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

        let processor = IntrospectionSpanProcessor::new(
            SpanProcessorConfig::with_token("test-token").advanced(SpanProcessorAdvancedOptions {
                span_exporter: Some(test_exporter),
                ..Default::default()
            }),
        )
        .unwrap();

        assert!(processor.force_flush().is_ok());
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
}
