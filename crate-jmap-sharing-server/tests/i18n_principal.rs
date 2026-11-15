//! draft-atwood-jmap-updates-00 on Principal (§5.4: `name` and
//! `description` are localizable) through the sharing handlers and the
//! in-memory backend.
//!
//! Oracle: the draft's normative text at jmap-chat-spec 2b85363 (§3,
//! §4.1, §4.2, §5.4), with hand-written requests and expected responses.

mod common;

use common::MemoryBackend;
use jmap_sharing_server::{
    SharingBackend, handle_principal_get, handle_principal_query, handle_principal_query_changes,
    handle_principal_set,
};
use jmap_sharing_types::Principal;
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "acc1";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:principals".to_owned(),
        "urn:ietf:params:jmap:i18n".to_owned(),
    ]
}

fn using_without_i18n() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:principals".to_owned(),
    ]
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

/// A room with a French name and no French description.
async fn room(backend: &MemoryBackend) -> String {
    let principal = |id: &Id| -> Principal {
        serde_json::from_value(json!({
            "id": id,
            "type": "location",
            "name": "Room 4B",
            "description": "Second floor",
            "email": null,
            "timeZone": null,
            "capabilities": {},
            "accounts": null,
            "locale": "en",
            "localizations": { "fr": { "name": "Salle 4B", "description": null } }
        }))
        .unwrap()
    };
    let (id, _) = backend
        .create_object(&(), &Id::from(ACCOUNT_ID), "seed", principal)
        .await
        .unwrap();
    id.as_ref().to_owned()
}

/// §4.2.1 on Principal/get, including a null localized `description`.
#[tokio::test]
async fn get_with_language() {
    let backend = MemoryBackend::new_with_accounts(&[ACCOUNT_ID]);
    let id = room(&backend).await;
    let resp = handle_principal_get(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID, "ids": [id],
            "properties": ["name", "description", "locale"], "language": ["fr-CA"]
        }),
        &using(),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(
        resp["list"][0],
        json!({ "id": id, "name": "Salle 4B", "description": null, "locale": "fr" })
    );
}

/// §4.2.3 on Principal/query and /queryChanges: the queryState records
/// the language and a mismatch is cannotCalculateChanges.
#[tokio::test]
async fn query_state_records_language() {
    let backend = MemoryBackend::new_with_accounts(&[ACCOUNT_ID]);
    room(&backend).await;
    let q = handle_principal_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "language": ["fr"] }),
        &using(),
    )
    .await
    .unwrap()
    .primary;
    let state = q["queryState"].as_str().unwrap().to_owned();
    assert!(state.ends_with("|lang=fr"), "{state}");

    let err = handle_principal_query_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceQueryState": state, "language": ["de"] }),
        &using(),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), "cannotCalculateChanges");
    let ok = handle_principal_query_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceQueryState": state, "language": ["fr"] }),
        &using(),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(ok["oldQueryState"], json!(state));
}

/// §3: without the capability `language` is invalidArguments on
/// Principal/query, and `/set` naming `localizations` is invalidProperties.
#[tokio::test]
async fn without_capability() {
    let backend = MemoryBackend::new_with_accounts(&[ACCOUNT_ID]);
    let id = room(&backend).await;
    let err = handle_principal_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "language": ["fr"] }),
        &using_without_i18n(),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), "invalidArguments");
    let resp = handle_principal_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { id.clone(): { "localizations": null } } }),
        &using_without_i18n(),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(
        resp["notUpdated"][&id]["properties"],
        json!(["localizations"])
    );
}

/// §4.1: renaming the Principal prunes the French name and keeps the
/// French (null) description; an i18n client gets `localizations` back.
#[tokio::test]
async fn rename_prunes_and_echoes() {
    let backend = MemoryBackend::new_with_accounts(&[ACCOUNT_ID]);
    let id = room(&backend).await;
    let resp = handle_principal_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { id.clone(): { "name": "Room 4C" } } }),
        &using(),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(
        resp["updated"][&id]["localizations"],
        json!({ "fr": { "description": null } }),
        "{resp}"
    );
}
