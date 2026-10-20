//! Docs snippet for `patterns/files`: upload a file, attach it to a task,
//! and download a file the task wrote.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx OUTPUT_FILE_ID=<file-id> cargo run --example docs_files
//! ```

use std::error::Error;

use introspection_sdk::{
    ClientConfig, FileType, FileUpload, IntrospectionClient, RunRequest, TaskCreate, TaskFileRef,
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
    let pdf = std::fs::read("contract.pdf")?;
    let file_id = std::env::var("OUTPUT_FILE_ID")?;

    // #region docs:patterns/files/verify
    let f = runner
        .files()
        .upload(FileUpload::from_bytes(pdf, "contract.pdf").with_file_type(FileType::Upload))
        .await?;
    let handle = runner
        .tasks()
        .start(&TaskCreate {
            prompt: Some("Review contract.pdf".into()),
            files: Some(vec![TaskFileRef {
                id: f.id.to_string(),
                ..Default::default()
            }]),
            ..Default::default()
        })
        .await?;
    let bytes = runner.files().download(&file_id).await?;
    // #endregion docs:patterns/files/verify

    println!("{}: {} bytes", handle.run.id, bytes.len());
    Ok(())
}
