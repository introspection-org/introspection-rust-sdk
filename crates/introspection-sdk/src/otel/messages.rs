//! Strongly typed message types for gen_ai semantic conventions.
//!
//! These types mirror the gen_ai semantic convention schema and match
//! the message representation the backend stores and returns.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A text content part.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextPart {
    /// Always `"text"`.
    #[serde(rename = "type")]
    pub r#type: String,
    /// The text content.
    pub content: String,
    /// Fields the SDK does not type. Open by design: the GenAI conventions
    /// let providers add their own fields to messages and parts.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl TextPart {
    /// Create a new text part.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            r#type: "text".to_string(),
            content: content.into(),
            other: BTreeMap::new(),
        }
    }
}

/// A tool call request part (assistant requests tool use).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallRequestPart {
    /// Always `"tool_call"`.
    #[serde(rename = "type")]
    pub r#type: String,
    /// The tool call ID.
    pub id: String,
    /// The function/tool name.
    pub name: String,
    /// The arguments as a JSON string.
    pub arguments: String,
    /// Fields the SDK does not type. Open by design: the GenAI conventions
    /// let providers add their own fields to messages and parts.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl ToolCallRequestPart {
    /// Create a new tool call request part.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            r#type: "tool_call".to_string(),
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
            other: BTreeMap::new(),
        }
    }
}

/// A tool call response part (result returned from executing a tool).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallResponsePart {
    /// Always `"tool_call_response"`.
    #[serde(rename = "type")]
    pub r#type: String,
    /// The tool call ID this result corresponds to.
    pub id: String,
    /// The result content.
    ///
    /// The `result` alias reads back parts written by older deployments, which
    /// used that key before the semantic convention settled on `response`. It
    /// is a read-side compatibility shim only — we always write `response`.
    #[serde(alias = "result")]
    pub response: Option<String>,
    /// Fields the SDK does not type. Open by design: the GenAI conventions
    /// let providers add their own fields to messages and parts.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl ToolCallResponsePart {
    /// Create a new tool call response part.
    pub fn new(id: impl Into<String>, response: Option<String>) -> Self {
        Self {
            r#type: "tool_call_response".to_string(),
            id: id.into(),
            response,
            other: BTreeMap::new(),
        }
    }
}

/// A reasoning/thinking content part.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThinkingPart {
    /// Always `"thinking"`.
    #[serde(rename = "type")]
    pub r#type: String,
    /// The reasoning/thinking summary content.
    pub content: Option<String>,
    /// Encrypted reasoning signature (maps to OpenAI encrypted_content).
    pub signature: Option<String>,
    /// Fields the SDK does not type. Open by design: the GenAI conventions
    /// let providers add their own fields to messages and parts.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl ThinkingPart {
    /// Create a new thinking part.
    pub fn new(content: Option<String>, signature: Option<String>) -> Self {
        Self {
            r#type: "thinking".to_string(),
            content,
            signature,
            other: BTreeMap::new(),
        }
    }
}

/// A content part in a message — text, tool call, tool response, or thinking.
///
/// Serializes untagged: each variant already carries its own `type` field.
/// Deserialization dispatches on that `type` explicitly rather than by
/// try-each-variant, because untagged matching is decided by *shape* and every
/// part shape here is a subset of another — a `{"type":"uri", …}` part matched
/// [`ThinkingPart`] (whose fields are all optional) and came back out as
/// `{"type":"uri"}` with its payload silently gone.
///
/// [`Self::Other`] is the escape hatch, and it is load-bearing on the read
/// path: the semconv part vocabulary is `Development`-stability and still
/// growing (`blob`, `uri`, `file`, `server_tool_call`), so a part type this
/// SDK release has never seen must neither fail the conversation page that
/// contains it nor lose its content. It round-trips verbatim instead.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(untagged)]
pub enum ContentPart {
    /// Plain text content.
    Text(TextPart),
    /// A tool use request from the assistant.
    ToolCallRequest(ToolCallRequestPart),
    /// A tool result returned to the model.
    ToolCallResponse(ToolCallResponsePart),
    /// Reasoning/thinking content. Accepts both the `thinking` spelling we
    /// have always written and the spec's `reasoning`, preserving whichever
    /// arrived — telemetry is append-only, so both exist forever.
    Thinking(ThinkingPart),
    /// A part shape this SDK build does not model, preserved as-is.
    Other(serde_json::Value),
}

impl<'de> Deserialize<'de> for ContentPart {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let part_type = value.get("type").and_then(serde_json::Value::as_str);
        // A known `type` whose body does not fit the model is kept whole
        // rather than rejected: on a read path, dropping a page because one
        // part gained a field is the worse failure.
        let typed = match part_type {
            Some("text") => serde_json::from_value(value.clone()).map(ContentPart::Text),
            Some("tool_call") => {
                serde_json::from_value(value.clone()).map(ContentPart::ToolCallRequest)
            }
            Some("tool_call_response") => {
                serde_json::from_value(value.clone()).map(ContentPart::ToolCallResponse)
            }
            Some("thinking") | Some("reasoning") => {
                serde_json::from_value(value.clone()).map(ContentPart::Thinking)
            }
            _ => return Ok(ContentPart::Other(value)),
        };
        Ok(typed.unwrap_or(ContentPart::Other(value)))
    }
}

