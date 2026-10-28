//! `client.projects` (CP) — read the org's projects, and under each the
//! runtime peerings it offers or is granted ([`ProjectPeerings`]).
//!
//! Read-only. Creating, renaming or deleting a project is project
//! administration and lives in the CLI; reading one is what a tool needs to
//! turn a project id into the slug its dashboard URLs use. Writing a peering
//! needs `projects:write` held by a person, which no API key has, so it stays
//! in the web app.
//!
//! The project and peering calls need `projects:read`.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::encoding::encode;
use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Project, ProjectListParams, RuntimePeering, RuntimePeeringListParams, StringOrUuid,
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
