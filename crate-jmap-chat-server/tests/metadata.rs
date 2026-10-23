//! draft-ietf-jmap-metadata-04 on Chat, Message and Space in the
//! in-memory backend opted in with `MemoryBackend::with_metadata_info`
//! (bd JMAP-s4kp.2). draft-atwood-jmap-chat-00 (lines 667, 780, 911):
//! "Implementations MAY include `Chat` [`Message`, `Space`] in that map".
//!
//! Oracle: the -04 text quoted on each test
//! (/home/mark/PROJECT/IETF/specs/draft-ietf-jmap-metadata-04.txt, the
//! revision of October 2026), with hand-written requests and expected
//! responses. The capability below allows vendor namespaces only, no
//! private metadata, and `maxDepth` 2.

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, MemoryBackend, seed_group_chat};
use jmap_chat_server::message::{handle_message_query, handle_message_set};
use jmap_chat_server::{
    handle_chat_changes, handle_chat_get, handle_chat_query, handle_chat_set, handle_space_get,
    handle_space_query, handle_space_set,
};
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_server::HandlerOutput;
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const NS: &str = "acme.example.com";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:chat".to_owned(),
        "urn:ietf:params:jmap:metadata".to_owned(),
    ]
}

fn using_without_metadata() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:chat".to_owned(),
    ]
}

fn info() -> DataTypeMetadataInfo {
    serde_json::from_value(json!({
        "namespaces": [],
        "supportsVendorNamespaces": true,
        "supportsPrivate": false,
        "maxDepth": 2
    }))
    .unwrap()
}

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new()
        .with_metadata_info("Chat", info())
        .with_metadata_info("Message", info())
        .with_metadata_info("Space", info());
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

fn error_type(err: &JmapError) -> Value {
    serde_json::to_value(err).unwrap()["type"].clone()
}

fn with_account(mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT_ID);
    args
}

async fn chat_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let HandlerOutput { primary, .. } =
        handle_chat_set(backend, &(), &BTreeMap::new(), with_account(args), using)
            .await
            .expect("Chat/set must not fail at method level");
    primary
}

async fn create_chat(backend: &MemoryBackend, extra: Value) -> String {
    let mut create = json!({ "kind": "group", "name": "G" });
    if let (Some(c), Some(e)) = (create.as_object_mut(), extra.as_object()) {
        c.extend(e.clone());
    }
    let resp = chat_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Chat/set create failed: {resp}"))
        .to_owned()
}

async fn update_chat(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let resp = chat_set(backend, json!({ "update": { id: patch } }), &using()).await;
    assert!(resp["updated"].get(id).is_some(), "{resp}");
    resp
}

async fn chat_metadata(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary, .. } =
        handle_chat_get(backend, &(), with_account(json!({ "ids": [id] })))
            .await
            .expect("Chat/get");
    primary["list"][0]["metadata"].clone()
}

async fn chat_query(backend: &MemoryBackend, filter: Value) -> Result<Value, JmapError> {
    handle_chat_query(backend, &(), with_account(json!({ "filter": filter })))
        .await
        .map(|o| o.primary["ids"].clone())
}

async fn chat_changes(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    handle_chat_changes(backend, &(), with_account(args), using)
        .await
        .expect("Chat/changes")
        .primary
}

async fn message_set(backend: &MemoryBackend, args: Value) -> Value {
    handle_message_set(backend, &(), &BTreeMap::new(), with_account(args), &using())
        .await
        .expect("Message/set must not fail at method level")
        .primary
}

async fn space_set(backend: &MemoryBackend, args: Value) -> Value {
    handle_space_set(backend, &(), with_account(args), &using())
        .await
        .expect("Space/set must not fail at method level")
        .primary
}

// ---------------------------------------------------------------------------
// /query (-04 §3.6)
// ---------------------------------------------------------------------------

