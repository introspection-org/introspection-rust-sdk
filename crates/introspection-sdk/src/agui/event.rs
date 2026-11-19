//! AG-UI protocol event types.
//!
//! These types faithfully mirror the [AG-UI protocol] 1.0 event taxonomy as
//! emitted by the Introspection platform and validated by the JS
//! (`@ag-ui/core`) and Python (`ag-ui-protocol`) SDKs. We own the types
//! rather than depend on a third-party Rust port because the published Rust
//! crates diverge from the dialect Introspection speaks on the wire — they
//! lack the `REASONING_*` event family and type message/thread/run ids as
//! UUIDs, whereas Introspection uses the canonical `@ag-ui/core` contract:
//! plain-string ids (the worker emits ids like `{run_id}:text:0`) and the
//! full reasoning taxonomy.
//!
//! Owning the types also lets us add Introspection-specific extensions on the
//! sanctioned `CUSTOM` channel (e.g. the `introspection.reconnect` marker)
//! without forking an upstream crate.
//!
//! # Serialization
//!
//! [`Event`] is internally tagged by a `type` discriminant in
//! `SCREAMING_SNAKE_CASE`; per-event fields serialize as `camelCase`
//! (`messageId`, `threadId`, …), matching the wire format byte-for-byte.
//! Unknown event payload fields are ignored on read (mirroring `@ag-ui/core`'s
//! `passthrough` schemas), and an unrecognised `type` deserializes to
//! [`Event::Unknown`] rather than failing — so a future protocol addition
//! never breaks an in-flight stream.
//!
//! Every event except the run-scoped ones (`RUN_*`, `MESSAGES_SNAPSHOT`) and
//! the `SUBAGENT_*` family carries an optional `subagent_run_id` naming the
//! subagent invocation that produced it.
//!
//! [AG-UI protocol]: https://github.com/ag-ui-protocol/ag-ui

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Discriminant for an AG-UI [`Event`]. Wire values are `SCREAMING_SNAKE_CASE`.
///
/// Mirrors `@ag-ui/core@1.0.5`. AG-UI 1.0 removed the `THINKING_*` family in
/// favour of `REASONING_*`; a legacy `THINKING_*` frame decodes to
/// [`Event::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventType {
    TextMessageStart,
    TextMessageContent,
    TextMessageEnd,
    TextMessageChunk,
    ToolCallStart,
    ToolCallArgs,
    ToolCallEnd,
    ToolCallChunk,
    ToolCallResult,
    StateSnapshot,
    StateDelta,
    MessagesSnapshot,
    ActivitySnapshot,
    ActivityDelta,
    Raw,
    Custom,
    RunStarted,
    RunFinished,
    RunError,
    StepStarted,
    StepFinished,
    ReasoningStart,
    ReasoningMessageStart,
    ReasoningMessageContent,
    ReasoningMessageEnd,
    ReasoningMessageChunk,
    ReasoningEnd,
    ReasoningEncryptedValue,
    SubagentStarted,
    SubagentFinished,
    SubagentError,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Reuse the serde mapping so the rendered name matches the wire value.
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        f.write_str(&s)
    }
}

/// Fields common to every event, flattened into each payload struct.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BaseEvent {
    /// Unix timestamp in milliseconds, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
    /// Opaque provider event passed through for debugging.
    #[serde(rename = "rawEvent", default, skip_serializing_if = "Option::is_none")]
    pub raw_event: Option<Value>,
    /// Extra information attached to the event, open by key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

/// Start of an assistant text message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A streaming text delta appended to an open message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageContentEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub delta: String,
}

/// End of an assistant text message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub message_id: String,
}

/// A complete (non-streaming) text message chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageChunkEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Start of a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub tool_call_id: String,
    pub tool_call_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<String>,
}

/// A streaming chunk of tool-call arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallArgsEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub tool_call_id: String,
    pub delta: String,
}

/// End of a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub tool_call_id: String,
}

/// A complete (non-streaming) tool-call chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallChunkEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
}

