//! draft-atwood-jmap-updates-00 on Mailbox (§5.1: `name` is localizable)
//! through the mail handlers and the in-memory backend.
//!
//! Oracle: the draft's normative text at jmap-chat-spec 2b85363 (§3,
//! §4.1, §4.2, §5.1), with hand-written requests and expected responses.

mod common;

use std::collections::BTreeMap;

use common::MemoryBackend;
use jmap_mail_server::{
    handle_mailbox_get, handle_mailbox_query, handle_mailbox_query_changes, handle_mailbox_set,
};
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "a1";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:mail".to_owned(),
        "urn:ietf:params:jmap:i18n".to_owned(),
    ]
}

fn using_without_i18n() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:mail".to_owned(),
    ]
}

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

async fn set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    handle_mailbox_set(backend, &(), &BTreeMap::new(), args, using)
        .await
        .unwrap()
        .primary
}

/// Create a Mailbox with `extra` properties and return its id.
async fn create(backend: &MemoryBackend, extra: Value) -> String {
    let mut props = json!({ "name": "x" });
    props
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let resp = set(
        backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "c": props } }),
        &using(),
    )
    .await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("create failed: {resp}"))
        .to_owned()
}

async fn get(backend: &MemoryBackend, args: Value, using: &[String]) -> Result<Value, JmapError> {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_mailbox_get(backend, &(), args, using)
        .await
        .map(|o| o.primary)
}

/// Three mailboxes whose base names sort A, B, C and whose German names
/// sort the other way round.
async fn three(backend: &MemoryBackend) -> [String; 3] {
    [
        create(
            backend,
            json!({ "name": "A", "locale": "en", "localizations": { "de": { "name": "Z" } } }),
        )
        .await,
        create(
            backend,
            json!({ "name": "B", "locale": "en", "localizations": { "de": { "name": "Y" } } }),
        )
        .await,
        create(
            backend,
            json!({ "name": "C", "locale": "en", "localizations": { "de": { "name": "X" } } }),
        )
        .await,
    ]
}

/// §4.2.1: Mailbox/get with `language` returns the localized `name` and
/// the chosen `locale`; `state` does not change.
#[tokio::test]
async fn get_with_language() {
    let backend = backend();
    let [a, ..] = three(&backend).await;
    let plain = get(&backend, json!({ "ids": [a] }), &using())
        .await
        .unwrap();
    let de = get(
        &backend,
        json!({ "ids": [a], "properties": ["name", "locale"], "language": ["de-AT"] }),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(
        de["list"][0],
        json!({ "id": a, "name": "Z", "locale": "de" })
    );
    assert_eq!(de["state"], plain["state"]);
    assert_eq!(plain["list"][0]["name"], "A");
    assert_eq!(
        plain["list"][0]["localizations"],
        json!({ "de": { "name": "Z" } })
    );
}

/// §3: without the capability in `using` the properties are not returned
/// and `language` is invalidArguments; `/set` naming them is
/// invalidProperties.
#[tokio::test]
async fn without_capability() {
    let backend = backend();
    let [a, ..] = three(&backend).await;
    let resp = get(&backend, json!({ "ids": [a] }), &using_without_i18n())
        .await
        .unwrap();
    assert!(resp["list"][0].get("locale").is_none());
    assert!(resp["list"][0].get("localizations").is_none());
    let err = get(
        &backend,
        json!({ "language": ["de"] }),
        &using_without_i18n(),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), "invalidArguments");

    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { a.clone(): { "locale": "fr" } } }),
        &using_without_i18n(),
    )
    .await;
    assert_eq!(resp["notUpdated"][&a]["type"], "invalidProperties");
    assert_eq!(resp["notUpdated"][&a]["properties"], json!(["locale"]));
}

/// §4.2.2: Mailbox/query sorted on `name` with `language` sorts on the
/// localized names; the queryState records the language (§4.2.3).
#[tokio::test]
async fn query_sorted_by_localized_name() {
    let backend = backend();
    let [a, b, c] = three(&backend).await;
    let sort = json!([{ "property": "name", "isAscending": true }]);
    let base = handle_mailbox_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sort": sort }),
        &using(),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(base["ids"], json!([a, b, c]));
    let de = handle_mailbox_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sort": sort, "language": ["de"] }),
        &using(),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(de["ids"], json!([c, b, a]));
    assert!(de["queryState"].as_str().unwrap().ends_with("|lang=de"));

    // Filter on the localized name too.
    let de_y = handle_mailbox_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": { "name": "Y" }, "language": ["de"] }),
        &using(),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(de_y["ids"], json!([b]));
}

