//! `client.projects` (CP) — project listing.

use std::sync::Arc;

use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Project, ProjectListParams};

#[derive(Clone)]
pub struct Projects {
    http: Arc<HttpClient>,
}

impl Projects {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/projects` — paginated.
    pub fn list(&self, params: &ProjectListParams) -> Paginator<Project> {
        Paginator::new(self.http.clone(), "/v1/projects", params)
            .expect("ProjectListParams must serialize to a JSON object")
    }
}
