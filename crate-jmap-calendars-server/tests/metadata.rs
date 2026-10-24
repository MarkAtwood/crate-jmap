//! draft-ietf-jmap-metadata-04 on Calendar and CalendarEvent in the
//! in-memory backend opted in with `MemoryBackend::with_metadata_info`
//! (bd JMAP-s4kp.3).
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
use jmap_calendars_server::{
    CalendarsBackend, handle_calendar_changes, handle_calendar_event_changes,
    handle_calendar_event_copy, handle_calendar_event_get, handle_calendar_event_query,
    handle_calendar_event_query_changes, handle_calendar_event_set, handle_calendar_get,
    handle_calendar_set, register_calendars_handlers,
};
use jmap_calendars_types::Calendar;
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_server::{Dispatcher, HandlerOutput};
use jmap_types::{Id, JmapError, JmapRequest, State};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "a1";
const NS: &str = "acme.example.com";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:calendars".to_owned(),
        "urn:ietf:params:jmap:metadata".to_owned(),
    ]
}

fn using_without_metadata() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:calendars".to_owned(),
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
        .with_metadata_info("Calendar", info())
        .with_metadata_info("CalendarEvent", info())
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

/// Store a Calendar whose `myRights.maySetMetadata` is `may_set_metadata`
/// (server-set, so not writable through Calendar/set) and return its id.
async fn calendar_fixture(backend: &MemoryBackend, name: &str, may_set_metadata: bool) -> String {
    let name = name.to_owned();
    let (id, _) = CalendarsBackend::create_object::<Calendar, _>(
        backend,
        &(),
        &Id::from(ACCOUNT_ID),
        "fixture",
        move |id| {
            serde_json::from_value(json!({
                "id": id,
                "name": name,
                "isSubscribed": true,
                "isDefault": false,
                "includeInAvailability": "all",
                "myRights": {
                    "mayReadFreeBusy": true, "mayReadItems": true, "mayWriteAll": true,
                    "mayWriteOwn": true, "mayUpdatePrivate": true, "mayRSVP": true,
                    "mayShare": true, "mayDelete": true,
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

async fn calendar_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } = handle_calendar_set(backend, &(), args, using)
        .await
        .expect("Calendar/set must not fail at method level");
    primary
}

async fn event_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } =
        handle_calendar_event_set(backend, &(), &BTreeMap::new(), args, using)
            .await
            .expect("CalendarEvent/set must not fail at method level");
    primary
}

async fn create_calendar(backend: &MemoryBackend, create: Value) -> String {
    let resp = calendar_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Calendar/set create failed: {resp}"))
        .to_owned()
}

/// Apply a Calendar/set update that must succeed; returns the response.
async fn update_calendar(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let resp = calendar_set(backend, json!({ "update": { id: patch } }), &using()).await;
    assert_eq!(resp["updated"], json!({ id: null }), "{resp}");
    resp
}

async fn create_event(backend: &MemoryBackend, calendar: &str, metadata: Value) -> String {
    let create = json!({
        "calendarIds": { calendar: true },
        "title": "t",
        "metadata": metadata,
    });
    let resp = event_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("CalendarEvent/set create failed: {resp}"))
        .to_owned()
}

async fn calendar_property(backend: &MemoryBackend, id: &str, property: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": [property] });
    let HandlerOutput { primary, .. } = handle_calendar_get(backend, &(), args, &using())
        .await
        .unwrap();
    primary["list"][0][property].clone()
}

async fn event_property(backend: &MemoryBackend, id: &str, property: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": [property] });
    let HandlerOutput { primary, .. } = handle_calendar_event_get(backend, &(), args, &using())
        .await
        .unwrap();
    primary["list"][0][property].clone()
}

async fn event_query(backend: &MemoryBackend, filter: Value) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "filter": filter });
    handle_calendar_event_query(backend, &(), args, &using())
        .await
        .map(|o| o.primary["ids"].clone())
}

