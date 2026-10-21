//! Docs snippets for `patterns/structured-output`: collect the arguments of
//! the `submit_result` tool call from the stream, validate them, and ask the
//! agent once more when they don't parse.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx cargo run --example docs_structured_output
//! ```

use std::{collections::HashMap, error::Error};

use futures::StreamExt;
use introspection_sdk::{
    AgUiEvent, ClientConfig, IntrospectionClient, RunRequest, TaskPrompt, TaskRunCreate,
    TaskRunKind,
};
use serde::Deserialize;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    let runner = client
        .runtime("support-agent")
        .await?
        .run(RunRequest::default())
        .await?;

    // #region docs:patterns/structured-output/read-stream
    let handle = runner
        .tasks()
        .start_prompt("Triage ticket 4411: ...")
        .await?;
    let mut args: HashMap<String, String> = HashMap::new();
    let mut submit_id = None;

    let stream = handle.stream().await?;
    tokio::pin!(stream);
    while let Some(event) = stream.next().await {
        match event? {
            AgUiEvent::ToolCallStart(e) if e.tool_call_name == "submit_result" => {
                submit_id = Some(e.tool_call_id)
            }
            AgUiEvent::ToolCallArgs(e) => {
                args.entry(e.tool_call_id).or_default().push_str(&e.delta)
            }
            // A reconnect behind the replay buffer sends the run so far instead.
            AgUiEvent::MessagesSnapshot(e) => {
                args.clear();
                let calls = e.messages.iter().filter_map(|m| m["toolCalls"].as_array());
                for call in calls.flatten() {
                    let id = call["id"].as_str().unwrap_or_default().to_string();
                    if call["function"]["name"] == "submit_result" {
                        submit_id = Some(id.clone());
                    }
                    let arguments = call["function"]["arguments"].as_str().unwrap_or_default();
                    args.insert(id, arguments.into());
                }
            }
            AgUiEvent::RunError(e) => return Err(format!("run failed: {}", e.message).into()),
            _ => {}
        }
    }
    let raw = submit_id.and_then(|id| args.remove(&id));
    // #endregion docs:patterns/structured-output/read-stream

    let task_id = handle.run.task_id.to_string();

    // #region docs:patterns/structured-output/validate
    // The `Triage` schema the `submit_result` tool declares.
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Category {
        Billing,
        Bug,
        Other,
    }

    #[derive(Debug, Deserialize)]
    struct Triage {
        category: Category,
        priority: u8,
        summary: String,
    }

    let result = raw
        .and_then(|raw| serde_json::from_str::<Triage>(&raw).ok())
        .filter(|t| (1..=4).contains(&t.priority) && t.summary.chars().count() <= 280);
    if result.is_none() {
        runner
            .tasks()
            .runs
            .create(
                &task_id,
                &TaskRunCreate {
                    kind: Some(TaskRunKind::Prompt),
                    prompt: Some(TaskPrompt {
                        text: "You did not call submit_result with a valid triage. Call it now."
                            .into(),
                        images: None,
                    }),
                    ..Default::default()
                },
            )
            .await?;
    }
    // #endregion docs:patterns/structured-output/validate

    if let Some(triage) = result {
        println!(
            "{:?} p{}: {}",
            triage.category, triage.priority, triage.summary
        );
    }
    Ok(())
}
