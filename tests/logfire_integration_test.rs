//! Integration tests for the logfire + Introspection dual-export pipeline.
//!
//! Tests are organized into two groups:
//! - **Logfire tracing tests** (`test_logfire_tracing_pipeline`): Share a single logfire
//!   instance with a global tracing subscriber. These run as sub-sections of one test
//!   function to avoid global subscriber conflicts.
//! - **Dual-export tests**: Use a standalone `SdkTracerProvider` with `InMemorySpanExporter`.
//!   These are fully independent and can run in parallel.
//!
//! Run with: `cargo test --features logfire,testing,openai --test logfire_integration_test`
#![cfg(feature = "logfire")]

use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
};
use async_openai::Client;
use introspection_sdk::testing::{InMemorySpanExporter, SimpleSpanProcessor, SpanData};
use opentelemetry::trace::SpanId;
use std::collections::BTreeMap;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ============================================================
// Shared helpers
// ============================================================

/// Convert SpanData to JSON, filtering out dynamic logfire metadata
/// (code.*, thread.*, logfire.*, busy_ns, idle_ns) for deterministic snapshots.
fn span_to_stable_json(span: &SpanData) -> serde_json::Value {
    let mut map = BTreeMap::new();
    map.insert(
        "name".to_string(),
        serde_json::Value::String(span.name.to_string()),
    );
    map.insert(
        "span_kind".to_string(),
        serde_json::Value::String(format!("{:?}", span.span_kind)),
    );
    map.insert(
        "status".to_string(),
        serde_json::Value::String(format!("{:?}", span.status)),
    );

    // Identity fields
    map.insert(
        "trace_id".to_string(),
        serde_json::Value::String(span.span_context.trace_id().to_string()),
    );
    map.insert(
        "span_id".to_string(),
        serde_json::Value::String(span.span_context.span_id().to_string()),
    );
    if span.parent_span_id != SpanId::INVALID {
        map.insert(
            "parent_span_id".to_string(),
            serde_json::Value::String(span.parent_span_id.to_string()),
        );
    }

    // Timestamps (epoch nanos as string)
    map.insert(
        "start_time".to_string(),
        serde_json::Value::String(
            span.start_time
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .to_string(),
        ),
    );
    map.insert(
        "end_time".to_string(),
        serde_json::Value::String(
            span.end_time
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .to_string(),
        ),
    );

    let mut attrs = BTreeMap::new();
    for kv in &span.attributes {
        let key = kv.key.as_str();
        if key.starts_with("code.")
            || key.starts_with("thread.")
            || key.starts_with("logfire.")
            || key == "busy_ns"
            || key == "idle_ns"
        {
            continue;
        }
        attrs.insert(key.to_string(), format!("{}", kv.value));
    }
    map.insert(
        "attributes".to_string(),
        serde_json::to_value(attrs).unwrap(),
    );

    serde_json::to_value(map).unwrap()
}

fn spans_to_stable_json(spans: &[SpanData]) -> serde_json::Value {
    serde_json::Value::Array(spans.iter().map(span_to_stable_json).collect())
}

/// Filter completed logfire spans (logfire creates pending + completed pairs).
fn completed_logfire_spans(spans: &[SpanData]) -> Vec<SpanData> {
    spans
        .iter()
        .filter(|s| {
            s.attributes
                .iter()
                .any(|a| a.key.as_str() == "logfire.span_type" && a.value.as_str() == "span")
        })
        .cloned()
        .collect()
}

/// Create a wiremock-backed OpenAI client.
fn wiremock_openai_client(server: &MockServer) -> Client<OpenAIConfig> {
    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(server.uri());
    Client::with_config(config)
}

// ============================================================
// Logfire tracing tests (share a single global subscriber)
// ============================================================

