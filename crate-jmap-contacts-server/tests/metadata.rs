//! draft-ietf-jmap-metadata-04 on AddressBook and ContactCard in the
//! in-memory backend opted in with `MemoryBackend::with_metadata_info`
//! (bd JMAP-s4kp.5).
//!
//! Oracle: the -04 text quoted on each test
//! (/home/mark/PROJECT/IETF/specs/draft-ietf-jmap-metadata-04.txt, the
//! revision of October 2026), with hand-written requests and expected
//! responses. The capability below allows vendor namespaces only, no
//! private metadata, and `maxDepth` 2.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::MemoryBackend;
use jmap_contacts_server::{
    ContactsBackend, handle_address_book_changes, handle_address_book_get, handle_address_book_set,
    handle_contact_card_changes, handle_contact_card_copy, handle_contact_card_get,
    handle_contact_card_query, handle_contact_card_query_changes, handle_contact_card_set,
    register_contacts_handlers,
};
use jmap_contacts_types::AddressBook;
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_server::{Dispatcher, HandlerOutput};
use jmap_types::{Id, JmapError, JmapRequest, State};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "a1";
const NS: &str = "acme.example.com";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:contacts".to_owned(),
        "urn:ietf:params:jmap:metadata".to_owned(),
    ]
}

fn using_without_metadata() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:contacts".to_owned(),
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
    MemoryBackend::new()
        .with_account(&Id::from(ACCOUNT_ID))
        .with_metadata_info("AddressBook", info())
        .with_metadata_info("ContactCard", info())
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

/// Store an AddressBook whose `myRights.maySetMetadata` is
/// `may_set_metadata` (server-set, so not writable through
/// AddressBook/set) and return its id.
async fn book_fixture(backend: &MemoryBackend, name: &str, may_set_metadata: bool) -> String {
    let name = name.to_owned();
    let (id, _) = ContactsBackend::create_object::<AddressBook, _>(
        backend,
        &(),
        &Id::from(ACCOUNT_ID),
        "fixture",
        move |id| {
            serde_json::from_value(json!({
                "id": id,
                "name": name,
                "isDefault": false,
                "isSubscribed": true,
                "myRights": {
                    "mayRead": true, "mayWrite": true, "mayShare": true,
                    "mayDelete": true,
                    "maySetMetadata": may_set_metadata
                }
            }))
            .unwrap()
        },
    )
    .await
    .unwrap();
    id.to_string()
}

async fn book_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } = handle_address_book_set(backend, &(), args, using)
        .await
        .expect("AddressBook/set must not fail at method level");
    primary
}

async fn card_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } =
        handle_contact_card_set(backend, &(), &BTreeMap::new(), args, using)
            .await
            .expect("ContactCard/set must not fail at method level");
    primary
}

async fn create_book(backend: &MemoryBackend, create: Value) -> String {
    let resp = book_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("AddressBook/set create failed: {resp}"))
        .to_owned()
}

/// Apply an AddressBook/set update that must succeed; returns the response.
async fn update_book(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let resp = book_set(backend, json!({ "update": { id: patch } }), &using()).await;
    assert_eq!(resp["updated"], json!({ id: null }), "{resp}");
    resp
}

/// Create a ContactCard in the AddressBooks `books` carrying `metadata`.
async fn create_card(backend: &MemoryBackend, books: &[&str], metadata: Value) -> String {
    let book_ids: serde_json::Map<String, Value> = books
        .iter()
        .map(|b| ((*b).to_owned(), json!(true)))
        .collect();
    let create = json!({ "addressBookIds": book_ids, "metadata": metadata });
    let resp = card_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("ContactCard/set create failed: {resp}"))
        .to_owned()
}

async fn book_property(backend: &MemoryBackend, id: &str, property: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id] });
    let HandlerOutput { primary, .. } = handle_address_book_get(backend, &(), args, &using())
        .await
        .unwrap();
    primary["list"][0][property].clone()
}

async fn card_property(backend: &MemoryBackend, id: &str, property: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id] });
    let HandlerOutput { primary, .. } = handle_contact_card_get(backend, &(), args, &using())
        .await
        .unwrap();
    primary["list"][0][property].clone()
}

async fn card_query(backend: &MemoryBackend, filter: Value) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "filter": filter });
    handle_contact_card_query(backend, &(), args, &using())
        .await
        .map(|o| o.primary["ids"].clone())
}

async fn book_changes(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_address_book_changes(backend, &(), args, using)
        .await
        .expect("AddressBook/changes must succeed")
        .primary
}

fn sorted(mut ids: Vec<&str>) -> Value {
    ids.sort_unstable();
    json!(ids)
}

fn id_list(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect()
}

// ---------------------------------------------------------------------------
// /query (-04 §3.6)
// ---------------------------------------------------------------------------

