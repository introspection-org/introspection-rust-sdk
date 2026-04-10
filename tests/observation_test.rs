use introspection_sdk::messages::{ContentPart, InputMessage, OutputMessage};
use introspection_sdk::testing::{setup_test_provider, span_data_to_json, spans_to_json};
use introspection_sdk::{GenerationUpdate, Observation, ObservationConfig};
use opentelemetry::trace::{SpanId, TracerProvider};

#[cfg(feature = "openai")]
use introspection_sdk::openai::traced_chat_completion_stream;

// ---------------------------------------------------------------------------
// Test 1: Generation observation sets request attributes
// ---------------------------------------------------------------------------
#[test]
fn test_generation_observation_sets_request_attributes() {
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    {
        let _obs = Observation::start(
            &tracer,
            ObservationConfig::generation("chat", "gpt-4o-mini")
                .with_input(vec![InputMessage::user("Say hello")]),
        );
    } // obs dropped → span ended

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 1);
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
// Test 2: Generation observation with update (request + response attrs)
// ---------------------------------------------------------------------------
#[test]
fn test_generation_observation_with_update() {
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    {
        let mut obs = Observation::start(
            &tracer,
            ObservationConfig::generation("chat", "gpt-4o-mini")
                .with_input(vec![InputMessage::user("Say hello")]),
        );

        obs.update_generation(
            GenerationUpdate::new()
                .with_response_model("gpt-4o-mini")
                .with_response_id("chatcmpl-test123")
                .with_output(vec![OutputMessage::assistant(
                    "Hello! How can I help you today?",
                )])
                .with_usage(12, 8),
        );
        obs.set_ok();
    }

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 1);
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
// Test 3: Span observation (general pipeline step)
// ---------------------------------------------------------------------------
#[test]
fn test_span_observation() {
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    {
        let _obs = Observation::start(&tracer, ObservationConfig::span("retrieval-step"));
    }

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 1);
    let json = span_data_to_json(&spans[0]);

    // Span type should be Internal, no gen_ai attrs, has "input" attr
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
// Test 4: Observation nesting (parent → child via context)
// ---------------------------------------------------------------------------
#[test]
fn test_observation_nesting() {
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    {
        let _parent = Observation::start(&tracer, ObservationConfig::span("pipeline"));
        {
            let mut child = Observation::start(
                &tracer,
                ObservationConfig::generation("chat", "gpt-4o-mini"),
            );
            child.set_ok();
        } // child dropped
    } // parent dropped

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 2, "Expected parent + child spans");

    // Child is ended first, so it appears first in the exporter
    let child = &spans[0];
    let parent = &spans[1];

    assert_eq!(child.name.as_ref(), "chat");
    assert_eq!(parent.name.as_ref(), "pipeline");

    // Verify parent-child relationship
    let parent_span_id = parent.span_context.span_id();
    assert_ne!(
        parent_span_id,
        SpanId::INVALID,
        "parent should have a valid span_id"
    );
    assert_eq!(
        child.parent_span_id, parent_span_id,
        "child's parent_span_id should match parent's span_id"
    );

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 5: Auto system inference
// ---------------------------------------------------------------------------
#[test]
fn test_observation_auto_system_inference() {
    use introspection_sdk::observation::infer_system;

    assert_eq!(infer_system("gpt-4o-mini"), Some("openai".to_string()));
    assert_eq!(infer_system("gpt-4"), Some("openai".to_string()));
    assert_eq!(infer_system("o1-preview"), Some("openai".to_string()));
    assert_eq!(infer_system("claude-3-opus"), Some("anthropic".to_string()));
    assert_eq!(
        infer_system("claude-3.5-sonnet"),
        Some("anthropic".to_string())
    );
    assert_eq!(infer_system("gemini-1.5-pro"), Some("google".to_string()));
    assert_eq!(infer_system("mistral-large"), Some("mistral".to_string()));
    assert_eq!(infer_system("llama-3.1-70b"), Some("meta".to_string()));
    assert_eq!(infer_system("command-r-plus"), Some("cohere".to_string()));
    assert_eq!(infer_system("custom-model"), None);
}

