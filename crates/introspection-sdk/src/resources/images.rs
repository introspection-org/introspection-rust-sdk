//! `images()` (DP) — what an eval task's compose services run as.
//!
//! An image is built from a context archive uploaded through the Files API, or
//! mirrored from an upstream reference pinned by digest, and always lands in
//! the plane's registry. A create is idempotent on content: the same source
//! returns the existing row, and resending it retries a failed build. Its
//! `status` is the only progress report, so poll [`Images::get`] until it is
//! `ready` (or `failed`).
//!
//! Gated on `images:read` / `images:write`, which an org admin or owner and
//! the `operator` role hold, on a plane with evals enabled (a create answers
//! 422 otherwise). The credential must name a project.

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Image, ImageCreate, ImageListParams};

/// `images()` namespace. Holds a DP-bound HTTP client.
#[derive(Clone)]
pub struct Images {
    http: Arc<HttpClient>,
}

impl Images {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/images` — paginated.
    pub fn list(&self, params: &ImageListParams) -> Paginator<Image> {
        Paginator::new(self.http.clone(), "/v1/images", params)
            .expect("ImageListParams must serialize to a JSON object")
    }

    /// `GET /v1/images/{id}`.
    pub async fn get(&self, image_id: Uuid) -> ApiResult<Image> {
        let path = format!("/v1/images/{image_id}");
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/images` — record the image and send its build.
    pub async fn create(&self, params: &ImageCreate) -> ApiResult<Image> {
        self.http.post_json("/v1/images", params).await
    }

    /// `DELETE /v1/images/{id}`. Deleting a runtime image rebakes it; one
    /// still baking answers 409.
    pub async fn delete(&self, image_id: Uuid) -> ApiResult<()> {
        let path = format!("/v1/images/{image_id}");
        self.http.delete_empty(&path).await
    }
}
