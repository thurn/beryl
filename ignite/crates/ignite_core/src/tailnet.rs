//! Names the fault where a `.ts.net` hub host resolves outside the tailnet.
//!
//! When the machine's Tailscale DNS resolver stops answering, a `.ts.net` name resolves through
//! public DNS to Tailscale Funnel's relay. Funnel serves only port 443, so a connection to the
//! hub's tailnet-only port is closed with no certificate, and the client reports only an
//! unexpected end of file. This module turns that signature into a message that says what
//! happened and how to fix it.

use std::net::{IpAddr, ToSocketAddrs};

/// How a hub request failed, as far as the classification cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// The connection or the TLS handshake failed, or the stream ended early.
    ConnectOrTls,
    /// The hub answered; the failure is not a transport one.
    Other,
}

/// The command that restarts Tailscale's DNS handling.
const FIX: &str = "`tailscale set --accept-dns=false && tailscale set --accept-dns=true`";

/// The host part of an `http(s)` URL, without credentials or port.
#[must_use]
pub fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = match authority.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next()?,
        None => authority.split(':').next()?,
    };
    (!host.is_empty()).then_some(host)
}

/// Whether the address is in the tailnet range 100.64.0.0/10 (or a Tailscale IPv6 address).
#[must_use]
pub fn in_tailnet(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            let [a, b, _, _] = v4.octets();
            a == 100 && (64..128).contains(&b)
        }
        // Tailscale's IPv6 range is fd7a:115c:a1e0::/48.
        IpAddr::V6(v6) => v6.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// The message for a `.ts.net` host that resolved only outside the tailnet, if that is what
/// happened.
#[must_use]
pub fn classify(host: &str, resolved: &[IpAddr], kind: FailureKind) -> Option<String> {
    if kind != FailureKind::ConnectOrTls || !host.to_ascii_lowercase().ends_with(".ts.net") {
        return None;
    }
    let outside = resolved.iter().find(|a| !in_tailnet(**a))?;
    if resolved.iter().any(|a| in_tailnet(*a)) {
        return None;
    }
    Some(format!(
        "{host} resolves to {outside}, outside the tailnet: this machine's Tailscale DNS is \
         not answering. To fix: {FIX}."
    ))
}

/// Resolves the hub URL's host with the system resolver and classifies the failure.
#[must_use]
pub fn diagnose_url(url: &str, kind: FailureKind) -> Option<String> {
    let host = host_of(url)?;
    let resolved: Vec<IpAddr> = (host, 443)
        .to_socket_addrs()
        .ok()?
        .map(|a| a.ip())
        .collect();
    classify(host, &resolved, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const HOST: &str = "jarvis.tail4b0774.ts.net";

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn funnel_address_names_the_dns_fault() {
        let message = classify(HOST, &[v4(208, 111, 1, 2)], FailureKind::ConnectOrTls).unwrap();
        assert_eq!(
            message,
            "jarvis.tail4b0774.ts.net resolves to 208.111.1.2, outside the tailnet: this \
             machine's Tailscale DNS is not answering. To fix: `tailscale set \
             --accept-dns=false && tailscale set --accept-dns=true`."
        );
    }

    #[test]
    fn tailnet_address_is_not_the_fault() {
        assert_eq!(
            classify(HOST, &[v4(100, 101, 2, 3)], FailureKind::ConnectOrTls),
            None
        );
        assert!(classify(HOST, &[v4(100, 128, 0, 1)], FailureKind::ConnectOrTls).is_some());
    }

    #[test]
    fn other_hosts_and_failures_are_not_classified() {
        assert_eq!(
            classify("example.com", &[v4(8, 8, 8, 8)], FailureKind::ConnectOrTls),
            None
        );
        assert_eq!(classify(HOST, &[v4(8, 8, 8, 8)], FailureKind::Other), None);
        assert_eq!(classify(HOST, &[], FailureKind::ConnectOrTls), None);
    }

    #[test]
    fn host_is_taken_from_the_url() {
        assert_eq!(
            host_of("https://jarvis.tail4b0774.ts.net:8443/x"),
            Some(HOST)
        );
        assert_eq!(host_of("http://user@[::1]:80/"), Some("::1"));
        assert_eq!(host_of("https:///"), None);
    }
}
