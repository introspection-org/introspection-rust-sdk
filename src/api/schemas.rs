//! Wire types for the DP `/v1/tasks` and `/v1/files` surface.
//!
//! Kept in lockstep with the published Data Plane OpenAPI schema.
//!
//! Field names are kept on-the-wire (`snake_case`) so the JSON round-trips
//! verbatim — no camelCase translation layer.
//!
//! Unknown fields on responses are silently ignored. Enum values added by
//! the DP after this SDK is compiled deserialize into the `Other`/`Unknown`
//! fallback variant so callers can still read the rest of the record.

use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringOrUuid {
    String(String),
    Uuid(Uuid),
}

impl Default for StringOrUuid {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl From<String> for StringOrUuid {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for StringOrUuid {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<Uuid> for StringOrUuid {
    fn from(value: Uuid) -> Self {
        Self::Uuid(value)
    }
}

impl fmt::Display for StringOrUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => f.write_str(value),
            Self::Uuid(value) => write!(f, "{value}"),
        }
    }
}

impl Serialize for StringOrUuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::String(value) => serializer.serialize_str(value),
            Self::Uuid(value) => serializer.serialize_str(&value.to_string()),
        }
    }
}

// ----- enums -----------------------------------------------------------------

/// Execution shape of a task — mirrors the DP `TaskKind` enum.
///
/// `Agent` boots the runtime-agent image and runs an interactive LLM agent;
/// `Process` runs a one-shot baked script and reports through the same
/// completion path.
///
/// This replaced the retired `TaskMode`. There are no task modes any more:
/// every agent task is a conversation, and the recipe agent is selected with
/// `TaskCreate::agent_name`.
///
/// The `Other` variant captures any new kind added by the DP that the SDK
/// has not been recompiled against. The string is the raw on-the-wire value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TaskKind {
    #[default]
    Agent,
    Process,
    /// Forward-compatible escape hatch for kinds the DP adds later.
    Other(String),
}

impl TaskKind {
    /// On-the-wire string form.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Agent => "agent",
            Self::Process => "process",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for TaskKind {
    fn from(s: &str) -> Self {
        match s {
            "agent" => Self::Agent,
            "process" => Self::Process,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for TaskKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TaskKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Status of a task or run — mirrors the DP `TaskStatus` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Queued,
    Scheduled,
    Running,
    AwaitingUser,
    Idle,
    Completed,
    Failed,
    Cancelling,
    Cancelled,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl TaskStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Scheduled => "scheduled",
            Self::Running => "running",
            Self::AwaitingUser => "awaiting_user",
            Self::Idle => "idle",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for TaskStatus {
    fn from(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "queued" => Self::Queued,
            "scheduled" => Self::Scheduled,
            "running" => Self::Running,
            "awaiting_user" => Self::AwaitingUser,
            "idle" => Self::Idle,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelling" => Self::Cancelling,
            "cancelled" => Self::Cancelled,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for TaskStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TaskStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Mirrors the DP `FileType` enum.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FileType {
    Upload,
    Filesystem,
    #[default]
    Other,
    /// Forward-compatible escape hatch.
    Unknown(String),
}

impl FileType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Upload => "upload",
            Self::Filesystem => "filesystem",
            Self::Other => "other",
            Self::Unknown(s) => s,
        }
    }
}

impl From<&str> for FileType {
    fn from(s: &str) -> Self {
        match s {
            "upload" => Self::Upload,
            "filesystem" => Self::Filesystem,
            "other" => Self::Other,
            other => Self::Unknown(other.to_string()),
        }
    }
}

impl Serialize for FileType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FileType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

// ----- pagination ------------------------------------------------------------

/// Cursor pagination envelope shared by every DP list endpoint.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Paginated<T> {
    pub records: Vec<T>,
    pub count: u64,
    #[serde(default)]
    pub total_count: Option<u64>,
    #[serde(default)]
    pub next: Option<String>,
}

/// Shared cursor-pagination query params for every paginated list
/// endpoint (`?limit`, `?next`, `?include_total`). Embedded by the
/// per-endpoint `*ListParams` structs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PaginationParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

// ----- tasks -----------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Task {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_index: Option<i64>,
    #[serde(default)]
    pub kind: TaskKind,
    #[serde(default = "default_task_status")]
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_user_message_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_metadata: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentInfo>,
    /// `key:value` grouping tags stamped on this task.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_task_status() -> TaskStatus {
    TaskStatus::Pending
}

/// A reference to an already-uploaded file, attached to a task or a turn.
///
/// Bytes go through `POST /v1/files` first; a task only ever carries the
/// reference. `name` is the workspace-relative path to mount at — omit it and
/// the file is mounted under its own name. It must be relative and must not
/// traverse outside the task's files directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskFileRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// One `repositories[]` entry: a repository plus the state to clone it at.
///
/// The recipe's `runtime.github.repositories` grant decides what a runtime
/// MAY clone; this decides what a task DOES clone, and at what ref. An entry
/// outside the grant is dropped by the server, never a launch failure.
///
/// `repo` is a registered `owner/name` slug. Leave `git_ref` and `depth`
/// unset to take the platform defaults: a shallow clone of the repository's
/// default branch. Nothing is stored server-side to make that work — the ref
/// stays null and git resolves the remote's HEAD, so it cannot go stale on a
/// rename.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskRepoRequest {
    pub repo: String,
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
}

