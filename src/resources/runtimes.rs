//! `client.runtimes` (CP) — read and run runtime versions.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    RunRequest, RuntimeRunSelector, RuntimeVersion, RuntimeVersionListParams, StringOrUuid,
};
use crate::runner::Runner;

/// `client.runtimes` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Runtimes {
    http: Arc<HttpClient>,
}

impl Runtimes {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/runtimes` — paginated.
    pub fn list(&self, params: &RuntimeVersionListParams) -> Paginator<RuntimeVersion> {
        Paginator::new(self.http.clone(), "/v1/runtimes", params)
            .expect("RuntimeVersionListParams must serialize to a JSON object")
    }

    /// `GET /v1/runtimes/{id}?project=...`.
    pub async fn get(
        &self,
        runtime_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<RuntimeVersion> {
        #[derive(Serialize)]
        struct Q {
            project: StringOrUuid,
        }
        let path = format!("/v1/runtimes/{}", runtime_id);
        self.http
            .get_json(
                &path,
                &Q {
                    project: project.into(),
                },
            )
            .await
    }

    /// Run either a stable Runtime or one exact immutable Runtime version.
    pub async fn run(&self, selector: RuntimeRunSelector, ctx: RunRequest) -> ApiResult<Runner> {
        let body = runtime_run_request_body(&selector, &ctx)?;
        let spec = self.http.post_json("/v1/runtimes/run", &body).await?;
        Runner::from_spec(spec)
    }
}

fn runtime_run_request_body(
    selector: &RuntimeRunSelector,
    ctx: &RunRequest,
) -> ApiResult<serde_json::Value> {
    let mut body = serde_json::to_value(ctx)
        .map_err(|error| IntrospectionAPIError::InvalidConfig(error.to_string()))?;
    let (field, value) = match selector {
        RuntimeRunSelector::Runtime(runtime) => ("runtime", runtime.to_string()),
        RuntimeRunSelector::RuntimeId(runtime_id) => ("runtime_id", runtime_id.to_string()),
    };
    body[field] = serde_json::Value::String(value);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::runtime_run_request_body;
    use crate::api::schemas::{RunRequest, RuntimeRunSelector};
    use uuid::Uuid;

    #[test]
    fn stable_runner_request_uses_only_runtime_selector() {
        let body = runtime_run_request_body(
            &RuntimeRunSelector::Runtime("support-agent".into()),
            &RunRequest::default(),
        )
        .expect("serialize stable runner request");

        assert_eq!(body["runtime"], "support-agent");
        assert!(body.get("runtime_id").is_none());
    }

    #[test]
    fn exact_runner_request_uses_only_runtime_id_selector() {
        let runtime_id = Uuid::parse_str("11111111-1111-4111-8111-111111111111")
            .expect("valid runtime version id");
        let body = runtime_run_request_body(
            &RuntimeRunSelector::RuntimeId(runtime_id),
            &RunRequest::default(),
        )
        .expect("serialize exact runner request");

        assert_eq!(body["runtime_id"], runtime_id.to_string());
        assert!(body.get("runtime").is_none());
    }
}