/// §4.2.3: a /queryChanges whose `language` differs from the /query that
/// produced `sinceQueryState` is cannotCalculateChanges; the same language
/// works and echoes the state.
#[tokio::test]
async fn query_changes_language_mismatch() {
    let backend = backend();
    three(&backend).await;
    let de = handle_mailbox_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "language": ["de"] }),
        &using(),
    )
    .await
    .unwrap()
    .primary;
    let state = de["queryState"].clone();
    let qc = |lang: Value| {
        let backend = &backend;
        let state = state.clone();
        async move {
            handle_mailbox_query_changes(
                backend,
                &(),
                json!({ "accountId": ACCOUNT_ID, "sinceQueryState": state, "language": lang }),
                &using(),
            )
            .await
        }
    };
    let err = qc(json!(["fr"])).await.unwrap_err();
    assert_eq!(error_type(err), "cannotCalculateChanges");
    let err = qc(Value::Null).await.unwrap_err();
    assert_eq!(error_type(err), "cannotCalculateChanges");
    let ok = qc(json!(["de"])).await.unwrap().primary;
    assert_eq!(ok["oldQueryState"], state);
}

/// §5.1: "The rule that sibling Mailboxes have distinct names applies to
/// base names only, so two siblings can share a localized name."
#[tokio::test]
async fn siblings_may_share_a_localized_name() {
    let backend = backend();
    create(
        &backend,
        json!({ "name": "Work", "localizations": { "de": { "name": "Arbeit" } } }),
    )
    .await;
    create(
        &backend,
        json!({ "name": "Job", "localizations": { "de": { "name": "Arbeit" } } }),
    )
    .await;
}

/// §4.1: a client without the capability renames a Mailbox; the stale
/// translation is pruned in the same update. A client with it that also
/// sets the German name keeps it, and gets `localizations` echoed in
/// `updated`.
#[tokio::test]
async fn rename_prunes_stale_translations() {
    let backend = backend();
    let [a, b, _] = three(&backend).await;

    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { a.clone(): { "name": "A2" } } }),
        &using_without_i18n(),
    )
    .await;
    // The client does not use the capability: nothing i18n in the echo.
    assert!(resp["updated"][&a].get("localizations").is_none(), "{resp}");
    let got = get(&backend, json!({ "ids": [a] }), &using())
        .await
        .unwrap();
    assert_eq!(got["list"][0]["localizations"], Value::Null);

    let resp = set(
        &backend,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { b.clone(): { "name": "B2", "localizations/de/name": "Y2" } }
        }),
        &using(),
    )
    .await;
    assert!(resp["updated"].get(&b).is_some(), "{resp}");
    let got = get(&backend, json!({ "ids": [b] }), &using())
        .await
        .unwrap();
    assert_eq!(
        got["list"][0]["localizations"],
        json!({ "de": { "name": "Y2" } })
    );
}

/// §4.1: an echo of the pruned `localizations` goes to a client that uses
/// the capability.
#[tokio::test]
async fn pruning_echoes_localizations_to_i18n_clients() {
    let backend = backend();
    let [a, ..] = three(&backend).await;
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { a.clone(): { "name": "A2" } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"][&a]["localizations"], Value::Null, "{resp}");
    assert!(resp["updated"][&a].get("locale").is_some(), "{resp}");
}

/// §4.1 restrictions on create, and the patched-object check on update:
/// `locale` changed to an existing key.
#[tokio::test]
async fn invalid_localizations_are_rejected() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "create": {
            "c": { "name": "x", "localizations": { "de": { "sortOrder": 1 } } }
        } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["c"]["properties"],
        json!(["localizations"])
    );

    let [a, ..] = three(&backend).await;
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { a.clone(): { "locale": "de" } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&a]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][&a]["properties"],
        json!(["localizations"])
    );
}
