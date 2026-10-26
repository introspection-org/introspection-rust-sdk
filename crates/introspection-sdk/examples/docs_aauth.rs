//! Snippets for the docs' AAuth page (`content/identity/aauth.mdx`). Each
//! marked docs region is what the page shows; the code around it is setup
//! the page leaves out.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx INTROSPECTION_APPROVAL_ID=... \
//!   cargo run --example docs_aauth
//! ```

use std::error::Error;

use introspection_sdk::{ClientConfig, IntrospectionClient};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let approval_id: Uuid = std::env::var("INTROSPECTION_APPROVAL_ID")?.parse()?;

    let client = IntrospectionClient::new(ClientConfig::default())?;
    decide_approval(&client, approval_id).await?;
    client.shutdown()?;
    Ok(())
}

#[rustfmt::skip]
async fn decide_approval(
    client: &IntrospectionClient,
    approval_id: Uuid,
) -> Result<(), Box<dyn Error>> {
    use introspection_sdk::{ApprovalDecision, Budget, DecidedBy};

    // #region docs:identity/aauth/decide-approval
    let decision = ApprovalDecision::approve(DecidedBy::user("dana@acme.example"))
        .with_budget(Budget { amount: 300_000, unit: "USD".into(), decimals: 2, extra: Default::default() });
    client.aauth().approvals().decide(approval_id, &decision).await?;
    // #endregion docs:identity/aauth/decide-approval
    Ok(())
}
