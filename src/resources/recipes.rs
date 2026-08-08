//! `client.recipes` (CP) — recipe lookup.
//!
//! Read-only: a runner resolves the recipe it is running under, it does not
//! author one. Defining a recipe is a project-authoring act and lives in the
//! CLI. A recipe describes a (repository, git_ref, git_commit_sha
//! [, sub_path]) tuple used by platform-managed runtime versions.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Recipe, RecipeListParams};

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
}