/// -04 §3.6 `metadataExists`: "Matches an object if and only if a value
/// is present at the given path within the object's metadata property. A
/// namespace-only path matches if the namespace key is present and its
/// value is not the empty object {}."
#[tokio::test]
async fn card_query_metadata_exists() {
    let backend = backend();
    let book = book_fixture(&backend, "Work", true).await;
    let tagged = create_card(&backend, &[&book], json!({ NS: { "tier": "gold" } })).await;
    create_card(&backend, &[&book], json!({ NS: {} })).await;
    create_card(&backend, &[&book], json!({})).await;

    for path in [NS, "acme.example.com/tier"] {
        assert_eq!(
            card_query(&backend, json!({ "metadataExists": path }))
                .await
                .unwrap(),
            json!([tagged]),
            "{path}"
        );
    }
}

/// -04 §3.6 conditions extend the type's own FilterCondition, so they sit
/// in one leaf with RFC 9610 §3.3.1 `inAddressBook` and both must hold;
/// `metadataTextContains` is a substring match "under the
/// "i;unicode-casemap" collation", so case is ignored.
#[tokio::test]
async fn card_query_combines_in_address_book_and_text_contains() {
    let backend = backend();
    let work = book_fixture(&backend, "Work", true).await;
    let home = book_fixture(&backend, "Home", true).await;
    let hit = create_card(
        &backend,
        &[&work],
        json!({ NS: { "memo": "Please FOLLOW UP soon" } }),
    )
    .await;
    create_card(&backend, &[&work], json!({ NS: { "memo": "done" } })).await;
    create_card(&backend, &[&home], json!({ NS: { "memo": "follow up" } })).await;

    let filter = json!({
        "inAddressBook": work,
        "metadataTextContains": { "path": "acme.example.com/memo", "value": "follow up" }
    });
    assert_eq!(card_query(&backend, filter).await.unwrap(), json!([hit]));
}

/// -04 §3.6: "These conditions compose with the data type's existing
/// filter conditions through the usual AND/OR/NOT operators (Section 5.5
/// of [RFC8620])", and `metadataTextEquals` is "exactly equal to the
/// supplied value, comparing Unicode code points without case folding".
#[tokio::test]
async fn card_query_text_equals_in_operator_tree() {
    let backend = backend();
    let book = book_fixture(&backend, "Work", true).await;
    let exact = create_card(&backend, &[&book], json!({ NS: { "tag": "Urgent" } })).await;
    let lower = create_card(&backend, &[&book], json!({ NS: { "tag": "urgent" } })).await;

    let equals =
        json!({ "metadataTextEquals": { "path": "acme.example.com/tag", "value": "Urgent" } });
    assert_eq!(
        card_query(&backend, equals.clone()).await.unwrap(),
        json!([exact])
    );
    let not = json!({ "operator": "NOT", "conditions": [equals] });
    assert_eq!(card_query(&backend, not).await.unwrap(), json!([lower]));
}

/// -04 §3.6: "Any privateMetadata* condition (existence or text) MUST be
/// rejected with an "unsupportedFilter" error if supportsPrivate is false
/// for the data type", and "A path that is not well formed (for example,
/// one with an invalid ~ escape) MUST be rejected with an
/// "invalidArguments" error" (RFC 6901 §3 allows only `~0` and `~1`),
/// also inside an operator tree.
#[tokio::test]
async fn invalid_conditions_are_rejected() {
    let backend = backend();
    let err = card_query(&backend, json!({ "privateMetadataExists": NS }))
        .await
        .unwrap_err();
    assert_eq!(error_type(err), json!("unsupportedFilter"));

    let err = card_query(
        &backend,
        json!({ "operator": "OR", "conditions": [{ "metadataExists": "acme.example.com/a~2" }] }),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), json!("invalidArguments"));
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account", so a
/// backend that did not opt ContactCard in cannot process the condition
/// (RFC 8620 §5.5 `unsupportedFilter`).
#[tokio::test]
async fn unlisted_type_rejects_metadata_condition() {
    let backend = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    let err = card_query(&backend, json!({ "metadataExists": NS }))
        .await
        .unwrap_err();
    assert_eq!(error_type(err), json!("unsupportedFilter"));
}

// ---------------------------------------------------------------------------
// /set (-04 §3, §3.1, §3.2, §4.1)
// ---------------------------------------------------------------------------

/// -04 §3.2 and the §6.3 example: a patch path below a namespace changes
/// that key and keeps the others; the create stores its metadata. -04
/// §3.2: "In an update, null is equivalent to {}".
#[tokio::test]
async fn book_patch_within_namespace_then_null() {
    let backend = backend();
    let id = create_book(
        &backend,
        json!({
            "name": "b", "isSubscribed": true,
            "metadata": { NS: { "color": "red", "icon": "star" } }
        }),
    )
    .await;
    update_book(
        &backend,
        &id,
        json!({ "metadata/acme.example.com/color": "blue" }),
    )
    .await;
    assert_eq!(
        book_property(&backend, &id, "metadata").await,
        json!({ NS: { "color": "blue", "icon": "star" } })
    );
    update_book(&backend, &id, json!({ "metadata": null })).await;
    assert_eq!(book_property(&backend, &id, "metadata").await, json!({}));
}