/// POST /v1/tasks body. All fields optional — the DP fills in defaults.
///
/// Note there is no `runtime_id`: this client is runner-bound, and a runner
/// credential's JWT claim is authoritative for runtime selection, so the
/// field is only meaningful to browser/session callers.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskCreate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Recipe agent to run; `None` uses the recipe default (`agents/agent.yaml`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    /// Workspace repositories to clone into `workspace/repos/`. No count limit —
    /// the server refuses a statically wrong list (duplicate slugs, folder
    /// collisions), not a long one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<TaskRepoRequest>>,
    /// Files to attach before the first turn, by id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<TaskFileRef>>,
    /// Seconds the sandbox stays warm between turns before teardown. `0` tears
    /// it down as soon as it is provisioned; `None` uses the deployment default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_timeout_seconds: Option<u32>,
    /// Fork from a shared conversation: the `/v1/shares` grant id for the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork_share_id: Option<String>,
    /// Grouping tags stamped on the task at create time (e.g.
    /// `customer:acme`). A tag is an opaque, exact, case-sensitive string;
    /// `key:value` is a convention, not a grammar. Each tag is 1..128
    /// characters with no whitespace or control characters; at most 64 tags,
    /// and duplicates collapse. Filter with [`TaskListParams::tag`].
    ///
    /// Tags are access-bearing: a caller whose member tags intersect a row's
    /// tags can read and write it, so a tag shared with a member cohort hands
    /// them the task. Shared writers may not replace the tags themselves;
    /// that remains owner/privileged-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Immutable, filter-only metadata stamped onto every span in the
    /// conversation. The server validates keys, values, and cardinality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Replaces the tag list wholesale (unlike `metadata`, which is merged).
    /// `None` leaves tags untouched; `Some(vec![])` clears them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Filters supported by `GET /v1/tasks`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_total: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<TaskStatus>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_automation_id: Option<bool>,
    /// Filter by one `key:value` tag. ANDed with the ownership predicate, so
    /// it only ever narrows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPrompt {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskRunCreate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<TaskPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<TaskRunKind>,
    /// Files to attach to this turn — the way to add one mid-conversation.
    ///
    /// The workspace is built when the sandbox starts, so a file attached on a
    /// later turn is materialized into the running sandbox before that turn
    /// runs, and joins the task's set so a restart replays it. Not accepted
    /// alongside a resume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<TaskFileRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskRunKind {
    Prompt,
    Steer,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeEntry {
    pub interrupt_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskRunResume {
    pub resume: Vec<ResumeEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskRun {
    pub id: String,
    pub task_id: Uuid,
    pub status: TaskStatus,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskCreateResponse {
    pub task: Task,
    pub run: TaskRun,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskRunResponse {
    pub run: TaskRun,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskCancelResponse {
    pub id: String,
}

/// Typed body for the run-cancel endpoint.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TaskCancelOptions {
    /// Interrupt the current turn immediately while keeping the sandbox warm.
    #[default]
    Abort,
    /// Let the current turn settle, then tear down the sandbox.
    Drain {
        /// Optional upper bound before teardown is forced.
        #[serde(skip_serializing_if = "Option::is_none")]
        drain_within_seconds: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareResourceType {
    File,
    Conversation,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResourceShare {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub resource_type: ShareResourceType,
    pub resource_id: String,
    #[serde(default)]
    pub granted_member_id: Option<Uuid>,
    pub created_by_member_id: Uuid,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShareCreate {
    pub resource_type: ShareResourceType,
    pub resource_id: String,
    /// Target one member; `None` grants project-wide read. An end customer is
    /// a member, so there is no separate identity target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_member_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ShareListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<ShareResourceType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_me: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_to_me: Option<bool>,
}

// ----- files -----------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct File {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    #[serde(default)]
    pub file_type: FileType,
    pub storage_path: String,
    #[serde(default = "default_mime")]
    pub mime_type: String,
    #[serde(default)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub member_id: Option<Uuid>,
    /// Task this file was created from (accounting only).
    #[serde(default)]
    pub task_id: Option<Uuid>,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub storage_version_id: Option<String>,
    /// Grouping tags stamped on this file. Tags belong to the file rather than
    /// to a version, so they carry forward when a new version is written.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_mime() -> String {
    "application/octet-stream".to_string()
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FileUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Replaces the tag list wholesale (unlike `metadata`, which is merged).
    /// `None` leaves tags untouched; `Some(vec![])` clears them.
    ///
    /// A tag is an opaque, exact, case-sensitive string: `key:value` is a
    /// convention, not a grammar. Each tag is 1–128 characters with no
    /// whitespace or control characters; at most 64 tags.
    ///
    /// Access-bearing: a caller whose member tags intersect a file's tags can
    /// read and write it. Shared writers may not replace the tags themselves;
    /// that remains owner/privileged-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileCreateText {
    pub name: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FileListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_total: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_type: Option<FileType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
    /// Filter by one tag. ANDed with the ownership predicate, so it only
    /// ever narrows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

// ----- SSE -------------------------------------------------------------------

/// A single Server-Sent Event frame.
///
/// The API does not define the event taxonomy — frames are proxied verbatim,
/// so callers branch on `event` and parse `data`
/// themselves (typically `serde_json::from_str(&ev.data)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

impl SseEvent {
    pub(crate) fn empty() -> Self {
        Self {
            event: "message".to_string(),
            data: String::new(),
            id: None,
            retry: None,
        }
    }
}

// ----- recipes (CP) ----------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Recipe {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub repository_id: Uuid,
    pub name: String,
    pub slug: String,
    pub git_ref: String,
    pub git_commit_sha: String,
    #[serde(default)]
    pub sub_path: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub created_by_member_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RecipeListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(flatten)]
    pub pagination: PaginationParams,
}

// ----- runtimes (CP) ---------------------------------------------------------

/// How a Runtime acquires LLM provider credentials at session create —
/// mirrors the CP `RuntimeLlmMode` enum.
///
/// `Managed` (the default) uses Introspection-managed keys; `Byok` uses
/// the project's Endpoint pool. The `Other` variant captures any future
/// mode the CP adds, so callers can still read the rest of the record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RuntimeLlmMode {
    #[default]
    Managed,
    Byok,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl RuntimeLlmMode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Managed => "managed",
            Self::Byok => "byok",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for RuntimeLlmMode {
    fn from(s: &str) -> Self {
        match s {
            "managed" => Self::Managed,
            "byok" => Self::Byok,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for RuntimeLlmMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RuntimeLlmMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Runtime {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub recipe_id: Uuid,
    pub created_by_member_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub allow_hot_swap: bool,
    /// LLM credential source. Defaults to `Managed` when the CP omits
    /// the field (older servers) or sends `"managed"` explicitly.
    #[serde(default)]
    pub llm_mode: RuntimeLlmMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_json: Option<HashMap<String, serde_json::Value>>,
    /// Per-environment git ref each lane tracks (`environment` -> `main` /
    /// `pr/N` / commit sha), projected from the runtime group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_ref: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<StringOrUuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe_id: Option<Uuid>,
    /// Restrict to runtimes serving this environment. An API key already
    /// selects its environment, so passing both is a 400.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

// ----- experiments (CP) ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExperimentStatus {
    Draft,
    Running,
    Ended,
    Cancelled,
    Other(String),
}

impl ExperimentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Draft => "draft",
            Self::Running => "running",
            Self::Ended => "ended",
            Self::Cancelled => "cancelled",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for ExperimentStatus {
    fn from(s: &str) -> Self {
        match s {
            "draft" => Self::Draft,
            "running" => Self::Running,
            "ended" => Self::Ended,
            "cancelled" => Self::Cancelled,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for ExperimentStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ExperimentStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// How a composite goal's component scores combine into the optimized reward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExperimentGoalDirection {
    #[default]
    Maximize,
    Minimize,
}

/// Canary bound over one goal component's rate.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ExperimentGoalGuard {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

fn default_component_weight() -> f64 {
    1.0
}

/// Judge-backed reward component. `judge_id` comes from `GET /v1/judges` —
/// judges cannot be created through the API; author a `judges/*.yaml` in the
/// recipe repository and it syncs when a runtime versions that commit.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JudgeGoalComponent {
    pub judge_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_definition_hash: Option<String>,
    #[serde(default = "default_component_weight")]
    pub weight: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<ExperimentGoalGuard>,
}

/// Reserved shape for future telemetry-backed reward components.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelemetryGoalComponent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregation: Option<String>,
    #[serde(default = "default_component_weight")]
    pub weight: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<ExperimentGoalGuard>,
}

/// One reward component of a composite goal, discriminated on `source`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum ExperimentGoalComponent {
    Judge(JudgeGoalComponent),
    Telemetry(TelemetryGoalComponent),
}

fn default_goal_kind() -> String {
    "composite".to_string()
}

/// Composite objective the bandit optimizes. A create goal must carry at
/// least one `Judge` component with `weight > 0` — the v1 scorer only
/// implements judge-backed reward.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExperimentGoal {
    #[serde(default = "default_goal_kind")]
    pub kind: String,
    #[serde(default)]
    pub direction: ExperimentGoalDirection,
    #[serde(default)]
    pub components: Vec<ExperimentGoalComponent>,
}

impl Default for ExperimentGoal {
    fn default() -> Self {
        Self {
            kind: default_goal_kind(),
            direction: ExperimentGoalDirection::default(),
            components: Vec::new(),
        }
    }
}

/// One arm of an experiment — a Runtime version in the experiment's group
/// plus a display label. On create only `runtime_id`, `arm_label`, and the
/// optional `agent_overrides` are sent; reads may carry additional
/// server-set fields (arm id, seeded weight) which deserialize is happy to
/// ignore.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Arm {
    pub runtime_id: Uuid,
    pub arm_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_overrides: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Experiment {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    pub status: ExperimentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_strategy: Option<String>,
    #[serde(default)]
    pub arms: Vec<Arm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_json: Option<ExperimentGoal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scoring_interval_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash_key_fields: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posterior_json: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights_json: Option<HashMap<String, i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halted_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExperimentListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    /// Runtime slug or group id.
    ///
    /// A `String` rather than a `Uuid`: the route resolves either form, and
    /// typing it as a uuid made the slug half of that contract unreachable.
    /// The wire name is `runtime`; `runtime_group_id` is accepted as a legacy
    /// alias, which is what this field used to send.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ExperimentStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

// ----- connectors (CP) -------------------------------------------------------

/// How a connector authenticates against its provider — mirrors the CP
/// `ConnectorAuthMode` enum.
///
/// The variant is `Static` for the on-the-wire `"static"` value because
/// `static` is a Rust keyword. The `Other` variant captures any mode the CP
/// adds after this SDK is compiled; the string is the raw on-the-wire value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectorAuthMode {
    /// `"static"` on the wire.
    Static,
    OauthStored,
    IdentityAssertion,
    FederatedExchange,
    PersonAuthorized,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl ConnectorAuthMode {
    /// On-the-wire string form.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Static => "static",
            Self::OauthStored => "oauth_stored",
            Self::IdentityAssertion => "identity_assertion",
            Self::FederatedExchange => "federated_exchange",
            Self::PersonAuthorized => "person_authorized",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for ConnectorAuthMode {
    fn from(s: &str) -> Self {
        match s {
            "static" => Self::Static,
            "oauth_stored" => Self::OauthStored,
            "identity_assertion" => Self::IdentityAssertion,
            "federated_exchange" => Self::FederatedExchange,
            "person_authorized" => Self::PersonAuthorized,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for ConnectorAuthMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ConnectorAuthMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Lifecycle status of a connector — mirrors the CP `ConnectorStatus` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectorStatus {
    Pending,
    Active,
    Error,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl ConnectorStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Error => "error",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for ConnectorStatus {
    fn from(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "active" => Self::Active,
            "error" => Self::Error,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for ConnectorStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ConnectorStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Lifecycle status of a connection — mirrors the CP `ConnectionStatus` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    PendingAuthorization,
    Active,
    RefreshFailed,
    Revoked,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl ConnectionStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::PendingAuthorization => "pending_authorization",
            Self::Active => "active",
            Self::RefreshFailed => "refresh_failed",
            Self::Revoked => "revoked",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for ConnectionStatus {
    fn from(s: &str) -> Self {
        match s {
            "pending_authorization" => Self::PendingAuthorization,
            "active" => Self::Active,
            "refresh_failed" => Self::RefreshFailed,
            "revoked" => Self::Revoked,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for ConnectionStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ConnectionStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Who a connection acts as — mirrors the CP `ConnectionSubjectType` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionSubjectType {
    App,
    User,
    Federated,
    Person,
    Workspace,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl ConnectionSubjectType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::App => "app",
            Self::User => "user",
            Self::Federated => "federated",
            Self::Person => "person",
            Self::Workspace => "workspace",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for ConnectionSubjectType {
    fn from(s: &str) -> Self {
        match s {
            "app" => Self::App,
            "user" => Self::User,
            "federated" => Self::Federated,
            "person" => Self::Person,
            "workspace" => Self::Workspace,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for ConnectionSubjectType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ConnectionSubjectType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Subjects currently accepted by registered connection creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionCreateSubjectType {
    App,
    User,
}

/// Subjects currently accepted by authorize and token-broker operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionBrokerSubjectType {
    App,
    User,
    Person,
}

/// A connector — the CP read model returned by every `/v1/connectors` route.
///
/// `client_secret` and `signing_secret` are **write-only**: accepted on create
/// and update, absent from every response — which is why this read model does
/// not have them. `requires_runtime` is server-derived and is the **only**
/// signal for whether [`ConnectorAuthorizeParams::runtime`] must be set;
/// never hardcode a provider list.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Connector {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    /// Stable per-org identifier; create is idempotent on it.
    pub slug: String,
    pub name: String,
    /// Provider slug, e.g. `"slack"`, `"gmail"`, `"stripe"`.
    pub provider: String,
    pub auth_mode: ConnectorAuthMode,
    /// Create-time fact (`development` / `staging` / `production`), not
    /// updatable.
    pub environment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_member_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub api_hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// `managed` / `byo` / `discovered`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person_server_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person_server_url: Option<String>,
    /// `human` / `judge_advises_human` / `judge_auto_within_envelope`.
    pub approval_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assertion_audience: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    pub status: ConnectorStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_member_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Server-derived: whether [`Connectors::authorize`] must name a runtime
    /// for this connector (chat providers need the agent that replies).
    ///
    /// [`Connectors::authorize`]: crate::resources::connectors::Connectors::authorize
    #[serde(default)]
    pub requires_runtime: bool,
}

/// A connection under a connector. Access/refresh tokens are **never**
/// serialized by the API, so they do not appear here.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Connection {
    pub id: Uuid,
    pub org_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub connector_id: Uuid,
    /// `None` = org-owned (app subject); for a Slack workspace install this
    /// points at the workspace customer member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
    /// The member who performed the grant, as distinct from `member_id`
    /// (whose credential this is). For `app` and `workspace` subjects those
    /// are never the same principal. `None` for grants made before the
    /// column existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_member_id: Option<Uuid>,
    /// Runtime group answering this connection's channels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    pub subject_type: ConnectionSubjectType,
    #[serde(default)]
    pub scopes_granted: Vec<String>,
    pub status: ConnectionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_expires_at: Option<String>,
}

/// Filters supported by `GET /v1/connectors`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectorListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// `POST /v1/connectors` body. `name`, `provider`, and `auth_mode` are
/// required; build with [`Self::new`] and struct-update syntax for the rest:
/// `ConnectorCreateParams { slug: Some("slack-support".into()), ..ConnectorCreateParams::new(...) }`.
///
/// `client_secret` and `signing_secret` are **write-only**: they are accepted
/// here (and on update) but never returned on any read. `issuer` drives
/// server-side OAuth discovery (endpoints resolved from `.well-known` when
/// omitted) and is not persisted.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectorCreateParams {
    pub name: String,
    /// Provider slug, e.g. `"slack"`, `"gmail"`, `"stripe"`.
    pub provider: String,
    pub auth_mode: ConnectorAuthMode,
    /// Derived from `name` server-side when omitted; create is idempotent on
    /// the resulting slug.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// Defaults to `"production"` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_hosts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Write-only; never present on a read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// Write-only; never present on a read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// OAuth discovery: when set and the endpoints are omitted, the server
    /// resolves them from `.well-known`. Not persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_server_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_server_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assertion_audience: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
}

impl ConnectorCreateParams {
    /// Create params with the three required fields set and every optional
    /// field unset. Combine with struct-update syntax.
    pub fn new(
        name: impl Into<String>,
        provider: impl Into<String>,
        auth_mode: ConnectorAuthMode,
    ) -> Self {
        Self {
            name: name.into(),
            provider: provider.into(),
            auth_mode,
            slug: None,
            environment: None,
            agent_member_id: None,
            authorization_endpoint: None,
            token_endpoint: None,
            scopes: None,
            api_hosts: None,
            client_id: None,
            client_secret: None,
            signing_secret: None,
            metadata: None,
            issuer: None,
            person_server_mode: None,
            person_server_url: None,
            approval_policy: None,
            application_id: None,
            assertion_audience: None,
            webhook_url: None,
        }
    }
}

/// `PATCH /v1/connectors/{id}` body — only these fields are mutable
/// (`environment`, `provider`, `auth_mode`, and `slug` are create-time facts).
/// Only provided fields change; omitting `client_secret` / `signing_secret`
/// means "unchanged", not "clear".
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectorUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_hosts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ConnectorStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    /// Write-only; omitted = unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// Write-only; omitted = unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_secret: Option<String>,
}

/// `POST /v1/connectors/{connector_id}/connections` body — registered mode,
/// where the caller supplies an already-minted provider token.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionCreateParams {
    pub access_token: String,
    /// Defaults to `App` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_type: Option<ConnectionCreateSubjectType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_granted: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_expires_at: Option<String>,
}

impl ConnectionCreateParams {
    /// A registered-mode body carrying only the token the route requires.
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            subject_type: None,
            scopes_granted: None,
            refresh_token: None,
            token_expires_at: None,
        }
    }
}

/// `POST /v1/oauth/connections/authorize` options (the `connector_id` itself
/// is the first argument to [`Connectors::authorize`]).
///
/// `runtime` is required (the server 422s without it) when the connector's
/// `requires_runtime` is true — it names the agent that replies on the
/// connected channel. `expires_in` bounds how long the minted URL stays valid
/// (60–86400 seconds, default 600); raise it when handing the URL to someone
/// else to open.
///
/// [`Connectors::authorize`]: crate::resources::connectors::Connectors::authorize
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectorAuthorizeParams {
    /// Runtime selector (slug or runtime group id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<StringOrUuid>,
    /// `App` (default) / `User` / `Person`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<ConnectionBrokerSubjectType>,
    /// Where the browser lands after consent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_url: Option<String>,
    /// Seconds the URL stays valid (60–86400, default 600).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u32>,
    /// The end customer this grant is being made for, asserted by the caller.
    /// Its `user_id` resolves a `customer` member recorded as the connection's
    /// `created_by_member_id`, so a partner can associate the connection with
    /// their own caller rather than the agent member that made the API call.
    /// `None` attributes the grant to the authenticated principal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<RunnerIdentity>,
}

/// Response of `POST /v1/oauth/connections/authorize` — a freshly minted
/// consent URL.
///
/// Each call writes a fresh **single-use** OAuth `state`: two calls give two
/// different URLs, and responses must never be cached — mint a new one per
/// hand-off. The `state` is deliberately not a response field and must never
/// be parsed out of the URL, logged, or surfaced.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectorAuthorization {
    pub authorize_url: String,
    /// Seconds the URL stays valid.
    pub expires_in: u64,
    pub expires_at: String,
}

/// Deterministic, non-PII envelope for a person-authorized action.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectionMissionConstraints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Opaque or hashed resource identifier; never raw PII.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_end: Option<String>,
    /// SHA-256 of the approved artifact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_binding: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectionTokenParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<ConnectionBrokerSubjectType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_permissions: Option<ConnectionMissionConstraints>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionToken {
    pub token: String,
    pub token_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionAuthorizationPending {
    pub status: String,
    pub mission_id: Uuid,
    pub approval_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ConnectionTokenResult {
    Token(ConnectionToken),
    AuthorizationPending(ConnectionAuthorizationPending),
}

// ----- runner ----------------------------------------------------------------

/// Identity captured at session creation. Drives experiment routing
/// (HRW / beta-sample) and rides on the access-token claims so DP can
/// stamp it onto `task.metadata.identity` + forward as
/// `TASK_USER_ID` / `TASK_ANONYMOUS_ID` / `TASK_CONVERSATION_ID`
/// sandbox env.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunnerIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// Tags to stamp on the `customer` member this identity mints, **if that
    /// member is new**. Access-bearing, and bounded on both sides: attenuated
    /// to the asserting agent member's own tags, and applied on create only —
    /// an existing member's tags are never changed here. Tags use the same
    /// opaque, exact-match validation as every other tag write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Optional segment.io-style observability payload attached to a
/// [`RunRequest`]. Used by CP for telemetry / experiment-report
/// slicing only — **routing never reads `caller`** (it walks
/// `identity.*` via `hash_key_fields` only). Mixing the two would be
/// a privacy + stability footgun.
///
/// Unknown fields ride along verbatim via [`Self::extra`].
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunCaller {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<RunCallerLibrary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<RunCallerPage>,
    /// Any additional fields the caller supplied (app / device / os /
    /// campaign / network / screen / timezone / traits / custom keys)
    /// pass through verbatim.
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunCallerLibrary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunCallerPage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referrer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `POST /v1/runtimes/{id}/run` and `/v1/experiments/{id}/run` body.
///
/// User-facing request type. CP infers everything else (runtime_id /
/// experiment_id from the URL; member_id / org_id / project_id from
/// the bearer key).
#[derive(Debug, Clone, Default, Serialize)]
pub struct RunRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<RunnerIdentity>,
    /// Optional segment.io-style observability payload — see
    /// [`RunCaller`]. Echoed on the response's `runtime_context.caller`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller: Option<RunCaller>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    /// Session lifetime override, max 24h. Default 1h on CP side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Resolved context attached to a [`RunnerSpec`] — the runtime / arm /
/// identity CP picked. Surfaced on `runner.context()` for telemetry.
#[derive(Debug, Clone, Deserialize)]
pub struct RunnerContext {
    pub runtime_id: Uuid,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default)]
    pub experiment_id: Option<Uuid>,
    pub recipe_id: Uuid,
    #[serde(default)]
    pub recipe_repository_id: Option<Uuid>,
    #[serde(default)]
    pub recipe_git_ref: Option<String>,
    #[serde(default)]
    pub recipe_git_commit_sha: Option<String>,
    #[serde(default)]
    pub arm_label: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub identity: RunnerIdentity,
    /// Echoed from the request body when supplied — see [`RunCaller`].
    #[serde(default)]
    pub caller: Option<RunCaller>,
}

/// DP deployment the runner should talk to. CP picks per project /
/// deployment and surfaces the resolved endpoint plus its slug + region
/// for telemetry / UX.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunnerDeployment {
    /// DP base URL (e.g. `https://api.gcp01.introspection.dev`).
    pub endpoint: String,
    /// Short deployment slug (e.g. `gcp01`).
    pub slug: String,
    /// Region the deployment lives in (e.g. `us-east-1`).
    pub region: String,
}

/// CP `/run` response — the customer-facing shape.
///
/// The fields it omits are server-internal and are never returned to
/// customer callers.
#[derive(Debug, Clone, Deserialize)]
pub struct RunnerSpec {
    pub session_id: String,
    /// DP deployment the runner should talk to.
    pub deployment: RunnerDeployment,
    /// RS256 session-locator JWT — the customer's only credential.
    /// SDK sends it as `Authorization: Bearer …`; the DP server looks
    /// up the session by JWT claims and reads the materialized access
    /// token from its Redis cache.
    pub session_token: String,
    /// Session lifetime (ISO-8601 string).
    pub expires_at: String,
    /// Resolved runtime context — runtime / arm / recipe / identity /
    /// caller that CP picked. Surfaced on `runner.context()` for
    /// telemetry + UX.
    pub runtime_context: RunnerContext,
}

// ----- telemetry: conversations / events / metrics (DP, runner-scoped) -------
//
// These are Data-Plane telemetry reads — they hang off the [`crate::Runner`]
// (DP bearer + `events:read`), never the CP-scoped top-level client. The
// stores are append-only (`otel_traces` → `/v1/conversations`, `otel_logs` →
// `/v1/events`); all aggregation goes through the bounded `POST /v1/metrics`
// contract. Records carry open telemetry attributes, so the typed structs keep
// a `#[serde(flatten)] extra` bag — unknown fields ride along verbatim rather
// than being dropped.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::api::error::{ApiResult, IntrospectionAPIError};

/// Sort direction for the telemetry list reads. Maps to the wire `direction`
/// query param; defaults to descending (newest-first) like the DP.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    /// The DP default — newest-first.
    #[default]
    Desc,
}

impl SortDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

impl From<&str> for SortDirection {
    fn from(s: &str) -> Self {
        match s {
            "asc" => Self::Asc,
            _ => Self::Desc,
        }
    }
}

impl Serialize for SortDirection {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SortDirection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Shared window / ordering / pagination inputs for the telemetry list reads.
///
/// Borrowed view over the ergonomic client params — applied onto the wire
/// query object by [`Window::apply`], which performs the client-side
/// validation (limit range, `lookback` vs `start`/`end` mutual exclusion) and
/// the ergonomic → wire mapping (`order`→`direction`, `start`→`start_date`,
/// `end`→`end_date`, `lookback`→computed `start_date`).
struct Window<'a> {
    limit: Option<u32>,
    next: Option<&'a str>,
    sort: Option<&'a str>,
    order: Option<SortDirection>,
    start: Option<&'a str>,
    end: Option<&'a str>,
    lookback: Option<&'a str>,
    include_total: Option<bool>,
}

impl Window<'_> {
    fn apply(&self, obj: &mut serde_json::Map<String, serde_json::Value>) -> ApiResult<()> {
        if let Some(limit) = self.limit {
            if !(1..=1000).contains(&limit) {
                return Err(IntrospectionAPIError::InvalidConfig(format!(
                    "limit must be between 1 and 1000 (got {limit})"
                )));
            }
            obj.insert("limit".to_string(), limit.into());
        }
        if let Some(next) = self.next {
            obj.insert("next".to_string(), next.into());
        }
        if let Some(sort) = self.sort {
            obj.insert("sort".to_string(), sort.into());
        }
        if let Some(order) = self.order {
            obj.insert("direction".to_string(), order.as_str().into());
        }
        apply_time_window(
            obj,
            self.start,
            self.end,
            self.lookback,
            "start_date",
            "end_date",
        )?;
        if let Some(include_total) = self.include_total {
            obj.insert("include_total".to_string(), include_total.into());
        }
        Ok(())
    }
}

/// Resolve the ergonomic `start` / `end` / `lookback` triple into the wire
/// window keys. `lookback` (relative, e.g. `"24h"`) is **mutually exclusive**
/// with `start`/`end` — the mismatch is rejected client-side *before* any
/// request is sent. When `lookback` is set the start key is computed as
/// `now - lookback`.
fn apply_time_window(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    start: Option<&str>,
    end: Option<&str>,
    lookback: Option<&str>,
    start_key: &str,
    end_key: &str,
) -> ApiResult<()> {
    if lookback.is_some() && (start.is_some() || end.is_some()) {
        return Err(IntrospectionAPIError::InvalidConfig(
            "`lookback` is mutually exclusive with `start`/`end`".to_string(),
        ));
    }
    if let Some(lookback) = lookback {
        let dur = parse_lookback(lookback)?;
        let start_at = SystemTime::now().checked_sub(dur).unwrap_or(UNIX_EPOCH);
        obj.insert(start_key.to_string(), rfc3339_utc(start_at).into());
    } else {
        if let Some(start) = start {
            obj.insert(start_key.to_string(), start.into());
        }
        if let Some(end) = end {
            obj.insert(end_key.to_string(), end.into());
        }
    }
    Ok(())
}

/// Parse a relative lookback like `"24h"`, `"30m"`, `"7d"`, or a compound
/// `"1h30m"` into a [`Duration`]. Units: `s`, `m`, `h`, `d`, `w`.
fn parse_lookback(s: &str) -> ApiResult<Duration> {
    let trimmed = s.trim();
    let invalid = || {
        IntrospectionAPIError::InvalidConfig(format!(
            "invalid lookback `{s}` (expected e.g. `24h`, `30m`, `7d`, `1h30m`)"
        ))
    };
    if trimmed.is_empty() {
        return Err(invalid());
    }
    let mut total: u64 = 0;
    let mut digits = String::new();
    let mut saw_unit = false;
    for c in trimmed.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        if digits.is_empty() {
            return Err(invalid());
        }
        let value: u64 = digits.parse().map_err(|_| invalid())?;
        let unit_secs = match c.to_ascii_lowercase() {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86_400,
            'w' => 604_800,
            _ => return Err(invalid()),
        };
        total = total
            .checked_add(value.checked_mul(unit_secs).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        digits.clear();
        saw_unit = true;
    }
    // A trailing number with no unit (`"24"`) or no units at all is invalid.
    if !digits.is_empty() || !saw_unit {
        return Err(invalid());
    }
    Ok(Duration::from_secs(total))
}

/// Format a [`SystemTime`] as an RFC 3339 / ISO-8601 UTC instant
/// (`YYYY-MM-DDThh:mm:ssZ`) with second precision. Dependency-free (the crate
/// does not pull `chrono`) via Howard Hinnant's civil-from-days algorithm.
fn rfc3339_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Convert a count of days since the Unix epoch to a `(year, month, day)`
/// civil date. Howard Hinnant's public-domain algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationAgent {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationCost {
    pub usd: f64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationMetrics {
    pub duration_ms: f64,
    pub trace_count: i64,
    pub span_count: i64,
    pub tool_use_count: i64,
    pub failed_tool_use_count: i64,
    pub has_errors: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Conversation {
    pub object: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<ConversationAgent>>,
    pub usage: ConversationUsage,
    pub cost: ConversationCost,
    pub metrics: ConversationMetrics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_git_commit_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

/// `resolution` filter on `GET /v1/conversations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationResolution {
    Resolved,
    Blocked,
    Unresolved,
    Pending,
}

impl ConversationResolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Blocked => "blocked",
            Self::Unresolved => "unresolved",
            Self::Pending => "pending",
        }
    }
}

/// `sentiment` filter on `GET /v1/conversations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationSentiment {
    Positive,
    Negative,
    Mixed,
    Neutral,
}

impl ConversationSentiment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
            Self::Mixed => "mixed",
            Self::Neutral => "neutral",
        }
    }
}

/// `status` filter on `GET /v1/conversations` — the span status a
/// conversation ended on. Distinct from [`crate::api::genai_span::SpanStatus`],
/// which is the status object carried *on* a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationStatus {
    Ok,
    Error,
    Unset,
}

impl ConversationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "Ok",
            Self::Error => "Error",
            Self::Unset => "Unset",
        }
    }
}

