//! Tests for the Session extensions of draft-atwood-jmap-chat-federation-00
//! §Session Object Extensions for Federation and the anonymous
//! `/.well-known/jmap` Session of §Key Publication.
//!
//! Oracles: the `peerSigningKeys` example quoted from the draft
//! (jmap-chat-spec draft-atwood-jmap-chat-federation-00.md L284-293; it is
//! the RFC 8037 App. A.1 public key with its App. A.3 thumbprint as `kid`),
//! the RFC 8037 App. A.1 private key member `d`, the RFC 8620 §2.1 example
//! Session values, and `jmap_base_client::Session` as an RFC 8620 parser
//! written independently of this crate.

use jmap_chat_federation_types::{
    anonymous_well_known_session, owner_endpoints, AnonymousSessionError, FederationSession,
};
use jmap_chat_types::ChatContact;
use serde_json::{json, Map, Value};

/// L284-293, verbatim.
fn draft_key() -> Value {
    json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
        "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
    })
}

/// An authenticated RFC 8620 §2.1 Session object (values from the RFC's
/// example), as the server would serve its owner.
fn owner_session() -> Map<String, Value> {
    let v = json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {
                "maxSizeUpload": 50000000,
                "maxConcurrentUpload": 8,
                "maxSizeRequest": 10000000,
                "maxConcurrentRequests": 8,
                "maxCallsInRequest": 32,
                "maxObjectsInGet": 256,
                "maxObjectsInSet": 128,
                "collationAlgorithms": ["i;ascii-numeric", "i;ascii-casemap", "i;unicode-casemap"]
            }
        },
        "accounts": {
            "A13824": {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {}
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:core": "A13824" },
        "username": "john@example.com",
        "apiUrl": "https://jmap.example.com/api/",
        "downloadUrl": "https://jmap.example.com/download/{accountId}/{blobId}/{name}?accept={type}",
        "uploadUrl": "https://jmap.example.com/upload/{accountId}/",
        "eventSourceUrl": "https://jmap.example.com/eventsource/?types={types}&closeafter={closeafter}&ping={ping}",
        "state": "75128aab4b1b"
    });
    v.as_object().expect("object").clone()
}

/// L276: "Servers implementing this specification MUST include the
/// following fields in their JMAP Session object"; L278 `ownerUserId`
/// (String), L281-293 `peerSigningKeys` (JWK[]), L297 `ownerDirectAddress`
/// (String, optional). Parsed by an independent RFC 8620 parser, the fields
/// land in its extension map and read back.
#[test]
fn reads_fields_from_parsed_session_extra() {
    let mut wire = owner_session();
    wire.insert("ownerUserId".into(), json!("john@example.com"));
    wire.insert("peerSigningKeys".into(), json!([draft_key()]));
    wire.insert(
        "ownerDirectAddress".into(),
        json!("john-laptop.tailnet.example"),
    );
    let session: jmap_base_client::Session =
        serde_json::from_value(Value::Object(wire)).expect("valid RFC 8620 Session");

    let f = FederationSession::from_session_extra(&session.extra).expect("federation fields");
    assert_eq!(f.owner_user_id, "john@example.com");
    assert_eq!(f.peer_signing_keys, vec![draft_key()]);
    assert_eq!(
        f.owner_direct_address.as_deref(),
        Some("john-laptop.tailnet.example")
    );
}

/// L295-297: "Servers MAY include the following additional field:
/// `ownerDirectAddress`" — absent is valid and stays absent on the wire.
#[test]
fn owner_direct_address_is_optional() {
    let wire = json!({
        "ownerUserId": "john@example.com",
        "peerSigningKeys": [draft_key()]
    });
    let f = FederationSession::from_session_extra(wire.as_object().expect("object"))
        .expect("federation fields");
    assert_eq!(f.owner_direct_address, None);
    assert_eq!(serde_json::to_value(&f).expect("serialize"), wire);
}

/// L276-282: `ownerUserId` and `peerSigningKeys` are MUST fields.
#[test]
fn required_fields_missing_is_an_error() {
    let no_owner = json!({ "peerSigningKeys": [draft_key()] });
    assert!(FederationSession::from_session_extra(no_owner.as_object().expect("object")).is_err());
    let no_keys = json!({ "ownerUserId": "john@example.com" });
    assert!(FederationSession::from_session_extra(no_keys.as_object().expect("object")).is_err());
}

/// L210: the receiving server "MUST ignore any other entry" than a usable
/// key, so a malformed entry does not make the fields unreadable.
#[test]
fn malformed_key_entry_is_kept_for_the_receiver_to_ignore() {
    let wire = json!({
        "ownerUserId": "john@example.com",
        "peerSigningKeys": [{ "kty": "RSA" }, "not a jwk", draft_key()]
    });
    let f = FederationSession::from_session_extra(wire.as_object().expect("object"))
        .expect("federation fields");
    assert_eq!(f.peer_signing_keys.len(), 3);
    assert_eq!(f.peer_signing_keys[2], draft_key());
}