// ---------------------------------------------------------------------------
// Test 6: Observation drop ends span
// ---------------------------------------------------------------------------
#[test]
fn test_observation_drop_ends_span() {
    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    // No spans before creation
    assert_eq!(exporter.get_finished_spans().unwrap().len(), 0);

    {
        let _obs = Observation::start(
            &tracer,
            ObservationConfig::generation("chat", "gpt-4o-mini"),
        );
        // Span should not be finished yet
        assert_eq!(exporter.get_finished_spans().unwrap().len(), 0);
    }
    // After drop, span should be finished
    assert_eq!(exporter.get_finished_spans().unwrap().len(), 1);

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 7: Generation with wiremock (end-to-end)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_generation_with_wiremock() {
    use async_openai::config::OpenAIConfig;
    use async_openai::types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
    };
    use async_openai::Client;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;
    let fixture = include_str!("fixtures/chat_completion_response.json");

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(fixture, "application/json"))
        .mount(&mock_server)
        .await;

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(mock_server.uri());
    let openai_client = Client::with_config(config);

    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

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

    {
        use introspection_sdk::openai::convert_request_messages;
        let mut obs = Observation::start(
            &tracer,
            ObservationConfig::generation("chat", &request.model)
                .with_input(convert_request_messages(&request.messages)),
        );

        let response = openai_client.chat().create(request).await.unwrap();

        let output_messages: Vec<OutputMessage> = response
            .choices
            .iter()
            .map(|c| OutputMessage::assistant(c.message.content.as_deref().unwrap_or("")))
            .collect();

        let usage = response.usage.as_ref().unwrap();
        obs.update_generation(
            GenerationUpdate::new()
                .with_response_model(&response.model)
                .with_response_id(&response.id)
                .with_output(output_messages)
                .with_usage(usage.prompt_tokens as i64, usage.completion_tokens as i64),
        );
        obs.set_ok();
    }

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 1);
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
// Test 8: Tool call with wiremock (multi-turn)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_tool_call_with_wiremock() {
    use async_openai::config::OpenAIConfig;
    use async_openai::types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
    };
    use async_openai::Client;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    // First response: model requests a tool call
    let tool_call_fixture = include_str!("fixtures/tool_call_response.json");
    // Second response: final answer after tool result
    let final_fixture = include_str!("fixtures/tool_call_final_response.json");

    // Mount responses in order
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(tool_call_fixture, "application/json"),
        )
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(final_fixture, "application/json"))
        .mount(&mock_server)
        .await;

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(mock_server.uri());
    let openai_client = Client::with_config(config);

    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

    {
        // Outer pipeline span
        let _pipeline = Observation::start(&tracer, ObservationConfig::span("tool-call-pipeline"));

        // First LLM call — model requests tool call
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
            use introspection_sdk::openai::{convert_request_messages, convert_response_choices};
            let mut obs1 = Observation::start(
                &tracer,
                ObservationConfig::generation("chat", &request1.model)
                    .with_input(convert_request_messages(&request1.messages)),
            );

            let response1 = openai_client.chat().create(request1).await.unwrap();
            let usage1 = response1.usage.as_ref().unwrap();

            obs1.update_generation(
                GenerationUpdate::new()
                    .with_response_model(&response1.model)
                    .with_response_id(&response1.id)
                    .with_output(convert_response_choices(&response1.choices))
                    .with_usage(usage1.prompt_tokens as i64, usage1.completion_tokens as i64),
            );
            obs1.set_ok();
        }

        // Second LLM call — final response after tool result
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

            let response2 = openai_client.chat().create(request2).await.unwrap();
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
    } // pipeline span dropped

    let spans = exporter.get_finished_spans().unwrap();
    // Should have: 2 generation spans + 1 pipeline span = 3
    assert_eq!(
        spans.len(),
        3,
        "Expected 3 spans (2 generations + 1 pipeline)"
    );

    let json = spans_to_json(&spans);
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
// Test 9: Streaming generation with wiremock (end-to-end)
// ---------------------------------------------------------------------------
#[cfg(feature = "openai")]
#[tokio::test]
async fn test_streaming_generation_with_wiremock() {
    use async_openai::config::OpenAIConfig;
    use async_openai::types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
    };
    use async_openai::Client;
    use futures::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;
    let fixture = include_str!("fixtures/streaming_chat_completion.txt");

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(fixture, "text/event-stream"))
        .mount(&mock_server)
        .await;

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(mock_server.uri());
    let openai_client = Client::with_config(config);

    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

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

    {
        let mut stream = traced_chat_completion_stream(&tracer, &openai_client, request)
            .await
            .unwrap();

        while let Some(result) = stream.next().await {
            let _chunk = result.unwrap();
        }
    }

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 1);
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
// Test 10: Streaming generation — early drop still ends span
// ---------------------------------------------------------------------------
#[cfg(feature = "openai")]
#[tokio::test]
async fn test_streaming_generation_early_drop() {
    use async_openai::config::OpenAIConfig;
    use async_openai::types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
    };
    use async_openai::Client;
    use futures::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;
    let fixture = include_str!("fixtures/streaming_chat_completion.txt");

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(fixture, "text/event-stream"))
        .mount(&mock_server)
        .await;

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(mock_server.uri());
    let openai_client = Client::with_config(config);

    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

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

    {
        let mut stream = traced_chat_completion_stream(&tracer, &openai_client, request)
            .await
            .unwrap();

        // Consume only 3 chunks then drop
        for _ in 0..3 {
            let _ = stream.next().await;
        }
        // stream is dropped here — observation should still be finalized
    }

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 1, "Span should be ended even after early drop");

    // Verify it has a valid name and the partial content was captured
    assert_eq!(spans[0].name.as_ref(), "chat");

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 11: Streaming generation nested under parent observation
// ---------------------------------------------------------------------------
#[cfg(feature = "openai")]
#[tokio::test]
async fn test_streaming_generation_nested() {
    use async_openai::config::OpenAIConfig;
    use async_openai::types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
    };
    use async_openai::Client;
    use futures::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;
    let fixture = include_str!("fixtures/streaming_chat_completion.txt");

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(fixture, "text/event-stream"))
        .mount(&mock_server)
        .await;

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(mock_server.uri());
    let openai_client = Client::with_config(config);

    let (provider, exporter) = setup_test_provider();
    let tracer = provider.tracer("test");

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

    {
        let _parent = Observation::start(&tracer, ObservationConfig::span("streaming-pipeline"));

        {
            let mut stream = traced_chat_completion_stream(&tracer, &openai_client, request)
                .await
                .unwrap();

            while let Some(result) = stream.next().await {
                let _chunk = result.unwrap();
            }
        }
    }

    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 2, "Expected parent + child spans");

    // Child (streaming generation) ends first
    let child = &spans[0];
    let parent = &spans[1];

    assert_eq!(child.name.as_ref(), "chat");
    assert_eq!(parent.name.as_ref(), "streaming-pipeline");

    // Verify parent-child relationship
    let parent_span_id = parent.span_context.span_id();
    assert_ne!(parent_span_id, SpanId::INVALID);
    assert_eq!(
        child.parent_span_id, parent_span_id,
        "streaming generation should be nested under parent"
    );

    provider.shutdown().unwrap();
}

