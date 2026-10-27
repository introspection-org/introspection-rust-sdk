//! `client.endpoints` (CP) — read the project's endpoint bindings, and with
//! the `authoring` feature create, update and delete them.
//!
//! Reading a binding is what a tool needs to reach the server it names;
//! binding a host, an MCP server or a model provider is project
//! administration.
//!
//! The reads need `endpoints:read` and the writes `endpoints:write` /
//! `endpoints:delete`, which only a member login or the project's service
//! account carries; a project API key gets 403.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Endpoint, EndpointListParams, StringOrUuid};
#[cfg(feature = "authoring")]
use crate::api::{
    error::IntrospectionAPIError,
    schemas::{EndpointCreateParams, EndpointUpdateParams, Secret},
};
#[cfg(feature = "authoring")]
use crate::resources::with_project;

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

    /// `POST /v1/endpoints[?project=...]` — create the binding, or update the
    /// one with the same host, runtime scope and environment. Needs
    /// `endpoints:write` and a member identity.
    ///
    /// A `verify_path` that does not answer 2xx fails with 422
    /// `endpoint_verify_failed`. An error has every header value replaced by
    /// `[redacted]`.
    #[cfg(feature = "authoring")]
    pub async fn create(
        &self,
        params: &EndpointCreateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Endpoint> {
        let path = with_project("/v1/endpoints", project.as_ref());
        self.http
            .post_json(&path, params)
            .await
            .map_err(|err| redact_header_values(err, Some(&params.headers)))
    }

    /// `PATCH /v1/endpoints/{id}[?project=...]`. Needs `endpoints:write`. An
    /// error has every header value replaced by `[redacted]`.
    #[cfg(feature = "authoring")]
    pub async fn update(
        &self,
        endpoint_id: Uuid,
        params: &EndpointUpdateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Endpoint> {
        let path = with_project(&format!("/v1/endpoints/{endpoint_id}"), project.as_ref());
        self.http
            .patch_json(&path, params)
            .await
            .map_err(|err| redact_header_values(err, params.headers.as_ref()))
    }

    /// `DELETE /v1/endpoints/{id}[?project=...]` — soft-delete. Needs
    /// `endpoints:delete`. A new binding for the same host, scope and
    /// environment can be created afterwards.
    #[cfg(feature = "authoring")]
    pub async fn delete(&self, endpoint_id: Uuid, project: Option<StringOrUuid>) -> ApiResult<()> {
        let path = with_project(&format!("/v1/endpoints/{endpoint_id}"), project.as_ref());
        self.http.delete_empty(&path).await
    }
}

/// `err` with each of `headers`' values replaced by `[redacted]`: a
/// validation error echoes the request body it refused.
#[cfg(feature = "authoring")]
fn redact_header_values(
    err: IntrospectionAPIError,
    headers: Option<&std::collections::BTreeMap<String, Secret>>,
) -> IntrospectionAPIError {
    let mut values: Vec<&str> = headers
        .into_iter()
        .flat_map(|headers| headers.values())
        .map(Secret::expose)
        .filter(|value| !value.is_empty())
        .collect();
    if values.is_empty() {
        return err;
    }
    // Longest first, so a value inside another is not left half-replaced.
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    let redact = |text: &str| {
        values.iter().fold(text.to_string(), |text, value| {
            text.replace(value, "[redacted]")
        })
    };
    match err {
        IntrospectionAPIError::Http {
            message,
            status,
            code,
            request_id,
            body,
            retry_after,
        } => IntrospectionAPIError::Http {
            message: redact(&message),
            status,
            code,
            request_id,
            body: body.map(|body| redact_json(body, &redact)),
            retry_after,
        },
        IntrospectionAPIError::Decode(message) => IntrospectionAPIError::Decode(redact(&message)),
        other => other,
    }
}

#[cfg(feature = "authoring")]
fn redact_json(value: serde_json::Value, redact: &dyn Fn(&str) -> String) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(text) => Value::String(redact(&text)),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|v| redact_json(v, redact)).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, v)| (key, redact_json(v, redact)))
                .collect(),
        ),
        other => other,
    }
}
