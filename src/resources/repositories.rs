//! `client.repositories` (CP) — repository listing.

use std::sync::Arc;

use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Repository, RepositoryListParams};

#[derive(Clone)]
pub struct Repositories {
    http: Arc<HttpClient>,
}

impl Repositories {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/repositories` — paginated.
    pub fn list(&self, params: &RepositoryListParams) -> Paginator<Repository> {
        Paginator::new(self.http.clone(), "/v1/repositories", params)
            .expect("RepositoryListParams must serialize to a JSON object")
    }
}
