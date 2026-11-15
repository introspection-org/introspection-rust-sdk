//! `routines()` (DP) — scheduled prompts and platform work on a project,
//! on the client or a [`Runner`](crate::Runner).
//!
//! A routine either runs a prompt as an agent task (a person's own
//! routine, [`Routine::kind`] `None`) or runs platform work (a
//! [`RoutineKind`]). Set `task_id` to post each firing into an existing
//! task instead of creating one; a one-off reminder is a
//! [`RoutineTriggerType::Manual`] routine with a future
//! `next_trigger_at`. Each firing of a routine that runs as a task is
//! recorded as an [`IntrospectionEventName::RoutineTriggered`] or
//! [`IntrospectionEventName::RoutineSkipped`] event, read through
//! [`crate::Events`] with the `routine_id` and `task_id` filters.
//!
//! Beside its schedule and a manual trigger, a prompt routine that starts a
//! task per run can run on events: its [`Routine::triggers`] listen to an
//! integration (Slack, GitHub) or a project connector, such as a `webhook`
//! connector, and each event-started run reads the event's body as its
//! parameters.
//!
//! These are Data Plane routes, gated on `routines:read` /
//! `routines:write`; a runner a member opens for themself carries both.
//! The API serves them to administrators only today and answers 403 to anyone
//! else. introspection-cloud#3137 opens them to members for their own
//! routines that post into one of their own tasks; until it ships,
//! `RoutineListParams::task_id` is not served either.
//!
//! [`RoutineKind`]: crate::RoutineKind
//! [`RoutineTriggerType::Manual`]: crate::RoutineTriggerType::Manual
//! [`IntrospectionEventName::RoutineTriggered`]: crate::IntrospectionEventName::RoutineTriggered
//! [`IntrospectionEventName::RoutineSkipped`]: crate::IntrospectionEventName::RoutineSkipped

use std::path::Path;
use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::files::Files;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    FileCreateText, Routine, RoutineCreateParams, RoutineListParams, RoutineParameters,
    RoutineTriggerParams, RoutineTriggerRequest, RoutineTriggerResponse, RoutineUpdateParams,
    TaskFileRef, ROUTINE_PARAMETERS_FILE_NAME,
};

/// How long a run's uploaded parameters file lives: 30 days.
const PARAMETERS_FILE_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

/// `routines()` namespace. Holds a DP-bound HTTP client.
#[derive(Clone)]
pub struct Routines {
    http: Arc<HttpClient>,
}

impl Routines {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/routines` — paginated.
    pub fn list(&self, params: &RoutineListParams) -> Paginator<Routine> {
        Paginator::new(self.http.clone(), "/v1/routines", params)
            .expect("RoutineListParams must serialize to a JSON object")
    }

    /// `GET /v1/routines/{id}`. A soft-deleted routine is still
    /// returned (the read model does not say it was deleted).
    pub async fn get(&self, routine_id: Uuid) -> ApiResult<Routine> {
        let path = format!("/v1/routines/{routine_id}");
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/routines`.
    pub async fn create(&self, params: &RoutineCreateParams) -> ApiResult<Routine> {
        self.http.post_json("/v1/routines", params).await
    }

    /// `PATCH /v1/routines/{id}` — only the fields set on `params` are
    /// sent.
    pub async fn update(
        &self,
        routine_id: Uuid,
        params: &RoutineUpdateParams,
    ) -> ApiResult<Routine> {
        let path = format!("/v1/routines/{routine_id}");
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/routines/{id}` — soft delete. A platform default answers
    /// 409; disable it instead.
    pub async fn delete(&self, routine_id: Uuid) -> ApiResult<()> {
        let path = format!("/v1/routines/{routine_id}");
        self.http.delete_empty(&path).await
    }

    /// `POST /v1/routines/{id}/trigger` — run it now. Answers with the task
    /// it created or posted into; neither reads nor clears a scheduled slot.
    ///
    /// [`RoutineParameters::Inline`] parameters, and those read from a
    /// [`RoutineParameters::Path`], are sent inline and may
    /// first be uploaded with `POST /v1/files` as
    /// `routine-parameters/<uuid>.json` (expiring in 30 days) and attached as
    /// `parameters.json`, as [`RoutineTriggerParams::attach_parameters_file`]
    /// decides; left `None` it reads the routine first to decide.
    /// [`RoutineParameters::File`] sends only that reference, and the server
    /// reads the parameters from it. Default params send `{}`.
    pub async fn trigger(
        &self,
        routine_id: Uuid,
        params: &RoutineTriggerParams,
    ) -> ApiResult<RoutineTriggerResponse> {
        let local = match &params.parameters {
            Some(RoutineParameters::Path(path)) => Some(read_parameters(path).await?),
            Some(RoutineParameters::Inline(parameters)) => Some(parameters.clone()),
            _ => None,
        };
        let (inline, parameters_file) = match (local, &params.parameters) {
            (None, Some(RoutineParameters::File(file_id))) => (None, Some(*file_id)),
            (None, _) => (None, None),
            (Some(parameters), _) => {
                let attach = match params.attach_parameters_file {
                    Some(attach) => attach,
                    None => self.get(routine_id).await?.takes_files(),
                };
                let file_id = if attach {
                    Some(self.upload_parameters(&parameters).await?)
                } else {
                    None
                };
                (Some(parameters), file_id)
            }
        };
        let mut files = Vec::with_capacity(params.files.len() + 1);
        if let Some(id) = parameters_file {
            files.push(TaskFileRef {
                id,
                name: Some(ROUTINE_PARAMETERS_FILE_NAME.to_string()),
                size_bytes: None,
            });
        }
        files.extend(params.files.iter().cloned());
        let body = RoutineTriggerRequest {
            parameters: inline,
            files,
        };
        let path = format!("/v1/routines/{routine_id}/trigger");
        self.http.post_json(&path, &body).await
    }

    /// The same as [`Self::trigger`].
    pub async fn trigger_with(
        &self,
        routine_id: Uuid,
        params: &RoutineTriggerParams,
    ) -> ApiResult<RoutineTriggerResponse> {
        self.trigger(routine_id, params).await
    }

    async fn upload_parameters(
        &self,
        parameters: &serde_json::Map<String, serde_json::Value>,
    ) -> ApiResult<Uuid> {
        let content = serde_json::to_string(parameters).map_err(|err| {
            IntrospectionAPIError::Decode(format!("routine parameters must serialize: {err}"))
        })?;
        // A JSON create versions a file of the same name, so each run's is new.
        let body = FileCreateText {
            name: format!("routine-parameters/{}.json", Uuid::now_v7()),
            content,
            mime_type: Some("application/json".to_string()),
            ttl_seconds: Some(PARAMETERS_FILE_TTL_SECONDS),
            ..FileCreateText::default()
        };
        let file = Files::new(self.http.clone()).create_text(&body).await?;
        Ok(file.id)
    }
}

/// A local parameters file's JSON object.
async fn read_parameters(path: &Path) -> ApiResult<serde_json::Map<String, serde_json::Value>> {
    let bytes = tokio::fs::read(path).await?;
    match serde_json::from_slice(&bytes) {
        Ok(serde_json::Value::Object(parameters)) => Ok(parameters),
        Ok(_) => Err(IntrospectionAPIError::InvalidConfig(format!(
            "{}: routine parameters must be a JSON object",
            path.display()
        ))),
        Err(err) => Err(IntrospectionAPIError::InvalidConfig(format!(
            "{}: routine parameters are not valid JSON: {err}",
            path.display()
        ))),
    }
}
