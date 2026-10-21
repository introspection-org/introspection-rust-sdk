//! An AAuth Access Server ([draft-hardt-oauth-aauth-protocol] `-11`,
//! four-party, §Access Server Federation): a resource's own policy, beside
//! the person's.
//!
//! A resource that keeps its own policy names this server as the `aud` of
//! its resource tokens. The person's Person Server redeems them here: it
//! signs the request with its own key (`sig=jwks_uri`, `aauth-person.json`)
//! and sends the resource token, the agent's token and the token the
//! resource saw. This server verifies all of it, asks the resource's
//! [`Policy`], and issues an auth token for the resource, refuses, or makes
//! the Person Server wait while someone decides. Both sides must agree: the
//! Person Server for the person (mission, budget, approver), and this server
//! for the resource (its customers, its limits).
//!
//! It is a port of [`@introspection-sdk/access-server`]. The Person Server's
//! side is [`aauth_protocol::federation`], which [`aauth-person-server`]
//! runs for any resource token whose `aud` is not itself.
//!
//! ```
//! use aauth_access_server::{AccessRequest, AccessServer, Config, Verdict};
//! use bytes::Bytes;
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Flight Sector serves the travellers of companies with an account.
//! let policy = |request: &AccessRequest| match request.tenant.as_deref() {
//!     Some("acme") => Verdict::allow(),
//!     _ => Verdict::deny("no Flight Sector account"),
//! };
//! let access = AccessServer::new(
//!     Config::new("https://as.flightsector.example", policy)
//!         .resources(["https://booking.flightsector.example"])
//!         .person_servers(["https://ps.acme.example"])
//!         .agent_providers(["https://api.introspection.dev"])
//!         .signing_key(aauth_protocol::SigningKey::generate()),
//! )?;
//!
//! let request = http::Request::get("/.well-known/aauth-access.json").body(Bytes::new())?;
//! let response = access.handle(request).await;
//! assert_eq!(response.status(), http::StatusCode::OK);
//! # Ok(())
//! # }
//! ```
//!
//! # Routes
//!
//! | Route | |
//! | --- | --- |
//! | `GET /.well-known/aauth-access.json` | metadata |
//! | `GET /.well-known/jwks.json` | auth-token-signing keys |
//! | `POST /token` | a Person Server's token request |
//! | `GET /pending/{id}` | a Person Server polls a request the policy waits on |
//!
//! # What it checks
//!
//! - The request is signed by a Person Server it takes requests from, and
//!   the resource token names that Person Server as `ps` and this server as
//!   `aud`, from a resource it decides for.
//! - The agent token is from a trusted agent provider and binds the key the
//!   resource token's `agent_jkt` names.
//! - The presented token is a person token from that Person Server, or an
//!   auth token this server issued, for the resource and the agent's key,
//!   matching the resource token's `presented_jti`, `sub`, `mission_s256`
//!   and `tenant` exactly.
//!
//! - A `budget` the Person Server sends ([draft-hardt-aauth-budgets]
//!   `#as-token-endpoint`) must be the resource token's, no higher. The auth
//!   token carries it, or less when the policy's [`Verdict::Allow`] lowers
//!   it, and carries none when none was sent.
//!
//! Only those parties' keys, and the resource's own R3 documents, are ever
//! fetched.
//!
//! # Deploying
//!
//! - Pending requests are kept in this process's memory, so run one process
//!   or route each Person Server's polls back to the one that answered.
//! - Bound request bodies and rate-limit in front: this crate does neither.
//!
//! # Not yet
//!
//! `requirement=claims` and `clarification`, `402` payment, `revocation_endpoint`,
//! and the `subagent_token` and `upstream_token` parameters (a request
//! carrying either is refused `invalid_request`).
//!
//! [draft-hardt-oauth-aauth-protocol]: https://github.com/dickhardt/AAuth
//! [draft-hardt-aauth-budgets]: https://github.com/dickhardt/AAuth
//! [`@introspection-sdk/access-server`]: https://www.npmjs.com/package/@introspection-sdk/access-server
//! [`aauth-person-server`]: https://docs.rs/aauth-person-server

#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod error;
mod server;
mod types;
mod verify;

pub use aauth_protocol;
pub use error::{problem, AAuthError};
pub use server::{AccessServer, Config, Endpoints, Hooks, NoHooks, Policy, MAX_TOKEN_TTL};
pub use types::{
    AccessDecision, AccessRequest, DecisionStatus, Grant, IssuedAccess, PendingAccess,
    PendingStatus, RequestingAgent, Verdict, VerifiedClaims,
};
