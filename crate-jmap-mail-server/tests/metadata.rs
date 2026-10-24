//! draft-ietf-jmap-metadata-04 on Email and Mailbox in the in-memory
//! backend opted in with `MemoryBackend::with_metadata_info` (bd
//! JMAP-s4kp.1).
//!
//! Oracle: the -04 text quoted on each test
//! (/home/mark/PROJECT/IETF/specs/draft-ietf-jmap-metadata-04.txt, the
//! revision of October 2026), with hand-written requests and expected
//! responses. The capability below allows vendor namespaces only, no
//! private metadata, and `maxDepth` 2.

mod common;

use std::collections::BTreeMap;

use common::MemoryBackend;
use jmap_mail_server::{
    MailBackend, handle_email_changes, handle_email_copy, handle_email_get, handle_email_query,
    handle_email_set, handle_mailbox_changes, handle_mailbox_get, handle_mailbox_query,
    handle_mailbox_set,
};
use jmap_mail_types::Mailbox;
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_server::HandlerOutput;
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "a1";
/// The destination account of the Email/copy tests.
const OTHER_ID: &str = "a2";
const NS: &str = "acme.example.com";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:mail".to_owned(),
        "urn:ietf:params:jmap:metadata".to_owned(),
    ]
}

fn using_without_metadata() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:mail".to_owned(),
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
        .with_metadata_info("Mailbox", info())
        .with_metadata_info("Email", info());
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.register_account(&Id::from(OTHER_ID));
    backend
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

/// Store a Mailbox whose `myRights.maySetMetadata` is `may_set_metadata`
/// (server-set, so not writable through Mailbox/set) and return its id.
async fn mailbox_fixture(backend: &MemoryBackend, name: &str, may_set_metadata: bool) -> String {
    mailbox_fixture_in(backend, ACCOUNT_ID, name, may_set_metadata).await
}

/// [`mailbox_fixture`] in `account`.
async fn mailbox_fixture_in(
    backend: &MemoryBackend,
    account: &str,
    name: &str,
    may_set_metadata: bool,
) -> String {
    let name = name.to_owned();
    let (id, _) = MailBackend::create_object::<Mailbox, _>(
        backend,
        &(),
        &Id::from(account),
        "fixture",
        move |id| {
            serde_json::from_value(json!({
                "id": id,
                "name": name,
                "parentId": null,
                "role": null,
                "sortOrder": 0,
                "totalEmails": 0, "unreadEmails": 0, "totalThreads": 0, "unreadThreads": 0,
                "myRights": {
                    "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
                    "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
                    "mayRename": true, "mayDelete": true, "maySubmit": true,
                    "maySetMetadata": may_set_metadata
                },
                "isSubscribed": true
            }))
            .unwrap()
        },
    )
    .await
    .unwrap();
    id.to_string()
}

async fn mailbox_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } =
        handle_mailbox_set(backend, &(), &BTreeMap::new(), args, using)
            .await
            .expect("Mailbox/set must not fail at method level");
    primary
}

async fn email_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } =
        handle_email_set(backend, &(), &BTreeMap::new(), args, using)
            .await
            .expect("Email/set must not fail at method level");
    primary
}

/// Apply a Mailbox/set update that must succeed; returns the response.
async fn update_mailbox(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let resp = mailbox_set(backend, json!({ "update": { id: patch } }), &using()).await;
    assert_eq!(resp["updated"], json!({ id: null }), "{resp}");
    resp
}

async fn create_mailbox(backend: &MemoryBackend, create: Value) -> String {
    let resp = mailbox_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Mailbox/set create failed: {resp}"))
        .to_owned()
}

async fn create_email(backend: &MemoryBackend, mailbox: &str, metadata: Value) -> String {
    let create = json!({
        "mailboxIds": { mailbox: true },
        "subject": "s",
        "textBody": [{ "partId": "1", "type": "text/plain" }],
        "bodyValues": { "1": { "value": "b" } },
        "metadata": metadata,
    });
    let resp = email_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Email/set create failed: {resp}"))
        .to_owned()
}

async fn mailbox_metadata(backend: &MemoryBackend, id: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": ["metadata"] });
    let HandlerOutput { primary, .. } = handle_mailbox_get(backend, &(), args, &using())
        .await
        .unwrap();
    primary["list"][0]["metadata"].clone()
}