/// All tests that rely on logfire's global tracing subscriber live here.
/// Each section resets the exporter before running its assertions.
#[tokio::test]
async fn test_logfire_tracing_pipeline() {
    use introspection_sdk::openai::tracing_traced_chat_completion;

    let exporter = InMemorySpanExporter::default();
    let processor = SimpleSpanProcessor::new(exporter.clone());

    let logfire = logfire::configure()
        .send_to_logfire(false)
        .with_console(None)
        .with_additional_span_processor(processor)
        .finish()
        .expect("logfire configuration should succeed");

    // --- 1. Generation span with gen_ai attributes ---
    {
        let _span = tracing::info_span!(
            "chat",
            "gen_ai.system" = "openai",
            "gen_ai.operation.name" = "chat",
            "gen_ai.request.model" = "gpt-4o-mini",
            "gen_ai.response.model" = "gpt-4o-mini",
            "gen_ai.response.id" = "chatcmpl-logfire1",
            "gen_ai.usage.input_tokens" = 5_i64,
            "gen_ai.usage.output_tokens" = 3_i64,
        )
        .entered();
    }

    let _ = logfire.force_flush();
    let spans = exporter.get_finished_spans().unwrap();
    let completed = completed_logfire_spans(&spans);
    assert!(
        !completed.is_empty(),
        "Should capture completed 'chat' span"
    );

    let span = &completed[0];
    let gen_ai_keys: Vec<_> = span
        .attributes
        .iter()
        .filter(|a| a.key.as_str().starts_with("gen_ai."))
        .map(|a| a.key.as_str())
        .collect();
    assert!(gen_ai_keys.contains(&"gen_ai.system"));
    assert!(gen_ai_keys.contains(&"gen_ai.request.model"));
    assert!(gen_ai_keys.contains(&"gen_ai.response.model"));
    assert!(gen_ai_keys.contains(&"gen_ai.usage.input_tokens"));

    let json = span_to_stable_json(span);
    insta::assert_json_snapshot!("logfire_generation_span", json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    // --- 2. Nested spans (parent-child) ---
    exporter.reset();

    {
        let _parent = tracing::info_span!("pipeline").entered();
        {
            let _child = tracing::info_span!(
                "chat",
                "gen_ai.system" = "anthropic",
                "gen_ai.request.model" = "claude-3.5-sonnet",
            )
            .entered();
        }
    }

    let _ = logfire.force_flush();
    let spans = exporter.get_finished_spans().unwrap();
    let completed = completed_logfire_spans(&spans);
    assert_eq!(
        completed.len(),
        2,
        "Should have pipeline + chat completed spans"
    );

    let child = completed
        .iter()
        .find(|s| s.name.as_ref() == "chat")
        .unwrap();
    let parent = completed
        .iter()
        .find(|s| s.name.as_ref() == "pipeline")
        .unwrap();
    assert_eq!(
        child.parent_span_id,
        parent.span_context.span_id(),
        "child should be nested under parent"
    );

    let json = spans_to_stable_json(&completed);
    insta::assert_json_snapshot!("logfire_nested_spans", json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    // --- 3. tracing_traced_chat_completion with wiremock ---
    exporter.reset();

    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("fixtures/chat_completion_response.json"),
            "application/json",
        ))
        .mount(&mock_server)
        .await;

    let client = wiremock_openai_client(&mock_server);
    let request = CreateChatCompletionRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: "Hello!".into(),
                ..Default::default()
            },
        )],
        ..Default::default()
    };

    let response = tracing_traced_chat_completion(&client, request)
        .await
        .unwrap();
    assert_eq!(response.model, "gpt-4o-mini");

    let _ = logfire.force_flush();
    let spans = exporter.get_finished_spans().unwrap();
    let completed = completed_logfire_spans(&spans);
    let chat_spans: Vec<_> = completed
        .iter()
        .filter(|s| s.name.as_ref() == "chat")
        .collect();
    assert_eq!(chat_spans.len(), 1, "Should have one 'chat' span");

    let json = span_to_stable_json(chat_spans[0]);
    insta::assert_json_snapshot!("logfire_traced_chat_completion", json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    // --- 4. tracing_traced_chat_completion tool call multi-turn ---
    exporter.reset();

    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("fixtures/tool_call_response.json"),
            "application/json",
        ))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("fixtures/tool_call_final_response.json"),
            "application/json",
        ))
        .mount(&mock_server)
        .await;

    let client = wiremock_openai_client(&mock_server);

    {
        let _pipeline = tracing::info_span!("pipeline").entered();

        let request1 = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: "What's the weather in San Francisco?".into(),
                    ..Default::default()
                },
            )],
            ..Default::default()
        };
        let _r1 = tracing_traced_chat_completion(&client, request1)
            .await
            .unwrap();

        let request2 = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: "What's the weather in San Francisco?".into(),
                    ..Default::default()
                },
            )],
            ..Default::default()
        };
        let _r2 = tracing_traced_chat_completion(&client, request2)
            .await
            .unwrap();
    }

    let _ = logfire.force_flush();
    let spans = exporter.get_finished_spans().unwrap();
    let completed = completed_logfire_spans(&spans);
    assert_eq!(completed.len(), 3, "Should have 2 chat + 1 pipeline spans");

    let pipeline = completed
        .iter()
        .find(|s| s.name.as_ref() == "pipeline")
        .unwrap();
    let chats: Vec<_> = completed
        .iter()
        .filter(|s| s.name.as_ref() == "chat")
        .collect();
    assert_eq!(chats.len(), 2);
    for chat in &chats {
        assert_eq!(
            chat.parent_span_id,
            pipeline.span_context.span_id(),
            "chat spans should be nested under pipeline"
        );
    }

    let json = spans_to_stable_json(&completed);
    insta::assert_json_snapshot!("logfire_traced_tool_call", json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    // --- 5. Error handling (500 response) ---
    exporter.reset();

    let mock_server = MockServer::start().await;
    // Use 401 instead of 500: async-openai retries on 5xx with exponential backoff,
    // which would cause the test to hang. 4xx errors are not retried.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_raw(
            r#"{"error":{"message":"Invalid API key","type":"authentication_error","param":null,"code":"invalid_api_key"}}"#,
            "application/json",
        ))
        .mount(&mock_server)
        .await;

    let client = wiremock_openai_client(&mock_server);
    let request = CreateChatCompletionRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: "Hello!".into(),
                ..Default::default()
            },
        )],
        ..Default::default()
    };

    let result = tracing_traced_chat_completion(&client, request).await;
    assert!(result.is_err(), "Should return error for 401 response");

    let _ = logfire.force_flush();
    let spans = exporter.get_finished_spans().unwrap();
    let completed = completed_logfire_spans(&spans);
    let chat_spans: Vec<_> = completed
        .iter()
        .filter(|s| s.name.as_ref() == "chat")
        .collect();
    assert_eq!(
        chat_spans.len(),
        1,
        "Should have one 'chat' span even on error"
    );

    let json = span_to_stable_json(chat_spans[0]);
    insta::assert_json_snapshot!("logfire_error_handling", json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    // --- Shutdown logfire ---
    let guard = logfire.shutdown_guard();
    guard.shutdown().expect("shutdown should succeed");
}

