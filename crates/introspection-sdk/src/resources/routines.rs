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

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Routine, RoutineCreateParams, RoutineListParams, RoutineTriggerParams, RoutineTriggerResponse,
    RoutineUpdateParams,
};

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
    pub async fn trigger(&self, routine_id: Uuid) -> ApiResult<RoutineTriggerResponse> {
        self.trigger_with(routine_id, &RoutineTriggerParams::default())
            .await
    }

    /// `POST /v1/routines/{id}/trigger` with a body: run it now on the
    /// given `conversation_ids` (at most 100). Only an
    /// [`RoutineKind::ObservationSynthesis`](crate::RoutineKind::ObservationSynthesis)
    /// routine takes them; any other kind answers `422`. An unreadable
    /// conversation answers `404`, one whose turn is still running `409`.
    /// Default params send `{}`, like [`Self::trigger`].
    pub async fn trigger_with(
        &self,
        routine_id: Uuid,
        params: &RoutineTriggerParams,
    ) -> ApiResult<RoutineTriggerResponse> {
        let path = format!("/v1/routines/{routine_id}/trigger");
        self.http.post_json(&path, params).await
    }
}
