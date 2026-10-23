//! draft-ietf-jmap-metadata-04 on TaskList and Task in the in-memory
//! backend opted in with `MemoryBackend::with_metadata_info`
//! (bd JMAP-s4kp.4).
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
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_server::{Dispatcher, HandlerOutput};
use jmap_tasks_server::{
    TasksBackend, handle_task_changes, handle_task_get, handle_task_list_changes,
    handle_task_list_get, handle_task_list_set, handle_task_query, handle_task_set,
    register_tasks_handlers,
};
use jmap_tasks_types::TaskList;
use jmap_types::{Id, JmapError, JmapRequest, State};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "a1";
const NS: &str = "acme.example.com";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:tasks".to_owned(),
        "urn:ietf:params:jmap:metadata".to_owned(),
    ]
}

fn using_without_metadata() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:tasks".to_owned(),
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
        .with_metadata_info("TaskList", info())
        .with_metadata_info("Task", info())
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

/// Store a TaskList whose `myRights.maySetMetadata` is `may_set_metadata`
/// (server-set, so not writable through TaskList/set) and return its id.
async fn task_list_fixture(backend: &MemoryBackend, name: &str, may_set_metadata: bool) -> String {
    let name = name.to_owned();
    let (id, _) = TasksBackend::create_object::<TaskList, _>(
        backend,
        &(),
        &Id::from(ACCOUNT_ID),
        "fixture",
        move |id| {
            serde_json::from_value(json!({
                "id": id,
                "name": name,
                "isSubscribed": true,
                "myRights": {
                    "mayReadItems": true, "mayWriteAll": true, "mayWriteOwn": true,
                    "mayUpdatePrivate": true, "mayRSVP": true, "mayAdmin": true,
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

async fn list_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } = handle_task_list_set(backend, &(), args, using)
        .await
        .expect("TaskList/set must not fail at method level");
    primary
}

async fn task_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } =
        handle_task_set(backend, &(), &BTreeMap::new(), args, using)
            .await
            .expect("Task/set must not fail at method level");
    primary
}

async fn create_list(backend: &MemoryBackend, create: Value) -> String {
    let resp = list_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("TaskList/set create failed: {resp}"))
        .to_owned()
}

/// Apply a TaskList/set update that must succeed; returns the response.
async fn update_list(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let resp = list_set(backend, json!({ "update": { id: patch } }), &using()).await;
    assert_eq!(resp["updated"], json!({ id: null }), "{resp}");
    resp
}

async fn create_task(backend: &MemoryBackend, list: &str, metadata: Value) -> String {
    let create = json!({
        "@type": "Task",
        "taskListId": list,
        "title": "t",
        "metadata": metadata,
    });
    let resp = task_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Task/set create failed: {resp}"))
        .to_owned()
}

async fn list_property(backend: &MemoryBackend, id: &str, property: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id] });
    let HandlerOutput { primary, .. } = handle_task_list_get(backend, &(), args).await.unwrap();
    primary["list"][0][property].clone()
}

async fn task_property(backend: &MemoryBackend, id: &str, property: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id] });
    let HandlerOutput { primary, .. } = handle_task_get(backend, &(), args).await.unwrap();
    primary["list"][0][property].clone()
}

async fn task_query(backend: &MemoryBackend, filter: Value) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "filter": filter });
    handle_task_query(backend, &(), args)
        .await
        .map(|o| o.primary["ids"].clone())
}

async fn list_changes(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_task_list_changes(backend, &(), args, using)
        .await
        .expect("TaskList/changes must succeed")
        .primary
}

fn sorted(mut ids: Vec<&str>) -> Value {
    ids.sort_unstable();
    json!(ids)
}

// ---------------------------------------------------------------------------
// /query (-04 §3.6)
// ---------------------------------------------------------------------------

/// -04 §3.6 `metadataExists`: "Matches an object if and only if a value
/// is present at the given path within the object's metadata property. A
/// namespace-only path matches if the namespace key is present and its
/// value is not the empty object {}."
#[tokio::test]
async fn task_query_metadata_exists() {
    let backend = backend();
    let list = task_list_fixture(&backend, "Work", true).await;
    let tagged = create_task(
        &backend,
        &list,
        json!({ NS: { "approvalStatus": "pending" } }),
    )
    .await;
    create_task(&backend, &list, json!({ NS: {} })).await;
    create_task(&backend, &list, json!({})).await;

    for path in [NS, "acme.example.com/approvalStatus"] {
        assert_eq!(
            task_query(&backend, json!({ "metadataExists": path }))
                .await
                .unwrap(),
            json!([tagged]),
            "{path}"
        );
    }
}