/// Ergonomic params for `GET /v1/conversations`. `order`/`start`/`end`/
/// `lookback` map to the wire `direction`/`start_date`/`end_date` window (see
/// a relative window); `filters` is a passthrough for resource filters that avoids
/// baking the open attribute vocabulary into the SDK.
#[derive(Debug, Clone, Default)]
pub struct ConversationListParams {
    pub limit: Option<u32>,
    pub next: Option<String>,
    pub sort: Option<String>,
    pub order: Option<SortDirection>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub lookback: Option<String>,
    /// Restrict to one conversation.
    pub conversation_id: Option<String>,
    /// Restrict to several conversations.
    pub conversation_ids: Option<Vec<String>>,
    /// Read through one or more share grants.
    pub share_id: Option<Vec<String>>,
    pub model: Option<String>,
    pub agent_name: Option<String>,
    pub status: Option<ConversationStatus>,
    pub service_name: Option<String>,
    pub service_names: Option<Vec<String>>,
    pub environment: Option<String>,
    pub runtime_id: Option<Uuid>,
    pub runtime_group_id: Option<Uuid>,
    pub experiment_id: Option<Uuid>,
    pub recipe_git_commit_sha: Option<String>,
    pub resolution: Option<ConversationResolution>,
    pub sentiment: Option<ConversationSentiment>,
    pub owner_key: Option<String>,
    /// Match conversations whose spans contain every metadata pair. Values may
    /// contain `:`; each pair is lowered to a repeated `metadata=key:value`
    /// query parameter, splitting on the first colon server-side.
    pub metadata: Option<HashMap<String, String>>,
    /// Escape hatch for a filter this SDK build predates: merged verbatim
    /// onto the query string, after the typed fields above.
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Optional expansions for conversation-item list/detail reads.
///
/// The message-family includes (`gen_ai.input.messages` and friends) are
/// deleted: the detail read returns the full history unconditionally, so there
/// is nothing left for them to gate. A parameter that is *always* required is
/// not a parameter, it is a trap — forgetting it silently forked a
/// conversation with one turn of context. The remaining values are genuine
/// optional encrypted or structural expansions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConversationItemInclude {
    #[serde(rename = "gen_ai.system_instructions")]
    GenAiSystemInstructions,
    #[serde(rename = "gen_ai.tool.definitions")]
    GenAiToolDefinitions,
    #[serde(rename = "events")]
    Events,
    #[serde(rename = "span_attributes")]
    SpanAttributes,
    #[serde(rename = "resource_attributes")]
    ResourceAttributes,
}

