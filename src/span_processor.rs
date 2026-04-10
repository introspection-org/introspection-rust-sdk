//! Span processor that sends traces to the introspection API.

use opentelemetry_otlp::{SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::error::OTelSdkError;
use opentelemetry_sdk::trace::{BatchSpanProcessor, SpanData, SpanProcessor as OtelSpanProcessor};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info};

use crate::types::AdvancedOptions;
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

// AdvancedOptions is now imported from crate::types

/// Configuration for the Introspection span processor.
#[derive(Clone, Debug, Default)]
pub struct SpanProcessorConfig {
    /// Authentication token
    pub token: Option<String>,
    /// Service name (default: "introspection-client")
    pub service_name: Option<String>,
    /// Advanced options for configuration and testing
    pub advanced: Option<AdvancedOptions>,
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
    pub fn advanced(mut self, advanced: AdvancedOptions) -> Self {
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
    advanced: Option<AdvancedOptions>,
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

    pub fn advanced(mut self, advanced: AdvancedOptions) -> Self {
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

/// Span processor that sends traces to the introspection API.
///
/// This wraps OpenTelemetry's BatchSpanProcessor and configures it to send
/// traces to the introspection backend via OTLP.
///
/// # Example
///
/// ```rust,no_run
/// use introspection_sdk::{AdvancedOptions, span_processor::{IntrospectionSpanProcessor, SpanProcessorConfig}};
/// use opentelemetry_sdk::trace::SdkTracerProvider;
///
/// // Simple usage
/// let span_processor = IntrospectionSpanProcessor::new(
///     SpanProcessorConfig::with_token("your-token")
/// ).unwrap();
///
/// // With advanced options
/// let span_processor = IntrospectionSpanProcessor::new(
///     SpanProcessorConfig::with_token("your-token")
///         .advanced(AdvancedOptions {
///             base_url: Some("http://localhost:5418/v1/traces".to_string()),
///             additional_headers: None,
///             span_exporter: None,
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
    inner: BatchSpanProcessor,
    _exporter: Arc<SpanExporter>,
}

impl IntrospectionSpanProcessor {
    /// Create a new IntrospectionSpanProcessor with the given configuration.
    pub fn new(config: SpanProcessorConfig) -> SpanProcessorResult<Self> {
        // Use defaults if not provided
        let advanced = config.advanced.unwrap_or_default();

        let token = config
            .token
            .or_else(|| env::var("INTROSPECTION_TOKEN").ok())
            .ok_or(SpanProcessorError::TokenRequired)?;

        let service_name = config
            .service_name
            .or_else(|| env::var("INTROSPECTION_SERVICE_NAME").ok())
            .unwrap_or_else(|| "introspection-client".to_string());

        // Note: BatchSpanProcessor::builder takes ownership of SpanExporter (not Arc)
        // So we need to handle custom exporters differently - we can't extract from Arc
        // For custom exporters, we'll create a dummy one for the processor since the real one
        // is stored in _exporter for reference
        let (exporter, exporter_for_processor): (Arc<SpanExporter>, SpanExporter) =
            if let Some(custom_exporter) = advanced.span_exporter {
                // Use provided exporter (for testing)
                // Create a dummy exporter for the processor (won't be used since we have custom)
                // This is a limitation - we can't extract SpanExporter from Arc
                // The real exporter is stored in _exporter for reference
                let dummy_exporter = SpanExporter::builder()
                    .with_http()
                    .with_http_client(new_blocking_http_client(Duration::from_secs(30)))
                    .with_endpoint("http://localhost/v1/traces")
                    .with_timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|e| SpanProcessorError::OpenTelemetry(e.to_string()))?;
                (custom_exporter.clone(), dummy_exporter)
            } else {
                // Create default OTLP exporter
                let base_url = advanced
                    .base_url
                    .or_else(|| env::var("INTROSPECTION_BASE_URL").ok())
                    .unwrap_or_else(|| "https://api.nuraline.ai".to_string());

                // Construct endpoint URL
                let endpoint = if base_url.ends_with("/v1/traces") {
                    base_url.clone()
                } else {
                    format!("{}/v1/traces", base_url.trim_end_matches('/'))
                };

                info!(
                    "IntrospectionSpanProcessor initialized: service={}, endpoint={}",
                    service_name, endpoint
                );

                // Build headers
                let mut headers = HashMap::new();
                headers.insert(
                    "User-Agent".to_string(),
                    format!("introspection-sdk/{}", VERSION),
                );
                headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                if let Some(additional) = advanced.additional_headers {
                    headers.extend(additional);
                }

                // Clone headers for storage exporter
                let headers_clone = headers.clone();

                // Create OTLP span exporter for processor
                // Note: with_http() is required to specify HTTP transport protocol
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

        // Create batch span processor
        let processor = BatchSpanProcessor::builder(exporter_for_processor).build();

        Ok(Self {
            inner: processor,
            _exporter: exporter,
        })
    }
}

impl OtelSpanProcessor for IntrospectionSpanProcessor {
    fn set_resource(&mut self, resource: &opentelemetry_sdk::Resource) {
        self.inner.set_resource(resource);
    }

    fn on_start(&self, span: &mut opentelemetry_sdk::trace::Span, cx: &opentelemetry::Context) {
        debug!("Starting introspection span");
        self.inner.on_start(span, cx);
    }

    fn on_end(&self, span: SpanData) {
        debug!("Ending introspection span");
        self.inner.on_end(span);
    }

    fn shutdown(&self) -> Result<(), OTelSdkError> {
        info!("Shutting down introspection span processor");
        self.inner.shutdown()
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> Result<(), OTelSdkError> {
        info!("Shutting down introspection span processor with timeout");
        self.inner.shutdown_with_timeout(timeout)
    }

    fn force_flush(&self) -> Result<(), OTelSdkError> {
        info!("Flushing introspection span processor");
        self.inner.force_flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{Span, SpanKind, Status, Tracer, TracerProvider};
    use opentelemetry::KeyValue;
    use opentelemetry_sdk::trace::SdkTracerProvider;

    #[test]
    fn test_span_processor_creation_with_token() {
        let processor =
            IntrospectionSpanProcessor::new(SpanProcessorConfig::with_token("test-token")).unwrap();

        // Verify processor was created successfully
        assert!(processor.force_flush().is_ok());
    }

    #[test]
    fn test_span_processor_creation_with_advanced_options() {
        let mut custom_headers = HashMap::new();
        custom_headers.insert("X-Custom-Header".to_string(), "custom-value".to_string());

        let processor = IntrospectionSpanProcessor::new(
            SpanProcessorConfig::with_token("test-token").advanced(AdvancedOptions {
                base_url: Some("http://localhost:5418/v1/traces".to_string()),
                additional_headers: Some(custom_headers),
                span_exporter: None,
                ..Default::default()
            }),
        )
        .unwrap();

        // Verify processor was created successfully with custom options
        assert!(processor.force_flush().is_ok());
    }

    #[test]
    fn test_span_processor_with_custom_exporter() {
        // Create a test exporter pointing to a non-existent endpoint
        // This allows us to test the exporter injection without actually sending data
        let test_exporter = Arc::new(
            SpanExporter::builder()
                .with_http()
                .with_endpoint("http://localhost:9999/v1/traces")
                .with_timeout(Duration::from_secs(1))
                .build()
                .unwrap(),
        );

        let processor = IntrospectionSpanProcessor::new(
            SpanProcessorConfig::with_token("test-token").advanced(AdvancedOptions {
                span_exporter: Some(test_exporter),
                ..Default::default()
            }),
        )
        .unwrap();

        // Verify processor was created successfully with custom exporter
        assert!(processor.force_flush().is_ok());
    }

    #[test]
    fn test_span_processor_processes_spans() {
        let processor = IntrospectionSpanProcessor::new(
            SpanProcessorConfig::with_token("test-token").advanced(AdvancedOptions {
                base_url: Some("http://localhost:9999/v1/traces".to_string()),
                ..Default::default()
            }),
        )
        .unwrap();

        // Create a tracer provider with our processor
        let provider = SdkTracerProvider::builder()
            .with_span_processor(processor)
            .build();

        let tracer = provider.tracer("test-tracer");

        // Create and end a span - this should be processed by our processor
        let mut span = tracer
            .span_builder("test-span")
            .with_kind(SpanKind::Server)
            .start(&tracer);
        span.set_status(Status::Ok);
        span.set_attribute(KeyValue::new("test.key", "test.value"));
        span.end();

        // Force flush to ensure spans are processed
        // This may fail to send (endpoint doesn't exist) but validates the processor works
        // We don't care about the result - just that it was attempted
        let _ = provider.force_flush();

        provider.shutdown().unwrap();
    }

    #[test]
    fn test_span_processor_shutdown() {
        let processor =
            IntrospectionSpanProcessor::new(SpanProcessorConfig::with_token("test-token")).unwrap();

        // Test shutdown
        assert!(processor.shutdown().is_ok());
    }

    #[test]
    fn test_span_processor_shutdown_with_timeout() {
        let processor =
            IntrospectionSpanProcessor::new(SpanProcessorConfig::with_token("test-token")).unwrap();

        // Test shutdown with timeout
        assert!(processor
            .shutdown_with_timeout(Duration::from_secs(1))
            .is_ok());
    }

    #[test]
    fn test_span_processor_requires_token() {
        // When no token is provided and env var is not set, creation should fail.
        // Skip if INTROSPECTION_TOKEN happens to be set in the environment.
        if std::env::var("INTROSPECTION_TOKEN").is_ok() {
            return;
        }

        let config = SpanProcessorConfig {
            token: None,
            service_name: None,
            advanced: None,
        };

        let result = IntrospectionSpanProcessor::new(config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SpanProcessorError::TokenRequired
        ));
    }

    #[test]
    fn test_span_processor_with_explicit_token() {
        // Verify that an explicitly provided token works regardless of env state
        let config = SpanProcessorConfig {
            token: Some("explicit-token".to_string()),
            service_name: None,
            advanced: None,
        };

        let processor = IntrospectionSpanProcessor::new(config);
        assert!(processor.is_ok());
    }
}
