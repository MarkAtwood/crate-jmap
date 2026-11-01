//! Server-set Space and ChatContact properties are rejected on `/set`
//! (bd:JMAP-t8ej.13).
//!
//! Oracles (jmap-chat-spec commit 77c3474, draft-atwood-jmap-chat-00):
//!
//! - L886-895: "`roles` (SpaceRole\[], server-set)", "`members`
//!   (SpaceMember\[], server-set)", "`categories` (Category\[],
//!   server-set)", "`uncategorizedChannelIds` (String\[], server-set)",
//!   each "Changed through the ... patch keys of `Space/set`".
//! - L542: "`endpoints` (Endpoint\[], optional, server-set)".
//! - RFC 8620 §1.1: "server-set" -- "Only the server can set the value for
//!   this property. The client MUST NOT send this property when creating a
//!   new object of this type." §5.3 `invalidProperties`: "It contains a
//!   property that may only be set by the server (e.g., "id") and is
//!   different to the current value."
//!
//! Each update sends a value that differs from the stored one, so the
//! RFC 8620 §5.3 identical-value allowance does not apply.

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::{
    handle_contact_get, handle_contact_set, handle_space_get, handle_space_set,
};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const CONTACT: &str = "ct1";

/// The four server-set Space structure properties, each with a value no
/// newly created Space holds.
fn space_server_set_values() -> [(&'static str, Value); 4] {
    [
        (
            "roles",
            json!([{ "id": "r9", "name": "Mod", "permissions": [], "position": 7 }]),
        ),
        (
            "members",
            json!([{ "id": "intruder", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }]),
        ),
        (
            "categories",
            json!([{ "id": "cat9", "name": "Ops", "position": 0, "channelIds": [] }]),
        ),
        ("uncategorizedChannelIds", json!(["chan9"])),
    ]
}

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

async fn space_set(backend: &MemoryBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } =
        handle_space_set(backend, &(), &std::collections::BTreeMap::new(), args, &[])
            .await
            .expect("Space/set");
    primary
}

async fn all_spaces(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary, .. } = handle_space_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": null }),
        &[],
    )
    .await
    .expect("Space/get");
    primary["list"].clone()
}

#[tokio::test]
async fn space_create_with_server_set_structure_is_invalid_properties() {
    for (key, value) in space_server_set_values() {
        let backend = backend();
        let resp = space_set(
            &backend,
            json!({
                "accountId": ACCOUNT_ID,
                "create": { "s0": { "name": "Team", key: value } },
            }),
        )
        .await;
        assert_eq!(
            resp["notCreated"]["s0"],
            json!({ "type": "invalidProperties", "properties": [key] }),
            "{key}: {resp}"
        );
        assert_eq!(
            all_spaces(&backend).await,
            json!([]),
            "{key}: nothing stored"
        );
    }
}

#[tokio::test]
async fn space_update_with_server_set_structure_is_invalid_properties() {
    let backend = backend();
    let resp = space_set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "s0": { "name": "Team" } } }),
    )
    .await;
    let id = resp["created"]["s0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Space not created: {resp}"))
        .to_owned();
    let before = all_spaces(&backend).await;

    for (key, value) in space_server_set_values() {
        let resp = space_set(
            &backend,
            json!({ "accountId": ACCOUNT_ID, "update": { &id: { key: value } } }),
        )
        .await;
        assert_eq!(
            resp["notUpdated"][&id],
            json!({ "type": "invalidProperties", "properties": [key] }),
            "{key}: {resp}"
        );
        assert_eq!(all_spaces(&backend).await, before, "{key}: Space unchanged");
    }
}

async fn stored_contact(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary, .. } = handle_contact_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [CONTACT] }),
    )
    .await
    .expect("ChatContact/get");
    primary["list"][0].clone()
}

#[tokio::test]
async fn contact_update_with_endpoints_is_invalid_properties() {
    let backend = backend();
    backend.insert_object_for_test(
        "ChatContact",
        ACCOUNT_ID,
        CONTACT,
        json!({
            "id": CONTACT,
            "identity": "alice@example.com",
            "login": "alice@example.com",
            "firstSeenAt": "2026-01-01T00:00:00Z",
            "lastSeenAt": "2026-01-01T00:00:00Z",
            "blocked": false,
        }),
    );
    let before = stored_contact(&backend).await;

    let HandlerOutput { primary: resp, .. } = handle_contact_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { CONTACT: { "endpoints": [{
                "type": "urn:ietf:params:jmap:chat:cap:vtc",
                "uri": "https://meet.example.com/alice",
            }] } },
        }),
    )
    .await
    .expect("ChatContact/set");
    assert_eq!(
        resp["notUpdated"][CONTACT],
        json!({ "type": "invalidProperties", "properties": ["endpoints"] }),
        "{resp}"
    );
    assert_eq!(stored_contact(&backend).await, before);
}
