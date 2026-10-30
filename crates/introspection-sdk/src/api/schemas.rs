//! Wire types for the DP `/v1/tasks` and `/v1/files` surface.
//!
//! Kept in lockstep with the published Data Plane OpenAPI schema.
//!
//! Field names are kept on-the-wire (`snake_case`) so the JSON round-trips
//! verbatim — no camelCase translation layer.
//!
//! A response type serializes back to the JSON the server sent, except for
//! its `null` fields: a `None` is left out. A field this SDK build does not
//! model is ignored, or reported by the `strict-schema` feature. Enum values
//! added by the DP after this SDK is compiled deserialize into the
//! `Other`/`Unknown` fallback variant so callers can still read the rest of
//! the record.

use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
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

/// Reads a UUID-shaped string as [`StringOrUuid::Uuid`] and any other as a
/// slug, so a document naming a project either way reads back.
impl<'de> Deserialize<'de> for StringOrUuid {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = String::deserialize(d)?;
        Ok(match Uuid::parse_str(&value) {
            Ok(id) => Self::Uuid(id),
            Err(_) => Self::String(value),
        })
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
    /// The agent shape run as an evaluation trial; the only kind that takes
    /// task commands.
    Eval,
    Process,
    /// Forward-compatible escape hatch for kinds the DP adds later.
    Other(String),
}

impl TaskKind {
    /// On-the-wire string form.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Agent => "agent",
            Self::Eval => "eval",
            Self::Process => "process",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for TaskKind {
    fn from(s: &str) -> Self {
        match s {
            "agent" => Self::Agent,
            "eval" => Self::Eval,
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

/// Read an explicit `null` as the field's default, as a missing key is with
/// `#[serde(default)]`.
fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Cursor pagination envelope shared by every DP list endpoint.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct Paginated<T> {
    /// A `null` or missing list reads as empty.
    #[serde(default, deserialize_with = "null_as_default")]
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

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentInfo {
    #[serde(default)]
    pub sandbox_status: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Task {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub display_index: Option<i64>,
    #[serde(default)]
    pub kind: TaskKind,
    #[serde(default = "default_task_status")]
    pub status: TaskStatus,
    #[serde(default)]
    pub member_id: Option<Uuid>,
    #[serde(default)]
    pub automation_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub last_user_message_at: Option<String>,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub conversation_metadata: Option<BTreeMap<String, String>>,
    #[serde(default)]
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
    pub id: Uuid,
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskRepoRequest {
    pub repo: String,
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    /// Shallow-clone depth, `0` for the full history. `None` takes the
    /// platform default, a depth of `1`.
    #[serde(default)]
    pub depth: Option<u32>,
}

/// POST /v1/tasks body. All fields optional — the DP fills in defaults.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskCreate {
    /// The runtime to run on, for a caller whose credential names none: a
    /// member token such as a native email-code session. A runner
    /// credential's claim is authoritative and the API ignores this. Only a
    /// runtime id is accepted, not a group or slug.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// `None` is an agent task. [`TaskKind::Eval`] needs the `evals`
    /// capability; [`TaskKind::Process`] is internal and refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<TaskKind>,
    /// Consent to `tasks.commands` against the live sandbox; eval tasks only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<bool>,
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
    /// A caller whose member tags intersect a row's tags can still read and
    /// write it today, but that implicit access is being retired: to share
    /// with a cohort, create a share with [`ShareCreate::granted_tag`]. Shared
    /// writers may not replace the tags themselves; that remains
    /// owner/privileged-only.
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
    /// Only tasks created for this runtime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    /// Filter by one `key:value` tag. ANDed with the ownership predicate, so
    /// it only ever narrows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPrompt {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskRunCreate {
    /// Stable identity of this message across retries (1..256 characters).
    /// Resend a request whose outcome is unknown with the same `delivery_id`
    /// and the same body: the API answers with the run it already accepted
    /// instead of starting a second turn. Reusing it for a different body
    /// answers `409`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<String>,
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

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskRun {
    pub id: String,
    pub task_id: Uuid,
    pub status: TaskStatus,
    /// Why the run failed; set only on a failed run.
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub completion_reason: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskCreateResponse {
    pub task: Task,
    pub run: TaskRun,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskRunResponse {
    pub run: TaskRun,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
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
    Issue,
    /// A channel's audience. Written only by the control plane, so it appears
    /// on reads but `POST /v1/shares` refuses it.
    Channel,
}

#[serde_with::skip_serializing_none]
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
    #[serde(default)]
    pub granted_tag: Option<String>,
    /// Conversation shares only: the grantee sees records from this instant on.
    #[serde(default)]
    pub visible_from: Option<String>,
    pub created_by_member_id: Uuid,
    /// The plain resource URL. Shares apply automatically to the grantee's
    /// reads, so it carries no share id.
    #[serde(default)]
    pub url: Option<String>,
}

/// `POST /v1/shares` body.
///
/// The grantee fields are ANDed: `granted_member_id` alone grants that member,
/// `granted_tag` alone grants everyone whose token carries the tag, both grant
/// that member only while they hold the tag, and neither grants the whole
/// project. A share admits the grantee; the caller's token scopes decide
/// whether they may read, write or delete the resource. A share with
/// `granted_tag` is the supported way to share with a cohort; implicit access
/// through tags is being retired.
#[derive(Debug, Clone, Serialize)]
pub struct ShareCreate {
    pub resource_type: ShareResourceType,
    pub resource_id: String,
    /// An end customer is a member, so there is no separate identity target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_tag: Option<String>,
    /// Conversation shares only (ISO 8601, not in the future): hide records
    /// before this instant from the grantee. A conversation shared this way
    /// cannot be forked through `fork_share_id` (409).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_from: Option<String>,
}

impl ShareCreate {
    /// A project-wide share of one resource. Narrow it with struct-update
    /// syntax.
    pub fn new(resource_type: ShareResourceType, resource_id: impl Into<String>) -> Self {
        Self {
            resource_type,
            resource_id: resource_id.into(),
            granted_member_id: None,
            granted_tag: None,
            visible_from: None,
        }
    }
}

/// `PATCH /v1/shares/{id}` body. A share's grantee never changes in place;
/// only a conversation share's `visible_from` does. Only the grantor or a
/// privileged caller may update a share.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ShareUpdate {
    /// Always sent: `Some(ts)` sets it, `None` clears it (sent as `null`) so
    /// the grantee sees the whole history. Conversation shares only, and not
    /// in the future (422 otherwise).
    pub visible_from: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_tag: Option<String>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

// ----- files -----------------------------------------------------------------

#[serde_with::skip_serializing_none]
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
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
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
    /// Task whose run generated this file as an output.
    #[serde(default)]
    pub generated_output_task_id: Option<Uuid>,
    /// When the file is deleted; `None` keeps it.
    #[serde(default)]
    pub expires_at: Option<String>,
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
    /// A caller whose member tags intersect a file's tags can still read and
    /// write it today, but that implicit access is being retired: to share
    /// with a cohort, create a share with [`ShareCreate::granted_tag`]. Shared
    /// writers may not replace the tags themselves; that remains
    /// owner/privileged-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FileCreateText {
    pub name: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Tags stamped on the file when this request creates it; a new version
    /// keeps the file's existing tags — change them with
    /// [`Files::update`](crate::api::Files::update).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Expire the file this many seconds from now (1 up to the server's
    /// maximum): it gains an `expires_at`, and a maintenance sweep deletes it
    /// after that instant. `None`, the default, keeps it forever. When the
    /// request adds a version to an existing file, a set value replaces that
    /// file's expiry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u64>,
}

/// Body for [`FileVersions::create_text`](crate::api::FileVersions::create_text)
/// (`POST /v1/files/{id}/versions` as JSON) — a new text version of an
/// existing file. The file id travels in the path and must be the chain's
/// newest version.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FileVersionCreateText {
    pub content: String,
    /// Defaults to `text/markdown` on the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Refuse the version unless the file's current content has this SHA-256
    /// (64 hex characters), so a concurrent writer is not overwritten.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
    /// Replace the file's expiry with this many seconds from now; see
    /// [`FileCreateText::ttl_seconds`]. `None` keeps the current expiry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FileListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_total: Option<bool>,
    /// Include superseded file versions. Unset, the server lists only the
    /// latest version of each file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_versions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Case-insensitive substring of the file name; `%`, `_` and `\` match
    /// literally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_contains: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_type: Option<FileType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
    /// Filter by the task that wrote the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    /// Filter by one tag. ANDed with the ownership predicate, so it only
    /// ever narrows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Match files whose `metadata` holds every pair, each compared exactly
    /// against the string value (at most 16 pairs). Values may contain `:`;
    /// keys are letters, digits, `_` and `-`. Each pair is lowered to a
    /// repeated `metadata=key:value` query parameter. A server that predates
    /// this filter ignores it and returns the unfiltered list.
    #[serde(
        serialize_with = "serialize_metadata_filter",
        skip_serializing_if = "metadata_filter_is_empty"
    )]
    pub metadata: Option<HashMap<String, String>>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Parameters for `GET /v1/files/{id}/versions`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FileVersionListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
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

#[serde_with::skip_serializing_none]
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
    pub git_commit_subject: Option<String>,
    #[serde(default)]
    pub sub_path: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// MCP servers the recipe declares (`id`, `required`, `tools`).
    #[serde(default)]
    pub mcp_servers: Option<Vec<serde_json::Value>>,
    /// The recipe's Cedar policies (`schema`, `policies`, `routes`).
    #[serde(default)]
    pub policies: Option<serde_json::Value>,
    /// Result of the recipe check.
    #[serde(default)]
    pub validation: Option<RecipeValidation>,
    pub created_by_member_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
}

/// Where a Recipe's check stands. `pending` and `failed` may be retried
/// (`RecipeUpdateParams::retry_validation`, with `authoring`); `valid` and `invalid` are the
/// checker's verdict. `Other` captures a status the CP adds later so the rest
/// of the record still reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RecipeValidationStatus {
    #[default]
    Pending,
    Valid,
    Invalid,
    /// The check could not run; `error_message` says why.
    Failed,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl RecipeValidationStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Failed => "failed",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for RecipeValidationStatus {
    fn from(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "valid" => Self::Valid,
            "invalid" => Self::Invalid,
            "failed" => Self::Failed,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for RecipeValidationStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RecipeValidationStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Whether a diagnostic fails the check (`error`) or only reports a finding
/// (`warning`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RecipeValidationSeverity {
    #[default]
    Error,
    Warning,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl RecipeValidationSeverity {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for RecipeValidationSeverity {
    fn from(s: &str) -> Self {
        match s {
            "error" => Self::Error,
            "warning" => Self::Warning,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for RecipeValidationSeverity {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RecipeValidationSeverity {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// One-based source location of a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RecipeValidationSpan {
    pub line: u32,
    pub column: u32,
}

/// One finding of the Recipe check on the committed content.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RecipeValidationDiagnostic {
    pub code: String,
    #[serde(default)]
    pub severity: RecipeValidationSeverity,
    /// The file the diagnostic is about.
    pub path: String,
    #[serde(default)]
    pub span: Option<RecipeValidationSpan>,
    pub message: String,
    #[serde(default)]
    pub help: Option<String>,
}

/// Result of the Recipe check. `diagnostics` carry the checker's per-file
/// findings; `error_message` says why a `failed` check could not run.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct RecipeValidation {
    #[serde(default)]
    pub status: RecipeValidationStatus,
    #[serde(default)]
    pub validator_name: Option<String>,
    #[serde(default)]
    pub validator_version: Option<String>,
    #[serde(default)]
    pub checked_at: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub diagnostics: Vec<RecipeValidationDiagnostic>,
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
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Where a new Recipe's GitHub repository comes from, when the project has no
/// repository row for it yet: the Control Plane links it to the project.
#[cfg(feature = "authoring")]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RecipeRepositorySource {
    pub integration_id: Uuid,
    /// `owner/repo`.
    pub full_name: String,
}

/// Body of `POST /v1/recipes`. Name the repository with `repository_id`, or
/// with `repository` for a GitHub one the project has not linked yet.
#[cfg(feature = "authoring")]
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RecipeCreateParams {
    /// Project slug or id.
    pub project: StringOrUuid,
    /// Operator label; versions of one Recipe may share it.
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub repository_id: Option<Uuid>,
    #[serde(default)]
    pub repository: Option<RecipeRepositorySource>,
    /// Branch, tag or short sha.
    #[serde(default)]
    pub git_ref: Option<String>,
    /// The full 40-character commit sha.
    #[serde(default)]
    pub git_commit_sha: Option<String>,
    #[serde(default)]
    pub git_commit_subject: Option<String>,
    /// The Recipe's directory inside the repository (monorepo support).
    #[serde(default)]
    pub sub_path: Option<String>,
    /// Derived from the name and the short sha when unset.
    #[serde(default)]
    pub slug: Option<String>,
}

#[cfg(feature = "authoring")]
impl RecipeCreateParams {
    pub fn new(project: impl Into<StringOrUuid>, name: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            name: name.into(),
            ..Default::default()
        }
    }
}

/// The one validation change a client may ask for: back to `pending`, which
/// runs the Recipe check again.
#[cfg(feature = "authoring")]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RecipeValidationUpdate {
    /// Only [`RecipeValidationStatus::Pending`] is accepted.
    pub status: RecipeValidationStatus,
}

#[cfg(feature = "authoring")]
impl RecipeValidationUpdate {
    pub fn pending() -> Self {
        Self {
            status: RecipeValidationStatus::Pending,
        }
    }
}

/// Body of `PATCH /v1/recipes/{id}`. A field left `None` is not sent and stays
/// unchanged. The git pin cannot change: pinning other content is a new
/// Recipe.
#[cfg(feature = "authoring")]
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RecipeUpdateParams {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// See [`Self::retry_validation`].
    #[serde(default)]
    pub validation: Option<RecipeValidationUpdate>,
}

#[cfg(feature = "authoring")]
impl RecipeUpdateParams {
    /// `{"validation": {"status": "pending"}}`: run the Recipe check again.
    /// Answers 409 unless the validation is `pending` or `failed`.
    pub fn retry_validation() -> Self {
        Self {
            validation: Some(RecipeValidationUpdate::pending()),
            ..Default::default()
        }
    }
}

// ----- repositories (CP) -----------------------------------------------------

/// Who serves a repository's Git, which decides how a client reaches it.
/// `introspection` is served by the project's own data plane (`hosted` is the
/// name it shipped under, read as the same provider); `github` needs the
/// installation the row's `integration_id` names. `Other` captures a provider
/// the CP adds later so the rest of the record still reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RepositoryProvider {
    #[default]
    Github,
    Introspection,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl RepositoryProvider {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Github => "github",
            Self::Introspection => "introspection",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for RepositoryProvider {
    fn from(s: &str) -> Self {
        match s {
            "github" => Self::Github,
            "introspection" | "hosted" => Self::Introspection,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for RepositoryProvider {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RepositoryProvider {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Whether the hosted copy of a repository exists on its data plane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RepositoryProvisioningStatus {
    #[default]
    Pending,
    Ready,
    Failed,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl RepositoryProvisioningStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for RepositoryProvisioningStatus {
    fn from(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "ready" => Self::Ready,
            "failed" => Self::Failed,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for RepositoryProvisioningStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RepositoryProvisioningStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

fn default_branch() -> String {
    "main".to_string()
}

/// A repository registered to a project — the Git source a recipe pins.
/// Mirrors the CP `RepositoryResponse`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Repository {
    pub id: Uuid,
    pub project_id: Uuid,
    /// The GitHub App installation that reaches a `github` repository;
    /// `None` for a hosted one.
    #[serde(default)]
    pub integration_id: Option<Uuid>,
    /// Credential-free Git transport URL. Authentication comes from the
    /// credential helper configured for its origin, never from the URL.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub provider: RepositoryProvider,
    #[serde(default = "default_branch")]
    pub default_branch: String,
    #[serde(default)]
    pub provisioning_status: RepositoryProvisioningStatus,
    #[serde(default)]
    pub seed_template: Option<String>,
    pub created_at: String,
    /// When the repository last received a push; `None` before the first.
    #[serde(default)]
    pub pushed_at: Option<String>,
    /// The default branch's head commit as of the last push.
    #[serde(default)]
    pub head_commit_sha: Option<String>,
    #[serde(default)]
    pub is_recipe_source: bool,
}

/// Filters supported by `GET /v1/repositories`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RepositoryListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    /// Only the repository with this slug: `owner/repo` for a GitHub one,
    /// the Runtime slug for a hosted one. The route answers a bare array
    /// either way, so a hit is a one-element `Vec`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Body of `POST /v1/repositories?project=…`: a repository the project's data
/// plane hosts, or a GitHub repository to link.
///
/// The variant is the body's `provider`, so a document read with serde must
/// name it (`introspection`, or `hosted`, the name it shipped under, or
/// `github`).
#[cfg(feature = "authoring")]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "provider")]
pub enum RepositoryCreateParams {
    #[serde(rename = "introspection", alias = "hosted")]
    Hosted(HostedRepositoryCreateParams),
    #[serde(rename = "github")]
    Github(GithubRepositoryCreateParams),
}

#[cfg(feature = "authoring")]
impl RepositoryCreateParams {
    /// A hosted repository seeded with the deployment's default template.
    pub fn hosted(slug: impl Into<String>) -> Self {
        Self::Hosted(HostedRepositoryCreateParams {
            slug: slug.into(),
            default_branch: None,
            seed_template: None,
        })
    }

    /// The GitHub repository `owner/repo`, reached through the installation
    /// `integration_id` names.
    pub fn github(integration_id: Uuid, full_name: &str) -> Self {
        Self::Github(GithubRepositoryCreateParams {
            integration_id,
            url: format!("https://github.com/{full_name}"),
            name: full_name.to_string(),
        })
    }
}

/// [`RepositoryCreateParams::Hosted`]. Idempotent on `slug`: a repeat answers
/// the live row. The hosted copy exists once the first push lands; see
/// [`Repository::provisioning_status`].
#[cfg(feature = "authoring")]
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct HostedRepositoryCreateParams {
    /// A Runtime slug.
    pub slug: String,
    /// `main` when unset.
    #[serde(default)]
    pub default_branch: Option<String>,
    /// The template rendered as the first commit. `None` sends nothing and
    /// takes the deployment's default; `Some(None)` sends `null`, an empty
    /// repository.
    #[serde(
        default,
        with = "serde_with::rust::double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub seed_template: Option<Option<String>>,
}

/// [`RepositoryCreateParams::Github`]. A second link of the same URL answers
/// 409 `repository_already_registered`.
#[cfg(feature = "authoring")]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GithubRepositoryCreateParams {
    /// The GitHub App installation that reaches the repository.
    pub integration_id: Uuid,
    pub url: String,
    /// `owner/repo`.
    pub name: String,
}

// ----- repository contents (DP) ----------------------------------------------

/// What a directory entry is — mirrors the DP `RepositoryEntryType`.
///
/// `Other` keeps a listing readable when the Data Plane reports a kind this
/// SDK build predates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryEntryType {
    File,
    Dir,
    Symlink,
    Submodule,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl RepositoryEntryType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::File => "file",
            Self::Dir => "dir",
            Self::Symlink => "symlink",
            Self::Submodule => "submodule",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl From<&str> for RepositoryEntryType {
    fn from(s: &str) -> Self {
        match s {
            "file" => Self::File,
            "dir" => Self::Dir,
            "symlink" => Self::Symlink,
            "submodule" => Self::Submodule,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for RepositoryEntryType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RepositoryEntryType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// One entry of a repository directory listing.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: RepositoryEntryType,
    #[serde(default)]
    pub size: u64,
    pub sha: String,
}

/// One page of a directory listing.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryDirectory {
    pub path: String,
    /// What `ref` resolved to; every page of the listing is read at it.
    pub commit_sha: String,
    pub records: Vec<RepositoryEntry>,
    pub count: u64,
    /// Opaque cursor for the next page; `None` on the last.
    #[serde(default)]
    pub next: Option<String>,
}

/// A file's content at one commit.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryFile {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub sha: String,
    /// What `ref` resolved to.
    pub commit_sha: String,
    /// `utf-8` or `base64`.
    pub encoding: String,
    pub content: String,
    /// The file is larger than a read returns, and `content` is empty.
    #[serde(default)]
    pub truncated: bool,
}

/// `GET /v1/repositories/{id}/contents/{path}` — a directory or a file.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RepositoryContent {
    Dir(RepositoryDirectory),
    File(RepositoryFile),
}

/// Query for a repository contents read.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ContentsQuery {
    /// Repository-relative path; empty is the root. Read by
    /// `RepositoryContents::list`; `RepositoryContents::get` takes its path as
    /// an argument instead.
    #[serde(skip)]
    pub path: String,
    /// Branch, tag or commit; the default branch when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    /// Page size for a directory listing (server default 100, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

// ----- repository commits (DP) -----------------------------------------------

/// The author or committer of a commit.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryCommitPerson {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
}

/// One commit of a repository's history.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryCommit {
    pub sha: String,
    #[serde(default)]
    pub parents: Vec<String>,
    pub message: String,
    pub author: RepositoryCommitPerson,
    pub committer: RepositoryCommitPerson,
}

/// How a commit changed a file — mirrors the DP `RepositoryCommitFileStatus`.
///
/// `Other` keeps a commit readable when the Data Plane reports a status this
/// SDK build predates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryCommitFileStatus {
    Added,
    Removed,
    Modified,
    Renamed,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl RepositoryCommitFileStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Modified => "modified",
            Self::Renamed => "renamed",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl From<&str> for RepositoryCommitFileStatus {
    fn from(s: &str) -> Self {
        match s {
            "added" => Self::Added,
            "removed" => Self::Removed,
            "modified" => Self::Modified,
            "renamed" => Self::Renamed,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for RepositoryCommitFileStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RepositoryCommitFileStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// One file a commit changed.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryCommitFile {
    pub filename: String,
    pub status: RepositoryCommitFileStatus,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
    #[serde(default)]
    pub changes: u64,
}

/// `GET /v1/repositories/{id}/commits/{sha}` — a commit with what it changed.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryCommitDetail {
    #[serde(flatten)]
    pub commit: RepositoryCommit,
    #[serde(default)]
    pub files: Vec<RepositoryCommitFile>,
    /// The whole commit as a unified git diff.
    #[serde(default)]
    pub patch: String,
}

/// One branch of a repository, from `GET /v1/repositories/{id}/branches`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryBranch {
    pub name: String,
    /// The branch's head commit.
    pub sha: String,
    /// When the head commit was committed; only for
    /// [`RepositoryBranchSort::Recent`].
    #[serde(default)]
    pub committed_at: Option<String>,
}

/// The order of [`BranchesQuery`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryBranchSort {
    /// By name, page by page.
    #[default]
    Name,
    /// One name-sorted page of up to `limit` branches, newest commit first,
    /// with `committed_at`, and never a next page.
    Recent,
}

/// Query for a repository's branches.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BranchesQuery {
    /// Only branches whose name contains this, ignoring case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<RepositoryBranchSort>,
    /// Page size (server default and max 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query for a repository's commit history.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CommitsQuery {
    /// Branch, tag or commit to walk back from; the default branch when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    /// Only commits that touch this repository-relative path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Page size (server default 30, max 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

// ----- repository merges (DP) ------------------------------------------------

/// `POST /v1/repositories/{id}/merges` body — mirrors GitHub's merges API.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RepositoryMergeCreate {
    /// The branch to merge into.
    pub base: String,
    /// A branch name or a full 40-character commit sha to merge.
    pub head: String,
    /// Defaults server-side to `Merge {head} into {base}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<String>,
}

/// The merge commit a `201` answers with. For a `base` with no commits, `sha`
/// equals `head_sha` and `parents` is `[head_sha]`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryMergeCommit {
    pub sha: String,
    pub base: String,
    pub head: String,
    pub head_sha: String,
    #[serde(default)]
    pub parents: Vec<String>,
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

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Runtime {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    /// `None` until a recipe is pinned; the CP declares it nullable.
    #[serde(default)]
    pub recipe_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    pub created_by_member_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    pub slug: String,
    /// Environments this version serves (`development` / `staging` /
    /// `production`). Empty when the CP omits it.
    #[serde(default)]
    pub environments: Vec<String>,
    /// Set when the runtime has been withdrawn: it never resolves as the
    /// active runtime for its environment, while in-flight sticky runs keep
    /// using it.
    #[serde(default)]
    pub yanked_at: Option<String>,
    #[serde(default)]
    pub yanked_reason: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// LLM credential source. Defaults to `Managed` when the CP omits
    /// the field (older servers) or sends `"managed"` explicitly.
    #[serde(default)]
    pub llm_mode: RuntimeLlmMode,
    #[serde(default)]
    pub config_json: Option<BTreeMap<String, serde_json::Value>>,
    /// Per-environment git ref each lane tracks (`environment` -> `main` /
    /// `pr/N` / commit sha), projected from the runtime group.
    #[serde(default)]
    pub environment_ref: Option<BTreeMap<String, String>>,
    /// Git ref the runtime's recipe tracks (`main`, `pr/N`, a commit sha).
    #[serde(default)]
    pub recipe_ref: Option<String>,
    /// Kind of recipe the runtime serves (`production`, `preview`, ...).
    #[serde(default)]
    pub recipe_kind: Option<String>,
    /// State of the runtime's image build (`pending`, `building`, `ready`,
    /// `failed`, ...).
    #[serde(default)]
    pub image_build_status: Option<String>,
    #[serde(default)]
    pub image_build_metadata: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub image_build_error_message: Option<String>,
    /// The recipe commit this version's image carries.
    #[serde(default)]
    pub image_revision: Option<String>,
    /// Subject line of that commit.
    #[serde(default)]
    pub image_commit_subject: Option<String>,
    /// The recipe's GitHub repository (`owner/name`).
    #[serde(default)]
    pub image_repository: Option<String>,
    /// `managed` for a first-party runtime Introspection owns, otherwise
    /// `custom`; projected from the runtime group.
    #[serde(default)]
    pub runtime_group_type: Option<String>,
    /// The template the runtime group was created from, projected from the
    /// runtime group.
    #[serde(default)]
    pub template_ref: Option<String>,
    /// The runtime peering the runtime group's sessions take their
    /// configuration from, projected from the runtime group.
    #[serde(default)]
    pub peering_id: Option<Uuid>,
    /// Every MCP server the runtime's recipe declares, resolved for each
    /// environment the runtime serves; sent only by
    /// [`crate::Runtimes::get_with_mcp_requirements`].
    #[serde(default)]
    pub mcp_requirements: Option<Vec<RuntimeMcpRequirement>>,
}

/// One MCP server a runtime's recipe declares, resolved for one environment
/// against the project's connectors, connections and MCP endpoint bindings.
/// Identifiers, statuses and a URL only, never a token.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeMcpRequirement {
    /// The connector slug backing the server, when one does.
    #[serde(default)]
    pub slug: Option<String>,
    pub environment: String,
    /// `ready`, or what is still missing (a connector, a connection, an
    /// endpoint binding, ...).
    pub state: String,
    #[serde(default)]
    pub connector_id: Option<Uuid>,
    #[serde(default)]
    pub connector_provider: Option<String>,
    /// Every connector that could satisfy the slug; more than one with no
    /// environment match makes the requirement ambiguous.
    #[serde(default)]
    pub candidate_connector_ids: Vec<Uuid>,
    #[serde(default)]
    pub connection_id: Option<Uuid>,
    #[serde(default)]
    pub connection_status: Option<String>,
    /// The recipe's MCP server declaration this resolves.
    pub mcp_server_id: String,
    #[serde(default)]
    pub mcp_required: Option<bool>,
    #[serde(default)]
    pub endpoint_id: Option<Uuid>,
    /// The bound endpoint's URL when ready, else the best known URL for the
    /// binding the connect flow would create.
    #[serde(default)]
    pub mcp_server_url: Option<String>,
    /// Where `mcp_server_url` came from (`endpoint`, `catalog`, `connector`
    /// or `template`).
    #[serde(default)]
    pub mcp_server_url_source: Option<String>,
    /// Why the requirement is not ready, in words.
    #[serde(default)]
    pub detail: Option<String>,
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
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// A runtime group's environment lane: a key of its `environment_ref` map and
/// of [`RuntimeUpdateParams::environment_ref`].
///
/// Ordered by its wire value, so a map of lanes serializes in a stable order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Lane {
    Development,
    Staging,
    Production,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl Lane {
    /// The on-the-wire value.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Development => "development",
            Self::Staging => "staging",
            Self::Production => "production",
            Self::Other(value) => value,
        }
    }
}

impl From<&str> for Lane {
    fn from(value: &str) -> Self {
        match value {
            "development" => Self::Development,
            "staging" => Self::Staging,
            "production" => Self::Production,
            other => Self::Other(other.to_string()),
        }
    }
}

impl fmt::Display for Lane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialOrd for Lane {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Lane {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Serialize for Lane {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Lane {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = String::deserialize(d)?;
        Ok(Self::from(value.as_str()))
    }
}

/// What a `PATCH /v1/runtimes/{id}` does to one lane of the group's
/// `environment_ref` map.
///
/// A lane has three states on the wire, and this type plus its key's presence
/// in [`RuntimeUpdateParams::environment_ref`] expresses each one:
///
/// | in the map              | wire                       | effect                  |
/// |-------------------------|----------------------------|-------------------------|
/// | key absent              | (key absent)               | the lane is left as is  |
/// | [`LaneRef::Unpin`]      | `"staging": null`          | unpinned                |
/// | [`LaneRef::Ref`]        | `"staging": "pr/12"`       | pinned to that ref      |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneRef {
    /// `null`: unpin the lane. The server drops it from the group's map and
    /// re-points it at the newest non-yanked default-branch build of the
    /// group, if there is one. From then on each default-branch build moves
    /// the lane onto itself, and an unpinned staging lane can be claimed by
    /// the next PR preview that opens.
    Unpin,
    /// A git ref: the default branch or another branch name, `pr/N`, or a
    /// 7-40 character lowercase hex commit sha.
    ///
    /// A branch or `pr/N` makes the lane follow that ref: the server points
    /// it at the newest non-yanked build of the ref now, and each later build
    /// of the ref advances it. A sha holds the lane on the runtime named in
    /// the request path (unless that runtime is yanked) until the lane is
    /// changed again. The server does not check that the sha is that
    /// runtime's commit, so send it on the version built from the sha.
    Ref(String),
}

impl LaneRef {
    /// The ref, or `None` for [`LaneRef::Unpin`].
    pub fn git_ref(&self) -> Option<&str> {
        match self {
            Self::Unpin => None,
            Self::Ref(git_ref) => Some(git_ref),
        }
    }
}

impl Serialize for LaneRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Unpin => s.serialize_none(),
            Self::Ref(git_ref) => s.serialize_str(git_ref),
        }
    }
}

impl<'de> Deserialize<'de> for LaneRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<String>::deserialize(d)? {
            None => Self::Unpin,
            Some(git_ref) => Self::Ref(git_ref),
        })
    }
}

/// `PATCH /v1/runtimes/{id}` body. A field left `None` (or a lane left out of
/// [`Self::environment_ref`]) is not sent and stays unchanged.
///
/// `name`, `description` and `environment_ref` belong to the runtime group, so
/// they change every version of the Runtime, not only the one in the path.
/// The other fields apply to that version.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeUpdateParams {
    /// Renames the runtime group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `Some(Some(_))` sets the group's description and `Some(None)` sends
    /// `null`, which clears it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_mode: Option<RuntimeLlmMode>,
    /// Replaces this version's `config_json` wholesale; it is not merged. Its
    /// `connectors` entry is validated by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_json: Option<HashMap<String, serde_json::Value>>,
    /// The older way to place a lane on this version. Only [`Lane::Staging`]
    /// is accepted (anything else is a 400; production moves through
    /// [`Self::environment_ref`]). Unless this request also gives the lane a
    /// ref in [`Self::environment_ref`], the server drops the lane from the
    /// group's map and points it at this version; see
    /// [`Self::pin_staging_to_this_version`] for how long that lasts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Lane>,
    /// `true` withdraws this version: it stops resolving as a lane's active
    /// runtime (in-flight sticky runs keep it), running experiments pinned to
    /// it are halted, and every lane it served moves to the newest non-yanked
    /// default-branch build, with any pin of those lanes cleared. `false`
    /// restores it and clears `yanked_at` and `yanked_reason`, and each
    /// unpinned lane returns to the newest default-branch build, which may be
    /// this one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yanked: Option<bool>,
    /// The note recorded with `yanked: Some(true)`. `Some(None)` sends
    /// `null`, as [`Self::unyank`] does, so a restore says outright that no
    /// reason stays recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yanked_reason: Option<Option<String>>,
    /// Lane to [`LaneRef`], merged per key onto
    /// the group's map. A lane missing here is left as it is. Empty, the field
    /// is not sent.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub environment_ref: BTreeMap<Lane, LaneRef>,
}

impl RuntimeUpdateParams {
    /// `{"environment_ref": {lane: git_ref}}`: pin `lane` to `git_ref`. A
    /// branch or `pr/N` makes the lane follow that ref's newest build; a
    /// commit sha holds it on the version in the request path. See
    /// [`LaneRef::Ref`].
    pub fn pin_ref(lane: impl Into<Lane>, git_ref: impl Into<String>) -> Self {
        Self {
            environment_ref: BTreeMap::from([(lane.into(), LaneRef::Ref(git_ref.into()))]),
            ..Default::default()
        }
    }

