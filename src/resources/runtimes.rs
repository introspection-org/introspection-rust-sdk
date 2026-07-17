//! Runtime runner opening. Control-plane management is intentionally internal.

use std::sync::Arc;

use serde::Deserialize;
use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Paginated, RunRequest, RunnerSpec};
use crate::runner::{Runner, RunnerSource};

#[derive(Debug, Deserialize)]
struct ResolvedRuntime {
    id: Uuid,
}

#[derive(serde::Serialize)]
struct ResolveParams<'a> {
    runtime: &'a str,
    only_active: bool,
    limit: u32,
}

#[derive(Clone)]
pub struct RuntimeHandle {
    http: Arc<HttpClient>,
    runtime_ref: String,
}

impl RuntimeHandle {
    pub(crate) fn new(http: Arc<HttpClient>, runtime_ref: impl Into<String>) -> Self {
        Self {
            http,
            runtime_ref: runtime_ref.into(),
        }
    }

    async fn resolve_id(&self) -> ApiResult<Uuid> {
        let mut pages: Paginator<ResolvedRuntime> = Paginator::new(
            self.http.clone(),
            "/v1/runtimes",
            &ResolveParams {
                runtime: &self.runtime_ref,
                only_active: true,
                limit: 1,
            },
        )?;
        pages
            .next_page()
            .await?
            .and_then(|Paginated { records, .. }| records.into_iter().next())
            .map(|runtime| runtime.id)
            .ok_or_else(|| IntrospectionAPIError::Http {
                message: format!("no active runtime '{}'", self.runtime_ref),
                status: 404,
                code: None,
                request_id: None,
                body: None,
            })
    }

    /// Resolve the configured runtime group slug or ID and open a runner.
    pub async fn run(&self, ctx: RunRequest) -> ApiResult<Runner> {
        let runtime_id = self.resolve_id().await?;
        let path = format!("/v1/runtimes/{runtime_id}/run");
        let spec: RunnerSpec = self.http.post_json(&path, &ctx).await?;
        Runner::from_spec(
            spec,
            RunnerSource::Runtime {
                cp_http: self.http.clone(),
                runtime_id,
                ctx,
            },
        )
    }
}
