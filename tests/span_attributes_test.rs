use introspection_sdk::otel::testing::{setup_test_provider, span_data_to_json, spans_to_json};
use opentelemetry::trace::{Span, SpanKind, Status, Tracer, TracerProvider};
use opentelemetry::KeyValue;

// ---------------------------------------------------------------------------
// Test 1: Basic span capture
// ---------------------------------------------------------------------------
#[test]
fn test_basic_span_capture() {
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    let mut span = tracer
        .span_builder("test-span")
        .with_kind(SpanKind::Internal)
        .start(&tracer);
    span.set_attribute(KeyValue::new("key", "value"));
    span.end();

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].name.as_ref(), "test-span");

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 2: gen_ai span attributes snapshot
// ---------------------------------------------------------------------------
#[test]
fn test_gen_ai_span_attributes_snapshot() {
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    let mut span = tracer
        .span_builder("chat")
        .with_kind(SpanKind::Client)
        .start(&tracer);

    span.set_attribute(KeyValue::new("gen_ai.system", "openai"));
    span.set_attribute(KeyValue::new("gen_ai.request.model", "gpt-4o-mini"));
    span.set_attribute(KeyValue::new("gen_ai.operation.name", "chat"));
    span.set_attribute(KeyValue::new("gen_ai.response.model", "gpt-4o-mini"));
    span.set_attribute(KeyValue::new("gen_ai.response.id", "chatcmpl-test123"));
    span.set_attribute(KeyValue::new("gen_ai.usage.input_tokens", 12i64));
    span.set_attribute(KeyValue::new("gen_ai.usage.output_tokens", 8i64));
    span.set_attribute(KeyValue::new(
        "gen_ai.input.messages",
        serde_json::json!([{"role": "user", "content": "Say hello"}]).to_string(),
    ));
    span.set_attribute(KeyValue::new(
        "gen_ai.output.messages",
        serde_json::json!([{"role": "assistant", "content": "Hello! How can I help you today?"}])
            .to_string(),
    ));
    span.set_status(Status::Ok);
    span.end();

    let spans = exporter.get_finished_spans().unwrap();
    let json = span_data_to_json(&spans[0]);
    insta::assert_json_snapshot!(json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 3: gen_ai attributes with wiremock (async, end-to-end)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_gen_ai_attributes_with_wiremock() {
    use async_openai::{config::OpenAIConfig, Client};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Start mock server
    let mock_server = MockServer::start().await;

    // Load fixture
    let fixture = include_str!("fixtures/chat_completion_response.json");

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(fixture, "application/json"))
        .mount(&mock_server)
        .await;

    // Configure async-openai to use mock server
    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(mock_server.uri());
    let openai_client = Client::with_config(config);

    // Set up tracing
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    // Create span & make API call
    let mut span = tracer
        .span_builder("chat")
        .with_kind(SpanKind::Client)
        .start(&tracer);

    span.set_attribute(KeyValue::new("gen_ai.system", "openai"));
    span.set_attribute(KeyValue::new("gen_ai.request.model", "gpt-4o-mini"));
    span.set_attribute(KeyValue::new("gen_ai.operation.name", "chat"));
    span.set_attribute(KeyValue::new(
        "gen_ai.input.messages",
        serde_json::json!([{"role": "user", "content": "Say hello"}]).to_string(),
    ));

    // Make the actual API call against mock
    use async_openai::types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
    };

    let request = CreateChatCompletionRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: "Say hello".into(),
                ..Default::default()
            },
        )],
        ..Default::default()
    };

    let response = openai_client.chat().create(request).await.unwrap();

    // Set response attributes from actual response
    span.set_attribute(KeyValue::new(
        "gen_ai.response.model",
        response.model.clone(),
    ));
    span.set_attribute(KeyValue::new("gen_ai.response.id", response.id.clone()));
    if let Some(usage) = &response.usage {
        span.set_attribute(KeyValue::new(
            "gen_ai.usage.input_tokens",
            usage.prompt_tokens as i64,
        ));
        span.set_attribute(KeyValue::new(
            "gen_ai.usage.output_tokens",
            usage.completion_tokens as i64,
        ));
    }
    let output_content = response.choices[0].message.content.as_deref().unwrap_or("");
    span.set_attribute(KeyValue::new(
        "gen_ai.output.messages",
        serde_json::json!([{"role": "assistant", "content": output_content}]).to_string(),
    ));
    span.set_status(Status::Ok);
    span.end();

    let spans = exporter.get_finished_spans().unwrap();
    let json = span_data_to_json(&spans[0]);
    insta::assert_json_snapshot!(json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 4: IntrospectionSpanProcessor with provider (smoke test)
// ---------------------------------------------------------------------------
#[test]
fn test_introspection_span_processor_with_provider() {
    use introspection_sdk::otel::{
        IntrospectionSpanProcessor, SpanProcessorAdvancedOptions, SpanProcessorConfig,
    };
    use opentelemetry_sdk::trace::SdkTracerProvider;

    let processor = IntrospectionSpanProcessor::new(
        SpanProcessorConfig::with_token("test-token").advanced(SpanProcessorAdvancedOptions {
            base_otel_url: Some("http://localhost:19876".to_string()),
            ..Default::default()
        }),
    )
    .unwrap();

    let provider = SdkTracerProvider::builder()
        .with_span_processor(processor)
        .build();

    let tracer = provider.tracer("test");
    let mut span = tracer
        .span_builder("smoke-test")
        .with_kind(SpanKind::Internal)
        .start(&tracer);
    span.set_attribute(KeyValue::new("gen_ai.system", "openai"));
    span.end();

    // Force flush — may fail (no server) but should not panic
    let _ = provider.force_flush();
    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 5: Multiple spans capture
// ---------------------------------------------------------------------------
#[test]
fn test_multiple_spans_capture() {
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    for i in 0..3 {
        let mut span = tracer
            .span_builder(format!("span-{i}"))
            .with_kind(SpanKind::Internal)
            .start(&tracer);
        span.set_attribute(KeyValue::new("index", i as i64));
        span.end();
    }

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 3);

    let json = spans_to_json(&spans);
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 3);

    // Verify each span has the right index attribute
    for (i, entry) in arr.iter().enumerate() {
        let idx = entry["attributes"]["index"].as_i64().unwrap();
        assert_eq!(idx, i as i64);
    }

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 6: gen_ai attributes complete check
// ---------------------------------------------------------------------------
#[test]
fn test_gen_ai_attributes_complete_check() {
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    let mut span = tracer
        .span_builder("chat")
        .with_kind(SpanKind::Client)
        .start(&tracer);

    span.set_attribute(KeyValue::new("gen_ai.system", "openai"));
    span.set_attribute(KeyValue::new("gen_ai.request.model", "gpt-4o-mini"));
    span.set_attribute(KeyValue::new("gen_ai.operation.name", "chat"));
    span.set_attribute(KeyValue::new("gen_ai.response.model", "gpt-4o-mini"));
    span.set_attribute(KeyValue::new("gen_ai.response.id", "chatcmpl-test123"));
    span.set_attribute(KeyValue::new("gen_ai.usage.input_tokens", 12i64));
    span.set_attribute(KeyValue::new("gen_ai.usage.output_tokens", 8i64));
    span.set_attribute(KeyValue::new("gen_ai.input.messages", "[]"));
    span.set_attribute(KeyValue::new("gen_ai.output.messages", "[]"));
    span.set_status(Status::Ok);
    span.end();

    let spans = exporter.get_finished_spans().unwrap();
    let json = span_data_to_json(&spans[0]);
    let attrs = json["attributes"].as_object().unwrap();

    let expected_keys = [
        "gen_ai.system",
        "gen_ai.request.model",
        "gen_ai.operation.name",
        "gen_ai.response.model",
        "gen_ai.response.id",
        "gen_ai.usage.input_tokens",
        "gen_ai.usage.output_tokens",
        "gen_ai.input.messages",
        "gen_ai.output.messages",
    ];

    for key in expected_keys {
        assert!(attrs.contains_key(key), "Missing expected attribute: {key}");
    }

    provider.shutdown().unwrap();
}