// ---------------------------------------------------------------------------
// Test 12: Responses API output conversion — merged tool calls
// ---------------------------------------------------------------------------
#[cfg(feature = "openai")]
#[test]
fn test_responses_output_merges_function_calls() {
    use async_openai::types::responses::{FunctionToolCall, OutputItem, OutputStatus};
    use introspection_sdk::openai::convert_responses_output;

    let output = vec![
        OutputItem::FunctionCall(FunctionToolCall {
            call_id: "call_1".to_string(),
            name: "get_weather".to_string(),
            arguments: r#"{"city":"Boston"}"#.to_string(),
            id: Some("fc_1".to_string()),
            status: Some(OutputStatus::Completed),
        }),
        OutputItem::FunctionCall(FunctionToolCall {
            call_id: "call_2".to_string(),
            name: "get_weather".to_string(),
            arguments: r#"{"city":"Atlanta"}"#.to_string(),
            id: Some("fc_2".to_string()),
            status: Some(OutputStatus::Completed),
        }),
    ];

    let messages = convert_responses_output(&output);
    assert_eq!(
        messages.len(),
        1,
        "Multiple function calls should merge into one message"
    );
    assert_eq!(messages[0].parts.len(), 2, "Should have 2 tool_call parts");
    assert_eq!(messages[0].finish_reason.as_deref(), Some("tool-calls"));
}