/// -04 §3.6: `metadataExists` "a value MUST be present at this path",
/// `metadataTextContains` matches a substring under `i;unicode-casemap`,
/// `metadataTextEquals` matches exactly; an object without `metadata`
/// has `{}` (-04 §2 "default: {}").
#[tokio::test]
async fn chat_query_metadata_conditions() {
    let backend = backend();
    let tagged = create_chat(
        &backend,
        json!({ "metadata": { NS: { "summary": "Vendor Invoices" } } }),
    )
    .await;
    let plain = create_chat(&backend, json!({})).await;

    assert_eq!(
        chat_query(&backend, json!({ "metadataExists": NS }))
            .await
            .unwrap(),
        json!([tagged])
    );
    assert_eq!(
        chat_query(
            &backend,
            json!({ "metadataTextContains": { "path": "acme.example.com/summary", "value": "invoice" } })
        )
        .await
        .unwrap(),
        json!([tagged])
    );
    assert_eq!(
        chat_query(
            &backend,
            json!({ "operator": "NOT", "conditions": [
                { "metadataTextEquals": { "path": "acme.example.com/summary", "value": "Vendor Invoices" } }
            ] })
        )
        .await
        .unwrap(),
        json!([plain])
    );
}

/// -04 §3.6: "Any privateMetadata* condition ... MUST be rejected with an
/// "unsupportedFilter" error if supportsPrivate is false", and "A path
/// that is not well formed ... MUST be rejected with an
/// "invalidArguments" error" (`~2` is not an RFC 6901 escape). -04
/// §1.2.1: an unlisted type "does not gain the metadata or
/// privateMetadata properties", so its metadata filter is
/// `unsupportedFilter` (RFC 8620 §5.5).
#[tokio::test]
async fn invalid_metadata_conditions_are_rejected() {
    let backend = backend();
    let err = chat_query(&backend, json!({ "privateMetadataExists": NS }))
        .await
        .unwrap_err();
    assert_eq!(error_type(&err), json!("unsupportedFilter"));
    let err = chat_query(
        &backend,
        json!({ "metadataExists": "acme.example.com/a~2" }),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(&err), json!("invalidArguments"));

    let unlisted = MemoryBackend::new();
    unlisted.register_account(&Id::from(ACCOUNT_ID));
    let err = chat_query(&unlisted, json!({ "metadataExists": NS }))
        .await
        .unwrap_err();
    assert_eq!(error_type(&err), json!("unsupportedFilter"));
}

// ---------------------------------------------------------------------------
// /set (-04 §3, §3.2)
// ---------------------------------------------------------------------------

/// -04 §2 `metadata` "default: {}", so a `metadata/<namespace>` patch
/// finds its parent; "Setting a patch value to null removes the targeted
/// key" keeps the other namespaces; and -04 §3.2 "In an update, null is
/// equivalent to {}".
#[tokio::test]
async fn chat_update_patches_metadata() {
    let backend = backend();
    let id = create_chat(&backend, json!({})).await;
    update_chat(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 }, "metadata/other.example.com": {} }),
    )
    .await;
    update_chat(&backend, &id, json!({ "metadata/other.example.com": null })).await;
    assert_eq!(
        chat_metadata(&backend, &id).await,
        json!({ NS: { "k": 1 } })
    );
    update_chat(&backend, &id, json!({ "metadata": null })).await;
    assert_eq!(chat_metadata(&backend, &id).await, json!({}));
}

