//! `ChatContact.identity` and local `ChatContact.id` (bd:JMAP-405z.1).
//!
//! Oracle: draft-atwood-jmap-chat-00 at jmap-chat-spec cefc217, which made
//! `id` a server-assigned local Id and moved the authentication-layer
//! identity into the new `identity` property.

use jmap_chat_types::ChatContact;
use serde_json::json;

/// §ChatContact: "`identity` (String, immutable): The identity string the
/// authentication layer provides for this user, for example a
/// `user@host`-style string, a Decentralized Identifier URI, or another
/// URI." It is a typed property of ChatContact, not a vendor extension, and
/// its value round-trips verbatim beside a local Id.
#[test]
fn identity_is_a_typed_property() {
    let wire = json!({
        "id": "c1",
        "identity": "did:web:alice.example",
        "login": "alice@alice.example",
        "firstSeenAt": "2026-01-01T00:00:00Z",
        "lastSeenAt": "2026-04-01T00:00:00Z",
        "blocked": false
    });
    let c: ChatContact = serde_json::from_value(wire.clone()).expect("deserialize ChatContact");
    assert!(
        !c.extra.contains_key("identity"),
        "identity must be a typed ChatContact property, not an extension field"
    );
    assert_eq!(c.id.as_ref(), "c1");
    assert_eq!(serde_json::to_value(&c).expect("serialize"), wire);
}

/// §ChatContact lists "`identity` (String, immutable)" without "optional",
/// and the Terminology section says a user's identity is "recorded in the
/// `identity` property of the user's ChatContact". A ChatContact without
/// `identity` is therefore not a valid ChatContact.
#[test]
fn identity_is_required() {
    let wire = json!({
        "id": "c1",
        "login": "alice@example.com",
        "firstSeenAt": "2026-01-01T00:00:00Z",
        "lastSeenAt": "2026-04-01T00:00:00Z",
        "blocked": false
    });
    assert!(
        serde_json::from_value::<ChatContact>(wire).is_err(),
        "ChatContact without identity must not deserialize"
    );
}