/// L212: "A server MUST answer an unsigned GET of its `/.well-known/jmap`
/// with a Session object that includes `ownerUserId` and `peerSigningKeys`
/// ...; in it, `accounts` and `primaryAccounts` MAY be empty objects and
/// `username` MAY be the empty string." L166: "The response is the peer's
/// JMAP Session object as defined in {{RFC8620}} Section 2", so the result
/// still parses as one.
#[test]
fn anonymous_session_matches_l212() {
    let mut fields = FederationSession::new("john@example.com", vec![draft_key()]);
    fields.owner_direct_address = Some("john-laptop.tailnet.example".into());
    let anon = anonymous_well_known_session(owner_session(), &fields).expect("built");

    assert_eq!(anon["accounts"], json!({}));
    assert_eq!(anon["primaryAccounts"], json!({}));
    assert_eq!(anon["username"], json!(""));
    assert_eq!(anon["ownerUserId"], json!("john@example.com"));
    assert_eq!(anon["peerSigningKeys"], json!([draft_key()]));
    assert_eq!(
        anon["ownerDirectAddress"],
        json!("john-laptop.tailnet.example")
    );
    for k in [
        "capabilities",
        "apiUrl",
        "downloadUrl",
        "uploadUrl",
        "eventSourceUrl",
        "state",
    ] {
        assert_eq!(anon[k], owner_session()[k], "{k} kept");
    }

    let parsed: jmap_base_client::Session =
        serde_json::from_value(Value::Object(anon)).expect("valid RFC 8620 Session");
    assert!(parsed.accounts.is_empty());
    assert!(parsed.primary_accounts.is_empty());
    assert_eq!(
        FederationSession::from_session_extra(&parsed.extra).expect("fields"),
        fields
    );
}

/// L297-298: `ownerDirectAddress` is optional; a stale value in the base
/// Session is not published when `fields` has none.
#[test]
fn anonymous_session_omits_absent_direct_address() {
    let mut base = owner_session();
    base.insert("ownerDirectAddress".into(), json!("stale.example"));
    let fields = FederationSession::new("john@example.com", vec![draft_key()]);
    let anon = anonymous_well_known_session(base, &fields).expect("built");
    assert!(!anon.contains_key("ownerDirectAddress"));
}

/// L282: "Servers MUST advertise this field with at least one key."
#[test]
fn anonymous_session_requires_a_key() {
    let fields = FederationSession::new("john@example.com", vec![]);
    assert_eq!(
        anonymous_well_known_session(owner_session(), &fields),
        Err(AnonymousSessionError::NoPeerSigningKeys)
    );
}

/// L208: "A JWK in `peerSigningKeys` SHOULD NOT contain private key
/// members." RFC 8037 App. A.1 gives the private member `d` of the draft's
/// example key.
#[test]
fn anonymous_session_refuses_private_key_member() {
    let mut private = draft_key();
    private["d"] = json!("nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A");
    let fields = FederationSession::new("john@example.com", vec![draft_key(), private]);
    assert_eq!(
        anonymous_well_known_session(owner_session(), &fields),
        Err(AnonymousSessionError::PrivateKeyMember(1))
    );
}

/// draft-atwood-jmap-chat-00 L340-341: "`ownerEndpoints` (Endpoint[],
/// optional): ... Peers that probe `/.well-known/jmap` SHOULD merge these
/// into the `endpoints` field of the corresponding ChatContact record."
/// L531-532: "Servers populate this field from the ChatContact's
/// `ownerEndpoints` at `/.well-known/jmap`." The Endpoint values are the
/// draft's L375-377 type URIs with hand-written URIs; the result is
/// checked by writing it into a hand-written ChatContact.
#[test]
fn owner_endpoints_become_contact_endpoints() {
    let advertised = json!([
        { "type": "urn:ietf:params:jmap:chat:cap:vtc", "uri": "https://meet.b.example/bob", "label": "Personal Jitsi" },
        { "type": "urn:ietf:params:jmap:chat:cap:payment", "uri": "lightning:lnurl1bob", "metadata": { "network": "lightning" } },
    ]);
    let session = json!({ "ownerUserId": "bob@b.example", "ownerEndpoints": advertised.clone() });
    let endpoints = owner_endpoints(session.as_object().unwrap()).expect("advertised");

    let mut contact = json!({
        "id": "c1",
        "identity": "bob@b.example",
        "login": "bob",
        "firstSeenAt": "2026-01-01T00:00:00Z",
        "lastSeenAt": "2026-01-02T00:00:00Z",
    });
    contact["endpoints"] = serde_json::to_value(&endpoints).unwrap();
    let contact: ChatContact = serde_json::from_value(contact).expect("valid ChatContact");
    assert_eq!(
        serde_json::to_value(&contact.endpoints).unwrap(),
        advertised
    );
}

/// L340: `ownerEndpoints` is "optional"; a Session without it, or whose
/// value is not an Endpoint[] array, advertises no endpoints.
#[test]
fn absent_owner_endpoints_is_none() {
    assert_eq!(owner_endpoints(&Map::new()), None);
    let not_array = json!({ "ownerEndpoints": { "type": "urn:ietf:params:jmap:chat:cap:vtc" } });
    assert_eq!(owner_endpoints(not_array.as_object().unwrap()), None);
    let empty = json!({ "ownerEndpoints": [] });
    assert_eq!(
        owner_endpoints(empty.as_object().unwrap()),
        Some(Vec::new())
    );
}

/// L372 and L385: an Endpoint has "`type` (String)" and "`uri`
/// (String)". An entry missing either is not an Endpoint and is skipped;
/// L370: "clients MUST silently ignore Endpoint records whose `type` they
/// do not recognize", so an unknown type is kept for them to ignore.
#[test]
fn malformed_entries_skipped_unknown_types_kept() {
    let session = json!({ "ownerEndpoints": [
        { "type": "urn:ietf:params:jmap:chat:cap:vtc" },
        { "uri": "https://x.example/" },
        "https://y.example/",
        { "type": "urn:example:future", "uri": "https://z.example/" },
    ] });
    let endpoints = owner_endpoints(session.as_object().unwrap()).expect("array");
    assert_eq!(
        serde_json::to_value(&endpoints).unwrap(),
        json!([{ "type": "urn:example:future", "uri": "https://z.example/" }])
    );
}
