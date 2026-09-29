//! Connect a custom MCP app (Linear by default) end to end: find it in the open
//! MCP registry, discover its OAuth server, create the connector, and mint an
//! install link that also binds the MCP server to a runtime.
//!
//! Run with `INTROSPECTION_RUNTIME` set. `CUSTOM_APP` defaults to `linear`;
//! set `MCP_URL` to skip the registry search and use that server directly.
//! `MCP_SERVER_ID` defaults to the connector's provider slug and must match the
//! Recipe's `package.json#pi.mcp.servers[].id`. `INTROSPECTION_ENVIRONMENT`
//! defaults to `production`.
//!
//! ```sh
//! cargo run --example connectors-custom-app
//! ```

use std::error::Error;

use introspection_sdk::{
    ClientConfig, ConnectorAuthMode, ConnectorAuthorizeBinding, ConnectorAuthorizeParams,
    ConnectorCreateParams, ConnectorCustomAppSearchParams, ConnectorOAuthDiscoveryParams,
    IntrospectionClient,
};

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    let runtime = std::env::var("INTROSPECTION_RUNTIME")
        .map_err(|_| "INTROSPECTION_RUNTIME must name the runtime receiving this connection")?;
    let environment =
        std::env::var("INTROSPECTION_ENVIRONMENT").unwrap_or_else(|_| "production".into());
    let requested_app = std::env::var("CUSTOM_APP").unwrap_or_else(|_| "linear".into());

    // 1) Find the app's MCP server in the open registry, unless given one.
    let (name, mcp_url) = match std::env::var("MCP_URL") {
        Ok(url) => (requested_app.clone(), url),
        Err(_) => {
            let listings = client
                .connectors()
                .search_custom_apps(&ConnectorCustomAppSearchParams {
                    limit: Some(10),
                    ..ConnectorCustomAppSearchParams::new(requested_app.clone())
                })
                .await?;
            let listing = listings
                .into_iter()
                .find(|listing| listing.mcp_url.is_some())
                .ok_or_else(|| {
                    format!("no registry listing with an MCP server: {requested_app}")
                })?;
            let url = listing.mcp_url.clone().unwrap_or_default();
            println!("registry -> {} ({})", listing.name, url);
            (listing.name, url)
        }
    };

    // 2) Discover the OAuth server behind the MCP URL. This may register an
    //    OAuth client with the provider; reuse what it returns in `create`
    //    so a second client is not registered.
    let discovered = client
        .connectors()
        .discover_oauth(&ConnectorOAuthDiscoveryParams::new(mcp_url.clone()))
        .await?;
    println!(
        "discovery -> registration={}, scopes={:?}",
        discovered
            .client_registration
            .as_ref()
            .map(|method| method.as_str())
            .unwrap_or("none (supply a client_id by hand)"),
        discovered.scopes_supported,
    );

    // 3) Create the connector. A custom app named "Linear" gets provider
    //    `linear`, which is runtime-bound: authorize must name a runtime.
    let provider = slugify(&name);
    let api_host = reqwest::Url::parse(&mcp_url)?
        .host_str()
        .ok_or("MCP_URL has no host")?
        .to_string();
    let connector = client
        .connectors()
        .create(&ConnectorCreateParams {
            slug: Some(provider.clone()),
            environment: Some(environment.clone()),
            issuer: Some(mcp_url.clone()),
            api_hosts: Some(vec![api_host]),
            client_id: discovered.client_id.clone(),
            client_secret: discovered.client_secret.clone(),
            scopes: Some(discovered.scopes_supported.clone()),
            ..ConnectorCreateParams::new(
                name.clone(),
                provider.clone(),
                ConnectorAuthMode::OauthStored,
            )
        })
        .await?;
    println!("connector -> {} ({})", connector.slug, connector.id);

    // 4) Mint the install link. The binding makes a successful grant also
    //    write the runtime's MCP endpoint, in the same transaction as the
    //    connection, so the runtime is never authorized but unbound.
    let mcp_server_id = std::env::var("MCP_SERVER_ID").unwrap_or_else(|_| provider.clone());
    let authorization = client
        .connectors()
        .authorize(
            connector.id,
            &ConnectorAuthorizeParams {
                runtime: Some(runtime.into()),
                binding: Some(ConnectorAuthorizeBinding::new(
                    environment,
                    mcp_server_id,
                    mcp_url,
                )),
                ..Default::default()
            },
        )
        .await?;

    // 5) A human opens this URL to consent.
    println!("{name} authorization -> {}", authorization.authorize_url);

    client.shutdown()?;
    Ok(())
}
