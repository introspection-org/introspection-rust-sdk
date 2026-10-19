//! `connections()` (DP) — the apps (Gmail, Slack, …) members connected for
//! themselves, at `/v1/connections`. The agent acts with a member's
//! connections in that member's sessions.
//!
//! These are distinct from a connector's connections ([`crate::Connections`],
//! `/v1/connectors/{id}/connections`), which an integrator administers for the
//! project.
//!
//! Data Plane routes gated on `connections:read`, `connections:write` and
//! `connections:delete`, which a runner a member opens for themself carries.
//! A caller who is not an administrator only ever lists, reads and removes
//! their own connections.

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    ConnectPage, MemberConnection, MemberConnectionCreate, MemberConnectionListParams, StringOrUuid,
};

/// `connections()` namespace. Holds a DP-bound HTTP client and, on a runner,
/// the runner's runtime group, which [`Self::create`] defaults to.
#[derive(Clone)]
pub struct MemberConnections {
    http: Arc<HttpClient>,
    default_runtime_group_id: Option<Uuid>,
}

impl MemberConnections {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>, default_runtime_group_id: Option<Uuid>) -> Self {
        Self {
            http,
            default_runtime_group_id,
        }
    }

    /// `GET /v1/connections` — paginated.
    pub fn list(&self, params: &MemberConnectionListParams) -> Paginator<MemberConnection> {
        Paginator::new(self.http.clone(), "/v1/connections", params)
            .expect("MemberConnectionListParams must serialize to a JSON object")
    }

    /// `POST /v1/connections` — a connect page for `params.app`, for the
    /// caller themself. Hand the member [`ConnectPage::authorize_url`].
    ///
    /// On a runner, `params.runtime: None` connects the app for the runner's
    /// runtime group. On the client, or a runner whose context has no runtime
    /// group, `None` answers [`IntrospectionAPIError::InvalidConfig`] without
    /// a request.
    pub async fn create(&self, params: &MemberConnectionCreate) -> ApiResult<ConnectPage> {
        if params.runtime.is_some() {
            return self.http.post_json("/v1/connections", params).await;
        }
        let Some(runtime_group_id) = self.default_runtime_group_id else {
            return Err(IntrospectionAPIError::InvalidConfig(
                "MemberConnectionCreate::runtime is required: there is no runner runtime group to default to"
                    .to_string(),
            ));
        };
        let params = MemberConnectionCreate {
            runtime: Some(StringOrUuid::Uuid(runtime_group_id)),
            ..params.clone()
        };
        self.http.post_json("/v1/connections", &params).await
    }

    /// `GET /v1/connections/{id}`.
    pub async fn get(&self, connection_id: Uuid) -> ApiResult<MemberConnection> {
        let path = format!("/v1/connections/{connection_id}");
        self.http.get_json(&path, &()).await
    }

    /// `DELETE /v1/connections/{id}` — remove the connection.
    pub async fn delete(&self, connection_id: Uuid) -> ApiResult<()> {
        let path = format!("/v1/connections/{connection_id}");
        self.http.delete_empty(&path).await
    }
}