    /// `{"environment_ref": {lane: null}}`: unpin `lane` back to the default
    /// branch. See [`LaneRef::Unpin`].
    ///
    /// The runtime in the request path only names the group; the lane moves
    /// to the group's newest default-branch build, not to that runtime.
    pub fn unpin(lane: impl Into<Lane>) -> Self {
        Self {
            environment_ref: BTreeMap::from([(lane.into(), LaneRef::Unpin)]),
            ..Default::default()
        }
    }

    /// `{"environment": "staging", "environment_ref": {"staging": null}}`:
    /// put staging on the version in the request path now.
    ///
    /// Staging is left out of the group's map afterwards, which is the
    /// unpinned state: the next default-branch build moves staging onto
    /// itself, and a PR preview that opens may claim it. To keep staging on
    /// one version until it is changed by hand, use
    /// [`Self::pin_ref`]`("staging", sha)` on the version built from `sha`.
    pub fn pin_staging_to_this_version() -> Self {
        Self {
            environment: Some(Lane::Staging),
            environment_ref: BTreeMap::from([(Lane::Staging, LaneRef::Unpin)]),
            ..Default::default()
        }
    }

    /// `{"yanked": true, "yanked_reason": reason}`: withdraw this version. See
    /// [`Self::yanked`].
    pub fn yank(reason: Option<String>) -> Self {
        Self {
            yanked: Some(true),
            yanked_reason: reason.map(Some),
            ..Default::default()
        }
    }