/// -04 §3.6 conditions extend the type's own FilterCondition, so they sit
/// in one leaf with draft-ietf-jmap-tasks-06 §4 `taskListId` and both must
/// hold; `metadataTextContains` is a substring match "under the
/// "i;unicode-casemap" collation", so case is ignored.
#[tokio::test]
async fn task_query_combines_task_list_id_and_text_contains() {
    let backend = backend();
    let work = task_list_fixture(&backend, "Work", true).await;
    let home = task_list_fixture(&backend, "Home", true).await;
    let hit = create_task(
        &backend,
        &work,
        json!({ NS: { "memo": "Please FOLLOW UP soon" } }),
    )
    .await;
    create_task(&backend, &work, json!({ NS: { "memo": "done" } })).await;
    create_task(&backend, &home, json!({ NS: { "memo": "follow up" } })).await;

    let filter = json!({
        "taskListId": work,
        "metadataTextContains": { "path": "acme.example.com/memo", "value": "follow up" }
    });
    assert_eq!(task_query(&backend, filter).await.unwrap(), json!([hit]));
}

/// -04 §3.6 `metadataTextEquals`: "exactly equal to the supplied value,
/// comparing Unicode code points without case folding".
#[tokio::test]
async fn task_query_text_equals_is_case_sensitive() {
    let backend = backend();
    let list = task_list_fixture(&backend, "Work", true).await;
    let exact = create_task(&backend, &list, json!({ NS: { "tag": "Urgent" } })).await;
    create_task(&backend, &list, json!({ NS: { "tag": "urgent" } })).await;

    let equals =
        json!({ "metadataTextEquals": { "path": "acme.example.com/tag", "value": "Urgent" } });
    assert_eq!(task_query(&backend, equals).await.unwrap(), json!([exact]));
}

