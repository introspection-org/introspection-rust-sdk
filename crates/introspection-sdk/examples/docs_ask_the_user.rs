//! Docs snippets for `patterns/ask-the-user`: notice that a run paused on an
//! interrupt, then resume it with the user's answer.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx cargo run --example docs_ask_the_user
//! ```

use std::error::Error;

use futures::StreamExt;
use introspection_sdk::{
    AgUiEvent, ClientConfig, IntrospectionClient, ResumeEntry, RunRequest, TaskRunResume,
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
    let handle = runner
        .tasks()
        .start_prompt("Which region should the new workspace live in?")
        .await?;
    let task_id = handle.run.task_id.to_string();

    // #region docs:patterns/ask-the-user/detect-pause
    let mut interrupts = Vec::new();
    let stream = handle.stream().await?;
    tokio::pin!(stream);
    while let Some(event) = stream.next().await {
        match event? {
            AgUiEvent::RunFinished(e) => {
                if let Some(outcome) = e.outcome.filter(|o| o["type"] == "interrupt") {
                    interrupts = outcome["interrupts"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                }
            }
            AgUiEvent::RunError(e) => return Err(format!("run failed: {}", e.message).into()),
            _ => {}
        }
    }
    // #endregion docs:patterns/ask-the-user/detect-pause

    let Some(interrupt) = interrupts.first() else {
        println!("the run finished without asking");
        return Ok(());
    };

    // #region docs:patterns/ask-the-user/resume
    runner
        .tasks()
        .runs
        .resume(
            &task_id,
            &TaskRunResume {
                resume: vec![ResumeEntry {
                    interrupt_id: interrupt["id"].as_str().unwrap_or_default().into(),
                    status: "resolved".into(),
                    payload: Some(serde_json::json!({ "answer": "EU" })),
                }],
            },
        )
        .await?;
    // #endregion docs:patterns/ask-the-user/resume

    Ok(())
}
