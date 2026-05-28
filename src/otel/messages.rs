//! Strongly typed message types for gen_ai semantic conventions.
//!
//! These types mirror the gen_ai semantic convention schema and match
//! the Python and JS SDK message representations.

use serde::{Deserialize, Serialize};

/// A text content part.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextPart {
    /// Always `"text"`.
    #[serde(rename = "type")]
    pub r#type: String,
    /// The text content.
    pub content: String,
}

impl TextPart {
    /// Create a new text part.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            r#type: "text".to_string(),
            content: content.into(),
        }
    }
}

/// A tool call request part (assistant requests tool use).
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
        }
    }
}

/// A tool call response part (result returned from executing a tool).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallResponsePart {
    /// Always `"tool_call_response"`.
    #[serde(rename = "type")]
    pub r#type: String,
    /// The tool call ID this result corresponds to.
    pub id: String,
    /// The result content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

impl ToolCallResponsePart {
    /// Create a new tool call response part.
    pub fn new(id: impl Into<String>, response: Option<String>) -> Self {
        Self {
            r#type: "tool_call_response".to_string(),
            id: id.into(),
            response,
        }
    }
}

/// A reasoning/thinking content part.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThinkingPart {
    /// Always `"thinking"`.
    #[serde(rename = "type")]
    pub r#type: String,
    /// The reasoning/thinking summary content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Encrypted reasoning signature (maps to OpenAI encrypted_content).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl ThinkingPart {
    /// Create a new thinking part.
    pub fn new(content: Option<String>, signature: Option<String>) -> Self {
        Self {
            r#type: "thinking".to_string(),
            content,
            signature,
        }
    }
}

/// A content part in a message — text, tool call, tool response, or thinking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ContentPart {
    /// Plain text content.
    Text(TextPart),
    /// A tool use request from the assistant.
    ToolCallRequest(ToolCallRequestPart),
    /// A tool result returned to the model.
    ToolCallResponse(ToolCallResponsePart),
    /// Reasoning/thinking content.
    Thinking(ThinkingPart),
}

/// An input message in an LLM conversation (user, system, assistant, tool).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputMessage {
    /// The role of the message sender (e.g. `"user"`, `"system"`, `"assistant"`, `"tool"`).
    pub role: String,
    /// The message content as an ordered list of parts.
    pub parts: Vec<ContentPart>,
}

impl InputMessage {
    /// Create a user message with plain text content.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            parts: vec![ContentPart::Text(TextPart::new(content))],
        }
    }

    /// Create a system message with plain text content.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            parts: vec![ContentPart::Text(TextPart::new(content))],
        }
    }

    /// Create an assistant message with plain text content.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            parts: vec![ContentPart::Text(TextPart::new(content))],
        }
    }

    /// Create a message with an arbitrary role and content parts.
    pub fn with_role(role: impl Into<String>, parts: Vec<ContentPart>) -> Self {
        Self {
            role: role.into(),
            parts,
        }
    }
}

/// An output message from an LLM (typically role `"assistant"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputMessage {
    /// The role of the message sender (typically `"assistant"`).
    pub role: String,
    /// The message content as an ordered list of parts.
    pub parts: Vec<ContentPart>,
    /// Finish reason (e.g. `"stop"`, `"tool-calls"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

impl OutputMessage {
    /// Create an assistant message with plain text content.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            parts: vec![ContentPart::Text(TextPart::new(content))],
            finish_reason: Some("stop".to_string()),
        }
    }

    /// Create a message with an arbitrary role and content parts.
    pub fn with_role(role: impl Into<String>, parts: Vec<ContentPart>) -> Self {
        Self {
            role: role.into(),
            parts,
            finish_reason: None,
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
    fn test_serialization_tool_call_response() {
        let msg = InputMessage {
            role: "tool".to_string(),
            parts: vec![ContentPart::ToolCallResponse(ToolCallResponsePart::new(
                "call_123",
                Some("result data".into()),
            ))],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"tool_call_response""#));
        assert!(json.contains(r#""response":"result data""#));
        assert!(json.contains(r#""id":"call_123""#));
    }
}
