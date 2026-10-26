//! Snippets for the docs' Rust SDK reference page
//! (`content/reference/rust.mdx`). Each marked docs region is what
//! the page shows; the code around it is setup the page leaves out.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx \
//! INTROSPECTION_TASK_ID=... INTROSPECTION_RUN_ID=... INTROSPECTION_INTERRUPT_ID=... \
//!   cargo run --example docs_reference_rust --features otel
//! ```

use std::error::Error;

use introspection_sdk::{IntrospectionClient, RunHandle, Runner};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let task_id = std::env::var("INTROSPECTION_TASK_ID").unwrap_or_default();
    let interrupt_id = std::env::var("INTROSPECTION_INTERRUPT_ID").unwrap_or_default();

    let (client, runner) = run_task().await?;
    let handle = runner.tasks().start_prompt("Wait for approval.").await?;
    continue_control(&runner, &handle, task_id.clone(), interrupt_id).await?;
    files_shares(&runner).await?;
    conversations_events_metrics(&runner).await?;
    #[cfg(feature = "otel")]
    feedback(task_id)?;
    experiments(&client).await?;

    runner.close();
    client.shutdown()?;
    Ok(())
}

#[rustfmt::skip]
async fn run_task() -> Result<(IntrospectionClient, Runner), Box<dyn Error>> {
    // #region docs:reference/rust/run-task
    use futures::StreamExt;
    use introspection_sdk::{
        AgUiEvent, ClientConfig, IntrospectionAPIError, IntrospectionClient, RunRequest, RunnerIdentity,
    };

    /// Applies one stream event to the assistant text read so far. A reconnect
    /// past the replay buffer yields a `MessagesSnapshot` whose assistant messages
    /// replace the text read before it; later deltas append to it.
    fn append_text(text: &mut String, event: AgUiEvent) -> Result<(), Box<dyn Error>> {
        match event {
            AgUiEvent::MessagesSnapshot(e) => {
                *text = e
                    .messages
                    .iter()
                    .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .collect();
            }
            AgUiEvent::TextMessageContent(m) => text.push_str(&m.delta),
            AgUiEvent::TextMessageChunk(m) => text.push_str(m.delta.as_deref().unwrap_or_default()),
            AgUiEvent::RunError(e) => {
                return Err(IntrospectionAPIError::RunFailed {
                    message: e.message,
                    code: e.code,
                }
                .into());
            }
            AgUiEvent::Custom(e) if e.name == "resume_gap" => {
                return Err(IntrospectionAPIError::StreamIncomplete(
                    "The replay buffer lost output; read the conversation transcript".into(),
                )
                .into());
            }
            _ => {}
        }
        Ok(())
    }

    let client = IntrospectionClient::new(ClientConfig::default())?;
    let runner = client
        .runtime("support-agent")
        .await?
        .run(RunRequest {
            identity: Some(RunnerIdentity { user_id: Some("user_123".into()), ..Default::default() }),
            ..Default::default()
        })
        .await?;

    let handle = runner.tasks().start_prompt("Summarize the open tickets.").await?;
    println!("{}", handle.text().await?);

    let mut events = runner.tasks().start_prompt("Say hello.").await?.into_stream().await?;
    let mut text = String::new();
    while let Some(event) = events.next().await { append_text(&mut text, event?)?; }
    println!("{text}");
    // #endregion docs:reference/rust/run-task
    Ok((client, runner))
}

async fn continue_control(
    runner: &Runner,
    handle: &RunHandle,
    task_id: String,
    interrupt_id: String,
) -> Result<(), Box<dyn Error>> {
    // #region docs:reference/rust/continue-control
    use introspection_sdk::{ResumeEntry, TaskCancelOptions, TaskRunResume};

    runner
        .tasks()
        .runs
        .resume(
            &task_id,
            &TaskRunResume {
                resume: vec![ResumeEntry {
                    interrupt_id,
                    status: "resolved".into(),
                    payload: Some(serde_json::json!({ "approved": true })),
                }],
            },
        )
        .await?;

    handle.cancel().await?; // abort the turn, keep the sandbox warm
    handle
        .cancel_with(&TaskCancelOptions::Drain {
            drain_within_seconds: Some(60),
        })
        .await?;
    // #endregion docs:reference/rust/continue-control
    Ok(())
}

async fn files_shares(runner: &Runner) -> Result<(), Box<dyn Error>> {
    // #region docs:reference/rust/files
    use introspection_sdk::{FileType, FileUpload, TaskCreate, TaskFileRef};

    let uploaded = runner
        .files()
        .upload(FileUpload::from_path("notes.txt").with_file_type(FileType::Upload))
        .await?;
    runner
        .tasks()
        .start(&TaskCreate {
            prompt: Some("Summarize the attached notes.".into()),
            files: Some(vec![TaskFileRef {
                id: uploaded.id,
                ..Default::default()
            }]),
            ..Default::default()
        })
        .await?;
    // #endregion docs:reference/rust/files
    Ok(())
}

async fn conversations_events_metrics(runner: &Runner) -> Result<(), Box<dyn Error>> {
    // #region docs:reference/rust/conversations-events-metrics
    use introspection_sdk::{ConversationListParams, MetricSpec, MetricsQuery};

    let mut pages = runner
        .conversations()
        .list(&ConversationListParams::default())?;
    while let Some(page) = pages.next_page().await? {
        for c in &page.records {
            println!("{} {}", c.id, c.usage.total_tokens);
        }
    }

    let result = runner
        .metrics()
        .query(&MetricsQuery {
            view: "conversations".into(),
            metrics: vec![MetricSpec::of("sum", "total_cost_usd")],
            lookback: Some("7d".into()),
            ..Default::default()
        })
        .await?;
    // #endregion docs:reference/rust/conversations-events-metrics
    println!("{} rows", result.data.len());
    Ok(())
}

#[cfg(feature = "otel")]
fn feedback(task_id: String) -> Result<(), Box<dyn Error>> {
    // #region docs:reference/rust/feedback
    use introspection_sdk::otel::{FeedbackOptions, IntrospectionLogs};

    let logs = IntrospectionLogs::builder()
        .service_name("support-api")
        .build()?;
    {
        let _c = logs.set_conversation_id(&task_id);
        logs.feedback(
            "thumbs_up",
            FeedbackOptions::new().with_comments("Solved it"),
        );
    }
    logs.shutdown()?;
    // #endregion docs:reference/rust/feedback
    Ok(())
}

async fn experiments(client: &IntrospectionClient) -> Result<(), Box<dyn Error>> {
    let a = std::env::var("BASELINE_RUNTIME_ID").unwrap_or_default();
    let b = std::env::var("CANDIDATE_RUNTIME_ID").unwrap_or_default();
    let judge_id = std::env::var("JUDGE_ID").unwrap_or_default();
    // #region docs:reference/rust/experiments
    let created = client.experiments().create(&serde_json::json!({
        "project": "my-project", "name": "candidate", "runtime": "support-agent",
        "arms": [{ "runtime_id": a, "arm_label": "baseline" }, { "runtime_id": b, "arm_label": "candidate" }],
        "goal_json": { "kind": "composite", "direction": "maximize",
                       "components": [{ "source": "judge", "judge_id": judge_id, "weight": 1.0 }] }
    })).await?;
    client.experiment(created.id, "my-project").start().await?;
    // #endregion docs:reference/rust/experiments
    Ok(())
}
