//! Docs snippet for `patterns/approve-outside`: tell the agent what a person
//! did with its draft by steering the task.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx TASK_ID=<task-id> \
//!   cargo run --example docs_approve_outside
//! ```

use std::error::Error;

use introspection_sdk::{
    ClientConfig, IntrospectionClient, RunRequest, TaskPrompt, TaskRunCreate, TaskRunKind,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    let runner = client
        .runtime("support-agent")
        .await?
        .run(RunRequest::default())
        .await?;
    let task_id = std::env::var("TASK_ID")?;

    // #region docs:patterns/approve-outside/report-outcome
    runner
        .tasks()
        .runs
        .create(
            &task_id,
            &TaskRunCreate {
                kind: Some(TaskRunKind::Steer),
                prompt: Some(TaskPrompt {
                    text: r#"<user_action kind="draft" action="sent" id="d-17" by="maria">Sent text</user_action>"#.into(),
                    images: None,
                }),
                ..Default::default()
            },
        )
        .await?;
    // #endregion docs:patterns/approve-outside/report-outcome

    Ok(())
}