// ============================================================
// Dual-export tests (standalone SdkTracerProvider, no logfire)
// ============================================================

/// Observation API with standalone `SdkTracerProvider` + wiremock multi-turn.
#[tokio::test]
async fn test_logfire_observation_tool_call() {
    use introspection_sdk::{GenerationUpdate, Observation, ObservationConfig};
    use opentelemetry::trace::TracerProvider;

    let obs_exporter = InMemorySpanExporter::default();
    let obs_processor = SimpleSpanProcessor::new(obs_exporter.clone());
    let obs_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_span_processor(obs_processor)
        .build();
    let tracer = obs_provider.tracer("logfire-obs-test");

    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("fixtures/tool_call_response.json"),
            "application/json",
        ))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("fixtures/tool_call_final_response.json"),
            "application/json",
        ))
        .mount(&mock_server)
        .await;

    let client = wiremock_openai_client(&mock_server);

    {
        let _pipeline = Observation::start(&tracer, ObservationConfig::span("tool-call-pipeline"));

        let request1 = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: "What's the weather in San Francisco?".into(),
                    ..Default::default()
                },
            )],
            ..Default::default()
        };

        {
            use introspection_sdk::openai::convert_request_messages;
            let mut obs1 = Observation::start(
                &tracer,
                ObservationConfig::generation("chat", &request1.model)
                    .with_input(convert_request_messages(&request1.messages)),
            );
            let response1 = client.chat().create(request1).await.unwrap();
            let usage1 = response1.usage.as_ref().unwrap();
            obs1.update_generation(
                GenerationUpdate::new()
                    .with_response_model(&response1.model)
                    .with_response_id(&response1.id)
                    .with_usage(usage1.prompt_tokens as i64, usage1.completion_tokens as i64),
            );
            obs1.set_ok();
        }

        let request2 = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: "What's the weather in San Francisco?".into(),
                    ..Default::default()
                },
            )],
            ..Default::default()
        };

        {
            use introspection_sdk::openai::{convert_request_messages, convert_response_choices};
            let mut obs2 = Observation::start(
                &tracer,
                ObservationConfig::generation("chat", &request2.model)
                    .with_input(convert_request_messages(&request2.messages)),
            );
            let response2 = client.chat().create(request2).await.unwrap();
            let usage2 = response2.usage.as_ref().unwrap();
            obs2.update_generation(
                GenerationUpdate::new()
                    .with_response_model(&response2.model)
                    .with_response_id(&response2.id)
                    .with_output(convert_response_choices(&response2.choices))
                    .with_usage(usage2.prompt_tokens as i64, usage2.completion_tokens as i64),
            );
            obs2.set_ok();
        }
    }

    let obs_spans = obs_exporter.get_finished_spans().unwrap();
    assert_eq!(
        obs_spans.len(),
        3,
        "Expected 3 Observation spans (2 generations + 1 pipeline)"
    );

    let json = introspection_sdk::testing::spans_to_json(&obs_spans);
    insta::assert_json_snapshot!("logfire_observation_tool_call", json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    obs_provider.shutdown().unwrap();
}

