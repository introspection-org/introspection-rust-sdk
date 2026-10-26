//! `client.recipes` (CP) — recipe lookup, and with the `authoring` feature
//! create, update and delete.
//!
//! A runner resolves the recipe it is running under; defining one is a
//! project-authoring act, which a push to the recipe's repository usually
//! does. A recipe describes a (repository, git_ref, git_commit_sha
//! [, sub_path]) tuple used by platform-managed runtime versions.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Recipe, RecipeListParams};
#[cfg(feature = "authoring")]
use crate::api::schemas::{RecipeCreateParams, RecipeUpdateParams, StringOrUuid};
#[cfg(feature = "authoring")]
use crate::resources::with_project;

/// `client.recipes` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Recipes {
    http: Arc<HttpClient>,
}

impl Recipes {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
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

    /// `POST /v1/recipes` — pin a repository commit as a Recipe. Needs
    /// `recipes:write`. The Recipe check then runs in the background; see
    /// [`Recipe::validation`].
    #[cfg(feature = "authoring")]
    pub async fn create(&self, params: &RecipeCreateParams) -> ApiResult<Recipe> {
        self.http.post_json("/v1/recipes", params).await
    }

    /// `PATCH /v1/recipes/{id}[?project=...]` — rename or describe a Recipe,
    /// or run its check again. Needs `recipes:write`. `project` is needed only
    /// when the credential does not already name one.
    #[cfg(feature = "authoring")]
    pub async fn update(
        &self,
        recipe_id: Uuid,
        params: &RecipeUpdateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Recipe> {
        let path = with_project(&format!("/v1/recipes/{recipe_id}"), project.as_ref());
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/recipes/{id}[?project=...]` — soft-delete. Needs
    /// `recipes:delete`. Runtimes built from it keep running, but no task can
    /// start on it any more.
    #[cfg(feature = "authoring")]
    pub async fn delete(&self, recipe_id: Uuid, project: Option<StringOrUuid>) -> ApiResult<()> {
        let path = with_project(&format!("/v1/recipes/{recipe_id}"), project.as_ref());
        self.http.delete_empty(&path).await
    }
}
