//! Runner-bound sharing grants for files, conversations and issues.
//!
//! Shares apply automatically: a grantee reads the shared resource through its
//! ordinary routes, with no share id on the request.

use std::sync::Arc;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{ResourceShare, ShareCreate, ShareListParams, ShareUpdate};

#[derive(Clone)]
pub struct Shares {
    http: Arc<HttpClient>,
}

impl Shares {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// List grants visible to the current runner identity.
    pub fn list(&self, params: &ShareListParams) -> Paginator<ResourceShare> {
        Paginator::new(self.http.clone(), "/v1/shares", params)
            .expect("ShareListParams must serialize to a JSON object")
    }

    /// Grant a member, a tag cohort, or the whole project access to a file,
    /// conversation or issue.
    pub async fn create(&self, body: &ShareCreate) -> ApiResult<ResourceShare> {
        self.http.post_json("/v1/shares", body).await
    }

    /// Read one grant by ID.
    pub async fn get(&self, share_id: &str) -> ApiResult<ResourceShare> {
        self.http
            .get_json(
                &format!("/v1/shares/{}", crate::api::encoding::encode(share_id)),
                &(),
            )
            .await
    }

    /// Change a grant's mode or conversation visibility window. Only the
    /// grantor or an admin may update a share; anyone else gets 404.
    pub async fn update(&self, share_id: &str, body: &ShareUpdate) -> ApiResult<ResourceShare> {
        self.http
            .patch_json(
                &format!("/v1/shares/{}", crate::api::encoding::encode(share_id)),
                body,
            )
            .await
    }

    /// Revoke one grant by ID.
    pub async fn delete(&self, share_id: &str) -> ApiResult<()> {
        self.http
            .delete_empty(&format!(
                "/v1/shares/{}",
                crate::api::encoding::encode(share_id)
            ))
            .await
    }
}
