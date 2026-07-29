#![cfg(all(feature = "otel", feature = "openai"))]

use async_openai::config::OpenAIConfig;
use async_openai::types::embeddings::CreateEmbeddingRequestArgs;
use async_openai::Client;
use introspection_sdk::otel::openai::traced_embeddings_create;
use introspection_sdk::otel::testing::{setup_test_provider, span_data_to_json};
use opentelemetry::trace::TracerProvider;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn traced_embeddings_capture_usage_without_content_or_vectors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .and(body_json(serde_json::json!({
            "model": "text-embedding-3-small",
            "input": ["private observation text"],
            "dimensions": 4,
            "encoding_format": "float"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "object": "list",
            "data": [{
                "object": "embedding",
                "embedding": [0.1, 0.2, 0.3, 0.4],
                "index": 0
            }],
            "model": "text-embedding-3-small",
            "usage": {"prompt_tokens": 8, "total_tokens": 8}
        })))
        .mount(&server)
        .await;

    let client = Client::with_config(
        OpenAIConfig::new()
            .with_api_key("test-key")
            .with_api_base(server.uri()),
    );
    let request = CreateEmbeddingRequestArgs::default()
        .model("text-embedding-3-small")
        .input(vec!["private observation text"])
        .dimensions(4_u32)
        .encoding_format(async_openai::types::embeddings::EncodingFormat::Float)
        .build()
        .unwrap();

    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("embedding-test");
    let response = traced_embeddings_create(&tracer, &client, request)
        .await
        .unwrap();

    assert_eq!(response.usage.prompt_tokens, 8);
    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 1);
    let span = span_data_to_json(&spans[0]);
    let attributes = span["attributes"].as_object().unwrap();
    assert_eq!(span["name"], "embeddings text-embedding-3-small");
    assert_eq!(attributes["gen_ai.operation.name"], "embeddings");
    assert_eq!(attributes["gen_ai.provider.name"], "openai");
    assert_eq!(attributes["gen_ai.request.model"], "text-embedding-3-small");
    assert_eq!(
        attributes["gen_ai.response.model"],
        "text-embedding-3-small"
    );
    assert_eq!(attributes["gen_ai.usage.input_tokens"], 8);
    assert_eq!(attributes["gen_ai.embeddings.dimension.count"], 4);
    assert!(!attributes.contains_key("gen_ai.usage.output_tokens"));
    assert!(!attributes.contains_key("gen_ai.input.messages"));
    assert!(!attributes.contains_key("gen_ai.output.messages"));
    let serialized = serde_json::to_string(attributes).unwrap();
    assert!(!serialized.contains("private observation text"));
    assert!(!serialized.contains("0.1"));

    provider.shutdown().unwrap();
}
