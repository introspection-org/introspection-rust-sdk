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
//!
//! Read and lifecycle only. Authoring these objects — creating, editing, or
//! deleting them, and administering projects, repositories, keys, and
//! bindings — is operator work and lives in the CLI, not here.

pub mod experiments;
pub mod recipes;
pub mod runtimes;

pub use experiments::{ExperimentHandle, Experiments};
pub use recipes::Recipes;
pub use runtimes::{RuntimeHandle, Runtimes};
