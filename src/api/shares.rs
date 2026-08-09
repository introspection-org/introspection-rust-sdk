//! Runner-bound read-sharing grants for files and conversations.

use std::sync::Arc;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{ResourceShare, ShareCreate, ShareListParams};

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

    /// Create a read grant for a file or conversation.
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
