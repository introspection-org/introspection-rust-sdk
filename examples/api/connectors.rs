//! B2B2C connector walkthrough — the flow a Business runs from its own
//! backend, without ever touching the Introspection UI.
//!
//! Creates a Slack connector for the org, mints the install link that gets
//! handed to a customer, then lists the workspaces that connected and
//! (optionally) disconnects one.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx \
//! SLACK_CLIENT_ID=... SLACK_CLIENT_SECRET=... \
//! INTROSPECTION_RUNTIME=support-agent \
//!   cargo run --example connectors
//! ```
//!
//! Optional: `REVOKE_FIRST_CONNECTION=1` revokes the first listed connection.
//!
//! Connectors sit behind a server-side feature flag. If every call fails with
//! "Connectors are not enabled", the deployment has not opted in yet.

use std::error::Error;

use futures::StreamExt;
use introspection_sdk::{
    ClientConfig, ConnectorAuthMode, ConnectorAuthorizeParams, ConnectorCreateParams,
    IntrospectionClient, PaginationParams,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;

    let client_id = std::env::var("SLACK_CLIENT_ID")
        .map_err(|_| "SLACK_CLIENT_ID is your own Slack app's client id")?;
    let client_secret = std::env::var("SLACK_CLIENT_SECRET")
        .map_err(|_| "SLACK_CLIENT_SECRET is your own Slack app's client secret")?;

    // 1) Create the connector — the org-level definition of the provider:
    //    your Slack app's credentials and the scopes it asks for. Create is
    //    idempotent on `slug`, so re-running this returns the existing row
    //    rather than duplicating it. `client_secret` is write-only: it goes
    //    up here and is absent from every response.
    //
    //    This assumes the Slack app already exists. Registering a new one is a
    //    second pass: its delivery URL contains the connector id
    //    ({cp-host}/v1/webhooks/slack/{connector.id}), so the connector has to
    //    exist first, and the credentials come back afterwards through
    //    `connectors().update(...)` with `webhook_url` / `client_secret`.
    let params = ConnectorCreateParams {
        slug: Some("slack-support".into()),
        scopes: Some(vec![
            "chat:write".into(),
            "channels:read".into(),
            "app_mentions:read".into(),
        ]),
        api_hosts: Some(vec!["slack.com".into()]),
        client_id: Some(client_id),
        client_secret: Some(client_secret),
        ..ConnectorCreateParams::new("Slack (support)", "slack", ConnectorAuthMode::OauthStored)
    };
    let connector = client.connectors().create(&params).await?;
    println!(
        "connector -> {} ({}), status={}",
        connector.slug,
        connector.id,
        connector.status.as_str(),
    );

    // 2) Mint the install link. This is the whole point of the surface: the
    //    URL below is what you put in front of *your* customer, in your own
    //    product, so their Slack workspace connects to an agent.
    //
    //    `requires_runtime` is derived server-side from the provider — read
    //    it rather than hardcoding which providers are chat providers. When
    //    it is true, `runtime` names the agent that answers the messages, and
    //    omitting it is a 422.
    let runtime = std::env::var("INTROSPECTION_RUNTIME").ok();
    if connector.requires_runtime && runtime.is_none() {
        return Err(format!(
            "{} delivers conversations, so INTROSPECTION_RUNTIME must name the runtime that replies",
            connector.provider
        )
        .into());
    }

    let install = client
        .connectors()
        .authorize(
            connector.id,
            &ConnectorAuthorizeParams {
                runtime: runtime.map(Into::into),
                // The default (600s) suits following the link immediately.
                // Raise it when the link is emailed to someone else — an
                // admin does not open it in ten minutes. Ceiling is one day.
                expires_in: Some(3600),
                ..Default::default()
            },
        )
        .await?;
    // For Pipedream, select an app with
    // `client.connectors().list_apps(connector.id, Some("sheets"), Some(5))`
    // and pass `app: Some("google_sheets".into())`. Enable
    // `allow_progressive_scopes` only if the runtime tolerates partial grants.
    println!("install link -> {}", install.authorize_url);
    println!(
        "  valid for {}s (until {})",
        install.expires_in, install.expires_at
    );
    //    The URL carries a single-use `state`: it is a bearer capability for
    //    exactly one install. Hand it to one recipient, do not cache it, and
    //    mint a fresh one per customer — two calls return two different URLs.

    // 3) List what connected. For Slack each connection is one workspace that
    //    completed the install; `member_id` is the workspace's customer member
    //    and `runtime_group_id` is the agent answering it. Tokens are never
    //    serialized.
    let mut connections = Vec::new();
    let mut listing = client
        .connectors()
        .connections
        .list(connector.id, &PaginationParams::default());
    while let Some(connection) = listing.next().await {
        let connection = connection?;
        println!(
            "  connection {}: subject={}, status={}, member={}",
            connection.id,
            connection.subject_type.as_str(),
            connection.status.as_str(),
            connection
                .member_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "-".into()),
        );
        connections.push(connection);
    }
    if connections.is_empty() {
        println!("  (none yet — open the install link above to connect one)");
    }

    // 4) Disconnect one. Revoking destroys the provider token behind that one
    //    connection; the connector and its other connections are untouched,
    //    and the customer must re-consent through a fresh install link.
    if std::env::var("REVOKE_FIRST_CONNECTION").as_deref() == Ok("1") {
        if let Some(connection) = connections.first() {
            client
                .connectors()
                .connections
                .revoke(connector.id, connection.id)
                .await?;
            println!("revoked connection {}", connection.id);
        }
    }

    client.shutdown()?;
    Ok(())
}
