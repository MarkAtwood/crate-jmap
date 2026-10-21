//! A client cannot create a ChatContact that breaks the identity/did
//! consistency rule (bd:JMAP-8tuq.3, bd:JMAP-0x0q.12).
//!
//! Oracle: draft-atwood-jmap-chat-did-00 (jmap-chat-spec 6bcd9d8).
//! L201: `did` "is server-set (`ChatContact/set` create accepts only
//! `identity`)". L203: "If `did` is present AND `identity` parses as a DID
//! URI, the two MUST refer to the same DID." L291: the rule "defends
//! against impersonation in which an attacker submits a ChatContact with
//! an `identity` they control and a `did` belonging to a different
//! principal." RFC 8620 §5.3 `invalidProperties`: "a property called
//! `properties` with the list of invalid properties".
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

/// The attack L291 names: a DID-form `identity` the attacker controls and
/// another principal's `did`. Since `did` is server-set (L201), the create
/// is rejected naming `did` and nothing is stored.
#[tokio::test]
async fn create_with_mismatched_did_rejected() {
    let backend = backend();
    let resp = create(&backend, json!({ "identity": DID_A, "did": DID_B })).await;

    assert!(resp["created"]["k1"].is_null(), "{resp}");
    assert_eq!(
        resp["notCreated"]["k1"],
        json!({ "type": "invalidProperties", "properties": ["did"] }),
        "{resp}"
    );
    assert_eq!(get_all(&backend).await, json!([]));
}

/// A consistent pair is still a client-supplied `did`, so it is rejected
/// too (L201).
#[tokio::test]
async fn create_with_matching_did_rejected() {
    let backend = backend();
    let resp = create(&backend, json!({ "identity": DID_B, "did": DID_B })).await;

    assert!(resp["created"]["k1"].is_null(), "{resp}");
    assert_eq!(
        resp["notCreated"]["k1"],
        json!({ "type": "invalidProperties", "properties": ["did"] }),
        "{resp}"
    );
}

/// A DID-form `identity` alone is created with no `did` bound, so the
/// L203 rule's precondition ("`did` is present") does not hold.
#[tokio::test]
async fn create_with_did_identity_binds_no_did() {
    let backend = backend();
    let resp = create(&backend, json!({ "identity": DID_A })).await;

    assert!(resp["notCreated"].is_null(), "{resp}");
    assert_eq!(resp["created"]["k1"]["identity"], json!(DID_A), "{resp}");
    assert!(resp["created"]["k1"].get("did").is_none(), "{resp}");
}
