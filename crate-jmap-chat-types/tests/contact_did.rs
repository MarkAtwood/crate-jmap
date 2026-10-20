//! `ChatContact.did` (bd:JMAP-8tuq.2).
//!
//! Oracle: draft-atwood-jmap-chat-did-00 (jmap-chat-spec 3227c0f)
//! §Optional `did` field, L160-163: "ChatContacts MAY carry an optional
//! `did` field to accommodate the pseudonymous case, in which a stable
//! `identity` that is not a DID is preserved" and "`did` (String,
//! optional): A DID URI ({{DID-CORE}}) bound to this ChatContact". The
//! draft has no ChatContact JSON example, so the JSON below is written by
//! hand from that definition and the draft-atwood-jmap-chat-00 §ChatContact
//! properties. `did:example:123456789abcdefghi` is the DID Core §3.1
//! example DID; DID Core §3.1 ABNF: `did = "did:" method-name ":"
//! method-specific-id`.

use jmap_chat_types::ChatContact;
use serde_json::json;

/// The pseudonymous case: a non-DID `identity` with a bound `did`. The
/// object round-trips unchanged and `did` is typed, not left in `extra`.
#[test]
fn contact_with_did_round_trips() {
    let wire = json!({
        "id": "ct1",
        "identity": "alice@example.com",
        "login": "alice@example.com",
        "firstSeenAt": "2026-01-01T00:00:00Z",
        "lastSeenAt": "2026-04-01T00:00:00Z",
        "blocked": false,
        "did": "did:example:123456789abcdefghi"
    });
    let c: ChatContact = serde_json::from_value(wire.clone()).expect("deserialize ChatContact");
    assert_eq!(
        c.did.as_ref().map(|d| d.as_str()),
        Some("did:example:123456789abcdefghi")
    );
    assert!(
        c.extra.is_empty(),
        "did must not land in extra: {:?}",
        c.extra
    );
    assert_eq!(serde_json::to_value(&c).expect("serialize"), wire);
}

/// "When absent, no DID is bound to this ChatContact" (L163): an absent
/// `did` reads as `None` and is not written back.
#[test]
fn contact_without_did_omits_it() {
    let wire = json!({
        "id": "ct1",
        "identity": "alice@example.com",
        "login": "alice@example.com",
        "firstSeenAt": "2026-01-01T00:00:00Z",
        "lastSeenAt": "2026-04-01T00:00:00Z",
        "blocked": false
    });
    let c: ChatContact = serde_json::from_value(wire.clone()).expect("deserialize ChatContact");
    assert!(c.did.is_none());
    assert_eq!(serde_json::to_value(&c).expect("serialize"), wire);
}

/// `did` is "A DID URI ({{DID-CORE}})" (L163). `alice@example.com` lacks
/// the ABNF's `"did:"` prefix, so it is not a valid `did` value.
#[test]
fn contact_with_non_did_value_is_rejected() {
    let wire = json!({
        "id": "ct1",
        "identity": "alice@example.com",
        "login": "alice@example.com",
        "firstSeenAt": "2026-01-01T00:00:00Z",
        "lastSeenAt": "2026-04-01T00:00:00Z",
        "blocked": false,
        "did": "alice@example.com"
    });
    assert!(serde_json::from_value::<ChatContact>(wire).is_err());
}
