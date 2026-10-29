//! `client.api_keys` (CP) — a project's API keys.
//!
//! Gated on `api_keys:read` / `api_keys:write` / `api_keys:delete`, which only
//! an org credential (a member session or access token) carries; a project API
//! key cannot manage keys.

use std::sync::Arc;

use serde::Deserialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::schemas::{
    ApiKey, ApiKeyAcknowledgement, ApiKeyCreateParams, ApiKeyCreated, ApiKeyDeleteParams,
    ApiKeyListParams, ApiKeyUpdateParams, StringOrUuid,
};
use crate::resources::with_project;

/// `client.api_keys` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct ApiKeys {
    http: Arc<HttpClient>,
}

impl ApiKeys {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/api-keys` — every key of the project, unpaginated.
    pub async fn list(&self, params: &ApiKeyListParams) -> ApiResult<Vec<ApiKey>> {
        #[derive(Deserialize, serde::Serialize)]
        struct Page {
            api_keys: Vec<ApiKey>,
        }
        let page: Page = self.http.get_json("/v1/api-keys", params).await?;
        Ok(page.api_keys)
    }

    /// `POST /v1/api-keys`. The plaintext key is in
    /// [`ApiKeyCreated::api_key`] and is never returned again; save it, then
    /// call [`Self::acknowledge`].
    pub async fn create(
        &self,
        params: &ApiKeyCreateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<ApiKeyCreated> {
        self.http
            .post_json(&with_project("/v1/api-keys", project.as_ref()), params)
            .await
    }

    /// `PATCH /v1/api-keys/{id}`.
    pub async fn update(
        &self,
        api_key_id: Uuid,
        params: &ApiKeyUpdateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<ApiKey> {
        let path = with_project(&format!("/v1/api-keys/{api_key_id}"), project.as_ref());
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/api-keys/{id}` — revoke now, or after
    /// [`ApiKeyDeleteParams::expires_in`] seconds for a rotation.
    pub async fn delete(&self, api_key_id: Uuid, params: &ApiKeyDeleteParams) -> ApiResult<()> {
        let mut path = with_project(
            &format!("/v1/api-keys/{api_key_id}"),
            params.project.as_ref(),
        );
        if let Some(expires_in) = params.expires_in {
            let separator = if path.contains('?') { '&' } else { '?' };
            path.push_str(&format!("{separator}expires_in={expires_in}"));
        }
        self.http.delete_empty(&path).await
    }

    /// `POST /v1/api-keys/{id}/acknowledge` — record that the plaintext from
    /// [`Self::create`] was saved. A second acknowledge answers 404.
    pub async fn acknowledge(
        &self,
        api_key_id: Uuid,
        project: Option<StringOrUuid>,
    ) -> ApiResult<ApiKeyAcknowledgement> {
        let path = with_project(
            &format!("/v1/api-keys/{api_key_id}/acknowledge"),
            project.as_ref(),
        );
        self.http.post_json(&path, &serde_json::Map::new()).await
    }
}
