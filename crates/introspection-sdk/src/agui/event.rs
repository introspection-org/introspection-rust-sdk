//! AG-UI protocol event types.
//!
//! These types mirror the [AG-UI protocol] event taxonomy of
//! `@ag-ui/core@1.0.5` exactly, as emitted by the Introspection platform and
//! validated by the JS (`@ag-ui/core`) and Python (`ag-ui-protocol`) SDKs. We
//! own the types rather than depend on a third-party Rust port because the
//! published Rust crates diverge from the dialect Introspection speaks on the
//! wire — they lack the `REASONING_*` event family and type message/thread/run
//! ids as UUIDs, whereas Introspection uses the canonical `@ag-ui/core`
//! contract: plain-string ids (the worker emits ids like `{run_id}:text:0`)
//! and the full reasoning taxonomy.
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
//! Unknown payload fields are ignored on read (mirroring `@ag-ui/core`'s
//! `passthrough` schemas). A frame whose `type` this SDK does not know, or
//! whose fields do not satisfy that type's `@ag-ui/core` schema (a missing
//! required field, a value outside an enum), deserializes to
//! [`Event::Unknown`] with the frame kept, rather than failing — so a future
//! protocol addition never breaks an in-flight stream.
//!
//! `tests/agui_schema_parity_test.rs` holds these types to a JSON Schema
//! snapshot of `@ag-ui/core` (`tests/fixtures/ag-ui-core.schema.json`).
//!
//! [AG-UI protocol]: https://github.com/ag-ui-protocol/ag-ui

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::de::{keep_null, non_empty};
use super::message::{Message, MessageContent};
use super::patch::JsonPatch;
use super::RunAgentInput;

/// Discriminant for an AG-UI [`Event`]. Wire values are `SCREAMING_SNAKE_CASE`.
///
/// AG-UI 1.0 removed the `THINKING_*` family in favour of `REASONING_*`; a
/// legacy `THINKING_*` frame decodes to [`Event::Unknown`].
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

impl EventType {
    /// Every event type, in `@ag-ui/core`'s order.
    pub const ALL: [EventType; 31] = [
        EventType::TextMessageStart,
        EventType::TextMessageContent,
        EventType::TextMessageEnd,
        EventType::TextMessageChunk,
        EventType::ToolCallStart,
        EventType::ToolCallArgs,
        EventType::ToolCallEnd,
        EventType::ToolCallChunk,
        EventType::ToolCallResult,
        EventType::StateSnapshot,
        EventType::StateDelta,
        EventType::MessagesSnapshot,
        EventType::ActivitySnapshot,
        EventType::ActivityDelta,
        EventType::Raw,
        EventType::Custom,
        EventType::RunStarted,
        EventType::RunFinished,
        EventType::RunError,
        EventType::StepStarted,
        EventType::StepFinished,
        EventType::ReasoningStart,
        EventType::ReasoningMessageStart,
        EventType::ReasoningMessageContent,
        EventType::ReasoningMessageEnd,
        EventType::ReasoningMessageChunk,
        EventType::ReasoningEnd,
        EventType::ReasoningEncryptedValue,
        EventType::SubagentStarted,
        EventType::SubagentFinished,
        EventType::SubagentError,
    ];
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseEvent {
    /// Unix timestamp in milliseconds, when present.
    pub timestamp: Option<i64>,
    /// Opaque provider event passed through for debugging.
    #[serde(default, deserialize_with = "keep_null")]
    pub raw_event: Option<Value>,
    /// Extra information attached to the event, open by key.
    pub metadata: Option<Map<String, Value>>,
}

/// The role a text message is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextMessageRole {
    Developer,
    System,
    Assistant,
    User,
}

/// The `role` of a reasoning message, which AG-UI fixes at `reasoning`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningRole {
    #[default]
    Reasoning,
}

/// The `role` of a tool result, which AG-UI fixes at `tool`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolRole {
    #[default]
    Tool,
}

/// What a [`ReasoningEncryptedValueEvent`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningEncryptedValueSubtype {
    ToolCall,
    Message,
}

/// Start of an assistant text message.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub role: Option<TextMessageRole>,
    pub name: Option<String>,
}

/// A streaming text delta appended to an open message.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageContentEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub delta: String,
}

/// End of an assistant text message.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
}

/// A complete (non-streaming) text message chunk.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageChunkEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: Option<String>,
    pub role: Option<TextMessageRole>,
    pub delta: Option<String>,
    pub name: Option<String>,
}

/// Start of a tool call.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub tool_call_id: String,
    pub tool_call_name: String,
    pub parent_message_id: Option<String>,
}

/// A streaming chunk of tool-call arguments.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallArgsEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub tool_call_id: String,
    pub delta: String,
}

