//! Identity binding (draft-atwood-jmap-chat-federation-00 §Identity
//! Binding).

use jmap_did_types::Did;
use url::Url;

use crate::PeerDomain;

/// The outcome of binding a peer identity to an authenticated peer
/// domain.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityBinding {
    /// The identity's host equals the peer domain: accept.
    Bound,
    /// The identity has a host and it is not the peer domain: reject.
    HostMismatch,
    /// A DID without a host (for example `did:key` or `did:plc`). The
    /// peer domain does not vouch for it: accept only after the
    /// DID-bound verification of draft-atwood-jmap-chat-did-00
    /// succeeds.
    NeedsDidVerification,
    /// An identity without a host that is not a DID: reject.
    NoHost,
}

/// Binds an identity to an authenticated peer domain.
///
/// Pass each caller identity of a Peer/* call (`senderUserId`,
/// `readerUserId`, `subscriberId` or `identity`): "A peer domain speaks
/// for every identity it hosts", and an identity bound this way is a peer
/// identity of the request (§Identity Binding). The peer's `ownerUserId`
/// plays no part in that binding. It is advisory owner metadata (§Session
/// Object Extensions for Federation): a peer applies it to the ChatContact
/// whose `identity` equals it only when it binds, through this function,
/// to the peer domain the Session was fetched from.
///
/// The host of an identity is, per §Identity Binding rule 1: for a URI
/// with an authority component, its host; for a `did:web` DID, the domain
/// name it encodes; for a string of the form `local@domain` without a URI
/// scheme, the part after the last `@`; for a bare DNS name, the name
/// itself; any other string without a scheme or `@` has no host. Hosts
/// are compared as peer domains are ([`PeerDomain`]). A
/// host that is not a domain name (an IP literal, a host with a port)
/// never equals a peer domain, so it yields
/// [`IdentityBinding::HostMismatch`].
pub fn bind_identity(identity: &str, peer_domain: &PeerDomain) -> IdentityBinding {
    match identity_host(identity) {
        IdentityHost::Domain(host) if &host == peer_domain => IdentityBinding::Bound,
        IdentityHost::Domain(_) | IdentityHost::NotDomain => IdentityBinding::HostMismatch,
        IdentityHost::HostlessDid => IdentityBinding::NeedsDidVerification,
        IdentityHost::NoHost => IdentityBinding::NoHost,
    }
}

/// The host of an identity (§Identity Binding rule 1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityHost {
    /// The host is a domain name, in the form peer domains are compared
    /// in.
    Domain(PeerDomain),
    /// The host is not a domain name (an IP literal, a host with a port),
    /// so it is no peer domain.
    NotDomain,
    /// A DID without a host (for example `did:key` or `did:plc`).
    HostlessDid,
    /// No host, and not a DID.
    NoHost,
}

/// The host of `identity`, by the rules [`bind_identity`] lists.
///
/// A sending server uses it to find the peer domain of a `Peer/*` call:
/// "The sending server sends the call to the peer domain that is the
/// host of `recipientUserId`, as {{identity-binding}} defines the host of
/// an identity" (§Server-to-Server Methods).
pub fn identity_host(identity: &str) -> IdentityHost {
    let host = match uri_scheme(identity) {
        Some("did") => match identity.parse::<Did>() {
            Ok(did) if did.method() == "web" => did.web_domain().map(str::to_owned),
            Ok(_) => return IdentityHost::HostlessDid,
            Err(_) => return IdentityHost::NoHost,
        },
        Some(_) => match Url::parse(identity)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
        {
            Some(host) => Some(host),
            None => return IdentityHost::NoHost,
        },
        None => match identity.rsplit_once('@') {
            Some((_, domain)) => Some(domain.to_owned()),
            None if PeerDomain::parse(identity).is_ok() => Some(identity.to_owned()),
            None => return IdentityHost::NoHost,
        },
    };
    host.and_then(|h| PeerDomain::parse(&h).ok())
        .map_or(IdentityHost::NotDomain, IdentityHost::Domain)
}

/// The scheme of `s` when it starts like a URI: RFC 3986 §3.1
/// `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` then `:`.
fn uri_scheme(s: &str) -> Option<&str> {
    let (scheme, _) = s.split_once(':')?;
    let mut bytes = scheme.bytes();
    let first = bytes.next()?;
    (first.is_ascii_alphabetic()
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')))
    .then_some(scheme)
}
