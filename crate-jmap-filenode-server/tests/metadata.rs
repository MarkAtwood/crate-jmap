//! draft-ietf-jmap-metadata-04 on FileNode in the in-memory backend opted
//! in with `MemoryBackend::with_metadata_info` (bd JMAP-s4kp.6).
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
use jmap_filenode_server::{
    handle_filenode_changes, handle_filenode_copy, handle_filenode_get, handle_filenode_query,
    handle_filenode_query_changes, handle_filenode_set, register_filenode_handlers,
};
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_server::{Dispatcher, HandlerOutput};
use jmap_types::{Id, JmapError, JmapRequest, State};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "a1";
const NS: &str = "acme.example.com";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:filenode".to_owned(),
        "urn:ietf:params:jmap:metadata".to_owned(),
    ]
}

fn using_without_metadata() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:filenode".to_owned(),
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
        .with_metadata_info("FileNode", info())
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

/// Seed a top-level Directory `id` whose `myRights.maySetMetadata` is
/// `may_set_metadata` (`myRights` is server-set, so not writable through
/// FileNode/set) and whose server-set `changed` is fixed.
fn seed_dir(backend: &MemoryBackend, id: &str, may_set_metadata: bool) {
    backend.seed_object(
        &Id::from(ACCOUNT_ID),
        "FileNode",
        &Id::from(id),
        json!({
            "id": id, "@type": "Directory", "name": id, "parentId": null,
            "changed": "2024-01-01T00:00:00Z",
            "myRights": {
                "mayRead": true, "mayAddChildren": true, "mayRename": true,
                "mayDelete": true, "mayModifyContent": true, "mayShare": true,
                "maySetMetadata": may_set_metadata
            }
        }),
    );
}

async fn node_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } =
        handle_filenode_set(backend, &(), &BTreeMap::new(), args, using)
            .await
            .expect("FileNode/set must not fail at method level");
    primary
}

/// Create a top-level Directory named `name` carrying `metadata`.
async fn create_dir(backend: &MemoryBackend, name: &str, metadata: Value) -> String {
    let create = json!({ "@type": "Directory", "name": name, "metadata": metadata });
    let resp = node_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("FileNode/set create failed: {resp}"))
        .to_owned()
}

/// Apply a FileNode/set update that must succeed; returns the response.
async fn update_node(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let resp = node_set(backend, json!({ "update": { id: patch } }), &using()).await;
    assert!(resp["updated"].get(id).is_some(), "{resp}");
    resp
}

async fn node_property(backend: &MemoryBackend, id: &str, property: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id] });
    let HandlerOutput { primary, .. } = handle_filenode_get(backend, &(), args, &using())
        .await
        .unwrap();
    primary["list"][0][property].clone()
}

async fn node_query(backend: &MemoryBackend, filter: Value) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "filter": filter });
    handle_filenode_query(backend, &(), args, &using())
        .await
        .map(|o| o.primary["ids"].clone())
}

async fn node_changes(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_filenode_changes(backend, &(), args, using)
        .await
        .expect("FileNode/changes must succeed")
        .primary
}

// ---------------------------------------------------------------------------
// /query (-04 §3.6)
// ---------------------------------------------------------------------------

/// -04 §3.6 `metadataExists`: "Matches an object if and only if a value
/// is present at the given path within the object's metadata property. A
/// namespace-only path matches if the namespace key is present and its
/// value is not the empty object {}."
#[tokio::test]
async fn query_metadata_exists() {
    let backend = backend();
    let tagged = create_dir(&backend, "tagged", json!({ NS: { "tier": "gold" } })).await;
    create_dir(&backend, "empty-ns", json!({ NS: {} })).await;
    create_dir(&backend, "bare", json!({})).await;

    for path in [NS, "acme.example.com/tier"] {
        assert_eq!(
            node_query(&backend, json!({ "metadataExists": path }))
                .await
                .unwrap(),
            json!([tagged]),
            "{path}"
        );
    }
}

