//! A Person Server on hyper, configured by flags with environment defaults.
//!
//!   PS_ADMIN_TOKEN=secret cargo run -p aauth-person-server --example server -- \
//!     --issuer https://ps.acme.example --default-tenant acme
//!
//! `--help` lists every flag. Put it behind TLS at `--issuer`. Approvals print
//! to stdout; decide them with `POST /consent/{id}/decision` and the admin
//! token.

use std::convert::Infallible;
use std::sync::Arc;

use aauth_person_server::aauth_protocol::SigningKey;
use aauth_person_server::{
    Approval, ApprovalKind, Config, Directory, Hooks, Person, PersonServer, DEFAULT_TENANT,
};
use async_trait::async_trait;
use bytes::Bytes;
use clap::Parser;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;

/// An AAuth Person Server that speaks for anyone whose login hint is
/// `user:<email>`.
#[derive(Parser)]
#[command(
    after_help = "Environment only, since anything on the host can read a process's arguments:
  PS_SIGNING_KEY     the token-signing key, an Ed25519 PKCS#8 PEM (unset: one per process)
  PS_SUBJECT_SECRET  derives each person's directed sub (unset: one per process)
  PS_ADMIN_TOKEN     bearer token for the management routes (unset: they are off)

Unset, the key and the secret change on every restart, so tokens and subs do too."
)]
struct Args {
    /// This server's identifier: https://host, with no port or path.
    #[arg(long, env = "PS_ISSUER", default_value = "https://ps.example")]
    issuer: String,
    /// The agent providers it trusts, comma-separated.
    #[arg(
        long,
        env = "PS_TRUSTED",
        default_value = "https://api.introspection.dev",
        value_delimiter = ','
    )]
    trusted: Vec<String>,
    /// Where it listens.
    #[arg(long, env = "PS_ADDR", default_value = "127.0.0.1:3400")]
    addr: String,
    /// Where its routes live under the issuer, e.g. /ps.
    #[arg(long, env = "PS_BASE_PATH", default_value = "")]
    base_path: String,
    /// Who decides each request; unset, the person decides their own.
    #[arg(long, env = "PS_APPROVER")]
    approver: Option<String>,
    /// The tenant a request that names none is under.
    #[arg(long, env = "PS_DEFAULT_TENANT", default_value = DEFAULT_TENANT, conflicts_with = "require_tenant")]
    default_tenant: String,
    /// Refuse a request that names no tenant.
    #[arg(long, env = "PS_REQUIRE_TENANT", value_parser = clap::builder::BoolishValueParser::new())]
    require_tenant: bool,
}

struct Emails {
    approver: Option<String>,
}

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
        self.approver.as_deref().map(Person::new)
    }
}

struct Print;

#[async_trait]
impl Hooks for Print {
    async fn on_approval(&self, approval: &Approval) {
        println!(
            "approval {} in {} for {}: {:?} {}",
            approval.id, approval.tenant, approval.approver.id, approval.kind, approval.description
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut config = Config::new(
        &args.issuer,
        Emails {
            approver: args.approver,
        },
        args.trusted.iter().map(|t| t.trim()),
    )
    .hooks(Print)
    .base_path(&args.base_path);
    config = if args.require_tenant {
        config.require_tenant()
    } else {
        config.default_tenant(args.default_tenant)
    };
    match std::env::var("PS_SIGNING_KEY") {
        Ok(pem) => config = config.signing_key(SigningKey::from_pkcs8_pem(&pem)?),
        Err(_) => {
            eprintln!("PS_SIGNING_KEY is unset: tokens are signed with a key for this process only")
        }
    }
    match std::env::var("PS_SUBJECT_SECRET") {
        Ok(secret) => config = config.subject_secret(secret),
        Err(_) => eprintln!("PS_SUBJECT_SECRET is unset: directed subs change on restart"),
    }
    if let Ok(token) = std::env::var("PS_ADMIN_TOKEN") {
        config = config.admin_token(token);
    }
    let ps = Arc::new(PersonServer::new(config)?);
    let listener = tokio::net::TcpListener::bind(&args.addr).await?;
    println!("Person Server {} on {}", args.issuer, args.addr);
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
