//! `client.projects` (CP) — the org's projects, and under each the runtime
//! peerings it offers or is granted ([`ProjectPeerings`]).
//!
//! Reading one is what a tool needs to turn a project id into the slug its
//! dashboard URLs use; the project and peering reads need `projects:read`.
//! Creating, updating and deleting a project need `projects:write`, which only
//! an owner's or admin's member session or access token carries: no API key,
//! Application or CLI login has it. Peerings are written in the web app.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::encoding::encode;
use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Project, ProjectCreateParams, ProjectListParams, ProjectUpdateParams, RuntimePeering,
    RuntimePeeringListParams, StringOrUuid,
};

/// `client.projects` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Projects {
    http: Arc<HttpClient>,
    /// `/v1/projects/{project}/peerings`.
    pub peerings: ProjectPeerings,
}

impl Projects {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self {
            peerings: ProjectPeerings { http: http.clone() },
            http,
        }
    }

    /// `GET /v1/projects` — paginated, the credential's org only.
    pub fn list(&self, params: &ProjectListParams) -> Paginator<Project> {
        Paginator::new(self.http.clone(), "/v1/projects", params)
            .expect("ProjectListParams must serialize to a JSON object")
    }

    /// `GET /v1/projects/{project}`, by slug or id. A project outside the
    /// credential's org answers 404.
    pub async fn get(&self, project: impl Into<StringOrUuid>) -> ApiResult<Project> {
        #[derive(Serialize)]
        struct Q {}
        let path = format!("/v1/projects/{}", encode(&project.into().to_string()));
        self.http.get_json(&path, &Q {}).await
    }

    /// `POST /v1/projects` — create a project on a deployment. A slug or
    /// name already taken answers 409, and the org's project limit 403.
    pub async fn create(&self, params: &ProjectCreateParams) -> ApiResult<Project> {
        self.http.post_json("/v1/projects", params).await
    }

    /// `PATCH /v1/projects/{project}`, by slug or id — only the fields set on
    /// `params` change.
    pub async fn update(
        &self,
        project: impl Into<StringOrUuid>,
        params: &ProjectUpdateParams,
    ) -> ApiResult<Project> {
        let path = format!("/v1/projects/{}", encode(&project.into().to_string()));
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/projects/{project}`, by slug or id, with everything in it.
    pub async fn delete(&self, project: impl Into<StringOrUuid>) -> ApiResult<()> {
        let path = format!("/v1/projects/{}", encode(&project.into().to_string()));
        self.http.delete_empty(&path).await
    }
}

/// `client.projects().peerings` — the runtime peerings a project offers, or
/// those other projects grant to it.
#[derive(Clone)]
pub struct ProjectPeerings {
    http: Arc<HttpClient>,
}

impl ProjectPeerings {
    /// `GET /v1/projects/{project}/peerings` — paginated. `project` is a slug
    /// or id; [`RuntimePeeringListParams::runtime`] narrows the list to one
    /// runtime group and [`RuntimePeeringListParams::granted`] lists the
    /// peerings granted to the project instead of those it offers.
    pub fn list(
        &self,
        project: impl Into<StringOrUuid>,
        params: &RuntimePeeringListParams,
    ) -> Paginator<RuntimePeering> {
        let path = format!(
            "/v1/projects/{}/peerings",
            encode(&project.into().to_string())
        );
        Paginator::new(self.http.clone(), &path, params)
            .expect("RuntimePeeringListParams must serialize to a JSON object")
    }

    /// `GET /v1/projects/{project}/peerings/{peering_id}` — a peering the
    /// project offers or one that admits it; 404 otherwise.
    pub async fn get(
        &self,
        project: impl Into<StringOrUuid>,
        peering_id: Uuid,
    ) -> ApiResult<RuntimePeering> {
        #[derive(Serialize)]
        struct Q {}
        let path = format!(
            "/v1/projects/{}/peerings/{peering_id}",
            encode(&project.into().to_string())
        );
        self.http.get_json(&path, &Q {}).await
    }
}
