//! `client.endpoints` (CP) — read the project's endpoint bindings.
//!
//! Read-only. Binding a host, an MCP server or a model provider is project
//! administration and lives in the CLI; reading a binding is what a tool
//! needs to reach the server it names.
//!
//! Both calls need `endpoints:read`, which only a member login carries; a
//! project API key gets 403.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Endpoint, EndpointListParams, StringOrUuid};

/// `client.endpoints` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Endpoints {
    http: Arc<HttpClient>,
}

impl Endpoints {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/endpoints` — paginated.
    pub fn list(&self, params: &EndpointListParams) -> Paginator<Endpoint> {
        Paginator::new(self.http.clone(), "/v1/endpoints", params)
            .expect("EndpointListParams must serialize to a JSON object")
    }

    /// `GET /v1/endpoints/{id}`. `project` is needed only when the credential
    /// does not already name one.
    pub async fn get(
        &self,
        endpoint_id: Uuid,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Endpoint> {
        #[derive(Serialize)]
        struct Q {
            #[serde(skip_serializing_if = "Option::is_none")]
            project: Option<StringOrUuid>,
        }
        let path = format!("/v1/endpoints/{}", endpoint_id);
        self.http.get_json(&path, &Q { project }).await
    }
}
