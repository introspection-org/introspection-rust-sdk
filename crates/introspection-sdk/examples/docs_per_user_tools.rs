//! Docs snippets for `patterns/per-user-tools`: mint a Pipedream link for an
//! end user from your backend, and back a Connect button for a signed-in user.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx CONNECTOR_ID=<connector-uuid> \
//!   USER_SESSION_TOKEN=<application-session-token> DATA_PLANE_URL=<dp-url> \
//!   cargo run --example docs_per_user_tools
//! ```

use std::error::Error;

use futures::TryStreamExt;
use introspection_sdk::{
    AdvancedOptions, ClientConfig, ConnectorAuthorizeParams, IntrospectionClient,
    MemberConnectionCreate, MemberConnectionListParams, RunnerIdentity,
};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    let connector_id: Uuid = std::env::var("CONNECTOR_ID")?.parse()?;

    // #region docs:patterns/per-user-tools/mint-link
    let authorize_url = client
        .connectors()
        .authorize(
            connector_id,
            &ConnectorAuthorizeParams {
                app: Some("<app>".into()),
                runtime: Some("support-agent".into()),
                identity: Some(RunnerIdentity {
                    user_id: Some("user-4821".into()),
                    ..Default::default()
                }),
                expires_in: Some(3600),
                ..Default::default()
            },
        )
        .await?
        .authorize_url;
    // #endregion docs:patterns/per-user-tools/mint-link
    println!("{authorize_url}");

    let session_token = std::env::var("USER_SESSION_TOKEN")?;
    let data_plane_url = std::env::var("DATA_PLANE_URL")?;

    // #region docs:patterns/per-user-tools/verify
    // Authenticate as the signed-in end user, not with your backend token,
    // so the connections listed and created are their own.
    let user = IntrospectionClient::new(ClientConfig {
        token: Some(session_token),
        advanced: Some(AdvancedOptions {
            dp_url: Some(data_plane_url),
            ..Default::default()
        }),
    })?;
    let mine: Vec<_> = user
        .connections()
        .list(&MemberConnectionListParams {
            app: Some("<app>".into()),
            ..Default::default()
        })
        .try_collect()
        .await?;
    let authorize_url = user
        .connections()
        .create(&MemberConnectionCreate {
            runtime: Some("support-agent".into()),
            ..MemberConnectionCreate::new("<app>")
        })
        .await?
        .authorize_url;
    // #endregion docs:patterns/per-user-tools/verify
    println!("{} connections; connect at {authorize_url}", mine.len());

    Ok(())
}
