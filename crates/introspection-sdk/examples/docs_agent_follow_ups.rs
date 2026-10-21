//! Docs snippet for `patterns/agent-follow-ups`: keep the end user's time
//! zone on the task so the agent's follow-ups fire in that zone.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx cargo run --example docs_agent_follow_ups
//! ```

use std::{collections::HashMap, error::Error};

use introspection_sdk::{ClientConfig, IntrospectionClient, RunRequest, TaskCreate};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    let runner = client
        .runtime("support-agent")
        .await?
        .run(RunRequest::default())
        .await?;
    let prompt = "Remind me to send the invoice tomorrow at 9.";

    // #region docs:patterns/agent-follow-ups/time-zone
    runner
        .tasks()
        .start(&TaskCreate {
            prompt: Some(prompt.into()),
            metadata: Some(HashMap::from([("timezone".into(), json!("Europe/London"))])),
            ..Default::default()
        })
        .await?;
    // #endregion docs:patterns/agent-follow-ups/time-zone

    Ok(())
}