impl ConversationItemInclude {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GenAiSystemInstructions => "gen_ai.system_instructions",
            Self::GenAiToolDefinitions => "gen_ai.tool.definitions",
            Self::Events => "events",
            Self::SpanAttributes => "span_attributes",
            Self::ResourceAttributes => "resource_attributes",
        }
    }
}

/// Parameters for `GET /v1/conversations/{conversation_id}/items`.
///
/// `next` is the opaque token returned by
/// [`GenAiSpanList::next`](crate::api::genai_span::GenAiSpanList::next).
/// `first_id` and `last_id` on that envelope are informational span IDs and
/// are not valid pagination inputs.
#[derive(Debug, Clone, Default)]
pub struct ConversationItemListParams {
    pub limit: Option<u32>,
    pub next: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub include: Vec<ConversationItemInclude>,
    /// `root`, an exact invocation id, or omitted for the complete conversation.
    pub agent: Option<String>,
    pub service_name: Option<String>,
    pub operation_name: Option<String>,
    pub lookback_days: Option<u32>,
    pub share_id: Option<Uuid>,
}

/// Parameters for `GET /v1/conversations/{conversation_id}/items/{item_id}`.
#[derive(Debug, Clone, Default)]
pub struct ConversationItemGetParams {
    pub include: Vec<ConversationItemInclude>,
    pub share_id: Option<Uuid>,
}

// The conversation item and its page envelope are the GenAI span types in
// [`crate::api::genai_span`] — `GenAiSpan` and `GenAiSpanList`. The flat
// ~40-column item that used to live here is gone, not deprecated: it renamed a
// standard vocabulary, dropped every attribute nobody had added a column for,
// and rendered the absence of a value as the presence of one.

impl ConversationListParams {
    /// Validate and lower to the wire query object. Returns
    /// [`IntrospectionAPIError::InvalidConfig`] for an out-of-range `limit` or
    /// a `lookback`/`start`/`end` conflict — *before* any request is issued.
    pub fn to_wire(&self) -> ApiResult<serde_json::Value> {
        let mut obj = serde_json::Map::new();
        Window {
            limit: self.limit,
            next: self.next.as_deref(),
            sort: self.sort.as_deref(),
            order: self.order,
            start: self.start.as_deref(),
            end: self.end.as_deref(),
            lookback: self.lookback.as_deref(),
            // `/v1/conversations` declares no `include_total`; the count is
            // not available on this read, so there is nothing to ask for.
            include_total: None,
        }
        .apply(&mut obj)?;
        put_str(&mut obj, "conversation_id", self.conversation_id.as_ref());
        put_list(&mut obj, "conversation_ids", self.conversation_ids.as_ref());
        put_list(&mut obj, "share_id", self.share_id.as_ref());
        put_str(&mut obj, "model", self.model.as_ref());
        put_str(&mut obj, "agent_name", self.agent_name.as_ref());
        if let Some(status) = self.status {
            obj.insert("status".to_string(), status.as_str().into());
        }
        put_str(&mut obj, "service_name", self.service_name.as_ref());
        put_list(&mut obj, "service_names", self.service_names.as_ref());
        put_str(&mut obj, "environment", self.environment.as_ref());
        put_uuid(&mut obj, "runtime_id", self.runtime_id);
        put_uuid(&mut obj, "runtime_group_id", self.runtime_group_id);
        put_uuid(&mut obj, "experiment_id", self.experiment_id);
        put_str(
            &mut obj,
            "recipe_git_commit_sha",
            self.recipe_git_commit_sha.as_ref(),
        );
        if let Some(resolution) = self.resolution {
            obj.insert("resolution".to_string(), resolution.as_str().into());
        }
        if let Some(sentiment) = self.sentiment {
            obj.insert("sentiment".to_string(), sentiment.as_str().into());
        }
        put_str(&mut obj, "owner_key", self.owner_key.as_ref());
        put_metadata(&mut obj, self.metadata.as_ref());
        merge_filters(&mut obj, self.filters.as_ref());
        Ok(serde_json::Value::Object(obj))
    }
}

// ----- events: typed six-family read (`GET /v1/events`) ----------------------

/// The six canonical platform event families served by `GET /v1/events`.
///
/// The events read is a **closed, typed set**: `event_name` is required on
/// every list read — exactly one family per request — so a response page is
/// always homogeneous and fully typeable (JSON discriminated member; Arrow
/// typed payload struct column). Legacy verb-suffixed names on historical
/// rows are normalized to these canonical names server-side; anything outside
/// the set (`gen_ai.*`, customer / `track()` events) is not returned and
/// remains aggregable via `POST /v1/metrics`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntrospectionEventName {
    Feedback,
    Observation,
    ObservationClusteringRun,
    Judgement,
    Pattern,
    PatternAssignment,
    /// A family added after this SDK was released. Keeping the wire value
    /// makes the [`Event::Unknown`] response fallback reachable in practice.
    Unknown(String),
}

impl IntrospectionEventName {
    /// On-the-wire dotted family name.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Feedback => "introspection.feedback",
            Self::Observation => "introspection.observation",
            Self::ObservationClusteringRun => "introspection.observation_clustering.run",
            Self::Judgement => "introspection.judgement",
            Self::Pattern => "introspection.pattern",
            Self::PatternAssignment => "introspection.pattern.assignment",
            Self::Unknown(value) => value,
        }
    }
}

impl Serialize for IntrospectionEventName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for IntrospectionEventName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "introspection.feedback" => Self::Feedback,
            "introspection.observation" => Self::Observation,
            "introspection.observation_clustering.run" => Self::ObservationClusteringRun,
            "introspection.judgement" => Self::Judgement,
            "introspection.pattern" => Self::Pattern,
            "introspection.pattern.assignment" => Self::PatternAssignment,
            _ => Self::Unknown(value),
        })
    }
}

impl fmt::Display for IntrospectionEventName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Common envelope shared by every typed event family — the queryable
/// surface. `org`/`project` never appear on the wire (tenant scope is implied
/// by auth). The `event_name` discriminator lives on the [`Event`] enum tag,
/// not duplicated here.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TypedEvent<P> {
    pub id: String,
    /// Per-family semantics: observation → `observed_at`, pattern →
    /// `updated_at` (catalog cursor), stream families → emit/observed time.
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_git_commit_sha: Option<String>,
    /// Family detail — one of the six `*Payload` types, fixed by the
    /// [`Event`] variant.
    pub payload: P,
}

/// `introspection.observation` payload — one **resolved** observation (the
/// server-side fold: supersession applied, current pattern assignment
/// joined). All fields optional except the `observation_id` identity.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservationPayload {
    pub observation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_refs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces_observation_id: Option<Uuid>,
    /// CURRENT pattern assignment (fold), not the assignment history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// `introspection.pattern` payload — one **folded** catalog row (current
/// state: latest lifecycle action, status, fold timestamps).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatternPayload {
    pub pattern_id: String,
    /// Latest lifecycle action (`created` / `updated` / `retired`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    /// `active` | `retired` (fold).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_detected_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_pattern_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from_pattern_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

/// `introspection.pattern.assignment` payload — one observation→pattern
/// assignment event (stream family). `observation_id` is the sole identity
/// field; `pattern_id: None` means the observation was explicitly unassigned.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatternAssignmentPayload {
    pub observation_id: Uuid,
    /// Target pattern; `None` = explicitly unassigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// `introspection.observation_clustering.run` payload — one clustering run
