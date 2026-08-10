//! The GenAI span returned by conversation item reads.
//!
//! A conversation item **is** an OpenTelemetry span, so it is modelled as one:
//! identity and timing at the top level, everything else under
//! [`GenAiSpan::attributes`] keyed by its OpenTelemetry semantic-convention
//! name. [`GenAiRequest::model`] is reached as `gen_ai.request.model` because
//! that is what the SDK wrote when it created the span — no private dialect to
//! learn, no renamed columns to memorize.
//!
//! Both item reads return this same type. Only message depth differs:
//!
//! - `GET /v1/conversations/{id}/items` — that turn's input delta.
//! - `GET /v1/conversations/{id}/items/{item_id}` — the **full history**, so a
//!   conversation can be resumed with complete context.
//!
//! A depth difference, not a schema difference: one parser, one renderer.
//!
//! Two properties carry the weight, and both are load-bearing rather than
//! stylistic:
//!
//! - **The tree is open.** Every model here carries a `#[serde(flatten)]`
//!   `extra` map, so an attribute nobody modelled still arrives and still
//!   round-trips. The server returns the attribute tree as stored, not as an
//!   allow-list; typing it closed would reintroduce exactly the lossiness this
//!   representation exists to remove.
//! - **Nothing serializes as `null`.** An absent value is an absent key —
//!   every optional field is `skip_serializing_if = "Option::is_none"`, every
//!   collection `skip_serializing_if` empty. A real `0` still serializes: a
//!   turn that genuinely produced no output tokens is a fact, not an absence.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::otel::messages::{InputMessage, OutputMessage};

/// A `{ "name": … }` attribute node — `gen_ai.operation`, `gen_ai.provider`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct NameRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// An `{ "id": … }` attribute node — `gen_ai.conversation`,
/// `introspection.org`, `introspection.member`, and friends.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IdRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ----- gen_ai.* --------------------------------------------------------------

/// `gen_ai.agent.*`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiAgent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `gen_ai.request.*` — what was asked for.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `gen_ai.response.*` — what came back.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reasons: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// A nested token count — `gen_ai.usage.cache_read.input_tokens` and
/// `gen_ai.usage.cache_creation.input_tokens`.
///
/// The nesting is the adopted spelling: cache tokens were a local extension
/// until the GenAI conventions took them, and they took them nested.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct TokenCount {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `gen_ai.usage.*`.
///
/// On an item these are that operation's usage. On a conversation summary they
/// are the conversation's totals — same attribute, same honest meaning for its
/// scope, disambiguated by which read the object came from.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<TokenCount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation: Option<TokenCount>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `gen_ai.tool.call.*`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `gen_ai.tool.*`.
///
/// `definitions` stays untyped: the tool-definition schema is
/// `Development`-stability upstream, and a shape change there must not fail a
/// whole page of conversation reads.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiTool {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub tool_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<GenAiToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definitions: Option<Vec<serde_json::Value>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `gen_ai.input.messages`.
///
/// The full history on item detail and that turn's delta on the items list.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiInput {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<InputMessage>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `gen_ai.output.messages`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiOutput {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<OutputMessage>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// The `gen_ai.*` attribute family, nested as the convention names it.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiAttributes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<NameRef>,
    /// `gen_ai.provider.name` — current, having replaced `gen_ai.system`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<NameRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<IdRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<GenAiAgent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<GenAiRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<GenAiResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<GenAiUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<GenAiTool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<GenAiInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<GenAiOutput>,
    /// `gen_ai.system_instructions` — untyped for the same reason as
    /// [`GenAiTool::definitions`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instructions: Option<Vec<serde_json::Value>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ----- introspection.* -------------------------------------------------------

