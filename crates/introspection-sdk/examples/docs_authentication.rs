//! Snippets for the docs' Authentication page
//! (`content/reference/authentication.mdx`). Each marked docs region
//! is what the page shows; the code around it is setup the page leaves out.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx \
//! INTROSPECTION_SERVICE_ACCOUNT_CLIENT_ID=intro_app_xxx \
//! INTROSPECTION_SERVICE_ACCOUNT_CLIENT_SECRET=intro_sk_xxx \
//! INTROSPECTION_PROJECT=my-project \
//!   cargo run --example docs_authentication
//! ```

use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    api_key()?;
    service_account().await?;
    if let (Ok(code), Ok(redirect_uri), Ok(code_verifier)) = (
        std::env::var("INTROSPECTION_AUTH_CODE"),
        std::env::var("INTROSPECTION_REDIRECT_URI"),
        std::env::var("INTROSPECTION_CODE_VERIFIER"),
    ) {
        hosted_login(code, redirect_uri, code_verifier).await?;
    }
    Ok(())
}

fn api_key() -> Result<(), Box<dyn Error>> {
    // #region docs:reference/authentication/api-key
    use introspection_sdk::{ClientConfig, IntrospectionClient};

    // Falls back to INTROSPECTION_TOKEN when the config carries no token.
    let client = IntrospectionClient::new(ClientConfig::default())?;

    // Or pass it explicitly.
    let explicit = IntrospectionClient::new(ClientConfig::with_token(std::env::var(
        "INTROSPECTION_TOKEN",
    )?))?;
    // #endregion docs:reference/authentication/api-key
    client.shutdown()?;
    explicit.shutdown()?;
    Ok(())
}

async fn service_account() -> Result<(), Box<dyn Error>> {
    // #region docs:reference/authentication/service-account
    use introspection_sdk::{auth::ServiceAccountTokenParams, IntrospectionClient};

    let client = IntrospectionClient::from_service_account(
        ServiceAccountTokenParams::builder()
            .client_id(std::env::var("INTROSPECTION_SERVICE_ACCOUNT_CLIENT_ID")?)
            .client_secret(std::env::var(
                "INTROSPECTION_SERVICE_ACCOUNT_CLIENT_SECRET",
            )?)
            .project(std::env::var("INTROSPECTION_PROJECT")?)
            .build()?,
        None,
    )
    .await?;
    // #endregion docs:reference/authentication/service-account
    client.shutdown()?;
    Ok(())
}

async fn hosted_login(
    code: String,
    redirect_uri: String,
    code_verifier: String,
) -> Result<(), Box<dyn Error>> {
    // #region docs:reference/authentication/hosted-login
    use introspection_sdk::auth::{authorization_code_token, AuthorizationCodeParams};

    // In your backend, after the hosted-login redirect.
    let token = authorization_code_token(
        AuthorizationCodeParams::builder()
            .code(code) // from the redirect URI
            .client_id(std::env::var("INTROSPECTION_SPA_CLIENT_ID")?) // public client, no secret
            .redirect_uri(redirect_uri) // must match the authorize call
            .code_verifier(code_verifier) // pairs with the PKCE challenge
            .build()?,
    )
    .await?;
    // #endregion docs:reference/authentication/hosted-login
    println!("expires in {}s", token.expires_in);
    Ok(())
}
