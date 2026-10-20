//! `ChatContact/set` create enforces the identity/did consistency rule
//! (bd:JMAP-8tuq.3).
//!
//! Oracle: draft-atwood-jmap-chat-did-00 (jmap-chat-spec 3227c0f).
//! L165: "If `did` is present AND `identity` parses as a DID URI, the two
//! MUST refer to the same DID. A server detecting a mismatch (the parsed
//! DID from `identity` differs from the DID in `did`) MUST treat the
//! ChatContact record as invalid and MUST NOT use either DID for
//! authentication." L264: "Implementations MUST enforce the consistency
//! rule; failure to do so is a security bug." RFC 8620 §5.3
//! `invalidProperties`: "The record given is invalid in some way", with
//! "a property called `properties` with the list of invalid properties".
//! `did:example:123456789abcdefghi` is the DID Core §3.1 example DID and
//! `did:web:w3c-ccg.github.io:user:alice` is the did:web method
//! specification's example.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_contact_get, handle_contact_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

const DID_A: &str = "did:example:123456789abcdefghi";
const DID_B: &str = "did:web:w3c-ccg.github.io:user:alice";

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

async fn create(backend: &MemoryBackend, obj: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_contact_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": { "k1": obj } }),
    )
    .await
    .expect("ChatContact/set");
    resp
}

async fn get_all(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_contact_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": null }),
    )
    .await
    .expect("ChatContact/get");
    resp["list"].clone()
}

/// The attack L264 names: "an `identity` they control and a `did`
/// belonging to a different principal". The record is invalid (L165), so
/// the create is rejected with `invalidProperties` and nothing is stored.
#[tokio::test]
async fn create_with_mismatched_did_rejected() {
    let backend = backend();
    let resp = create(&backend, json!({ "identity": DID_A, "did": DID_B })).await;

    assert!(resp["created"]["k1"].is_null(), "{resp}");
    let err = &resp["notCreated"]["k1"];
    assert_eq!(err["type"], json!("invalidProperties"), "{resp}");
    assert_eq!(err["properties"], json!(["identity", "did"]), "{resp}");
    assert_eq!(get_all(&backend).await, json!([]));
}

/// "the two MUST refer to the same DID" (L165): a DID-form `identity`
/// with the same DID in `did` is consistent and is created.
#[tokio::test]
async fn create_with_matching_did_accepted() {
    let backend = backend();
    let resp = create(&backend, json!({ "identity": DID_B, "did": DID_B })).await;

    assert!(resp["notCreated"].is_null(), "{resp}");
    assert_eq!(resp["created"]["k1"]["identity"], json!(DID_B), "{resp}");
    assert_eq!(resp["created"]["k1"]["did"], json!(DID_B), "{resp}");
}

/// The rule applies only "If `did` is present AND `identity` parses as a
/// DID URI" (L165). A DID-form `identity` without `did`, and the
/// pseudonymous non-DID `identity` with `did` (L160), are both created.
#[tokio::test]
async fn create_outside_rule_preconditions_accepted() {
    for obj in [
        json!({ "identity": DID_A }),
        json!({ "identity": DID_A, "did": null }),
        json!({ "identity": "alice@example.com", "did": DID_B }),
    ] {
        let backend = backend();
        let resp = create(&backend, obj.clone()).await;
        assert!(resp["notCreated"].is_null(), "{obj}: {resp}");
        assert!(resp["created"]["k1"]["id"].is_string(), "{obj}: {resp}");
    }
}
