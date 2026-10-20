//! A Person Server on hyper.
//!
//!   PS_ISSUER=https://ps.acme.example PS_TRUSTED=https://api.introspection.dev \
//!   PS_ADMIN_TOKEN=secret cargo run -p aauth-person-server --example server
//!
//! Put it behind TLS at `PS_ISSUER`; it listens on `PS_ADDR` (default
//! 127.0.0.1:3400). Approvals print to stdout; decide them with
//! `POST /consent/{id}/decision` and the admin token.

use std::convert::Infallible;
use std::sync::Arc;

use aauth_person_server::{Approval, ApprovalKind, Config, Directory, Hooks, Person, PersonServer};
use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;

/// Speaks for anyone whose login hint is `user:<email>`; their manager is
/// whoever `PS_APPROVER` names.
struct Emails;

#[async_trait]
impl Directory for Emails {
    async fn person(&self, hint: &str, _tenant: &str) -> Option<Person> {
        let email = hint.strip_prefix("user:")?;
        Some(Person {
            email: Some(email.to_string()),
            ..Person::new(email)
        })
    }

    async fn approver(&self, _kind: ApprovalKind, _person: &Person) -> Option<Person> {
        std::env::var("PS_APPROVER").ok().map(Person::new)
    }
}

struct Print;

#[async_trait]
impl Hooks for Print {
    async fn on_approval(&self, approval: &Approval) {
        println!(
            "approval {} for {}: {:?} {}",
            approval.id, approval.approver.id, approval.kind, approval.description
        );
    }
}

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let issuer = env("PS_ISSUER", "https://ps.example");
    let trusted = env("PS_TRUSTED", "https://api.introspection.dev");
    let mut config = Config::new(&issuer, Emails, trusted.split(',').map(str::trim)).hooks(Print);
    if let Ok(pem) = std::env::var("PS_SIGNING_KEY") {
        config = config
            .signing_key(aauth_person_server::aauth_protocol::SigningKey::from_pkcs8_pem(&pem)?);
    }
    if let Ok(secret) = std::env::var("PS_SUBJECT_SECRET") {
        config = config.subject_secret(secret);
    }
    if let Ok(token) = std::env::var("PS_ADMIN_TOKEN") {
        config = config.admin_token(token);
    }
    let ps = Arc::new(PersonServer::new(config)?);
    let addr = env("PS_ADDR", "127.0.0.1:3400");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Person Server {issuer} on {addr}");
    loop {
        let (stream, _) = listener.accept().await?;
        let ps = ps.clone();
        tokio::spawn(async move {
            let service = service_fn(move |request: http::Request<Incoming>| {
                let ps = ps.clone();
                async move {
                    let (parts, body) = request.into_parts();
                    let body = match Limited::new(body, 256 * 1024).collect().await {
                        Ok(collected) => collected.to_bytes(),
                        Err(_) => Bytes::new(),
                    };
                    let response = ps.handle(http::Request::from_parts(parts, body)).await;
                    Ok::<_, Infallible>(response.map(Full::new))
                }
            });
            if let Err(e) = http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
            {
                eprintln!("connection error: {e}");
            }
        });
    }
}