/// (stream family).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusteringRunPayload {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `introspection.feedback` payload — mirrors what the SDK `feedback()`
/// surfaces actually emit (`properties.*` / `identity.*` attributes).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeedbackPayload {
    /// The feedback label (`"thumbs_up"`, …) — `properties.name`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments: Option<String>,
    /// Numeric axis, when present — `properties.value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous_id: Option<String>,
    /// Optional **emitted** field (positive/negative/neutral) — never derived
    /// server-side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<String>,
    /// Response the feedback anchors to —
    /// `gen_ai.request.previous_response_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// `gen_ai.agent.name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    /// `gen_ai.agent.id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Remaining `properties.*` extras.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
}

/// `introspection.judgement` payload — mirrors the runtime-agent judges
/// emitter (`introspection.judgement.*` / `introspection.judge.*` attributes).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JudgementPayload {
    pub judgement_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_arm_id: Option<Uuid>,
}

/// Whole-event envelope + typed payload per family.
pub type ObservationEvent = TypedEvent<ObservationPayload>;
pub type PatternEvent = TypedEvent<PatternPayload>;
pub type PatternAssignmentEvent = TypedEvent<PatternAssignmentPayload>;
pub type ClusteringRunEvent = TypedEvent<ClusteringRunPayload>;
pub type FeedbackEvent = TypedEvent<FeedbackPayload>;
pub type JudgementEvent = TypedEvent<JudgementPayload>;

/// One event from `GET /v1/events` — a discriminated union of the six
/// canonical platform families, tagged by the top-level `event_name`.
///
/// Because `event_name` is required on the list read, a page is always
/// homogeneous — every record matches the requested family. The hidden
/// [`Event::Unknown`] fallback tolerates a family this SDK build doesn't know
/// yet (a seventh family added server-side must not fail the whole page);
/// match on it to skip or hand-parse such rows.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "event_name")]
pub enum Event {
    #[serde(rename = "introspection.feedback")]
    Feedback(FeedbackEvent),
    #[serde(rename = "introspection.observation")]
    Observation(ObservationEvent),
    #[serde(rename = "introspection.observation_clustering.run")]
    ObservationClusteringRun(ClusteringRunEvent),
    #[serde(rename = "introspection.judgement")]
    Judgement(JudgementEvent),
    #[serde(rename = "introspection.pattern")]
    Pattern(PatternEvent),
    #[serde(rename = "introspection.pattern.assignment")]
    PatternAssignment(PatternAssignmentEvent),
    /// Forward-compatible escape hatch: a row whose `event_name` this SDK
    /// build doesn't recognise. Carries the raw record verbatim.
    #[serde(untagged)]
    Unknown(serde_json::Value),
}

impl Event {
    /// The canonical family, or `None` for [`Event::Unknown`] rows.
    pub fn event_name(&self) -> Option<IntrospectionEventName> {
        match self {
            Self::Feedback(_) => Some(IntrospectionEventName::Feedback),
            Self::Observation(_) => Some(IntrospectionEventName::Observation),
            Self::ObservationClusteringRun(_) => {
                Some(IntrospectionEventName::ObservationClusteringRun)
            }
            Self::Judgement(_) => Some(IntrospectionEventName::Judgement),
            Self::Pattern(_) => Some(IntrospectionEventName::Pattern),
            Self::PatternAssignment(_) => Some(IntrospectionEventName::PatternAssignment),
            Self::Unknown(_) => None,
        }
    }
}

/// Ergonomic params for `GET /v1/events`. [`Self::event_name`] is
/// **required** (compile-enforced) — exactly one family per request, so the
/// response is always homogeneous. Per-family filters (§4.3 of the telemetry
/// read design — e.g. observation `pattern_id` / `lens` /
/// `include_superseded`, pattern `lens` / `status`) pass through
/// [`Self::filters`] verbatim.
#[derive(Debug, Clone)]
pub struct EventListParams {
    /// The one family to list — required; there is no unfiltered read.
    pub event_name: IntrospectionEventName,
    pub limit: Option<u32>,
    pub next: Option<String>,
    pub sort: Option<String>,
    pub order: Option<SortDirection>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub lookback: Option<String>,
    pub conversation_id: Option<String>,
    pub conversation_ids: Option<Vec<String>>,
    pub event_id: Option<Vec<String>>,
    pub service_name: Option<String>,
    pub environment: Option<String>,
    pub runtime_group_id: Option<Uuid>,
    pub runtime_group_unattributed: Option<bool>,
    pub lens: Option<String>,
    pub pattern_id: Option<Uuid>,
    pub status: Option<String>,
    pub include_superseded: Option<bool>,
    pub severities: Option<Vec<String>>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub owner_key: Option<String>,
    /// Escape hatch for a filter this SDK build predates: merged verbatim
    /// onto the query
    /// string. A filter outside the requested family's allow-map is a 422.
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

impl EventListParams {
    /// Params for one family with every optional field unset. Combine with
    /// struct-update syntax:
    /// `EventListParams { limit: Some(10), ..EventListParams::new(family) }`.
    pub fn new(event_name: IntrospectionEventName) -> Self {
        Self {
            event_name,
            limit: None,
            next: None,
            sort: None,
            order: None,
            start: None,
            end: None,
            lookback: None,
            conversation_id: None,
            conversation_ids: None,
            event_id: None,
            service_name: None,
            environment: None,
            runtime_group_id: None,
            runtime_group_unattributed: None,
            lens: None,
            pattern_id: None,
            status: None,
            include_superseded: None,
            severities: None,
            trace_id: None,
            span_id: None,
            owner_key: None,
            filters: None,
        }
    }

    /// Validate and lower to the wire query object (see
    /// [`ConversationListParams::to_wire`]).
    pub fn to_wire(&self) -> ApiResult<serde_json::Value> {
        let mut obj = serde_json::Map::new();
        obj.insert("event_name".to_string(), self.event_name.as_str().into());
        Window {
            limit: self.limit,
            next: self.next.as_deref(),
            sort: self.sort.as_deref(),
            order: self.order,
            start: self.start.as_deref(),
            end: self.end.as_deref(),
            lookback: self.lookback.as_deref(),
            // `/v1/events` declares no `include_total`; the count is not
            // available on this read, so there is nothing to ask for.
            include_total: None,
        }
        .apply(&mut obj)?;
        put_str(&mut obj, "conversation_id", self.conversation_id.as_ref());
        put_list(&mut obj, "conversation_ids", self.conversation_ids.as_ref());
        put_list(&mut obj, "event_id", self.event_id.as_ref());
        put_str(&mut obj, "service_name", self.service_name.as_ref());
        put_str(&mut obj, "environment", self.environment.as_ref());
        put_uuid(&mut obj, "runtime_group_id", self.runtime_group_id);
        put_bool(
            &mut obj,
            "runtime_group_unattributed",
            self.runtime_group_unattributed,
        );
        put_str(&mut obj, "lens", self.lens.as_ref());
        put_uuid(&mut obj, "pattern_id", self.pattern_id);
        put_str(&mut obj, "status", self.status.as_ref());
        put_bool(&mut obj, "include_superseded", self.include_superseded);
        put_list(&mut obj, "severities", self.severities.as_ref());
        put_str(&mut obj, "trace_id", self.trace_id.as_ref());
        put_str(&mut obj, "span_id", self.span_id.as_ref());
        put_str(&mut obj, "owner_key", self.owner_key.as_ref());
        if self
            .filters
            .as_ref()
            .is_some_and(|filters| filters.contains_key("event_name"))
        {
            return Err(IntrospectionAPIError::InvalidConfig(
                "event_name is reserved; select the family with EventListParams::new".to_string(),
            ));
        }
        merge_filters(&mut obj, self.filters.as_ref());
        Ok(serde_json::Value::Object(obj))
    }
}

/// Insert an optional scalar filter under `key` when it is set.
fn put_str(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&String>,
) {
    if let Some(v) = value {
        obj.insert(key.to_string(), v.as_str().into());
    }
}

/// Insert an optional `Uuid` filter under `key` when it is set.
fn put_uuid(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str, value: Option<Uuid>) {
    if let Some(v) = value {
        obj.insert(key.to_string(), v.to_string().into());
    }
}

/// Insert an optional `bool` filter under `key` when it is set.
fn put_bool(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str, value: Option<bool>) {
    if let Some(v) = value {
        obj.insert(key.to_string(), v.into());
    }
}

/// Insert a repeated filter under `key` when it is set and non-empty. An
/// empty vec is "no filter", not "match nothing": sending `?key=` would be a
/// filter on the empty string.
fn put_list(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&Vec<String>>,
) {
    if let Some(values) = value {
        if !values.is_empty() {
            obj.insert(
                key.to_string(),
                serde_json::Value::Array(values.iter().map(|v| v.as_str().into()).collect()),
            );
        }
    }
}

/// Insert conversation metadata as repeated `metadata=key:value` filters.
/// Sorting makes request serialization deterministic despite `HashMap`
/// iteration order. An empty map means no filter and is omitted.
fn put_metadata(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    metadata: Option<&HashMap<String, String>>,
) {
    let Some(metadata) = metadata.filter(|metadata| !metadata.is_empty()) else {
        return;
    };
    let mut pairs: Vec<_> = metadata
        .iter()
        .map(|(key, value)| format!("{key}:{value}"))
        .collect();
    pairs.sort();
    obj.insert(
        "metadata".to_string(),
        serde_json::Value::Array(pairs.into_iter().map(Into::into).collect()),
    );
}

fn merge_filters(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    filters: Option<&HashMap<String, serde_json::Value>>,
) {
    if let Some(filters) = filters {
        for (k, v) in filters {
            obj.insert(k.clone(), v.clone());
        }
    }
}

// ----- metrics (POST /v1/metrics) --------------------------------------------

/// One `{measure, aggregation}` metric term in a [`MetricsQuery`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSpec {
    pub measure: String,
    pub aggregation: String,
}

/// One grouping dimension `{field}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    pub field: String,
}

/// One `{field, operator, value}` filter term.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricFilter {
    pub field: String,
    pub operator: String,
    pub value: serde_json::Value,
}

/// Time-bucketing dimension — `bins` (count) or `granularity`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimeDimension {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bins: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<String>,
}

/// One typed ordering term: metric-index, dimension-field, or time bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderTerm {
    #[serde(rename = "type")]
    pub term_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub direction: SortDirection,
}

/// One post-grouping `having` term over an aggregated metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HavingTerm {
    pub metric_index: u32,
    pub operator: String,
    pub value: serde_json::Value,
}

/// Bounded execution config — `row_limit` (default 100, max 10 000) and the
/// grouped-time-series `series_limit`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_limit: Option<u32>,
}

/// Request body for the bounded `POST /v1/metrics` analytics endpoint.
///
/// Ergonomic `start` / `end` / `lookback` map to the wire
/// `from_timestamp` / `to_timestamp` window (same mutual-exclusion validation
/// as the list reads). This is not a general query endpoint — the DP enforces
/// the allow-listed views / measures / dimensions and hard limits.
#[derive(Debug, Clone, Default)]
pub struct MetricsQuery {
    pub view: String,
    pub metrics: Vec<MetricSpec>,
    pub dimensions: Option<Vec<Dimension>>,
    pub filters: Option<Vec<MetricFilter>>,
    pub time_dimension: Option<TimeDimension>,
    pub order_by: Option<Vec<OrderTerm>>,
    pub having: Option<Vec<HavingTerm>>,
    pub config: Option<MetricsConfig>,
    /// Window start (→ `from_timestamp`). Mutually exclusive with `lookback`.
    pub start: Option<String>,
    /// Window end (→ `to_timestamp`). Mutually exclusive with `lookback`.
    pub end: Option<String>,
    /// Relative window (e.g. `"24h"`) → computed `from_timestamp`.
    pub lookback: Option<String>,
}

