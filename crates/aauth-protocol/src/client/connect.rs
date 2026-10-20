//! Dev-only connect-to overrides (like curl's `--connect-to`): a request still names and signs for its origin's
//! authority, but the socket goes elsewhere. It lets a proxy in a container
//! reach servers that identify as `localhost` on the developer's machine.

use url::Url;

use crate::error::{Error, Result};
use crate::identifier::authority;

/// Authorities whose requests go to another origin. Empty by default: every
/// request goes where its URL says.
#[derive(Clone, Debug, Default)]
pub struct ConnectTo(Vec<(String, String)>);

/// Where to send a request, and the `Host` it must still carry.
#[derive(Debug, PartialEq, Eq)]
pub struct Route {
    /// The URL to send the request to.
    pub url: String,
    /// The `Host` header it must carry, when it goes somewhere other than its URL's authority.
    pub host: Option<String>,
}

impl ConnectTo {
    /// `from_authority=to_origin,...`, e.g. `ps.acme.localhost=http://host.docker.internal:3400`.
    pub fn parse(raw: &str) -> Result<Self> {
        raw.split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                entry
                    .split_once('=')
                    .map(|(from, to)| (from.trim().to_ascii_lowercase(), to.trim().to_string()))
                    .filter(|(from, to)| !from.is_empty() && !to.is_empty())
                    .ok_or_else(|| {
                        Error::Invalid(format!("connect-to entry {entry:?} is not from=to"))
                    })
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// Whether no authority is mapped.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Where a request to `url` goes.
    pub fn route(&self, url: &str) -> Result<Route> {
        let parsed = Url::parse(url).map_err(|e| Error::Url(format!("{url}: {e}")))?;
        let from = authority(&parsed).ok_or_else(|| Error::Url(format!("{url} has no host")))?;
        let Some((_, to)) = self.0.iter().find(|(candidate, _)| *candidate == from) else {
            return Ok(Route {
                url: url.to_string(),
                host: None,
            });
        };
        let mut rest = parsed.path().to_string();
        if let Some(query) = parsed.query() {
            rest = format!("{rest}?{query}");
        }
        Ok(Route {
            url: format!("{}{rest}", to.trim_end_matches('/')),
            host: Some(from),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mapped_authority_dials_elsewhere_and_keeps_its_host() {
        let connect = ConnectTo::parse(
            "ps.acme.localhost=http://host.docker.internal:3400, localhost:3400=http://host.docker.internal:3400/",
        )
        .unwrap();
        assert_eq!(
            connect
                .route("https://ps.acme.localhost/ps/token?x=1")
                .unwrap(),
            Route {
                url: "http://host.docker.internal:3400/ps/token?x=1".into(),
                host: Some("ps.acme.localhost".into()),
            }
        );
        assert_eq!(
            connect.route("https://ps.example/token").unwrap(),
            Route {
                url: "https://ps.example/token".into(),
                host: None,
            }
        );
    }

    #[test]
    fn a_malformed_entry_is_refused() {
        assert!(ConnectTo::parse("localhost:3400").is_err());
        assert!(ConnectTo::parse("").unwrap().is_empty());
    }
}
