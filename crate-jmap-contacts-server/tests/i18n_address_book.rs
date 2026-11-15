//! draft-atwood-jmap-updates-00 on AddressBook (§5.3: `name` and
//! `description` are localizable) through the contacts handlers and the
//! in-memory backend.
//!
//! Oracle: the draft's Appendix A examples and normative text at
//! jmap-chat-spec 2b85363 (§3, §4.1, §4.2.1, §5.3), with hand-written
//! requests and expected responses.

mod common;

use common::MemoryBackend;
use jmap_contacts_server::{
    handle_address_book_get, handle_address_book_set, handle_contact_card_get,
};
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "a1";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:contacts".to_owned(),
        "urn:ietf:params:jmap:i18n".to_owned(),
    ]
}

fn using_without_i18n() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:contacts".to_owned(),
    ]
}

fn backend() -> MemoryBackend {
    MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID))
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

async fn set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    handle_address_book_set(backend, &(), args, using)
        .await
        .unwrap()
        .primary
}

async fn get(backend: &MemoryBackend, args: Value, using: &[String]) -> Result<Value, JmapError> {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_address_book_get(backend, &(), args, using)
        .await
        .map(|o| o.primary)
}

/// Create the book of Appendix A ("Team contacts", which the draft does
/// not name; the base name is this test's choice) and run the Appendix
/// A.1 update on it. Returns its id.
async fn appendix_a_book(backend: &MemoryBackend) -> String {
    let resp = set(
        backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "c": { "name": "Team contacts" } } }),
        &using(),
    )
    .await;
    let id = resp["created"]["c"]["id"].as_str().unwrap().to_owned();
    // Appendix A.1, verbatim apart from the id.
    let resp = set(
        backend,
        json!({
            "accountId": ACCOUNT_ID,
            "update": {
                id.clone(): {
                    "locale": "en",
                    "localizations": {
                        "fr": { "name": "Contacts du groupe" },
                        "de": { "name": "Team-Kontakte" }
                    }
                }
            }
        }),
        &using(),
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
    id
}

/// Appendix A.2: AddressBook/get with `properties: ["name", "locale"]`
/// and `language: ["fr-CH", "de"]` returns `"name": "Contacts du groupe",
/// "locale": "fr"`.
#[tokio::test]
async fn appendix_a() {
    let backend = backend();
    let id = appendix_a_book(&backend).await;
    let resp = get(
        &backend,
        json!({ "ids": [id], "properties": ["name", "locale"], "language": ["fr-CH", "de"] }),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(
        resp["list"][0],
        json!({ "id": id, "name": "Contacts du groupe", "locale": "fr" })
    );
    assert_eq!(resp["notFound"], json!([]));
}

/// §4.1: a null localized `description` says there is none in that
/// language; `name` is not nullable.
#[tokio::test]
async fn description_is_nullable_name_is_not() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "c": {
            "name": "Team", "description": "Our team", "locale": "en",
            "localizations": { "fr": { "description": null } }
        } } }),
        &using(),
    )
    .await;
    let id = resp["created"]["c"]["id"].as_str().unwrap().to_owned();
    let fr = get(
        &backend,
        json!({ "ids": [id], "language": ["fr"] }),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(fr["list"][0]["description"], Value::Null);
    assert_eq!(fr["list"][0]["name"], "Team");

    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "c": {
            "name": "Team", "localizations": { "fr": { "name": null } }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["c"]["properties"],
        json!(["localizations"])
    );
}

/// §4.1 pruning: changing `description` drops stale description
/// translations and keeps the names.
#[tokio::test]
async fn changing_description_prunes_its_translations() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "c": {
            "name": "Team", "description": "Our team", "locale": "en",
            "localizations": { "fr": { "name": "Équipe", "description": "Notre équipe" } }
        } } }),
        &using(),
    )
    .await;
    let id = resp["created"]["c"]["id"].as_str().unwrap().to_owned();
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { id.clone(): { "description": "All of us" } } }),
        &using_without_i18n(),
    )
    .await;
    assert!(
        resp["updated"][&id].get("localizations").is_none(),
        "{resp}"
    );
    let got = get(&backend, json!({ "ids": [id] }), &using())
        .await
        .unwrap();
    assert_eq!(
        got["list"][0]["localizations"],
        json!({ "fr": { "name": "Équipe" } })
    );
}

/// §3: without the capability in `using` the properties are neither
/// returned nor accepted, and `language` is invalidArguments.
#[tokio::test]
async fn without_capability() {
    let backend = backend();
    let id = appendix_a_book(&backend).await;
    let resp = get(&backend, json!({ "ids": [id] }), &using_without_i18n())
        .await
        .unwrap();
    assert!(resp["list"][0].get("localizations").is_none());
    let err = get(
        &backend,
        json!({ "language": ["fr"] }),
        &using_without_i18n(),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), "invalidArguments");
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "c": { "name": "x", "locale": "en" } } }),
        &using_without_i18n(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["locale"]));
}

/// §5.3: "the language argument does not apply to ContactCard/get".
#[tokio::test]
async fn contact_card_rejects_language() {
    let backend = backend();
    let err = handle_contact_card_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "language": ["fr"] }),
        &using(),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), "invalidArguments");
}