/// After the -04 §6.6 example: a primary property and two vendor metadata
/// keys change in one /set call, answered with `"updated": { id: null }`.
/// -04 §3.2 patch paths follow RFC 8620 §5.3, so "the parent of the target
/// MUST exist", and a missing one is `invalidPatch` that leaves the card
/// unchanged ("either all of these changes apply or none do").
#[tokio::test]
async fn card_update_primary_and_metadata_atomically() {
    let backend = backend();
    let book = book_fixture(&backend, "Work", true).await;
    let bare = create_card(&backend, &[&book], json!({})).await;
    let resp = card_set(
        &backend,
        json!({ "update": { &bare: {
            "kind": "org",
            "metadata/acme.example.com/approvalStatus": "pending"
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&bare]["type"],
        json!("invalidPatch"),
        "{resp}"
    );
    assert_eq!(card_property(&backend, &bare, "kind").await, Value::Null);

    let id = create_card(&backend, &[&book], json!({ NS: {} })).await;
    let resp = card_set(
        &backend,
        json!({ "update": { &id: {
            "kind": "org",
            "metadata/acme.example.com/lastModifiedReason": "Rescheduled per manager request",
            "metadata/acme.example.com/approvalStatus": "pending"
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &id: null }), "{resp}");
    assert_eq!(
        card_property(&backend, &id, "metadata").await,
        json!({ NS: {
            "lastModifiedReason": "Rescheduled per manager request",
            "approvalStatus": "pending"
        } })
    );
    assert_eq!(card_property(&backend, &id, "kind").await, json!("org"));
}

/// -04 §3.1: "Changes to metadata or privateMetadata alone MUST NOT update
/// server-maintained modification timestamps of the related object".
/// A ContactCard's `updated` (RFC 9553 §2.1.10) is such a timestamp.
#[tokio::test]
async fn metadata_only_update_keeps_updated_timestamp() {
    let backend = backend();
    let book = book_fixture(&backend, "Work", true).await;
    let create = json!({
        "addressBookIds": { &book: true },
        "updated": "2024-01-01T00:00:00Z",
    });
    let resp = card_set(&backend, json!({ "create": { "c": create } }), &using()).await;
    let id = resp["created"]["c"]["id"].as_str().unwrap().to_owned();
    let resp = card_set(
        &backend,
        json!({ "update": { &id: { "metadata/acme.example.com": { "k": 1 } } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &id: null }), "{resp}");
    assert_eq!(
        card_property(&backend, &id, "updated").await,
        json!("2024-01-01T00:00:00Z")
    );
}

/// The set of /set rejections that answer `invalidProperties`, each with
/// the offending request key:
///
/// - -04 §3.2: "A value of null for either property MUST be rejected with
///   an "invalidProperties" SetError" (create).
/// - -04 §3: for a request without the capability, "Foo/set MUST reject
///   any write to them with an "invalidProperties" SetError".
/// - -04 §3.2 rule 2: "If the path begins with privateMetadata and
///   supportsPrivate is false for this data type, reject with
///   "invalidProperties"."
/// - rule 3: "If the targeted namespace is not supported on this data
///   type, reject with "invalidProperties"" (`photography` is a registered
///   name this capability does not list).
/// - rule 4: "If a namespace value would exceed maxDepth, reject with
///   "invalidProperties"" (§2.1: the value below has depth 3; maxDepth is
///   2).
#[tokio::test]
async fn invalid_metadata_writes_are_invalid_properties() {
    let backend = backend();
    let book = book_fixture(&backend, "Work", true).await;
    let resp = card_set(
        &backend,
        json!({ "create": { "c": {
            "addressBookIds": { &book: true }, "metadata": null
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));

    let id = create_card(&backend, &[&book], json!({})).await;
    let cases: [(Value, &[String], &str); 4] = [
        (
            json!({ "metadata/acme.example.com": { "k": 1 } }),
            &using_without_metadata(),
            "metadata/acme.example.com",
        ),
        (
            json!({ "privateMetadata/acme.example.com": { "k": 1 } }),
            &using(),
            "privateMetadata/acme.example.com",
        ),
        (
            json!({ "metadata/photography": { "k": 1 } }),
            &using(),
            "metadata/photography",
        ),
        (
            json!({ "metadata/acme.example.com": { "a": { "b": { "c": 1 } } } }),
            &using(),
            "metadata/acme.example.com",
        ),
    ];
    for (patch, using, property) in cases {
        let resp = card_set(&backend, json!({ "update": { &id: patch } }), using).await;
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
    assert_eq!(card_property(&backend, &id, "metadata").await, json!({}));
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account", so a
/// backend that did not opt in rejects the write.
#[tokio::test]
async fn unlisted_type_rejects_metadata_write() {
    let backend = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    let resp = book_set(
        &backend,
        json!({ "create": { "c": {
            "name": "b", "isSubscribed": true, "metadata": { NS: {} }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));
}

/// -04 §4.1: "A write to metadata without maySetMetadata MUST be rejected
/// with a "forbidden" SetError." An AddressBook carries its own right;
/// other properties stay writable.
#[tokio::test]
async fn book_without_may_set_metadata_is_forbidden() {
    let backend = backend();
    let id = book_fixture(&backend, "locked", false).await;
    let resp = book_set(
        &backend,
        json!({ "update": { &id: { "metadata/acme.example.com": { "k": 1 } } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&id]["type"],
        json!("forbidden"),
        "{resp}"
    );

    update_book(&backend, &id, json!({ "name": "renamed" })).await;
}

/// -04 §4.1: a type such as ContactCard "takes the right from its
/// containers. Modifying its metadata requires maySetMetadata on every
/// container it belongs to; when the same update changes its containers,
/// on every container both before and after the change; and on create,
/// on the containers given in the create." A ContactCard's containers are
/// the AddressBooks of its `addressBookIds` (RFC 9610 §3).
#[tokio::test]
async fn card_metadata_needs_right_on_every_address_book() {
    let backend = backend();
    let open = book_fixture(&backend, "open", true).await;
    let locked = book_fixture(&backend, "locked", false).await;
    let meta = json!({ "metadata/acme.example.com": { "k": 1 } });
    let forbidden = |resp: &Value, id: &str| {
        assert_eq!(resp["notUpdated"][id]["type"], json!("forbidden"), "{resp}");
    };

    // In both books: the right is needed on every container.
    let both = create_card(&backend, &[&open], json!({})).await;
    let resp = card_set(
        &backend,
        json!({ "update": { &both: { format!("addressBookIds/{locked}"): true } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &both: null }), "{resp}");
    let resp = card_set(&backend, json!({ "update": { &both: meta } }), &using()).await;
    forbidden(&resp, &both);

    // Adding the locked book while writing metadata ("after the change").
    let card = create_card(&backend, &[&open], json!({})).await;
    let mut patch = meta.clone();
    patch[format!("addressBookIds/{locked}")] = json!(true);
    let resp = card_set(&backend, json!({ "update": { &card: patch } }), &using()).await;
    forbidden(&resp, &card);

    // Replacing locked with open while writing metadata ("before").
    let mut patch = meta.clone();
    patch["addressBookIds"] = json!({ &open: true });
    let resp = card_set(&backend, json!({ "update": { &both: patch } }), &using()).await;
    forbidden(&resp, &both);

    // The same container change without a metadata write is allowed.
    let resp = card_set(
        &backend,
        json!({ "update": { &both: { "addressBookIds": { &open: true } } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &both: null }), "{resp}");

    // On create.
    let create = json!({ "addressBookIds": { &locked: true }, "metadata": { NS: { "k": 1 } } });
    let resp = card_set(&backend, json!({ "create": { "c": create } }), &using()).await;
    assert_eq!(
        resp["notCreated"]["c"]["type"],
        json!("forbidden"),
        "{resp}"
    );
}

/// -04 §3.2 rule 1: "If the user cannot read the related object, reject
/// with "notFound". If the write targets metadata and the user lacks
/// maySetMetadata (Section 4.1), reject with "forbidden"" — in that order.
/// And: "Patch errors that can be determined from the request alone (for
/// example, two paths where one is a prefix of the other) MUST be
/// reported with "invalidPatch" before rule 1 is evaluated."
#[tokio::test]
async fn rule_order_invalid_patch_then_not_found_then_forbidden() {
    let backend = backend();
    let resp = book_set(
        &backend,
        json!({ "update": { "nope": { "metadata/acme.example.com": { "k": 1 } } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["nope"]["type"],
        json!("notFound"),
        "{resp}"
    );

    let locked = book_fixture(&backend, "locked", false).await;
    for id in [locked.as_str(), "nope"] {
        let resp = book_set(
            &backend,
            json!({ "update": { id: {
                "metadata": { NS: {} },
                "metadata/acme.example.com/k": 1
            } } }),
            &using(),
        )
        .await;
        assert_eq!(
            resp["notUpdated"][id]["type"],
            json!("invalidPatch"),
            "{resp}"
        );
    }
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
async fn metadata_only_book_change() {
    let backend = backend();
    let id = book_fixture(&backend, "b", true).await;
    let resp = update_book(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    let new_state = resp["newState"].clone();

    let report = book_changes(&backend, json!({ "sinceState": since }), &using()).await;
    assert_eq!(report["updated"], json!([id]));
    assert_eq!(report["updatedProperties"], json!(["metadata"]));
    assert_eq!(report["newState"], new_state);

    for (args, using) in [
        (
            json!({ "sinceState": since, "ignoreMetadataOnlyChanges": true }),
            using(),
        ),
        (json!({ "sinceState": since }), using_without_metadata()),
    ] {
        let ignored = book_changes(&backend, args, &using).await;
        assert_eq!(ignored["updated"], json!([]), "{ignored}");
        assert_eq!(ignored["updatedProperties"], Value::Null);
        assert_eq!(ignored["newState"], new_state);
    }
}

/// -04 §3.3: "If any other property ... of any id in the array may also
/// have changed ... this argument MUST be null", and "Ids with any other
/// change remain in updated" under `ignoreMetadataOnlyChanges`.
#[tokio::test]
async fn mixed_change_is_reported_without_updated_properties() {
    let backend = backend();
    let id = book_fixture(&backend, "b", true).await;
    let resp = update_book(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_book(&backend, &id, json!({ "name": "n" })).await;

    for ignore in [false, true] {
        let resp = book_changes(
            &backend,
            json!({ "sinceState": since, "ignoreMetadataOnlyChanges": ignore }),
            &using(),
        )
        .await;
        assert_eq!(resp["updated"], json!([id]), "{resp}");
        assert_eq!(resp["updatedProperties"], Value::Null, "{resp}");
    }
}

/// -04 §3.3: "Ids with any other change remain in updated." A metadata-only
/// update of an AddressBook that clears `isDefault` on another (RFC 9610
/// §2: `isDefault` "MUST NOT be true for more than one AddressBook within
/// an account") changed that other book's `isDefault`, so it stays in
/// `updated` under `ignoreMetadataOnlyChanges`, while the patched book,
/// whose "only changes since sinceState ... are confined to metadata",
/// is excluded.
#[tokio::test]
async fn demoted_book_is_not_metadata_only() {
    let backend = backend();
    let rights = json!({ "mayRead": true, "mayWrite": true, "mayShare": true, "mayDelete": true });
    for id in ["b1", "b2"] {
        backend.seed_object(
            &Id::from(ACCOUNT_ID),
            "AddressBook",
            &Id::from(id),
            json!({
                "id": id, "name": id, "isDefault": true, "isSubscribed": true,
                "myRights": rights
            }),
        );
    }
    let resp = book_set(
        &backend,
        json!({ "update": { "b1": { "metadata/acme.example.com": { "k": 1 } } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"]["b1"], Value::Null, "{resp}");
    assert_eq!(resp["updated"]["b2"]["isDefault"], json!(false), "{resp}");
    let since = resp["oldState"].clone();

    let resp = book_changes(
        &backend,
        json!({ "sinceState": since, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!(["b2"]), "{resp}");
    assert_eq!(
        book_property(&backend, "b2", "isDefault").await,
        json!(false)
    );
}

/// -04 §3.3: "Excluded ids do not count toward maxChanges." Two changes
/// with `maxChanges` 1 would otherwise be `cannotCalculateChanges`
/// (RFC 8620 §5.2) in this backend.
#[tokio::test]
async fn excluded_ids_do_not_count_toward_max_changes() {
    let backend = backend();
    let quiet = book_fixture(&backend, "quiet", true).await;
    let loud = book_fixture(&backend, "loud", true).await;
    let resp = update_book(
        &backend,
        &quiet,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_book(&backend, &loud, json!({ "name": "louder" })).await;

    let resp = book_changes(
        &backend,
        json!({ "sinceState": since, "maxChanges": 1, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!([loud]), "{resp}");
    assert_eq!(resp["hasMoreChanges"], json!(false), "{resp}");
}

/// -04 §3.3 on ContactCard/changes: two cards whose only change is
/// metadata give `updatedProperties` `["metadata"]`.
#[tokio::test]
async fn metadata_only_card_changes() {
    let backend = backend();
    let book = book_fixture(&backend, "Work", true).await;
    let c1 = create_card(&backend, &[&book], json!({})).await;
    let c2 = create_card(&backend, &[&book], json!({})).await;
    let resp = card_set(
        &backend,
        json!({ "update": {
            &c1: { "metadata/acme.example.com": { "k": 1 } },
            &c2: { "metadata": { NS: { "k": 2 } } }
        } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &c1: null, &c2: null }), "{resp}");
    let since = resp["oldState"].clone();

    let args = json!({ "accountId": ACCOUNT_ID, "sinceState": since });
    let resp = handle_contact_card_changes(&backend, &(), args, &using())
        .await
        .unwrap()
        .primary;
    assert_eq!(sorted(id_list(&resp["updated"])), sorted(vec![&c1, &c2]));
    assert_eq!(resp["updatedProperties"], json!(["metadata"]));
}

// ---------------------------------------------------------------------------
// Dispatcher wiring (-04 §3)
// ---------------------------------------------------------------------------

/// `register_contacts_handlers` forwards the request's `using`: -04 §3
/// "Foo/set MUST reject any write to them" without the capability, and the
/// same write succeeds with it.
#[tokio::test]
async fn dispatcher_forwards_using() {
    let backend = backend();
    let book = book_fixture(&backend, "Work", true).await;
    let card = create_card(&backend, &[&book], json!({ NS: {} })).await;
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_contacts_handlers(&mut dispatcher, Arc::new(backend));

    for (method, id) in [("AddressBook/set", &book), ("ContactCard/set", &card)] {
        let call = json!([[method, {
            "accountId": ACCOUNT_ID,
            "update": { id: { "metadata/acme.example.com": { "k": 1 } } }
        }, "0"]]);
        for (using, expect_ok) in [(using_without_metadata(), false), (using(), true)] {
            let request: JmapRequest =
                serde_json::from_value(json!({ "using": using, "methodCalls": call })).unwrap();
            let resp = dispatcher.dispatch(request, (), State::from("s0")).await;
            let args = &resp.method_responses[0].arguments;
            if expect_ok {
                assert_eq!(args["updated"], json!({ id: null }), "{method}: {args}");
            } else {
                assert_eq!(
                    args["notUpdated"][id]["type"],
                    json!("invalidProperties"),
                    "{method}: {args}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// /get and the /query capability gate (-04 §2, §3, §3.1; bd JMAP-s4kp.9)
// ---------------------------------------------------------------------------

/// A second vendor namespace for the subselector tests.
const NS2: &str = "other.example.org";

/// The `type` of a method-level error.
fn method_error_type(err: &JmapError) -> Value {
    serde_json::to_value(err).unwrap()["type"].clone()
}

/// Run AddressBook/get with `properties` on `id` and return its one list entry.
async fn book_get_one(
    b: &MemoryBackend,
    id: &str,
    properties: Value,
    using: &[String],
) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": properties });
    handle_address_book_get(b, &(), args, using)
        .await
        .map(|o| o.primary["list"][0].clone())
}

/// -04 §3.1 (L475-477): "when properties is omitted or null, the server
/// MUST include metadata and, where supportsPrivate is true,
/// privateMetadata"; §2 (L296-297): "When returned, its value MUST be an
/// object ({} when no metadata is set), never null"; §2 (L306-307): "When
/// supportsPrivate is false for the data type, this property MUST NOT be
/// returned"; §3 (L461-463): for other requests "metadata and
/// privateMetadata are not default properties of Foo/get (although they
/// are still returned when requested by name)".
#[tokio::test]
async fn book_get_default_properties_follow_capability() {
    let b = backend();
    let bare = create_book(&b, json!({ "name": "bare" })).await;
    let tagged = create_book(
        &b,
        json!({ "name": "tagged", "metadata": json!({ NS: { "k": 1 } }) }),
    )
    .await;

    let got = book_get_one(&b, &bare, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({}), "{got}");
    assert!(got.get("privateMetadata").is_none(), "{got}");
    let got = book_get_one(&b, &tagged, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({ NS: { "k": 1 } }), "{got}");

    let without = using_without_metadata();
    let got = book_get_one(&b, &tagged, Value::Null, &without)
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");
    let props = json!(["id", "metadata", "privateMetadata"]);
    let got = book_get_one(&b, &tagged, props, &without).await.unwrap();
    assert_eq!(got["metadata"], json!({ NS: { "k": 1 } }), "{got}");
    assert!(got.get("privateMetadata").is_none(), "{got}");
}

/// -04 §3.1 (L493-500): "Multiple subselectors with the same root combine
/// by union" and "An explicit root entry supersedes subselectors with the
/// same root"; (L509-512) a namespace that is not supported "MUST be
/// silently omitted from the response, unless the object has data stored
/// under that namespace"; (L488-491) "Any other properties entry beginning
/// with metadata/ or privateMetadata/ MUST be rejected with an
/// "invalidArguments" method-level error"; §3 (L460-461) subselectors are
/// part of the extension, so without the capability they name no
/// property.
#[tokio::test]
async fn book_get_metadata_subselectors() {
    let b = backend();
    let id = create_book(
        &b,
        json!({ "name": "tagged", "metadata": json!({ NS: { "k": 1 }, NS2: { "k": 2 } }) }),
    )
    .await;
    let cases = [
        (
            json!(["metadata/acme.example.com"]),
            json!({ NS: { "k": 1 } }),
        ),
        (
            json!(["metadata/acme.example.com", "metadata/other.example.org"]),
            json!({ NS: { "k": 1 }, NS2: { "k": 2 } }),
        ),
        (
            json!(["metadata", "metadata/acme.example.com"]),
            json!({ NS: { "k": 1 }, NS2: { "k": 2 } }),
        ),
        (json!(["metadata/unlisted.example.net"]), json!({})),
    ];
    for (props, want) in cases {
        let got = book_get_one(&b, &id, props.clone(), &using())
            .await
            .unwrap();
        assert_eq!(got["metadata"], want, "{props}: {got}");
    }

    let props = json!(["metadata/acme.example.com"]);
    let got = book_get_one(&b, &id, props, &using_without_metadata())
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");

    for props in [
        json!(["metadata/"]),
        json!(["metadata/acme.example.com/k"]),
        json!(["privateMetadata/"]),
    ] {
        let err = book_get_one(&b, &id, props.clone(), &using())
            .await
            .unwrap_err();
        assert_eq!(
            method_error_type(&err),
            json!("invalidArguments"),
            "{props}"
        );
    }
}

/// Run ContactCard/get with `properties` on `id` and return its one list entry.
async fn card_get_one(
    b: &MemoryBackend,
    id: &str,
    properties: Value,
    using: &[String],
) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": properties });
    handle_contact_card_get(b, &(), args, using)
        .await
        .map(|o| o.primary["list"][0].clone())
}

/// -04 §3.1 (L475-477): "when properties is omitted or null, the server
/// MUST include metadata and, where supportsPrivate is true,
/// privateMetadata"; §2 (L296-297): "When returned, its value MUST be an
/// object ({} when no metadata is set), never null"; §2 (L306-307): "When
/// supportsPrivate is false for the data type, this property MUST NOT be
/// returned"; §3 (L461-463): for other requests "metadata and
/// privateMetadata are not default properties of Foo/get (although they
/// are still returned when requested by name)".
#[tokio::test]
async fn card_get_default_properties_follow_capability() {
    let b = backend();
    let book = create_book(&b, json!({ "name": "b" })).await;
    let bare = create_card(&b, &[&book], json!({})).await;
    let tagged = create_card(&b, &[&book], json!({ NS: { "k": 1 } })).await;

    let got = card_get_one(&b, &bare, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({}), "{got}");
    assert!(got.get("privateMetadata").is_none(), "{got}");
    let got = card_get_one(&b, &tagged, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({ NS: { "k": 1 } }), "{got}");

    let without = using_without_metadata();
    let got = card_get_one(&b, &tagged, Value::Null, &without)
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");
    let props = json!(["id", "metadata", "privateMetadata"]);
    let got = card_get_one(&b, &tagged, props, &without).await.unwrap();
    assert_eq!(got["metadata"], json!({ NS: { "k": 1 } }), "{got}");
    assert!(got.get("privateMetadata").is_none(), "{got}");
}

/// -04 §3.1 (L493-500): "Multiple subselectors with the same root combine
/// by union" and "An explicit root entry supersedes subselectors with the
/// same root"; (L509-512) a namespace that is not supported "MUST be
/// silently omitted from the response, unless the object has data stored
/// under that namespace"; (L488-491) "Any other properties entry beginning
/// with metadata/ or privateMetadata/ MUST be rejected with an
/// "invalidArguments" method-level error"; §3 (L460-461) subselectors are
/// part of the extension, so without the capability they name no
/// property.
#[tokio::test]
async fn card_get_metadata_subselectors() {
    let b = backend();
    let book = create_book(&b, json!({ "name": "b" })).await;
    let id = create_card(&b, &[&book], json!({ NS: { "k": 1 }, NS2: { "k": 2 } })).await;
    let cases = [
        (
            json!(["metadata/acme.example.com"]),
            json!({ NS: { "k": 1 } }),
        ),
        (
            json!(["metadata/acme.example.com", "metadata/other.example.org"]),
            json!({ NS: { "k": 1 }, NS2: { "k": 2 } }),
        ),
        (
            json!(["metadata", "metadata/acme.example.com"]),
            json!({ NS: { "k": 1 }, NS2: { "k": 2 } }),
        ),
        (json!(["metadata/unlisted.example.net"]), json!({})),
    ];
    for (props, want) in cases {
        let got = card_get_one(&b, &id, props.clone(), &using())
            .await
            .unwrap();
        assert_eq!(got["metadata"], want, "{props}: {got}");
    }

    let props = json!(["metadata/acme.example.com"]);
    let got = card_get_one(&b, &id, props, &using_without_metadata())
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");

    for props in [
        json!(["metadata/"]),
        json!(["metadata/acme.example.com/k"]),
        json!(["privateMetadata/"]),
    ] {
        let err = card_get_one(&b, &id, props.clone(), &using())
            .await
            .unwrap_err();
        assert_eq!(
            method_error_type(&err),
            json!("invalidArguments"),
            "{props}"
        );
    }
}

/// -04 §3 (L460-461): the extensions, the §3.6 filter conditions among
/// them, "apply to requests whose using includes
/// urn:ietf:params:jmap:metadata"; RFC 8620 §5.5 `unsupportedFilter`: "The
/// filter is syntactically valid, but the server cannot process it".
#[tokio::test]
async fn card_metadata_filter_requires_capability() {
    let b = backend();
    let without = using_without_metadata();
    for filter in [
        json!({ "metadataExists": NS }),
        json!({ "operator": "NOT", "conditions": [json!({ "metadataExists": NS })] }),
    ] {
        let args = json!({ "accountId": ACCOUNT_ID, "filter": filter.clone() });
        let err = handle_contact_card_query(&b, &(), args, &without)
            .await
            .unwrap_err();
        assert_eq!(
            method_error_type(&err),
            json!("unsupportedFilter"),
            "{filter}"
        );
        let args =
            json!({ "accountId": ACCOUNT_ID, "filter": filter.clone(), "sinceQueryState": "0" });
        let err = handle_contact_card_query_changes(&b, &(), args, &without)
            .await
            .unwrap_err();
        assert_eq!(
            method_error_type(&err),
            json!("unsupportedFilter"),
            "{filter}"
        );
    }
    let args = json!({ "accountId": ACCOUNT_ID, "filter": json!({ "metadataExists": NS }) });
    assert!(
        handle_contact_card_query(&b, &(), args, &using())
            .await
            .is_ok()
    );
}

/// The destination account of the /copy test.
const OTHER_ID: &str = "a2";

/// Run ContactCard/copy of one create entry from [`ACCOUNT_ID`] into
/// [`OTHER_ID`] and return the response.
async fn card_copy(b: &MemoryBackend, entry: Value, using: &[String]) -> Value {
    let args = json!({
        "fromAccountId": ACCOUNT_ID,
        "accountId": OTHER_ID,
        "create": { "k": entry },
    });
    handle_contact_card_copy(b, &(), args, "c0", using)
        .await
        .expect("/copy must not fail at method level")
        .primary
}

/// -04 §3.4 (L648-655): "Foo/copy copies metadata to the new object" and
/// "When the request does not use this capability, only metadata is
/// copied. The create object MAY include metadata and privateMetadata to
/// override the copied values, with the same rules as a create in
/// Foo/set"; those rules: without the capability "Foo/set MUST reject any
/// write to them with an "invalidProperties" SetError" (§3, L463-464), and
/// on create "A value of null for either property MUST be rejected with
/// an "invalidProperties" SetError" (§3.2).
#[tokio::test]
async fn card_copy_metadata() {
    let b = backend().with_account(&Id::from(OTHER_ID));
    let book = create_book(&b, json!({ "name": "b" })).await;
    let src = create_card(&b, &[&book], json!({ NS: { "k": 1 } })).await;
    let entry = |extra: Value| {
        let mut e = json!({ "id": &src });
        if let (Some(e), Some(x)) = (e.as_object_mut(), extra.as_object()) {
            e.extend(x.clone());
        }
        e
    };
    let dest_metadata = |resp: &Value| {
        let id = resp["copied"]["k"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("copy failed: {resp}"))
            .to_owned();
        let args = json!({ "accountId": OTHER_ID, "ids": [id], "properties": ["metadata"] });
        (args, id)
    };

    let resp = card_copy(&b, entry(json!({})), &using_without_metadata()).await;
    let (args, _) = dest_metadata(&resp);
    let got = handle_contact_card_get(&b, &(), args, &using())
        .await
        .unwrap()
        .primary;
    assert_eq!(
        got["list"][0]["metadata"],
        json!({ NS: { "k": 1 } }),
        "{got}"
    );

    let resp = card_copy(
        &b,
        entry(json!({ "metadata": { NS: { "k": 2 } } })),
        &using_without_metadata(),
    )
    .await;
    assert_eq!(
        resp["notCopied"]["k"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );
    assert_eq!(
        resp["notCopied"]["k"]["properties"],
        json!(["metadata"]),
        "{resp}"
    );

    let resp = card_copy(&b, entry(json!({ "metadata": null })), &using()).await;
    assert_eq!(
        resp["notCopied"]["k"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );

    let resp = card_copy(
        &b,
        entry(json!({ "metadata": { NS: { "k": 2 } } })),
        &using(),
    )
    .await;
    let (args, _) = dest_metadata(&resp);
    let got = handle_contact_card_get(&b, &(), args, &using())
        .await
        .unwrap()
        .primary;
    assert_eq!(
        got["list"][0]["metadata"],
        json!({ NS: { "k": 2 } }),
        "{got}"
    );
}