// ---------------------------------------------------------------------------
// Test 13: Responses API output conversion — reasoning with text
// ---------------------------------------------------------------------------
#[cfg(feature = "openai")]
#[test]
fn test_responses_output_reasoning_merges_into_message() {
    use async_openai::types::responses::{
        OutputItem, OutputMessage as OAIOutputMessage, OutputStatus, ReasoningItem, SummaryPart,
        SummaryTextContent,
    };
    use introspection_sdk::openai::convert_responses_output;

    let output = vec![
        OutputItem::Reasoning(ReasoningItem {
            id: "rs_1".to_string(),
            summary: vec![SummaryPart::SummaryText(SummaryTextContent {
                text: "Thinking step by step...".to_string(),
            })],
            encrypted_content: Some("encrypted-blob".to_string()),
            content: None,
            status: None,
        }),
        OutputItem::Message(OAIOutputMessage {
            id: "msg_1".to_string(),
            status: OutputStatus::Completed,
            content: vec![serde_json::from_value(serde_json::json!({
                "type": "output_text",
                "text": "The answer is 42.",
                "annotations": []
            }))
            .unwrap()],
            role: serde_json::from_value(serde_json::json!("assistant")).unwrap(),
        }),
    ];

    let messages = convert_responses_output(&output);
    assert_eq!(messages.len(), 1, "Reasoning should merge into the message");
    assert_eq!(
        messages[0].parts.len(),
        2,
        "Should have thinking + text parts"
    );
    assert_eq!(messages[0].finish_reason.as_deref(), Some("stop"));

    // Check thinking part
    if let ContentPart::Thinking(ref t) = messages[0].parts[0] {
        assert_eq!(t.content.as_deref(), Some("Thinking step by step..."));
        assert_eq!(t.signature.as_deref(), Some("encrypted-blob"));
    } else {
        panic!("Expected thinking part, got {:?}", messages[0].parts[0]);
    }

    // Check text part
    if let ContentPart::Text(ref t) = messages[0].parts[1] {
        assert_eq!(t.content, "The answer is 42.");
    } else {
        panic!("Expected text part, got {:?}", messages[0].parts[1]);
    }
}

// ---------------------------------------------------------------------------
// Test 14: Responses API input conversion — text input
// ---------------------------------------------------------------------------
#[cfg(feature = "openai")]
#[test]
fn test_responses_input_text() {
    use async_openai::types::responses::InputParam;
    use introspection_sdk::openai::convert_responses_input;

    let input = InputParam::Text("Hello world".to_string());
    let messages = convert_responses_input(&input);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[0].parts.len(), 1);
    if let ContentPart::Text(ref t) = messages[0].parts[0] {
        assert_eq!(t.content, "Hello world");
    } else {
        panic!("Expected text part");
    }
}

// ---------------------------------------------------------------------------
// Test 15: Responses API input conversion — function_call_output items
// ---------------------------------------------------------------------------
#[cfg(feature = "openai")]
#[test]
fn test_responses_input_function_call_output() {
    use async_openai::types::responses::InputParam;
    use introspection_sdk::openai::convert_responses_input;

    let input = InputParam::Items(vec![serde_json::from_value(serde_json::json!({
        "type": "function_call_output",
        "call_id": "call_abc",
        "output": "sunny, 72F"
    }))
    .unwrap()]);
    let messages = convert_responses_input(&input);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "tool");
    if let ContentPart::ToolCallResponse(ref t) = messages[0].parts[0] {
        assert_eq!(t.id, "call_abc");
        assert_eq!(t.response.as_deref(), Some("sunny, 72F"));
    } else {
        panic!("Expected tool_call_response part");
    }
}
