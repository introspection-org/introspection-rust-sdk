use introspection_sdk::otel::messages::{InputMessage, OutputMessage};
use introspection_sdk::otel::testing::{setup_test_provider, span_data_to_json};
use introspection_sdk::{GenerationUpdate, Observation, ObservationConfig};
use opentelemetry::trace::{SpanId, TracerProvider};

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
    use introspection_sdk::otel::observation::infer_system;

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