async fn mailbox_query(backend: &MemoryBackend, filter: Value) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "filter": filter });
    handle_mailbox_query(backend, &(), args, &using())
        .await
        .map(|o| o.primary["ids"].clone())
}

async fn email_query(backend: &MemoryBackend, filter: Value) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "filter": filter });
    handle_email_query(backend, &(), args, &using())
        .await
        .map(|o| o.primary["ids"].clone())
}

async fn mailbox_changes(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_mailbox_changes(backend, &(), args, using)
        .await
        .expect("Mailbox/changes must succeed")
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
async fn mailbox_query_metadata_exists() {
    let backend = backend();
    let red = create_mailbox(
        &backend,
        json!({ "name": "red", "metadata": { NS: { "color": "red" } } }),
    )
    .await;
    create_mailbox(&backend, json!({ "name": "empty", "metadata": { NS: {} } })).await;
    create_mailbox(&backend, json!({ "name": "none" })).await;

    assert_eq!(
        mailbox_query(&backend, json!({ "metadataExists": NS }))
            .await
            .unwrap(),
        json!([red])
    );
    assert_eq!(
        mailbox_query(
            &backend,
            json!({ "metadataExists": "acme.example.com/color" })
        )
        .await
        .unwrap(),
        json!([red])
    );
}

/// The -04 §6.7 example filter shape, with shared metadata: §3.6 conditions
/// "compose with the data type's existing filter conditions through the
/// usual AND/OR/NOT operators", and `metadataTextContains` is a substring
/// match "under the "i;unicode-casemap" collation", so case is ignored.
#[tokio::test]
async fn email_query_combines_mailbox_and_text_contains() {
    let backend = backend();
    let inbox = mailbox_fixture(&backend, "Inbox", true).await;
    let other = mailbox_fixture(&backend, "Other", true).await;
    let hit = create_email(
        &backend,
        &inbox,
        json!({ NS: { "memo": "Please FOLLOW UP soon" } }),
    )
    .await;
    create_email(&backend, &inbox, json!({ NS: { "memo": "done" } })).await;
    create_email(&backend, &other, json!({ NS: { "memo": "follow up" } })).await;

    let filter = json!({
        "operator": "AND",
        "conditions": [
            { "inMailbox": inbox },
            { "metadataTextContains": { "path": "acme.example.com/memo", "value": "follow up" } }
        ]
    });
    assert_eq!(email_query(&backend, filter).await.unwrap(), json!([hit]));
}

/// -04 §3.6 `metadataTextEquals`: "exactly equal to the supplied value,
/// comparing Unicode code points without case folding". Under NOT the
/// complement is returned.
#[tokio::test]
async fn email_query_text_equals_under_not() {
    let backend = backend();
    let inbox = mailbox_fixture(&backend, "Inbox", true).await;
    let exact = create_email(&backend, &inbox, json!({ NS: { "tag": "Urgent" } })).await;
    let other_case = create_email(&backend, &inbox, json!({ NS: { "tag": "urgent" } })).await;

    let equals =
        json!({ "metadataTextEquals": { "path": "acme.example.com/tag", "value": "Urgent" } });
    assert_eq!(
        email_query(&backend, equals.clone()).await.unwrap(),
        json!([exact])
    );
    assert_eq!(
        email_query(
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
/// for the data type."
#[tokio::test]
async fn private_condition_is_unsupported_filter() {
    let backend = backend();
    let err = email_query(
        &backend,
        json!({ "operator": "OR", "conditions": [{ "privateMetadataExists": NS }] }),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), json!("unsupportedFilter"));
}

/// -04 §3.6: "A path that is not well formed (for example, one with an
/// invalid ~ escape) MUST be rejected with an "invalidArguments" error."
/// RFC 6901 §3 allows only `~0` and `~1`.
#[tokio::test]
async fn malformed_path_is_invalid_arguments() {
    let backend = backend();
    let err = mailbox_query(
        &backend,
        json!({ "metadataExists": "acme.example.com/a~2" }),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), json!("invalidArguments"));
}

/// -04 §3.6: "A condition naming an unsupported namespace with no stored
/// data, or a string that is not a valid namespace identifier, matches no
/// objects; it does not produce an error."
#[tokio::test]
async fn unsupported_namespace_condition_matches_nothing() {
    let backend = backend();
    create_mailbox(
        &backend,
        json!({ "name": "m", "metadata": { NS: { "k": "v" } } }),
    )
    .await;
    assert_eq!(
        mailbox_query(&backend, json!({ "metadataExists": "photography" }))
            .await
            .unwrap(),
        json!([])
    );
}

// ---------------------------------------------------------------------------
// /set (-04 §3, §3.2, §4.1)
// ---------------------------------------------------------------------------

/// -04 §3.2 and the §6.3 example: a patch path below a namespace changes
/// that key and keeps the others; the create stores its metadata.
#[tokio::test]
async fn patch_within_namespace_keeps_other_keys() {
    let backend = backend();
    let id = create_mailbox(
        &backend,
        json!({ "name": "m", "metadata": { NS: { "color": "red", "icon": "star" } } }),
    )
    .await;
    update_mailbox(
        &backend,
        &id,
        json!({ "metadata/acme.example.com/color": "blue" }),
    )
    .await;
    assert_eq!(
        mailbox_metadata(&backend, &id).await,
        json!({ NS: { "color": "blue", "icon": "star" } })
    );
}

/// -04 §3.2: "In an update, null is equivalent to {}", and §2: when
/// returned, metadata's "value MUST be an object ({} when no metadata is
/// set), never null".
#[tokio::test]
async fn update_null_stores_empty_object() {
    let backend = backend();
    let id = create_mailbox(
        &backend,
        json!({ "name": "m", "metadata": { NS: { "k": 1 } } }),
    )
    .await;
    update_mailbox(&backend, &id, json!({ "metadata": null })).await;
    assert_eq!(mailbox_metadata(&backend, &id).await, json!({}));
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
    let resp = mailbox_set(
        &backend,
        json!({ "create": { "c": { "name": "n", "metadata": null } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));

    let id = create_mailbox(&backend, json!({ "name": "m" })).await;
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
        let resp = mailbox_set(&backend, json!({ "update": { &id: patch } }), using).await;
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
    assert_eq!(mailbox_metadata(&backend, &id).await, json!({}));
}

/// -04 §1.2.1: "A type that does not appear in this object does not gain
/// the metadata or privateMetadata properties in this account", so a
/// backend that did not opt in rejects the write.
#[tokio::test]
async fn unlisted_type_rejects_metadata_write() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let resp = mailbox_set(
        &backend,
        json!({ "create": { "c": { "name": "n", "metadata": { NS: {} } } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["c"]["properties"], json!(["metadata"]));
}

/// -04 §4.1: "A write to metadata without maySetMetadata MUST be rejected
/// with a "forbidden" SetError." A Mailbox carries its own right; other
/// properties stay writable.
#[tokio::test]
async fn mailbox_without_may_set_metadata_is_forbidden() {
    let backend = backend();
    let id = mailbox_fixture(&backend, "locked", false).await;
    let resp = mailbox_set(
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

    update_mailbox(&backend, &id, json!({ "name": "renamed" })).await;
}

/// -04 §4.1: an Email "takes the right from its containers. Modifying its
/// metadata requires maySetMetadata on every container it belongs to; when
/// the same update changes its containers, on every container both before
/// and after the change; and on create, on the containers given in the
/// create."
#[tokio::test]
async fn email_metadata_needs_right_on_every_container() {
    let backend = backend();
    let open = mailbox_fixture(&backend, "open", true).await;
    let locked = mailbox_fixture(&backend, "locked", false).await;
    let email = create_email(&backend, &open, json!({})).await;

    // Moving into a locked mailbox while writing metadata.
    let resp = email_set(
        &backend,
        json!({ "update": { &email: {
            format!("mailboxIds/{locked}"): true,
            "metadata/acme.example.com": { "k": 1 }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&email]["type"],
        json!("forbidden"),
        "{resp}"
    );

    // The same move without a metadata write is allowed.
    let resp = email_set(
        &backend,
        json!({ "update": { &email: { format!("mailboxIds/{locked}"): true } } }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!({ &email: null }), "{resp}");

    // Now in the locked mailbox: leaving it in the same update still needs
    // the right "before ... the change".
    let resp = email_set(
        &backend,
        json!({ "update": { &email: {
            format!("mailboxIds/{locked}"): null,
            "metadata/acme.example.com": { "k": 1 }
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&email]["type"],
        json!("forbidden"),
        "{resp}"
    );

    // On create.
    let create = json!({
        "mailboxIds": { &open: true, &locked: true },
        "subject": "s",
        "textBody": [{ "partId": "1", "type": "text/plain" }],
        "bodyValues": { "1": { "value": "b" } },
        "metadata": { NS: { "k": 1 } },
    });
    let resp = email_set(&backend, json!({ "create": { "c": create } }), &using()).await;
    assert_eq!(
        resp["notCreated"]["c"]["type"],
        json!("forbidden"),
        "{resp}"
    );
}

/// -04 §3.2 rule 1: "If the user cannot read the related object, reject
/// with "notFound". If the write targets metadata and the user lacks
/// maySetMetadata (Section 4.1), reject with "forbidden"" — in that order.
#[tokio::test]
async fn missing_object_is_not_found_before_forbidden() {
    let backend = backend();
    let resp = mailbox_set(
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
}

/// -04 §3.2: "Patch errors that can be determined from the request alone
/// (for example, two paths where one is a prefix of the other) MUST be
/// reported with "invalidPatch" before rule 1 is evaluated." Each patch
/// below would otherwise fail rule 1 (`notFound`, `forbidden`) or rule 2
/// (`privateMetadata` is unsupported here).
#[tokio::test]
async fn request_only_patch_error_precedes_rule_1() {
    let backend = backend();
    let locked = mailbox_fixture(&backend, "locked", false).await;
    let open = mailbox_fixture(&backend, "open", true).await;
    let email = create_email(&backend, &open, json!({})).await;

    for id in [locked.as_str(), "nope"] {
        let resp = mailbox_set(
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

    let resp = email_set(
        &backend,
        json!({ "update": { &email: {
            "privateMetadata": {},
            "privateMetadata/x": 1
        } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&email]["type"],
        json!("invalidPatch"),
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
async fn metadata_only_mailbox_change() {
    let backend = backend();
    let id = create_mailbox(&backend, json!({ "name": "m" })).await;
    let resp = update_mailbox(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    let new_state = resp["newState"].clone();

    let report = mailbox_changes(&backend, json!({ "sinceState": since }), &using()).await;
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
        let ignored = mailbox_changes(&backend, args, &using).await;
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
    let id = create_mailbox(&backend, json!({ "name": "m" })).await;
    let resp = update_mailbox(
        &backend,
        &id,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    update_mailbox(&backend, &id, json!({ "name": "n" })).await;

    for ignore in [false, true] {
        let resp = mailbox_changes(
            &backend,
            json!({ "sinceState": since, "ignoreMetadataOnlyChanges": ignore }),
            &using(),
        )
        .await;
        assert_eq!(resp["updated"], json!([id]), "{resp}");
        assert_eq!(resp["updatedProperties"], Value::Null, "{resp}");
    }
}

/// -04 §3.3: "Excluded ids do not count toward maxChanges."
#[tokio::test]
async fn excluded_ids_do_not_count_toward_max_changes() {
    let backend = backend();
    let quiet = create_mailbox(&backend, json!({ "name": "quiet" })).await;
    let loud = create_mailbox(&backend, json!({ "name": "loud" })).await;
    let resp = update_mailbox(
        &backend,
        &quiet,
        json!({ "metadata/acme.example.com": { "k": 1 } }),
    )
    .await;
    let since = resp["oldState"].clone();
    let resp = update_mailbox(&backend, &loud, json!({ "name": "louder" })).await;
    let new_state = resp["newState"].clone();

    let resp = mailbox_changes(
        &backend,
        json!({ "sinceState": since, "maxChanges": 1, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await;
    assert_eq!(resp["updated"], json!([loud]), "{resp}");
    assert_eq!(resp["hasMoreChanges"], json!(false), "{resp}");
    assert_eq!(resp["newState"], new_state);
}

/// -04 §3.3 on Email/changes: two Emails whose only change is metadata
/// give `updatedProperties` `["metadata"]`.
#[tokio::test]
async fn metadata_only_email_changes() {
    let backend = backend();
    let inbox = mailbox_fixture(&backend, "Inbox", true).await;
    let e1 = create_email(&backend, &inbox, json!({})).await;
    let e2 = create_email(&backend, &inbox, json!({})).await;
    let resp = email_set(
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
    let resp = handle_email_changes(&backend, &(), args, &using())
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
// /get (-04 §2, §3, §3.1)
// ---------------------------------------------------------------------------

/// Run a /get with `properties` on `id` in `account` and return its one
/// list entry.
async fn get_one(
    backend: &MemoryBackend,
    method: &str,
    account: &str,
    id: &str,
    properties: Value,
    using: &[String],
) -> Result<Value, JmapError> {
    let args = json!({ "accountId": account, "ids": [id], "properties": properties });
    let out = match method {
        "Mailbox/get" => handle_mailbox_get(backend, &(), args, using).await,
        _ => handle_email_get(backend, &(), args, using).await,
    };
    out.map(|o| o.primary["list"][0].clone())
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
async fn mailbox_get_default_properties_follow_capability() {
    let backend = backend();
    let bare = create_mailbox(&backend, json!({ "name": "bare" })).await;
    let tagged = create_mailbox(
        &backend,
        json!({ "name": "tagged", "metadata": { NS: { "k": 1 } } }),
    )
    .await;

    let got = get_one(
        &backend,
        "Mailbox/get",
        ACCOUNT_ID,
        &bare,
        Value::Null,
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(got["metadata"], json!({}), "{got}");
    assert!(got.get("privateMetadata").is_none(), "{got}");
    assert_eq!(got["name"], json!("bare"), "{got}");

    let got = get_one(
        &backend,
        "Mailbox/get",
        ACCOUNT_ID,
        &tagged,
        Value::Null,
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(got["metadata"], json!({ NS: { "k": 1 } }), "{got}");

    let without = using_without_metadata();
    let got = get_one(
        &backend,
        "Mailbox/get",
        ACCOUNT_ID,
        &tagged,
        Value::Null,
        &without,
    )
    .await
    .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");
    let got = get_one(
        &backend,
        "Mailbox/get",
        ACCOUNT_ID,
        &bare,
        json!(["metadata", "privateMetadata"]),
        &without,
    )
    .await
    .unwrap();
    assert_eq!(got, json!({ "id": &bare, "metadata": {} }));
}

/// -04 §3.1 (L477-478): the default "except for Email/get, whose default
/// property list (Section 4.2 of [RFC8621]) is unchanged"; by name it is
/// returned like any other property (L478-480).
#[tokio::test]
async fn email_get_default_properties_unchanged() {
    let backend = backend();
    let mailbox = create_mailbox(&backend, json!({ "name": "m" })).await;
    let email = create_email(&backend, &mailbox, json!({ NS: { "k": 1 } })).await;

    let got = get_one(
        &backend,
        "Email/get",
        ACCOUNT_ID,
        &email,
        Value::Null,
        &using(),
    )
    .await
    .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");
    assert_eq!(got["subject"], json!("s"), "{got}");

    let got = get_one(
        &backend,
        "Email/get",
        ACCOUNT_ID,
        &email,
        json!(["metadata"]),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(got, json!({ "id": &email, "metadata": { NS: { "k": 1 } } }));
}

/// -04 §3.1 (L493-500): "Multiple subselectors with the same root combine
/// by union. For example, ["metadata/x", "metadata/y"] returns
/// {"metadata": {"x": ..., "y": ...}} (and nothing else under metadata)"
/// and "An explicit root entry supersedes subselectors with the same root.
/// For example, ["metadata", "metadata/x"] returns the complete metadata
/// object".
#[tokio::test]
async fn get_subselectors_union_and_root_supersedes() {
    let backend = backend();
    let all = json!({
        "x.example.com": { "a": 1 },
        "y.example.com": { "b": 2 },
        "z.example.com": { "c": 3 }
    });
    let mailbox = create_mailbox(&backend, json!({ "name": "m", "metadata": all })).await;
    let email = create_email(&backend, &mailbox, all.clone()).await;

    for (method, id) in [("Mailbox/get", &mailbox), ("Email/get", &email)] {
        let got = get_one(
            &backend,
            method,
            ACCOUNT_ID,
            id,
            json!(["metadata/x.example.com", "metadata/y.example.com"]),
            &using(),
        )
        .await
        .unwrap();
        assert_eq!(
            got,
            json!({ "id": id, "metadata": {
                "x.example.com": { "a": 1 },
                "y.example.com": { "b": 2 }
            } }),
            "{method}"
        );

        let got = get_one(
            &backend,
            method,
            ACCOUNT_ID,
            id,
            json!(["metadata", "metadata/x.example.com"]),
            &using(),
        )
        .await
        .unwrap();
        assert_eq!(got, json!({ "id": id, "metadata": all }), "{method}");
    }
}

/// -04 §3.1 (L487-491): "A subselector has the form metadata/<namespace>
/// or privateMetadata/<namespace>, where <namespace> is a non-empty
/// namespace identifier. Any other properties entry beginning with
/// metadata/ or privateMetadata/ MUST be rejected with an
/// "invalidArguments" method-level error." §2.1 (L346-347): domain names
/// "MUST be in lowercase".
#[tokio::test]
async fn get_malformed_subselector_is_invalid_arguments() {
    let backend = backend();
    let mailbox = create_mailbox(&backend, json!({ "name": "m" })).await;
    let email = create_email(&backend, &mailbox, json!({})).await;
    for (method, id) in [("Mailbox/get", &mailbox), ("Email/get", &email)] {
        for bad in [
            "metadata/",
            "privateMetadata/",
            "metadata/acme.example.com/k",
            "metadata/Acme.Example.COM",
        ] {
            let err = get_one(&backend, method, ACCOUNT_ID, id, json!([bad]), &using())
                .await
                .expect_err(bad);
            assert_eq!(error_type(err), json!("invalidArguments"), "{method} {bad}");
        }
    }
}

/// -04 §3.1 (L509-512): "A subselector naming a namespace that is not
/// supported (Section 2.1) on the data type MUST be silently omitted from
/// the response, unless the object has data stored under that namespace
/// (Section 2.1.1), which is then returned." Here the data is stored
/// under a vendor namespace and the server is then reconfigured to
/// support only the registered name `photography` (§2.1.1 L408-409:
/// "after a server reconfiguration").
#[tokio::test]
async fn get_unsupported_namespace_subselector() {
    let backend = backend();
    let mailbox = create_mailbox(
        &backend,
        json!({ "name": "m", "metadata": { NS: { "k": 1 } } }),
    )
    .await;
    let narrowed: DataTypeMetadataInfo = serde_json::from_value(json!({
        "namespaces": ["photography"],
        "supportsVendorNamespaces": false,
        "supportsPrivate": false,
        "maxDepth": 2
    }))
    .unwrap();
    let backend = backend.with_metadata_info("Mailbox", narrowed);

    let got = get_one(
        &backend,
        "Mailbox/get",
        ACCOUNT_ID,
        &mailbox,
        json!(["metadata/acme.example.com", "metadata/other.example.org"]),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(
        got,
        json!({ "id": &mailbox, "metadata": { NS: { "k": 1 } } })
    );
}

// ---------------------------------------------------------------------------
// /copy (-04 §3.4)
// ---------------------------------------------------------------------------

/// Email/copy one Email from [`ACCOUNT_ID`] to [`OTHER_ID`] with the
/// create object `create`; returns the response.
async fn email_copy(backend: &MemoryBackend, create: Value, using: &[String]) -> Value {
    let args = json!({
        "fromAccountId": ACCOUNT_ID,
        "accountId": OTHER_ID,
        "create": { "c": create }
    });
    handle_email_copy(backend, &(), &Default::default(), args, "call", using)
        .await
        .expect("Email/copy must not fail at method level")
        .primary
}

/// The `metadata` of the Email `copy_resp` created, read in [`OTHER_ID`].
async fn copied_metadata(backend: &MemoryBackend, copy_resp: &Value) -> Value {
    let id = copy_resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Email/copy failed: {copy_resp}"));
    get_one(
        backend,
        "Email/get",
        OTHER_ID,
        id,
        json!(["metadata"]),
        &using(),
    )
    .await
    .unwrap()["metadata"]
        .clone()
}

/// -04 §3.4 (L649, L653-655): "Foo/copy copies metadata to the new
/// object", and "The create object MAY include metadata and
/// privateMetadata to override the copied values, with the same rules as
/// a create in Foo/set"; L650-653: "When the request does not use this
/// capability, only metadata is copied."
#[tokio::test]
async fn email_copy_keeps_metadata_and_takes_override() {
    let backend = backend();
    let mailbox = create_mailbox(&backend, json!({ "name": "m" })).await;
    let email = create_email(&backend, &mailbox, json!({ NS: { "k": 1 } })).await;
    let target = mailbox_fixture_in(&backend, OTHER_ID, "t", true).await;

    let plain = json!({ "id": &email, "mailboxIds": { &target: true } });
    let resp = email_copy(&backend, plain.clone(), &using()).await;
    assert_eq!(
        copied_metadata(&backend, &resp).await,
        json!({ NS: { "k": 1 } })
    );
    let resp = email_copy(&backend, plain, &using_without_metadata()).await;
    assert_eq!(
        copied_metadata(&backend, &resp).await,
        json!({ NS: { "k": 1 } })
    );

    let resp = email_copy(
        &backend,
        json!({
            "id": &email,
            "mailboxIds": { &target: true },
            "metadata": { "other.example.org": { "n": 2 } }
        }),
        &using(),
    )
    .await;
    assert_eq!(
        copied_metadata(&backend, &resp).await,
        json!({ "other.example.org": { "n": 2 } })
    );
}

/// The Foo/set create rules an Email/copy override follows (-04 §3.4
/// L653-655): §3 (L463-464) "Foo/set MUST reject any write to them with an
/// "invalidProperties" SetError" without the capability; §3.2 (L527-528)
/// "A value of null for either property MUST be rejected with an
/// "invalidProperties" SetError"; rule 2 (L569-570) privateMetadata when
/// supportsPrivate is false; rule 3 (L572-573) an unsupported namespace;
/// rule 4 (L576-577) maxDepth, here 2; and §4.1 / rule 1 (L566-567)
/// "forbidden" without maySetMetadata on the container given in the
/// create.
#[tokio::test]
async fn email_copy_override_follows_set_create_rules() {
    let backend = backend();
    let mailbox = create_mailbox(&backend, json!({ "name": "m" })).await;
    let email = create_email(&backend, &mailbox, json!({})).await;
    let open = mailbox_fixture_in(&backend, OTHER_ID, "open", true).await;
    let locked = mailbox_fixture_in(&backend, OTHER_ID, "locked", false).await;

    let cases = [
        (
            "metadata",
            json!({ NS: {} }),
            using_without_metadata(),
            &open,
            "invalidProperties",
        ),
        ("metadata", Value::Null, using(), &open, "invalidProperties"),
        (
            "privateMetadata",
            json!({ NS: {} }),
            using(),
            &open,
            "invalidProperties",
        ),
        (
            "metadata",
            json!({ "photography": {} }),
            using(),
            &open,
            "invalidProperties",
        ),
        (
            "metadata",
            json!({ NS: { "a": { "b": { "c": 1 } } } }),
            using(),
            &open,
            "invalidProperties",
        ),
        (
            "metadata",
            json!({ NS: { "k": 1 } }),
            using(),
            &locked,
            "forbidden",
        ),
    ];
    for (property, value, using, target, expected) in cases {
        let resp = email_copy(
            &backend,
            json!({ "id": &email, "mailboxIds": { target: true }, property: value }),
            &using,
        )
        .await;
        assert_eq!(
            resp["notCreated"]["c"]["type"],
            json!(expected),
            "{property} {value}: {resp}"
        );
    }

    // Copying into the locked mailbox without an override writes no
    // metadata from the create object.
    let resp = email_copy(
        &backend,
        json!({ "id": &email, "mailboxIds": { &locked: true } }),
        &using(),
    )
    .await;
    assert_eq!(copied_metadata(&backend, &resp).await, json!({}));
}

// ---------------------------------------------------------------------------
// /query without the capability (-04 §3)
// ---------------------------------------------------------------------------

/// -04 §3 (L460-461): the method extensions, among them the §3.6 filter
/// conditions, "apply to requests whose using includes
/// urn:ietf:params:jmap:metadata". Without it the condition is not part
/// of the FilterCondition, so RFC 8620 §5.5 "unsupportedFilter: The
/// "filter" is syntactically valid, but the server cannot process it"
/// applies, also when the leaf is nested under an operator.
#[tokio::test]
async fn query_without_capability_is_unsupported_filter() {
    let backend = backend();
    let filter = json!({ "operator": "AND", "conditions": [{ "metadataExists": NS }] });
    let without = using_without_metadata();
    let args = json!({ "accountId": ACCOUNT_ID, "filter": filter });
    let err = handle_mailbox_query(&backend, &(), args.clone(), &without)
        .await
        .expect_err("Mailbox/query");
    assert_eq!(error_type(err), json!("unsupportedFilter"));
    let err = handle_email_query(&backend, &(), args, &without)
        .await
        .expect_err("Email/query");
    assert_eq!(error_type(err), json!("unsupportedFilter"));
}