/// -04 §3.6 conditions extend the type's own FilterCondition, so they sit
/// in one leaf with draft-ietf-jmap-filenode `nameMatch` and both must
/// hold; `metadataTextContains` is a substring match "under the
/// "i;unicode-casemap" collation", so case is ignored.
#[tokio::test]
async fn query_combines_name_match_and_text_contains() {
    let backend = backend();
    let hit = create_dir(
        &backend,
        "report-2026",
        json!({ NS: { "memo": "Please FOLLOW UP soon" } }),
    )
    .await;
    create_dir(&backend, "report-2025", json!({ NS: { "memo": "done" } })).await;
    create_dir(&backend, "notes", json!({ NS: { "memo": "follow up" } })).await;

    let filter = json!({
        "nameMatch": "report-*",
        "metadataTextContains": { "path": "acme.example.com/memo", "value": "follow up" }
    });
    assert_eq!(node_query(&backend, filter).await.unwrap(), json!([hit]));
}

/// -04 §3.6: "These conditions compose with the data type's existing
/// filter conditions through the usual AND/OR/NOT operators (Section 5.5
/// of [RFC8620])", and `metadataTextEquals` is "exactly equal to the
/// supplied value, comparing Unicode code points without case folding".
#[tokio::test]
async fn query_text_equals_in_operator_tree() {
    let backend = backend();
    let exact = create_dir(&backend, "a", json!({ NS: { "tag": "Urgent" } })).await;
    let lower = create_dir(&backend, "b", json!({ NS: { "tag": "urgent" } })).await;

    let equals =
        json!({ "metadataTextEquals": { "path": "acme.example.com/tag", "value": "Urgent" } });
    assert_eq!(
        node_query(&backend, equals.clone()).await.unwrap(),
        json!([exact])
    );
    let not = json!({ "operator": "NOT", "conditions": [equals] });
    assert_eq!(node_query(&backend, not).await.unwrap(), json!([lower]));
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
    let err = node_query(&backend, json!({ "privateMetadataExists": NS }))
        .await
        .unwrap_err();
    assert_eq!(error_type(err), json!("unsupportedFilter"));

    let err = node_query(
        &backend,
        json!({ "operator": "OR", "conditions": [{ "metadataExists": "acme.example.com/a~2" }] }),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), json!("invalidArguments"));
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account", so a
/// backend that did not opt FileNode in cannot process the condition
/// (RFC 8620 §5.5 `unsupportedFilter`).
#[tokio::test]
async fn unlisted_type_rejects_metadata_condition() {
    let backend = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    let err = node_query(&backend, json!({ "metadataExists": NS }))
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
async fn patch_within_namespace_then_null() {
    let backend = backend();
    let id = create_dir(
        &backend,
        "d",
        json!({ NS: { "color": "red", "icon": "star" } }),
    )
    .await;
    update_node(
        &backend,
        &id,
        json!({ "metadata/acme.example.com/color": "blue" }),
    )
    .await;
    assert_eq!(
        node_property(&backend, &id, "metadata").await,
        json!({ NS: { "color": "blue", "icon": "star" } })
    );
    update_node(&backend, &id, json!({ "metadata": null })).await;
    assert_eq!(node_property(&backend, &id, "metadata").await, json!({}));
}

/// After the -04 §6.6 example: a primary property and two vendor metadata
/// keys change in one /set call. -04 §3.2 patch paths follow RFC 8620
/// §5.3, so "the parent of the target MUST exist", and a missing one is
/// `invalidPatch` that leaves the node unchanged ("either all of these
/// changes apply or none do").
#[tokio::test]
async fn update_primary_and_metadata_atomically() {
    let backend = backend();
    let bare = create_dir(&backend, "bare", json!({})).await;
    let resp = node_set(
        &backend,
        json!({ "update": { &bare: {
            "name": "renamed",
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
    assert_eq!(node_property(&backend, &bare, "name").await, json!("bare"));

    let id = create_dir(&backend, "d", json!({ NS: {} })).await;
    update_node(
        &backend,
        &id,
        json!({
            "name": "renamed",
            "metadata/acme.example.com/lastModifiedReason": "Rescheduled per manager request",
            "metadata/acme.example.com/approvalStatus": "pending"
        }),
    )
    .await;
    assert_eq!(
        node_property(&backend, &id, "metadata").await,
        json!({ NS: {
            "lastModifiedReason": "Rescheduled per manager request",
            "approvalStatus": "pending"
        } })
    );
    assert_eq!(node_property(&backend, &id, "name").await, json!("renamed"));
}

/// -04 §3.1: "Changes to metadata or privateMetadata alone MUST NOT update
/// server-maintained modification timestamps of the related object, such
/// as the changed property of FileNode".
#[tokio::test]
async fn metadata_only_update_keeps_changed() {
    let backend = backend();
    seed_dir(&backend, "d", true);
    update_node(
        &backend,
        "d",
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    assert_eq!(
        node_property(&backend, "d", "metadata").await,
        json!({ NS: { "k": 1 } })
    );
    assert_eq!(
        node_property(&backend, "d", "changed").await,
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
    let resp = node_set(
        &backend,
        json!({ "create": { "c": {
            "@type": "Directory", "name": "n", "metadata": null
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));

    let resp = node_set(
        &backend,
        json!({ "create": { "c": {
            "@type": "Directory", "name": "deep",
            "metadata": { NS: { "a": { "b": { "c": 1 } } } }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["c"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );

    let id = create_dir(&backend, "d", json!({})).await;
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
        let resp = node_set(&backend, json!({ "update": { &id: patch } }), using).await;
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
    assert_eq!(node_property(&backend, &id, "metadata").await, json!({}));
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account", so a
/// backend that did not opt in rejects the write, on create and update.
#[tokio::test]
async fn unlisted_type_rejects_metadata_write() {
    let backend = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    let resp = node_set(
        &backend,
        json!({ "create": { "c": {
            "@type": "Directory", "name": "n", "metadata": { NS: {} }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));

    seed_dir(&backend, "d", true);
    let resp = node_set(
        &backend,
        json!({ "update": { "d": { "metadata/acme.example.com": { "k": 1 } } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notUpdated"]["d"]["type"], json!("invalidProperties"));
}

/// -04 §4.1 names FileNode among the types with a myRights property, whose
/// rights object gains `maySetMetadata`: "A write to metadata without
/// maySetMetadata MUST be rejected with a "forbidden" SetError." Other
/// properties stay writable.
#[tokio::test]
async fn node_without_may_set_metadata_is_forbidden() {
    let backend = backend();
    seed_dir(&backend, "locked", false);
    let resp = node_set(
        &backend,
        json!({ "update": { "locked": { "metadata/acme.example.com": { "k": 1 } } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["locked"]["type"],
        json!("forbidden"),
        "{resp}"
    );

    update_node(&backend, "locked", json!({ "name": "renamed" })).await;
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
    let resp = node_set(
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

    seed_dir(&backend, "locked", false);
    for id in ["locked", "nope"] {
        let resp = node_set(
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
async fn metadata_only_change() {
    let backend = backend();
    seed_dir(&backend, "d", true);
    let resp = update_node(
        &backend,
        "d",
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    let new_state = resp["newState"].clone();

    let report = node_changes(&backend, json!({ "sinceState": since }), &using()).await;
    assert_eq!(report["updated"], json!(["d"]));
    assert_eq!(report["updatedProperties"], json!(["metadata"]));
    assert_eq!(report["newState"], new_state);

    for (args, using) in [
        (
            json!({ "sinceState": since, "ignoreMetadataOnlyChanges": true }),
            using(),
        ),
        (json!({ "sinceState": since }), using_without_metadata()),
    ] {
        let ignored = node_changes(&backend, args, &using).await;
        assert_eq!(ignored["updated"], json!([]), "{ignored}");
        assert_eq!(ignored["updatedProperties"], Value::Null);
        assert_eq!(ignored["newState"], new_state);
    }
}

/// -04 §3.3: "If any other property ... of any id in the array may also
/// have changed ... this argument MUST be null", and "Ids with any other
/// change remain in updated" under `ignoreMetadataOnlyChanges`, also when
/// the other change comes after a metadata-only one.
#[tokio::test]
async fn mixed_change_is_reported_without_updated_properties() {
    let backend = backend();
    seed_dir(&backend, "d", true);
    let resp = update_node(
        &backend,
        "d",
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_node(&backend, "d", json!({ "name": "n" })).await;

    let ignored = node_changes(
        &backend,
        json!({ "sinceState": since, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await;
    assert_eq!(ignored["updated"], json!(["d"]), "{ignored}");
    assert_eq!(ignored["updatedProperties"], Value::Null, "{ignored}");

    let report = node_changes(&backend, json!({ "sinceState": since }), &using()).await;
    assert!(
        report["updated"]
            .as_array()
            .unwrap()
            .iter()
            .all(|id| id == "d"),
        "{report}"
    );
    assert_eq!(report["updatedProperties"], Value::Null, "{report}");
}

/// -04 §3.3: "Excluded ids do not count toward maxChanges." With
/// `maxChanges` 1, the metadata-only change would otherwise fill the
/// window and leave `hasMoreChanges` true (RFC 8620 §5.2).
#[tokio::test]
async fn excluded_ids_do_not_count_toward_max_changes() {
    let backend = backend();
    seed_dir(&backend, "quiet", true);
    seed_dir(&backend, "loud", true);
    let resp = update_node(
        &backend,
        "quiet",
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_node(&backend, "loud", json!({ "name": "louder" })).await;

    let resp = node_changes(
        &backend,
        json!({ "sinceState": since, "maxChanges": 1, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!(["loud"]), "{resp}");
    assert_eq!(resp["hasMoreChanges"], json!(false), "{resp}");
}

// ---------------------------------------------------------------------------
// Dispatcher wiring (-04 §3)
// ---------------------------------------------------------------------------

/// `register_filenode_handlers` forwards the request's `using`: -04 §3
/// "Foo/set MUST reject any write to them" without the capability, and the
/// same write succeeds with it.
#[tokio::test]
async fn dispatcher_forwards_using() {
    let backend = backend();
    seed_dir(&backend, "d", true);
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_filenode_handlers(&mut dispatcher, Arc::new(backend));

    let call = json!([["FileNode/set", {
        "accountId": ACCOUNT_ID,
        "update": { "d": { "metadata/acme.example.com": { "k": 1 } } }
    }, "0"]]);
    for (using, expect_ok) in [(using_without_metadata(), false), (using(), true)] {
        let request: JmapRequest =
            serde_json::from_value(json!({ "using": using, "methodCalls": call })).unwrap();
        let resp = dispatcher.dispatch(request, (), State::from("s0")).await;
        let args = &resp.method_responses[0].arguments;
        if expect_ok {
            assert!(args["updated"].get("d").is_some(), "{args}");
        } else {
            assert_eq!(
                args["notUpdated"]["d"]["type"],
                json!("invalidProperties"),
                "{args}"
            );
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

/// Run FileNode/get with `properties` on `id` and return its one list entry.
async fn node_get_one(
    b: &MemoryBackend,
    id: &str,
    properties: Value,
    using: &[String],
) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": properties });
    handle_filenode_get(b, &(), args, using)
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
async fn node_get_default_properties_follow_capability() {
    let b = backend();
    let bare = create_dir(&b, "bare", json!({})).await;
    let tagged = create_dir(&b, "tagged", json!({ NS: { "k": 1 } })).await;

    let got = node_get_one(&b, &bare, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({}), "{got}");
    assert!(got.get("privateMetadata").is_none(), "{got}");
    let got = node_get_one(&b, &tagged, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({ NS: { "k": 1 } }), "{got}");

    let without = using_without_metadata();
    let got = node_get_one(&b, &tagged, Value::Null, &without)
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");
    let props = json!(["id", "metadata", "privateMetadata"]);
    let got = node_get_one(&b, &tagged, props, &without).await.unwrap();
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
async fn node_get_metadata_subselectors() {
    let b = backend();
    let id = create_dir(&b, "tagged", json!({ NS: { "k": 1 }, NS2: { "k": 2 } })).await;
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
        let got = node_get_one(&b, &id, props.clone(), &using())
            .await
            .unwrap();
        assert_eq!(got["metadata"], want, "{props}: {got}");
    }

    let props = json!(["metadata/acme.example.com"]);
    let got = node_get_one(&b, &id, props, &using_without_metadata())
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");

    for props in [
        json!(["metadata/"]),
        json!(["metadata/acme.example.com/k"]),
        json!(["privateMetadata/"]),
    ] {
        let err = node_get_one(&b, &id, props.clone(), &using())
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
async fn node_metadata_filter_requires_capability() {
    let b = backend();
    let without = using_without_metadata();
    for filter in [
        json!({ "metadataExists": NS }),
        json!({ "operator": "NOT", "conditions": [json!({ "metadataExists": NS })] }),
    ] {
        let args = json!({ "accountId": ACCOUNT_ID, "filter": filter.clone() });
        let err = handle_filenode_query(&b, &(), args, &without)
            .await
            .unwrap_err();
        assert_eq!(
            method_error_type(&err),
            json!("unsupportedFilter"),
            "{filter}"
        );
        let args =
            json!({ "accountId": ACCOUNT_ID, "filter": filter.clone(), "sinceQueryState": "0" });
        let err = handle_filenode_query_changes(&b, &(), args, &without)
            .await
            .unwrap_err();
        assert_eq!(
            method_error_type(&err),
            json!("unsupportedFilter"),
            "{filter}"
        );
    }
    let args = json!({ "accountId": ACCOUNT_ID, "filter": json!({ "metadataExists": NS }) });
    assert!(handle_filenode_query(&b, &(), args, &using()).await.is_ok());
}

/// The destination account of the /copy test.
const OTHER_ID: &str = "a2";

/// Run FileNode/copy of one create entry from [`ACCOUNT_ID`] into
/// [`OTHER_ID`] and return the response.
async fn node_copy(b: &MemoryBackend, entry: Value, using: &[String]) -> Value {
    let args = json!({
        "fromAccountId": ACCOUNT_ID,
        "accountId": OTHER_ID,
        "create": { "k": entry },
    });
    handle_filenode_copy(b, &(), args, "c0", using)
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
async fn node_copy_metadata() {
    let b = backend().with_account(&Id::from(OTHER_ID));
    let src = create_dir(&b, "src", json!({ NS: { "k": 1 } })).await;
    let entry = |extra: Value| {
        let mut e = json!({ "id": &src });
        if let (Some(e), Some(x)) = (e.as_object_mut(), extra.as_object()) {
            e.extend(x.clone());
        }
        e
    };
    let dest_metadata = |resp: &Value| {
        let id = resp["created"]["k"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("copy failed: {resp}"))
            .to_owned();
        let args = json!({ "accountId": OTHER_ID, "ids": [id], "properties": ["metadata"] });
        (args, id)
    };

    let resp = node_copy(&b, entry(json!({})), &using_without_metadata()).await;
    let (args, _) = dest_metadata(&resp);
    let got = handle_filenode_get(&b, &(), args, &using())
        .await
        .unwrap()
        .primary;
    assert_eq!(
        got["list"][0]["metadata"],
        json!({ NS: { "k": 1 } }),
        "{got}"
    );

    let resp = node_copy(
        &b,
        entry(json!({ "metadata": { NS: { "k": 2 } } })),
        &using_without_metadata(),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["k"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );
    assert_eq!(
        resp["notCreated"]["k"]["properties"],
        json!(["metadata"]),
        "{resp}"
    );

    let resp = node_copy(&b, entry(json!({ "metadata": null })), &using()).await;
    assert_eq!(
        resp["notCreated"]["k"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );

    // A second copy into the same destination directory needs its own name.
    let resp = node_copy(
        &b,
        entry(json!({ "name": "copy2", "metadata": { NS: { "k": 2 } } })),
        &using(),
    )
    .await;
    let (args, _) = dest_metadata(&resp);
    let got = handle_filenode_get(&b, &(), args, &using())
        .await
        .unwrap()
        .primary;
    assert_eq!(
        got["list"][0]["metadata"],
        json!({ NS: { "k": 2 } }),
        "{got}"
    );
}