    /// `{"yanked": false, "yanked_reason": null}`: restore a withdrawn
    /// version. See [`Self::yanked`].
    pub fn unyank() -> Self {
        Self {
            yanked: Some(false),
            yanked_reason: Some(None),
            ..Default::default()
        }
    }
}

/// Body of `POST /v1/runtimes`: a Git runtime built from a Recipe.
#[cfg(feature = "authoring")]
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RuntimeCreateParams {
    /// Project slug or id.
    pub project: StringOrUuid,
    pub recipe_id: Uuid,
    /// The runtime group name, shared across versions.
    pub name: String,
    /// The runtime group slug; derived from the name when unset. A slug the
    /// project already has answers 409.
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// `byor` (the default) or `byoh`; `platform` is refused.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub llm_mode: Option<RuntimeLlmMode>,
    /// Its `connectors` entry is validated by the server.
    #[serde(default)]
    pub config_json: Option<BTreeMap<String, serde_json::Value>>,
    /// Create only the group's first live version, as zero-to-one setup
    /// does; answers 409 when the group already has one.
    #[serde(default)]
    pub bootstrap: Option<bool>,
}

#[cfg(feature = "authoring")]
impl RuntimeCreateParams {
    pub fn new(project: impl Into<StringOrUuid>, recipe_id: Uuid, name: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            recipe_id,
            name: name.into(),
            ..Default::default()
        }
    }
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ExperimentGoalGuard {
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

fn default_component_weight() -> f64 {
    1.0
}

/// Judge-backed reward component. `judge_id` comes from `GET /v1/judges` —
/// judges cannot be created through the API; author a `judges/*.yaml` in the
/// recipe repository and it syncs when a runtime versions that commit.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JudgeGoalComponent {
    pub judge_id: Uuid,
    #[serde(default)]
    pub judge_definition_hash: Option<String>,
    #[serde(default = "default_component_weight")]
    pub weight: f64,
    #[serde(default)]
    pub guard: Option<ExperimentGoalGuard>,
}

/// Reserved shape for future telemetry-backed reward components.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelemetryGoalComponent {
    #[serde(default)]
    pub column: Option<String>,
    #[serde(default)]
    pub aggregation: Option<String>,
    #[serde(default = "default_component_weight")]
    pub weight: f64,
    #[serde(default)]
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
#[serde_with::skip_serializing_none]
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Arm {
    pub id: Uuid,
    pub runtime_id: Uuid,
    pub arm_label: String,
    #[serde(default)]
    pub agent_overrides: Option<BTreeMap<String, String>>,
    /// Share of traffic the arm starts with, before the bandit reweights.
    pub initial_weight: i64,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Experiment {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default)]
    pub environment: Option<String>,
    pub status: ExperimentStatus,
    #[serde(default)]
    pub routing_strategy: Option<String>,
    #[serde(default)]
    pub arms: Vec<Arm>,
    #[serde(default)]
    pub goal_json: Option<ExperimentGoal>,
    #[serde(default)]
    pub scoring_interval_seconds: Option<u64>,
    #[serde(default)]
    pub hash_key_fields: Option<Vec<String>>,
    #[serde(default)]
    pub sample_rate: Option<f64>,
    #[serde(default)]
    pub posterior_json: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub weights_json: Option<BTreeMap<String, i64>>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub halted_at: Option<String>,
    #[serde(default)]
    pub halted_reason: Option<String>,
    pub created_by_member_id: Uuid,
    #[serde(default)]
    pub archived_at: Option<String>,
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
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
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
    ClientCredentials,
    IdentityAssertion,
    FederatedExchange,
    PersonAuthorized,
    /// AAuth (-11): the platform is the connector's Agent Provider and its
    /// egress signs for the session; approvals come from `person_server_url`.
    Aauth,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl ConnectorAuthMode {
    /// On-the-wire string form.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Static => "static",
            Self::OauthStored => "oauth_stored",
            Self::ClientCredentials => "client_credentials",
            Self::IdentityAssertion => "identity_assertion",
            Self::FederatedExchange => "federated_exchange",
            Self::PersonAuthorized => "person_authorized",
            Self::Aauth => "aauth",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for ConnectorAuthMode {
    fn from(s: &str) -> Self {
        match s {
            "static" => Self::Static,
            "oauth_stored" => Self::OauthStored,
            "client_credentials" => Self::ClientCredentials,
            "identity_assertion" => Self::IdentityAssertion,
            "federated_exchange" => Self::FederatedExchange,
            "person_authorized" => Self::PersonAuthorized,
            "aauth" => Self::Aauth,
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Connector {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    /// Stable identifier, unique per project; create upserts on it.
    pub slug: String,
    pub name: String,
    /// Provider slug, e.g. `"slack"`, `"gmail"`, `"stripe"`.
    pub provider: String,
    pub auth_mode: ConnectorAuthMode,
    /// Create-time fact (`development` / `staging` / `production`), not
    /// updatable.
    pub environment: String,
    #[serde(default)]
    pub agent_member_id: Option<Uuid>,
    #[serde(default)]
    pub authorization_endpoint: Option<String>,
    #[serde(default)]
    pub token_endpoint: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub api_hosts: Vec<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub person_server_url: Option<String>,
    #[serde(default)]
    pub application_id: Option<Uuid>,
    #[serde(default)]
    pub assertion_audience: Option<String>,
    #[serde(default)]
    pub webhook_url: Option<String>,
    pub status: ConnectorStatus,
    #[serde(default)]
    pub created_by_member_id: Option<Uuid>,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
    /// Cedar entities the recipe's policies read when a request reaches this
    /// connector's hosts.
    #[serde(default)]
    pub policy_entities: Option<Vec<PolicyEntity>>,
    /// Server-derived: whether [`Connectors::authorize`] must name a runtime
    /// for this connector (chat providers need the agent that replies).
    ///
    /// [`Connectors::authorize`]: crate::resources::connectors::Connectors::authorize
    #[serde(default)]
    pub requires_runtime: bool,
}

/// A connection under a connector. Access/refresh tokens are **never**
/// serialized by the API, so they do not appear here.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Connection {
    pub id: Uuid,
    pub org_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub connector_id: Uuid,
    /// The project the connection serves. For a connector reached through a
    /// runtime peering, the member's project, not the connector's own.
    #[serde(default)]
    pub project_id: Option<Uuid>,
    /// `None` = org-owned (app subject); for a Slack workspace install this
    /// points at the workspace customer member.
    #[serde(default)]
    pub member_id: Option<Uuid>,
    /// The member who performed the grant, as distinct from `member_id`
    /// (whose credential this is). For `app` and `workspace` subjects those
    /// are never the same principal. `None` for grants made before the
    /// column existed.
    #[serde(default)]
    pub created_by_member_id: Option<Uuid>,
    /// Runtime group answering this connection's channels.
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    pub subject_type: ConnectionSubjectType,
    #[serde(default)]
    pub scopes_granted: Vec<String>,
    /// Application slug within a multi-application provider (Pipedream), as
    /// [`Connectors::list_apps`] returns it. `None` for a provider with a
    /// dedicated host.
    ///
    /// [`Connectors::list_apps`]: crate::resources::connectors::Connectors::list_apps
    #[serde(default)]
    pub provider_app: Option<String>,
    /// The provider's own identifier for the connected account. An opaque
    /// routing id, not a secret.
    #[serde(default)]
    pub provider_account_id: Option<String>,
    /// The provider's display name for the connected account.
    #[serde(default)]
    pub provider_account_name: Option<String>,
    /// When the grant was last confirmed usable.
    #[serde(default)]
    pub last_verified_at: Option<String>,
    pub status: ConnectionStatus,
    #[serde(default)]
    pub token_expires_at: Option<String>,
}

/// Filters supported by `GET /v1/connectors`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectorListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// Project slug or id. Needed only when the credential does not already
    /// name one, as with a CLI session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Parameters for `GET /v1/connectors/{connector_id}/connections`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectionListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// `POST /v1/connectors` body. `name`, `provider`, and `auth_mode` are
/// required; build with [`Self::new`] and struct-update syntax for the rest:
/// `ConnectorCreateParams { slug: Some("slack-support".into()), ..ConnectorCreateParams::new(...) }`.
///
/// `client_secret` and `signing_secret` are **write-only**: they are accepted
/// here (and on update) but never returned on any read. `issuer` drives
/// server-side OAuth discovery (endpoints resolved from `.well-known` when
/// omitted) and is not persisted.
///
/// Create upserts on `(project, slug)`: a repeat create with the same slug
/// replaces the live connector's configuration (name, environment, endpoints,
/// scopes, API hosts, client id, metadata, ...) and keeps its provider, auth
/// mode and stored secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorCreateParams {
    pub name: String,
    /// Provider slug, e.g. `"slack"`, `"gmail"`, `"stripe"`.
    pub provider: String,
    pub auth_mode: ConnectorAuthMode,
    /// Derived from `name` server-side when omitted. Unique per project; a
    /// repeat create with the same slug replaces that connector's
    /// configuration rather than adding a second one.
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
    /// Provider-specific settings. A `pipedream` connector requires
    /// `provider_workspace_id` — its Pipedream Connect project id
    /// (`proj_...`). `provider_environment` is derived server-side from
    /// `environment` and must not be sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// OAuth discovery: when set and the endpoints are omitted, the server
    /// resolves them from `.well-known` (and registers a client when neither
    /// `client_id` nor a discovered one is supplied — see
    /// [`Connectors::discover_oauth`]). Not persisted.
    ///
    /// [`Connectors::discover_oauth`]: crate::resources::connectors::Connectors::discover_oauth
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_server_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assertion_audience: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    /// Cedar entities for the recipe's policies; see [`PolicyEntity`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_entities: Option<Vec<PolicyEntity>>,
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
            person_server_url: None,
            application_id: None,
            assertion_audience: None,
            webhook_url: None,
            policy_entities: None,
        }
    }
}

/// `PATCH /v1/connectors/{id}` body — only these fields are mutable
/// (`environment`, `provider`, `auth_mode`, and `slug` are create-time facts).
/// Only provided fields change; omitting `client_secret` / `signing_secret`
/// means "unchanged", not "clear".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Replaces the connector's Cedar entities; omitted = unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_entities: Option<Vec<PolicyEntity>>,
}

/// A Cedar entity a recipe's policies read, in Cedar's JSON entity format:
/// e.g. a company and the seniority its business-class bookings need.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PolicyEntity {
    pub uid: EntityUid,
    #[serde(default)]
    pub attrs: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub parents: Vec<EntityUid>,
}

/// A Cedar entity reference, `{"type": "Company", "id": "acme"}`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct EntityUid {
    #[serde(rename = "type")]
    pub entity_type: String,
    pub id: String,
}

/// `POST /v1/connectors/{connector_id}/connections` body — registered mode,
/// where the caller supplies an already-minted provider token.
///
/// Deserializable too, so a body can be read from a JSON document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionCreateParams {
    pub access_token: String,
    /// Defaults to `App` server-side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_type: Option<ConnectionCreateSubjectType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes_granted: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Provider application slug (required for Pipedream connectors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Allow consent to a supported subset of the application's scopes.
    /// Defaults to `false`, matching Pipedream's default.
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_progressive_scopes: bool,
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
    /// Complete the grant into an MCP endpoint binding for one Runtime
    /// environment. Requires `runtime`. On a successful grant the control
    /// plane writes the binding in the same transaction as the connection, so
    /// the runtime is never authorized but unbound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<ConnectorAuthorizeBinding>,
}

/// The MCP endpoint binding an authorize completes into
/// ([`ConnectorAuthorizeParams::binding`]). Non-secret: the provider token
/// stays on the connection and is injected at the egress.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectorAuthorizeBinding {
    /// Runtime environment lane: `development` / `staging` / `production`.
    pub environment: String,
    /// The Recipe MCP server id this connector backs
    /// (`package.json#pi.mcp.servers[].id`): lowercase alphanumerics, `_` and
    /// `-`, starting and ending alphanumeric, at most 255 characters.
    pub mcp_server_id: String,
    /// Streamable-HTTP MCP resource URL (https, at most 1024 characters).
    pub url: String,
    /// Display label; defaults to `mcp_server_id` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Extra non-`Authorization` headers injected alongside the connection
    /// token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

impl ConnectorAuthorizeBinding {
    /// A binding with the three required fields set; combine with
    /// struct-update syntax for `name` / `headers`.
    pub fn new(
        environment: impl Into<String>,
        mcp_server_id: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        Self {
            environment: environment.into(),
            mcp_server_id: mcp_server_id.into(),
            url: url.into(),
            name: None,
            headers: None,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Parameters for `GET /v1/connectors/{id}/apps`, the provider catalogue search.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectorAppListParams {
    /// Free-text search over the catalogue (`q` on the wire).
    #[serde(rename = "q", skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Parameters for `GET /v1/connectors/custom/apps`, the open MCP registry
/// search. `query` is required (2–100 characters); build with [`Self::new`].
#[derive(Debug, Clone, Serialize)]
pub struct ConnectorCustomAppSearchParams {
    /// Free-text search over the registry (`q` on the wire).
    #[serde(rename = "q")]
    pub query: String,
    /// 1–50, default 20 server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

impl ConnectorCustomAppSearchParams {
    /// Search params for `query` with the server's default limit.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: None,
            filters: None,
        }
    }
}

/// An application listing — from a connector's provider catalogue
/// ([`Connectors::list_apps`]) or the open MCP registry
/// ([`Connectors::search_custom_apps`]).
///
/// [`Connectors::list_apps`]: crate::resources::connectors::Connectors::list_apps
/// [`Connectors::search_custom_apps`]: crate::resources::connectors::Connectors::search_custom_apps
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectorApp {
    pub slug: String,
    pub name: String,
    pub icon_url: Option<String>,
    pub description: Option<String>,
    /// How the provider authenticates; the open registry leaves it unset on
    /// most listings.
    pub auth_type: Option<String>,
    /// The listing's MCP server, where it has one. A Pipedream app is reached
    /// through Pipedream instead.
    #[serde(default)]
    pub mcp_url: Option<String>,
    /// The vendor's own documentation for this server, where the registry
    /// knows it.
    #[serde(default)]
    pub docs_url: Option<String>,
}

/// `POST /v1/connectors/discover-oauth` body.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectorOAuthDiscoveryParams {
    /// OAuth authorization-server issuer URL, or the URL of an MCP server /
    /// protected resource whose RFC 9728 metadata names its authorization
    /// server.
    pub issuer: String,
}

impl ConnectorOAuthDiscoveryParams {
    pub fn new(issuer: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
        }
    }
}

/// How the platform identified itself to a custom OAuth provider — mirrors
/// the CP `ClientRegistrationMethod` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientRegistrationMethod {
    /// The platform's Client ID Metadata Document URL is the client id; no
    /// state is kept on the provider.
    ClientIdMetadataDocument,
    /// A client registered with the provider through RFC 7591 dynamic
    /// registration.
    Dynamic,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl ClientRegistrationMethod {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ClientIdMetadataDocument => "client_id_metadata_document",
            Self::Dynamic => "dynamic",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for ClientRegistrationMethod {
    fn from(s: &str) -> Self {
        match s {
            "client_id_metadata_document" => Self::ClientIdMetadataDocument,
            "dynamic" => Self::Dynamic,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for ClientRegistrationMethod {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ClientRegistrationMethod {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Response of `POST /v1/connectors/discover-oauth` — the provider's OAuth
/// metadata plus, when the provider allows it, a client the platform
/// registered for itself.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectorOAuthDiscovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub token_endpoint_auth_methods_supported: Vec<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
    #[serde(default)]
    pub client_id_metadata_document_supported: bool,
    /// RFC 9728 resource identifier when discovery started at an MCP server;
    /// sent as the RFC 8707 `resource` parameter.
    #[serde(default)]
    pub resource: Option<String>,
    /// The callback this deployment sends. A client registered by hand must
    /// be given exactly this.
    pub redirect_uri: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    /// How `client_id` was obtained; `None` when the provider offers no
    /// automatic registration.
    #[serde(default)]
    pub client_registration: Option<ClientRegistrationMethod>,
}

/// Response of `POST /v1/oauth/connections/authorize` — a freshly minted
/// consent URL.
///
/// Each call writes a fresh **single-use** OAuth `state`: two calls give two
/// different URLs, and responses must never be cached — mint a new one per
/// hand-off. The `state` is deliberately not a response field and must never
/// be parsed out of the URL, logged, or surfaced.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectorAuthorization {
    pub authorize_url: String,
    /// Seconds the URL stays valid.
    pub expires_in: u64,
    pub expires_at: String,
}

/// Deterministic, non-PII envelope for a person-authorized action.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionMissionConstraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Opaque or hashed resource identifier; never raw PII.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_end: Option<String>,
    /// SHA-256 of the approved artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_binding: Option<String>,
}

/// `POST /v1/oauth/connections/token` options (the `connector_id` itself is
/// the method's first argument).
///
/// Deserializable too, so the options can be read from a JSON document;
/// `project` is not part of the body and is never read from one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionTokenParams {
    /// The project the connector belongs to, sent as the `project` query
    /// parameter. Leave it unset for a credential scoped to one project; a
    /// session or an org-wide token has to name it.
    #[serde(skip)]
    pub project: Option<StringOrUuid>,
    /// The exact connection to use. Unset, the platform resolves it from the
    /// authenticated subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<ConnectionBrokerSubjectType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_permissions: Option<ConnectionMissionConstraints>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionToken {
    pub token: String,
    pub token_type: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[serde_with::skip_serializing_none]
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

// ----- members (CP) ----------------------------------------------------------

/// What kind of principal a member is — mirrors the CP `MemberType` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberType {
    /// A person at the business, who signs in to the console.
    Business,
    /// A system agent member or application service account.
    Agent,
    /// An end customer, minted by an identity assertion, a federated IdP or a
    /// channel delivery.
    Customer,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl MemberType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Business => "business",
            Self::Agent => "agent",
            Self::Customer => "customer",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for MemberType {
    fn from(s: &str) -> Self {
        match s {
            "business" => Self::Business,
            "agent" => Self::Agent,
            "customer" => Self::Customer,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for MemberType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MemberType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// A member — the CP read model returned by every `/v1/members` route.
///
/// `tags` and `metadata` are both `key:value`-shaped labels but differ in
/// what they do: a tag selects the member for every share granted to it
/// ([`ShareCreate::granted_tag`]), while metadata grants nothing and only
/// narrows a list. A tag also still grants implicit read and write over every
/// file and task whose tags intersect the member's own, but that is being
/// retired in favour of tag shares.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Member {
    pub id: Uuid,
    pub org_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    /// Set for a business member; a customer member carries one only when a
    /// brokered IdP supplied it.
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// The identity a customer member is keyed on, tier-prefixed (e.g.
    /// `user:abc`); the IdP subject for a business member.
    #[serde(default)]
    pub external_user_id: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    /// `owner` / `admin` / `member` for a business member.
    pub role: String,
    pub member_type: MemberType,
    #[serde(default)]
    pub is_deactivated: bool,
    /// Selects this member for shares granted to any of these tags. (Implicit
    /// access to files and tasks whose tags intersect these still works but is
    /// being retired.)
    #[serde(default)]
    pub tags: Vec<String>,
    /// Customer-defined `key: value` labels. Grants nothing; filter on them
    /// with [`MemberListParams::metadata`].
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    /// The brokered IdP a customer member came through.
    #[serde(default)]
    pub application_idp_id: Option<Uuid>,
    /// The connector whose verified delivery minted a customer member.
    #[serde(default)]
    pub connector_id: Option<Uuid>,
    /// The org's own provider install a channel customer member came through.
    #[serde(default)]
    pub integration_id: Option<Uuid>,
    /// Server-derived: whether this is one of the fixed agents an API key or
    /// Application may act as.
    #[serde(default)]
    pub is_external_credential_agent: bool,
    /// Facts the recipe's Cedar policies read about this member (its
    /// principal's attributes); an `{"__entity": {...}}` value references a
    /// [`PolicyEntity`].
    #[serde(default)]
    pub policy_attributes: Option<BTreeMap<String, serde_json::Value>>,
}

/// Filters supported by `GET /v1/members`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MemberListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_type: Option<MemberType>,
    /// The customer members one connector's deliveries minted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_id: Option<Uuid>,
    /// The customer members one brokered IdP federated in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_idp_id: Option<Uuid>,
    /// Members carrying this tag, e.g. `customer:acme` — who a share granted
    /// to that tag reaches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Match members whose `metadata` holds every pair, each compared exactly
    /// against the string value (at most 16 pairs). Values may contain `:`;
    /// keys are letters, digits, `_` and `-`. Each pair is lowered to a
    /// repeated `metadata=key:value` query parameter; the API answers 422 on a
    /// malformed pair. An empty map sends no filter.
    #[serde(
        serialize_with = "serialize_metadata_filter",
        skip_serializing_if = "metadata_filter_is_empty"
    )]
    pub metadata: Option<HashMap<String, String>>,
    /// Resolve these member ids. Unions with `external_user_ids` rather than
    /// intersecting; sent as repeated `id` parameters.
    #[serde(rename = "id", skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Uuid>>,
    /// Resolve these tier-prefixed external keys, e.g. `user:abc`. Unions with
    /// `ids`; sent as repeated `external_user_id` parameters.
    #[serde(rename = "external_user_id", skip_serializing_if = "Option::is_none")]
    pub external_user_ids: Option<Vec<String>>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// `POST /v1/members` body: a business member invited by email, or a customer
/// member keyed by the organization's own id for them. Each variant carries the
/// fields its kind requires, and serializes with its `member_type`.
///
/// A customer lands on the same member a runner identity asserting its
/// `external_user_id` would mint, so its attributes are ready before its first
/// session. Customers, tags and policy attributes need `members:manage` (403
/// otherwise).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "member_type", rename_all = "snake_case")]
pub enum MemberCreateParams {
    Business(BusinessMemberCreate),
    Customer(CustomerMemberCreate),
}

impl MemberCreateParams {
    /// A business member invited by `email`, with a full `name` (first and
    /// last). Combine with [`BusinessMemberCreate`]'s struct-update syntax for
    /// the rest.
    pub fn business(email: impl Into<String>, name: impl Into<String>) -> Self {
        Self::Business(BusinessMemberCreate {
            email: email.into(),
            name: name.into(),
            role: None,
            tags: None,
            metadata: None,
            policy_attributes: None,
        })
    }

    /// A customer member keyed by `external_user_id`: `user:{id}` or
    /// `anon:{id}`.
    pub fn customer(external_user_id: impl Into<String>) -> Self {
        Self::Customer(CustomerMemberCreate {
            external_user_id: external_user_id.into(),
            ..CustomerMemberCreate::default()
        })
    }
}

/// A missing `member_type` is a business member, as the API reads it.
impl<'de> Deserialize<'de> for MemberCreateParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
        let kind = match value.remove("member_type") {
            None | Some(serde_json::Value::Null) => "business".to_owned(),
            Some(serde_json::Value::String(kind)) => kind,
            Some(other) => {
                return Err(serde::de::Error::custom(format!(
                    "member_type must be a string, got {other}"
                )))
            }
        };
        let value = serde_json::Value::Object(value);
        match kind.as_str() {
            "business" => serde_json::from_value(value)
                .map(Self::Business)
                .map_err(serde::de::Error::custom),
            "customer" => serde_json::from_value(value)
                .map(Self::Customer)
                .map_err(serde::de::Error::custom),
            other => Err(serde::de::Error::custom(format!(
                "member_type must be business or customer, got {other}"
            ))),
        }
    }
}

/// A person at the organization, invited by email.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessMemberCreate {
    pub email: String,
    /// A full name, first and last.
    pub name: String,
    /// Defaults to `"member"` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Tags grant access (through tag shares). Same opaque, exact-match
    /// validation as every other tag write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// `key: value` labels (keys letters, digits, `_` and `-`; at most 64
    /// entries; 422 otherwise). Grants nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    /// The member's Cedar attributes for the egress policy gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_attributes: Option<HashMap<String, serde_json::Value>>,
}

/// One of the organization's own end users, keyed by its id for them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerMemberCreate {
    /// `user:{id}` or `anon:{id}`, as a runner identity asserts it.
    pub external_user_id: String,
    /// Display only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Display only; a customer never signs in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    /// The member's Cedar attributes for the egress policy gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_attributes: Option<HashMap<String, serde_json::Value>>,
}

