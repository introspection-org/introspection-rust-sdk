//! `client.automations()` (DP) — scheduled prompts and platform work on a
//! project.
//!
//! An automation either runs a prompt as an agent task (a person's own
//! automation, [`Automation::kind`] `None`) or runs platform work (a
//! [`AutomationKind`]). Set `task_id` to post each firing into an existing
//! task instead of creating one; a one-off reminder is a
//! [`AutomationTriggerType::Manual`] automation with a future
//! `next_trigger_at`. Each firing of an automation that runs as a task is
//! recorded as an [`IntrospectionEventName::AutomationTriggered`] or
//! [`IntrospectionEventName::AutomationSkipped`] event, read through
//! [`crate::Events`] with the `automation_id` and `task_id` filters.
//!
//! These are Data Plane routes, gated on `automations:read` /
//! `automations:write`. The API serves them to administrators only today and
//! answers 403 to anyone else. introspection-cloud#3137 opens them to members
//! for their own automations that post into one of their own tasks; until it
//! ships, `AutomationListParams::task_id` is not served either.
//!
//! [`AutomationKind`]: crate::AutomationKind
//! [`AutomationTriggerType::Manual`]: crate::AutomationTriggerType::Manual
//! [`IntrospectionEventName::AutomationTriggered`]: crate::IntrospectionEventName::AutomationTriggered
//! [`IntrospectionEventName::AutomationSkipped`]: crate::IntrospectionEventName::AutomationSkipped

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Automation, AutomationCreateParams, AutomationListParams, AutomationTriggerResponse,
    AutomationUpdateParams,
};

/// `client.automations()` namespace. Holds a DP-bound HTTP client.
#[derive(Clone)]
pub struct Automations {
    http: Arc<HttpClient>,
}

impl Automations {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/automations` — paginated.
    pub fn list(&self, params: &AutomationListParams) -> Paginator<Automation> {
        Paginator::new(self.http.clone(), "/v1/automations", params)
            .expect("AutomationListParams must serialize to a JSON object")
    }

    /// `GET /v1/automations/{id}`. A soft-deleted automation is still
    /// returned (the read model does not say it was deleted).
    pub async fn get(&self, automation_id: Uuid) -> ApiResult<Automation> {
        let path = format!("/v1/automations/{automation_id}");
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/automations`.
    pub async fn create(&self, params: &AutomationCreateParams) -> ApiResult<Automation> {
        self.http.post_json("/v1/automations", params).await
    }

    /// `PATCH /v1/automations/{id}` — only the fields set on `params` are
    /// sent.
    pub async fn update(
        &self,
        automation_id: Uuid,
        params: &AutomationUpdateParams,
    ) -> ApiResult<Automation> {
        let path = format!("/v1/automations/{automation_id}");
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/automations/{id}` — soft delete. A platform default answers
    /// 409; disable it instead.
    pub async fn delete(&self, automation_id: Uuid) -> ApiResult<()> {
        let path = format!("/v1/automations/{automation_id}");
        self.http.delete_empty(&path).await
    }

    /// `POST /v1/automations/{id}/trigger` — run it now. Answers with the task
    /// it created or posted into; neither reads nor clears a scheduled slot.
    pub async fn trigger(&self, automation_id: Uuid) -> ApiResult<AutomationTriggerResponse> {
        let path = format!("/v1/automations/{automation_id}/trigger");
        self.http.post_json(&path, &serde_json::json!({})).await
    }
}