/// `introspection.runtime.*` — the runtime deployment and its group.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionRuntime {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `introspection.recipe.*`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionRecipe {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit_sha: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `introspection.conversation.*`.
///
/// On an item these describe the turn's place in the conversation.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionConversation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_new: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_hash_hit: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_messages_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_messages_end: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_tool_use_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_errors: Option<bool>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// The `introspection.*` attribute family.
///
/// Everything here is ours. `cost_usd` sits here rather than under
/// `gen_ai.usage` because cost is not in the GenAI conventions at all — on
/// spans or anywhere else — and beside `environment` rather than inside
/// `conversation` because it is the cost of *this object*: the operation's on
/// an item, the conversation's on a summary, scoped by which read returned the
/// span exactly as `gen_ai.usage.*` is.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionAttributes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org: Option<IdRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<IdRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<IdRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<IdRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<IdRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<IntrospectionRuntime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment: Option<IdRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<IntrospectionRecipe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<IntrospectionConversation>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ----- the span --------------------------------------------------------------

/// The span's attribute tree.
///
/// Typed for the two families whose meaning we own; open for everything else,
/// so a customer's own attribute survives the round trip.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct SpanAttributes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen_ai: Option<GenAiAttributes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introspection: Option<IntrospectionAttributes>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// OpenTelemetry span status.
///
/// `code` stays a `String` rather than an enum: `Ok` / `Error` / `Unset` is
/// the current set, and an unrecognised value must not fail a page.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct SpanStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// One conversation item, or one conversation summary.
///
/// Same type either way — see the module docs for what differs.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiSpan {
    pub trace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `INTERNAL`, `SERVER`, `CLIENT`, … — a `String` for the same reason as
    /// [`SpanStatus::code`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub start_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ns: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SpanStatus>,
    /// OTel resource attributes, returned with `include=resource_attributes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<HashMap<String, serde_json::Value>>,
    /// Span events, returned with `include=events`. A top-level OTel span
    /// field, not an attribute — they carry their own name and timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "SpanAttributes::is_empty")]
    pub attributes: SpanAttributes,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl SpanAttributes {
    /// Whether the tree carries nothing at all — used to keep an empty
    /// `attributes` out of the serialized form.
    pub fn is_empty(&self) -> bool {
        self.gen_ai.is_none() && self.introspection.is_none() && self.extra.is_empty()
    }
}

impl GenAiSpan {
    /// `attributes.gen_ai.conversation.id`, if present.
    pub fn conversation_id(&self) -> Option<&str> {
        self.attributes
            .gen_ai
            .as_ref()?
            .conversation
            .as_ref()?
            .id
            .as_deref()
    }

    /// `attributes.gen_ai.operation.name`, if present.
    pub fn operation_name(&self) -> Option<&str> {
        self.attributes
            .gen_ai
            .as_ref()?
            .operation
            .as_ref()?
            .name
            .as_deref()
    }

    /// `attributes.gen_ai.request.model`, if present.
    pub fn request_model(&self) -> Option<&str> {
        self.attributes
            .gen_ai
            .as_ref()?
            .request
            .as_ref()?
            .model
            .as_deref()
    }

    /// `attributes.gen_ai.input.messages` — empty rather than absent.
    ///
    /// A convenience for the common read. Reaching four levels down to answer
    /// "what was said" is the one place this shape is worse than the flat one,
    /// so the accessor pays that cost once here instead of at every call site.
    pub fn input_messages(&self) -> &[InputMessage] {
        self.attributes
            .gen_ai
            .as_ref()
            .and_then(|gen_ai| gen_ai.input.as_ref())
            .map(|input| input.messages.as_slice())
            .unwrap_or_default()
    }

    /// `attributes.gen_ai.output.messages` — empty rather than absent.
    pub fn output_messages(&self) -> &[OutputMessage] {
        self.attributes
            .gen_ai
            .as_ref()
            .and_then(|gen_ai| gen_ai.output.as_ref())
            .map(|output| output.messages.as_slice())
            .unwrap_or_default()
    }
}

/// OpenAI-style page of conversation items.
///
/// `first_id` and `last_id` are informational span IDs; pagination uses the
/// opaque `next` cursor.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct GenAiSpanList {
    #[serde(default = "list_object")]
    pub object: String,
    #[serde(default)]
    pub data: Vec<GenAiSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_id: Option<String>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

impl Default for GenAiSpanList {
    fn default() -> Self {
        Self {
            object: list_object(),
            data: Vec::new(),
            first_id: None,
            last_id: None,
            has_more: false,
            next: None,
        }
    }
}

fn list_object() -> String {
    "list".to_string()
}