/// `PATCH /v1/members/{id}` body. Only provided fields change.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Replaces the tag list wholesale. `None` leaves tags untouched;
    /// `Some(vec![])` clears them. Adding a tag grants this member every share
    /// granted to it (and, until implicit tag access is retired, read and
    /// write over every file and task carrying it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Replaces the metadata map wholesale. `None` leaves it untouched;
    /// `Some(HashMap::new())` clears it. Grants nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    /// Replaces the member's Cedar attributes wholesale; `None` leaves them
    /// untouched. Setting them needs an org owner or admin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_attributes: Option<HashMap<String, serde_json::Value>>,
}

// ----- endpoints (CP) --------------------------------------------------------

/// What an [`Endpoint`] binds: a model provider, a plain upstream API, or an
/// MCP server. `Other` keeps a binding readable when the CP reports a kind
/// this SDK predates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointKind {
    Llm,
    Api,
    Mcp,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl EndpointKind {
    /// On-the-wire string form.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Llm => "llm",
            Self::Api => "api",
            Self::Mcp => "mcp",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for EndpointKind {
    fn from(s: &str) -> Self {
        match s {
            "llm" => Self::Llm,
            "api" => Self::Api,
            "mcp" => Self::Mcp,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for EndpointKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EndpointKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Non-secret metadata on an [`Endpoint`]. An MCP binding names its server
/// in `mcp_server_id`, and how it authenticates in `connector_id` (a stored
/// connection) or `provider_app` (one app on a multi-app host).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct EndpointMetadata {
    #[serde(default)]
    pub mcp_server_id: Option<String>,
    #[serde(default)]
    pub connector_id: Option<Uuid>,
    #[serde(default)]
    pub provider_app: Option<String>,
    /// Hosts the endpoint hands out URLs on that carry their own credential.
    #[serde(default)]
    pub extra_hosts: Vec<String>,
    #[serde(default)]
    pub auth_audience: Option<String>,
    /// Open by design: the server stores and returns metadata keys it does not
    /// declare (`extra="allow"`), so they are kept here.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

/// An endpoint binding (`/v1/endpoints`): a host the project's agents reach,
/// with the headers or identity the platform attaches on the way.
///
/// `headers` carries configured values with `${NAME}` credential references
/// left unresolved; a credential's secret value is never returned.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Endpoint {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    /// Hostname only: no scheme, path or port.
    pub host: String,
    /// Full base URL, when the binding overrides `https://{host}`.
    #[serde(default)]
    pub base_url: Option<String>,
    pub kind: EndpointKind,
    /// A `null` or missing object reads as empty.
    #[serde(default, deserialize_with = "null_as_default")]
    pub metadata: EndpointMetadata,
    #[serde(default)]
    pub assertion_claims: Option<BTreeMap<String, String>>,
    /// Set when the binding is scoped to one runtime; unset, it serves every
    /// runtime in the project.
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub verify_path: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub header_names: Vec<String>,
    #[serde(default)]
    pub credential_refs: Vec<String>,
    #[serde(default)]
    pub last_verified_at: Option<String>,
    /// The application that signs this endpoint's identity assertions.
    #[serde(default)]
    pub authorized_application_id: Option<Uuid>,
    pub created_by_member_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
}

/// Filters supported by `GET /v1/endpoints`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EndpointListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<EndpointKind>,
    /// Only bindings scoped to this runtime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    /// Only bindings scoped to this Runtime, by slug or runtime group id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<StringOrUuid>,
    /// Only bindings scoped to this environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// Only bindings whose `metadata.mcp_server_id` is this server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_server_id: Option<String>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Body of `POST /v1/endpoints`, which upserts by host, runtime scope and
/// environment.
///
/// Header values are plaintext on the wire and stored encrypted; a `${NAME}`
/// in one is a credential reference resolved at egress. An error from the
/// call has every header value replaced by `[redacted]`, since a validation
/// error can echo the request.
#[cfg(feature = "authoring")]
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EndpointCreateParams {
    /// Display label; not unique.
    pub name: String,
    /// Hostname only: no scheme, path or port.
    pub host: String,
    /// Full base URL overriding `https://{host}`; its host must be `host`.
    #[serde(default)]
    pub base_url: Option<String>,
    /// `llm` when unset.
    #[serde(default)]
    pub kind: Option<EndpointKind>,
    #[serde(default)]
    pub metadata: Option<EndpointMetadata>,
    /// Header name to value. May be empty, for a public or allowlist-only
    /// host.
    #[serde(default)]
    pub headers: BTreeMap<String, Secret>,
    /// Extra claims for the identity assertion; not with a stored
    /// `Authorization` header.
    #[serde(default)]
    pub assertion_claims: Option<BTreeMap<String, String>>,
    /// Only this runtime may use the endpoint. At most one of `runtime_id`
    /// and `runtime_group_id`.
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default)]
    pub environment: Option<Lane>,
    /// A path the save first `GET`s, and which must answer 2xx (else 422
    /// `endpoint_verify_failed`).
    #[serde(default)]
    pub verify_path: Option<String>,
    /// The application that signs this endpoint's identity assertions; not
    /// with a stored `Authorization` header.
    #[serde(default)]
    pub authorized_application_id: Option<Uuid>,
}

#[cfg(feature = "authoring")]
impl EndpointCreateParams {
    pub fn new(name: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            host: host.into(),
            ..Default::default()
        }
    }
}

/// Body of `PATCH /v1/endpoints/{id}`. A field left `None` is not sent and
/// stays unchanged. `runtime_id`, `runtime_group_id` and `verify_path` cannot
/// be cleared; delete and recreate the endpoint instead.
///
/// Changing `headers`, `host`, `base_url` or `verify_path` on an endpoint with
/// a `verify_path` verifies it again first. Header values are redacted from
/// an error as for [`EndpointCreateParams`].
#[cfg(feature = "authoring")]
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EndpointUpdateParams {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub kind: Option<EndpointKind>,
    #[serde(default)]
    pub metadata: Option<EndpointMetadata>,
    /// Replaces the headers wholesale.
    #[serde(default)]
    pub headers: Option<BTreeMap<String, Secret>>,
    #[serde(default)]
    pub assertion_claims: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    /// `Some(None)` sends `null`: every environment.
    #[serde(
        default,
        with = "serde_with::rust::double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub environment: Option<Option<Lane>>,
    #[serde(default)]
    pub verify_path: Option<String>,
    /// Cannot be cleared.
    #[serde(default)]
    pub authorized_application_id: Option<Uuid>,
}

// ----- projects (CP) ---------------------------------------------------------

/// A project, as `GET /v1/projects` and `GET /v1/projects/{project}` return
/// it.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Project {
    pub id: Uuid,
    pub org_id: Uuid,
    /// The Data Plane deployment the project lives on.
    pub deployment_id: Uuid,
    pub name: String,
    /// The stable identifier the CLI and dashboard URLs use. The CP declares
    /// it nullable; a project created before slugs existed may have none.
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub settings: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub created_by_member_id: Option<Uuid>,
}

/// Filters supported by `GET /v1/projects`. The route lists the projects of
/// the credential's org.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectListParams {
    /// Only the project with this slug or id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

/// A runtime peering: a source project offering one of its runtime groups'
/// configuration to other projects, as `GET /v1/projects/{project}/peerings`
/// and `GET /v1/projects/{project}/peerings/{peering_id}` return it.
///
/// Read-only here. Creating, changing or removing a peering needs
/// `projects:write` held by a person, which no API key has; it is done from
/// the web app.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimePeering {
    pub id: Uuid,
    /// The source project's org.
    pub org_id: Uuid,
    /// The source project, which offers the runtime group.
    pub project_id: Uuid,
    /// The runtime group whose configuration is offered.
    pub runtime_group_id: Uuid,
    /// The org admitted, when the grant names one.
    #[serde(default)]
    pub granted_org_id: Option<Uuid>,
    /// The project admitted, when the grant names one.
    #[serde(default)]
    pub granted_project_id: Option<Uuid>,
    /// The deployment admitted, when the grant names one.
    #[serde(default)]
    pub granted_deployment_id: Option<Uuid>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub created_by_member_id: Option<Uuid>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub deleted_at: Option<String>,
}

/// Filters supported by `GET /v1/projects/{project}/peerings`.
///
/// By default the list is the peerings the project offers. Set `granted` to
/// list the peerings other projects grant to it instead; the server answers
/// 422 when `runtime` is set with it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimePeeringListParams {
    /// Only the peerings of this runtime group, by slug or id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<StringOrUuid>,
    /// `true` lists the peerings other projects grant to this one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted: Option<bool>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

/// `POST /v1/projects` body. Needs `projects:write`, which only an owner's or
/// admin's member session or access token carries.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
pub struct ProjectCreateParams {
    pub name: String,
    /// The Data Plane deployment the project lives on.
    pub deployment_id: Uuid,
    /// Derived from `name` when `None`.
    pub slug: Option<String>,
    pub description: Option<String>,
    pub display_id_prefixes: Option<DisplayIdPrefixes>,
    pub settings: Option<BTreeMap<String, serde_json::Value>>,
    /// Seed the Ark runtime. `None`: an organization's first project takes
    /// it and later ones do not.
    pub include_ark: Option<bool>,
}

impl ProjectCreateParams {
    pub fn new(name: impl Into<String>, deployment_id: Uuid) -> Self {
        Self {
            name: name.into(),
            deployment_id,
            slug: None,
            description: None,
            display_id_prefixes: None,
            settings: None,
            include_ark: None,
        }
    }
}

/// `PATCH /v1/projects/{project}` body. Only the fields set are sent; a
/// request that sets none answers 400.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectUpdateParams {
    pub name: Option<String>,
    pub description: Option<String>,
    pub display_id_prefixes: Option<DisplayIdPrefixes>,
    pub settings: Option<BTreeMap<String, serde_json::Value>>,
}

/// The prefixes of a project's display ids (`I-1`, `R-1`, ...), each at
/// most five characters. The server fills a prefix left out with its
/// default.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize)]
pub struct DisplayIdPrefixes {
    pub issue: Option<String>,
    pub report: Option<String>,
    pub observation: Option<String>,
    pub task: Option<String>,
}

/// `GET /v1/organizations/me` — the organization the credential belongs to.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    /// The identity provider's id for the organization.
    #[serde(default)]
    pub external_org_id: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    /// Whether new hosted repositories are allowed.
    #[serde(default = "default_true")]
    pub hosted_git: bool,
}

/// `POST /v1/tokens` body: a Data Plane token for one project, minted from
/// a member session or access token (`tokens:write`).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
pub struct TokenCreateParams {
    /// The project, by slug or id.
    pub project: StringOrUuid,
    /// Lifetime in hours, 1 to 720.
    pub expires_hours: Option<u32>,
    /// Lifetime in minutes, 1 to 60, for a short-lived token.
    pub expires_minutes: Option<u32>,
    /// Also mint a refresh token for the OAuth `/token` endpoint.
    pub include_refresh_token: Option<bool>,
    /// The environment lane the token belongs to; `production` when `None`.
    /// Only a human session may name a lane other than its own credential's.
    pub environment: Option<String>,
}

impl TokenCreateParams {
    pub fn new(project: impl Into<StringOrUuid>) -> Self {
        Self {
            project: project.into(),
            expires_hours: None,
            expires_minutes: None,
            include_refresh_token: None,
            environment: None,
        }
    }
}

/// What `POST /v1/tokens` answers: a Data Plane JWT for one project.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DataPlaneToken {
    pub access_token: String,
    #[serde(default = "default_bearer")]
    pub token_type: String,
    pub expires_at: String,
    pub project_id: Uuid,
    pub org_id: Uuid,
    /// Present only when `include_refresh_token` was set.
    #[serde(default)]
    pub refresh_token: Option<String>,
}

