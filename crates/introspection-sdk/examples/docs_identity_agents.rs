//! Docs snippet for `identity/agents`: open a runner that runs tasks for one
//! end user, so the tasks and their memory belong to that user.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx APP_USER_ID=user_123 \
//!   cargo run --example docs_identity_agents
//! ```

use std::error::Error;

use introspection_sdk::{ClientConfig, IntrospectionClient, RunRequest, RunnerIdentity};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    // The signed-in user, from your server session.
    let user_id = std::env::var("APP_USER_ID")?;

    // #region docs:identity/agents/how-agents-act
    let runner = client
        .runtime("support-agent")
        .await?
        .run(RunRequest {
            identity: Some(RunnerIdentity {
                user_id: Some(user_id),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await?;
    // #endregion docs:identity/agents/how-agents-act

    println!("{:?}", runner.context());
    Ok(())
}
