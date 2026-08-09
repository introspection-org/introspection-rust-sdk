//! `client.connectors` (CP) — connectors, their connections, and the
//! consent URL a Business hands its customer.
//!
//! A connector is the org-level definition of a provider (Slack, Gmail,
//! Stripe): endpoints, client credentials, requested scopes. A connection is
//! one authorized subject underneath it — a Slack workspace that installed
//! the app, a person, an app credential — so connections are nested here
//! rather than standing on their own.
//!
//! [`Connectors::authorize`] is the reason this namespace exists for an
//! integrator: it mints the install link their own backend puts in front of
//! their own customer, without anyone touching our UI.
//!
//! `client_secret` / `signing_secret` are write-only — accepted on create and
//! update, absent from every response — so they appear on the param structs
//! and never on [`Connector`]. Omitting one on update leaves it as it is.
//!
//! The whole family sits behind a server-side feature flag; when it is off
//! every route answers 404 with detail "Connectors are not enabled", which
//! surfaces as the error message rather than a bare not-found.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Connection, ConnectionCreateParams, Connector, ConnectorAuthorization,
    ConnectorAuthorizeParams, ConnectorCreateParams, ConnectorListParams, ConnectorUpdateParams,
    PaginationParams, StringOrUuid,
};

/// `client.connectors.connections` — the authorized subjects under one
/// connector (`/v1/connectors/{connector_id}/connections`).
///
/// Every method takes the connector id first: a connection is addressed
/// through its connector, and is meaningless without it. There is no PATCH,
/// and the platform never serializes access or refresh tokens.
#[derive(Clone)]
pub struct Connections {
    http: Arc<HttpClient>,
}

impl Connections {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/connectors/{connector_id}/connections` — paginated.
    pub fn list(&self, connector_id: Uuid, params: &PaginationParams) -> Paginator<Connection> {
        let path = format!("/v1/connectors/{}/connections", connector_id);
        Paginator::new(self.http.clone(), path, params)
            .expect("PaginationParams must serialize to a JSON object")
    }

    /// `POST /v1/connectors/{connector_id}/connections` — registered mode:
    /// store a token the caller already holds.
    ///
    /// For the consent flow use [`Connectors::authorize`] instead; its
    /// callback creates the connection server-side.
    pub async fn create(
        &self,
        connector_id: Uuid,
        params: &ConnectionCreateParams,
    ) -> ApiResult<Connection> {
        let path = format!("/v1/connectors/{}/connections", connector_id);
        self.http.post_json(&path, params).await
    }

    /// `GET /v1/connectors/{connector_id}/connections/{connection_id}`.
    pub async fn get(&self, connector_id: Uuid, connection_id: Uuid) -> ApiResult<Connection> {
        #[derive(Serialize)]
        struct Q {}
        let path = format!(
            "/v1/connectors/{}/connections/{}",
            connector_id, connection_id
        );
        self.http.get_json(&path, &Q {}).await
    }

    /// `DELETE /v1/connectors/{connector_id}/connections/{connection_id}`.
    ///
    /// Named `revoke` rather than `delete` because it destroys the provider
    /// token behind the connection: the subject must consent again through a
    /// fresh install link. The connector and its other connections are
    /// untouched.
    pub async fn revoke(&self, connector_id: Uuid, connection_id: Uuid) -> ApiResult<()> {
        let path = format!(
            "/v1/connectors/{}/connections/{}",
            connector_id, connection_id
        );
        self.http.delete_empty(&path).await
    }
}

/// `client.connectors` namespace. Holds a CP-bound HTTP client, with
/// [`Connections`] nested under `.connections`.
#[derive(Clone)]
pub struct Connectors {
    http: Arc<HttpClient>,
    /// Nested `connections` namespace.
    pub connections: Connections,
}

impl Connectors {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        let connections = Connections::new(http.clone());
        Self { http, connections }
    }

    /// `GET /v1/connectors` — paginated.
    pub fn list(&self, params: &ConnectorListParams) -> Paginator<Connector> {
        Paginator::new(self.http.clone(), "/v1/connectors", params)
            .expect("ConnectorListParams must serialize to a JSON object")
    }

    /// `POST /v1/connectors?project=…` — create, idempotent on `slug`.
    ///
    /// A repeat POST with the same slug returns the live row rather than
    /// duplicating it.
    pub async fn create(
        &self,
        params: &ConnectorCreateParams,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Connector> {
        let path = format!("/v1/connectors?project={}", encode_project(project));
        self.http.post_json(&path, params).await
    }

    /// `GET /v1/connectors/{id}?project=…`.
    pub async fn get(
        &self,
        connector_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Connector> {
        #[derive(Serialize)]
        struct Q {
            project: StringOrUuid,
        }
        let path = format!("/v1/connectors/{}", connector_id);
        self.http
            .get_json(
                &path,
                &Q {
                    project: project.into(),
                },
            )
            .await
    }

    /// `PATCH /v1/connectors/{id}?project=…` — partial update.
    ///
    /// Only the fields set on `params` change. Leaving `client_secret` /
    /// `signing_secret` unset means "unchanged", never "clear".
    pub async fn update(
        &self,
        connector_id: Uuid,
        params: &ConnectorUpdateParams,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Connector> {
        let path = format!(
            "/v1/connectors/{}?project={}",
            connector_id,
            encode_project(project)
        );
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/connectors/{id}?project=…` — soft delete.
    ///
    /// The server revokes the connector's connections as it goes.
    pub async fn delete(
        &self,
        connector_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<()> {
        let path = format!(
            "/v1/connectors/{}?project={}",
            connector_id,
            encode_project(project)
        );
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
    pub async fn authorize(
        &self,
        connector_id: Uuid,
        params: &ConnectorAuthorizeParams,
    ) -> ApiResult<ConnectorAuthorization> {
        // The route takes `connector_id` in the body; the SDK takes it as an
        // argument, so merge it into the serialized params.
        let mut body = serde_json::to_value(params).map_err(|err| {
            IntrospectionAPIError::Decode(format!(
                "ConnectorAuthorizeParams must serialize to a JSON object: {err}"
            ))
        })?;
        let map = body.as_object_mut().ok_or_else(|| {
            IntrospectionAPIError::Decode(
                "ConnectorAuthorizeParams must serialize to a JSON object".to_string(),
            )
        })?;
        map.insert(
            "connector_id".to_string(),
            serde_json::Value::String(connector_id.to_string()),
        );
        self.http
            .post_json("/v1/oauth/connections/authorize", &body)
            .await
    }
}

/// Percent-encode a project selector for the query string. Slugs and uuids
/// are URL-safe in practice, but the value is caller-supplied.
fn encode_project(project: impl Into<StringOrUuid>) -> String {
    urlencode(&project.into().to_string())
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_project_passes_slugs_and_uuids_through() {
        assert_eq!(encode_project("proj-1"), "proj-1");
        let id = Uuid::nil();
        assert_eq!(encode_project(id), id.to_string());
    }

    #[test]
    fn encode_project_escapes_separators() {
        assert_eq!(encode_project("a b/c&d"), "a%20b%2Fc%26d");
    }
}