fn default_bearer() -> String {
    "bearer".to_string()
}

// ----- automations (DP) ------------------------------------------------------

/// A string-backed wire enum with an `Other(String)` fallback, so a value the
/// Data Plane adds after this SDK was built still decodes.
macro_rules! open_wire_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $wire:literal, )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
            /// Forward-compatible escape hatch.
            Other(String),
        }

        impl $name {
            /// The on-the-wire value.
            pub fn as_str(&self) -> &str {
                match self {
                    $( Self::$variant => $wire, )+
                    Self::Other(value) => value,
                }
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                match value {
                    $( $wire => Self::$variant, )+
                    other => Self::Other(other.to_string()),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let value = String::deserialize(d)?;
                Ok(Self::from(value.as_str()))
            }
        }
    };
}

open_wire_enum! {
    /// How an automation is triggered. Immutable after create.
    pub enum AutomationTriggerType {
        /// Fires on `cron_schedule` (or `metadata.cron_schedules`).
        Cron => "cron",
        /// Runs by hand, or once at a client-set `next_trigger_at` (a one-off
        /// reminder).
        Manual => "manual",
    }
}

open_wire_enum! {
    /// A platform automation kind. An automation a person created has no kind
    /// (`Automation::kind` is `None`); any value marks a platform automation.
    /// Immutable after create.
    pub enum AutomationKind {
        /// The project's default check-in. Runs as an agent task, so it carries
        /// a `prompt`.
        ProjectCheckIn => "project_check_in",
        /// The project's synthesis job. Project-wide: takes no
        /// `runtime_group_id`.
        ObservationSynthesis => "observation_synthesis",
        /// The clustering job for one runtime group.
        ObservationClustering => "observation_clustering",
    }
}

open_wire_enum! {
    /// A built-in condition evaluated before an automation runs.
    pub enum AutomationConditionType {
        HasNewTasksSinceLastRun => "has_new_tasks_since_last_run",
        LastRunIssuesResolved => "last_run_issues_resolved",
        NoLiveTaskForAutomation => "no_live_task_for_automation",
    }
}

open_wire_enum! {
    /// Outcome of one automation execution attempt.
    pub enum AutomationExecutionStatus {
        Triggered => "triggered",
        Cancelled => "cancelled",
        Failed => "failed",
        Skipped => "skipped",
    }
}

open_wire_enum! {
    /// Why a trigger ran nothing (`introspection.automation.skipped`).
    pub enum AutomationSkipReason {
        AutomationDeleted => "automation_deleted",
        ExecutionBlocked => "execution_blocked",
        ConditionsNotMet => "conditions_not_met",
        NoProductionRuntime => "no_production_runtime",
        SlackNotConfigured => "slack_not_configured",
        TargetTaskDeleted => "target_task_deleted",
        TargetTaskArchived => "target_task_archived",
        TargetTaskUnavailable => "target_task_unavailable",
        TargetTaskRefused => "target_task_refused",
        /// The target task stayed mid-turn through every retry.
        TargetBusy => "target_busy",
    }
}

/// One condition stored in an automation's metadata.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationCondition {
    #[serde(rename = "type")]
    pub condition_type: AutomationConditionType,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
}

impl AutomationCondition {
    /// A condition with no runtime group.
    pub fn new(condition_type: AutomationConditionType) -> Self {
        Self {
            condition_type,
            runtime_group_id: None,
        }
    }
}

/// The typed shape of an automation's `metadata` object.
///
/// The runtime group is the top-level `runtime_group_id`; the API rejects it
/// inside metadata, so there is no field for it here. Empty lists are not
/// sent. On update the whole object replaces the stored one.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutomationMetadata {
    /// Repositories a task-run automation clones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<TaskRepoRequest>,
    /// Several cron expressions; `cron_schedule` is the single-schedule form.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cron_schedules: Vec<String>,
    /// IANA time zone the cron schedules are evaluated in.
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<AutomationCondition>,
    /// Every other key, kept as sent: metadata is free-form, and a key this
    /// type does not name survives a read-modify-write.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

/// A stored automation: a prompt run as an agent task, or platform work when
/// `kind` is set. The read model of every `/v1/automations` route.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Automation {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub trigger_type: AutomationTriggerType,
    #[serde(default)]
    pub cron_schedule: Option<String>,
    /// `None` for an automation a person created; a platform automation
    /// carries its kind.
    #[serde(default)]
    pub kind: Option<AutomationKind>,
    #[serde(default)]
    pub prompt: Option<String>,
    /// The runtime group it runs on (or clusters); `None` for
    /// [`AutomationKind::ObservationSynthesis`].
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    /// The existing task each firing posts the prompt into; `None` creates a
    /// task per firing.
    #[serde(default)]
    pub task_id: Option<Uuid>,
    /// The person who created it; `None` on a platform automation.
    #[serde(default)]
    pub created_by_member_id: Option<Uuid>,
    /// Why the scheduler will not run this automation, when it will not.
    #[serde(default)]
    pub execution_blocked_reason: Option<String>,
    /// Whether the caller may change it.
    #[serde(default)]
    pub can_manage: bool,
    /// `operator` for one that runs as a task (a person's automation or
    /// [`AutomationKind::ProjectCheckIn`]), `None` for one that runs on the
    /// platform's own workers.
    #[serde(default)]
    pub owner_role: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Open-ended as stored; read the known fields with
    /// [`Self::typed_metadata`].
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub last_triggered_at: Option<String>,
    /// The next slot: derived from the schedule for a cron automation,
    /// client-set for a one-off manual automation and cleared once that slot
    /// fires.
    #[serde(default)]
    pub next_trigger_at: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Automation {
    /// `metadata` decoded into its typed shape, or `None` when it is absent
    /// or does not fit.
    pub fn typed_metadata(&self) -> Option<AutomationMetadata> {
        let metadata = self.metadata.as_ref()?;
        let object: serde_json::Map<String, serde_json::Value> = metadata
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        serde_json::from_value(serde_json::Value::Object(object)).ok()
    }
}

/// `POST /v1/automations` body. Build with [`Self::new`] and struct-update
/// syntax for the rest.
///
/// A one-off reminder is a [`AutomationTriggerType::Manual`] automation with a
/// future `next_trigger_at`; a cron automation derives its own slots and must
/// not send one.
#[derive(Debug, Clone, Serialize)]
pub struct AutomationCreateParams {
    pub name: String,
    pub trigger_type: AutomationTriggerType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Required for a cron automation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_schedule: Option<String>,
    /// Omit for a person's own automation, which then needs `prompt`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<AutomationKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Required unless `kind` is [`AutomationKind::ObservationSynthesis`],
    /// which must omit it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    /// An existing task each firing posts the prompt into. Only an automation
    /// without a `kind` can set it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    /// RFC 3339 timestamp with an offset: a manual automation's one-off slot,
    /// which must be in the future.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_trigger_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AutomationMetadata>,
    /// Defaults to `true` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

impl AutomationCreateParams {
    /// Create params with the required fields set and every optional field
    /// unset. Combine with struct-update syntax.
    pub fn new(name: impl Into<String>, trigger_type: AutomationTriggerType) -> Self {
        Self {
            name: name.into(),
            trigger_type,
            description: None,
            cron_schedule: None,
            kind: None,
            prompt: None,
            runtime_group_id: None,
            task_id: None,
            next_trigger_at: None,
            metadata: None,
            enabled: None,
        }
    }
}

/// `PATCH /v1/automations/{id}` body. Only set fields are sent and `None`
/// leaves a field as it is, so nothing can be cleared. `kind` and
/// `trigger_type` are immutable; `metadata` replaces the stored object.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AutomationUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_schedule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Moves an automation that runs as a task to another runtime group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    /// Schedules, moves or re-arms a manual automation's one-off slot (RFC
    /// 3339 with an offset, in the future).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_trigger_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AutomationMetadata>,
    /// `false` pauses and keeps the slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Filters supported by `GET /v1/automations`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AutomationListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<AutomationKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// `true`: only automations with a next slot; `false`: only those without.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled: Option<bool>,
    /// Only automations that post into this task. Not served until
    /// introspection-cloud#3137 ships; a server without it ignores the
    /// parameter and returns the unfiltered list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string. On a collision the passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// `POST /v1/automations/{id}/trigger` response. A `skipped` status carries
/// its `reason` and records no event.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AutomationTriggerResponse {
    pub status: AutomationExecutionStatus,
    pub automation_id: Uuid,
    /// The task created, or posted into.
    #[serde(default)]
    pub task_id: Option<Uuid>,
    #[serde(default)]
    pub reason: Option<String>,
}

// ----- member connections (DP) -----------------------------------------------

/// An app (Gmail, Slack, …) a member connected for themself, which the agent
/// acts with in that member's sessions. Distinct from a connector's
/// [`Connection`].
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemberConnection {
    pub id: Uuid,
    /// A member's own grant, or the project's own account on a connector.
    #[serde(default)]
    pub owner: ConnectionOwner,
    /// The member whose grant this is; `None` for the project's own.
    #[serde(default)]
    pub member_id: Option<Uuid>,
    /// The connector the connection is on, as the project sees it.
    #[serde(default)]
    pub connector: Option<ConnectorRef>,
    #[serde(default)]
    pub subject_type: Option<ConnectionSubjectType>,
    /// Provider application slug, e.g. `"gmail"`, or the connector's slug
    /// for a single-app connector.
    pub app: String,
    /// The provider's id for the account, e.g. Pipedream's `apn_...`.
    #[serde(default)]
    pub provider_account_id: Option<String>,
    /// The provider account connected, when the provider names one.
    #[serde(default)]
    pub account_name: Option<String>,
    /// Whether the grant was usable when last verified; `false` when the
    /// connection needs reconnecting.
    pub healthy: bool,
    #[serde(default)]
    pub status: Option<ConnectionStatus>,
    /// When the grant was last confirmed usable.
    #[serde(default)]
    pub last_verified_at: Option<String>,
    /// The member who performed the grant.
    #[serde(default)]
    pub created_by_member_id: Option<Uuid>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

open_wire_enum! {
    /// Who owns a [`MemberConnection`].
    #[derive(Default)]
    pub enum ConnectionOwner {
        /// A member's own grant.
        #[default]
        Member => "member",
        /// The project's own account on a connector.
        Project => "project",
    }
}

/// The connector a [`MemberConnection`] is on, as the project sees it.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectorRef {
    pub id: Uuid,
    pub slug: String,
    pub provider: String,
    /// The peering the project reaches the connector through; `None` for
    /// the project's own connector.
    #[serde(default)]
    pub peering: Option<String>,
}

/// Filters supported by `GET /v1/connections`. A caller who is not an
/// administrator only ever gets their own connections, whatever `member_id`
/// says.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MemberConnectionListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
    /// Provider application slug, e.g. `"gmail"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string. On a collision the passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Body of `POST /v1/connections`. Build with [`MemberConnectionCreate::new`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct MemberConnectionCreate {
    /// Provider application slug, e.g. `"gmail"`.
    pub app: String,
    /// Runtime slug or runtime group id whose sessions use the connection.
    /// Required on the client; on a runner, `None` means the runner's own
    /// runtime group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<StringOrUuid>,
}

impl MemberConnectionCreate {
    pub fn new(app: impl Into<String>) -> Self {
        Self {
            app: app.into(),
            runtime: None,
        }
    }
}

/// Response of `POST /v1/connections`: a connect page for one app, which ends
/// on a page saying the app is connected. Mint a new one per hand-off.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectPage {
    pub authorize_url: String,
    /// Seconds the URL stays valid.
    pub expires_in: u64,
    #[serde(default)]
    pub expires_at: Option<String>,
}

// ----- project administration (CP) ------------------------------------------
//
// API keys, Applications, credentials, variables, integrations, judges,
// projects and the signed-in member (`/v1/oidc/me`). Each route needs an org
// credential carrying its `*:read` / `*:write` / `*:delete` scope; a project
// API key carries none of them.

/// A plaintext secret the API returns once or takes on write. `Debug` prints
/// `<redacted>`, so logging a request or response never leaks it; read it with
/// [`Secret::expose`].
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The plaintext.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// The `?project=` selector most project-scoped Control Plane routes take: a
/// slug or id, needed only when the credential does not already name one.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectSelector {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
}

impl ProjectSelector {
    pub fn new(project: Option<StringOrUuid>) -> Self {
        Self { project }
    }
}

// api keys

open_wire_enum! {
    /// What a project API key may do beyond the runner set.
    pub enum ApiKeyCapability {
        Runtimes => "runtimes",
        Evals => "evals",
    }
}

open_wire_enum! {
    pub enum ApiKeyStatus {
        Active => "active",
        /// Deleted with a grace period: still valid until `expires_at`.
        Rotating => "rotating",
        Expired => "expired",
        Revoked => "revoked",
    }
}

/// A project API key. The plaintext key is never readable after create; see
/// [`ApiKeyCreated`].
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    #[serde(default)]
    pub name: Option<String>,
    pub environment: Lane,
    #[serde(default)]
    pub allow: Vec<ApiKeyCapability>,
    pub scope: String,
    /// The agent member the key acts as, when it is not the default one.
    #[serde(default)]
    pub member_id: Option<Uuid>,
    pub key_prefix: String,
    pub key_suffix: String,
    #[serde(default)]
    pub redacted_key: Option<String>,
    pub status: ApiKeyStatus,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
    /// Set once [`crate::ApiKeys::acknowledge`] records the key as saved.
    #[serde(default)]
    pub acknowledged_at: Option<String>,
    pub key_display: String,
    pub prefix: String,
}

/// Response of `POST /v1/api-keys`: the key, plus its plaintext — the only
/// time the API returns it.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiKeyCreated {
    #[serde(flatten)]
    pub key: ApiKey,
    #[serde(default)]
    pub api_key: Option<Secret>,
}

/// Filters supported by `GET /v1/api-keys`. Not paginated.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApiKeyListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    /// Only keys close to expiry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiring: Option<bool>,
}

/// Body of `POST /v1/api-keys`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApiKeyCreateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Defaults to `production`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Lane>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow: Option<Vec<ApiKeyCapability>>,
    /// Act as this external-credential agent rather than the default one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Body of `PATCH /v1/api-keys/{id}`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApiKeyUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow: Option<Vec<ApiKeyCapability>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
}

/// Query of `DELETE /v1/api-keys/{id}`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApiKeyDeleteParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    /// Keep the key working this many more seconds (a rotation); `None` or `0`
    /// revokes it at once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
}

/// Response of `POST /v1/api-keys/{id}/acknowledge`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiKeyAcknowledgement {
    pub acknowledged: bool,
}

// applications

open_wire_enum! {
    pub enum ApplicationType {
        Native => "native",
        Spa => "spa",
        Jwks => "jwks",
        ServiceAccount => "service_account",
    }
}

open_wire_enum! {
    pub enum ApplicationGrant {
        AuthorizationCode => "authorization_code",
        RefreshToken => "refresh_token",
        ClientCredentials => "client_credentials",
        DeviceCode => "device_code",
        EmailCode => "email_code",
    }
}

/// An OAuth client registered in a project.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Application {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    pub application_type: ApplicationType,
    pub environment: Lane,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub allowed_scopes: Vec<String>,
    pub client_id: String,
    #[serde(default)]
    pub agent_member_id: Option<Uuid>,
    #[serde(default)]
    pub allowed_grants: Vec<ApplicationGrant>,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub revoked_at: Option<String>,
    pub created_by_member_id: Uuid,
}

/// Filters supported by `GET /v1/applications`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplicationListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
}

/// Body of `POST /v1/applications`.
#[derive(Debug, Clone, Serialize)]
pub struct ApplicationCreateParams {
    pub name: String,
    pub application_type: ApplicationType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Lane>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uris: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_origins: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_member_id: Option<Uuid>,
}

impl ApplicationCreateParams {
    pub fn new(name: impl Into<String>, application_type: ApplicationType) -> Self {
        Self {
            name: name.into(),
            application_type,
            environment: None,
            redirect_uris: None,
            allowed_origins: None,
            allowed_scopes: None,
            agent_member_id: None,
        }
    }
}

/// Body of `PATCH /v1/applications/{id}`. A list replaces the whole value.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplicationUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uris: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_origins: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_member_id: Option<Uuid>,
}

open_wire_enum! {
    pub enum ApplicationIdpProvider {
        Auth0 => "auth0",
        Okta => "okta",
        Supabase => "supabase",
        Neon => "neon",
        Generic => "generic",
        Zitadel => "zitadel",
    }
}

open_wire_enum! {
    pub enum ApplicationIdpStatus {
        Pending => "pending",
        Active => "active",
        Failed => "failed",
    }
}

/// An identity provider brokered into an Application, whose users sign in as
/// customer members.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApplicationIdp {
    pub id: Uuid,
    pub org_id: Uuid,
    pub application_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub idp_provider: ApplicationIdpProvider,
    pub issuer: String,
    pub client_id: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub external_org_id: Option<String>,
    #[serde(default)]
    pub external_idp_id: Option<String>,
    #[serde(default)]
    pub audience_client_id: Option<String>,
    pub status: ApplicationIdpStatus,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub disabled_at: Option<String>,
    #[serde(default)]
    pub client_secret: Option<Secret>,
    pub is_active: bool,
}

/// Body of `POST /v1/applications/{id}/idps`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplicationIdpCreateParams {
    /// Defaults to `generic`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idp_provider: Option<ApplicationIdpProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<Secret>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience_client_id: Option<String>,
}

open_wire_enum! {
    pub enum ApplicationSecretStatus {
        Active => "active",
        Disabled => "disabled",
        Expired => "expired",
    }
}

