//! Identity binding and peer domains
//! (draft-atwood-jmap-chat-federation-00 §Identity Binding, §Terminology).

use jmap_peer_auth::{IdentityBinding, IdentityHost, PeerDomain, bind_identity, identity_host};

fn domain(name: &str) -> PeerDomain {
    PeerDomain::parse(name).unwrap()
}

/// §Terminology: "A peer domain is compared in lowercase A-label form,
/// without a trailing dot or port."
#[test]
fn peer_domain_compared_form() {
    assert_eq!(domain("Alice.Example.").as_str(), "alice.example");
    // The IDNA2008 A-label of "bücher" (RFC 3492 Punycode).
    assert_eq!(domain("Bücher.example").as_str(), "xn--bcher-kva.example");
    for bad in [
        "",
        ".",
        "alice.example:443",
        "alice example",
        "alice..example",
        "-alice.example",
        "alice.example/x",
        // §Terminology: a peer domain is "The DNS domain name that
        // identifies a mailbox server", not an address. WHATWG URL host
        // parsing reads a host whose last label is a number as IPv4.
        "192.0.2.1",
        "192.0.2.1.",
        "127.1",
        "2130706433",
        "0x7f.1",
        "alice.0x",
        "alice.123",
    ] {
        assert!(PeerDomain::parse(bad).is_err(), "{bad:?}");
    }
}

/// Rule 2: "If the peer identity has a host, that host MUST equal the
/// authenticated peer domain. The host of an identity is: for a URI with
/// an authority component, its host; for a `did:web` DID, the domain name
/// it encodes; for a string of the form `local@domain` without a URI
/// scheme, the part after the last `@`; for a bare DNS name, the name
/// itself. Hosts are compared as peer domains are."
#[test]
fn identity_with_host_binds_to_that_host() {
    let alice = domain("alice.example");
    for identity in [
        "alice@alice.example",
        "Alice@ALICE.example.",
        "a@b@alice.example",
        "alice.example",
        "https://alice.example/users/alice",
        "https://user@Alice.Example:8443/",
        "xmpp://alice.example/alice",
        "did:web:alice.example",
        "did:web:alice.example%3A8443:users:alice",
    ] {
        assert_eq!(
            bind_identity(identity, &alice),
            IdentityBinding::Bound,
            "{identity}"
        );
    }
    // The draft's opaque relay example: "provided its host is the sending
    // server's peer domain (for example, `k3x9q7@relay.example`)".
    assert_eq!(
        bind_identity("k3x9q7@relay.example", &domain("relay.example")),
        IdentityBinding::Bound
    );
    assert_eq!(
        bind_identity("bob@bücher.example", &domain("xn--bcher-kva.example")),
        IdentityBinding::Bound
    );
}

/// "A peer domain cannot claim an identity whose host is another domain."
#[test]
fn identity_with_other_host_is_rejected() {
    let alice = domain("alice.example");
    for identity in [
        "alice@bob.example",
        "alice@alice.example.evil.example",
        "bob.example",
        "https://bob.example/alice.example",
        "https://alice.example@bob.example/",
        "did:web:bob.example",
        "alice@alice.example:8443",
        "https://[2001:db8::1]/",
        "https://192.0.2.1/",
        "https://127.1/",
        "alice@192.0.2.1",
        "k3x9q7",
    ] {
        assert_eq!(
            bind_identity(identity, &alice),
            IdentityBinding::HostMismatch,
            "{identity}"
        );
    }
}

/// Rule 3: "If the peer identity has no host (for example, a `did:key`
/// DID), the peer domain does not vouch for it. ... A DID-form identity
/// is accepted only after the DID-bound verification of {{JMAP-CHAT-DID}}
/// succeeds; any other host-less identity MUST be rejected."
#[test]
fn host_less_identities() {
    let alice = domain("alice.example");
    for did in [
        "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
        "did:plc:ewvi7nxzyoun6zhxrhs64oiz",
    ] {
        assert_eq!(
            bind_identity(did, &alice),
            IdentityBinding::NeedsDidVerification,
            "{did}"
        );
    }
    for identity in [
        "mailto:alice@alice.example",
        "urn:uuid:6e8bc430-9c3a-11d9-9669-0800200c9a66",
        "did:",
        // Not a bare DNS name (§Terminology), so it has no host.
        "192.0.2.1",
        "",
    ] {
        assert_eq!(
            bind_identity(identity, &alice),
            IdentityBinding::NoHost,
            "{identity:?}"
        );
    }
}

/// jmap-chat-compliance-tests.md 7.16 (jmap-chat-spec c08568e): from
/// peer `a.example`, "`senderUserId` `alice@A.Example` and then
/// `carol@a.example`, two users of one peer domain. Verify both succeed.
/// Send one with `senderUserId: "bob@b.example"`. Verify `forbidden`".
/// fed c08568e L335: "Hosts are compared as peer domains are."
#[test]
fn compliance_7_16_users_of_one_peer_domain() {
    let a = domain("a.example");
    assert_eq!(bind_identity("alice@A.Example", &a), IdentityBinding::Bound);
    assert_eq!(bind_identity("carol@a.example", &a), IdentityBinding::Bound);
    assert_eq!(
        bind_identity("bob@b.example", &a),
        IdentityBinding::HostMismatch
    );
}

/// fed b1cd625 L335, rule 1: "The host of an identity is: for a URI with
/// an authority component, its host; for a `did:web` DID, the domain name
/// it encodes; for a string of the form `local@domain` without a URI
/// scheme, the part after the last `@`; for a bare DNS name, the name
/// itself. Hosts are compared as peer domains are." Rule 2: "If the
/// identity has no host (for example, a `did:key` DID)".
#[test]
fn identity_host_rule_1() {
    for (identity, host) in [
        ("https://Alice.Example/users/alice", "alice.example"),
        ("did:web:alice.example", "alice.example"),
        ("a@b@Alice.Example", "alice.example"),
        ("alice.example", "alice.example"),
    ] {
        assert_eq!(
            identity_host(identity),
            IdentityHost::Domain(domain(host)),
            "{identity}"
        );
    }
    assert_eq!(
        identity_host("did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"),
        IdentityHost::HostlessDid
    );
    assert_eq!(
        identity_host("alice"),
        IdentityHost::Domain(domain("alice"))
    );
    assert_eq!(identity_host("not a host"), IdentityHost::NoHost);
    assert_eq!(identity_host("bob@192.0.2.1"), IdentityHost::NotDomain);
}
