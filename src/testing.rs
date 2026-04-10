//! Test utilities for the Introspection SDK.
//!
//! Provides in-memory span capture and JSON conversion for snapshot testing.
//!
//! Enable with the `testing` feature flag:
//! ```toml
//! introspection-sdk = { version = "0.1", features = ["testing"] }
//! ```

pub use opentelemetry_sdk::trace::{InMemorySpanExporter, SimpleSpanProcessor, SpanData};

use opentelemetry::trace::SpanId;
use opentelemetry_sdk::trace::SdkTracerProvider;
use serde_json::Value;
use std::collections::BTreeMap;

/// Create a [`SdkTracerProvider`] wired to an [`InMemorySpanExporter`] via a
/// synchronous [`SimpleSpanProcessor`].
///
/// Returns `(provider, exporter)` — the exporter can be used to retrieve
/// finished spans after they are ended.
pub fn setup_test_provider() -> (SdkTracerProvider, InMemorySpanExporter) {
    let exporter = InMemorySpanExporter::default();
    let processor = SimpleSpanProcessor::new(exporter.clone());
    let provider = SdkTracerProvider::builder()
        .with_span_processor(processor)
        .build();
    (provider, exporter)
}

/// Convert a single [`SpanData`] into a JSON [`Value`] including all fields.
///
/// Uses [`BTreeMap`] for stable key ordering so snapshots are reproducible.
/// Dynamic values (trace_id, span_id, timestamps) are included — use insta
/// redactions to replace them with placeholders in snapshot assertions.
pub fn span_data_to_json(span: &SpanData) -> Value {
    let mut map = BTreeMap::new();

    map.insert("name".to_string(), Value::String(span.name.to_string()));
    map.insert(
        "span_kind".to_string(),
        Value::String(format!("{:?}", span.span_kind)),
    );
    map.insert(
        "status".to_string(),
        Value::String(format!("{:?}", span.status)),
    );

    // Identity fields
    map.insert(
        "trace_id".to_string(),
        Value::String(span.span_context.trace_id().to_string()),
    );
    map.insert(
        "span_id".to_string(),
        Value::String(span.span_context.span_id().to_string()),
    );
    if span.parent_span_id != SpanId::INVALID {
        map.insert(
            "parent_span_id".to_string(),
            Value::String(span.parent_span_id.to_string()),
        );
    }

    // Timestamps (epoch nanos as string)
    map.insert(
        "start_time".to_string(),
        Value::String(
            span.start_time
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .to_string(),
        ),
    );
    map.insert(
        "end_time".to_string(),
        Value::String(
            span.end_time
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .to_string(),
        ),
    );

    // Attributes — sorted by key for deterministic output
    let mut attrs = BTreeMap::new();
    for kv in &span.attributes {
        attrs.insert(kv.key.to_string(), otel_value_to_json(&kv.value));
    }
    map.insert(
        "attributes".to_string(),
        serde_json::to_value(attrs).unwrap_or(Value::Null),
    );

    serde_json::to_value(map).unwrap_or(Value::Null)
}

/// Convert a slice of [`SpanData`] into a JSON array.
pub fn spans_to_json(spans: &[SpanData]) -> Value {
    Value::Array(spans.iter().map(span_data_to_json).collect())
}

/// Convert an [`opentelemetry::Value`] to a [`serde_json::Value`].
fn otel_value_to_json(value: &opentelemetry::Value) -> Value {
    match value {
        opentelemetry::Value::Bool(b) => Value::Bool(*b),
        opentelemetry::Value::I64(n) => Value::Number((*n).into()),
        opentelemetry::Value::F64(n) => {
            serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number)
        }
        opentelemetry::Value::String(s) => Value::String(s.to_string()),
        opentelemetry::Value::Array(arr) => otel_array_to_json(arr),
        _ => Value::String(format!("{value:?}")),
    }
}

/// Convert an [`opentelemetry::Array`] to a [`serde_json::Value`].
fn otel_array_to_json(arr: &opentelemetry::Array) -> Value {
    match arr {
        opentelemetry::Array::Bool(v) => Value::Array(v.iter().map(|b| Value::Bool(*b)).collect()),
        opentelemetry::Array::I64(v) => {
            Value::Array(v.iter().map(|n| Value::Number((*n).into())).collect())
        }
        opentelemetry::Array::F64(v) => Value::Array(
            v.iter()
                .map(|n| serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number))
                .collect(),
        ),
        opentelemetry::Array::String(v) => {
            Value::Array(v.iter().map(|s| Value::String(s.to_string())).collect())
        }
        _ => Value::String(format!("{arr:?}")),
    }
}
