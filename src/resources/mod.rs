//! CP-side resources reachable from [`crate::IntrospectionClient`].
//!
//! - [`Runtimes`] — read and resolve `/v1/runtimes`; obtain a
//!   [`RuntimeHandle`] via `client.runtimes().handle(id)` for `.run()`.
//! - [`Experiments`] — `GET /v1/experiments` lookup plus lifecycle
//!   (`/start` / `/end` / `/cancel`); obtain an [`ExperimentHandle`]
//!   via `client.experiment(id, project)` for `.run()`.
//! - [`Recipes`] — `GET /v1/recipes` lookup. Recipes describe a
//!   (repo, git_ref, git_commit_sha) tuple used by platform-managed runtime
//!   versions.
//! - [`Connectors`] — `/v1/connectors` CRUD with [`Connections`] nested
//!   under `.connections`, plus `authorize()`, which mints the consent URL
//!   (`POST /v1/oauth/connections/authorize`) a Business hands its customer
//!   so their workspace connects to an agent.
//!
//! Read and lifecycle only, with one exception: connectors are full CRUD.
//! A connector is not an authoring artifact but the B2B2C seam an integrator
//! drives from their own backend — creating one and minting install links for
//! their customers is runner-plane work, not operator work. Authoring the rest
//! — creating, editing, or deleting runtimes, recipes and experiments, and
//! administering projects, repositories, keys, and bindings — lives in the
//! CLI, not here.

pub mod annotations;
pub mod connectors;
pub mod experiments;
pub mod recipes;
pub mod runtimes;

pub use annotations::{Annotations, ProjectLabels};
pub use connectors::{Connections, Connectors};
pub use experiments::{ExperimentHandle, Experiments};
pub use recipes::Recipes;
pub use runtimes::{RuntimeHandle, Runtimes};
