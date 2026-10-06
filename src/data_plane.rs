//! [`DataPlaneResources`] — the one Data Plane surface that
//! [`IntrospectionClient`] and [`Runner`] both expose, and that the Data
//! Plane client [`EmailCodeAuth::with_data_plane`] hands out implements too.
//!
//! Each accessor returns a cheap handle over the holder's Data Plane HTTP
//! client, so the same call reads the same on either: `client.issues()` acts
//! on the client's credential, `runner.issues()` on the runner's session
//! token. Both types also carry these as inherent methods, so calling them
//! needs no import; the trait is what keeps the two sets from drifting, and
//! what generic code takes (`fn f(dp: &impl DataPlaneResources)`).
//!
//! Which routes a call may reach is decided by the credential's scopes, not by
//! the type. A runner a member opens for themself carries `automations:read` /
//! `automations:write`, `connections:read` / `connections:write` /
//! `connections:delete` and `issues:read` / `issues:write` on top of the
//! sandbox set; a runner opened for an asserted end customer carries only the
//! sandbox set, or what its `RunRequest::scope` asks for.

use std::sync::Arc;

use crate::api::files::Files;
use crate::api::http::HttpClient;
use crate::api::shares::Shares;
use crate::api::tasks::Tasks;
use crate::api::telemetry::{Conversations, Events, Metrics};
use crate::client::IntrospectionClient;
use crate::resources::{Automations, Issues, MemberConnections};
use crate::runner::Runner;

#[cfg(doc)]
use crate::auth::EmailCodeAuth;

/// The Data Plane namespaces shared by [`IntrospectionClient`] and
/// [`Runner`]. See the [module docs](self).
pub trait DataPlaneResources {
    /// `/v1/tasks`, with each task's runs under `.runs`.
    fn tasks(&self) -> Tasks;
    /// `/v1/files`.
    fn files(&self) -> Files;
    /// `/v1/shares` — read grants for files and conversations.
    fn shares(&self) -> Shares;
    /// `GET /v1/conversations`.
    fn conversations(&self) -> Conversations;
    /// `GET /v1/events`.
    fn events(&self) -> Events;
    /// `POST /v1/metrics`.
    fn metrics(&self) -> Metrics;
    /// `/v1/automations`.
    fn automations(&self) -> Automations;
    /// `/v1/issues`.
    fn issues(&self) -> Issues;
    /// `/v1/connections` — the apps members connected for themselves. On a
    /// runner, `create` defaults `runtime` to the runner's runtime group.
    fn connections(&self) -> MemberConnections;
}

macro_rules! delegate_data_plane {
    ($ty:ty) => {
        impl DataPlaneResources for $ty {
            fn tasks(&self) -> Tasks {
                <$ty>::tasks(self)
            }
            fn files(&self) -> Files {
                <$ty>::files(self)
            }
            fn shares(&self) -> Shares {
                <$ty>::shares(self)
            }
            fn conversations(&self) -> Conversations {
                <$ty>::conversations(self)
            }
            fn events(&self) -> Events {
                <$ty>::events(self)
            }
            fn metrics(&self) -> Metrics {
                <$ty>::metrics(self)
            }
            fn automations(&self) -> Automations {
                <$ty>::automations(self)
            }
            fn issues(&self) -> Issues {
                <$ty>::issues(self)
            }
            fn connections(&self) -> MemberConnections {
                <$ty>::connections(self)
            }
        }
    };
}

delegate_data_plane!(IntrospectionClient);
delegate_data_plane!(Runner);

/// A Data Plane HTTP client, such as the one [`EmailCodeAuth::data_plane`] and
/// [`EmailCodeAuth::with_data_plane`] hand out, serves every namespace on its
/// own credential. It has no runtime context, so `connections().create` needs
/// an explicit `runtime`.
impl DataPlaneResources for Arc<HttpClient> {
    fn tasks(&self) -> Tasks {
        Tasks::new(self.clone())
    }
    fn files(&self) -> Files {
        Files::new(self.clone())
    }
    fn shares(&self) -> Shares {
        Shares::new(self.clone())
    }
    fn conversations(&self) -> Conversations {
        Conversations::new(self.clone())
    }
    fn events(&self) -> Events {
        Events::new(self.clone())
    }
    fn metrics(&self) -> Metrics {
        Metrics::new(self.clone())
    }
    fn automations(&self) -> Automations {
        Automations::new(self.clone())
    }
    fn issues(&self) -> Issues {
        Issues::new(self.clone())
    }
    fn connections(&self) -> MemberConnections {
        MemberConnections::new(self.clone(), None)
    }
}
