//! AG-UI protocol event types.
//!
//! These types faithfully mirror the [AG-UI protocol] event taxonomy as
//! emitted by the Introspection runtime-worker and validated by the JS
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
//! [AG-UI protocol]: https://github.com/ag-ui-protocol/ag-ui

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Discriminant for an AG-UI [`Event`]. Wire values are `SCREAMING_SNAKE_CASE`.
///
/// Mirrors `@ag-ui/core@0.0.57`. The `THINKING_*` family is deprecated
/// upstream in favour of `REASONING_*` but kept here so legacy frames still
/// type-check.
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
    /// Deprecated upstream — use [`EventType::ReasoningStart`].
    ThinkingStart,
    /// Deprecated upstream — use [`EventType::ReasoningEnd`].
    ThinkingEnd,
    /// Deprecated upstream — use [`EventType::ReasoningMessageStart`].
    ThinkingTextMessageStart,
    /// Deprecated upstream — use [`EventType::ReasoningMessageContent`].
    ThinkingTextMessageContent,
    /// Deprecated upstream — use [`EventType::ReasoningMessageEnd`].
    ThinkingTextMessageEnd,
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
}

/// Start of an assistant text message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// A streaming text delta appended to an open message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageContentEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub message_id: String,
    pub delta: String,
}

/// End of an assistant text message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub message_id: String,
}

/// A complete (non-streaming) text message chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageChunkEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
}

/// Start of a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
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
    pub tool_call_id: String,
    pub delta: String,
}

/// End of a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub tool_call_id: String,
}

/// A complete (non-streaming) tool-call chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallChunkEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
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
    pub message_id: String,
    pub tool_call_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Boundary marker opening a reasoning step (or its deprecated `THINKING_*`
/// alias).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Boundary marker closing a reasoning step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// Start of a reasoning (chain-of-thought) message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageStartEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
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
    pub message_id: String,
    pub delta: String,
}

/// End of a reasoning message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageEndEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub message_id: String,
}

/// A complete (non-streaming) reasoning chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageChunkEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
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
    pub snapshot: Value,
}

/// An incremental state update expressed as a JSON Patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDeltaEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
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
    pub event: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// An application-specific event on the sanctioned `CUSTOM` channel.
///
/// Introspection rides this channel for its own markers — e.g. the
/// `introspection.reconnect` event the resumable stream surfaces — so they are
/// expressible identically across the JS / Python / Rust SDKs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
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
}

/// A run has finished. `result` / `outcome` are present on success paths.
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
}

/// A step within a run has started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub step_name: String,
}

/// A step within a run has finished.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFinishedEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub step_name: String,
}

/// A single AG-UI protocol event.
///
/// Internally tagged by `type`. An unrecognised `type` deserializes to
/// [`Event::Unknown`] (the body is not retained) so a forward-compatible
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
    ThinkingStart(ReasoningStartEvent),
    ThinkingEnd(ReasoningEndEvent),
    ThinkingTextMessageStart(ReasoningMessageStartEvent),
    ThinkingTextMessageContent(ReasoningMessageContentEvent),
    ThinkingTextMessageEnd(ReasoningMessageEndEvent),
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
    /// An event whose `type` is not recognised by this SDK version. The
    /// payload is not retained; the stream continues uninterrupted.
    #[serde(other)]
    Unknown,
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
            Event::ThinkingStart(_) => EventType::ThinkingStart,
            Event::ThinkingEnd(_) => EventType::ThinkingEnd,
            Event::ThinkingTextMessageStart(_) => EventType::ThinkingTextMessageStart,
            Event::ThinkingTextMessageContent(_) => EventType::ThinkingTextMessageContent,
            Event::ThinkingTextMessageEnd(_) => EventType::ThinkingTextMessageEnd,
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
            Event::Unknown => return None,
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
        assert_eq!(ev, Event::Unknown);
        assert_eq!(ev.event_type(), None);
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
    fn event_type_round_trips_through_serde() {
        let json = serde_json::to_string(&EventType::RunFinished).unwrap();
        assert_eq!(json, r#""RUN_FINISHED""#);
        assert_eq!(EventType::RunFinished.to_string(), "RUN_FINISHED");
    }
}
