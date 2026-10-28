//! Shim implementation of the `local_ip_address::list_afinet_netifas()` API.
//!
//! The upstream crate returns *every* IPv4/IPv6 address on the system, including
//! unbindable ones such as the autoconfiguration addresses (169.254.0.0/16) of
//! disconnected virtual adapters (e.g. Microsoft Wi-Fi Direct adapters).
//! Consumers of this crate — notably `ableton-link-rs` — pick the *first*
//! non-loopback address and bind a UDP socket to it, which panics with
//! `WSAEADDRNOTAVAIL` ("The requested address is not valid in its context")
//! on such adapters.
//!
//! This shim keeps the same public function, name and signature, but:
//!   1. drops loopback and IPv6/IPv4 link-local autoconfiguration addresses,
//!   2. verifies every remaining IPv4 address by actually binding an ephemeral
//!      UDP socket to it, so only addresses that work on *this* machine are
//!      ever reported,
//!   3. deduplicates by address and preserves interface enumeration order.
use std::{
    io,
    net::{IpAddr, SocketAddr, UdpSocket},
};

use if_addrs::{IfAddr, Ifv4Addr, Ifv6Addr};

pub fn list_afinet_netifas() -> io::Result<Vec<(String, IpAddr)>> {
    let mut result: Vec<(String, IpAddr)> = Vec::new();

    for entry in if_addrs::get_if_addrs()? {
        let (name, ip) = match entry.addr {
            IfAddr::V4(Ifv4Addr { ip, .. }) => (entry.name, IpAddr::V4(ip)),
            IfAddr::V6(Ifv6Addr { ip, .. }) => (entry.name, IpAddr::V6(ip)),
        };

        if !is_bindable(&ip) {
            continue;
        }

        // Enumerators can report the same address on several adapters; keep
        // only the first entry so consumers do not cycle between duplicates.
        if !result.iter().any(|(_, existing)| *existing == ip) {
            result.push((name, ip));
        }
    }

    Ok(result)
}

/// Filter out addresses that either are known to be unusable or fail an
/// actual UDP bind test.
fn is_bindable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            // Loopback is meaningful but consumers look for site-local
            // addresses; autoconfiguration addresses (link-local) belong to
            // disconnected/virtual adapters on Windows and cannot be bound.
            !v4.is_loopback()
                && !v4.is_link_local()
                && UdpSocket::bind(SocketAddr::new(IpAddr::V4(*v4), 0)).is_ok()
        }
        // IPv6 link-local addresses need a scope id to be useful here and
        // the direct target of this API is unicast IPv4.
        IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_at_least_one_usable_address_or_empty_without_error() {
        let list = list_afinet_netifas().unwrap();
        for (name, ip) in &list {
            assert!(is_bindable(ip), "{} reported unbindable {}", name, ip);
            assert!(!name.is_empty());
        }
    }
}