/// A client secret of a confidential Application. `secret` is set only on the
/// create response.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApplicationSecret {
    pub id: Uuid,
    pub org_id: Uuid,
    pub application_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub secret_prefix: String,
    pub secret_suffix: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub disabled_at: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
    pub created_by_member_id: Uuid,
    #[serde(default)]
    pub secret: Option<Secret>,
    pub status: ApplicationSecretStatus,
}

/// Body of `POST /v1/applications/{id}/secrets`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplicationSecretCreateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A signing key of a `jwks` Application: the public half, published at the
/// Application's JWKS URL.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApplicationKey {
    pub id: Uuid,
    pub org_id: Uuid,
    pub application_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub kid: String,
    pub algorithm: String,
    pub public_key: String,
    #[serde(default)]
    pub created_by_member_id: Option<Uuid>,
}

// credentials and variables

/// A runtime credential. The secret is write-only: reads report only whether
/// one is set and a display hint.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Credential {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    /// `None` applies to every lane.
    #[serde(default)]
    pub environment: Option<Lane>,
    #[serde(default)]
    pub has_secret: bool,
    #[serde(default)]
    pub secret_display: Option<String>,
    #[serde(default)]
    pub secret_set_at: Option<String>,
    pub created_by_member_id: Uuid,
}

/// Filters supported by `GET /v1/credentials` and `GET /v1/variables`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeSettingListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    /// A runtime slug.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Lane>,
}

/// Body of `POST /v1/credentials`. With neither `runtime_id` nor
/// `runtime_group_id` the credential is project-wide.
#[derive(Debug, Clone, Serialize)]
pub struct CredentialCreateParams {
    pub name: String,
    pub secret: Secret,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Lane>,
}

impl CredentialCreateParams {
    pub fn new(name: impl Into<String>, secret: impl Into<Secret>) -> Self {
        Self {
            name: name.into(),
            secret: secret.into(),
            runtime_id: None,
            runtime_group_id: None,
            environment: None,
        }
    }
}

/// Body of `PATCH /v1/credentials/{id}`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CredentialUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<Secret>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    /// `Some(Some(_))` moves it to one lane and `Some(None)` sends `null`,
    /// which makes it shared across every lane.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Option<Lane>>,
}

open_wire_enum! {
    pub enum VariableKind {
        /// A plain value, returned on read.
        Value => "value",
        /// A reference to a credential by name.
        Credential => "credential",
    }
}

/// A runtime environment variable.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Variable {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    pub kind: VariableKind,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default)]
    pub environment: Option<Lane>,
    pub created_by_member_id: Uuid,
}

/// Body of `POST /v1/variables`.
#[derive(Debug, Clone, Serialize)]
pub struct VariableCreateParams {
    pub name: String,
    /// Defaults to `value`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<VariableKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Lane>,
}

impl VariableCreateParams {
    /// A plain `value` variable.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: None,
            value: Some(value.into()),
            runtime_id: None,
            runtime_group_id: None,
            environment: None,
        }
    }
}

/// Body of `PATCH /v1/variables/{id}`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct VariableUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<VariableKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    /// `Some(Some(_))` moves it to one lane and `Some(None)` sends `null`,
    /// which makes it shared across every lane.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Option<Lane>>,
}

// integrations

open_wire_enum! {
    pub enum IntegrationType {
        Github => "github",
        Slack => "slack",
        Linear => "linear",
        Jira => "jira",
        Notion => "notion",
        Vercel => "vercel",
        Custom => "custom",
    }
}

open_wire_enum! {
    pub enum IntegrationStatus {
        Pending => "pending",
        Active => "active",
        Error => "error",
        Disabled => "disabled",
    }
}

/// The org's own install of a provider (a GitHub App installation, a Slack
/// workspace). At most one live integration per type per org.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Integration {
    pub id: Uuid,
    pub org_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    #[serde(rename = "type")]
    pub integration_type: IntegrationType,
    #[serde(default)]
    pub name: Option<String>,
    /// The provider's id for the install, set once it is activated.
    #[serde(default)]
    pub external_id: Option<String>,
    pub status: IntegrationStatus,
    pub created_by_member_id: Uuid,
    #[serde(default)]
    pub token_expires_at: Option<String>,
    #[serde(default)]
    pub data: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

/// Body of `POST /v1/integrations`. The row is always created `pending`; the
/// provider's activation flow then sets `external_id` with an update.
#[derive(Debug, Clone, Serialize)]
pub struct IntegrationCreateParams {
    #[serde(rename = "type")]
    pub integration_type: IntegrationType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

impl IntegrationCreateParams {
    pub fn new(integration_type: IntegrationType) -> Self {
        Self {
            integration_type,
            name: None,
            external_id: None,
        }
    }
}

/// Body of `PATCH /v1/integrations/{id}`. `code` and `redirect_uri` complete
/// a provider's OAuth activation.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IntegrationUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IntegrationStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<Secret>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
    /// Only `false` is accepted: turns Slack interactivity off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_interactivity_enabled: Option<bool>,
}

// judges

/// A judge a Recipe declares. Its definition is authored in the Recipe; only
/// `enabled` and `sample_rate` are operational and changeable here.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Judge {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub runtime_group_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub enabled: bool,
    pub sample_rate: f64,
    pub source: String,
    #[serde(default)]
    pub repository_id: Option<Uuid>,
    #[serde(default)]
    pub recipe_git_commit_sha: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
    pub definition_hash: String,
    #[serde(default)]
    pub created_by_member_id: Option<Uuid>,
}

/// Filters supported by `GET /v1/judges`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct JudgeListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<StringOrUuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
}

/// Body of `PATCH /v1/judges/{id}`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct JudgeUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// The fraction of conversations judged, `0.0`–`1.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<f64>,
}

// signed-in member

/// A feature flag's resolved value.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FeatureFlagValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

/// Response of `GET /v1/oidc/me`: who the credential is.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Me {
    #[serde(default = "default_true")]
    pub authenticated: bool,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub member_id: Option<Uuid>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
    #[serde(default)]
    pub feature_flags: BTreeMap<String, FeatureFlagValue>,
    /// Set while an operator impersonates this member.
    #[serde(default)]
    pub impersonating_member_id: Option<Uuid>,
    #[serde(default)]
    pub impersonator_email: Option<String>,
}

// ----- images (DP) -----------------------------------------------------------

open_wire_enum! {
    /// What produced an image.
    pub enum ImageKind {
        /// A Recipe bake.
        Runtime => "runtime",
        /// A caller's `POST /v1/images`.
        Service => "service",
    }
}

open_wire_enum! {
    pub enum ImageStatus {
        Queued => "queued",
        Building => "building",
        Ready => "ready",
        Failed => "failed",
        Cancelled => "cancelled",
        Evicted => "evicted",
    }
}

open_wire_enum! {
    pub enum ImageScanStatus {
        Pending => "pending",
        Passed => "passed",
        Findings => "findings",
        Failed => "failed",
        Skipped => "skipped",
    }
}

open_wire_enum! {
    pub enum ImageSourceKind {
        Context => "context",
        Mirror => "mirror",
        Bake => "bake",
    }
}

/// An image in the plane's registry, which is what an eval task's compose
/// services run as.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Image {
    pub id: Uuid,
    pub kind: ImageKind,
    pub name: String,
    /// What the create dedupes on: a repeat create of the same content
    /// returns this row.
    pub content_id: String,
    pub source_kind: ImageSourceKind,
    pub source: BTreeMap<String, serde_json::Value>,
    pub build_platform: String,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub repository_id: Option<Uuid>,
    #[serde(default)]
    pub git_sha: Option<String>,
    #[serde(default)]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub task_id: Option<Uuid>,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub base_image_digest: Option<String>,
    #[serde(default)]
    pub base_image_tag: Option<String>,
    #[serde(default)]
    pub external_artifacts: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub external_image_name: Option<String>,
    pub status: ImageStatus,
    #[serde(default)]
    pub image_ref: Option<String>,
    #[serde(default)]
    pub image_digest: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub scan_status: Option<ImageScanStatus>,
    #[serde(default)]
    pub scan_started_at: Option<String>,
    #[serde(default)]
    pub scanned_at: Option<String>,
    #[serde(default)]
    pub scan_db_built_at: Option<String>,
    #[serde(default)]
    pub scan_summary: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub scan_report_file_id: Option<Uuid>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub evicted_at: Option<String>,
    #[serde(default)]
    pub retired_at: Option<String>,
}

impl Image {
    /// The digest-pinned reference a compose service names, once ready.
    pub fn pull_reference(&self) -> Option<String> {
        match (&self.image_ref, &self.image_digest) {
            (Some(image_ref), Some(digest)) => Some(format!("{image_ref}@{digest}")),
            _ => None,
        }
    }
}

/// Filters supported by `GET /v1/images`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ImageListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ImageKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ImageStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Where an image's bytes come from.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSource {
    /// Build from a Dockerfile inside a `.tar.gz` uploaded through the Files
    /// API.
    Context {
        file_id: Uuid,
        /// A path inside the archive; defaults to `Dockerfile`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dockerfile: Option<String>,
        /// `linux/amd64` or `linux/arm64`; defaults to the plane's sandbox
        /// platform.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        platform: Option<String>,
    },
    /// Copy an upstream image, pinned by digest (`…@sha256:…`), into the
    /// plane's registry.
    Mirror { reference: String },
}

/// Where a Recipe publish built an image from, and the compose service it
/// runs as.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ImageProvenance {
    /// The compose service name, a DNS label.
    pub service: String,
    #[serde(default)]
    pub repository_id: Option<Uuid>,
    #[serde(default)]
    pub git_sha: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

/// Body of `POST /v1/images`.
#[derive(Debug, Clone, Serialize)]
pub struct ImageCreate {
    /// Lowercase path segments, e.g. `hello-mcp/mcp-server`.
    pub name: String,
    pub source: ImageSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ImageProvenance>,
    /// Ask for (`true`) or opt out of (`false`) a vulnerability scan within
    /// what the plane's policy allows; `None` takes the policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan: Option<bool>,
}

impl ImageCreate {
    pub fn new(name: impl Into<String>, source: ImageSource) -> Self {
        Self {
            name: name.into(),
            source,
            provenance: None,
            scan: None,
        }
    }
}

// ----- task commands (DP) ----------------------------------------------------

open_wire_enum! {
    pub enum TaskCommandKind {
        Exec => "exec",
        Upload => "upload",
        Download => "download",
    }
}

open_wire_enum! {
    pub enum TaskCommandStatus {
        Queued => "queued",
        Running => "running",
        Completed => "completed",
        Failed => "failed",
    }
}

impl TaskCommandStatus {
    /// Whether the command has finished, either way.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

/// A command against an eval task's live sandbox. `service` names a compose
/// service; `None` (or `main`) is the agent's own container.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskCommandSpec {
    /// Run an argv (never a shell string) and record its exit and output.
    Exec {
        command: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// Defaults to 300.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_seconds: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        service: Option<String>,
    },
    /// Copy a Files API object to an absolute `path` inside the sandbox.
    Upload { file_id: Uuid, path: String },
    /// Copy the file at an absolute `path` out into a new Files API object,
    /// reported as [`TaskCommand::output_file_id`].
    Download {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        service: Option<String>,
    },
}

impl TaskCommandSpec {
    /// An `exec` of `command` in the agent's container, with the defaults.
    pub fn exec<I, S>(command: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Exec {
            command: command.into_iter().map(Into::into).collect(),
            cwd: None,
            timeout_seconds: None,
            service: None,
        }
    }
}

/// A recorded command and, once terminal, its outcome.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskCommand {
    pub id: Uuid,
    pub task_id: Uuid,
    pub kind: TaskCommandKind,
    /// The spec as the API normalized it.
    pub spec: BTreeMap<String, serde_json::Value>,
    pub status: TaskCommandStatus,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
    #[serde(default)]
    pub output_file_id: Option<Uuid>,
    #[serde(default)]
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

/// Filters supported by `GET /v1/tasks/{id}/commands`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskCommandListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskCommandStatus>,
}

// ----- remotes (DP) ----------------------------------------------------------

/// What `GET /v1/remotes/connect` answers: the remote connection's protocol,
/// described. WebSocket routes are not expressible in OpenAPI, so the Data
/// Plane documents the socket on a `GET` of the same path.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemotesConnectDocs {
    /// The socket's path, `/v1/remotes/connect`.
    pub endpoint: String,
    /// Which schemes the socket takes, and where.
    pub protocol: String,
    pub authentication: WsAuthenticationDocs,
    /// The handshake, step by step, in order.
    pub handshake: Vec<String>,
    /// The `remote.proto` schema version the server speaks.
    pub schema_version: i64,
    /// How a frame is carried in a WebSocket message.
    pub encoding: String,
    pub message_protocol: WsMessageProtocolDocs,
}

/// How a WebSocket route authenticates.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WsAuthenticationDocs {
    pub method: String,
    pub description: String,
    pub example: String,
}

/// The frames each side of the remote connection sends.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WsMessageProtocolDocs {
    pub remote_to_cloud: Vec<WsFrameDocs>,
    pub cloud_to_remote: Vec<WsFrameDocs>,
}

/// One frame of the remote protocol. `frame` is the field name in the
/// protobuf `Frame.body` oneof.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WsFrameDocs {
    pub frame: String,
    pub description: String,
}

// ----- runner ----------------------------------------------------------------

/// Identity captured at session creation. Drives experiment routing
/// (HRW / beta-sample) and rides on the access-token claims so DP can
/// stamp it onto `task.metadata.identity` + forward as
/// `TASK_USER_ID` / `TASK_ANONYMOUS_ID` / `TASK_CONVERSATION_ID`
/// sandbox env.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunnerIdentity {
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub anonymous_id: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Tags to stamp on the `customer` member this identity mints, **if that
    /// member is new**. Tags grant access (through tag shares), so this is
    /// bounded on both sides: attenuated
    /// to the asserting agent member's own tags, and applied on create only —
    /// an existing member's tags are never changed here. Tags use the same
    /// opaque, exact-match validation as every other tag write.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Metadata to set on the `customer` member this identity names. Unlike
    /// `tags` it grants nothing, so it applies to an existing member too: the
    /// keys are merged in, overwriting keys of the same name (including ones
    /// an admin set) and keeping the rest. `None` or an empty map changes
    /// nothing. Same bounds as [`MemberUpdateParams::metadata`]; not carried
    /// on the access-token claims.
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, String>>,
}

