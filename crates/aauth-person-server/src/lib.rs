//! An AAuth Person Server ([draft-hardt-oauth-aauth-protocol] `-11`,
//! three-party) you can run instead of building one.
//!
//! A Person Server speaks for people when agents want to act for them. This
//! one:
//!
//! - verifies each agent by its HTTP Message Signature and the agent token
//!   it presents, from agent providers you trust;
//! - puts each **mission** an agent proposes to whoever decides for the
//!   person, and issues **person tokens** and **auth tokens** under it;
//! - weighs each auth token request against its mission's **budget**
//!   ([draft-hardt-aauth-budgets]): one that fits what is left is issued at
//!   once and spent, and one that does not goes back to the approver;
//! - turns whatever needs a human into an [`Approval`], which you deliver
//!   however you reach people, and answer with [`PersonServer::decide`].
//!
//! It is a port of [`@introspection-sdk/person-server`]. The two speak the
//! same protocol, and an agent cannot tell them apart.
//!
//! # Running one
//!
//! You supply a [`Directory`]: who you speak for, by the `login_hint` an
//! agent names, and who decides for them. [`Hooks`] tell you when a decision
//! is needed. [`PersonServer::handle`] takes an `http::Request<Bytes>` and
//! returns an `http::Response<Bytes>`, so it mounts behind hyper, axum, or
//! anything else built on the `http` crate; `examples/server.rs` runs it on
//! hyper.
//!
//! ```
//! use aauth_person_server::{
//!     Approval, ApprovalKind, Config, Decision, Directory, Hooks, Person, PersonServer,
//! };
//! use async_trait::async_trait;
//! use bytes::Bytes;
//!
//! struct Staff;
//!
//! #[async_trait]
//! impl Directory for Staff {
//!     async fn person(&self, login_hint: &str, _tenant: Option<&str>) -> Option<Person> {
//!         let email = login_hint.strip_prefix("user:")?.strip_suffix("@acme.example")?;
//!         Some(Person::new(email))
//!     }
//!
//!     async fn approver(&self, kind: ApprovalKind, _person: &Person) -> Option<Person> {
//!         // Managers approve trips; completing one is the traveller's own call.
//!         (kind != ApprovalKind::Completion).then(|| Person::new("dana"))
//!     }
//! }
//!
//! struct Notify;
//!
//! #[async_trait]
//! impl Hooks for Notify {
//!     async fn on_approval(&self, approval: &Approval) {
//!         println!("ask {} about {}", approval.approver.id, approval.description);
//!     }
//! }
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ps = PersonServer::new(
//!     Config::new("https://ps.acme.example", Staff, ["https://api.introspection.dev"])
//!         // In production, a stable key and secret; unset, both are fresh per process.
//!         .signing_key(aauth_protocol::SigningKey::generate())
//!         .subject_secret("a long random secret")
//!         .hooks(Notify),
//! )?;
//!
//! let request = http::Request::get("/.well-known/aauth-person.json").body(Bytes::new())?;
//! let response = ps.handle(request).await;
//! assert_eq!(response.status(), http::StatusCode::OK);
//!
//! // Later, when Dana answers an approval:
//! # let approval_id = "an-approval-id";
//! # let _ =
//! ps.decide(approval_id, Decision::approve().by("dana@acme.example")).await;
//! # Ok(())
//! # }
//! ```
//!
//! # Policy
//!
//! - A mission is always put to the approver, who may set its budget.
//! - An auth token needs a live mission covering the resource token's
//!   issuer, and only those issuers' keys are ever fetched.
//! - A request whose `budget` is in the mission's unit and scale, and fits
//!   what is left, is issued at once and spent. Any other budgeted request
//!   is put to the approver, and spent when approved.
//! - A request with no `budget` is issued within the mission without spending.
//! - Completing a mission is the person's own decision; nothing is issued
//!   under it afterwards.
//! - Each agent key is bound to the first person it names.
//!
//! # Routes
//!
//! | Route | |
//! | --- | --- |
//! | `GET /.well-known/aauth-person.json` | metadata |
//! | `GET /.well-known/jwks.json` | token-signing keys |
//! | `POST /mission` | propose a mission |
//! | `POST /mission/{s256}` | `update` or `completion` |
//! | `POST /person-token` | a person token |
//! | `POST /token` | an auth token, for a resource token |
//! | `GET`, `DELETE /pending/{id}` | poll, or withdraw, a pending request |
//!
//! With [`Config::admin_token`], the management routes are on too, behind
//! `Authorization: Bearer <token>`: `GET /consent[?status=]`,
//! `POST /consent/{id}/decision`, `GET /missions[/{s256}]`, and
//! `PATCH /missions/{s256}` to terminate one.
//!
//! # Deploying
//!
//! - [`MemoryStore`] is one process's memory, and keeps missions and key
//!   bindings forever. For more than one process, implement [`Store`] on a
//!   database: its `spend` and `transition_approval` must be atomic, which
//!   is what makes each budget spend and each decision happen once.
//! - Bound request bodies before handing them over (the example caps them
//!   at 256 KiB), and rate-limit in front: this crate does neither.
//! - Only a trusted agent provider's keys, a mission's resources' keys, and
//!   a resource's own R3 documents are ever fetched, through
//!   `ReqwestFetch::guarded` (the `reqwest` feature, on by default): `https`
//!   only, never an IP literal, `localhost` or a private address. Give
//!   [`Config::fetch`] your own to change that.
//! - An approved auth token is collected once: the poll that receives it
//!   moves the approval to [`ApprovalStatus::Collected`], and later polls
//!   are `410 Gone`.
//!
//! [draft-hardt-oauth-aauth-protocol]: https://github.com/dickhardt/AAuth
//! [draft-hardt-aauth-budgets]: https://github.com/dickhardt/AAuth
//! [`@introspection-sdk/person-server`]: https://www.npmjs.com/package/@introspection-sdk/person-server

#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod directory;
mod error;
mod server;
mod store;
mod time;
mod types;

pub use aauth_protocol;
pub use directory::{Directory, Hooks, NoHooks};
pub use error::{problem, AAuthError};
pub use server::{Config, Endpoints, PersonServer};
pub use store::{MemoryStore, Spend, Store};
pub use types::{
    Agent, Approval, ApprovalKind, ApprovalStatus, Decision, Issued, IssuedKind, LogEntry, Mission,
    Person, Verdict,
};
