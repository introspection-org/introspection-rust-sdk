//! `client.connectors` (CP) — connectors, the consent URL a Business hands
//! its customer, and the token broker for a connector's connections.
//!
//! A connector is the org-level definition of a provider (Slack, Gmail,
//! Stripe): endpoints, client credentials, requested scopes. A connection is
//! one authorized subject underneath it — a Slack workspace that installed
//! the app, a person, an app credential — read and removed through the data
//! plane's `client.connections()`.
//!
//! [`Connectors::authorize`] is the reason this namespace exists for an
//! integrator: it mints the install link their own backend puts in front of
//! their own customer, without anyone touching our UI.
//!
//! `client_secret` / `signing_secret` are write-only — accepted on create and
//! update, absent from every response — so they appear on the param structs
//! and never on [`Connector`]. Omitting one on update leaves it as it is.
//!
//! Project scope comes from the authenticated credential. A session or an
//! org-wide token names the project instead: the connector methods take a
//! trailing `project` selector, [`ConnectorListParams::project`] does the
//! same for the list, and the token broker reads
//! [`ConnectionTokenParams::project`].

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    ConnectionTokenParams, ConnectionTokenResult, Connector, ConnectorApp, ConnectorAppListParams,
    ConnectorAuthorization, ConnectorAuthorizeParams, ConnectorCreateParams,
    ConnectorCustomAppSearchParams, ConnectorListParams, ConnectorOAuthDiscovery,
    ConnectorOAuthDiscoveryParams, ConnectorUpdateParams, ProjectSelector, StringOrUuid,
};
use crate::resources::with_project;

/// `client.connectors` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Connectors {
    http: Arc<HttpClient>,
}

impl Connectors {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `POST /v1/oauth/connections/token` — resolve a provider credential
    /// for one of the connector's connections. A person-authorized connector
    /// may instead return a pending mission and approval URL; callers must
    /// handle both [`ConnectionTokenResult`] arms.
    pub async fn connection_token(
        &self,
        connector_id: Uuid,
        params: &ConnectionTokenParams,
    ) -> ApiResult<ConnectionTokenResult> {
        let body = with_id(
            params,
            "ConnectionTokenParams",
            "connector_id",
            connector_id,
        )?;
        let path = with_project("/v1/oauth/connections/token", params.project.as_ref());
        self.http.post_json(&path, &body).await
    }

    /// `GET /v1/connectors` — paginated.
    pub fn list(&self, params: &ConnectorListParams) -> Paginator<Connector> {
        Paginator::new(self.http.clone(), "/v1/connectors", params)
            .expect("ConnectorListParams must serialize to a JSON object")
    }

