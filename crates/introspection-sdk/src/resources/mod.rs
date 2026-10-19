//! Resources reachable from [`crate::IntrospectionClient`].
//!
//! - [`Runtimes`] — read and resolve `/v1/runtimes`, and steer a runtime's
//!   lanes or yank it through `update()`; obtain a
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
//! - [`Endpoints`] — `GET /v1/endpoints` lookup: the project's endpoint
//!   bindings, such as the `kind: mcp` binding naming an MCP server.
//! - [`Members`] — `/v1/members` list, read, invite and update: the customer
//!   members an integrator's identity assertions mint, and the `tags` and
//!   `metadata` it labels them with.
//! - [`Automations`] — Data Plane `/v1/automations` CRUD plus `trigger()`:
//!   scheduled prompts, one-off reminders, and platform work.
//! - [`MemberConnections`] — Data Plane `/v1/connections` CRUD: the apps
//!   members connected for themselves.
//! - [`Images`] — Data Plane `/v1/images`: what an eval task's compose
//!   services run as.
//!
//! Project administration, each on an org credential (a member session or
//! access token; a project API key carries none of these scopes):
//!
//! - [`Projects`] — `/v1/projects` CRUD, by slug or id.
//! - [`ApiKeys`] — `/v1/api-keys`: list, create, update, delete (or rotate
//!   with a grace period) and acknowledge.
//! - [`Applications`] — `/v1/applications` CRUD with each Application's
//!   [`ApplicationIdps`], [`ApplicationSecrets`] and [`ApplicationKeys`].
//! - [`Credentials`] and [`Variables`] — what a project's runtimes read.
//! - [`Integrations`] — the org's provider installs.
//! - [`Judges`] — read, and toggle or resample, the judges a Recipe declares.
//! - [`Oidc`] — `GET /v1/oidc/me`, who the credential is.
//!
//! The Data Plane namespaces here (automations, connections and images) are
//! reached through [`crate::DataPlaneResources`], which the client and the
//! runner both implement.
//!
//! A resource the `introspection` CLI addresses gets its typed client here
//! first, so the CLI calls this crate rather than building JSON by hand.
//! Writing endpoint bindings is the one CLI surface not here yet. Two things
//! stay out by design: authoring runtimes, recipes, experiments and judges
//! happens by publishing a Recipe, and the `introspection dev` tunnel
//! (`/v1/remotes/connect`) is a protobuf WebSocket protocol owned by the CLI's
//! `introspection-remote-protocol` crate.

pub mod aauth;
pub mod annotations;
pub mod api_keys;
pub mod applications;
pub mod automations;
pub mod connections;
pub mod connectors;
pub mod credentials;
pub mod endpoints;
pub mod experiments;
pub mod images;
pub mod integrations;
pub mod judges;
pub mod members;
pub mod oidc;
pub mod projects;
pub mod recipes;
pub mod repositories;
pub mod runtimes;
pub mod tags;
pub mod variables;

pub use aauth::{Aauth, Approvals, Missions};
pub use annotations::Annotations;
pub use api_keys::ApiKeys;
pub use applications::{ApplicationIdps, ApplicationKeys, ApplicationSecrets, Applications};
pub use automations::Automations;
pub use connections::MemberConnections;
pub use connectors::{Connections, Connectors};
pub use credentials::Credentials;
pub use endpoints::Endpoints;
pub use experiments::{ExperimentHandle, Experiments};
pub use images::Images;
pub use integrations::Integrations;
pub use judges::Judges;
pub use members::Members;
pub use oidc::Oidc;
pub use projects::Projects;
pub use recipes::Recipes;
pub use repositories::{Repositories, RepositoryContents};
pub use runtimes::{RuntimeHandle, Runtimes};
pub use tags::Tags;
pub use variables::Variables;

/// `path` with `?project=` appended when a project is named.
pub(crate) fn with_project(
    path: &str,
    project: Option<&crate::api::schemas::StringOrUuid>,
) -> String {
    match project {
        Some(project) => format!("{path}?project={}", experiments::encode_project(project)),
        None => path.to_string(),
    }
}
