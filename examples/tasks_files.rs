//! End-to-end walkthrough — find-or-create a runtime by name, open a
//! Runner, spawn a task, stream its run, then upload a file.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx \
//! INTROSPECTION_PROJECT_ID=<uuid> \
//! INTROSPECTION_RECIPE_ID=<uuid> \
//! INTROSPECTION_BASE_API_URL=http://localhost:8000 \
//!   cargo run --example tasks_files
//! ```

use std::error::Error;

use futures::StreamExt;
use introspection_sdk::{
    ClientConfig, FileCreateText, FileUpload, IntrospectionClient, RunRequest, RuntimeCreate,
    RuntimeListParams,
};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;

    let project_id: Uuid = std::env::var("INTROSPECTION_PROJECT_ID")
        .expect("set INTROSPECTION_PROJECT_ID")
        .parse()?;
    let recipe_id: Uuid = std::env::var("INTROSPECTION_RECIPE_ID")
        .expect("set INTROSPECTION_RECIPE_ID")
        .parse()?;
    let runtime_name =
        std::env::var("INTROSPECTION_RUNTIME_NAME").unwrap_or_else(|_| "customer-agent".into());

    // 1) Find-or-create the runtime by name.
    let runtime = {
        let mut paginator = client.runtimes().list(&RuntimeListParams {
            project_id,
            name: Some(runtime_name.clone()),
            only_active: Some(true),
            ..Default::default()
        });
        if let Some(existing) = paginator
            .next_page()
            .await?
            .and_then(|p| p.records.into_iter().next())
        {
            println!("reusing runtime -> {} ({})", existing.name, existing.id);
            existing
        } else {
            let created = client
                .runtimes()
                .create(&RuntimeCreate {
                    project_id,
                    recipe_id,
                    name: runtime_name,
                    ..Default::default()
                })
                .await?;
            let handle = client.runtime(created.id);
            let activated = handle.activate(Some(project_id)).await?;
            println!("created runtime -> {} ({})", activated.name, activated.id);
            activated
        }
    };

    // 2) Open a Runner against the runtime.
    let runner = client
        .runtime(runtime.id)
        .run(RunRequest {
            ttl_seconds: Some(3600),
            ..Default::default()
        })
        .await?;
    println!(
        "runner -> dp={}, session={}, expires={}",
        runner.dp_endpoint(),
        runner.session_id(),
        runner.expires_at(),
    );

    // 3) Spawn a task and stream its events.
    let tasks = runner.tasks();
    let run = tasks.start_prompt("Say hello in one sentence.").await?;
    println!(
        "spawned task={:?}, run={}",
        run.task.as_ref().map(|t| t.id),
        run.run.id
    );

    let stream = run.stream().await?;
    tokio::pin!(stream);
    while let Some(event) = stream.next().await {
        let event = event?;
        println!("[{}] {}", event.event, event.data);
    }

    // 4) Upload a file via the runner.
    let files = runner.files();
    let note = files
        .create_text(&FileCreateText {
            name: "notes.md".into(),
            content: "# Hello\n\nFrom the Rust SDK Runner.".into(),
            mime_type: Some("text/markdown".into()),
        })
        .await?;
    println!("created file: {}", note.id);

    let binary = files
        .upload(FileUpload::from_bytes(
            b"hello binary".to_vec(),
            "hello.bin",
        ))
        .await?;
    println!("uploaded binary file: {}", binary.id);

    runner.close();
    client.shutdown()?;
    Ok(())
}
