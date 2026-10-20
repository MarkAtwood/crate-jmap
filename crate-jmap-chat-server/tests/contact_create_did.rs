//! `ChatContact/set` create with the optional `did` (bd:JMAP-8tuq.2).
//!
//! Oracle: draft-atwood-jmap-chat-did-00 (jmap-chat-spec 3227c0f)
//! §Optional `did` field, L160-163: "ChatContacts MAY carry an optional
//! `did` field to accommodate the pseudonymous case, in which a stable
//! `identity` that is not a DID is preserved" and "`did` (String,
//! optional): A DID URI ({{DID-CORE}}) bound to this ChatContact". RFC 8620
//! §5.3 `invalidProperties`: "The record given is invalid in some way",
//! with "a property called `properties` with the list of invalid
//! properties". `did:web:w3c-ccg.github.io:user:alice` is the did:web
//! method specification's example; DID Core §3.1 ABNF: `did = "did:"
//! method-name ":" method-specific-id`.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_contact_get, handle_contact_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

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

async fn get(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_contact_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("ChatContact/get");
    resp["list"][0].clone()
}

/// The pseudonymous case: a non-DID `identity` with a bound `did` is
/// created, and `/get` returns the `did`.
#[tokio::test]
async fn create_with_did_stores_it() {
    let backend = backend();
    let resp = create(
        &backend,
        json!({
            "identity": "alice@example.com",
            "did": "did:web:w3c-ccg.github.io:user:alice"
        }),
    )
    .await;

    assert!(resp["notCreated"].is_null(), "{resp}");
    let id = resp["created"]["k1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created.k1.id missing: {resp}"))
        .to_owned();
    let contact = get(&backend, &id).await;
    assert_eq!(contact["identity"], json!("alice@example.com"));
    assert_eq!(
        contact["did"],
        json!("did:web:w3c-ccg.github.io:user:alice")
    );
}

/// `did` is "A DID URI": `alice@example.com` lacks the ABNF's `"did:"`
/// prefix, and `42` is not a String. Each is rejected with
/// `invalidProperties` naming `did`, and nothing is recorded.
#[tokio::test]
async fn create_with_invalid_did_rejected() {
    for bad in [json!("alice@example.com"), json!(42)] {
        let backend = backend();
        let resp = create(
            &backend,
            json!({ "identity": "alice@example.com", "did": bad }),
        )
        .await;

        assert!(resp["created"]["k1"].is_null(), "{resp}");
        assert_eq!(
            resp["notCreated"]["k1"],
            json!({ "type": "invalidProperties", "properties": ["did"] })
        );
        let retry = create(&backend, json!({ "identity": "alice@example.com" })).await;
        assert!(retry["created"]["k1"]["id"].is_string(), "{retry}");
    }
}

/// "When absent, no DID is bound to this ChatContact" (L163). A `null`
/// `did` reads as absent, as for other optional create properties.
#[tokio::test]
async fn create_with_null_did_binds_none() {
    let backend = backend();
    let resp = create(
        &backend,
        json!({ "identity": "alice@example.com", "did": null }),
    )
    .await;

    assert!(resp["notCreated"].is_null(), "{resp}");
    let id = resp["created"]["k1"]["id"].as_str().expect("id").to_owned();
    assert!(get(&backend, &id).await.get("did").is_none());
}