/// - -04 §3: without the capability, "Foo/set MUST reject any write to
///   them with an "invalidProperties" SetError".
/// - -04 §3.2: on create, "A value of null for either property MUST be
///   rejected with an "invalidProperties" SetError".
/// - rule 2: "If the path begins with privateMetadata and supportsPrivate
///   is false ..., reject with "invalidProperties"."
/// - rule 3: "If the targeted namespace is not supported on this data
///   type, reject with "invalidProperties"" (`photography` is a
///   registered name this capability does not list).
/// - rule 4: "If a namespace value would exceed maxDepth, reject with
///   "invalidProperties"" (the value below has depth 3; maxDepth is 2).
#[tokio::test]
async fn invalid_metadata_writes_are_invalid_properties() {
    let backend = backend();
    for (create, using) in [
        (
            json!({ "kind": "group", "name": "G", "metadata": { NS: {} } }),
            using_without_metadata(),
        ),
        (
            json!({ "kind": "group", "name": "G", "metadata": null }),
            using(),
        ),
    ] {
        let resp = chat_set(&backend, json!({ "create": { "c": create } }), &using).await;
        assert_eq!(
            resp["notCreated"]["c"]["type"],
            json!("invalidProperties"),
            "{resp}"
        );
        assert_eq!(
            resp["notCreated"]["c"]["properties"],
            json!(["metadata"]),
            "{resp}"
        );
    }

    let id = create_chat(&backend, json!({})).await;
    let cases: [(Value, Vec<String>, &str); 4] = [
        (
            json!({ "metadata/acme.example.com": { "k": 1 } }),
            using_without_metadata(),
            "metadata/acme.example.com",
        ),
        (
            json!({ "privateMetadata/acme.example.com": { "k": 1 } }),
            using(),
            "privateMetadata/acme.example.com",
        ),
        (
            json!({ "metadata/photography": { "k": 1 } }),
            using(),
            "metadata/photography",
        ),
        (
            json!({ "metadata/acme.example.com": { "a": { "b": { "c": 1 } } } }),
            using(),
            "metadata/acme.example.com",
        ),
    ];
    for (patch, using, property) in cases {
        let resp = chat_set(&backend, json!({ "update": { &id: patch } }), &using).await;
        assert_eq!(
            resp["notUpdated"][&id]["type"],
            json!("invalidProperties"),
            "{patch}: {resp}"
        );
        assert_eq!(
            resp["notUpdated"][&id]["properties"],
            json!([property]),
            "{patch}"
        );
    }
    assert_eq!(chat_metadata(&backend, &id).await, Value::Null);
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account".
#[tokio::test]
async fn unlisted_type_rejects_metadata_write() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let resp = chat_set(
        &backend,
        json!({ "create": { "c": { "kind": "group", "name": "G", "metadata": { NS: {} } } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["c"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));
}

/// -04 §3.2: "Patch errors that can be determined from the request alone
/// (for example, two paths where one is a prefix of the other) MUST be
/// reported with "invalidPatch" before rule 1 is evaluated", and rule 1
/// "If the user cannot read the related object, reject with
/// "notFound"" comes before rules 2 to 4.
#[tokio::test]
async fn request_only_patch_error_precedes_not_found() {
    let backend = backend();
    let resp = chat_set(
        &backend,
        json!({ "update": { "nope": {
            "metadata": { NS: {} },
            "metadata/acme.example.com/k": 1
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["nope"]["type"],
        json!("invalidPatch"),
        "{resp}"
    );

    let resp = chat_set(
        &backend,
        json!({ "update": { "nope": { "metadata/photography": {} } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["nope"]["type"],
        json!("notFound"),
        "{resp}"
    );
}

// ---------------------------------------------------------------------------
// /changes (-04 §3, §3.3)
// ---------------------------------------------------------------------------

/// -04 §3.3: `updatedProperties` "MUST be set to a list containing only
/// those property names (e.g., ["metadata"] ...)" when only metadata
/// changed; with `ignoreMetadataOnlyChanges` the server "MUST exclude from
/// the updated array any id whose only changes since sinceState ... are
/// confined to metadata", while "The state string still advances
/// normally"; and -04 §3: without the capability "Foo/changes MUST behave
/// as if ignoreMetadataOnlyChanges were true".
#[tokio::test]
async fn metadata_only_chat_change() {
    let backend = backend();
    let id = create_chat(&backend, json!({})).await;
    let resp = update_chat(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    let new_state = resp["newState"].clone();

    let report = chat_changes(&backend, json!({ "sinceState": since }), &using()).await;
    assert_eq!(report["updated"], json!([id]), "{report}");
    assert_eq!(report["updatedProperties"], json!(["metadata"]));
    assert_eq!(report["newState"], new_state);

    for (args, using) in [
        (
            json!({ "sinceState": since, "ignoreMetadataOnlyChanges": true }),
            using(),
        ),
        (json!({ "sinceState": since }), using_without_metadata()),
    ] {
        let ignored = chat_changes(&backend, args, &using).await;
        assert_eq!(ignored["updated"], json!([]), "{ignored}");
        assert_eq!(ignored["updatedProperties"], Value::Null);
        assert_eq!(ignored["newState"], new_state);
    }
}

/// -04 §3.3: "If any other property ... of any id in the array may also
/// have changed ... this argument MUST be null", and "Ids with any other
/// change remain in updated".
#[tokio::test]
async fn mixed_change_is_reported_without_updated_properties() {
    let backend = backend();
    let id = create_chat(&backend, json!({})).await;
    let resp = update_chat(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_chat(&backend, &id, json!({ "muted": true })).await;

    for ignore in [false, true] {
        let resp = chat_changes(
            &backend,
            json!({ "sinceState": since, "ignoreMetadataOnlyChanges": ignore }),
            &using(),
        )
        .await;
        assert_eq!(resp["updated"], json!([id]), "{resp}");
        assert_eq!(resp["updatedProperties"], Value::Null, "{resp}");
    }
}

/// -04 §3.3: "Excluded ids do not count toward maxChanges." Two changes
/// fall in the window, one metadata-only; with `maxChanges` 1 the
/// request succeeds only if the excluded one is not counted.
#[tokio::test]
async fn excluded_ids_do_not_count_toward_max_changes() {
    let backend = backend();
    let quiet = create_chat(&backend, json!({})).await;
    let loud = create_chat(&backend, json!({})).await;
    let resp = update_chat(
        &backend,
        &quiet,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_chat(&backend, &loud, json!({ "muted": true })).await;

    let resp = chat_changes(
        &backend,
        json!({ "sinceState": since, "maxChanges": 1, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!([loud]), "{resp}");
}

// ---------------------------------------------------------------------------
// Message and Space
// ---------------------------------------------------------------------------

/// draft-atwood-jmap-chat-00 §Message/set lists the only client updates;
/// -04 §3.2 adds `metadata` written by patch path, and §3.6 the
/// conditions, here inside a §Message/query `chatId` filter.
#[tokio::test]
async fn message_metadata_update_and_query() {
    let backend = backend();
    seed_group_chat(&backend, "g1");
    let resp = message_set(
        &backend,
        json!({ "create": {
            "m1": { "chatId": "g1", "body": "a", "bodyType": "text/plain", "sentAt": "2026-01-01T00:00:00Z" },
            "m2": { "chatId": "g1", "body": "b", "bodyType": "text/plain", "sentAt": "2026-01-01T00:00:01Z" }
        } }),
    )
    .await;
    let m1 = resp["created"]["m1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{resp}"))
        .to_owned();

    let resp = message_set(
        &backend,
        json!({ "update": { &m1: { "metadata/acme.example.com": { "moderationNote": "ok" } } } }),
    )
    .await;
    assert!(resp["updated"].get(&m1).is_some(), "{resp}");

    let ids = handle_message_query(
        &backend,
        &(),
        with_account(json!({ "filter": { "chatId": "g1", "metadataExists": "acme.example.com/moderationNote" } })),
    )
    .await
    .expect("Message/query")
    .primary["ids"]
        .clone();
    assert_eq!(ids, json!([m1]));
}

/// -04 §3.2 on Space: a create stores the `metadata` it carries, and an
/// update is an RFC 8620 §5.3 PatchObject ("metadata/<namespace>/..."
/// replaces only the targeted key), alongside a top-level field.
#[tokio::test]
async fn space_metadata_create_update_and_query() {
    let backend = backend();
    let resp = space_set(
        &backend,
        json!({ "create": { "s": { "name": "S", "metadata": { NS: { "welcomeMessage": "Hello", "guidelines": "Be kind" } } } } }),
    )
    .await;
    let id = resp["created"]["s"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{resp}"))
        .to_owned();

    let resp = space_set(
        &backend,
        json!({ "update": { &id: { "metadata/acme.example.com/welcomeMessage": "Hi", "description": "d" } } }),
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");

    let got = handle_space_get(&backend, &(), with_account(json!({ "ids": [&id] })))
        .await
        .expect("Space/get")
        .primary;
    assert_eq!(
        got["list"][0]["metadata"],
        json!({ NS: { "welcomeMessage": "Hi", "guidelines": "Be kind" } })
    );
    assert_eq!(got["list"][0]["description"], json!("d"));

    let ids = handle_space_query(
        &backend,
        &(),
        with_account(json!({ "filter": { "metadataTextEquals": { "path": "acme.example.com/welcomeMessage", "value": "Hi" } } })),
    )
    .await
    .expect("Space/query")
    .primary["ids"]
        .clone();
    assert_eq!(ids, json!([id]));
}

/// -04 §2: privateMetadata "MUST" be filtered per user; this backend has
/// no viewer identity, so it refuses `supportsPrivate: true`, and it
/// supports only the types draft-atwood-jmap-chat-00 names.
#[test]
#[should_panic(expected = "supportsPrivate")]
fn private_metadata_capability_is_refused() {
    let mut private = info();
    private.supports_private = true;
    let _ = MemoryBackend::new().with_metadata_info("Chat", private);
}

#[test]
#[should_panic(expected = "ChatContact")]
fn unnamed_type_is_refused() {
    let _ = MemoryBackend::new().with_metadata_info("ChatContact", info());
}
