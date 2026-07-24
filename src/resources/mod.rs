//! CP-side resources reachable from [`crate::IntrospectionClient`].
//!
//! - [`Runtimes`] — read `/v1/runtimes` and run stable or exact selectors;
//!   execution uses explicit selectors through [`Runtimes::run`].
//! - [`Experiments`] — `GET/POST/PATCH/DELETE /v1/experiments` plus
//!   execution and lifecycle (`/run` / `/start` / `/end` / `/cancel`).
//! - [`Recipes`] — `GET/POST/PATCH/DELETE /v1/recipes`. Pure CRUD —
//!   recipes describe a (repo, git_ref, git_commit_sha) tuple used by
//!   platform-managed runtime versions.

pub mod experiments;
pub mod projects;
pub mod recipes;
pub mod repositories;
pub mod runtimes;

pub use experiments::Experiments;
pub use projects::Projects;
pub use recipes::Recipes;
pub use repositories::Repositories;
pub use runtimes::Runtimes;
