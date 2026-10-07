//! Resources reachable from [`crate::IntrospectionClient`].
//!
//! - [`Runtimes`] — read and resolve `/v1/runtimes`; obtain a
//!   [`RuntimeHandle`] via `client.runtimes().handle(id)` or
//!   `client.runtimes().by_slug(slug)` for `.run()`.
//! - [`Experiments`] — `/v1/experiments` CRUD plus run lifecycle
//!   (`/start` / `/end` / `/cancel`); obtain an [`ExperimentHandle`]
//!   via `client.experiment(id, project)` for `.run()`.
//! - [`Recipes`] — `GET /v1/recipes` lookup. Recipes describe a
//!   (repo, git_ref, git_commit_sha) tuple used by platform-managed runtime
//!   versions.
//! - [`Repositories`] — `GET /v1/repositories` lookup: the Git source a
//!   recipe pins, resolved to its credential-free transport URL — and
//!   [`RepositoryContents`], its files read through the Data Plane, and its
//!   commits via [`Repositories::commits`] / [`Repositories::commit`], and
//!   branch merges via [`Repositories::merge`].
//! - [`Connectors`] — `/v1/connectors` CRUD with [`Connections`] nested
//!   under `.connections`, plus `authorize()`, which mints the consent URL
//!   (`POST /v1/oauth/connections/authorize`) a Business hands its customer
//!   so their workspace connects to an agent.
//! - [`Members`] — `/v1/members` list, read, invite and update: the customer
//!   members an integrator's identity assertions mint, and the `tags` and
//!   `metadata` it labels them with.
//! - [`Automations`] — Data Plane `/v1/automations` CRUD plus `trigger()`:
//!   scheduled prompts, one-off reminders, and platform work.
//! - [`MemberConnections`] — Data Plane `/v1/connections` CRUD: the apps
//!   members connected for themselves.
//!
//! The Data Plane namespaces here (automations and connections) are
//! reached through [`crate::DataPlaneResources`], which the client and the
//! runner both implement.
//!
//! Read and lifecycle only, with three exceptions: connectors, automations
//! and connections are full CRUD, members can be invited and relabelled, and a
//! repository's branches can be merged.
//! A connector is not an authoring artifact but the B2B2C seam an integrator
//! drives from their own backend — creating one and minting install links for
//! their customers is runner-plane work, not operator work. Automations are
//! in scope because introspection-cloud#3137 opens them to members: a person
//! schedules follow-ups into their own task, which is product work rather
//! than project administration (the routes are administrator-only until it
//! ships). Authoring the rest
//! — creating, editing, or deleting runtimes, recipes and experiments, and
//! administering projects, repositories, keys, and bindings — lives in the
//! CLI, not here.

pub mod annotations;
pub mod automations;
pub mod connections;
pub mod connectors;
pub mod experiments;
pub mod members;
pub mod recipes;
pub mod repositories;
pub mod runtimes;

pub use annotations::{Annotations, ProjectLabels};
pub use automations::Automations;
pub use connections::MemberConnections;
pub use connectors::{Connections, Connectors};
pub use experiments::{ExperimentHandle, Experiments};
pub use members::Members;
pub use recipes::Recipes;
pub use repositories::{Repositories, RepositoryContents};
pub use runtimes::{RuntimeHandle, Runtimes};
