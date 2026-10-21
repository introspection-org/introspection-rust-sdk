//! Metadata documents, published at `{issuer}/.well-known/{dwk}`.

use serde::{Deserialize, Serialize};

/// The `dwk` ("document well-known") names of each role's metadata document.
pub mod dwk {
    /// An agent provider's.
    pub const AGENT: &str = "aauth-agent.json";
    /// A Person Server's.
    pub const PERSON: &str = "aauth-person.json";
    /// A resource's.
    pub const RESOURCE: &str = "aauth-resource.json";
    /// An Access Server's.
    pub const ACCESS: &str = "aauth-access.json";

    /// Whether `name` is one of the four.
    pub fn is_known(name: &str) -> bool {
        [AGENT, PERSON, RESOURCE, ACCESS].contains(&name)
    }
}

/// The URL a metadata document is published at.
///
/// ```
/// # use aauth_protocol::metadata::{dwk, metadata_url};
/// assert_eq!(
///     metadata_url("https://ps.example/", dwk::PERSON),
///     "https://ps.example/.well-known/aauth-person.json"
/// );
/// ```
pub fn metadata_url(issuer: &str, dwk: &str) -> String {
    format!("{}/.well-known/{dwk}", crate::identifier::trim(issuer))
}

/// A Person Server's `aauth-person.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonServerMetadata {
    /// The Person Server's server identifier.
    pub issuer: String,
    /// Where an agent exchanges a resource token for an auth token.
    pub auth_token_endpoint: String,
    /// Where an agent asks for a person token. Only for resources that ask
    /// for one; a three-party Person Server need not publish it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person_token_endpoint: Option<String>,
    /// Where an agent proposes, updates and completes missions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_endpoint: Option<String>,
    /// The keys its tokens are signed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A display description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// An Access Server's `aauth-access.json` (AAuth §Access Server Metadata,
/// `#access-server-metadata`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessServerMetadata {
    /// The Access Server's server identifier: the `iss` of its auth tokens.
    pub issuer: String,
    /// The keys its auth tokens are signed with.
    pub jwks_uri: String,
    /// Where a Person Server sends its token requests (`#as-token-endpoint`).
    pub auth_token_endpoint: String,
    /// Where a Person Server or resource revokes a token it gave this
    /// Access Server. RECOMMENDED by the draft; not served by this crate yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<String>,
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A display description, in Markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The part of any role's metadata document that key discovery reads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyedMetadata {
    /// The issuer the document describes. When present it must be the one
    /// the document was fetched under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    /// Where the issuer's keys are published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,
    /// The issuer's keys, inline instead of at `jwks_uri`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks: Option<crate::jwk::Jwks>,
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