/// Result of executing a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResultEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub tool_call_id: String,
    pub content: ToolCallResultContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// What a tool returned: a string, or (since AG-UI 1.0) a list of content
/// parts, kept as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolCallResultContent {
    Text(String),
    Parts(Vec<Value>),
    Unknown(Value),
}

impl ToolCallResultContent {
    /// The content when it is a plain string.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ToolCallResultContent::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// Boundary marker opening a reasoning step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// Boundary marker closing a reasoning step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// Start of a reasoning (chain-of-thought) message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// A streaming delta of reasoning text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageContentEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub delta: String,
}

/// End of a reasoning message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub message_id: String,
}

/// A complete (non-streaming) reasoning chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageChunkEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
}

/// An encrypted, opaque reasoning value (e.g. redacted provider thinking).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEncryptedValueEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    pub encrypted_value: String,
}

/// A full state snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSnapshotEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub snapshot: Value,
}

/// An incremental state update expressed as a JSON Patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDeltaEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub delta: Value,
}

/// A snapshot of the full message list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagesSnapshotEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub messages: Vec<Value>,
}

/// A snapshot of activity (e.g. tool/compaction progress) state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshotEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub activity_type: String,
    pub content: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<bool>,
}

/// An incremental activity update expressed as a JSON Patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDeltaEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub activity_type: String,
    pub patch: Value,
}

/// A raw, provider-native event passed through verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub event: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// An application-specific event on the sanctioned `CUSTOM` channel.
///
/// Introspection rides this channel for its own markers — e.g. the
/// `introspection.reconnect` event the resumable stream surfaces — so they are
/// expressible identically in every language Introspection supports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub value: Value,
}

/// A run has started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStartedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub thread_id: String,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    /// The `RunAgentInput` that started the run, kept as JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
}

/// A run has finished without failing.
///
/// `outcome` stays raw JSON; [`RunFinishedEvent::parsed_outcome`] reads it as
/// a [`RunFinishedOutcome`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinishedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub thread_id: String,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Vec<TokenUsage>>,
}

impl RunFinishedEvent {
    /// Why the run ended. An absent outcome means success.
    pub fn parsed_outcome(&self) -> RunFinishedOutcome {
        match &self.outcome {
            None | Some(Value::Null) => RunFinishedOutcome::Success {
                pending_tool_call_ids: None,
            },
            Some(v) => serde_json::from_value(v.clone())
                .unwrap_or_else(|_| RunFinishedOutcome::Unknown(v.clone())),
        }
    }
}

/// Why a run ended, discriminated by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RunFinishedOutcome {
    /// The run completed, possibly leaving frontend tool calls to answer.
    #[serde(rename_all = "camelCase")]
    Success {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pending_tool_call_ids: Option<Vec<String>>,
    },
    /// The run is paused waiting on these interrupts (kept as JSON).
    Interrupt { interrupts: Vec<Value> },
    /// The run was stopped before it completed, and did not fail.
    Cancelled,
    /// An outcome this SDK version does not recognise, kept as sent.
    #[serde(untagged)]
    Unknown(Value),
}

/// Token counts for one provider and model.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<u64>,
}

/// A run ended with an error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunErrorEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Vec<TokenUsage>>,
}

/// A step within a run has started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub step_name: String,
}

/// A step within a run has finished.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFinishedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_run_id: Option<String>,
    pub step_name: String,
}

/// A subagent invocation has begun. Later events it produces carry its
/// `subagentRunId`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentStartedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_subagent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<String>,
}

/// A subagent invocation's segment of the run has ended. `outcome` is
/// `success` or `suspended`, kept as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentFinishedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Value>,
}

