//! Server identifiers (AAuth §Server Identifiers) and origin comparison.

use url::Url;

/// `https://host`: lowercase, no port, path, query, fragment or trailing slash,
/// and a host name rather than an IP address. Two identifiers compare by
/// exact string equality.
pub fn is_server_identifier(value: &str) -> bool {
    if value.is_empty() || value != value.to_lowercase() || value.ends_with('/') {
        return false;
    }
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.port().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.path(), "" | "/")
        && matches!(url.host(), Some(url::Host::Domain(_)))
        && url
            .host_str()
            .is_some_and(|host| value == format!("https://{host}"))
}

/// Whether two URLs share scheme, host and port.
pub fn same_origin(a: &str, b: &str) -> bool {
    match (Url::parse(a), Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}

/// A URL with its trailing slashes removed.
pub fn trim(url: &str) -> &str {
    url.trim_end_matches('/')
}

/// The `host[:port]` a request to `url` is addressed to, lowercased.
pub fn authority(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_identifiers_are_bare_https_origins() {
        assert!(is_server_identifier("https://ps.example"));
        for bad in [
            "https://ps.example/",
            "https://PS.example",
            "http://ps.example",
            "https://ps.example:8443",
            "https://ps.example/ps",
            "https://ps.example?x",
            "https://10.0.0.1",
            "https://[::1]",
            "",
        ] {
            assert!(!is_server_identifier(bad), "{bad}");
        }
    }

    #[test]
    fn origins_compare_scheme_host_and_port() {
        assert!(same_origin(
            "https://ps.example",
            "https://ps.example/token"
        ));
        assert!(!same_origin(
            "https://ps.example",
            "https://internal.example/token"
        ));
        assert!(!same_origin(
            "https://ps.example",
            "https://ps.example:8443/token"
        ));
        assert!(!same_origin(
            "https://ps.example",
            "http://ps.example/token"
        ));
    }
}