impl MetricsQuery {
    /// Validate and lower to the wire request body. Rejects a
    /// `lookback`/`start`/`end` conflict client-side before sending.
    pub fn to_wire(&self) -> ApiResult<serde_json::Value> {
        let mut obj = serde_json::Map::new();
        obj.insert("view".to_string(), self.view.clone().into());
        obj.insert(
            "metrics".to_string(),
            serde_json::to_value(&self.metrics).map_err(encode_err)?,
        );
        if let Some(dimensions) = &self.dimensions {
            obj.insert(
                "dimensions".to_string(),
                serde_json::to_value(dimensions).map_err(encode_err)?,
            );
        }
        if let Some(filters) = &self.filters {
            obj.insert(
                "filters".to_string(),
                serde_json::to_value(filters).map_err(encode_err)?,
            );
        }
        if let Some(time_dimension) = &self.time_dimension {
            obj.insert(
                "time_dimension".to_string(),
                serde_json::to_value(time_dimension).map_err(encode_err)?,
            );
        }
        if let Some(order_by) = &self.order_by {
            obj.insert(
                "order_by".to_string(),
                serde_json::to_value(order_by).map_err(encode_err)?,
            );
        }
        if let Some(having) = &self.having {
            obj.insert(
                "having".to_string(),
                serde_json::to_value(having).map_err(encode_err)?,
            );
        }
        if let Some(config) = &self.config {
            obj.insert(
                "config".to_string(),
                serde_json::to_value(config).map_err(encode_err)?,
            );
        }
        apply_time_window(
            &mut obj,
            self.start.as_deref(),
            self.end.as_deref(),
            self.lookback.as_deref(),
            "from_timestamp",
            "to_timestamp",
        )?;
        // The list reads let the server default an open-ended window, so
        // `apply_time_window` sets only the start for a `lookback`. Metrics
        // requires both ends, so a `lookback` on its own would be a 422 —
        // close the window at now.
        if self.lookback.is_some() {
            obj.insert(
                "to_timestamp".to_string(),
                rfc3339_utc(SystemTime::now()).into(),
            );
        }
        Ok(serde_json::Value::Object(obj))
    }
}

fn encode_err(e: serde_json::Error) -> IntrospectionAPIError {
    IntrospectionAPIError::Decode(format!("failed to encode metrics query: {e}"))
}