/// Single `traced_chat_completion` (Observation-based) with standalone provider.
#[tokio::test]
async fn test_dual_export_observation_with_wiremock() {
    use introspection_sdk::openai::traced_chat_completion;
    use opentelemetry::trace::TracerProvider;

    let exporter = InMemorySpanExporter::default();
    let processor = SimpleSpanProcessor::new(exporter.clone());
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_span_processor(processor)
        .build();
    let tracer = provider.tracer("dual-export-test");

    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("fixtures/chat_completion_response.json"),
            "application/json",
        ))
        .mount(&mock_server)
        .await;

    let client = wiremock_openai_client(&mock_server);
    let request = CreateChatCompletionRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: "Hello!".into(),
                ..Default::default()
            },
        )],
        ..Default::default()
    };

    let response = traced_chat_completion(&tracer, &client, request)
        .await
        .unwrap();
    assert_eq!(response.model, "gpt-4o-mini");
    assert_eq!(
        response.choices[0].message.content.as_deref(),
        Some("Hello! How can I help you today?")
    );

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(
        spans.len(),
        1,
        "Expected 1 span from traced_chat_completion"
    );

    assert_eq!(
        format!("{:?}", spans[0].span_kind),
        "Client",
        "Observation generation spans should be SpanKind::Client"
    );

    let json = introspection_sdk::testing::span_data_to_json(&spans[0]);
    insta::assert_json_snapshot!("dual_export_observation", json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    provider.shutdown().unwrap();
}

/// Full tool call pipeline via Observation API with standalone provider.
#[tokio::test]
async fn test_dual_export_tool_call_pipeline() {
    use introspection_sdk::openai::traced_chat_completion;
    use introspection_sdk::{Observation, ObservationConfig};
    use opentelemetry::trace::TracerProvider;

    let exporter = InMemorySpanExporter::default();
    let processor = SimpleSpanProcessor::new(exporter.clone());
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_span_processor(processor)
        .build();
    let tracer = provider.tracer("dual-export-pipeline-test");

    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("fixtures/tool_call_response.json"),
            "application/json",
        ))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("fixtures/tool_call_final_response.json"),
            "application/json",
        ))
        .mount(&mock_server)
        .await;

    let client = wiremock_openai_client(&mock_server);

    {
        let _pipeline = Observation::start(&tracer, ObservationConfig::span("pipeline"));

        let request1 = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: "What's the weather in San Francisco?".into(),
                    ..Default::default()
                },
            )],
            ..Default::default()
        };
        let _r1 = traced_chat_completion(&tracer, &client, request1)
            .await
            .unwrap();

        let request2 = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: "What's the weather in San Francisco?".into(),
                    ..Default::default()
                },
            )],
            ..Default::default()
        };
        let _r2 = traced_chat_completion(&tracer, &client, request2)
            .await
            .unwrap();
    }

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(
        spans.len(),
        3,
        "Expected 3 spans (2 generations + 1 pipeline)"
    );

    let pipeline = spans
        .iter()
        .find(|s| s.name.as_ref() == "pipeline")
        .unwrap();
    let chats: Vec<_> = spans.iter().filter(|s| s.name.as_ref() == "chat").collect();
    assert_eq!(chats.len(), 2);
    for chat in &chats {
        assert_eq!(
            chat.parent_span_id,
            pipeline.span_context.span_id(),
            "chat spans should be nested under pipeline"
        );
    }

    let json = introspection_sdk::testing::spans_to_json(&spans);
    insta::assert_json_snapshot!("dual_export_tool_call_pipeline", json, {
        ".**.trace_id" => "[trace_id]",
        ".**.span_id" => "[span_id]",
        ".**.parent_span_id" => "[span_id]",
        ".**.start_time" => "[timestamp]",
        ".**.end_time" => "[timestamp]",
    });

    provider.shutdown().unwrap();
}
