//! draft-atwood-jmap-chat-federation-00 §Blob Fetch and SSRF — which
//! addresses a server may connect to when it fetches from a peer-supplied
//! URL.

use std::net::{IpAddr, Ipv4Addr};

/// Whether a server may connect to `ip` to fetch from a peer-supplied URL.
///
/// Spec: draft-atwood-jmap-chat-federation-00 §Blob Fetch and SSRF:
/// servers "SHOULD NOT connect to link-local, loopback, or private-network
/// addresses in response to a peer-supplied URL", and "The same
/// restriction applies to any other context in which a server fetches from
/// a URL supplied by a peer, including blob URIs carried in
/// `Message.actions` or `ChatContact.endpoints`."
///
/// Returns `false` for:
///
/// - loopback: `127.0.0.0/8`, `::1`;
/// - the unspecified address `0.0.0.0` or `::`, which a connect reaches as
///   the local host;
/// - link-local: `169.254.0.0/16`, `fe80::/10`;
/// - private networks: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
///   (RFC 1918) and unique local `fc00::/7` (RFC 4193);
/// - an IPv4-mapped IPv6 address `::ffff:a.b.c.d` whose IPv4 address is
///   one of the above.
///
/// Every other address returns `true`. Call it on each address actually
/// connected to, after name resolution and on every redirect hop; a name
/// or URL alone says nothing about where it leads. It fits
/// `jmap_peer_auth_io::HttpsFetcher::with_policy` as the policy.
///
/// The draft's other restriction, staying within "the peer's known IP
/// range or hostname" on redirects, needs the deployment's view of the
/// peer and is not checked here.
///
/// # Example
///
/// ```
/// use jmap_peer_auth::fetch_address_allowed;
///
/// assert!(fetch_address_allowed("192.0.2.10".parse().unwrap()));
/// assert!(!fetch_address_allowed("127.0.0.1".parse().unwrap()));
/// assert!(!fetch_address_allowed("::ffff:10.0.0.1".parse().unwrap()));
/// ```
// ponytail: the three classes the draft names, plus the unspecified
// address. Shared address space (100.64.0.0/10), NAT64 prefixes and other
// special-purpose ranges pass; a deployment that must refuse them wraps
// this predicate with its own ranges.
pub fn fetch_address_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_allowed(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4_allowed(v4),
            None => {
                !(v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_unicast_link_local()
                    || v6.is_unique_local())
            }
        },
    }
}

fn v4_allowed(ip: Ipv4Addr) -> bool {
    !(ip.is_loopback() || ip.is_unspecified() || ip.is_link_local() || ip.is_private())
}