/// Optional segment.io-style observability payload attached to a
/// [`RunRequest`]. Used by CP for telemetry / experiment-report
/// slicing only — **routing never reads `caller`** (it walks
/// `identity.*` via `hash_key_fields` only). Mixing the two would be
/// a privacy + stability footgun.
///
/// Unknown fields ride along verbatim via [`Self::extra`].
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunCaller {
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub library: Option<RunCallerLibrary>,
    #[serde(default)]
    pub page: Option<RunCallerPage>,
    /// Any additional fields the caller supplied (app / device / os /
    /// campaign / network / screen / timezone / traits / custom keys)
    /// pass through verbatim.
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunCallerLibrary {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunCallerPage {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub referrer: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `POST /v1/runtimes/{id}/run` and `/v1/experiments/{id}/run` body.
///
/// User-facing request type. CP infers everything else (runtime_id /
/// experiment_id from the URL; member_id / org_id / project_id from
/// the bearer key).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize)]
pub struct RunRequest {
    pub identity: Option<RunnerIdentity>,
    /// Environment whose bindings apply. A human member's session may
    /// override its credential's environment; an agent credential stays
    /// bound to its own.
    pub environment: Option<Lane>,
    /// Open the runner on this one Runtime version for the first task. The
    /// Recipe must belong to the Runtime group the URL names; it bypasses
    /// experiment and active-environment routing when the runner is minted.
    /// Runtime `/run` only.
    pub recipe_id: Option<Uuid>,
    /// Whether every application binding must materialize. CP defaults to
    /// `true`; `false` is for operator testing, where identity-dependent
    /// bindings may be reported as unavailable instead.
    pub bindings_required: Option<bool>,
    /// Optional segment.io-style observability payload — see
    /// [`RunCaller`]. Echoed on the response's `runtime_context.caller`.
    pub caller: Option<RunCaller>,
    pub agent_name: Option<String>,
    /// Session lifetime override, max 24h. Default 1h on CP side.
    pub ttl_seconds: Option<u32>,
    pub scope: Option<String>,
}

/// Resolved context attached to a [`RunnerSpec`] — the runtime / arm /
/// identity CP picked. Surfaced on `runner.context()` for telemetry.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunnerContext {
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default)]
    pub experiment_id: Option<Uuid>,
    #[serde(default)]
    pub recipe_id: Option<Uuid>,
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
#[serde_with::skip_serializing_none]
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
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

// ----- telemetry: conversations / events / metrics (DP) ----------------------
//
// These are Data-Plane telemetry reads (`events:read`), served through
// [`crate::DataPlaneResources`] on the client and the runner alike. The
// stores are append-only (`otel_traces` → `/v1/conversations`, `otel_logs` →
// `/v1/events`); all aggregation goes through the bounded `POST /v1/metrics`
// contract.

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

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationAgent {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub invocation_id: Option<String>,
    #[serde(default)]
    pub depth: Option<i64>,
}

/// `total_input_tokens` is every input token the conversation's model calls
/// read (uncached input plus cache reads and writes), not the sum of its
/// items' `gen_ai.usage.input_tokens`, which counts uncached input only.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct ConversationUsage {
    pub total_input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationCost {
    pub usd: f64,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationMetrics {
    pub duration_ms: f64,
    pub trace_count: i64,
    /// The highest `introspection.turn` stamped on the conversation's spans.
    #[serde(default)]
    pub max_turn: Option<i64>,
    pub span_count: i64,
    pub tool_use_count: i64,
    pub failed_tool_use_count: i64,
    #[serde(default)]
    pub llm_call_count: i64,
    #[serde(default)]
    pub failed_llm_call_count: i64,
    pub has_errors: bool,
}

/// The runtime that served a conversation, nested as its items'
/// `introspection.runtime.*`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationRuntime {
    #[serde(default)]
    pub id: Option<Uuid>,
    #[serde(default)]
    pub group_id: Option<Uuid>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationExperiment {
    #[serde(default)]
    pub id: Option<Uuid>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationRecipe {
    #[serde(default)]
    pub git_commit_sha: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ConversationOwner {
    #[serde(default)]
    pub key: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Conversation {
    pub object: String,
    pub id: String,
    #[serde(default)]
    pub task_title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub agents: Option<Vec<ConversationAgent>>,
    pub usage: ConversationUsage,
    pub cost: ConversationCost,
    pub metrics: ConversationMetrics,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub runtime: Option<ConversationRuntime>,
    #[serde(default)]
    pub experiment: Option<ConversationExperiment>,
    #[serde(default)]
    pub recipe: Option<ConversationRecipe>,
    #[serde(default)]
    pub owner: Option<ConversationOwner>,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, String>>,
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
    /// Restrict to the conversation containing this trace.
    pub trace_id: Option<String>,
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
    /// `contains` values to keep, any of which matches (at most 20): what the
    /// sealed messages must contain, as a GenAI semconv path. A chat span
    /// matched on one side opens only that side's messages. Empty keeps every
    /// row. A name may end in one `*` to match by prefix; see
    /// [`ConversationContains`]. Filter by operation with
    /// [`ConversationContains::Operation`] (`contains=gen_ai.operation.name:<op>`).
    pub contains: Vec<ConversationContains>,
    pub lookback_days: Option<u32>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists.
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// One value of the conversation item list's `contains` filter: a GenAI
/// semconv path over the sealed payload and what it must contain.
///
/// The message variants select chat spans by the plaintext summary stamped on
/// them at ingest (and, on a chat span recorded before that summary existed,
/// the whole span); an input variant opens only the span's new input and an
/// output variant only its output. `Tool` and `Operation` match a span
/// attribute and open the whole span. The set is closed because every value
/// is resolved without opening the payload.
///
/// An input path is evaluated over the messages new to the item, not the full
/// history that [`ConversationItemInclude`]'s input expansion adds.
///
/// # Prefix match
///
/// The four name variants ([`InputToolResponse`](Self::InputToolResponse),
/// [`OutputToolCall`](Self::OutputToolCall), [`Tool`](Self::Tool) and
/// [`Operation`](Self::Operation)) take a name that may end in one `*`, which
/// matches every name starting with what precedes it; `*` alone matches any
/// name. The server answers `422` for a `*` anywhere else in the name, and the
/// fixed variants take none.
///
/// ```
/// use introspection_sdk::ConversationContains;
///
/// // Every chat span whose output calls a Browserbase MCP tool.
/// let browserbase = ConversationContains::OutputToolCall("mcp_browserbase_*".into());
/// assert_eq!(
///     browserbase.to_wire(),
///     "gen_ai.output.messages.parts.name:mcp_browserbase_*",
/// );
/// // Every `execute_tool` span of an MCP tool.
/// assert_eq!(
///     ConversationContains::Tool("mcp_*".into()).to_wire(),
///     "gen_ai.tool.name:mcp_*",
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationContains {
    /// `gen_ai.input.messages.role:user`: a chat span whose new input carries
    /// a user-authored part.
    InputUser,
    /// `gen_ai.input.messages.parts.type:tool_call_response`: a chat span
    /// whose new input carries any tool's response.
    AnyInputToolResponse,
    /// `gen_ai.input.messages.parts.name:<tool>`: a chat span whose new input
    /// carries the named tool's response. The name may end in `*`.
    InputToolResponse(String),
    /// `gen_ai.output.messages.parts.type:text`: a chat span whose output
    /// carries assistant text.
    OutputText,
    /// `gen_ai.output.messages.parts.type:tool_call`: a chat span whose
    /// output calls any tool.
    AnyOutputToolCall,
    /// `gen_ai.output.messages.parts.name:<tool>`: a chat span whose output
    /// calls the named tool. The name may end in `*`.
    OutputToolCall(String),
    /// `gen_ai.tool.name:<tool>`: the `execute_tool` span of the named tool.
    /// The name may end in `*`.
    Tool(String),
    /// `gen_ai.operation.name:<operation>`: every span of that operation,
    /// e.g. the `invoke_agent` wrapper of a delegated run. The name may end
    /// in `*`.
    Operation(String),
}

impl ConversationContains {
    /// The value as it goes on the wire.
    pub fn to_wire(&self) -> String {
        match self {
            Self::InputUser => "gen_ai.input.messages.role:user".into(),
            Self::AnyInputToolResponse => {
                "gen_ai.input.messages.parts.type:tool_call_response".into()
            }
            Self::InputToolResponse(name) => format!("gen_ai.input.messages.parts.name:{name}"),
            Self::OutputText => "gen_ai.output.messages.parts.type:text".into(),
            Self::AnyOutputToolCall => "gen_ai.output.messages.parts.type:tool_call".into(),
            Self::OutputToolCall(name) => format!("gen_ai.output.messages.parts.name:{name}"),
            Self::Tool(name) => format!("gen_ai.tool.name:{name}"),
            Self::Operation(name) => format!("gen_ai.operation.name:{name}"),
        }
    }
}

impl Serialize for ConversationContains {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_wire())
    }
}

/// Parameters for `GET /v1/conversations/{conversation_id}/items/{item_id}`.
#[derive(Debug, Clone, Default)]
pub struct ConversationItemGetParams {
    pub include: Vec<ConversationItemInclude>,
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
        put_str(&mut obj, "trace_id", self.trace_id.as_ref());
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

// ----- events: typed family read (`GET /v1/events`) --------------------------

/// The canonical platform event families served by `GET /v1/events` that
/// this SDK types.
///
/// The events read is a **closed, typed set**: `event_name` is required on
/// every list read — exactly one family per request — so a response page is
/// always homogeneous and fully typeable (JSON discriminated member; Arrow
/// typed payload struct column). Legacy verb-suffixed names on historical
/// rows are normalized to these canonical names server-side; raw `gen_ai.*`
/// rows are not returned and remain aggregable via `POST /v1/metrics`.
///
/// The platform serves further families this SDK build does not type yet
/// (`introspection.track`, `introspection.issue`,
/// `introspection.repository.*`): request one with [`Self::Unknown`] and read
/// its rows as [`Event::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntrospectionEventName {
    Annotation,
    Feedback,
    Observation,
    ObservationClusteringRun,
    Judgement,
    Pattern,
    PatternAssignment,
    /// One automation trigger that ran.
    AutomationTriggered,
    /// One scheduled automation trigger that ran nothing. Project-owned, so
    /// readable only with project-wide telemetry access.
    AutomationSkipped,
    /// A family added after this SDK was released. Keeping the wire value
    /// makes the [`Event::Unknown`] response fallback reachable in practice.
    Unknown(String),
}

impl IntrospectionEventName {
    /// On-the-wire dotted family name.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Annotation => "introspection.annotation",
            Self::Feedback => "introspection.feedback",
            Self::Observation => "introspection.observation",
            Self::ObservationClusteringRun => "introspection.observation_clustering.run",
            Self::Judgement => "introspection.judgement",
            Self::Pattern => "introspection.pattern",
            Self::PatternAssignment => "introspection.pattern.assignment",
            Self::AutomationTriggered => "introspection.automation.triggered",
            Self::AutomationSkipped => "introspection.automation.skipped",
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
            "introspection.annotation" => Self::Annotation,
            "introspection.feedback" => Self::Feedback,
            "introspection.observation" => Self::Observation,
            "introspection.observation_clustering.run" => Self::ObservationClusteringRun,
            "introspection.judgement" => Self::Judgement,
            "introspection.pattern" => Self::Pattern,
            "introspection.pattern.assignment" => Self::PatternAssignment,
            "introspection.automation.triggered" => Self::AutomationTriggered,
            "introspection.automation.skipped" => Self::AutomationSkipped,
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TypedEvent<P> {
    pub id: String,
    /// Per-family semantics: observation → `observed_at`, pattern →
    /// `updated_at` (catalog cursor), stream families → emit/observed time.
    pub timestamp: String,
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub span_id: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub experiment_id: Option<Uuid>,
    #[serde(default)]
    pub recipe_git_commit_sha: Option<String>,
    /// Family detail — one of the `*Payload` types, fixed by the
    /// [`Event`] variant.
    pub payload: P,
}

/// `introspection.observation` payload — one **resolved** observation (the
/// server-side fold: supersession applied, current pattern assignment
/// joined). All fields optional except the `observation_id` identity.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservationPayload {
    pub observation_id: Uuid,
    #[serde(default)]
    pub lens: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub segment: Option<i64>,
    #[serde(default)]
    pub sentiment: Option<String>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub evidence_refs: Option<Vec<String>>,
    #[serde(default)]
    pub prompt_version: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub source_hash: Option<String>,
    #[serde(default)]
    pub replaces_observation_id: Option<Uuid>,
    /// CURRENT pattern assignment (fold), not the assignment history.
    #[serde(default)]
    pub pattern_id: Option<String>,
    #[serde(default)]
    pub assignment_score: Option<f64>,
    #[serde(default)]
    pub assignment_method: Option<String>,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

/// `introspection.pattern` payload — one **folded** catalog row (current
/// state: latest lifecycle action, status, fold timestamps).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatternPayload {
    pub pattern_id: String,
    /// Latest lifecycle action (`created` / `updated` / `retired`).
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub lens: Option<String>,
    /// `active` | `retired` (fold).
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub retired_at: Option<String>,
    #[serde(default)]
    pub last_detected_at: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub replacement_pattern_id: Option<String>,
    #[serde(default)]
    pub derived_from_pattern_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
}

/// `introspection.pattern.assignment` payload — one observation→pattern
/// assignment event (stream family). `observation_id` is the sole identity
/// field; `pattern_id: None` means the observation was explicitly unassigned.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatternAssignmentPayload {
    pub observation_id: Uuid,
    /// Target pattern; `None` = explicitly unassigned.
    #[serde(default)]
    pub pattern_id: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
}

/// `introspection.observation_clustering.run` payload — one clustering run
/// (stream family).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusteringRunPayload {
    pub run_id: String,
    #[serde(default)]
    pub lens: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub observation_count: Option<i64>,
    #[serde(default)]
    pub pattern_count: Option<i64>,
    #[serde(default)]
    pub noise_count: Option<i64>,
    #[serde(default)]
    pub params: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub replaces_run_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// `introspection.feedback` payload — mirrors what the SDK `feedback()`
/// surfaces actually emit (`properties.*` / `identity.*` attributes).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeedbackPayload {
    /// The feedback label (`"thumbs_up"`, …) — `properties.name`.
    pub name: String,
    #[serde(default)]
    pub comments: Option<String>,
    /// Numeric axis, when present — `properties.value`.
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub anonymous_id: Option<String>,
    /// Optional **emitted** field (positive/negative/neutral) — never derived
    /// server-side.
    #[serde(default)]
    pub sentiment: Option<String>,
    /// Response the feedback anchors to —
    /// `gen_ai.request.previous_response_id`.
    #[serde(default)]
    pub previous_response_id: Option<String>,
    /// `gen_ai.agent.name`.
    #[serde(default)]
    pub agent_name: Option<String>,
    /// `gen_ai.agent.id`.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Remaining `properties.*` extras.
    #[serde(default)]
    pub properties: Option<BTreeMap<String, serde_json::Value>>,
}

/// `introspection.annotation` payload — one member-authored mutation.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnnotationPayload {
    pub member_id: Uuid,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub assignee_member_ids: Option<Vec<Uuid>>,
}

/// `introspection.judgement` payload — mirrors the runtime-agent judges
/// emitter (`introspection.judgement.*` / `introspection.judge.*` attributes).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JudgementPayload {
    pub judgement_id: String,
    #[serde(default)]
    pub judge_id: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub definition_hash: Option<String>,
    #[serde(default)]
    pub contract_version: Option<String>,
    #[serde(default)]
    pub sequence_hash: Option<String>,
    #[serde(default)]
    pub experiment_arm_id: Option<Uuid>,
}

/// `introspection.automation.triggered` payload — one automation trigger
/// that ran (stream family, owned by the task's member).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AutomationTriggeredPayload {
    pub automation_id: Uuid,
    pub automation_name: String,
    #[serde(default)]
    pub prompt: Option<String>,
    pub trigger_type: AutomationTriggerType,
    /// The `next_trigger_at` a scheduled trigger claimed; `None` for a hand
    /// trigger.
    #[serde(default)]
    pub slot: Option<String>,
    /// The task created, or posted into when `posted` is true.
    pub task_id: Uuid,
    pub posted: bool,
    /// The task's member, who owns the event.
    pub member_id: Uuid,
    #[serde(default)]
    pub runtime_group_id: Option<Uuid>,
    /// The person who triggered it by hand.
    #[serde(default)]
    pub triggered_by_member_id: Option<Uuid>,
}

/// `introspection.automation.skipped` payload — one scheduled automation
/// trigger that ran nothing (stream family, project-owned).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AutomationSkippedPayload {
    pub automation_id: Uuid,
    pub trigger_type: AutomationTriggerType,
    #[serde(default)]
    pub slot: Option<String>,
    /// Set only when the automation targets an existing task.
    #[serde(default)]
    pub task_id: Option<Uuid>,
    pub reason: AutomationSkipReason,
}

/// Whole-event envelope + typed payload per family.
pub type ObservationEvent = TypedEvent<ObservationPayload>;
pub type PatternEvent = TypedEvent<PatternPayload>;
pub type PatternAssignmentEvent = TypedEvent<PatternAssignmentPayload>;
pub type ClusteringRunEvent = TypedEvent<ClusteringRunPayload>;
pub type FeedbackEvent = TypedEvent<FeedbackPayload>;
pub type AnnotationEvent = TypedEvent<AnnotationPayload>;
pub type JudgementEvent = TypedEvent<JudgementPayload>;
pub type AutomationTriggeredEvent = TypedEvent<AutomationTriggeredPayload>;
pub type AutomationSkippedEvent = TypedEvent<AutomationSkippedPayload>;

/// One event from `GET /v1/events` — a discriminated union of the
/// canonical platform families, tagged by the top-level `event_name`.
///
/// Because `event_name` is required on the list read, a page is always
/// homogeneous — every record matches the requested family. The hidden
/// [`Event::Unknown`] fallback tolerates a family this SDK build doesn't know
/// yet (a family added server-side must not fail the whole page);
/// match on it to skip or hand-parse such rows.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "event_name")]
pub enum Event {
    #[serde(rename = "introspection.annotation")]
    Annotation(AnnotationEvent),
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
    #[serde(rename = "introspection.automation.triggered")]
    AutomationTriggered(AutomationTriggeredEvent),
    #[serde(rename = "introspection.automation.skipped")]
    AutomationSkipped(AutomationSkippedEvent),
    /// Forward-compatible escape hatch: a row whose `event_name` this SDK
    /// build doesn't recognise. Carries the raw record verbatim.
    #[serde(untagged)]
    Unknown(serde_json::Value),
}

impl Event {
    /// The canonical family, or `None` for [`Event::Unknown`] rows.
    pub fn event_name(&self) -> Option<IntrospectionEventName> {
        match self {
            Self::Annotation(_) => Some(IntrospectionEventName::Annotation),
            Self::Feedback(_) => Some(IntrospectionEventName::Feedback),
            Self::Observation(_) => Some(IntrospectionEventName::Observation),
            Self::ObservationClusteringRun(_) => {
                Some(IntrospectionEventName::ObservationClusteringRun)
            }
            Self::Judgement(_) => Some(IntrospectionEventName::Judgement),
            Self::Pattern(_) => Some(IntrospectionEventName::Pattern),
            Self::PatternAssignment(_) => Some(IntrospectionEventName::PatternAssignment),
            Self::AutomationTriggered(_) => Some(IntrospectionEventName::AutomationTriggered),
            Self::AutomationSkipped(_) => Some(IntrospectionEventName::AutomationSkipped),
            Self::Unknown(_) => None,
        }
    }
}

