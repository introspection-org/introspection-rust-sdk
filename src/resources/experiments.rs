//! `client.experiments` (CP) — experiment CRUD + lifecycle +
//! `experiment(id, project).run()` to open a [`crate::Runner`].

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Experiment, ExperimentCreate, ExperimentListParams, ExperimentUpdate, RunRequest, RunnerSpec,
    StringOrUuid,
};
use crate::runner::{Runner, RunnerSource};

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

    pub fn handle(
        &self,
        experiment_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ExperimentHandle {
        ExperimentHandle {
            http: self.http.clone(),
            experiment_id,
            project: project.into(),
        }
    }
}

/// Handle returned by `client.experiment(id, project)`. Open a
/// [`Runner`] via [`Self::run`] or drive lifecycle via
/// [`Self::start`] / [`Self::end`] / [`Self::cancel`].
#[derive(Clone)]
pub struct ExperimentHandle {
    http: Arc<HttpClient>,
    experiment_id: Uuid,
    project: StringOrUuid,
}

impl ExperimentHandle {
    pub fn id(&self) -> Uuid {
        self.experiment_id
    }

    pub fn project(&self) -> &StringOrUuid {
        &self.project
    }

    /// `POST /v1/experiments/{id}/run` — open a [`Runner`] with the arm
    /// picked by CP.
    pub async fn run(&self, ctx: RunRequest) -> ApiResult<Runner> {
        let path = format!(
            "/v1/experiments/{}/run?project={}",
            self.experiment_id, self.project
        );
        let spec: RunnerSpec = self.http.post_json(&path, &ctx).await?;
        let source = RunnerSource::Experiment {
            cp_http: self.http.clone(),
            experiment_id: self.experiment_id,
            project: self.project.clone(),
            ctx,
        };
        Runner::from_spec(spec, source)
    }

    pub async fn start(&self) -> ApiResult<Experiment> {
        let path = format!(
            "/v1/experiments/{}/start?project={}",
            self.experiment_id, self.project
        );
        self.http.post_json(&path, &serde_json::json!({})).await
    }

    pub async fn end(&self) -> ApiResult<Experiment> {
        let path = format!(
            "/v1/experiments/{}/end?project={}",
            self.experiment_id, self.project
        );
        self.http.post_json(&path, &serde_json::json!({})).await
    }

    pub async fn cancel(&self) -> ApiResult<Experiment> {
        let path = format!(
            "/v1/experiments/{}/cancel?project={}",
            self.experiment_id, self.project
        );
        self.http.post_json(&path, &serde_json::json!({})).await
    }
}
