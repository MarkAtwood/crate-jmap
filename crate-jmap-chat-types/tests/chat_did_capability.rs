//! `urn:ietf:params:jmap:chat:did` marker capability and its advertising
//! rule.
//!
//! Oracle: sentences quoted from
//! /home/mark/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-did-00.md
//! (line numbers at HEAD) and hand-written method lists.

use jmap_chat_types::{JMAP_CHAT_DID_URI, MANDATORY_DID_METHODS, may_advertise_chat_did};

#[test]
fn uri_matches_draft() {
    // L118: "Servers supporting this specification MUST advertise the
    // `urn:ietf:params:jmap:chat:did` capability in the JMAP Session
    // object."
    assert_eq!(JMAP_CHAT_DID_URI, "urn:ietf:params:jmap:chat:did");
}

#[test]
fn mandatory_methods_are_web_key_plc() {
    // L134-138: the server "MUST be able to resolve DIDs of the following
    // methods": `did:web`, `did:key`, `did:plc`. The method names go in
    // `supportedMethods` as DID Core §8 names (draft-atwood-jmap-did-00
    // L123: "e.g., `\"key\"`, `\"web\"`").
    let mut got = MANDATORY_DID_METHODS.to_vec();
    got.sort_unstable();
    assert_eq!(got, ["key", "plc", "web"]);
}

#[test]
fn advertised_when_jmap_did_lists_all_three() {
    // L124: "The `supportedMethods` array MUST include at least the three
    // mandatory-to-implement methods".
    assert!(may_advertise_chat_did(Some(&["plc", "key", "web"])));
}

#[test]
fn extra_methods_do_not_block_advertising() {
    // L144: "Servers MAY additionally support any other DID method".
    let methods = vec![
        "web".to_owned(),
        "webvh".to_owned(),
        "key".to_owned(),
        "plc".to_owned(),
        "ethr".to_owned(),
    ];
    assert!(may_advertise_chat_did(Some(methods.as_slice())));
}

#[test]
fn not_advertised_without_jmap_did() {
    // L100: "A server advertising `urn:ietf:params:jmap:chat:did` MUST
    // also advertise `urn:ietf:params:jmap:did`".
    assert!(!may_advertise_chat_did::<&str>(None));
}

#[test]
fn not_advertised_when_any_mandatory_method_missing() {
    // L140: "A server that cannot resolve any one of these three methods
    // MUST NOT advertise the `urn:ietf:params:jmap:chat:did` capability."
    assert!(!may_advertise_chat_did(Some(&["key", "plc"])));
    assert!(!may_advertise_chat_did(Some(&["web", "plc"])));
    assert!(!may_advertise_chat_did(Some(&["web", "key"])));
}

#[test]
fn not_advertised_when_no_method_resolves() {
    // L96: "A deployment that supports JMAP Chat but does not resolve any
    // DID method MUST NOT advertise the capability defined here."
    assert!(!may_advertise_chat_did::<&str>(Some(&[])));
}

#[test]
fn prefixed_names_are_not_method_names() {
    // draft-atwood-jmap-did-00 L123: "Each value is a DID method name as
    // defined in {{DID-CORE}} Section 8 (e.g., `\"key\"`, `\"web\"`)" —
    // `"did:web"` is a DID prefix, not a method name.
    assert!(!may_advertise_chat_did(Some(&[
        "did:web", "did:key", "did:plc"
    ])));
}
