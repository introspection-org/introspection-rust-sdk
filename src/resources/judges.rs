//! `client.judges` (CP) — the judges a project's Recipes declare.
//!
//! A judge is authored in its Recipe and published with it, so there is no
//! create or delete here; an update changes only whether it runs and how
//! often. Gated on `judges:read` / `judges:write`.

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Judge, JudgeListParams, JudgeUpdateParams, ProjectSelector, StringOrUuid,
};
use crate::resources::with_project;

/// `client.judges` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Judges {
    http: Arc<HttpClient>,
}

impl Judges {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/judges` — paginated.
    pub fn list(&self, params: &JudgeListParams) -> Paginator<Judge> {
        Paginator::new(self.http.clone(), "/v1/judges", params)
            .expect("JudgeListParams must serialize to a JSON object")
    }

    /// `GET /v1/judges/{id}`.
    pub async fn get(&self, judge_id: Uuid, project: Option<StringOrUuid>) -> ApiResult<Judge> {
        let path = format!("/v1/judges/{judge_id}");
        self.http
            .get_json(&path, &ProjectSelector::new(project))
            .await
    }

    /// `PATCH /v1/judges/{id}`.
    pub async fn update(
        &self,
        judge_id: Uuid,
        params: &JudgeUpdateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Judge> {
        let path = with_project(&format!("/v1/judges/{judge_id}"), project.as_ref());
        self.http.patch_json(&path, params).await
    }
}
