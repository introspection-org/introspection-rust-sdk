//! Docs snippets for `patterns/trigger`: start one task per incoming event,
//! and run a batch of tasks in waves of five.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx cargo run --example docs_trigger
//! ```

use std::{collections::HashMap, error::Error};

use futures::future::try_join_all;
use introspection_sdk::{ClientConfig, IntrospectionClient, RunRequest, TaskCreate};

// #region docs:patterns/trigger/task-per-event
// POST /webhooks/events: answer `202` with the task ID.
async fn on_event(
    client: &IntrospectionClient,
    id: &str,
    text: &str,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let runner = client
        .runtime("triage-agent")
        .await?
        .run(RunRequest::default())
        .await?;
    let handle = runner
        .tasks()
        .start(&TaskCreate {
            prompt: Some(format!("New event {id}:\n\n{text}")),
            conversation_metadata: Some(HashMap::from([("event_id".into(), id.into())])),
            ..Default::default()
        })
        .await?;
    Ok(serde_json::json!({ "task_id": handle.run.task_id }))
}
// #endregion docs:patterns/trigger/task-per-event

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?; // reads INTROSPECTION_TOKEN
    println!(
        "{}",
        on_event(&client, "evt_1", "Customer charged twice").await?
    );

    let tickets = ["4411", "4412", "4413", "4414", "4415", "4416"];

    // #region docs:patterns/trigger/batch
    let runner = client
        .runtime("triage-agent")
        .await?
        .run(RunRequest::default())
        .await?;
    let tasks = &runner.tasks();
    for wave in tickets.chunks(5) {
        let handles = try_join_all(wave.iter().map(|t| async move {
            tasks
                .start(&TaskCreate {
                    prompt: Some(format!("Triage ticket {t}")),
                    tags: Some(vec!["batch:2026-10-15".into()]),
                    ..Default::default()
                })
                .await
        }))
        .await?;
        try_join_all(handles.iter().map(|h| h.text())).await?; // waits for each run to settle
    }
    // #endregion docs:patterns/trigger/batch

    Ok(())
}
