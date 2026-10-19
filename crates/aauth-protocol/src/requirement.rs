//! The `AAuth-Requirement` response header (AAuth §Requirement Responses):
//! what a resource or server needs before it will answer.

use crate::sf::{self, Member};

/// The header's name, lowercase as the `http` crate wants it.
pub const HEADER: &str = "aauth-requirement";

/// What a `401` (or a Person Server's `202`) asks for.
///
/// ```
/// # use aauth_protocol::Requirement;
/// let header = r#"requirement=auth-token;resource-token="eyJ.eyJ.sig""#;
/// assert_eq!(
///     Requirement::parse(header),
///     Some(Requirement::AuthToken { resource_token: "eyJ.eyJ.sig".into() })
/// );
/// assert_eq!(Requirement::Approval.to_header(), "requirement=approval");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Requirement {
    /// Present an agent token.
    AgentToken,
    /// Present a person token from the agent's Person Server.
    PersonToken,
    /// Exchange this resource token at the Person Server for an auth token.
    AuthToken {
        /// The resource token to exchange.
        resource_token: String,
    },
    /// A human is deciding; poll the response's `Location`.
    Approval,
    /// Send the person to `url` with `code`.
    Interaction {
        /// Where the person goes, when the server names one.
        url: Option<String>,
        /// The code the person enters or carries there.
        code: String,
    },
    /// The server needs the agent to clarify its request.
    Clarification,
    /// The server needs claims about the person.
    Claims,
}

impl Requirement {
    /// Parse a header value; `None` for one that is malformed, unknown, or
    /// missing the parameter its requirement needs.
    pub fn parse(raw: &str) -> Option<Self> {
        let dict = sf::parse_dictionary(raw.trim()).ok()?;
        let (_, Member::Item(item)) = dict.iter().find(|(k, _)| k == "requirement")? else {
            return None;
        };
        let param = |name: &str| {
            item.params
                .iter()
                .find(|(k, _)| k == name)
                .and_then(|(_, v)| v.as_text())
                .map(str::to_string)
        };
        Some(match item.bare.as_text()? {
            "agent-token" => Self::AgentToken,
            "person-token" => Self::PersonToken,
            "auth-token" => Self::AuthToken {
                resource_token: param("resource-token")?,
            },
            "approval" => Self::Approval,
            "interaction" => Self::Interaction {
                url: param("url"),
                code: param("code")?,
            },
            "clarification" => Self::Clarification,
            "claims" => Self::Claims,
            _ => return None,
        })
    }

    /// The requirement's registered value, such as `auth-token`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::AgentToken => "agent-token",
            Self::PersonToken => "person-token",
            Self::AuthToken { .. } => "auth-token",
            Self::Approval => "approval",
            Self::Interaction { .. } => "interaction",
            Self::Clarification => "clarification",
            Self::Claims => "claims",
        }
    }

    /// The header value.
    pub fn to_header(&self) -> String {
        let mut out = format!("requirement={}", self.name());
        match self {
            Self::AuthToken { resource_token } => {
                out.push_str(&format!(";resource-token={}", sf::quote(resource_token)));
            }
            Self::Interaction { url, code } => {
                if let Some(url) = url {
                    out.push_str(&format!(";url={}", sf::quote(url)));
                }
                out.push_str(&format!(";code={}", sf::quote(code)));
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements_parse_from_the_header() {
        assert_eq!(
            Requirement::parse(r#"requirement=auth-token; resource-token="a.b.c""#),
            Some(Requirement::AuthToken {
                resource_token: "a.b.c".into()
            })
        );
        assert_eq!(
            Requirement::parse("requirement=person-token"),
            Some(Requirement::PersonToken)
        );
        assert_eq!(Requirement::parse("requirement=auth-token"), None);
        assert_eq!(Requirement::parse("requirement=dance"), None);
        assert_eq!(Requirement::parse("garbage(("), None);
    }

    #[test]
    fn a_built_header_parses_back() {
        for requirement in [
            Requirement::AuthToken {
                resource_token: "a.b.c".into(),
            },
            Requirement::Interaction {
                url: Some("https://r.example/i".into()),
                code: "ABCD-1234".into(),
            },
            Requirement::Approval,
        ] {
            assert_eq!(
                Requirement::parse(&requirement.to_header()),
                Some(requirement)
            );
        }
    }
}