/// A subagent invocation failed; the run may continue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentErrorEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// A single AG-UI protocol event.
///
/// Internally tagged by `type`. An unrecognised `type` deserializes to
/// [`Event::Unknown`] (the frame is kept verbatim) so a forward-compatible
/// protocol addition never severs an in-flight stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    TextMessageStart(TextMessageStartEvent),
    TextMessageContent(TextMessageContentEvent),
    TextMessageEnd(TextMessageEndEvent),
    TextMessageChunk(TextMessageChunkEvent),
    ToolCallStart(ToolCallStartEvent),
    ToolCallArgs(ToolCallArgsEvent),
    ToolCallEnd(ToolCallEndEvent),
    ToolCallChunk(ToolCallChunkEvent),
    ToolCallResult(ToolCallResultEvent),
    StateSnapshot(StateSnapshotEvent),
    StateDelta(StateDeltaEvent),
    MessagesSnapshot(MessagesSnapshotEvent),
    ActivitySnapshot(ActivitySnapshotEvent),
    ActivityDelta(ActivityDeltaEvent),
    Raw(RawEvent),
    Custom(CustomEvent),
    RunStarted(RunStartedEvent),
    RunFinished(RunFinishedEvent),
    RunError(RunErrorEvent),
    StepStarted(StepStartedEvent),
    StepFinished(StepFinishedEvent),
    ReasoningStart(ReasoningStartEvent),
    ReasoningMessageStart(ReasoningMessageStartEvent),
    ReasoningMessageContent(ReasoningMessageContentEvent),
    ReasoningMessageEnd(ReasoningMessageEndEvent),
    ReasoningMessageChunk(ReasoningMessageChunkEvent),
    ReasoningEnd(ReasoningEndEvent),
    ReasoningEncryptedValue(ReasoningEncryptedValueEvent),
    SubagentStarted(SubagentStartedEvent),
    SubagentFinished(SubagentFinishedEvent),
    SubagentError(SubagentErrorEvent),
    /// An event whose `type` this SDK version does not model, kept as the
    /// server sent it (every field, `type` included).
    #[serde(untagged)]
    Unknown(Value),
}