/// An input message in an LLM conversation (user, system, assistant, tool).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputMessage {
    /// The role of the message sender (e.g. `"user"`, `"system"`, `"assistant"`, `"tool"`).
    pub role: String,
    /// The message content as an ordered list of parts. Empty parts are
    /// omitted on output, as the server stores a message without parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ContentPart>,
    /// The participant's name, when the provider records one.
    pub name: Option<String>,
    /// Fields the SDK does not type. Open by design: the GenAI conventions
    /// let providers add their own fields to messages and parts.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl InputMessage {
    /// Create a user message with plain text content.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            parts: vec![ContentPart::Text(TextPart::new(content))],
            name: None,
            other: BTreeMap::new(),
        }
    }

    /// Create a system message with plain text content.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            parts: vec![ContentPart::Text(TextPart::new(content))],
            name: None,
            other: BTreeMap::new(),
        }
    }

    /// Create an assistant message with plain text content.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            parts: vec![ContentPart::Text(TextPart::new(content))],
            name: None,
            other: BTreeMap::new(),
        }
    }

    /// Create a message with an arbitrary role and content parts.
    pub fn with_role(role: impl Into<String>, parts: Vec<ContentPart>) -> Self {
        Self {
            role: role.into(),
            parts,
            name: None,
            other: BTreeMap::new(),
        }
    }
}

