//! End-to-end walkthrough — look up a runtime by runtime group slug, open a Runner,
//! spawn a task, stream its run, then upload a file.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx \
//! INTROSPECTION_BASE_API_URL=http://localhost:8000 \
//!   cargo run --example runtimes
//! ```

use std::error::Error;

use futures::StreamExt;
use introspection_sdk::{
    ClientConfig, FileCreateText, FileUpload, IntrospectionClient, RunRequest,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;

    let runtime =
        std::env::var("INTROSPECTION_RUNTIME").unwrap_or_else(|_| "customer-agent".into());

    // 1) Look up the runtime by runtime group slug or ID and open a Runner.
    let runner = client
        .runtime(&runtime)
        .await?
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

    // 2) Spawn a task and stream its events.
    let run = runner
        .tasks()
        .start_prompt("Say hello in one sentence.")
        .await?;
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

    // 3) Upload files via the runner.
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
