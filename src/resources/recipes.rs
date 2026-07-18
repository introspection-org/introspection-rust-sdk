//! `client.recipes` (CP) — recipe CRUD.
//!
//! Recipes are pure CRUD: no handle subtype, no `.run()` lifecycle. They
//! describe a (repository, git_ref, git_commit_sha [, sub_path]) tuple
//! used by platform-managed runtime versions.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Recipe, RecipeCreate, RecipeListParams, RecipeUpdate};

/// `client.recipes` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Recipes {
    http: Arc<HttpClient>,
}

impl Recipes {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/recipes` — paginated.
    pub fn list(&self, params: &RecipeListParams) -> Paginator<Recipe> {
        Paginator::new(self.http.clone(), "/v1/recipes", params)
            .expect("RecipeListParams must serialize to a JSON object")
    }

    /// `GET /v1/recipes/{id}`.
    pub async fn get(&self, recipe_id: Uuid) -> ApiResult<Recipe> {
        #[derive(Serialize)]
        struct Q {}
        let path = format!("/v1/recipes/{}", recipe_id);
        self.http.get_json(&path, &Q {}).await
    }

    /// `POST /v1/recipes`.
    pub async fn create(&self, body: &RecipeCreate) -> ApiResult<Recipe> {
        self.http.post_json("/v1/recipes", body).await
    }

    /// `PATCH /v1/recipes/{id}`.
    pub async fn update(&self, recipe_id: Uuid, body: &RecipeUpdate) -> ApiResult<Recipe> {
        let path = format!("/v1/recipes/{}", recipe_id);
        self.http.patch_json(&path, body).await
    }

    /// `DELETE /v1/recipes/{id}`.
    pub async fn delete(&self, recipe_id: Uuid) -> ApiResult<()> {
        let path = format!("/v1/recipes/{}", recipe_id);
        self.http.delete_empty(&path).await
    }
}
