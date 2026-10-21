//! Docs snippet for `identity/sharing`: share a conversation with one member
//! from your app.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx TASK_ID=<task-id> MEMBER_ID=<member-id> \
//!   cargo run --example docs_identity_sharing
//! ```

use std::error::Error;

use introspection_sdk::{ClientConfig, IntrospectionClient, ShareCreate, ShareResourceType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    let task_id = std::env::var("TASK_ID")?;
    let granted_member_id = std::env::var("MEMBER_ID")?.parse()?;

    // #region docs:identity/sharing/share
    client
        .shares()
        .create(&ShareCreate {
            granted_member_id: Some(granted_member_id),
            ..ShareCreate::new(ShareResourceType::Conversation, task_id)
        })
        .await?;
    // #endregion docs:identity/sharing/share

    Ok(())
}
