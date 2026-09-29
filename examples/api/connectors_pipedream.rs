//! Create a Pipedream connector and authorize one downstream application.
//!
//! Run with either `PIPEDREAM_CONNECTOR_ID`, or `PIPEDREAM_PROJECT_ID` (the
//! Pipedream Connect project id, `proj_...`), `PIPEDREAM_CLIENT_ID`, and
//! `PIPEDREAM_CLIENT_SECRET`. Also set `INTROSPECTION_RUNTIME`.
//! `PIPEDREAM_APP` defaults to `google_sheets`.
//!
//! ```sh
//! cargo run --example connectors-pipedream
//! ```

use std::{collections::HashMap, error::Error};

use introspection_sdk::{
    ClientConfig, ConnectorAppListParams, ConnectorAuthMode, ConnectorAuthorizeParams,
    ConnectorCreateParams, IntrospectionClient,
};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    let runtime = std::env::var("INTROSPECTION_RUNTIME")
        .map_err(|_| "INTROSPECTION_RUNTIME must name the runtime receiving this connection")?;
    let requested_app = std::env::var("PIPEDREAM_APP").unwrap_or_else(|_| "google_sheets".into());

    let connector_id = match std::env::var("PIPEDREAM_CONNECTOR_ID") {
        Ok(id) => Uuid::parse_str(&id)?,
        Err(_) => {
            let project_id = std::env::var("PIPEDREAM_PROJECT_ID")?;
            let client_id = std::env::var("PIPEDREAM_CLIENT_ID")?;
            let client_secret = std::env::var("PIPEDREAM_CLIENT_SECRET")?;
            // A pipedream connector requires its Connect project id as
            // `provider_workspace_id`. `provider_environment` is derived from
            // the connector's `environment` server-side; do not send it.
            let mut metadata = HashMap::new();
            metadata.insert("provider_workspace_id".into(), project_id.into());

            let connector = client
                .connectors()
                .create(&ConnectorCreateParams {
                    slug: Some("pipedream-connect".into()),
                    client_id: Some(client_id),
                    client_secret: Some(client_secret),
                    metadata: Some(metadata),
                    ..ConnectorCreateParams::new(
                        "Pipedream Connect",
                        "pipedream",
                        ConnectorAuthMode::ClientCredentials,
                    )
                })
                .await?;
            println!("connector -> {} ({})", connector.slug, connector.id);
            connector.id
        }
    };

    let applications = client
        .connectors()
        .list_apps(
            connector_id,
            &ConnectorAppListParams {
                query: Some(requested_app.clone()),
                limit: Some(5),
                ..Default::default()
            },
        )
        .await?;
    let application = applications
        .into_iter()
        .find(|application| application.slug == requested_app)
        .ok_or_else(|| format!("Pipedream application not found: {requested_app}"))?;

    let authorization = client
        .connectors()
        .authorize(
            connector_id,
            &ConnectorAuthorizeParams {
                runtime: Some(runtime.into()),
                app: Some(application.slug.clone()),
                allow_progressive_scopes: std::env::var("PIPEDREAM_PROGRESSIVE_SCOPES").as_deref()
                    == Ok("true"),
                ..Default::default()
            },
        )
        .await?;
    println!(
        "{} authorization -> {}",
        application.name, authorization.authorize_url
    );

    client.shutdown()?;
    Ok(())
}