/// End of a tool call.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub tool_call_id: String,
}

/// A complete (non-streaming) tool-call chunk.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallChunkEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_call_name: Option<String>,
    pub parent_message_id: Option<String>,
    pub delta: Option<String>,
}

/// Result of executing a tool call.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResultEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub tool_call_id: String,
    pub content: MessageContent,
    pub role: Option<ToolRole>,
}

/// Boundary marker opening a reasoning step.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
}

/// Boundary marker closing a reasoning step.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
}

/// Start of a reasoning (chain-of-thought) message.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub role: ReasoningRole,
}

/// A streaming delta of reasoning text.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageContentEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub delta: String,
}

/// End of a reasoning message.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
}

/// A complete (non-streaming) reasoning chunk.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageChunkEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: Option<String>,
    pub delta: Option<String>,
}

/// An encrypted, opaque reasoning value (e.g. redacted provider thinking).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEncryptedValueEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub subtype: ReasoningEncryptedValueSubtype,
    pub entity_id: String,
    pub encrypted_value: String,
}

/// A full state snapshot.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSnapshotEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub snapshot: Value,
}

/// An incremental state update expressed as a JSON Patch.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDeltaEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub delta: JsonPatch,
}

/// A snapshot of the full message list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessagesSnapshotEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub messages: Vec<Message>,
}

/// A snapshot of activity (e.g. tool/compaction progress) state.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshotEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub activity_type: String,
    pub content: Value,
    pub replace: Option<bool>,
}

/// An incremental activity update expressed as a JSON Patch.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDeltaEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub message_id: String,
    pub activity_type: String,
    pub patch: JsonPatch,
}

/// A raw, provider-native event passed through verbatim.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub event: Value,
    pub source: Option<String>,
}

/// An application-specific event on the sanctioned `CUSTOM` channel.
///
/// Introspection rides this channel for its own markers — e.g. the
/// `introspection.reconnect` event the resumable stream surfaces — so they are
/// expressible identically in every language Introspection supports.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub name: String,
    pub value: Value,
}

/// A run has started.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStartedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub thread_id: String,
    pub run_id: String,
    pub parent_run_id: Option<String>,
    pub protocol_version: Option<String>,
    /// The input that started the run (boxed: it outweighs every other event).
    pub input: Option<Box<RunAgentInput>>,
}

/// A run has finished without failing. An absent `outcome` means success.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinishedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub thread_id: String,
    pub run_id: String,
    #[serde(default, deserialize_with = "keep_null")]
    pub result: Option<Value>,
    pub outcome: Option<RunFinishedOutcome>,
    pub usage: Option<Vec<TokenUsage>>,
}

/// Why a run ended, discriminated by `type`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RunFinishedOutcome {
    /// The run completed, possibly leaving frontend tool calls to answer.
    #[serde(rename_all = "camelCase")]
    Success {
        pending_tool_call_ids: Option<Vec<String>>,
    },
    /// The run is paused waiting on these interrupts (at least one).
    Interrupt {
        #[serde(deserialize_with = "non_empty")]
        interrupts: Vec<Interrupt>,
    },
    /// The run was stopped before it completed, and did not fail.
    Cancelled,
}

/// A question a paused run is waiting on.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interrupt {
    pub subagent_run_id: Option<String>,
    pub id: String,
    pub reason: String,
    pub message: Option<String>,
    pub tool_call_id: Option<String>,
    /// A JSON Schema the answer must satisfy.
    #[serde(default, deserialize_with = "keep_null")]
    pub response_schema: Option<Value>,
    pub expires_at: Option<String>,
    pub metadata: Option<Map<String, Value>>,
}

/// Token counts for one provider and model.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
}

/// A run ended with an error.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunErrorEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub message: String,
    pub code: Option<String>,
    pub usage: Option<Vec<TokenUsage>>,
}

/// A step within a run has started.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub step_name: String,
}

/// A step within a run has finished.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFinishedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: Option<String>,
    pub step_name: String,
}

/// A subagent invocation has begun. Later events it produces carry its
/// `subagentRunId`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentStartedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: String,
    pub name: String,
    pub description: Option<String>,
    pub parent_subagent_run_id: Option<String>,
    pub parent_tool_call_id: Option<String>,
    pub parent_message_id: Option<String>,
}

/// A subagent invocation's segment of the run has ended.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentFinishedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: String,
    #[serde(default, deserialize_with = "keep_null")]
    pub result: Option<Value>,
    pub outcome: Option<SubagentFinishedOutcome>,
}