/// Ergonomic params for `GET /v1/events`. [`Self::event_name`] is
/// **required** (compile-enforced); [`Self::extra_event_names`] merges more
/// families into the same page when they share its row shape and visibility
/// class, which the server checks (`422` otherwise). Per-family filters (§4.3 of the telemetry
/// read design — e.g. observation `pattern_id` / `lens` /
/// `include_superseded`, pattern `lens` / `status`) pass through
/// [`Self::filters`] verbatim.
#[derive(Debug, Clone)]
pub struct EventListParams {
    /// The family to list — required; there is no unfiltered read.
    pub event_name: IntrospectionEventName,
    /// More families read into the same page, sent as repeated `event_name`
    /// after [`Self::event_name`]. They must share its row shape and
    /// visibility class. Duplicates are harmless; the server drops them.
    pub extra_event_names: Option<Vec<IntrospectionEventName>>,
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
    /// Automation triggers and skips of one automation.
    pub automation_id: Option<Uuid>,
    /// Automation triggers and skips: the task created or posted into.
    pub task_id: Option<Uuid>,
    /// Judgements of one judge.
    pub judge_id: Option<Uuid>,
    /// Issue events of one issue.
    pub issue_id: Option<Uuid>,
    /// `introspection.track` events by exact stored event name (wire
    /// `name`, repeatable, at most 20).
    pub track_names: Option<Vec<String>>,
    /// The latest event per human request (wire `request`). Requires a
    /// descending order.
    pub latest_requests: Option<bool>,
    /// `introspection.track` only: the newest event per record in one
    /// conversation (wire `latest`). Requires `conversation_id` and
    /// [`Self::track_names`].
    pub latest_records: Option<bool>,
    /// One current human request; requires [`Self::latest_requests`].
    pub request_id: Option<Uuid>,
    /// The current request assignee; requires [`Self::latest_requests`].
    pub assignee_id: Option<Uuid>,
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
            extra_event_names: None,
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
            automation_id: None,
            task_id: None,
            judge_id: None,
            issue_id: None,
            track_names: None,
            latest_requests: None,
            latest_records: None,
            request_id: None,
            assignee_id: None,
            filters: None,
        }
    }

    /// Validate and lower to the wire query object (see
    /// [`ConversationListParams::to_wire`]).
    pub fn to_wire(&self) -> ApiResult<serde_json::Value> {
        let mut obj = serde_json::Map::new();
        let extra = self.extra_event_names.as_deref().unwrap_or_default();
        if extra.is_empty() {
            obj.insert("event_name".to_string(), self.event_name.as_str().into());
        } else {
            let names = std::iter::once(&self.event_name).chain(extra);
            obj.insert(
                "event_name".to_string(),
                serde_json::Value::Array(names.map(|name| name.as_str().into()).collect()),
            );
        }
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
        put_uuid(&mut obj, "automation_id", self.automation_id);
        put_uuid(&mut obj, "task_id", self.task_id);
        put_uuid(&mut obj, "judge_id", self.judge_id);
        put_uuid(&mut obj, "issue_id", self.issue_id);
        put_list(&mut obj, "name", self.track_names.as_ref());
        put_bool(&mut obj, "request", self.latest_requests);
        put_bool(&mut obj, "latest", self.latest_records);
        put_uuid(&mut obj, "request_id", self.request_id);
        put_uuid(&mut obj, "assignee_id", self.assignee_id);
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

/// Lower a metadata filter to its `key:value` pairs. Sorting makes request
/// serialization deterministic despite `HashMap` iteration order.
fn metadata_filter_pairs(metadata: &HashMap<String, String>) -> Vec<String> {
    let mut pairs: Vec<_> = metadata
        .iter()
        .map(|(key, value)| format!("{key}:{value}"))
        .collect();
    pairs.sort();
    pairs
}

/// Insert conversation metadata as repeated `metadata=key:value` filters.
/// An empty map means no filter and is omitted.
fn put_metadata(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    metadata: Option<&HashMap<String, String>>,
) {
    let Some(metadata) = metadata.filter(|metadata| !metadata.is_empty()) else {
        return;
    };
    obj.insert(
        "metadata".to_string(),
        serde_json::Value::Array(
            metadata_filter_pairs(metadata)
                .into_iter()
                .map(Into::into)
                .collect(),
        ),
    );
}

fn metadata_filter_is_empty(metadata: &Option<HashMap<String, String>>) -> bool {
    metadata.as_ref().is_none_or(HashMap::is_empty)
}

/// Serde form of [`put_metadata`] for derive-serialized list params.
fn serialize_metadata_filter<S: serde::Serializer>(
    metadata: &Option<HashMap<String, String>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let pairs = metadata
        .as_ref()
        .map(metadata_filter_pairs)
        .unwrap_or_default();
    serializer.collect_seq(pairs)
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
///
/// `count` takes no measure and every other aggregation requires one; the API
/// rejects either mismatch with a 422, so build a count with [`Self::count`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<String>,
    pub aggregation: String,
}

impl MetricSpec {
    /// The `count` aggregation, which counts rows and takes no measure.
    pub fn count() -> Self {
        Self {
            measure: None,
            aggregation: "count".into(),
        }
    }

    /// An aggregation over `measure`, such as `("sum", "status_error")`.
    pub fn of(aggregation: impl Into<String>, measure: impl Into<String>) -> Self {
        Self {
            measure: Some(measure.into()),
            aggregation: aggregation.into(),
        }
    }
}

/// One grouping dimension `{field}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    pub field: String,
}

/// One `{field, operator, value}` filter term. `exists` takes no value: pass
/// `serde_json::Value::Null`.
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

/// Response from `POST /v1/metrics`. Every row has the same shape whatever
/// the view: the grouping values in `dimensions`, and one value per requested
/// metric in `metrics`, in request order.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MetricsResponse {
    #[serde(default)]
    pub data: Vec<MetricRow>,
    #[serde(default)]
    pub meta: Option<MetricsMeta>,
}

impl MetricsResponse {
    /// Whether the rows were cut at `row_limit`, so they cover only part of
    /// the window.
    pub fn truncated(&self) -> bool {
        self.meta.as_ref().is_some_and(|meta| meta.truncated)
    }
}

/// One row of a [`MetricsResponse`].
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct MetricRow {
    /// Bucket start; set only with a `time_dimension`.
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub dimensions: Vec<MetricDimensionValue>,
    #[serde(default)]
    pub metrics: Vec<MetricValue>,
}

impl MetricRow {
    /// The value this row groups `field` by.
    pub fn dimension(&self, field: &str) -> Option<&str> {
        self.dimensions
            .iter()
            .find(|dimension| dimension.field == field)
            .map(|dimension| dimension.value.as_str())
    }

    /// The value of the metric at `metric_index` in the request.
    pub fn metric(&self, metric_index: u32) -> Option<f64> {
        self.metrics
            .iter()
            .find(|metric| metric.metric_index == metric_index)
            .map(|metric| metric.value)
    }
}

/// One grouping value of a [`MetricRow`].
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct MetricDimensionValue {
    pub field: String,
    pub value: String,
}

/// One aggregated value of a [`MetricRow`]; `metric_index` points into the
/// request's `metrics`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct MetricValue {
    pub metric_index: u32,
    #[serde(default)]
    pub measure: Option<String>,
    pub aggregation: String,
    pub value: f64,
}

/// What a [`MetricsResponse`] answered for.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MetricsMeta {
    #[serde(default)]
    pub view: String,
    /// The window the query ran over after the server resolved it.
    #[serde(default)]
    pub window: Option<MetricsWindow>,
    #[serde(default)]
    pub row_count: u64,
    #[serde(default)]
    pub row_limit: u64,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub step_seconds: Option<i64>,
    #[serde(default)]
    pub approximate: bool,
    /// The rows stop at `row_limit`; the counts cover part of the window.
    #[serde(default)]
    pub truncated: bool,
    /// The read was confined to the caller's own owner key, not the project.
    /// A project API key reads only its system agent's rows, so expect `true`
    /// with one; a member login reads the whole project.
    #[serde(default)]
    pub owner_scoped: bool,
    /// The `order_by` the query ran with, after the server's defaults.
    #[serde(default)]
    pub order_by: Vec<OrderTerm>,
}

/// `meta.window` of a [`MetricsResponse`].
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct MetricsWindow {
    pub start: String,
    pub end: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_kind_round_trips_known_variants() {
        for (wire, kind) in [
            ("agent", TaskKind::Agent),
            ("eval", TaskKind::Eval),
            ("process", TaskKind::Process),
        ] {
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
                measure: Some("conversation_id".into()),
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
            context.runtime_id.unwrap().to_string(),
            "00000000-0000-0000-0000-000000000041"
        );
        assert_eq!(
            context.runtime_group_id.unwrap().to_string(),
            "00000000-0000-0000-0000-000000000042"
        );
        assert_eq!(context.agent_name.as_deref(), Some("support-agent"));
    }

    #[test]
    fn a_runner_context_decodes_without_runtime_and_recipe_ids() {
        let context: RunnerContext = serde_json::from_value(json!({
            "runtime_group_id": "00000000-0000-0000-0000-000000000042",
            "identity": {}
        }))
        .unwrap();
        assert!(context.runtime_id.is_none());
        assert!(context.recipe_id.is_none());
        assert!(context.experiment_id.is_none());

        let empty: RunnerContext = serde_json::from_value(json!({})).unwrap();
        assert!(empty.runtime_id.is_none() && empty.recipe_id.is_none());
        assert!(empty.identity.user_id.is_none());
    }

    #[test]
    fn a_run_request_sends_environment_recipe_and_bindings_only_when_set() {
        let recipe_id = Uuid::parse_str("00000000-0000-0000-0000-000000000043").unwrap();
        let request = serde_json::to_value(RunRequest {
            environment: Some(Lane::Staging),
            recipe_id: Some(recipe_id),
            bindings_required: Some(false),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            request,
            json!({
                "environment": "staging",
                "recipe_id": "00000000-0000-0000-0000-000000000043",
                "bindings_required": false
            })
        );
        assert_eq!(
            serde_json::to_value(RunRequest::default()).unwrap(),
            json!({})
        );
    }

    #[test]
    fn a_runtime_reads_its_yank_and_group_fields_and_a_null_recipe() {
        let runtime: Runtime = serde_json::from_value(json!({
            "id": "00000000-0000-0000-0000-000000000041",
            "org_id": "00000000-0000-0000-0000-0000000000aa",
            "project_id": "00000000-0000-0000-0000-0000000000bb",
            "recipe_id": null,
            "runtime_group_id": "00000000-0000-0000-0000-000000000042",
            "created_by_member_id": "00000000-0000-0000-0000-0000000000cc",
            "created_at": "2026-09-01T00:00:00Z",
            "updated_at": "2026-09-02T00:00:00Z",
            "name": "Support",
            "slug": "support",
            "environments": ["staging", "production"],
            "yanked_at": "2026-09-03T00:00:00Z",
            "yanked_reason": "bad prompt"
        }))
        .unwrap();
        assert!(runtime.recipe_id.is_none());
        assert_eq!(
            runtime.runtime_group_id.unwrap().to_string(),
            "00000000-0000-0000-0000-000000000042"
        );
        assert_eq!(runtime.environments, ["staging", "production"]);
        assert_eq!(runtime.yanked_at.as_deref(), Some("2026-09-03T00:00:00Z"));
        assert_eq!(runtime.yanked_reason.as_deref(), Some("bad prompt"));

        // A Control Plane predating these fields still reads, and the optional
        // ones stay off the wire when absent.
        let older: Runtime = serde_json::from_value(json!({
            "id": "00000000-0000-0000-0000-000000000041",
            "org_id": "00000000-0000-0000-0000-0000000000aa",
            "project_id": "00000000-0000-0000-0000-0000000000bb",
            "recipe_id": "00000000-0000-0000-0000-000000000043",
            "created_by_member_id": "00000000-0000-0000-0000-0000000000cc",
            "created_at": "2026-09-01T00:00:00Z",
            "updated_at": "2026-09-02T00:00:00Z",
            "name": "Support",
            "slug": "support"
        }))
        .unwrap();
        assert!(older.environments.is_empty());
        assert!(older.yanked_at.is_none());
        let wire = serde_json::to_value(&older).unwrap();
        assert!(wire.get("yanked_at").is_none());
        assert!(wire.get("runtime_group_id").is_none());
    }

    #[test]
    fn repository_enums_round_trip_and_keep_unknown_values() {
        for (wire, provider) in [
            ("github", RepositoryProvider::Github),
            ("introspection", RepositoryProvider::Introspection),
            ("codeberg", RepositoryProvider::Other("codeberg".into())),
        ] {
            let decoded: RepositoryProvider = serde_json::from_value(json!(wire)).unwrap();
            assert_eq!(decoded, provider);
            assert_eq!(serde_json::to_value(&decoded).unwrap(), json!(wire));
        }
        let legacy: RepositoryProvider = serde_json::from_value(json!("hosted")).unwrap();
        assert_eq!(
            legacy,
            RepositoryProvider::Introspection,
            "the name it shipped under"
        );
        let status: RepositoryProvisioningStatus = serde_json::from_value(json!("failed")).unwrap();
        assert_eq!(status, RepositoryProvisioningStatus::Failed);
        assert_eq!(
            RepositoryProvisioningStatus::default(),
            RepositoryProvisioningStatus::Pending
        );
        assert_eq!(RepositoryProvider::default(), RepositoryProvider::Github);
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
            "created_by_member_id": "0195c0de-0000-7000-8000-000000000004",
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
    fn list_filters_ride_the_query_beside_the_typed_fields() {
        // Unset, the passthrough adds nothing: the object is exactly the
        // typed fields, so a Paginator still sees a plain object.
        let bare = serde_json::to_value(RuntimeListParams {
            runtime: Some("customer-agent".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(bare, json!({ "runtime": "customer-agent" }));

        // Set, every pair lands beside them, and a pair naming a typed field
        // replaces it — the passthrough is the caller's last word.
        let merged = serde_json::to_value(RuntimeListParams {
            runtime: Some("customer-agent".into()),
            limit: Some(1),
            filters: Some(HashMap::from([
                ("environment".to_string(), json!("staging")),
                ("yanked".to_string(), json!(false)),
                ("limit".to_string(), json!(5)),
            ])),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            merged,
            json!({ "runtime": "customer-agent", "limit": 5, "environment": "staging", "yanked": false })
        );

        let repositories = serde_json::to_value(RepositoryListParams {
            project: Some("acme".into()),
            slug: Some("example/recipes".into()),
            filters: None,
        })
        .unwrap();
        assert_eq!(
            repositories,
            json!({ "project": "acme", "slug": "example/recipes" })
        );
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
    fn extra_event_names_follow_the_family_as_repeated_event_name() {
        let wire = EventListParams {
            extra_event_names: Some(vec![
                IntrospectionEventName::AutomationSkipped,
                IntrospectionEventName::Unknown("introspection.issue".into()),
            ]),
            ..EventListParams::new(IntrospectionEventName::AutomationTriggered)
        }
        .to_wire()
        .unwrap();
        assert_eq!(
            wire["event_name"],
            json!([
                "introspection.automation.triggered",
                "introspection.automation.skipped",
                "introspection.issue"
            ])
        );
    }

    #[test]
    fn no_extra_event_names_keeps_a_single_event_name() {
        let wire = EventListParams {
            extra_event_names: Some(Vec::new()),
            ..EventListParams::new(IntrospectionEventName::Feedback)
        }
        .to_wire()
        .unwrap();
        assert_eq!(wire["event_name"], "introspection.feedback");
    }

    #[test]
    fn request_and_track_filters_reach_the_wire_under_server_names() {
        let id = Uuid::from_u128(7);
        let wire = EventListParams {
            judge_id: Some(id),
            issue_id: Some(id),
            track_names: Some(vec!["order.placed".into(), "order.paid".into()]),
            latest_requests: Some(true),
            latest_records: Some(false),
            request_id: Some(id),
            assignee_id: Some(id),
            ..EventListParams::new(IntrospectionEventName::Unknown(
                "introspection.track".into(),
            ))
        }
        .to_wire()
        .unwrap();
        assert_eq!(wire["judge_id"], id.to_string());
        assert_eq!(wire["issue_id"], id.to_string());
        assert_eq!(wire["name"], json!(["order.placed", "order.paid"]));
        assert_eq!(wire["request"], true);
        assert_eq!(wire["latest"], false);
        assert_eq!(wire["request_id"], id.to_string());
        assert_eq!(wire["assignee_id"], id.to_string());
        assert!(wire.get("track_names").is_none());
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
        // A family added server-side after this SDK build must not
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
                measure: Some("duration_ns".into()),
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

    #[test]
    fn trajectory_tool_call_record_serializes_content_null() {
        // A tool-call record is told apart from prose by `content: null`, so
        // the null must survive serialization, not be skipped like other
        // `None`s.
        let wire = json!({
            "role": "assistant",
            "content": null,
            "timestamp": "2025-01-01T00:00:01Z",
            "tool_calls": [{"id": "call_1", "name": "edit", "args": "{}"}],
        });
        let record: TrajectoryRecord = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&record).unwrap(), wire);
    }
}

// ----- trajectory-v1 (conversation export) ----------------------------------

/// One tool invocation inside a [`TrajectoryAssistantRecord`].
///
/// `args` is a **JSON-encoded string**, not an object. That is the upstream
/// trajectory-v1 contract rather than an oversight here: the encoded value is
/// an object, and a malformed or scalar source value arrives as
/// `{"_raw": ...}` so the evidence survives without breaking the schema.
#[serde_with::skip_serializing_none]
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryMetaRecord {
    /// Harness that produced the session, e.g. `"claude-code"`.
    pub source: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// A user turn.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryUserRecord {
    pub content: String,
    /// ISO-8601 timestamp.
    pub timestamp: String,
}

/// Model reasoning, when the source exposed it.
#[serde_with::skip_serializing_none]
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
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryAssistantRecord {
    #[serialize_always]
    pub content: Option<String>,
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Present only on a tool-call record, and then never empty.
    #[serde(default)]
    pub tool_calls: Option<Vec<TrajectoryToolCall>>,
}

/// A tool result, linked to its call by `tool_call_id`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrajectoryToolRecord {
    pub tool_call_id: String,
    pub content: String,
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Source-native success status; absent when the source exposes none.
    #[serde(default)]
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
    /// One `gen_ai.operation.name` to keep; `operation_names` adds more, any
    /// of which matches. Both go on the wire as the repeated `operation_name`
    /// key through [`to_wire`](Self::to_wire), which the export reads use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub operation_names: Vec<String>,
    /// Partition lookback bound in days (1-365).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookback_days: Option<u16>,
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

impl ConversationExportParams {
    /// The lowered query exactly as it goes on the wire: `operation_name` is
    /// repeated once per name, which a struct serialization cannot express
    /// (two fields under one key would overwrite each other).
    pub fn to_wire(&self) -> Vec<(String, String)> {
        let mut query = Vec::new();
        let mut push = |key: &str, value: Option<String>| {
            if let Some(value) = value {
                query.push((key.to_string(), value));
            }
        };
        push("agent", self.agent.clone());
        push("service_name", self.service_name.clone());
        push("operation_name", self.operation_name.clone());
        for name in &self.operation_names {
            push("operation_name", Some(name.clone()));
        }
        push(
            "lookback_days",
            self.lookback_days.map(|days| days.to_string()),
        );
        push("start_date", self.start_date.clone());
        push("end_date", self.end_date.clone());
        query
    }
}