/// An output message from an LLM (typically role `"assistant"`).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputMessage {
    /// The role of the message sender (typically `"assistant"`).
    pub role: String,
    /// The message content as an ordered list of parts. Empty parts are
    /// omitted on output, as the server stores a message without parts.
    // A turn that ends before producing content (`finish_reason: "aborted"`)
    // is stored without `parts`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ContentPart>,
    /// Finish reason (e.g. `"stop"`, `"tool-calls"`).
    pub finish_reason: Option<String>,
    /// The participant's name, when the provider records one.
    pub name: Option<String>,
    /// Fields the SDK does not type. Open by design: the GenAI conventions
    /// let providers add their own fields to messages and parts.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl OutputMessage {
    /// Create an assistant message with plain text content.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            parts: vec![ContentPart::Text(TextPart::new(content))],
            finish_reason: Some("stop".to_string()),
            name: None,
            other: BTreeMap::new(),
        }
    }

    /// Create a message with an arbitrary role and content parts.
    pub fn with_role(role: impl Into<String>, parts: Vec<ContentPart>) -> Self {
        Self {
            role: role.into(),
            parts,
            finish_reason: None,
            name: None,
            other: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_message_user() {
        let msg = InputMessage::user("Hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.parts.len(), 1);
        if let ContentPart::Text(ref t) = msg.parts[0] {
            assert_eq!(t.content, "Hello");
        } else {
            panic!("Expected text part");
        }
    }

    #[test]
    fn test_serialization_simple() {
        let msgs = vec![InputMessage::user("Say hello")];
        let json = serde_json::to_string(&msgs).unwrap();
        assert_eq!(
            json,
            r#"[{"role":"user","parts":[{"type":"text","content":"Say hello"}]}]"#
        );
    }

    #[test]
    fn test_serialization_tool_call() {
        let msgs = vec![OutputMessage::with_role(
            "assistant",
            vec![ContentPart::ToolCallRequest(ToolCallRequestPart::new(
                "call_123",
                "get_weather",
                r#"{"location":"SF"}"#,
            ))],
        )];
        let json = serde_json::to_string(&msgs).unwrap();
        assert!(json.contains("tool_call"));
        assert!(json.contains("get_weather"));
    }

    #[test]
    fn test_serialization_finish_reason() {
        let msg = OutputMessage::assistant("Hello");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""finish_reason":"stop""#));

        let msg_no_finish = OutputMessage::with_role("assistant", vec![]);
        let json = serde_json::to_string(&msg_no_finish).unwrap();
        assert!(!json.contains("finish_reason"));
    }

    #[test]
    fn test_serialization_thinking_part_with_content() {
        let msg = OutputMessage {
            role: "assistant".to_string(),
            parts: vec![
                ContentPart::Thinking(ThinkingPart::new(Some("Let me think...".into()), None)),
                ContentPart::Text(TextPart::new("The answer is 42.")),
            ],
            finish_reason: Some("stop".to_string()),
            name: None,
            other: Default::default(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"thinking""#));
        assert!(json.contains(r#""content":"Let me think...""#));
        assert!(!json.contains("signature"));
    }

    #[test]
    fn test_serialization_thinking_part_with_signature() {
        let msg = OutputMessage {
            role: "assistant".to_string(),
            parts: vec![ContentPart::Thinking(ThinkingPart::new(
                None,
                Some("encrypted-blob".into()),
            ))],
            finish_reason: None,
            name: None,
            other: Default::default(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"thinking""#));
        assert!(json.contains(r#""signature":"encrypted-blob""#));
        assert!(!json.contains(r#""content""#));
    }

    #[test]
    fn test_serialization_thinking_part_with_both() {
        let part = ThinkingPart::new(Some("summary text".into()), Some("sig123".into()));
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains(r#""content":"summary text""#));
        assert!(json.contains(r#""signature":"sig123""#));
    }

    #[test]
    fn test_unknown_part_type_round_trips_instead_of_failing() {
        // A `uri` part — spec-defined, not yet modelled here. Decoding must
        // not fail, and re-encoding must give the part back unchanged.
        let raw = serde_json::json!({
            "role": "user",
            "parts": [{"type": "uri", "modality": "image", "uri": "https://x/y.png"}]
        });

        let msg: InputMessage = serde_json::from_value(raw.clone()).unwrap();

        assert!(matches!(msg.parts[0], ContentPart::Other(_)));
        assert_eq!(serde_json::to_value(&msg).unwrap(), raw);
    }

    #[test]
    fn test_a_thinking_shaped_unknown_part_is_not_swallowed_as_thinking() {
        // The regression this dispatch exists to prevent: every field on
        // `ThinkingPart` is optional, so under untagged matching any tagged
        // object matched it and lost everything it did not declare.
        let raw = serde_json::json!({"type": "blob", "mime_type": "image/png", "data": "abc"});

        let part: ContentPart = serde_json::from_value(raw.clone()).unwrap();

        assert!(matches!(part, ContentPart::Other(_)));
        assert_eq!(serde_json::to_value(&part).unwrap(), raw);
    }

    #[test]
    fn test_spec_reasoning_spelling_decodes_and_keeps_its_name() {
        // Telemetry is append-only: spans written with `thinking` and spans
        // written with the spec's `reasoning` both exist forever, so the
        // reader accepts both and gives back whichever it was handed.
        let raw = serde_json::json!({"type": "reasoning", "content": "hmm"});

        let part: ContentPart = serde_json::from_value(raw.clone()).unwrap();

        assert!(matches!(part, ContentPart::Thinking(_)));
        assert_eq!(serde_json::to_value(&part).unwrap(), raw);
    }

    #[test]
    fn test_legacy_tool_call_response_result_key_is_read_as_response() {
        let part: ContentPart = serde_json::from_value(serde_json::json!({
            "type": "tool_call_response",
            "id": "call_1",
            "result": "42"
        }))
        .unwrap();

        match part {
            ContentPart::ToolCallResponse(response) => {
                assert_eq!(response.response.as_deref(), Some("42"))
            }
            other => panic!("expected a tool call response, got {other:?}"),
        }
    }

    #[test]
    fn test_aborted_output_message_without_parts_decodes_empty() {
        // As the Data Plane serves an aborted turn on the conversation items list.
        let msg: OutputMessage = serde_json::from_value(serde_json::json!({
            "role": "assistant",
            "finish_reason": "aborted",
            "provider": "introspection",
            "model": "scripted-llm",
            "api": "openai-completions"
        }))
        .unwrap();

        assert!(msg.parts.is_empty());
        assert_eq!(msg.finish_reason.as_deref(), Some("aborted"));
    }

    #[test]
    fn test_input_message_without_parts_decodes_empty() {
        let msg: InputMessage =
            serde_json::from_value(serde_json::json!({"role": "assistant"})).unwrap();

        assert!(msg.parts.is_empty());
    }

    #[test]
    fn test_output_message_without_parts_round_trips_without_parts() {
        let raw = serde_json::json!({"role": "assistant", "finish_reason": "aborted"});

        let msg: OutputMessage = serde_json::from_value(raw.clone()).unwrap();

        assert_eq!(serde_json::to_value(&msg).unwrap(), raw);
    }

    #[test]
    fn test_input_message_without_parts_round_trips_without_parts() {
        let raw = serde_json::json!({"role": "assistant"});

        let msg: InputMessage = serde_json::from_value(raw.clone()).unwrap();

        assert_eq!(serde_json::to_value(&msg).unwrap(), raw);
    }

    #[test]
    fn test_messages_with_parts_round_trip_unchanged() {
        let input = serde_json::json!({
            "role": "user",
            "parts": [{"type": "text", "content": "hi"}]
        });
        let output = serde_json::json!({
            "role": "assistant",
            "parts": [{"type": "text", "content": "hello"}],
            "finish_reason": "stop"
        });

        let input_msg: InputMessage = serde_json::from_value(input.clone()).unwrap();
        let output_msg: OutputMessage = serde_json::from_value(output.clone()).unwrap();

        assert_eq!(serde_json::to_value(&input_msg).unwrap(), input);
        assert_eq!(serde_json::to_value(&output_msg).unwrap(), output);
    }

    #[test]
    fn test_serialization_tool_call_response() {
        let msg = InputMessage {
            role: "tool".to_string(),
            parts: vec![ContentPart::ToolCallResponse(ToolCallResponsePart::new(
                "call_123",
                Some("result data".into()),
            ))],
            name: None,
            other: Default::default(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"tool_call_response""#));
        assert!(json.contains(r#""response":"result data""#));
        assert!(json.contains(r#""id":"call_123""#));
    }
}