/// How a subagent's segment ended, discriminated by `type`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SubagentFinishedOutcome {
    /// The subagent completed.
    Success,
    /// The subagent is paused on these interrupts.
    #[serde(rename_all = "camelCase")]
    Suspended { interrupt_ids: Option<Vec<String>> },
}

/// A subagent invocation failed; the run may continue.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentErrorEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub subagent_run_id: String,
    pub message: String,
    pub code: Option<String>,
}

/// A single AG-UI protocol event.
///
/// Internally tagged by `type`. A frame with an unrecognised `type`, or one
/// that does not satisfy its type's schema, deserializes to
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
    /// An event this SDK version does not model, or that does not satisfy
    /// its type's schema, kept as the server sent it (every field, `type`
    /// included).
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
    use serde_json::json;

    fn decode(frame: Value) -> Event {
        serde_json::from_value(frame).unwrap()
    }

    #[test]
    fn deserializes_text_content_with_string_id() {
        // Worker emits non-UUID message ids like `{run_id}:text:0`.
        match decode(json!({"type":"TEXT_MESSAGE_CONTENT","messageId":"run-1:text:0","delta":"hi"}))
        {
            Event::TextMessageContent(e) => {
                assert_eq!(e.message_id, "run-1:text:0");
                assert_eq!(e.delta, "hi");
            }
            other => panic!("expected TextMessageContent, got {other:?}"),
        }
    }

    #[test]
    fn unknown_type_round_trips_every_field() {
        let frame =
            json!({"type": "SOME_FUTURE_EVENT", "timestamp": 1, "nested": {"a": [1, null]}});
        let ev = decode(frame.clone());
        assert_eq!(ev, Event::Unknown(frame.clone()));
        assert_eq!(ev.event_type(), None);
        assert_eq!(serde_json::to_value(&ev).unwrap(), frame);
    }

    #[test]
    fn ignores_unknown_payload_fields() {
        // Mirrors @ag-ui/core's passthrough schemas — extra fields don't fail.
        let ev = decode(json!({"type":"RUN_STARTED","threadId":"t","runId":"r","extra":true}));
        assert!(matches!(ev, Event::RunStarted(e) if e.thread_id == "t" && e.run_id == "r"));
    }

    #[test]
    fn a_frame_outside_the_schema_is_unknown() {
        for frame in [
            json!({"type": "THINKING_START"}),
            json!({"type": "TEXT_MESSAGE_START", "messageId": "m", "role": "tool"}),
            json!({"type": "TEXT_MESSAGE_START", "messageId": "m", "timestamp": 1.5}),
            json!({"type": "REASONING_START"}),
            json!({"type": "CUSTOM", "name": "n"}),
            json!({"type": "RUN_FINISHED", "threadId": "t", "runId": "r",
                   "outcome": {"type": "interrupt", "interrupts": []}}),
            json!({"type": "STATE_DELTA", "delta": [{"op": "add", "path": "a", "value": 1}]}),
        ] {
            assert_eq!(decode(frame.clone()), Event::Unknown(frame));
        }
    }

    #[test]
    fn round_trips_custom_event() {
        let ev = Event::Custom(CustomEvent {
            base: BaseEvent::default(),
            subagent_run_id: None,
            name: "introspection.reconnect".to_string(),
            value: json!({"reason": "severed"}),
        });
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(
            json,
            json!({"type": "CUSTOM", "name": "introspection.reconnect", "value": {"reason": "severed"}})
        );
        assert_eq!(decode(json), ev);
    }

    #[test]
    fn run_finished_outcome_is_typed() {
        let ev = decode(json!({
            "type": "RUN_FINISHED", "threadId": "t", "runId": "r",
            "outcome": {"type": "interrupt", "interrupts": [{"id": "i1", "reason": "approval"}]},
            "usage": [{"model": "m", "inputTokens": 3}],
        }));
        let Event::RunFinished(e) = ev else {
            panic!("expected RunFinished")
        };
        let Some(RunFinishedOutcome::Interrupt { interrupts }) = e.outcome else {
            panic!("expected an interrupt outcome")
        };
        assert_eq!(interrupts[0].id, "i1");
        assert_eq!(e.usage.unwrap()[0].input_tokens, Some(3));
    }

    #[test]
    fn explicit_nulls_in_open_values_survive_a_round_trip() {
        let frame = json!({"type": "RUN_FINISHED", "threadId": "t", "runId": "r", "result": null});
        assert_eq!(serde_json::to_value(decode(frame.clone())).unwrap(), frame);
    }

    #[test]
    fn event_type_round_trips_through_serde() {
        let json = serde_json::to_string(&EventType::RunFinished).unwrap();
        assert_eq!(json, r#""RUN_FINISHED""#);
        assert_eq!(EventType::RunFinished.to_string(), "RUN_FINISHED");
    }
}
