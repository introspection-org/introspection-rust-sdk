//! CP-side resources reachable from [`crate::IntrospectionClient`].
//!
//! - [`Runtimes`] — read and resolve `/v1/runtimes`; obtain a
//!   [`RuntimeHandle`] via `client.runtimes().handle(id)` for `.run()`.
//! - [`Experiments`] — `GET/POST/PATCH/DELETE /v1/experiments` plus
//!   lifecycle (`/start` / `/conclude` / `/cancel`); obtain an
//!   [`ExperimentHandle`] via `client.experiment(id, project)` for
//!   `.run()`.
//! - [`Recipes`] — `GET/POST/PATCH/DELETE /v1/recipes`. Pure CRUD —
//!   recipes describe a (repo, git_ref, git_commit_sha) tuple used by
//!   platform-managed runtime versions.

pub mod experiments;
pub mod projects;
pub mod recipes;
pub mod repositories;
pub mod runtimes;

pub use experiments::{ExperimentHandle, Experiments};
pub use projects::Projects;
pub use recipes::Recipes;
pub use repositories::Repositories;
pub use runtimes::{RuntimeHandle, Runtimes};
