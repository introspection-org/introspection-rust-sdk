//! Native email-code sign-in for a `native` Application: email a code, verify
//! it, then call the Data Plane as the signed-in `customer` member.
//!
//! Run with:
//! ```sh
//! INTRO_NATIVE_CLIENT_ID=intro_app_xxx \
//! INTRO_PROJECT=acme \
//! INTRO_EMAIL=user@example.com \
//! INTROSPECTION_BASE_API_URL=http://localhost:8000 \
//!   cargo run --example native-email-code
//! ```
//!
//! The program prompts for the code on stdin. A returning user's code is six
//! digits; a new user's first code is six characters of A-Z and 0-9.

use std::error::Error;
use std::io::{self, BufRead, Write};

use introspection_sdk::auth::{EmailCodeAuth, EmailCodeAuthConfig};
use introspection_sdk::{TaskListParams, Tasks};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let email = std::env::var("INTRO_EMAIL")?;
    let auth = EmailCodeAuth::new(
        EmailCodeAuthConfig::builder()
            .client_id(std::env::var("INTRO_NATIVE_CLIENT_ID")?)
            .project(std::env::var("INTRO_PROJECT")?)
            .build()?,
    )?;

    // Persist the session on every change: each refresh rotates the refresh
    // token, and the old one stops working.
    let mut changes = auth.changes();
    tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            let state = changes.borrow_and_update().clone();
            println!("auth: {:?}", state.event);
        }
    });

    auth.send_code(&email).await?;
    print!("code sent to {email}; enter it: ");
    io::stdout().flush()?;
    let mut code = String::new();
    io::stdin().lock().read_line(&mut code)?;

    let session = auth.verify_code(&email, &code).await?;
    println!(
        "signed in: member={:?} dp={:?} scope={:?}",
        session.member_id, session.dp_url, session.scope
    );

    // A native token is a Data Plane credential. `with_data_plane` refreshes
    // it before expiry and retries once after a 401.
    let page = auth
        .with_data_plane(|dp| async move {
            Tasks::new(dp)
                .list(&TaskListParams::default())
                .next_page()
                .await
        })
        .await?;
    let count = page.map(|p| p.count).unwrap_or(0);
    println!("{count} task(s) visible to this member");

    auth.sign_out().await.ok();
    Ok(())
}
