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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::otel::messages::{InputMessage, OutputMessage};

/// A `{ "name": … }` attribute node — `gen_ai.operation`, `gen_ai.provider`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct NameRef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// An `{ "id": … }` attribute node — `gen_ai.conversation`,
/// `introspection.org`, `introspection.member`, and friends.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IdRef {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

// ----- gen_ai.* --------------------------------------------------------------

/// `gen_ai.agent.*`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiAgent {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `gen_ai.request.*` — what was asked for.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiRequest {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub top_k: Option<i64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `gen_ai.response.*` — what came back.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub finish_reasons: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// A nested token count — `gen_ai.usage.cache_read.input_tokens` and
/// `gen_ai.usage.cache_creation.input_tokens`.
///
/// The nesting is the adopted spelling: cache tokens were a local extension
/// until the GenAI conventions took them, and they took them nested.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct TokenCount {
    #[serde(default)]
    pub input_tokens: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `gen_ai.usage.*`.
///
/// On an item these are that operation's usage. On a conversation summary they
/// are the conversation's totals — same attribute, same honest meaning for its
/// scope, disambiguated by which read the object came from.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiUsage {
    #[serde(default)]
    pub input_tokens: Option<i64>,
    #[serde(default)]
    pub output_tokens: Option<i64>,
    #[serde(default)]
    pub cache_read: Option<TokenCount>,
    #[serde(default)]
    pub cache_creation: Option<TokenCount>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `gen_ai.tool.call.*`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiToolCall {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `gen_ai.tool.*`.
///
/// `definitions` stays untyped: the tool-definition schema is
/// `Development`-stability upstream, and a shape change there must not fail a
/// whole page of conversation reads.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiTool {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "type", default)]
    pub tool_type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub call: Option<GenAiToolCall>,
    #[serde(default)]
    pub definitions: Option<Vec<serde_json::Value>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `gen_ai.input.messages`.
///
/// The full history on item detail and that turn's delta on the items list.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiInput {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<InputMessage>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `gen_ai.output.messages`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiOutput {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<OutputMessage>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// The `gen_ai.*` attribute family, nested as the convention names it.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiAttributes {
    #[serde(default)]
    pub operation: Option<NameRef>,
    /// `gen_ai.provider.name` — current, having replaced `gen_ai.system`.
    #[serde(default)]
    pub provider: Option<NameRef>,
    #[serde(default)]
    pub conversation: Option<IdRef>,
    #[serde(default)]
    pub agent: Option<GenAiAgent>,
    #[serde(default)]
    pub request: Option<GenAiRequest>,
    #[serde(default)]
    pub response: Option<GenAiResponse>,
    #[serde(default)]
    pub usage: Option<GenAiUsage>,
    #[serde(default)]
    pub tool: Option<GenAiTool>,
    #[serde(default)]
    pub input: Option<GenAiInput>,
    #[serde(default)]
    pub output: Option<GenAiOutput>,
    /// `gen_ai.system_instructions` — untyped for the same reason as
    /// [`GenAiTool::definitions`].
    #[serde(default)]
    pub system_instructions: Option<Vec<serde_json::Value>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

// ----- introspection.* -------------------------------------------------------

/// `introspection.runtime.*` — the runtime deployment and its group.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionRuntime {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `introspection.recipe.*`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionRecipe {
    #[serde(default)]
    pub git_commit_sha: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `introspection.history.new_messages`: the slice of the full input history
/// this call added, `start` inclusive and `end` exclusive.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionNewMessages {
    #[serde(default)]
    pub start: Option<i64>,
    #[serde(default)]
    pub end: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `introspection.history.*`: where the conversation linker placed this model
/// call, inferred from hashes of its input history.
///
/// Absent on a span the linker never placed (tool, proxy and API calls).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionHistory {
    /// 1-based.
    #[serde(default)]
    pub position: Option<i64>,
    #[serde(default)]
    pub is_new: Option<bool>,
    #[serde(default)]
    pub continuation_method: Option<String>,
    #[serde(default)]
    pub hash_hit: Option<bool>,
    #[serde(default)]
    pub new_messages: Option<IntrospectionNewMessages>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `introspection.contains.*`: what a chat span's sealed input delta and
/// output contain, stamped at ingest so a read can select rows without
/// opening them.
///
/// Filter values only, never content. Absent on an `execute_tool` span.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionContains {
    /// Summary schema version. `0` or absent means the span was never
    /// summarized (it was recorded before the summary existed).
    #[serde(default)]
    pub version: Option<i64>,
    /// The `contains=` values this span answers, spelled exactly as a client
    /// passes them to the conversation items list, as `<path>:<value>` (e.g.
    /// `gen_ai.input.messages.role:user`,
    /// `gen_ai.output.messages.parts.name:message_user`). A name list the
    /// processor cut at its bound carries `<path>:*`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// The `introspection.*` attribute family.
///
/// Everything here is ours. `cost_usd` sits here rather than under
/// `gen_ai.usage` because cost is not in the GenAI conventions at all — on
/// spans or anywhere else — and beside `environment` because it is the cost of
/// *this object*: the operation's on
/// an item, the conversation's on a summary, scoped by which read returned the
/// span exactly as `gen_ai.usage.*` is.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct IntrospectionAttributes {
    #[serde(default)]
    pub org: Option<IdRef>,
    #[serde(default)]
    pub project: Option<IdRef>,
    #[serde(default)]
    pub member: Option<IdRef>,
    #[serde(default)]
    pub run: Option<IdRef>,
    #[serde(default)]
    pub task: Option<IdRef>,
    #[serde(default)]
    pub runtime: Option<IntrospectionRuntime>,
    #[serde(default)]
    pub experiment: Option<IdRef>,
    #[serde(default)]
    pub recipe: Option<IntrospectionRecipe>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// `introspection.turn`: the runtime's application-level turn, numbered
    /// from 1 per conversation. Absent where no runtime stamped one.
    #[serde(default)]
    pub turn: Option<i64>,
    #[serde(default)]
    pub history: Option<IntrospectionHistory>,
    #[serde(default)]
    pub contains: Option<IntrospectionContains>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

// ----- the span --------------------------------------------------------------

/// The span's attribute tree.
///
/// Typed for the two families whose meaning we own; open for everything else,
/// so a customer's own attribute survives the round trip.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct SpanAttributes {
    #[serde(default)]
    pub gen_ai: Option<GenAiAttributes>,
    #[serde(default)]
    pub introspection: Option<IntrospectionAttributes>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// OpenTelemetry span status.
///
/// `code` stays a `String` rather than an enum: `Ok` / `Error` / `Unset` is
/// the current set, and an unrecognised value must not fail a page.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct SpanStatus {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// One conversation item, or one conversation summary.
///
/// Same type either way — see the module docs for what differs.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct GenAiSpan {
    pub trace_id: String,
    #[serde(default)]
    pub span_id: Option<String>,
    #[serde(default)]
    pub parent_span_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// `INTERNAL`, `SERVER`, `CLIENT`, … — a `String` for the same reason as
    /// [`SpanStatus::code`].
    #[serde(default)]
    pub kind: Option<String>,
    pub start_time: String,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub duration_ns: Option<i64>,
    #[serde(default)]
    pub status: Option<SpanStatus>,
    /// OTel resource attributes, returned with `include=resource_attributes`.
    #[serde(default)]
    pub resource: Option<BTreeMap<String, serde_json::Value>>,
    /// Span events, returned with `include=events`. A top-level OTel span
    /// field, not an attribute — they carry their own name and timestamp.
    #[serde(default)]
    pub events: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "SpanAttributes::is_empty")]
    pub attributes: SpanAttributes,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct GenAiSpanList {
    #[serde(default = "list_object")]
    pub object: String,
    #[serde(default)]
    pub data: Vec<GenAiSpan>,
    #[serde(default)]
    pub first_id: Option<String>,
    #[serde(default)]
    pub last_id: Option<String>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub next: Option<String>,
    /// Fields this SDK build does not model, kept so the record serializes
    /// back unchanged.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
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
            extra: BTreeMap::new(),
        }
    }
}

fn list_object() -> String {
    "list".to_string()
}