impl Event {
    /// The [`EventType`] discriminant for this event, or `None` for
    /// [`Event::Unknown`].
    pub fn event_type(&self) -> Option<EventType> {
        Some(match self {
            Event::TextMessageStart(_) => EventType::TextMessageStart,
            Event::TextMessageContent(_) => EventType::TextMessageContent,
            Event::TextMessageEnd(_) => EventType::TextMessageEnd,
            Event::TextMessageChunk(_) => EventType::TextMessageChunk,
            Event::ToolCallStart(_) => EventType::ToolCallStart,
            Event::ToolCallArgs(_) => EventType::ToolCallArgs,
            Event::ToolCallEnd(_) => EventType::ToolCallEnd,
            Event::ToolCallChunk(_) => EventType::ToolCallChunk,
            Event::ToolCallResult(_) => EventType::ToolCallResult,
            Event::StateSnapshot(_) => EventType::StateSnapshot,
            Event::StateDelta(_) => EventType::StateDelta,
            Event::MessagesSnapshot(_) => EventType::MessagesSnapshot,
            Event::ActivitySnapshot(_) => EventType::ActivitySnapshot,
            Event::ActivityDelta(_) => EventType::ActivityDelta,
            Event::Raw(_) => EventType::Raw,
            Event::Custom(_) => EventType::Custom,
            Event::RunStarted(_) => EventType::RunStarted,
            Event::RunFinished(_) => EventType::RunFinished,
            Event::RunError(_) => EventType::RunError,
            Event::StepStarted(_) => EventType::StepStarted,
            Event::StepFinished(_) => EventType::StepFinished,
            Event::ReasoningStart(_) => EventType::ReasoningStart,
            Event::ReasoningMessageStart(_) => EventType::ReasoningMessageStart,
            Event::ReasoningMessageContent(_) => EventType::ReasoningMessageContent,
            Event::ReasoningMessageEnd(_) => EventType::ReasoningMessageEnd,
            Event::ReasoningMessageChunk(_) => EventType::ReasoningMessageChunk,
            Event::ReasoningEnd(_) => EventType::ReasoningEnd,
            Event::ReasoningEncryptedValue(_) => EventType::ReasoningEncryptedValue,
            Event::SubagentStarted(_) => EventType::SubagentStarted,
            Event::SubagentFinished(_) => EventType::SubagentFinished,
            Event::SubagentError(_) => EventType::SubagentError,
            Event::Unknown(_) => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_text_content_with_string_id() {
        // Worker emits non-UUID message ids like `{run_id}:text:0`.
        let ev: Event = serde_json::from_str(
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"run-1:text:0","delta":"hi"}"#,
        )
        .unwrap();
        match ev {
            Event::TextMessageContent(e) => {
                assert_eq!(e.message_id, "run-1:text:0");
                assert_eq!(e.delta, "hi");
            }
            other => panic!("expected TextMessageContent, got {other:?}"),
        }
    }

    #[test]
    fn deserializes_reasoning_family() {
        // The whole reason syncable-ag-ui-core was unusable: REASONING_* events.
        let ev: Event = serde_json::from_str(
            r#"{"type":"REASONING_MESSAGE_CONTENT","messageId":"run-1:reasoning:0","delta":"hmm"}"#,
        )
        .unwrap();
        assert!(matches!(ev, Event::ReasoningMessageContent(_)));
        assert_eq!(ev.event_type(), Some(EventType::ReasoningMessageContent));
    }

    #[test]
    fn unknown_type_does_not_error() {
        let ev: Event = serde_json::from_str(r#"{"type":"SOME_FUTURE_EVENT","foo":1}"#).unwrap();
        assert_eq!(
            ev,
            Event::Unknown(serde_json::json!({"type": "SOME_FUTURE_EVENT", "foo": 1}))
        );
        assert_eq!(ev.event_type(), None);
    }

    #[test]
    fn unknown_event_round_trips_every_field() {
        let frame = serde_json::json!({
            "type": "SOME_FUTURE_EVENT",
            "timestamp": 1,
            "nested": {"a": [1, null]},
        });
        let ev: Event = serde_json::from_value(frame.clone()).unwrap();
        assert_eq!(serde_json::to_value(&ev).unwrap(), frame);
    }

    #[test]
    fn ignores_unknown_payload_fields() {
        // Mirrors @ag-ui/core's passthrough schemas — extra fields don't fail.
        let ev: Event = serde_json::from_str(
            r#"{"type":"RUN_STARTED","threadId":"t","runId":"r","extra":true}"#,
        )
        .unwrap();
        match ev {
            Event::RunStarted(e) => {
                assert_eq!(e.thread_id, "t");
                assert_eq!(e.run_id, "r");
            }
            other => panic!("expected RunStarted, got {other:?}"),
        }
    }

    #[test]
    fn round_trips_custom_event() {
        let ev = Event::Custom(CustomEvent {
            base: BaseEvent::default(),
            subagent_run_id: None,
            name: "introspection.reconnect".to_string(),
            value: serde_json::json!({"reason": "severed"}),
        });
        let json = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        // Tag is on the wire as `type`.
        assert!(json.contains(r#""type":"CUSTOM""#));
        assert!(json.contains(r#""name":"introspection.reconnect""#));
    }

    #[test]
    fn removed_thinking_types_decode_to_unknown() {
        for ty in [
            "THINKING_START",
            "THINKING_END",
            "THINKING_TEXT_MESSAGE_START",
            "THINKING_TEXT_MESSAGE_CONTENT",
            "THINKING_TEXT_MESSAGE_END",
        ] {
            let frame = serde_json::json!({"type": ty, "messageId": "m", "delta": "hmm"});
            let ev: Event = serde_json::from_value(frame.clone()).unwrap();
            assert_eq!(ev, Event::Unknown(frame.clone()), "{ty}");
            assert_eq!(serde_json::to_value(&ev).unwrap(), frame, "{ty}");
        }
    }

    #[test]
    fn run_finished_outcomes() {
        let outcome = |json: &str| match serde_json::from_str::<Event>(json).unwrap() {
            Event::RunFinished(e) => e.parsed_outcome(),
            other => panic!("expected RunFinished, got {other:?}"),
        };
        assert_eq!(
            outcome(r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r"}"#),
            RunFinishedOutcome::Success {
                pending_tool_call_ids: None
            }
        );
        assert_eq!(
            outcome(
                r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r","outcome":{"type":"cancelled"}}"#
            ),
            RunFinishedOutcome::Cancelled
        );
        assert_eq!(
            outcome(
                r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r","outcome":{"type":"success","pendingToolCallIds":["c1"]}}"#
            ),
            RunFinishedOutcome::Success {
                pending_tool_call_ids: Some(vec!["c1".into()])
            }
        );
        match outcome(
            r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r","outcome":{"type":"interrupt","interrupts":[{"id":"i1","reason":"approval"}]}}"#,
        ) {
            RunFinishedOutcome::Interrupt { interrupts } => assert_eq!(interrupts[0]["id"], "i1"),
            other => panic!("expected Interrupt, got {other:?}"),
        }
        assert_eq!(
            outcome(
                r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r","outcome":{"type":"later"}}"#
            ),
            RunFinishedOutcome::Unknown(serde_json::json!({"type": "later"}))
        );
    }

    #[test]
    fn run_finished_outcome_stays_raw_json() {
        let ev: Event = serde_json::from_str(
            r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r","outcome":{"type":"cancelled"},"usage":[{"model":"m","inputTokens":3}]}"#,
        )
        .unwrap();
        let Event::RunFinished(e) = ev else {
            panic!("expected RunFinished")
        };
        assert_eq!(e.outcome.unwrap()["type"], "cancelled");
        assert_eq!(e.usage.unwrap()[0].input_tokens, Some(3));
    }

    #[test]
    fn decodes_subagent_attribution_and_metadata() {
        let ev: Event = serde_json::from_str(
            r#"{"type":"TEXT_MESSAGE_START","messageId":"m","role":"assistant","name":"helper","subagentRunId":"s1","metadata":{"k":null}}"#,
        )
        .unwrap();
        let Event::TextMessageStart(e) = ev else {
            panic!("expected TextMessageStart")
        };
        assert_eq!(e.subagent_run_id.as_deref(), Some("s1"));
        assert_eq!(e.name.as_deref(), Some("helper"));
        assert_eq!(e.base.metadata.unwrap()["k"], Value::Null);

        let ev: Event = serde_json::from_str(
            r#"{"type":"SUBAGENT_STARTED","subagentRunId":"s1","name":"researcher","parentToolCallId":"c1"}"#,
        )
        .unwrap();
        assert_eq!(ev.event_type(), Some(EventType::SubagentStarted));
        let ev: Event = serde_json::from_str(
            r#"{"type":"SUBAGENT_FINISHED","subagentRunId":"s1","outcome":{"type":"suspended"}}"#,
        )
        .unwrap();
        assert!(
            matches!(ev, Event::SubagentFinished(e) if e.outcome.as_ref().unwrap()["type"] == "suspended")
        );
        let ev: Event = serde_json::from_str(
            r#"{"type":"SUBAGENT_ERROR","subagentRunId":"s1","message":"boom"}"#,
        )
        .unwrap();
        assert!(matches!(ev, Event::SubagentError(e) if e.message == "boom"));
    }

    #[test]
    fn tool_call_result_content_is_a_string_or_parts() {
        let content = |json: &str| match serde_json::from_str::<Event>(json).unwrap() {
            Event::ToolCallResult(e) => e.content,
            other => panic!("expected ToolCallResult, got {other:?}"),
        };
        let text = content(
            r#"{"type":"TOOL_CALL_RESULT","messageId":"m","toolCallId":"c","content":"ok"}"#,
        );
        assert_eq!(text.as_text(), Some("ok"));
        let parts = content(
            r#"{"type":"TOOL_CALL_RESULT","messageId":"m","toolCallId":"c","content":[{"type":"text","text":"ok"}]}"#,
        );
        assert!(matches!(parts, ToolCallResultContent::Parts(p) if p[0]["text"] == "ok"));
    }

    #[test]
    fn event_type_round_trips_through_serde() {
        let json = serde_json::to_string(&EventType::RunFinished).unwrap();
        assert_eq!(json, r#""RUN_FINISHED""#);
        assert_eq!(EventType::RunFinished.to_string(), "RUN_FINISHED");
    }
}
