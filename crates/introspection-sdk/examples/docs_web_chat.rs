//! Docs snippet for `patterns/web-chat` (Verify, federated sign-in): a broker
//! that trades the user's IdP token for a Data Plane token the browser can use.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_FEDERATED_CLIENT_ID=intro_app_xxx INTROSPECTION_PROJECT=<project> \
//! ID_TOKEN=<idp-id-token> RUNTIME_ID=<runtime-id> \
//!   cargo run --example docs_web_chat
//! ```

use std::error::Error;

use introspection_sdk::{token_exchange, TokenExchangeParams};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    // The signed-in user's token from your IdP, and the runtime the browser runs.
    let id_token = std::env::var("ID_TOKEN")?;
    let runtime_id = std::env::var("RUNTIME_ID")?;

    // #region docs:patterns/web-chat/verify
    let token = token_exchange(
        TokenExchangeParams::builder()
            .subject_token(id_token)
            .client_id(std::env::var("INTROSPECTION_FEDERATED_CLIENT_ID")?)
            .project(std::env::var("INTROSPECTION_PROJECT")?)
            .build()?,
    )
    .await?;
    let body = serde_json::json!({
        "token": token.access_token,
        "runtimeId": runtime_id,
        "dpUrl": token.dp_url,
    });
    // #endregion docs:patterns/web-chat/verify

    println!("{body}");
    Ok(())
}