/// -04 §3.6: "Any privateMetadata* condition (existence or text) MUST be
/// rejected with an "unsupportedFilter" error if supportsPrivate is false
/// for the data type", and "A path that is not well formed (for example,
/// one with an invalid ~ escape) MUST be rejected with an
/// "invalidArguments" error" (RFC 6901 §3 allows only `~0` and `~1`),
/// also inside an operator tree, which this backend otherwise cannot
/// evaluate (RFC 8620 §5.5 `unsupportedFilter`).
#[tokio::test]
async fn invalid_conditions_are_rejected() {
    let backend = backend();
    let err = task_query(&backend, json!({ "privateMetadataExists": NS }))
        .await
        .unwrap_err();
    assert_eq!(error_type(err), json!("unsupportedFilter"));

    let err = task_query(
        &backend,
        json!({ "operator": "NOT", "conditions": [{ "metadataExists": "acme.example.com/a~2" }] }),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), json!("invalidArguments"));

    let err = task_query(
        &backend,
        json!({ "operator": "NOT", "conditions": [{ "metadataExists": NS }] }),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), json!("unsupportedFilter"));
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account", so a
/// backend that did not opt Task in cannot process the condition
/// (RFC 8620 §5.5 `unsupportedFilter`).
#[tokio::test]
async fn unlisted_type_rejects_metadata_condition() {
    let backend = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    let err = task_query(&backend, json!({ "metadataExists": NS }))
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
async fn task_list_patch_within_namespace_then_null() {
    let backend = backend();
    let id = create_list(
        &backend,
        json!({
            "name": "l", "isSubscribed": true,
            "metadata": { NS: { "color": "red", "icon": "star" } }
        }),
    )
    .await;
    update_list(
        &backend,
        &id,
        json!({ "metadata/acme.example.com/color": "blue" }),
    )
    .await;
    assert_eq!(
        list_property(&backend, &id, "metadata").await,
        json!({ NS: { "color": "blue", "icon": "star" } })
    );
    update_list(&backend, &id, json!({ "metadata": null })).await;
    assert_eq!(list_property(&backend, &id, "metadata").await, json!({}));
}

/// After the -04 §6.6 example: a primary property and two vendor metadata
/// keys change in one /set call, answered with `"updated": { id: null }`.
/// -04 §3.2 patch paths follow RFC 8620 §5.3, so "the parent of the target
/// MUST exist", and a missing one is `invalidPatch` that leaves the task
/// unchanged ("either all of these changes apply or none do").
#[tokio::test]
async fn task_update_primary_and_metadata_atomically() {
    let backend = backend();
    let list = task_list_fixture(&backend, "Work", true).await;
    let bare = create_task(&backend, &list, json!({})).await;
    let resp = task_set(
        &backend,
        json!({ "update": { &bare: {
            "title": "Quarterly Review",
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
    assert_eq!(task_property(&backend, &bare, "title").await, json!("t"));

    let id = create_task(&backend, &list, json!({ NS: {} })).await;
    let resp = task_set(
        &backend,
        json!({ "update": { &id: {
            "title": "Quarterly Review",
            "metadata/acme.example.com/lastModifiedReason": "Rescheduled per manager request",
            "metadata/acme.example.com/approvalStatus": "pending"
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &id: null }), "{resp}");
    assert_eq!(
        task_property(&backend, &id, "metadata").await,
        json!({ NS: {
            "lastModifiedReason": "Rescheduled per manager request",
            "approvalStatus": "pending"
        } })
    );
    assert_eq!(
        task_property(&backend, &id, "title").await,
        json!("Quarterly Review")
    );
}

/// -04 §3.1: "Changes to metadata or privateMetadata alone MUST NOT update
/// server-maintained modification timestamps of the related object".
/// A Task's `updated` (RFC 8984 §4.1.6) is such a timestamp.
#[tokio::test]
async fn metadata_only_update_keeps_updated_timestamp() {
    let backend = backend();
    let list = task_list_fixture(&backend, "Work", true).await;
    let create = json!({
        "@type": "Task",
        "taskListId": &list,
        "title": "t",
        "updated": "2024-01-01T00:00:00Z",
    });
    let resp = task_set(&backend, json!({ "create": { "c": create } }), &using()).await;
    let id = resp["created"]["c"]["id"].as_str().unwrap().to_owned();
    let resp = task_set(
        &backend,
        json!({ "update": { &id: { "metadata/acme.example.com": { "k": 1 } } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &id: null }), "{resp}");
    assert_eq!(
        task_property(&backend, &id, "updated").await,
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
    let list = task_list_fixture(&backend, "Work", true).await;
    let resp = task_set(
        &backend,
        json!({ "create": { "c": {
            "@type": "Task", "taskListId": &list, "title": "t", "metadata": null
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));

    let id = create_task(&backend, &list, json!({})).await;
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
        let resp = task_set(&backend, json!({ "update": { &id: patch } }), using).await;
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
    assert_eq!(task_property(&backend, &id, "metadata").await, json!({}));
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account", so a
/// backend that did not opt in rejects the write.
#[tokio::test]
async fn unlisted_type_rejects_metadata_write() {
    let backend = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    let resp = list_set(
        &backend,
        json!({ "create": { "c": {
            "name": "l", "isSubscribed": true, "metadata": { NS: {} }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));
}

/// -04 §4.1: "A write to metadata without maySetMetadata MUST be rejected
/// with a "forbidden" SetError." A TaskList carries its own right; other
/// properties stay writable.
#[tokio::test]
async fn task_list_without_may_set_metadata_is_forbidden() {
    let backend = backend();
    let id = task_list_fixture(&backend, "locked", false).await;
    let resp = list_set(
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

    update_list(&backend, &id, json!({ "name": "renamed" })).await;
}

/// -04 §4.1: an object without a rights object of its own "takes the
/// right from its containers. Modifying its metadata requires
/// maySetMetadata on every container it belongs to; when the same update
/// changes its containers, on every container both before and after the
/// change; and on create, on the containers given in the create." A
/// Task's one container is its `taskListId` (draft-ietf-jmap-tasks-06 §4).
#[tokio::test]
async fn task_metadata_needs_right_on_task_list() {
    let backend = backend();
    let open = task_list_fixture(&backend, "open", true).await;
    let locked = task_list_fixture(&backend, "locked", false).await;
    let task = create_task(&backend, &open, json!({})).await;

    // Moving into the locked list while writing metadata.
    let resp = task_set(
        &backend,
        json!({ "update": { &task: {
            "taskListId": &locked,
            "metadata/acme.example.com": { "k": 1 }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&task]["type"],
        json!("forbidden"),
        "{resp}"
    );

    // The same move without a metadata write is allowed.
    let resp = task_set(
        &backend,
        json!({ "update": { &task: { "taskListId": &locked } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &task: null }), "{resp}");

    // Now in the locked list: leaving it in the same update still needs
    // the right "before ... the change".
    let resp = task_set(
        &backend,
        json!({ "update": { &task: {
            "taskListId": &open,
            "metadata/acme.example.com": { "k": 1 }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&task]["type"],
        json!("forbidden"),
        "{resp}"
    );

    // On create.
    let create = json!({
        "@type": "Task",
        "taskListId": &locked,
        "title": "t",
        "metadata": { NS: { "k": 1 } },
    });
    let resp = task_set(&backend, json!({ "create": { "c": create } }), &using()).await;
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
    let resp = list_set(
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

    let locked = task_list_fixture(&backend, "locked", false).await;
    for id in [locked.as_str(), "nope"] {
        let resp = list_set(
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
async fn metadata_only_task_list_change() {
    let backend = backend();
    let id = task_list_fixture(&backend, "l", true).await;
    let resp = update_list(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    let new_state = resp["newState"].clone();

    let report = list_changes(&backend, json!({ "sinceState": since }), &using()).await;
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
        let ignored = list_changes(&backend, args, &using).await;
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
    let id = task_list_fixture(&backend, "l", true).await;
    let resp = update_list(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_list(&backend, &id, json!({ "name": "n" })).await;

    for ignore in [false, true] {
        let resp = list_changes(
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
/// with `maxChanges` 1 would otherwise be `cannotCalculateChanges`
/// (RFC 8620 §5.2) in this backend.
#[tokio::test]
async fn excluded_ids_do_not_count_toward_max_changes() {
    let backend = backend();
    let quiet = task_list_fixture(&backend, "quiet", true).await;
    let loud = task_list_fixture(&backend, "loud", true).await;
    let resp = update_list(
        &backend,
        &quiet,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_list(&backend, &loud, json!({ "name": "louder" })).await;

    let resp = list_changes(
        &backend,
        json!({ "sinceState": since, "maxChanges": 1, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!([loud]), "{resp}");
    assert_eq!(resp["hasMoreChanges"], json!(false), "{resp}");
}

/// -04 §3.3 on Task/changes: two tasks whose only change is metadata give
/// `updatedProperties` `["metadata"]`.
#[tokio::test]
async fn metadata_only_task_changes() {
    let backend = backend();
    let list = task_list_fixture(&backend, "Work", true).await;
    let t1 = create_task(&backend, &list, json!({})).await;
    let t2 = create_task(&backend, &list, json!({})).await;
    let resp = task_set(
        &backend,
        json!({ "update": {
            &t1: { "metadata/acme.example.com": { "k": 1 } },
            &t2: { "metadata": { NS: { "k": 2 } } }
        } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &t1: null, &t2: null }), "{resp}");
    let since = resp["oldState"].clone();

    let args = json!({ "accountId": ACCOUNT_ID, "sinceState": since });
    let resp = handle_task_changes(&backend, &(), args, &using())
        .await
        .unwrap()
        .primary;
    let updated: Vec<&str> = resp["updated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(sorted(updated), sorted(vec![&t1, &t2]));
    assert_eq!(resp["updatedProperties"], json!(["metadata"]));
}

// ---------------------------------------------------------------------------
// Dispatcher wiring (-04 §3)
// ---------------------------------------------------------------------------

/// `register_tasks_handlers` forwards the request's `using`: -04 §3
/// "Foo/set MUST reject any write to them" without the capability, and the
/// same write succeeds with it.
#[tokio::test]
async fn dispatcher_forwards_using() {
    let backend = backend();
    let list = task_list_fixture(&backend, "Work", true).await;
    let task = create_task(&backend, &list, json!({ NS: {} })).await;
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_tasks_handlers(&mut dispatcher, Arc::new(backend));

    for (method, id) in [("TaskList/set", &list), ("Task/set", &task)] {
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
