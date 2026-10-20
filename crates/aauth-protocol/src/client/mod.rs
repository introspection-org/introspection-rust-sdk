//! The agent's HTTP side: a client for its Person Server, and the guard a
//! client resolves through when the URLs it calls come from someone else.

pub mod connect;
pub mod guard;
pub mod person_server;

pub use connect::ConnectTo;
pub use guard::{guarded_client, DeniedNetworks, PublicResolver};
pub use person_server::{Agent, Answer, Person, PersonServerClient, Reach};
