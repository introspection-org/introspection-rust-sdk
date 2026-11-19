//! AG-UI messages (`MESSAGES_SNAPSHOT`, `RunAgentInput.messages`) and the
//! content parts a user message or a tool result carries.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::de::keep_null;

/// One message in a conversation, discriminated by `role`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    Developer(DeveloperMessage),
    System(SystemMessage),
    Assistant(AssistantMessage),
    User(UserMessage),
    Tool(ToolMessage),
    Activity(ActivityMessage),
    Reasoning(ReasoningMessage),
}

impl Message {
    /// The message's id, whatever its role.
    pub fn id(&self) -> &str {
        match self {
            Message::Developer(m) | Message::System(m) => &m.id,
            Message::Assistant(m) => &m.id,
            Message::User(m) => &m.id,
            Message::Tool(m) => &m.id,
            Message::Activity(m) => &m.id,
            Message::Reasoning(m) => &m.id,
        }
    }
}

/// A `developer` or `system` message: plain instruction text.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionMessage {
    pub subagent_run_id: Option<String>,
    pub id: String,
    pub name: Option<String>,
    pub encrypted_value: Option<String>,
    pub metadata: Option<Map<String, Value>>,
    pub content: String,
}

/// A `developer` message.
pub type DeveloperMessage = InstructionMessage;
/// A `system` message.
pub type SystemMessage = InstructionMessage;

/// An `assistant` message: text, tool calls, or both.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub subagent_run_id: Option<String>,
    pub id: String,
    pub name: Option<String>,
    pub encrypted_value: Option<String>,
    pub metadata: Option<Map<String, Value>>,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// A `user` message.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub subagent_run_id: Option<String>,
    pub id: String,
    pub name: Option<String>,
    pub encrypted_value: Option<String>,
    pub metadata: Option<Map<String, Value>>,
    pub content: MessageContent,
}

/// A `tool` message: the result of one tool call.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMessage {
    pub subagent_run_id: Option<String>,
    pub id: String,
    pub content: MessageContent,
    pub tool_call_id: String,
    pub error: Option<String>,
    pub encrypted_value: Option<String>,
    pub metadata: Option<Map<String, Value>>,
}

/// An `activity` message: structured progress an agent reports.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityMessage {
    pub subagent_run_id: Option<String>,
    pub id: String,
    pub activity_type: String,
    pub content: Value,
    pub metadata: Option<Map<String, Value>>,
}

/// A `reasoning` message.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessage {
    pub subagent_run_id: Option<String>,
    pub id: String,
    pub content: String,
    pub encrypted_value: Option<String>,
    pub metadata: Option<Map<String, Value>>,
}

/// A tool call an assistant message makes.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: ToolCallType,
    pub function: FunctionCall,
    pub encrypted_value: Option<String>,
    pub metadata: Option<Map<String, Value>>,
}

/// A [`ToolCall`]'s `type`, which AG-UI fixes at `function`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolCallType {
    #[default]
    Function,
}

/// The function a [`ToolCall`] invokes, with its arguments as a JSON string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// A user message's or a tool result's content: a string, or content parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<InputContent>),
}

impl MessageContent {
    /// The content when it is a plain string.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            MessageContent::Text(s) => Some(s),
            MessageContent::Parts(_) => None,
        }
    }
}

/// One content part, discriminated by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InputContent {
    Text(TextPart),
    Image(ImagePart),
    Audio(AudioPart),
    Video(VideoPart),
    Document(DocumentPart),
}

/// A text content part.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPart {
    pub id: Option<String>,
    pub text: String,
    #[serde(default, deserialize_with = "keep_null")]
    pub metadata: Option<Value>,
}

/// An image, audio, video or document content part: media read from a
/// [`PartSource`].
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaPart {
    pub id: Option<String>,
    pub source: PartSource,
    #[serde(default, deserialize_with = "keep_null")]
    pub metadata: Option<Value>,
}

/// An `image` content part.
pub type ImagePart = MediaPart;
/// An `audio` content part.
pub type AudioPart = MediaPart;
/// A `video` content part.
pub type VideoPart = MediaPart;
/// A `document` content part.
pub type DocumentPart = MediaPart;

/// Where a [`MediaPart`]'s bytes come from, discriminated by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PartSource {
    Data(DataSource),
    Url(UrlSource),
    File(FileSource),
}

/// Inline media, base64-encoded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSource {
    pub value: String,
    pub mime_type: String,
}

/// Media at a URL.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlSource {
    pub value: String,
    pub mime_type: Option<String>,
}

/// Media in a file a provider holds.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSource {
    pub value: String,
    pub provider: Option<String>,
    pub mime_type: Option<String>,
}