/// Response from `POST /v1/metrics`. The row shape depends on the requested
/// view / metrics / dimensions, so rows stay as `serde_json::Value` and any
/// envelope fields other than `data`/`meta` ride along in [`Self::extra`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetricsResponse {
    #[serde(default)]
    pub data: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_kind_round_trips_known_variants() {
        for (wire, kind) in [("agent", TaskKind::Agent), ("process", TaskKind::Process)] {
            let parsed: TaskKind = serde_json::from_str(&format!("\"{wire}\"")).unwrap();
            assert_eq!(parsed, kind);
            assert_eq!(
                serde_json::to_string(&parsed).unwrap(),
                format!("\"{wire}\"")
            );
        }
    }

    #[test]
    fn task_kind_tolerates_unknown_values() {
        let kind: TaskKind = serde_json::from_str("\"brand_new_kind\"").unwrap();
        assert_eq!(kind, TaskKind::Other("brand_new_kind".to_string()));
    }

    #[test]
    fn metrics_lookback_closes_the_window_at_now() {
        // Metrics requires both ends of the window. The list reads let the
        // server default an open end, so the shared window helper sets only
        // the start — which made a `lookback` on its own a guaranteed 422.
        let body = MetricsQuery {
            view: "observations".into(),
            metrics: vec![MetricSpec {
                measure: "conversation_id".into(),
                aggregation: "count_distinct".into(),
            }],
            lookback: Some("24h".into()),
            dimensions: None,
            filters: None,
            time_dimension: None,
            order_by: None,
            having: None,
            config: None,
            start: None,
            end: None,
        }
        .to_wire()
        .unwrap();

        assert!(body.get("from_timestamp").is_some());
        assert!(body.get("to_timestamp").is_some());
    }

    #[test]
    fn task_create_omits_the_fields_the_caller_left_unset() {
        // `TaskCreate` is `extra="forbid"` server-side, so a field the caller
        // did not ask for must not appear at all — a null is a 422, and `ref` /
        // `depth` defaults belong to the platform, not to this struct.
        let body = serde_json::to_value(TaskCreate {
            prompt: Some("hello".into()),
            repositories: Some(vec![TaskRepoRequest {
                repo: "acme/api-service".into(),
                ..Default::default()
            }]),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(
            body,
            json!({"prompt": "hello", "repositories": [{"repo": "acme/api-service"}]})
        );
    }

    #[test]
    fn task_create_serialises_the_repository_ref_under_its_wire_name() {
        // The field is `ref` on the wire and `git_ref` in Rust, because `ref`
        // is a keyword. A rename that silently stopped applying would send an
        // entry the server clones at the default branch instead.
        let body = serde_json::to_value(TaskCreate {
            repositories: Some(vec![TaskRepoRequest {
                repo: "acme/api-service".into(),
                git_ref: Some("main".into()),
                depth: Some(0),
            }]),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(
            body["repositories"][0],
            json!({"repo": "acme/api-service", "ref": "main", "depth": 0})
        );
    }

    #[test]
    fn task_create_serialises_conversation_metadata_at_the_top_level() {
        let body = serde_json::to_value(TaskCreate {
            prompt: Some("hello".into()),
            conversation_metadata: Some(HashMap::from([
                ("flow".into(), "checkout".into()),
                ("tenant".into(), "acme".into()),
            ])),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(body["conversation_metadata"]["flow"], "checkout");
        assert_eq!(body["conversation_metadata"]["tenant"], "acme");
        assert!(body.get("custom").is_none());
    }

    #[test]
    fn task_status_round_trips() {
        let s: TaskStatus = serde_json::from_str("\"running\"").unwrap();
        assert_eq!(s, TaskStatus::Running);
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"running\"");

        let awaiting: TaskStatus = serde_json::from_str("\"awaiting_user\"").unwrap();
        assert_eq!(awaiting, TaskStatus::AwaitingUser);
    }

    #[test]
    fn current_runner_request_and_context_fields_use_wire_names() {
        let request = serde_json::to_value(RunRequest {
            agent_name: Some("support-agent".into()),
            scope: Some("tasks:read shares:read".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(request["agent_name"], "support-agent");
        assert_eq!(request["scope"], "tasks:read shares:read");

        let context: RunnerContext = serde_json::from_value(json!({
            "runtime_id": "00000000-0000-0000-0000-000000000041",
            "runtime_group_id": "00000000-0000-0000-0000-000000000042",
            "experiment_id": null,
            "recipe_id": "00000000-0000-0000-0000-000000000043",
            "agent_name": "support-agent",
            "identity": {}
        }))
        .unwrap();
        assert_eq!(
            context.runtime_id.to_string(),
            "00000000-0000-0000-0000-000000000041"
        );
        assert_eq!(
            context.runtime_group_id.unwrap().to_string(),
            "00000000-0000-0000-0000-000000000042"
        );
        assert_eq!(context.agent_name.as_deref(), Some("support-agent"));
    }

    #[test]
    fn experiment_status_round_trips_ended() {
        let s: ExperimentStatus = serde_json::from_str("\"ended\"").unwrap();
        assert_eq!(s, ExperimentStatus::Ended);
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"ended\"");

        let unknown: ExperimentStatus = serde_json::from_str("\"paused\"").unwrap();
        assert_eq!(unknown, ExperimentStatus::Other("paused".to_string()));
    }

    #[test]
    fn experiment_read_parses_typed_goal_and_group() {
        let exp: Experiment = serde_json::from_value(json!({
            "id": "0195c0de-0000-7000-8000-000000000001",
            "org_id": "0195c0de-0000-7000-8000-000000000002",
            "project_id": "0195c0de-0000-7000-8000-000000000003",
            "name": "prompt-bake-off",
            "runtime_group_id": "0195c0de-0000-7000-8000-00000000000a",
            "status": "running",
            "arms": [
                {"id": "0195c0de-0000-7000-8000-0000000000f1", "runtime_id": "0195c0de-0000-7000-8000-00000000000b", "arm_label": "control", "initial_weight": 50}
            ],
            "goal_json": {
                "kind": "composite",
                "direction": "maximize",
                "components": [
                    {"source": "judge", "judge_id": "0195c0de-0000-7000-8000-00000000000d", "weight": 1.0}
                ]
            },
            "created_at": "2025-01-01T00:00:00Z",
            "updated_at": "2025-01-01T00:00:00Z"
        }))
        .unwrap();

        assert_eq!(exp.status, ExperimentStatus::Running);
        assert_eq!(
            exp.runtime_group_id.unwrap().to_string(),
            "0195c0de-0000-7000-8000-00000000000a"
        );
        assert_eq!(exp.arms[0].arm_label, "control");
        let goal = exp.goal_json.expect("goal parsed");
        match &goal.components[0] {
            ExperimentGoalComponent::Judge(j) => assert_eq!(j.weight, 1.0),
            ExperimentGoalComponent::Telemetry(_) => panic!("expected judge component"),
        }
    }

    #[test]
    fn cancel_options_default_to_abort() {
        assert_eq!(
            serde_json::to_value(TaskCancelOptions::default()).unwrap(),
            serde_json::json!({"mode": "abort"})
        );
    }

    #[test]
    fn file_type_round_trips() {
        let ft: FileType = serde_json::from_str("\"upload\"").unwrap();
        assert_eq!(ft, FileType::Upload);
        assert_eq!(serde_json::to_string(&ft).unwrap(), "\"upload\"");
    }

    #[test]
    fn paginated_envelope_parses() {
        let payload = r#"{"records":[],"count":0,"total_count":null,"next":null}"#;
        let page: Paginated<serde_json::Value> = serde_json::from_str(payload).unwrap();
        assert_eq!(page.count, 0);
        assert!(page.next.is_none());
    }

    #[test]
    fn runtime_llm_mode_round_trips_known_variants() {
        let m: RuntimeLlmMode = serde_json::from_str("\"managed\"").unwrap();
        assert_eq!(m, RuntimeLlmMode::Managed);
        assert_eq!(serde_json::to_string(&m).unwrap(), "\"managed\"");

        let m: RuntimeLlmMode = serde_json::from_str("\"byok\"").unwrap();
        assert_eq!(m, RuntimeLlmMode::Byok);
        assert_eq!(serde_json::to_string(&m).unwrap(), "\"byok\"");
    }

    #[test]
    fn runtime_llm_mode_tolerates_unknown_values() {
        let m: RuntimeLlmMode = serde_json::from_str("\"brand_new_mode\"").unwrap();
        assert_eq!(m, RuntimeLlmMode::Other("brand_new_mode".to_string()));
    }

    #[test]
    fn runtime_llm_mode_default_is_managed() {
        assert_eq!(RuntimeLlmMode::default(), RuntimeLlmMode::Managed);
    }

    #[test]
    fn runtime_list_params_serialize_runtime_not_name_or_slug() {
        let value = serde_json::to_value(RuntimeListParams {
            runtime: Some("customer-agent".into()),
            ..Default::default()
        })
        .expect("runtime list params serialize");

        assert_eq!(value["runtime"], "customer-agent");
        assert!(value.get("name").is_none());
        assert!(value.get("slug").is_none());
    }

    #[test]
    fn sort_direction_defaults_desc_and_round_trips() {
        assert_eq!(SortDirection::default(), SortDirection::Desc);
        assert_eq!(SortDirection::Asc.as_str(), "asc");
        let d: SortDirection = serde_json::from_str("\"asc\"").unwrap();
        assert_eq!(d, SortDirection::Asc);
        assert_eq!(
            serde_json::to_string(&SortDirection::Desc).unwrap(),
            "\"desc\""
        );
    }

    #[test]
    fn conversation_params_map_ergonomic_names_to_wire() {
        let wire = ConversationListParams {
            limit: Some(50),
            order: Some(SortDirection::Asc),
            start: Some("2026-01-01T00:00:00Z".into()),
            end: Some("2026-02-01T00:00:00Z".into()),
            ..Default::default()
        }
        .to_wire()
        .unwrap();
        assert_eq!(wire["limit"], 50);
        assert_eq!(wire["direction"], "asc");
        assert_eq!(wire["start_date"], "2026-01-01T00:00:00Z");
        assert_eq!(wire["end_date"], "2026-02-01T00:00:00Z");
        // Ergonomic aliases never leak onto the wire.
        assert!(wire.get("order").is_none());
        assert!(wire.get("start").is_none());
        assert!(wire.get("end").is_none());
    }

    #[test]
    fn typed_conversation_filters_reach_the_wire() {
        // These were a `HashMap<String, Value>` the caller had to spell by
        // hand: a typo produced no compile error and, per the DP's own
        // warning, an unrecognised filter can come back as an *unfiltered*
        // list rather than a 422. Typed fields, lowered here.
        let wire = ConversationListParams {
            conversation_id: Some("conv".into()),
            conversation_ids: Some(vec!["a".into(), "b".into()]),
            share_id: Some(vec!["s".into()]),
            model: Some("claude-opus-5".into()),
            agent_name: Some("agent".into()),
            status: Some(ConversationStatus::Error),
            service_name: Some("svc".into()),
            service_names: Some(vec!["svc".into()]),
            environment: Some("prod".into()),
            runtime_id: Some(Uuid::nil()),
            runtime_group_id: Some(Uuid::nil()),
            experiment_id: Some(Uuid::nil()),
            recipe_git_commit_sha: Some("deadbeef".into()),
            resolution: Some(ConversationResolution::Blocked),
            sentiment: Some(ConversationSentiment::Negative),
            owner_key: Some("owner".into()),
            metadata: Some(HashMap::from([
                ("flow".into(), "checkout".into()),
                ("route".into(), "checkout:retry".into()),
            ])),
            ..Default::default()
        }
        .to_wire()
        .unwrap();

        assert_eq!(wire["conversation_id"], "conv");
        assert_eq!(wire["conversation_ids"], serde_json::json!(["a", "b"]));
        assert_eq!(wire["share_id"], serde_json::json!(["s"]));
        assert_eq!(wire["model"], "claude-opus-5");
        assert_eq!(wire["agent_name"], "agent");
        assert_eq!(wire["status"], "Error");
        assert_eq!(wire["service_name"], "svc");
        assert_eq!(wire["service_names"], serde_json::json!(["svc"]));
        assert_eq!(wire["environment"], "prod");
        assert_eq!(wire["runtime_id"], Uuid::nil().to_string());
        assert_eq!(wire["runtime_group_id"], Uuid::nil().to_string());
        assert_eq!(wire["experiment_id"], Uuid::nil().to_string());
        assert_eq!(wire["recipe_git_commit_sha"], "deadbeef");
        assert_eq!(wire["resolution"], "blocked");
        assert_eq!(wire["sentiment"], "negative");
        assert_eq!(wire["owner_key"], "owner");
        assert_eq!(
            wire["metadata"],
            serde_json::json!(["flow:checkout", "route:checkout:retry"])
        );
    }

    #[test]
    fn empty_conversation_metadata_filter_is_omitted() {
        let wire = ConversationListParams {
            metadata: Some(HashMap::new()),
            ..Default::default()
        }
        .to_wire()
        .unwrap();

        assert!(wire.get("metadata").is_none());
    }

    #[test]
    fn typed_event_filters_reach_the_wire() {
        let wire = EventListParams {
            conversation_id: Some("conv".into()),
            conversation_ids: Some(vec!["a".into()]),
            event_id: Some(vec!["e".into()]),
            service_name: Some("svc".into()),
            environment: Some("prod".into()),
            runtime_group_id: Some(Uuid::nil()),
            runtime_group_unattributed: Some(true),
            lens: Some("lens".into()),
            pattern_id: Some(Uuid::nil()),
            status: Some("open".into()),
            include_superseded: Some(false),
            severities: Some(vec!["high".into()]),
            trace_id: Some("trace".into()),
            span_id: Some("span".into()),
            owner_key: Some("owner".into()),
            ..EventListParams::new(IntrospectionEventName::Observation)
        }
        .to_wire()
        .unwrap();

        assert_eq!(wire["event_name"], "introspection.observation");
        assert_eq!(wire["conversation_id"], "conv");
        assert_eq!(wire["conversation_ids"], serde_json::json!(["a"]));
        assert_eq!(wire["event_id"], serde_json::json!(["e"]));
        assert_eq!(wire["runtime_group_unattributed"], true);
        assert_eq!(wire["lens"], "lens");
        assert_eq!(wire["pattern_id"], Uuid::nil().to_string());
        assert_eq!(wire["include_superseded"], false);
        assert_eq!(wire["severities"], serde_json::json!(["high"]));
        assert_eq!(wire["trace_id"], "trace");
        assert_eq!(wire["span_id"], "span");
        assert_eq!(wire["owner_key"], "owner");
    }

    #[test]
    fn an_empty_repeated_filter_is_omitted_not_sent_empty() {
        // An empty vec is "no filter", not "match nothing" -- sending
        // `?conversation_ids=` would be a filter on the empty string.
        let wire = ConversationListParams {
            conversation_ids: Some(Vec::new()),
            ..Default::default()
        }
        .to_wire()
        .unwrap();
        assert!(wire.get("conversation_ids").is_none());
    }

    #[test]
    fn the_filters_escape_hatch_still_wins_over_a_typed_field() {
        // `filters` is merged last, so a caller working around a stale SDK
        // build can still override what the typed field lowered.
        let wire = ConversationListParams {
            environment: Some("prod".into()),
            filters: Some(HashMap::from([(
                "environment".to_string(),
                serde_json::Value::from("staging"),
            )])),
            ..Default::default()
        }
        .to_wire()
        .unwrap();
        assert_eq!(wire["environment"], "staging");
    }

    #[test]
    fn lookback_is_mutually_exclusive_with_start_end() {
        let err = ConversationListParams {
            lookback: Some("24h".into()),
            start: Some("2026-01-01T00:00:00Z".into()),
            ..Default::default()
        }
        .to_wire()
        .unwrap_err();
        assert!(matches!(err, IntrospectionAPIError::InvalidConfig(_)));
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn lookback_computes_start_date_and_omits_end() {
        let wire = EventListParams {
            lookback: Some("24h".into()),
            ..EventListParams::new(IntrospectionEventName::Feedback)
        }
        .to_wire()
        .unwrap();
        let start = wire["start_date"].as_str().unwrap();
        // RFC3339 UTC, second precision.
        assert!(start.ends_with('Z'));
        assert_eq!(start.len(), 20);
        assert!(wire.get("end_date").is_none());
    }

    #[test]
    fn parse_lookback_supports_compound_units() {
        assert_eq!(parse_lookback("24h").unwrap(), Duration::from_secs(86_400));
        assert_eq!(parse_lookback("30m").unwrap(), Duration::from_secs(1_800));
        assert_eq!(parse_lookback("7d").unwrap(), Duration::from_secs(604_800));
        assert_eq!(parse_lookback("1h30m").unwrap(), Duration::from_secs(5_400));
        assert!(parse_lookback("24").is_err());
        assert!(parse_lookback("").is_err());
        assert!(parse_lookback("10y").is_err());
    }

    #[test]
    fn rfc3339_utc_formats_known_epoch() {
        // 1_700_000_000 == 2023-11-14T22:13:20Z
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(rfc3339_utc(t), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn limit_out_of_range_is_rejected() {
        assert!(ConversationListParams {
            limit: Some(0),
            ..Default::default()
        }
        .to_wire()
        .is_err());
        assert!(ConversationListParams {
            limit: Some(1001),
            ..Default::default()
        }
        .to_wire()
        .is_err());
    }

    #[test]
    fn event_params_require_event_name_and_pass_family_filters() {
        // `event_name` is a required (compile-enforced) field: there is no
        // `Default` impl and no way to build the params without a family.
        let wire = EventListParams {
            filters: Some(HashMap::from([
                ("pattern_id".to_string(), json!("pat_1")),
                ("include_superseded".to_string(), json!(true)),
            ])),
            ..EventListParams::new(IntrospectionEventName::Observation)
        }
        .to_wire()
        .unwrap();
        assert_eq!(wire["event_name"], "introspection.observation");
        // Family-scoped filters pass through verbatim.
        assert_eq!(wire["pattern_id"], "pat_1");
        assert_eq!(wire["include_superseded"], true);
        // The retired grain-era params never reach the wire.
        assert!(wire.get("grain").is_none());
        assert!(wire.get("include").is_none());
        assert!(wire.get("event_name_prefix").is_none());
        assert!(wire.get("q").is_none());
        assert!(wire.get("q_regex").is_none());
    }

    #[test]
    fn event_params_reject_event_name_filter_override() {
        let params = EventListParams {
            filters: Some(HashMap::from([(
                "event_name".to_string(),
                json!("introspection.pattern"),
            )])),
            ..EventListParams::new(IntrospectionEventName::Feedback)
        };

        let err = params.to_wire().unwrap_err().to_string();
        assert!(err.contains("event_name"), "{err}");
        assert!(err.contains("EventListParams::new"), "{err}");
    }

    #[test]
    fn introspection_event_name_serde_uses_dotted_names() {
        for (variant, wire) in [
            (IntrospectionEventName::Feedback, "introspection.feedback"),
            (
                IntrospectionEventName::Observation,
                "introspection.observation",
            ),
            (
                IntrospectionEventName::ObservationClusteringRun,
                "introspection.observation_clustering.run",
            ),
            (IntrospectionEventName::Judgement, "introspection.judgement"),
            (IntrospectionEventName::Pattern, "introspection.pattern"),
            (
                IntrospectionEventName::PatternAssignment,
                "introspection.pattern.assignment",
            ),
        ] {
            assert_eq!(variant.as_str(), wire);
            assert_eq!(serde_json::to_value(&variant).unwrap(), json!(wire));
            let back: IntrospectionEventName = serde_json::from_value(json!(wire)).unwrap();
            assert_eq!(back, variant);
        }

        let future: IntrospectionEventName =
            serde_json::from_value(json!("introspection.future.family")).unwrap();
        assert_eq!(
            future,
            IntrospectionEventName::Unknown("introspection.future.family".to_string())
        );
        assert_eq!(
            serde_json::to_value(&future).unwrap(),
            json!("introspection.future.family")
        );
        let wire = EventListParams::new(future).to_wire().unwrap();
        assert_eq!(wire["event_name"], "introspection.future.family");
    }

    #[test]
    fn observation_event_round_trips_typed_payload() {
        let raw = json!({
            "id": "evt_1",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.observation",
            "conversation_id": "conv_1",
            "runtime_group_id": "00000000-0000-0000-0000-00000000cccc",
            "payload": {
                "observation_id": "00000000-0000-0000-0000-000000000042",
                "lens": "user_frustration",
                "summary": "User repeated the question",
                "severity": "high",
                "confidence": 0.92,
                "pattern_id": "pat_7",
                "assignment_score": 0.81,
            },
        });
        let event: Event = serde_json::from_value(raw.clone()).unwrap();
        let Event::Observation(obs) = &event else {
            panic!("expected Observation, got {event:?}");
        };
        assert_eq!(obs.id, "evt_1");
        assert_eq!(obs.conversation_id.as_deref(), Some("conv_1"));
        assert_eq!(
            obs.payload.observation_id.to_string(),
            "00000000-0000-0000-0000-000000000042"
        );
        assert_eq!(obs.payload.lens.as_deref(), Some("user_frustration"));
        assert_eq!(obs.payload.confidence, Some(0.92));
        // The current-assignment fold fields ride on the payload.
        assert_eq!(obs.payload.pattern_id.as_deref(), Some("pat_7"));
        assert_eq!(
            event.event_name(),
            Some(IntrospectionEventName::Observation)
        );
        // Serialize → the top-level discriminator is re-emitted.
        let back = serde_json::to_value(&event).unwrap();
        assert_eq!(back["event_name"], "introspection.observation");
        assert_eq!(back["payload"]["pattern_id"], "pat_7");
    }

    #[test]
    fn pattern_event_round_trips_fold_fields() {
        let raw = json!({
            "id": "evt_2",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.pattern",
            "payload": {
                "pattern_id": "pat_7",
                "action": "created",
                "name": "Repeated question",
                "status": "active",
                "created_at": "2026-06-01T00:00:00Z",
                "last_detected_at": "2026-07-01T00:00:00Z",
            },
        });
        let event: Event = serde_json::from_value(raw).unwrap();
        let Event::Pattern(pat) = &event else {
            panic!("expected Pattern, got {event:?}");
        };
        assert_eq!(pat.payload.pattern_id, "pat_7");
        // Legacy `introspection.pattern.created` rows normalize server-side
        // to the canonical family with `payload.action = "created"`.
        assert_eq!(pat.payload.action.as_deref(), Some("created"));
        assert_eq!(pat.payload.status.as_deref(), Some("active"));
        assert_eq!(event.event_name(), Some(IntrospectionEventName::Pattern));
    }

    #[test]
    fn feedback_event_round_trips_typed_payload() {
        let raw = json!({
            "id": "evt_3",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.feedback",
            "payload": {
                "name": "thumbs_up",
                "comments": "great answer",
                "value": 1.0,
                "user_id": "user_9",
                "sentiment": "positive",
                "previous_response_id": "resp_42",
                "agent_name": "support-agent",
                "agent_id": "agent_7",
                "properties": {"surface": "chat"},
            },
        });
        let event: Event = serde_json::from_value(raw).unwrap();
        let Event::Feedback(fb) = &event else {
            panic!("expected Feedback, got {event:?}");
        };
        assert_eq!(fb.payload.name, "thumbs_up");
        assert_eq!(fb.payload.value, Some(1.0));
        assert_eq!(fb.payload.sentiment.as_deref(), Some("positive"));
        // gen_ai anchoring fields (cloud phase-1 final models).
        assert_eq!(fb.payload.previous_response_id.as_deref(), Some("resp_42"));
        assert_eq!(fb.payload.agent_name.as_deref(), Some("support-agent"));
        assert_eq!(fb.payload.agent_id.as_deref(), Some("agent_7"));
        assert_eq!(
            fb.payload.properties.as_ref().unwrap()["surface"],
            json!("chat")
        );
        let back = serde_json::to_value(&event).unwrap();
        assert_eq!(back["payload"]["previous_response_id"], "resp_42");
        assert_eq!(back["payload"]["agent_name"], "support-agent");
        assert_eq!(back["payload"]["agent_id"], "agent_7");
    }

    #[test]
    fn pattern_assignment_event_tolerates_explicit_unassignment() {
        // `pattern_id: null` = explicitly unassigned — still the typed
        // variant (observation_id alone is identity), never Unknown.
        let raw = json!({
            "id": "evt_7",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.pattern.assignment",
            "payload": {
                "observation_id": "00000000-0000-0000-0000-000000000042",
                "pattern_id": null,
                "method": "manual",
            },
        });
        let event: Event = serde_json::from_value(raw).unwrap();
        let Event::PatternAssignment(pa) = &event else {
            panic!("expected PatternAssignment, got {event:?}");
        };
        assert_eq!(
            pa.payload.observation_id.to_string(),
            "00000000-0000-0000-0000-000000000042"
        );
        assert!(pa.payload.pattern_id.is_none());
        assert_eq!(pa.payload.method.as_deref(), Some("manual"));

        // Assigned rows still carry the pattern.
        let assigned = json!({
            "id": "evt_8",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.pattern.assignment",
            "payload": {
                "observation_id": "00000000-0000-0000-0000-000000000042",
                "pattern_id": "pat_7",
                "score": 0.8,
            },
        });
        let event: Event = serde_json::from_value(assigned).unwrap();
        let Event::PatternAssignment(pa) = &event else {
            panic!("expected PatternAssignment, got {event:?}");
        };
        assert_eq!(pa.payload.pattern_id.as_deref(), Some("pat_7"));
        assert_eq!(pa.payload.score, Some(0.8));
    }

    #[test]
    fn judgement_event_round_trips_typed_payload() {
        let raw = json!({
            "id": "evt_4",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.judgement",
            "payload": {
                "judgement_id": "jm_1",
                "judge_id": "judge_1",
                "result": "pass",
                "definition_hash": "abc123",
                "contract_version": "1",
                "sequence_hash": "def456",
                "experiment_arm_id": "00000000-0000-0000-0000-00000000eeee",
            },
        });
        let event: Event = serde_json::from_value(raw).unwrap();
        let Event::Judgement(j) = &event else {
            panic!("expected Judgement, got {event:?}");
        };
        assert_eq!(j.payload.judgement_id, "jm_1");
        assert_eq!(j.payload.result.as_deref(), Some("pass"));
        assert_eq!(
            j.payload.experiment_arm_id.unwrap().to_string(),
            "00000000-0000-0000-0000-00000000eeee"
        );
    }

    #[test]
    fn unknown_event_family_does_not_fail_the_page() {
        // A seventh family added server-side after this SDK build must not
        // fail the whole page — it falls into `Event::Unknown` verbatim.
        let payload = json!({
            "records": [
                {
                    "id": "evt_5",
                    "timestamp": "2026-07-01T00:00:00Z",
                    "event_name": "introspection.brand_new.family",
                    "payload": {"anything": true},
                },
                {
                    "id": "evt_6",
                    "timestamp": "2026-07-01T00:00:00Z",
                    "event_name": "introspection.feedback",
                    "payload": {"name": "thumbs_down"},
                },
            ],
            "count": 2,
            "next": null,
        });
        let page: Paginated<Event> = serde_json::from_value(payload).unwrap();
        assert_eq!(page.records.len(), 2);
        let Event::Unknown(raw) = &page.records[0] else {
            panic!("expected Unknown, got {:?}", page.records[0]);
        };
        assert_eq!(raw["event_name"], "introspection.brand_new.family");
        assert!(page.records[0].event_name().is_none());
        assert!(matches!(page.records[1], Event::Feedback(_)));
    }

    #[test]
    fn metrics_query_maps_window_to_from_to_timestamp() {
        let wire = MetricsQuery {
            view: "spans".into(),
            metrics: vec![MetricSpec {
                measure: "duration_ns".into(),
                aggregation: "p95".into(),
            }],
            start: Some("2026-06-01T00:00:00Z".into()),
            end: Some("2026-07-01T00:00:00Z".into()),
            ..Default::default()
        }
        .to_wire()
        .unwrap();
        assert_eq!(wire["view"], "spans");
        assert_eq!(wire["metrics"][0]["aggregation"], "p95");
        assert_eq!(wire["from_timestamp"], "2026-06-01T00:00:00Z");
        assert_eq!(wire["to_timestamp"], "2026-07-01T00:00:00Z");
    }

    #[test]
    fn metrics_query_rejects_lookback_with_start() {
        let err = MetricsQuery {
            view: "spans".into(),
            metrics: vec![],
            lookback: Some("7d".into()),
            start: Some("2026-06-01T00:00:00Z".into()),
            ..Default::default()
        }
        .to_wire()
        .unwrap_err();
        assert!(matches!(err, IntrospectionAPIError::InvalidConfig(_)));
    }
}

// ----- trajectory-v1 (conversation export) ----------------------------------

/// One tool invocation inside a [`TrajectoryAssistantRecord`].
///
/// `args` is a **JSON-encoded string**, not an object. That is the upstream
/// trajectory-v1 contract rather than an oversight here: the encoded value is
/// an object, and a malformed or scalar source value arrives as
/// `{"_raw": ...}` so the evidence survives without breaking the schema.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryToolCall {
    /// Identifier linking this call to its [`TrajectoryRecord::Tool`] result.
    pub id: String,
    /// Tool/function name.
    pub name: String,
    /// JSON-encoded arguments object.
    pub args: String,
}

/// Leading record identifying the session the trajectory came from.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryMetaRecord {
    /// Harness that produced the session, e.g. `"claude-code"`.
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// A user turn.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryUserRecord {
    pub content: String,
    /// ISO-8601 timestamp.
    pub timestamp: String,
}

/// Model reasoning, when the source exposed it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryReasoningRecord {
    pub content: String,
    /// ISO-8601 timestamp.
    pub timestamp: String,
}

/// An assistant turn — prose, or tool calls, never both.
///
/// The two are distinguished by `content`: a prose record carries text and no
/// `tool_calls`; a tool-call record carries `content: null`. That null is
/// load-bearing and always present on the wire, so `content` is
/// `Option<String>` rather than a skipped field.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryAssistantRecord {
    pub content: Option<String>,
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Present only on a tool-call record, and then never empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<TrajectoryToolCall>>,
}

/// A tool result, linked to its call by `tool_call_id`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryToolRecord {
    pub tool_call_id: String,
    pub content: String,
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Source-native success status; absent when the source exposes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
}

/// One record in a trajectory-v1 export, discriminated by `role`.
///
/// Unlike [`Event`], this union has no `Unknown` tail: the record vocabulary
/// is pinned by the `version=1` media-type parameter the client sends, and a
/// server that does not implement that version answers `406` rather than
/// returning a shape with new roles in it. A new role therefore arrives as a
/// new version, not as an unrecognised variant.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum TrajectoryRecord {
    Meta(TrajectoryMetaRecord),
    User(TrajectoryUserRecord),
    Reasoning(TrajectoryReasoningRecord),
    Assistant(TrajectoryAssistantRecord),
    Tool(TrajectoryToolRecord),
}

/// The trajectory-v1 wire shape: a non-empty top-level array of records.
///
/// A projection derived on read from the stored GenAI messages, not a second
/// storage format — nothing accepts a trajectory as input, so there is no
/// `TrajectoryCreate`.
pub type Trajectory = Vec<TrajectoryRecord>;

/// Query params for `GET /v1/conversations/{id}/export`.
///
/// The export is assembled server-side over the whole conversation, so there
/// is no cursor or page bound here: every field filters what gets assembled.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConversationExportParams {
    /// Agent selector: `"root"` for the depth-zero transcript, an exact agent
    /// id for one invocation, or `None` for the complete conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    /// Partition lookback bound in days (1-365).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookback_days: Option<u16>,
    /// Read via a `/v1/shares` grant for this conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_id: Option<Uuid>,
    /// Lower bound on which records are assembled (ISO 8601).
    ///
    /// Named for the wire rather than carrying the ergonomic
    /// `start`/`end`/`lookback` aliases the list params take, because this
    /// route's relative window is the separate `lookback_days` integer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    /// Upper bound on which records are assembled (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
}
