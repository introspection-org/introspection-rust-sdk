//! What an agent may dial on someone else's say-so. A Person Server URL is
//! often chosen by a tenant, and the agent calls it from its own network
//! position, so it must never reach that network itself (SSRF).
//!
//! Two halves, because reqwest dials an IP-literal URL without resolving it:
//! [`admit_host`] refuses literals and `localhost` before a request is built,
//! and [`PublicResolver`] is the client's resolver, so the addresses it checked
//! are the addresses the socket connects to.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use ipnet::IpNet;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::Url;

use crate::error::{Error, Result};

/// Ranges no tenant URL may resolve into, beyond the ones [`is_public`] knows:
/// a plane whose pod or service CIDR is not RFC 1918 lists it here.
#[derive(Clone, Debug, Default)]
pub struct DeniedNetworks(Vec<IpNet>);

impl DeniedNetworks {
    /// `cidr,cidr,...`, e.g. `240.10.0.0/16,2600:1900:4000::/44`.
    pub fn parse(raw: &str) -> Result<Self> {
        raw.split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                entry
                    .parse::<IpNet>()
                    .map_err(|e| Error::Invalid(format!("denied network {entry:?}: {e}")))
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    fn allows(&self, ip: IpAddr) -> bool {
        is_public(ip) && !self.0.iter().any(|net| net.contains(&ip))
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        // CGNAT, 100.64.0.0/10.
        || (a == 100 && (b & 0xc0) == 64)
        // IETF protocol assignments, 192.0.0.0/24.
        || (a == 192 && b == 0 && c == 0)
        // Benchmarking, 198.18.0.0/15.
        || (a == 198 && (b & 0xfe) == 18)
        // Reserved, 240.0.0.0/4.
        || a >= 240)
}

/// Whether `ip` is a public unicast address.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = embedded_v4(v6) {
                return is_public_v4(v4);
            }
            let first = v6.segments()[0];
            // 6to4, 2002::/16, carries an IPv4 address in its next 32 bits.
            if first == 0x2002 {
                let s = v6.segments();
                return is_public_v4(Ipv4Addr::new(
                    (s[1] >> 8) as u8,
                    s[1] as u8,
                    (s[2] >> 8) as u8,
                    s[2] as u8,
                ));
            }
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                // Unique local, fc00::/7.
                || (first & 0xfe00) == 0xfc00
                // Link-local, fe80::/10.
                || (first & 0xffc0) == 0xfe80
                // Documentation, 2001:db8::/32.
                || (first == 0x2001 && v6.segments()[1] == 0x0db8)
                // Local-use NAT64, 64:ff9b:1::/48.
                || (first == 0x64 && v6.segments()[1] == 0xff9b && v6.segments()[2] == 1)
                // Discard-only, 100::/64.
                || (first == 0x0100 && v6.segments()[1..4] == [0, 0, 0]))
        }
    }
}

/// The IPv4 address an IPv4-mapped (`::ffff:a.b.c.d`) or NAT64
/// (`64:ff9b::a.b.c.d`) address reaches.
fn embedded_v4(v6: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = v6.to_ipv4_mapped() {
        return Some(v4);
    }
    let s = v6.segments();
    (s[..6] == [0x64, 0xff9b, 0, 0, 0, 0])
        .then(|| Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8))
}

/// Refuse a URL whose host is an IP literal or `localhost`: the resolver never
/// sees those.
pub fn admit_host(url: &str) -> Result<()> {
    let parsed = Url::parse(url).map_err(|e| Error::Url(format!("{url}: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::Url(format!("refusing to call {url}: no host")))?;
    if host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok() {
        return Err(Error::Url(format!("refusing to call {url}: an IP address")));
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(Error::Url(format!("refusing to call {url}: localhost")));
    }
    Ok(())
}

/// Resolves through the system resolver and fails the lookup when any answer
/// is not public, so a name cannot resolve one way when checked and another
/// when dialled.
pub struct PublicResolver {
    denied: DeniedNetworks,
}

impl PublicResolver {
    /// A resolver refusing non-public addresses and anything in `denied`.
    pub fn new(denied: DeniedNetworks) -> Self {
        Self { denied }
    }
}

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let denied = self.denied.clone();
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if addrs.is_empty() {
                return Err(format!("{host} resolved to nothing").into());
            }
            if let Some(refused) = addrs.iter().find(|addr| !denied.allows(addr.ip())) {
                return Err(
                    format!("{host} resolves to {}, not a public address", refused.ip()).into(),
                );
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// An HTTP client for URLs someone else chose: a 10-second timeout, no
/// redirects, no proxy (which would resolve names past this resolver), and
/// a resolver that refuses any name answering with a non-public address or
/// one in `denied`.
///
/// reqwest dials an IP-literal URL without resolving it, so pair this with
/// [`admit_host`] on every URL, as [`crate::discovery::ReqwestFetch::guarded`]
/// and [`super::PersonServerClient`] do.
pub fn guarded_client(denied: DeniedNetworks) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .dns_resolver(Arc::new(PublicResolver::new(denied)))
        .build()
        .map_err(|e| Error::Http(format!("could not build the guarded client: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(ip: &str) -> bool {
        is_public(ip.parse().unwrap())
    }

    #[test]
    fn private_loopback_link_local_and_cgnat_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "240.0.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::a9fe:a9fe",
            "2002:a9fe:a9fe::1",
            "64:ff9b:1::a9fe:a9fe",
        ] {
            assert!(!public(ip), "{ip} should not be public");
        }
        for ip in ["8.8.8.8", "100.128.0.1", "2001:4860:4860::8888"] {
            assert!(public(ip), "{ip} should be public");
        }
    }

    #[test]
    fn a_listed_network_is_refused_too() {
        let denied = DeniedNetworks::parse("34.118.224.0/20, 2600:1900::/28").unwrap();
        assert!(!denied.allows("34.118.230.1".parse().unwrap()));
        assert!(!denied.allows("2600:1900::1".parse().unwrap()));
        assert!(denied.allows("34.120.0.1".parse().unwrap()));
        assert!(DeniedNetworks::parse("10.0.0.0").is_err());
    }

    #[test]
    fn ip_literals_and_localhost_are_refused_before_resolving() {
        for url in [
            "https://127.0.0.1/token",
            "https://10.0.0.5/",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]:8443/",
            "https://localhost/",
            "https://ps.localhost/",
        ] {
            assert!(admit_host(url).is_err(), "{url} should be refused");
        }
        assert!(admit_host("https://ps.example.com/token").is_ok());
    }

    #[tokio::test]
    async fn a_name_resolving_private_is_refused() {
        let resolver = PublicResolver::new(DeniedNetworks::default());
        let refused = resolver.resolve("localhost".parse().unwrap()).await;
        assert!(refused.is_err());
    }
}
