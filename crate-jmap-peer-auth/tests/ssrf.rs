//! draft-atwood-jmap-chat-federation-00 §Blob Fetch and SSRF, L824-826:
//! "servers SHOULD NOT connect to link-local, loopback, or private-network
//! addresses in response to a peer-supplied URL. ... The same restriction
//! applies to any other context in which a server fetches from a URL
//! supplied by a peer, including blob URIs carried in `Message.actions` or
//! `ChatContact.endpoints`."
//!
//! The address ranges, including their first and last addresses, come
//! from the RFCs that assign them.

use std::net::IpAddr;

use jmap_peer_auth::fetch_address_allowed;

fn allowed(ip: &str) -> bool {
    fetch_address_allowed(ip.parse::<IpAddr>().expect("address fixture"))
}

fn assert_refused(ips: &[&str]) {
    for ip in ips {
        assert!(!allowed(ip), "{ip} must be refused");
    }
}

/// Loopback. RFC 1122 §3.2.1.3 (g): "{ 127, <any> } Internal host
/// loopback address." RFC 4291 §2.5.3: "The unicast address
/// 0:0:0:0:0:0:0:1 is called the loopback address."
#[test]
fn loopback_refused() {
    assert_refused(&["127.0.0.1", "127.0.0.0", "127.255.255.255", "::1"]);
}

/// Link-local. RFC 3927 §2.1: "The IPv4 prefix 169.254/16 is registered
/// with the IANA for this purpose." RFC 4291 §2.4: Link-Local unicast is
/// "1111111010" (FE80::/10).
#[test]
fn link_local_refused() {
    assert_refused(&[
        "169.254.0.0",
        "169.254.169.254",
        "169.254.255.255",
        "fe80::1",
        "febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
    ]);
}

/// Private networks. RFC 1918 §3: "10.0.0.0 - 10.255.255.255 (10/8
/// prefix) 172.16.0.0 - 172.31.255.255 (172.16/12 prefix) 192.168.0.0 -
/// 192.168.255.255 (192.168/16 prefix)". RFC 4193 §3.1: Local IPv6
/// unicast addresses have the prefix "FC00::/7".
#[test]
fn private_networks_refused() {
    assert_refused(&[
        "10.0.0.0",
        "10.255.255.255",
        "172.16.0.0",
        "172.31.255.255",
        "192.168.0.0",
        "192.168.255.255",
        "fc00::",
        "fd12:3456:789a::1",
        "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
    ]);
}

/// RFC 4291 §2.5.5.2: "::ffff:<IPv4-address>" is "used to represent the
/// addresses of IPv4 nodes as IPv6 addresses", so a mapped address reaches
/// the IPv4 host it names and gets that host's answer.
#[test]
fn ipv4_mapped_follows_ipv4() {
    assert_refused(&[
        "::ffff:127.0.0.1",
        "::ffff:169.254.169.254",
        "::ffff:10.0.0.1",
        "::ffff:192.168.1.1",
    ]);
    assert!(allowed("::ffff:192.0.2.1"));
}

/// RFC 1122 §3.2.1.3 (a): "{ 0, 0 } This host on this network." A connect
/// to the unspecified address reaches the local host.
#[test]
fn unspecified_refused() {
    assert_refused(&["0.0.0.0", "::"]);
}

/// Addresses just outside each refused range, and global unicast, are not
/// in any range the draft names. RFC 5737 §3 reserves 192.0.2.0/24,
/// 198.51.100.0/24 and 203.0.113.0/24 for documentation; RFC 3849 reserves
/// 2001:db8::/32. 128.0.0.0, 169.253.255.255, 169.255.0.0, 11.0.0.0,
/// 172.15.255.255, 172.32.0.0, 192.167.255.255, 192.169.0.0, fec0::
/// and fe00:: lie outside the ranges above.
#[test]
fn other_addresses_allowed() {
    for ip in [
        "192.0.2.1",
        "198.51.100.7",
        "203.0.113.200",
        "2001:db8::1",
        "128.0.0.0",
        "169.253.255.255",
        "169.255.0.0",
        "11.0.0.0",
        "172.15.255.255",
        "172.32.0.0",
        "192.167.255.255",
        "192.169.0.0",
        "fec0::1",
        "fe00::1",
        "::2",
    ] {
        assert!(allowed(ip), "{ip} must be allowed");
    }
}
