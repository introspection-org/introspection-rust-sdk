//! Experiment runner opening. Experiment management is intentionally internal.

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::schemas::{RunRequest, RunnerSpec};
use crate::runner::{Runner, RunnerSource};

#[derive(Clone)]
pub struct ExperimentHandle {
    http: Arc<HttpClient>,
    experiment_id: Uuid,
}

impl ExperimentHandle {
    pub(crate) fn new(http: Arc<HttpClient>, experiment_id: Uuid) -> Self {
        Self {
            http,
            experiment_id,
        }
    }

    pub fn id(&self) -> Uuid {
        self.experiment_id
    }

    /// Open a runner with the experiment arm selected by the control plane.
    pub async fn run(&self, ctx: RunRequest) -> ApiResult<Runner> {
        let path = format!("/v1/experiments/{}/run", self.experiment_id);
        let spec: RunnerSpec = self.http.post_json(&path, &ctx).await?;
        Runner::from_spec(
            spec,
            RunnerSource::Experiment {
                cp_http: self.http.clone(),
                experiment_id: self.experiment_id,
                ctx,
            },
        )
    }
}
