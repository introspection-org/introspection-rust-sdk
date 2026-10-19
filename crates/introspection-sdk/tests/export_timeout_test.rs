//! The configured export deadline is the one that applies.
//!
//! `BatchConfig::max_export_timeout` is `#[allow(dead_code)]` in
//! `opentelemetry_sdk` 0.32 outside the experimental async-runtime processor:
//! it is populated from `OTEL_BSP_EXPORT_TIMEOUT` and then never read. The
//! deadline that actually bounds an export is the exporter's own HTTP
//! timeout, which was hardcoded to 30s with no way to change it.
//!
//! This drives a collector that never answers in time, so a wrong wiring
//! shows up as a 30-second hang rather than a prompt failure.

use std::time::{Duration, Instant};

use introspection_sdk::otel::{
    IntrospectionSpanProcessor, SpanProcessorAdvancedOptions, SpanProcessorConfig,
};
use opentelemetry::trace::{Span as _, Tracer, TracerProvider as _};
use opentelemetry::KeyValue;
use opentelemetry_sdk::trace::SdkTracerProvider;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn the_configured_export_deadline_is_the_one_that_applies() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(20)))
        .mount(&server)
        .await;

    let processor = IntrospectionSpanProcessor::new(
        SpanProcessorConfig::with_token("intro_test").advanced(SpanProcessorAdvancedOptions {
            base_otel_url: Some(server.uri()),
            export_timeout_ms: Some(300),
            ..Default::default()
        }),
    )
    .unwrap();

    let provider = SdkTracerProvider::builder()
        .with_span_processor(processor)
        .build();
    let tracer = provider.tracer("t");
    let mut span = tracer
        .span_builder("chat")
        .with_attributes([KeyValue::new("gen_ai.request.model", "claude-haiku-4-5")])
        .start(&tracer);
    span.end();

    let started = Instant::now();
    let _ = provider.force_flush();
    let elapsed = started.elapsed();

    // The collector holds the response for 20s. With the deadline wired the
    // export gives up in well under a second; with it ignored the exporter
    // waits out its hardcoded 30s instead.
    assert!(
        elapsed < Duration::from_secs(5),
        "export ignored the 300ms deadline and waited {elapsed:?}"
    );
    let _ = provider.shutdown();
}