    /// `POST /v1/connectors` — create, upserting on `(project, slug)`.
    ///
    /// A repeat POST with the same slug does not duplicate the connector: it
    /// replaces the live row's configuration (name, environment, endpoints,
    /// scopes, API hosts, client id, metadata, ...) and keeps its provider,
    /// auth mode and stored secrets.
    ///
    /// With only `issuer` set, the server discovers the endpoints and
    /// registers an OAuth client on its own; pass the `client_id` /
    /// `client_secret` from [`Self::discover_oauth`] when you already ran
    /// discovery, so a second client is not registered.
    pub async fn create(
        &self,
        params: &ConnectorCreateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Connector> {
        self.http
            .post_json(&with_project("/v1/connectors", project.as_ref()), params)
            .await
    }

    /// `GET /v1/connectors/{id}`.
    pub async fn get(
        &self,
        connector_id: Uuid,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Connector> {
        let path = format!("/v1/connectors/{}", connector_id);
        self.http
            .get_json(&path, &ProjectSelector::new(project))
            .await
    }

    /// `GET /v1/connectors/{id}/apps` — search the provider application
    /// catalogue (currently available for Pipedream connectors).
    pub async fn list_apps(
        &self,
        connector_id: Uuid,
        params: &ConnectorAppListParams,
    ) -> ApiResult<Vec<ConnectorApp>> {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Response {
            data: Vec<ConnectorApp>,
        }

        let path = format!("/v1/connectors/{}/apps", connector_id);
        let response: Response = self.http.get_json(&path, params).await?;
        Ok(response.data)
    }

    /// `GET /v1/connectors/custom/apps` — search the open MCP registry, the
    /// catalogue a custom connector picks from.
    ///
    /// Unlike [`Self::list_apps`] this takes no connector: it is read before
    /// any connector exists. A listing's `mcp_url` is what to hand
    /// [`Self::discover_oauth`] and [`Self::create`] as `issuer`.
    pub async fn search_custom_apps(
        &self,
        params: &ConnectorCustomAppSearchParams,
    ) -> ApiResult<Vec<ConnectorApp>> {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Response {
            data: Vec<ConnectorApp>,
        }

        let response: Response = self
            .http
            .get_json("/v1/connectors/custom/apps", params)
            .await?;
        Ok(response.data)
    }

    /// `POST /v1/connectors/discover-oauth` — resolve a provider's OAuth
    /// metadata before creating a custom connector.
    ///
    /// `issuer` may be the authorization server or the MCP server it
    /// protects. This is not a pure read: when the provider supports it, the
    /// server **registers an OAuth client** (dynamic registration) and returns
    /// its `client_id` / `client_secret`. Pass those to [`Self::create`] so a
    /// second client is not registered. Answers 400 when discovery fails.
    pub async fn discover_oauth(
        &self,
        params: &ConnectorOAuthDiscoveryParams,
    ) -> ApiResult<ConnectorOAuthDiscovery> {
        self.http
            .post_json("/v1/connectors/discover-oauth", params)
            .await
    }

    /// `PATCH /v1/connectors/{id}` — partial update.
    ///
    /// Only the fields set on `params` change. Leaving `client_secret` /
    /// `signing_secret` unset means "unchanged", never "clear".
    pub async fn update(
        &self,
        connector_id: Uuid,
        params: &ConnectorUpdateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Connector> {
        let path = with_project(&format!("/v1/connectors/{connector_id}"), project.as_ref());
        self.http.patch_json(&path, params).await
    }

    /// `PATCH /v1/connectors/{id}` with `rotate_signing_secret`: issue a
    /// `webhook` connector a new secret, returned once in
    /// [`Connector::signing_secret`]. The previous one is accepted until
    /// [`Connector::previous_signing_secret_expires_at`]. Any other provider
    /// answers `422`.
    pub async fn rotate_signing_secret(
        &self,
        connector_id: Uuid,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Connector> {
        let params = ConnectorUpdateParams {
            rotate_signing_secret: Some(true),
            ..Default::default()
        };
        self.update(connector_id, &params, project).await
    }

    /// `DELETE /v1/connectors/{id}` — soft delete.
    ///
    /// The server revokes the connector's connections as it goes.
    pub async fn delete(&self, connector_id: Uuid, project: Option<StringOrUuid>) -> ApiResult<()> {
        let path = with_project(&format!("/v1/connectors/{connector_id}"), project.as_ref());
        self.http.delete_empty(&path).await
    }

    /// `POST /v1/oauth/connections/authorize` — mint the consent URL.
    ///
    /// This is the install link: the URL a Business puts in front of its own
    /// customer so that customer's workspace connects to an agent. It is
    /// presented here as a connector operation even though the route lives in
    /// the `/v1/oauth/` family.
    ///
    /// Each call writes a fresh single-use `state`, so two calls return two
    /// different URLs and no response may be cached. Raise
    /// [`ConnectorAuthorizeParams::expires_in`] (60–86400 seconds, default
    /// 600) when handing the link to someone else to open later rather than
    /// following it immediately.
    ///
    /// A connector whose [`Connector::requires_runtime`] is true answers 422
    /// unless `runtime` names the agent that replies — read that field rather
    /// than hardcoding which providers are chat providers.
    ///
    /// Setting [`ConnectorAuthorizeParams::identity`] mints a `customer`
    /// member for the asserted end user, so it can answer 409 when the org has
    /// reached its member limit — a plan conflict, not back-pressure.
    ///
    /// Setting [`ConnectorAuthorizeParams::binding`] (with `runtime`) makes a
    /// successful grant also write the runtime's MCP endpoint binding, in the
    /// same transaction as the connection.
    pub async fn authorize(
        &self,
        connector_id: Uuid,
        params: &ConnectorAuthorizeParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<ConnectorAuthorization> {
        // The route takes `connector_id` in the body; the SDK takes it as an
        // argument.
        let body = with_id(
            params,
            "ConnectorAuthorizeParams",
            "connector_id",
            connector_id,
        )?;
        let path = with_project("/v1/oauth/connections/authorize", project.as_ref());
        self.http.post_json(&path, &body).await
    }

    /// `POST /v1/oauth/connections/authorize` for an organization
    /// integration (Google Workspace, Microsoft 365) instead of a
    /// connector: the consent URL that installs it. Needs
    /// `integrations:write`.
    pub async fn authorize_integration(
        &self,
        integration_id: Uuid,
        params: &ConnectorAuthorizeParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<ConnectorAuthorization> {
        let body = with_id(
            params,
            "ConnectorAuthorizeParams",
            "integration_id",
            integration_id,
        )?;
        let path = with_project("/v1/oauth/connections/authorize", project.as_ref());
        self.http.post_json(&path, &body).await
    }
}

/// `params` as a JSON object with `key` set to `id`: a route that takes in
/// its body an id the SDK takes as an argument.
fn with_id<T: serde::Serialize>(
    params: &T,
    type_name: &str,
    key: &str,
    id: Uuid,
) -> ApiResult<serde_json::Value> {
    let mut body = serde_json::to_value(params).map_err(|err| {
        IntrospectionAPIError::Decode(format!(
            "{type_name} must serialize to a JSON object: {err}"
        ))
    })?;
    let map = body.as_object_mut().ok_or_else(|| {
        IntrospectionAPIError::Decode(format!("{type_name} must serialize to a JSON object"))
    })?;
    map.insert(key.to_string(), serde_json::Value::String(id.to_string()));
    Ok(body)
}
