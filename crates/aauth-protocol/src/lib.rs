//! AAuth ([draft-hardt-oauth-aauth-protocol]) for Rust: how an agent proves
//! who it is, and what a person has allowed it to do, on every request.
//!
//! Every AAuth request is signed with an RFC 9421 HTTP Message Signature
//! under the AAuth profile, and names the signer's key in `Signature-Key`
//! ([draft-hardt-httpbis-signature-key]). Usually the key is a JWT whose
//! `cnf.jwk` is the signing key: an **agent token** from the agent's
//! provider, a **person token** from the person's Person Server, or an
//! **auth token** the Person Server issued for one resource. This crate is
//! the part of the protocol every role shares:
//!
//! | Module | What it does |
//! | --- | --- |
//! | [`keys`], [`jwk`], [`jwt`] | Ed25519 signing keys, public JWKs and RFC 7638 thumbprints, compact JWS |
//! | [`httpsig`] | Sign a request; verify one and learn which key signed it |
//! | [`token`] | Mint tokens; verify one against its issuer's keys and the request's signer |
//! | [`discovery`] | Find an issuer's keys through its metadata document, over a [`discovery::Fetch`] you choose |
//! | [`resource`] | A resource's side: [`resource::authenticate`] a request, mint resource tokens, challenge |
//! | [`requirement`] | The `AAuth-Requirement` header |
//! | [`mission`], [`budget`] | What a person approved, and what it may spend |
//! | [`client`] | The agent's side of a Person Server conversation, and an SSRF guard (feature `client`) |
//!
//! The Person Server itself is the [`aauth-person-server`] crate.
//!
//! # A request, end to end
//!
//! An agent provider vouches for an agent's key; the agent signs a request
//! with it; a resource verifies the signature and the token.
//!
//! ```
//! use std::sync::Arc;
//!
//! use aauth_protocol::discovery::{JwksResolver, StaticFetch};
//! use aauth_protocol::httpsig::{header_map, sign_jwt, Incoming, Outgoing, VerifyOptions};
//! use aauth_protocol::resource::authenticate;
//! use aauth_protocol::token::{mint, TokenKind};
//! use aauth_protocol::SigningKey;
//! use serde_json::json;
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), aauth_protocol::Error> {
//! // The agent provider publishes its keys and vouches for the agent's.
//! let provider = SigningKey::generate();
//! let agent = SigningKey::generate();
//! let now = aauth_protocol::now_unix();
//! let agent_token = mint(&provider, TokenKind::Agent, &json!({
//!     "iss": "https://agents.example",
//!     "dwk": "aauth-agent.json",
//!     "sub": "aauth:travel@agents.example",
//!     "iat": now,
//!     "exp": now + 3600,
//!     "cnf": { "jwk": agent.public_jwk() },
//! }));
//!
//! // The agent signs its request.
//! let body = br#"{"offer_id":"fl-qf1"}"#;
//! let signed = sign_jwt(&agent, &agent_token, &Outgoing {
//!     method: "POST",
//!     url: "https://api.booking.example/v1/reserve",
//!     content_type: None,
//!     body: Some(body),
//!     created: now,
//! })?;
//!
//! // The resource verifies it, finding the provider's keys by discovery.
//! // `StaticFetch` stands in for the network here; use `ReqwestFetch`.
//! let documents = StaticFetch::new();
//! documents.insert(
//!     "https://agents.example/.well-known/aauth-agent.json",
//!     json!({ "issuer": "https://agents.example", "jwks": provider.jwks() }),
//! );
//! let resolver = JwksResolver::new(Arc::new(documents));
//! let headers = header_map(&signed)?;
//! let request = Incoming {
//!     method: "POST",
//!     authority: "api.booking.example",
//!     path: "/v1/reserve",
//!     query: None,
//!     headers: &headers,
//!     body: Some(body),
//! };
//! let authenticated = authenticate(
//!     &request,
//!     "https://api.booking.example",
//!     &[TokenKind::Agent],
//!     &VerifyOptions::default(),
//!     &resolver,
//! )
//! .await?;
//! assert_eq!(authenticated.token.sub, "aauth:travel@agents.example");
//! assert_eq!(authenticated.signature.thumbprint, agent.thumbprint());
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! - `client` (default): [`client`], and [`discovery::ReqwestFetch`]. Without
//!   it the crate makes no network calls of its own and pulls in no HTTP
//!   client or async runtime.
//!
//! # Status
//!
//! This implements `-11` of the protocol draft. Signing is Ed25519;
//! verification accepts Ed25519 and ES256. The `jwt`, `jwks_uri` and `hwk`
//! `Signature-Key` schemes are supported, `jkt-jwt` and `x509` are not, and
//! revocation is not checked. Until 1.0 the API may change between minor
//! versions.
//!
//! [draft-hardt-oauth-aauth-protocol]: https://github.com/dickhardt/AAuth
//! [draft-hardt-httpbis-signature-key]: https://github.com/dickhardt/signature-key
//! [`aauth-person-server`]: https://docs.rs/aauth-person-server

#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod budget;
pub mod discovery;
pub mod error;
pub mod httpsig;
pub mod identifier;
pub mod jwk;
pub mod jwt;
pub mod keys;
pub mod metadata;
pub mod mission;
pub mod requirement;
pub mod resource;
pub mod token;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub mod client;

mod sf;

pub use budget::Budget;
pub use error::{Error, Result, SignatureError, TokenError};
pub use jwk::Jwk;
pub use keys::SigningKey;
pub use requirement::Requirement;
pub use token::{TokenKind, VerifiedToken};

/// The protocol draft this crate implements.
pub const PROTOCOL_DRAFT: &str = "draft-hardt-oauth-aauth-protocol-11";

/// `BASE64URL(SHA-256(bytes))`, unpadded: the encoding every `*_s256` value uses.
pub fn s256(bytes: impl AsRef<[u8]>) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(bytes.as_ref()))
}

/// 128 random bits, base64url: for `jti`s and other unguessable identifiers.
pub(crate) fn random_id() -> String {
    use base64::Engine;
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the operating system provides randomness");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Whether `a` equals `b`, in time that depends on their lengths alone:
/// for a secret a peer presents, such as a thumbprint or a bearer token.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Seconds since the Unix epoch.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
