//! Mint a short-lived token for a hosted repository from inside a Runtime's
//! session: only a runner's credential qualifies, so run this where the agent
//! runs. Any other credential is refused with a 403.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=<the runner's session token> \\
//! INTROSPECTION_BASE_API_URL=http://localhost:8000 \\
//! INTRO_REPOSITORY_ID=<repository uuid> \\
//!   cargo run --example repository-access-token
//! ```

use std::error::Error;

use introspection_sdk::{ClientConfig, IntrospectionClient, RepositoryAccessParams};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    let repository_id = std::env::var("INTRO_REPOSITORY_ID")?.parse()?;

    let grant = client
        .repositories()
        .access_token(
            repository_id,
            &RepositoryAccessParams {
                permissions: [("contents".to_string(), "read".to_string())].into(),
                lane: None,
                project_id: None,
            },
        )
        .await?;
    // `grant.token` is a `Secret`: hand `grant.token.expose()` to git, never a log.
    println!(
        "{} token for {}, expires {}",
        grant.provider, grant.repository, grant.expires_at
    );
    Ok(())
}