async fn calendar_changes(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_calendar_changes(backend, &(), args, using)
        .await
        .expect("Calendar/changes must succeed")
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
async fn event_query_metadata_exists() {
    let backend = backend();
    let cal = calendar_fixture(&backend, "Work", true).await;
    let tagged = create_event(
        &backend,
        &cal,
        json!({ NS: { "approvalStatus": "pending" } }),
    )
    .await;
    create_event(&backend, &cal, json!({ NS: {} })).await;
    create_event(&backend, &cal, json!({})).await;

    for path in [NS, "acme.example.com/approvalStatus"] {
        assert_eq!(
            event_query(&backend, json!({ "metadataExists": path }))
                .await
                .unwrap(),
            json!([tagged]),
            "{path}"
        );
    }
}

/// -04 §3.6 conditions "compose with the data type's existing filter
/// conditions through the usual AND/OR/NOT operators", and
/// `metadataTextContains` is a substring match "under the
/// "i;unicode-casemap" collation", so case is ignored.
#[tokio::test]
async fn event_query_combines_in_calendar_and_text_contains() {
    let backend = backend();
    let work = calendar_fixture(&backend, "Work", true).await;
    let home = calendar_fixture(&backend, "Home", true).await;
    let hit = create_event(
        &backend,
        &work,
        json!({ NS: { "memo": "Please FOLLOW UP soon" } }),
    )
    .await;
    create_event(&backend, &work, json!({ NS: { "memo": "done" } })).await;
    create_event(&backend, &home, json!({ NS: { "memo": "follow up" } })).await;

    let filter = json!({
        "operator": "AND",
        "conditions": [
            { "inCalendar": work },
            { "metadataTextContains": { "path": "acme.example.com/memo", "value": "follow up" } }
        ]
    });
    assert_eq!(event_query(&backend, filter).await.unwrap(), json!([hit]));
}

/// -04 §3.6 `metadataTextEquals`: "exactly equal to the supplied value,
/// comparing Unicode code points without case folding". Under NOT the
/// complement is returned.
#[tokio::test]
async fn event_query_text_equals_under_not() {
    let backend = backend();
    let cal = calendar_fixture(&backend, "Work", true).await;
    let exact = create_event(&backend, &cal, json!({ NS: { "tag": "Urgent" } })).await;
    let other_case = create_event(&backend, &cal, json!({ NS: { "tag": "urgent" } })).await;

    let equals =
        json!({ "metadataTextEquals": { "path": "acme.example.com/tag", "value": "Urgent" } });
    assert_eq!(
        event_query(&backend, equals.clone()).await.unwrap(),
        json!([exact])
    );
    assert_eq!(
        event_query(
            &backend,
            json!({ "operator": "NOT", "conditions": [equals] })
        )
        .await
        .unwrap(),
        json!([other_case])
    );
}

/// -04 §3.6: "Any privateMetadata* condition (existence or text) MUST be
/// rejected with an "unsupportedFilter" error if supportsPrivate is false
/// for the data type", and "A path that is not well formed (for example,
/// one with an invalid ~ escape) MUST be rejected with an
/// "invalidArguments" error" (RFC 6901 §3 allows only `~0` and `~1`).
#[tokio::test]
async fn invalid_conditions_are_rejected() {
    let backend = backend();
    let err = event_query(
        &backend,
        json!({ "operator": "OR", "conditions": [{ "privateMetadataExists": NS }] }),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), json!("unsupportedFilter"));

    let err = event_query(
        &backend,
        json!({ "metadataExists": "acme.example.com/a~2" }),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), json!("invalidArguments"));
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account", so a
/// backend that did not opt CalendarEvent in cannot process the condition
/// (RFC 8620 §5.5 `unsupportedFilter`).
#[tokio::test]
async fn unlisted_type_rejects_metadata_condition() {
    let backend = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    let err = event_query(&backend, json!({ "metadataExists": NS }))
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
async fn calendar_patch_within_namespace_then_null() {
    let backend = backend();
    let id = create_calendar(
        &backend,
        json!({
            "name": "c", "isSubscribed": true, "includeInAvailability": "all",
            "metadata": { NS: { "color": "red", "icon": "star" } }
        }),
    )
    .await;
    update_calendar(
        &backend,
        &id,
        json!({ "metadata/acme.example.com/color": "blue" }),
    )
    .await;
    assert_eq!(
        calendar_property(&backend, &id, "metadata").await,
        json!({ NS: { "color": "blue", "icon": "star" } })
    );
    update_calendar(&backend, &id, json!({ "metadata": null })).await;
    assert_eq!(
        calendar_property(&backend, &id, "metadata").await,
        json!({})
    );
}

/// The -04 §6.6 example: "This example updates a primary property of a
/// CalendarEvent and two vendor metadata properties in the same /set
/// call", answered by "a single CalendarEvent/set response" with
/// `"updated": { "CE789": null }`. The example assumes the namespace
/// already exists: -04 §3.2 patch paths follow RFC 8620 §5.3, so "the
/// parent of the target MUST exist", and a missing one is `invalidPatch`
/// that leaves the event unchanged ("either all of these changes apply or
/// none do").
#[tokio::test]
async fn event_update_primary_and_metadata_atomically() {
    let backend = backend();
    let cal = calendar_fixture(&backend, "Work", true).await;
    let bare = create_event(&backend, &cal, json!({})).await;
    let resp = event_set(
        &backend,
        json!({ "update": { &bare: {
            "title": "Quarterly Review Meeting",
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
    assert_eq!(event_property(&backend, &bare, "title").await, json!("t"));

    let id = create_event(&backend, &cal, json!({ NS: {} })).await;
    let resp = event_set(
        &backend,
        json!({ "update": { &id: {
            "title": "Quarterly Review Meeting",
            "start": "2024-12-15T14:00:00",
            "metadata/acme.example.com/lastModifiedReason": "Rescheduled per manager request",
            "metadata/acme.example.com/approvalStatus": "pending"
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &id: null }), "{resp}");
    assert_eq!(
        event_property(&backend, &id, "metadata").await,
        json!({ NS: {
            "lastModifiedReason": "Rescheduled per manager request",
            "approvalStatus": "pending"
        } })
    );
    assert_eq!(
        event_property(&backend, &id, "title").await,
        json!("Quarterly Review Meeting")
    );
}

/// -04 §3.1: "Changes to metadata or privateMetadata alone MUST NOT update
/// server-maintained modification timestamps of the related object, such
/// as ... the updated property of CalendarEvent".
#[tokio::test]
async fn metadata_only_update_keeps_updated_timestamp() {
    let backend = backend();
    let cal = calendar_fixture(&backend, "Work", true).await;
    let create = json!({
        "calendarIds": { &cal: true },
        "title": "t",
        "updated": "2024-01-01T00:00:00Z",
    });
    let resp = event_set(&backend, json!({ "create": { "c": create } }), &using()).await;
    let id = resp["created"]["c"]["id"].as_str().unwrap().to_owned();
    let resp = event_set(
        &backend,
        json!({ "update": { &id: { "metadata/acme.example.com": { "k": 1 } } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &id: null }), "{resp}");
    assert_eq!(
        event_property(&backend, &id, "updated").await,
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
    let cal = calendar_fixture(&backend, "Work", true).await;
    let resp = event_set(
        &backend,
        json!({ "create": { "c": {
            "calendarIds": { &cal: true }, "title": "t", "metadata": null
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));

    let id = create_event(&backend, &cal, json!({})).await;
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
        let resp = event_set(&backend, json!({ "update": { &id: patch } }), using).await;
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
    assert_eq!(event_property(&backend, &id, "metadata").await, json!({}));
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account", so a
/// backend that did not opt in rejects the write.
#[tokio::test]
async fn unlisted_type_rejects_metadata_write() {
    let backend = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    let resp = calendar_set(
        &backend,
        json!({ "create": { "c": {
            "name": "c", "isSubscribed": true, "includeInAvailability": "all",
            "metadata": { NS: {} }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));
}

/// -04 §4.1: "A write to metadata without maySetMetadata MUST be rejected
/// with a "forbidden" SetError." A Calendar carries its own right; other
/// properties stay writable.
#[tokio::test]
async fn calendar_without_may_set_metadata_is_forbidden() {
    let backend = backend();
    let id = calendar_fixture(&backend, "locked", false).await;
    let resp = calendar_set(
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

    update_calendar(&backend, &id, json!({ "name": "renamed" })).await;
}

/// -04 §4.1: "An object without a rights object of its own, such as an
/// Email, CalendarEvent or ContactCard, takes the right from its
/// containers. Modifying its metadata requires maySetMetadata on every
/// container it belongs to; when the same update changes its containers,
/// on every container both before and after the change; and on create, on
/// the containers given in the create."
#[tokio::test]
async fn event_metadata_needs_right_on_every_calendar() {
    let backend = backend();
    let open = calendar_fixture(&backend, "open", true).await;
    let locked = calendar_fixture(&backend, "locked", false).await;
    let event = create_event(&backend, &open, json!({})).await;

    // Adding a locked calendar while writing metadata.
    let resp = event_set(
        &backend,
        json!({ "update": { &event: {
            format!("calendarIds/{locked}"): true,
            "metadata/acme.example.com": { "k": 1 }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&event]["type"],
        json!("forbidden"),
        "{resp}"
    );

    // The same change without a metadata write is allowed.
    let resp = event_set(
        &backend,
        json!({ "update": { &event: { format!("calendarIds/{locked}"): true } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &event: null }), "{resp}");

    // Now in the locked calendar: leaving it in the same update still
    // needs the right "before ... the change".
    let resp = event_set(
        &backend,
        json!({ "update": { &event: {
            format!("calendarIds/{locked}"): null,
            "metadata/acme.example.com": { "k": 1 }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&event]["type"],
        json!("forbidden"),
        "{resp}"
    );

    // On create.
    let create = json!({
        "calendarIds": { &open: true, &locked: true },
        "title": "t",
        "metadata": { NS: { "k": 1 } },
    });
    let resp = event_set(&backend, json!({ "create": { "c": create } }), &using()).await;
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
    let resp = calendar_set(
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

    let locked = calendar_fixture(&backend, "locked", false).await;
    for id in [locked.as_str(), "nope"] {
        let resp = calendar_set(
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
async fn metadata_only_calendar_change() {
    let backend = backend();
    let id = calendar_fixture(&backend, "c", true).await;
    let resp = update_calendar(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    let new_state = resp["newState"].clone();

    let report = calendar_changes(&backend, json!({ "sinceState": since }), &using()).await;
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
        let ignored = calendar_changes(&backend, args, &using).await;
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
    let id = calendar_fixture(&backend, "c", true).await;
    let resp = update_calendar(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_calendar(&backend, &id, json!({ "name": "n" })).await;

    for ignore in [false, true] {
        let resp = calendar_changes(
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
    let quiet = calendar_fixture(&backend, "quiet", true).await;
    let loud = calendar_fixture(&backend, "loud", true).await;
    let resp = update_calendar(
        &backend,
        &quiet,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_calendar(&backend, &loud, json!({ "name": "louder" })).await;

    let resp = calendar_changes(
        &backend,
        json!({ "sinceState": since, "maxChanges": 1, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!([loud]), "{resp}");
    assert_eq!(resp["hasMoreChanges"], json!(false), "{resp}");
}

/// -04 §3.3 on CalendarEvent/changes: two events whose only change is
/// metadata give `updatedProperties` `["metadata"]`.
#[tokio::test]
async fn metadata_only_event_changes() {
    let backend = backend();
    let cal = calendar_fixture(&backend, "Work", true).await;
    let e1 = create_event(&backend, &cal, json!({})).await;
    let e2 = create_event(&backend, &cal, json!({})).await;
    let resp = event_set(
        &backend,
        json!({ "update": {
            &e1: { "metadata/acme.example.com": { "k": 1 } },
            &e2: { "metadata": { NS: { "k": 2 } } }
        } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &e1: null, &e2: null }), "{resp}");
    let since = resp["oldState"].clone();

    let args = json!({ "accountId": ACCOUNT_ID, "sinceState": since });
    let resp = handle_calendar_event_changes(&backend, &(), args, &using())
        .await
        .unwrap()
        .primary;
    let updated: Vec<&str> = resp["updated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(sorted(updated), sorted(vec![&e1, &e2]));
    assert_eq!(resp["updatedProperties"], json!(["metadata"]));
}

// ---------------------------------------------------------------------------
// Dispatcher wiring (-04 §3)
// ---------------------------------------------------------------------------

/// `register_calendars_handlers` forwards the request's `using`: -04 §3
/// "Foo/set MUST reject any write to them" without the capability, and the
/// same write succeeds with it.
#[tokio::test]
async fn dispatcher_forwards_using() {
    let backend = backend();
    let cal = calendar_fixture(&backend, "Work", true).await;
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_calendars_handlers(&mut dispatcher, Arc::new(backend));

    let call = json!([["Calendar/set", {
        "accountId": ACCOUNT_ID,
        "update": { &cal: { "metadata/acme.example.com": { "k": 1 } } }
    }, "0"]]);
    for (using, expect_ok) in [(using_without_metadata(), false), (using(), true)] {
        let request: JmapRequest =
            serde_json::from_value(json!({ "using": using, "methodCalls": call })).unwrap();
        let resp = dispatcher.dispatch(request, (), State::from("s0")).await;
        let args = &resp.method_responses[0].arguments;
        if expect_ok {
            assert_eq!(args["updated"], json!({ &cal: null }), "{args}");
        } else {
            assert_eq!(
                args["notUpdated"][&cal]["type"],
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

/// Run Calendar/get with `properties` on `id` and return its one list entry.
async fn calendar_get_one(
    b: &MemoryBackend,
    id: &str,
    properties: Value,
    using: &[String],
) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": properties });
    handle_calendar_get(b, &(), args, using)
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
async fn calendar_get_default_properties_follow_capability() {
    let b = backend();
    let bare = create_calendar(&b, json!({ "name": "bare" })).await;
    let tagged = create_calendar(
        &b,
        json!({ "name": "tagged", "metadata": json!({ NS: { "k": 1 } }) }),
    )
    .await;

    let got = calendar_get_one(&b, &bare, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({}), "{got}");
    assert!(got.get("privateMetadata").is_none(), "{got}");
    let got = calendar_get_one(&b, &tagged, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({ NS: { "k": 1 } }), "{got}");

    let without = using_without_metadata();
    let got = calendar_get_one(&b, &tagged, Value::Null, &without)
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");
    let props = json!(["id", "metadata", "privateMetadata"]);
    let got = calendar_get_one(&b, &tagged, props, &without)
        .await
        .unwrap();
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
async fn calendar_get_metadata_subselectors() {
    let b = backend();
    let id = create_calendar(
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
        let got = calendar_get_one(&b, &id, props.clone(), &using())
            .await
            .unwrap();
        assert_eq!(got["metadata"], want, "{props}: {got}");
    }

    let props = json!(["metadata/acme.example.com"]);
    let got = calendar_get_one(&b, &id, props, &using_without_metadata())
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");

    for props in [
        json!(["metadata/"]),
        json!(["metadata/acme.example.com/k"]),
        json!(["privateMetadata/"]),
    ] {
        let err = calendar_get_one(&b, &id, props.clone(), &using())
            .await
            .unwrap_err();
        assert_eq!(
            method_error_type(&err),
            json!("invalidArguments"),
            "{props}"
        );
    }
}

/// Run CalendarEvent/get with `properties` on `id` and return its one list entry.
async fn event_get_one(
    b: &MemoryBackend,
    id: &str,
    properties: Value,
    using: &[String],
) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": properties });
    handle_calendar_event_get(b, &(), args, using)
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
async fn event_get_default_properties_follow_capability() {
    let b = backend();
    let cal = create_calendar(&b, json!({ "name": "c" })).await;
    let bare = create_event(&b, &cal, json!({})).await;
    let tagged = create_event(&b, &cal, json!({ NS: { "k": 1 } })).await;

    let got = event_get_one(&b, &bare, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({}), "{got}");
    assert!(got.get("privateMetadata").is_none(), "{got}");
    let got = event_get_one(&b, &tagged, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({ NS: { "k": 1 } }), "{got}");

    let without = using_without_metadata();
    let got = event_get_one(&b, &tagged, Value::Null, &without)
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");
    let props = json!(["id", "metadata", "privateMetadata"]);
    let got = event_get_one(&b, &tagged, props, &without).await.unwrap();
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
async fn event_get_metadata_subselectors() {
    let b = backend();
    let cal = create_calendar(&b, json!({ "name": "c" })).await;
    let id = create_event(&b, &cal, json!({ NS: { "k": 1 }, NS2: { "k": 2 } })).await;
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
        let got = event_get_one(&b, &id, props.clone(), &using())
            .await
            .unwrap();
        assert_eq!(got["metadata"], want, "{props}: {got}");
    }

    let props = json!(["metadata/acme.example.com"]);
    let got = event_get_one(&b, &id, props, &using_without_metadata())
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");

    for props in [
        json!(["metadata/"]),
        json!(["metadata/acme.example.com/k"]),
        json!(["privateMetadata/"]),
    ] {
        let err = event_get_one(&b, &id, props.clone(), &using())
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
async fn event_metadata_filter_requires_capability() {
    let b = backend();
    let without = using_without_metadata();
    for filter in [
        json!({ "metadataExists": NS }),
        json!({ "operator": "NOT", "conditions": [json!({ "metadataExists": NS })] }),
    ] {
        let args = json!({ "accountId": ACCOUNT_ID, "filter": filter.clone() });
        let err = handle_calendar_event_query(&b, &(), args, &without)
            .await
            .unwrap_err();
        assert_eq!(
            method_error_type(&err),
            json!("unsupportedFilter"),
            "{filter}"
        );
        let args =
            json!({ "accountId": ACCOUNT_ID, "filter": filter.clone(), "sinceQueryState": "0" });
        let err = handle_calendar_event_query_changes(&b, &(), args, &without)
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
        handle_calendar_event_query(&b, &(), args, &using())
            .await
            .is_ok()
    );
}

/// The destination account of the /copy test.
const OTHER_ID: &str = "a2";

/// Run CalendarEvent/copy of one create entry from [`ACCOUNT_ID`] into
/// [`OTHER_ID`] and return the response.
async fn event_copy(b: &MemoryBackend, entry: Value, using: &[String]) -> Value {
    let args = json!({
        "fromAccountId": ACCOUNT_ID,
        "accountId": OTHER_ID,
        "create": { "k": entry },
    });
    handle_calendar_event_copy(b, &(), args, "c0", using)
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
async fn event_copy_metadata() {
    let b = backend().with_account(&Id::from(OTHER_ID));
    let cal = create_calendar(&b, json!({ "name": "c" })).await;
    let src = create_event(&b, &cal, json!({ NS: { "k": 1 } })).await;
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

    let resp = event_copy(&b, entry(json!({})), &using_without_metadata()).await;
    let (args, _) = dest_metadata(&resp);
    let got = handle_calendar_event_get(&b, &(), args, &using())
        .await
        .unwrap()
        .primary;
    assert_eq!(
        got["list"][0]["metadata"],
        json!({ NS: { "k": 1 } }),
        "{got}"
    );

    let resp = event_copy(
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

    let resp = event_copy(&b, entry(json!({ "metadata": null })), &using()).await;
    assert_eq!(
        resp["notCreated"]["k"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );

    let resp = event_copy(
        &b,
        entry(json!({ "metadata": { NS: { "k": 2 } } })),
        &using(),
    )
    .await;
    let (args, _) = dest_metadata(&resp);
    let got = handle_calendar_event_get(&b, &(), args, &using())
        .await
        .unwrap()
        .primary;
    assert_eq!(
        got["list"][0]["metadata"],
        json!({ NS: { "k": 2 } }),
        "{got}"
    );
}

/// -04 §3.4 (L657-660): "Copied values are not revalidated against the
/// destination account: data in a copied namespace that is not supported
/// there is handled as described in Section 2.1.1, and maxDepth applies
/// only to values supplied in the create object." §2.1.1 (L411-412): such
/// data "MUST continue to be returned by /get". The source event carries a
/// value of depth 3 (maxDepth is 2) and the registered namespace
/// `photography`, which the capability does not list; both are seeded,
/// since no /set may write them. The copy keeps both, while a create
/// object value deeper than maxDepth is still rejected (-04 §3.2 rule 4).
#[tokio::test]
async fn event_copy_does_not_revalidate_copied_metadata() {
    let b = backend().with_account(&Id::from(OTHER_ID));
    let cal = create_calendar(&b, json!({ "name": "c" })).await;
    let deep = json!({ "a": { "b": { "c": 1 } } });
    let copied = json!({ NS: deep, "photography": { "k": 1 } });
    b.seed_object(
        &Id::from(ACCOUNT_ID),
        "CalendarEvent",
        &Id::from("src"),
        json!({
            "@type": "Event", "id": "src", "uid": "u1", "title": "t",
            "calendarIds": { &cal: true }, "metadata": copied,
        }),
    );

    let resp = event_copy(&b, json!({ "id": "src" }), &using()).await;
    let id = resp["created"]["k"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("copy failed: {resp}"));
    let args = json!({ "accountId": OTHER_ID, "ids": [id], "properties": ["metadata"] });
    let got = handle_calendar_event_get(&b, &(), args, &using())
        .await
        .unwrap()
        .primary;
    assert_eq!(got["list"][0]["metadata"], copied, "{got}");

    let resp = event_copy(
        &b,
        json!({ "id": "src", "metadata": { NS: deep } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["k"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );
}
