//! `client.experiments` (CP) — experiment CRUD, lifecycle, and execution.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Experiment, ExperimentCreate, ExperimentListParams, ExperimentRunRequest, ExperimentUpdate,
    RunnerSpec, StringOrUuid,
};
use crate::runner::Runner;

#[derive(Clone)]
pub struct Experiments {
    http: Arc<HttpClient>,
}

impl Experiments {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub fn list(&self, params: &ExperimentListParams) -> Paginator<Experiment> {
        Paginator::new(self.http.clone(), "/v1/experiments", params)
            .expect("ExperimentListParams must serialize to a JSON object")
    }

    pub async fn get(
        &self,
        experiment_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Experiment> {
        #[derive(Serialize)]
        struct Q {
            project: StringOrUuid,
        }
        let path = format!("/v1/experiments/{}", experiment_id);
        self.http
            .get_json(
                &path,
                &Q {
                    project: project.into(),
                },
            )
            .await
    }

    pub async fn create(&self, body: &ExperimentCreate) -> ApiResult<Experiment> {
        self.http.post_json("/v1/experiments", body).await
    }

    pub async fn update(
        &self,
        experiment_id: Uuid,
        project: impl Into<StringOrUuid>,
        body: &ExperimentUpdate,
    ) -> ApiResult<Experiment> {
        let path = format!(
            "/v1/experiments/{}?project={}",
            experiment_id,
            project.into()
        );
        self.http.patch_json(&path, body).await
    }

    pub async fn delete(
        &self,
        experiment_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<()> {
        let path = format!(
            "/v1/experiments/{}?project={}",
            experiment_id,
            project.into()
        );
        self.http.delete_empty(&path).await
    }

    /// Run this Experiment and return its selected [`Runner`].
    pub async fn run(
        &self,
        experiment_id: Uuid,
        project: impl Into<StringOrUuid>,
        request: ExperimentRunRequest,
    ) -> ApiResult<Runner> {
        let path = format!(
            "/v1/experiments/{experiment_id}/run?project={}",
            project.into()
        );
        let spec: RunnerSpec = self.http.post_json(&path, &request).await?;
        Runner::from_spec(spec)
    }

    pub async fn start(
        &self,
        experiment_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Experiment> {
        let path = format!(
            "/v1/experiments/{}/start?project={}",
            experiment_id,
            project.into()
        );
        self.http.post_json(&path, &serde_json::json!({})).await
    }

    pub async fn end(
        &self,
        experiment_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Experiment> {
        let path = format!(
            "/v1/experiments/{}/end?project={}",
            experiment_id,
            project.into()
        );
        self.http.post_json(&path, &serde_json::json!({})).await
    }

    pub async fn cancel(
        &self,
        experiment_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Experiment> {
        let path = format!(
            "/v1/experiments/{}/cancel?project={}",
            experiment_id,
            project.into()
        );
        self.http.post_json(&path, &serde_json::json!({})).await
    }
}
