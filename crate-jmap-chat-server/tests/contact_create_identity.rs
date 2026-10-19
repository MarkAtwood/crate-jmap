//! `ChatContact/set` create by `identity` and the `ChatContact/query`
//! `identity` filter (bd:JMAP-405z.3).
//!
//! Oracle: draft-atwood-jmap-chat-00 (jmap-chat-spec cefc217)
//! §ChatContact/set and §ChatContact/query, quoted on each test.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_contact_get, handle_contact_query, handle_contact_set};
use jmap_types::Id;
use serde_json::{json, Value};

/// A backend with [`ACCOUNT_ID`] registered and no ChatContacts.
fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

/// Store a ChatContact with the given `id` and `identity`.
fn insert_contact(backend: &MemoryBackend, id: &str, identity: &str) {
    backend.insert_object_for_test(
        "ChatContact",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "identity": identity,
            "login": identity,
            "firstSeenAt": "2026-01-01T00:00:00Z",
            "lastSeenAt": "2026-01-01T00:00:00Z",
            "blocked": false
        }),
    );
}

/// `ChatContact/set` with the given `create` map.
async fn create(backend: &MemoryBackend, create: Value) -> Value {
    let (resp, _) = handle_contact_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": create }),
    )
    .await
    .expect("ChatContact/set");
    resp
}

/// The sorted `properties` of a SetError.
fn error_properties(err: &Value) -> Vec<String> {
    let mut props: Vec<String> = err["properties"]
        .as_array()
        .unwrap_or_else(|| panic!("properties missing from {err}"))
        .iter()
        .map(|p| p.as_str().expect("property name").to_owned())
        .collect();
    props.sort();
    props
}

/// draft-atwood-jmap-chat-00 §ChatContact/set: "`create` accepts only
/// `identity`, which is required; the server assigns `id` and sets the
/// other server-set properties."
#[tokio::test]
async fn contact_create_by_identity_assigns_id() {
    let backend = backend();
    let resp = create(
        &backend,
        json!({ "k1": { "identity": "alice@example.com" } }),
    )
    .await;

    assert!(
        resp["notCreated"].is_null(),
        "create with identity must succeed: {}",
        resp["notCreated"]
    );
    let id = resp["created"]["k1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created.k1.id missing: {resp}"))
        .to_owned();

    let (get, _) = handle_contact_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [&id] }),
    )
    .await
    .expect("ChatContact/get");
    assert_eq!(get["list"][0]["id"], json!(id));
    assert_eq!(get["list"][0]["identity"], json!("alice@example.com"));
}

/// draft-atwood-jmap-chat-00 §ChatContact/set: "If a ChatContact with
/// that `identity` already exists, the server MUST reject the create with
/// a SetError of type `alreadyExists`, including the existing
/// ChatContact's id in the `existingId` field ({{RFC8620}} Section 5.3)."
#[tokio::test]
async fn contact_create_existing_identity_already_exists() {
    let backend = backend();
    insert_contact(&backend, "ct1", "alice@example.com");

    let resp = create(
        &backend,
        json!({ "k1": { "identity": "alice@example.com" } }),
    )
    .await;

    assert!(resp["created"]["k1"].is_null(), "duplicate created: {resp}");
    assert_eq!(resp["notCreated"]["k1"]["type"], json!("alreadyExists"));
    assert_eq!(resp["notCreated"]["k1"]["existingId"], json!("ct1"));
}

/// draft-atwood-jmap-chat-00 §ChatContact/set: "`create` accepts only
/// `identity`, which is required". A create without `identity` is an
/// invalid record (RFC 8620 §5.3 `invalidProperties`).
#[tokio::test]
async fn contact_create_without_identity_invalid_properties() {
    let backend = backend();
    let resp = create(&backend, json!({ "k1": {} })).await;

    assert!(
        resp["created"]["k1"].is_null(),
        "created without identity: {resp}"
    );
    assert_eq!(resp["notCreated"]["k1"]["type"], json!("invalidProperties"));
    assert_eq!(error_properties(&resp["notCreated"]["k1"]), ["identity"]);
}

/// draft-atwood-jmap-chat-00 §ChatContact/set: "`create` accepts only
/// `identity`". Any other property in a create is rejected with an RFC
/// 8620 §5.3 `invalidProperties` SetError naming it, and no ChatContact
/// is recorded.
#[tokio::test]
async fn contact_create_other_properties_invalid_properties() {
    let backend = backend();
    let resp = create(
        &backend,
        json!({
            "k1": {
                "identity": "alice@example.com",
                "login": "alice",
                "blocked": true,
                "displayName": "Alice"
            }
        }),
    )
    .await;

    assert!(
        resp["created"]["k1"].is_null(),
        "created with extras: {resp}"
    );
    assert_eq!(resp["notCreated"]["k1"]["type"], json!("invalidProperties"));
    assert_eq!(
        error_properties(&resp["notCreated"]["k1"]),
        ["blocked", "displayName", "login"]
    );

    // The rejected create recorded nothing, so the identity is still free.
    let retry = create(
        &backend,
        json!({ "k2": { "identity": "alice@example.com" } }),
    )
    .await;
    assert!(
        retry["created"]["k2"]["id"].is_string(),
        "identity must still be creatable: {retry}"
    );
}

/// draft-atwood-jmap-chat-00 §ChatContact/query: "Filter properties:
/// `blocked` (Boolean), `presence` (String), `identity` (String; matches
/// the ChatContact whose `identity` equals the value octet for octet)."
/// The case-variant contact shows the match is octet-exact.
#[tokio::test]
async fn contact_query_identity_filter_matches_octet_for_octet() {
    let backend = backend();
    insert_contact(&backend, "ct1", "alice@example.com");
    insert_contact(&backend, "ct2", "Alice@example.com");
    insert_contact(&backend, "ct3", "bob@example.com");

    let (resp, _) = handle_contact_query(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "filter": { "identity": "alice@example.com" }
        }),
    )
    .await
    .expect("ChatContact/query");

    assert_eq!(resp["ids"], json!(["ct1"]));
}
