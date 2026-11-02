//! The device-code login: show a code, wait for the member to approve it in the
//! browser, renew the login, then end its session.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_BASE_API_URL=http://localhost:8000 \\
//!   cargo run --example device-login
//! ```

use std::error::Error;

use introspection_sdk::auth::{DeviceCodeAuth, DeviceCodeAuthConfig, DeviceCodeParams};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let auth = DeviceCodeAuth::new(DeviceCodeAuthConfig::default())?;

    let device = auth.request_code(&DeviceCodeParams::default()).await?;
    println!(
        "Open {} and enter {}",
        device.verification_uri, device.user_code
    );
    let token = auth.wait_for_token(&device).await?;

    let session_id = token.session_id.as_deref().ok_or("no session id")?;
    let org_id = token.org_id.as_deref().ok_or("no org id")?;
    let refresh_token = token.refresh_token.as_deref().ok_or("no refresh token")?;

    let renewed = auth.refresh(refresh_token, session_id, org_id).await?;
    println!("renewed, expires in {}s", renewed.expires_in);
    // A refresh may omit the refresh token; the old one still names the session.
    let refresh_token = renewed.refresh_token.as_deref().unwrap_or(refresh_token);

    let revoked = auth.revoke(session_id, org_id, Some(refresh_token)).await?;
    println!("session {}", revoked.status);
    Ok(())
}
