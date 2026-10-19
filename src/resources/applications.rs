//! `client.applications` (CP) — a project's OAuth clients, and under each the
//! identity providers it brokers ([`ApplicationIdps`]), its client secrets
//! ([`ApplicationSecrets`]) and its signing keys ([`ApplicationKeys`]).
//!
//! Gated on `applications:read` / `applications:write` /
//! `applications:delete`, which only an org credential carries.

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Application, ApplicationCreateParams, ApplicationIdp, ApplicationIdpCreateParams,
    ApplicationKey, ApplicationListParams, ApplicationSecret, ApplicationSecretCreateParams,
    ApplicationUpdateParams, PaginationParams, ProjectSelector, StringOrUuid,
};
use crate::resources::with_project;

/// `client.applications` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Applications {
    http: Arc<HttpClient>,
    /// `/v1/applications/{id}/idps`.
    pub idps: ApplicationIdps,
    /// `/v1/applications/{id}/secrets`.
    pub secrets: ApplicationSecrets,
    /// `/v1/applications/{id}/keys`.
    pub keys: ApplicationKeys,
}

impl Applications {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self {
            idps: ApplicationIdps { http: http.clone() },
            secrets: ApplicationSecrets { http: http.clone() },
            keys: ApplicationKeys { http: http.clone() },
            http,
        }
    }

    /// `GET /v1/applications` — paginated.
    pub fn list(&self, params: &ApplicationListParams) -> Paginator<Application> {
        Paginator::new(self.http.clone(), "/v1/applications", params)
            .expect("ApplicationListParams must serialize to a JSON object")
    }

    /// `GET /v1/applications/{id}`.
    pub async fn get(
        &self,
        application_id: Uuid,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Application> {
        let path = format!("/v1/applications/{application_id}");
        self.http
            .get_json(&path, &ProjectSelector::new(project))
            .await
    }

    /// `POST /v1/applications`.
    pub async fn create(
        &self,
        params: &ApplicationCreateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Application> {
        self.http
            .post_json(&with_project("/v1/applications", project.as_ref()), params)
            .await
    }

    /// `PATCH /v1/applications/{id}`.
    pub async fn update(
        &self,
        application_id: Uuid,
        params: &ApplicationUpdateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Application> {
        let path = with_project(
            &format!("/v1/applications/{application_id}"),
            project.as_ref(),
        );
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/applications/{id}`.
    pub async fn delete(
        &self,
        application_id: Uuid,
        project: Option<StringOrUuid>,
    ) -> ApiResult<()> {
        let path = with_project(
            &format!("/v1/applications/{application_id}"),
            project.as_ref(),
        );
        self.http.delete_empty(&path).await
    }
}

/// `client.applications().idps` — the identity providers an Application
/// brokers.
#[derive(Clone)]
pub struct ApplicationIdps {
    http: Arc<HttpClient>,
}

impl ApplicationIdps {
    /// `GET /v1/applications/{id}/idps` — paginated.
    pub fn list(
        &self,
        application_id: Uuid,
        params: &PaginationParams,
    ) -> Paginator<ApplicationIdp> {
        let path = format!("/v1/applications/{application_id}/idps");
        Paginator::new(self.http.clone(), &path, params)
            .expect("PaginationParams must serialize to a JSON object")
    }

    /// `POST /v1/applications/{id}/idps`.
    pub async fn create(
        &self,
        application_id: Uuid,
        params: &ApplicationIdpCreateParams,
    ) -> ApiResult<ApplicationIdp> {
        let path = format!("/v1/applications/{application_id}/idps");
        self.http.post_json(&path, params).await
    }

    /// `DELETE /v1/applications/{id}/idps/{idp_id}`.
    pub async fn delete(&self, application_id: Uuid, idp_id: Uuid) -> ApiResult<()> {
        let path = format!("/v1/applications/{application_id}/idps/{idp_id}");
        self.http.delete_empty(&path).await
    }
}

/// `client.applications().secrets` — a confidential Application's client
/// secrets.
#[derive(Clone)]
pub struct ApplicationSecrets {
    http: Arc<HttpClient>,
}

impl ApplicationSecrets {
    /// `GET /v1/applications/{id}/secrets` — paginated; no plaintext.
    pub fn list(
        &self,
        application_id: Uuid,
        params: &PaginationParams,
    ) -> Paginator<ApplicationSecret> {
        let path = format!("/v1/applications/{application_id}/secrets");
        Paginator::new(self.http.clone(), &path, params)
            .expect("PaginationParams must serialize to a JSON object")
    }

    /// `POST /v1/applications/{id}/secrets`. The plaintext is in
    /// [`ApplicationSecret::secret`] on this response only.
    pub async fn create(
        &self,
        application_id: Uuid,
        params: &ApplicationSecretCreateParams,
    ) -> ApiResult<ApplicationSecret> {
        let path = format!("/v1/applications/{application_id}/secrets");
        self.http.post_json(&path, params).await
    }

    /// `DELETE /v1/applications/{id}/secrets/{secret_id}` — disable now, or
    /// after `expires_in` seconds for a rotation.
    pub async fn delete(
        &self,
        application_id: Uuid,
        secret_id: Uuid,
        expires_in: Option<u64>,
    ) -> ApiResult<()> {
        let mut path = format!("/v1/applications/{application_id}/secrets/{secret_id}");
        if let Some(expires_in) = expires_in {
            path.push_str(&format!("?expires_in={expires_in}"));
        }
        self.http.delete_empty(&path).await
    }
}

/// `client.applications().keys` — a `jwks` Application's signing keys.
#[derive(Clone)]
pub struct ApplicationKeys {
    http: Arc<HttpClient>,
}

impl ApplicationKeys {
    /// `GET /v1/applications/{id}/keys` — unpaginated.
    pub async fn list(&self, application_id: Uuid) -> ApiResult<Vec<ApplicationKey>> {
        let path = format!("/v1/applications/{application_id}/keys");
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/applications/{id}/keys` — mint a key pair; the API keeps
    /// the private half.
    pub async fn create(&self, application_id: Uuid) -> ApiResult<ApplicationKey> {
        let path = format!("/v1/applications/{application_id}/keys");
        self.http.post_json(&path, &serde_json::Map::new()).await
    }

    /// `DELETE /v1/applications/{id}/keys/{key_id}`.
    pub async fn delete(&self, application_id: Uuid, key_id: Uuid) -> ApiResult<()> {
        let path = format!("/v1/applications/{application_id}/keys/{key_id}");
        self.http.delete_empty(&path).await
    }
}
