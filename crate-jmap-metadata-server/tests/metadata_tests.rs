//! Integration tests for Metadata/* method handlers
//! (draft-ietf-jmap-metadata-01 §3).
//!
//! Each test dispatches through `register_metadata_handlers` with the
//! in-memory `MemoryBackend`; results are asserted against the draft's
//! wire-format examples.

mod common;

use std::sync::Arc;

use jmap_metadata_server::{
    register_metadata_handlers, JmapBackend, MetadataBackend, JMAP_METADATA_URI,
};
use jmap_metadata_types::Metadata;
use jmap_server::{Dispatcher, JmapRequest, State};
use jmap_types::Id;
use serde_json::json;

use common::MemoryBackend;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal [`JmapRequest`] with a single method call carrying the
/// Metadata capability URI in `using`.
fn single_call(method: &str, args: serde_json::Value, call_id: &str) -> JmapRequest {
    JmapRequest::new(
        vec![JMAP_METADATA_URI.into()],
        vec![(method.into(), args, call_id.into())],
        None,
    )
}

/// Seed a [`Metadata`] object into the backend by calling `create_object`
/// directly (bypassing dispatcher). Returns the server-assigned [`Id`].
async fn seed_metadata(backend: &MemoryBackend, account_id: &str, v: serde_json::Value) -> Id {
    let meta: Metadata = serde_json::from_value(v).expect("test fixture must deserialize");
    let (id, _) = backend
        .create_object::<Metadata>(&(), &Id::from(account_id), "seed", meta)
        .await
        .expect("seed must succeed");
    id
}

// ---------------------------------------------------------------------------
// Metadata/get tests
// ---------------------------------------------------------------------------

/// Oracle: draft §3.2 — `Metadata/get` with `ids: null` returns all
/// Metadata objects in the account.
#[tokio::test]
async fn get_all_returns_full_list() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "acme.example.com:color": "blue"
        }),
    )
    .await;
    seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "ImapMetadata",
            "relatedType": "Mailbox",
            "relatedId": "MB1",
            "metadata": {"comment": "Team mailbox"}
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/get",
        json!({"accountId": "acc1", "ids": null}),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;

    assert_eq!(resp.method_responses.len(), 1);
    let (_, args, _) = &resp.method_responses[0];
    let list = args["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 2, "must return both seeded objects: {args}");
}

/// Oracle: §3.2 — `Metadata/get` with a non-existent id returns it in
/// `notFound` per RFC 8620 §5.1.
#[tokio::test]
async fn get_unknown_id_in_not_found() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/get",
        json!({"accountId": "acc1", "ids": ["md-does-not-exist"]}),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];
    let not_found = args["notFound"].as_array().expect("notFound must be array");
    assert_eq!(not_found.len(), 1);
    assert_eq!(not_found[0], "md-does-not-exist");
}

// ---------------------------------------------------------------------------
// Metadata/set tests
// ---------------------------------------------------------------------------

/// Oracle: draft §3.1 — a valid `Metadata/set` create returns the
/// server-assigned object in `created`. Vendor properties survive the
/// round-trip via the workspace extras-preservation policy.
#[tokio::test]
async fn set_create_annotation_returns_server_id_and_vendor_props() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/set",
        json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "@type": "Annotation",
                    "relatedType": "Email",
                    "relatedId": "EM1",
                    "isPrivate": true,
                    "acme.example.com:workflowState": "pending-review"
                }
            }
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    assert!(
        args["created"].is_object(),
        "created must be object: {args}"
    );
    let c1 = &args["created"]["c1"];
    assert!(c1.get("id").is_some(), "server id must be set: {c1}");
    assert_eq!(c1["@type"], "Annotation");
    assert_eq!(c1["relatedType"], "Email");
    assert_eq!(c1["isPrivate"], true);
    assert_eq!(c1["acme.example.com:workflowState"], "pending-review");
}

/// Oracle: draft §3.1 — duplicate (relatedType, relatedId, @type,
/// isPrivate) tuple produces `alreadyExists` with `existingId` pointing
/// at the conflicting object.
#[tokio::test]
async fn set_create_duplicate_returns_already_exists() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let first_id = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": false
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/set",
        json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "@type": "Annotation",
                    "relatedType": "Email",
                    "relatedId": "EM1",
                    "isPrivate": false
                }
            }
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let c1 = &args["notCreated"]["c1"];
    assert_eq!(c1["type"], "alreadyExists", "must be alreadyExists: {args}");
    assert_eq!(
        c1["existingId"],
        json!(first_id.as_ref()),
        "existingId must point at first object: {c1}",
    );
}

/// Oracle: draft §3.1 + RFC 8620 §5.3 — destroying an existing Metadata
/// id puts it in the `destroyed` response array.
#[tokio::test]
async fn set_destroy_existing_succeeds() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let id = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/set",
        json!({"accountId": "acc1", "destroy": [id.as_ref()]}),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let destroyed = args["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0], json!(id.as_ref()));
}

/// Oracle: RFC 8620 §3.6.2 — a method call carrying an `accountId` the
/// server does not recognise MUST return the method-level error
/// `accountNotFound` and MUST NOT mutate backend state. Regression
/// test for bd:JMAP-ayoz.1 (handler-level guard) and bd:JMAP-ayoz.2
/// (backend-level guard): prior to the fix, `Metadata/set` against an
/// unknown account silently auto-registered the account in
/// `known_accounts` and proceeded with `create`.
#[tokio::test]
async fn set_unknown_account_returns_account_not_found_and_does_not_mutate() {
    // Backend is seeded with "acc1" only; "acc-bogus" is unknown.
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    // Pre-condition: bogus account is not known.
    assert!(
        !backend
            .account_exists(&(), &Id::from("acc-bogus"))
            .await
            .expect("account_exists must succeed"),
        "pre-condition: acc-bogus must not be known",
    );

    let req = single_call(
        "Metadata/set",
        json!({
            "accountId": "acc-bogus",
            "create": {
                "c1": {
                    "@type": "Annotation",
                    "relatedType": "Email",
                    "relatedId": "EM1"
                }
            }
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;

    assert_eq!(resp.method_responses.len(), 1);
    let (_, args, _) = &resp.method_responses[0];
    assert_eq!(
        args["type"], "accountNotFound",
        "unknown accountId must produce accountNotFound; got: {args}",
    );

    // Post-condition: the bogus account was NOT silently registered.
    assert!(
        !backend
            .account_exists(&(), &Id::from("acc-bogus"))
            .await
            .expect("account_exists must succeed"),
        "backend must not auto-register unknown accountId during /set",
    );
}

// ---------------------------------------------------------------------------
// Metadata/changes tests
// ---------------------------------------------------------------------------

/// Oracle: draft §3.3 — `Metadata/changes` from state "0" returns every
/// created object's id in `created`.
#[tokio::test]
async fn changes_from_zero_returns_all_created() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let id1 = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;
    let id2 = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "ImapMetadata", "relatedType": "Mailbox", "relatedId": "MB1", "metadata": {}}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/changes",
        json!({"accountId": "acc1", "sinceState": "0"}),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let created: Vec<&str> = args["created"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(created.contains(&id1.as_ref()), "id1 must appear: {args}");
    assert!(created.contains(&id2.as_ref()), "id2 must appear: {args}");
}

/// Oracle: draft §3.3 — `filterRelatedType: "Email"` retains only
/// Metadata objects whose `relatedType == "Email"` in the `created`
/// array, while leaving the state token unfiltered.
#[tokio::test]
async fn changes_filter_related_type_drops_non_matching() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let email_id = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;
    let mailbox_id = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Mailbox", "relatedId": "MB1"}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/changes",
        json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterRelatedType": "Email"
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let created: Vec<&str> = args["created"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        created.contains(&email_id.as_ref()),
        "Email-related id must survive filter: {args}",
    );
    assert!(
        !created.contains(&mailbox_id.as_ref()),
        "Mailbox-related id must be dropped: {args}",
    );

    // State token reflects the complete state — not affected by the filter.
    // After two creates the state counter is "2".
    assert_eq!(
        args["newState"], "2",
        "state token must NOT be filtered: {args}",
    );
}

/// Oracle: draft §3.3 — `filterMetadataType: ["ImapMetadata"]` retains
/// only objects whose `@type` is in the list.
#[tokio::test]
async fn changes_filter_metadata_type_drops_non_matching() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let ann_id = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;
    let imap_id = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "ImapMetadata", "relatedType": "Mailbox", "relatedId": "MB1", "metadata": {}}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/changes",
        json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterMetadataType": ["ImapMetadata"]
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let created: Vec<&str> = args["created"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        created.contains(&imap_id.as_ref()),
        "ImapMetadata must survive: {args}",
    );
    assert!(
        !created.contains(&ann_id.as_ref()),
        "Annotation must be dropped: {args}",
    );
}

// ---------------------------------------------------------------------------
// Dispatcher registration sanity
// ---------------------------------------------------------------------------

/// Oracle: every method registered by `register_metadata_handlers` is
/// recognised by the dispatcher (no `unknownMethod` for any of the five).
#[tokio::test]
async fn all_five_methods_registered() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let methods = [
        ("Metadata/get", json!({"accountId": "acc1", "ids": null})),
        (
            "Metadata/changes",
            json!({"accountId": "acc1", "sinceState": "0"}),
        ),
        ("Metadata/set", json!({"accountId": "acc1", "destroy": []})),
        (
            "Metadata/query",
            json!({"accountId": "acc1", "filter": null, "sort": null}),
        ),
        (
            "Metadata/queryChanges",
            json!({"accountId": "acc1", "sinceQueryState": "0"}),
        ),
    ];

    for (method, args) in methods {
        let req = single_call(method, args, "c0");
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        assert_eq!(
            resp.method_responses.len(),
            1,
            "{method}: expected 1 response"
        );
        let (_, resp_args, _) = &resp.method_responses[0];
        assert_ne!(
            resp_args["type"], "unknownMethod",
            "{method}: must be registered (was: {resp_args})",
        );
    }
}

// ---------------------------------------------------------------------------
// Metadata/changes strict §3.3 conformance — destroyed-array filtering
//
// These tests exercise the `MemoryBackend::get_metadata_changes` override
// (bd:JMAP-06zp.3.5.2) which pre-filters all three arrays at the storage
// layer using the per-record (related_type, type_name) snapshot captured
// at mutation time. The default trait impl on `MetadataBackend` cannot
// filter `destroyed` because destroyed objects no longer exist for
// post-fetch inspection — so these tests would fail against a
// default-impl backend (e.g. MockBackend in the unit tests).
// ---------------------------------------------------------------------------

/// Oracle: draft §3.3 — `filterRelatedType: "Email"` must drop a
/// destroyed Metadata whose `relatedType` was "Mailbox" while retaining
/// a destroyed Metadata whose `relatedType` was "Email".
///
/// Verifies strict-conformance of the destroyed array against the
/// override (the default impl cannot honor this — see module-level note).
#[tokio::test]
async fn changes_filter_drops_non_matching_destroyed() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let email_id = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;
    let mailbox_id = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Mailbox", "relatedId": "MB1"}),
    )
    .await;

    // Destroy both. The (related_type, type_name) tuple is captured at
    // destroy time by the override's ChangeRecord — without that
    // snapshot the override could not filter the destroyed array.
    backend
        .destroy_object::<Metadata>(&(), &Id::from("acc1"), &email_id)
        .await
        .expect("destroy email-related must succeed");
    backend
        .destroy_object::<Metadata>(&(), &Id::from("acc1"), &mailbox_id)
        .await
        .expect("destroy mailbox-related must succeed");

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    // `sinceState: "2"` captures the state AFTER the two seeds and
    // BEFORE the two destroys — so the /changes window contains only
    // the destroy events. Using `sinceState: "0"` would put both the
    // create and the destroy for each id in the same window, which
    // under the RFC 8620 §5.2 SHOULD-conformant IdFate algorithm
    // (bd:JMAP-826m.8) omits the id from both `created` and
    // `destroyed`. The test's intent is destroyed-array filtering,
    // not create-then-destroy precedence, so we sidestep the omit-both
    // path here by separating the seed and destroy windows.
    let req = single_call(
        "Metadata/changes",
        json!({
            "accountId": "acc1",
            "sinceState": "2",
            "filterRelatedType": "Email"
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let destroyed: Vec<&str> = args["destroyed"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        destroyed.contains(&email_id.as_ref()),
        "Email-related destroyed id must survive filter: {args}",
    );
    assert!(
        !destroyed.contains(&mailbox_id.as_ref()),
        "Mailbox-related destroyed id must be dropped under strict §3.3: {args}",
    );

    // State counter is filter-independent: 2 creates + 2 destroys = "4".
    assert_eq!(
        args["newState"], "4",
        "state token must NOT be filtered: {args}",
    );
}

/// Oracle: draft §3.3 — `filterRelatedType` and `filterMetadataType`
/// combine with logical AND across all three change arrays.
///
/// Setup: four Metadata creates spanning the cross-product of
/// `relatedType in {Email, Mailbox}` × `@type in {Annotation,
/// ImapMetadata}`. Filter on `relatedType: Email` AND
/// `@type: Annotation` and assert only the one matching id survives in
/// `created`. Then destroy all four and re-assert the AND combination
/// applies identically to `destroyed`.
#[tokio::test]
async fn changes_filter_combined_related_type_and_metadata_type() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));

    // Four creates spanning the (relatedType × @type) cross-product.
    let email_ann = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;
    let email_imap = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "ImapMetadata", "relatedType": "Email", "relatedId": "EM2", "metadata": {}}),
    )
    .await;
    let mailbox_ann = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Mailbox", "relatedId": "MB1"}),
    )
    .await;
    let mailbox_imap = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "ImapMetadata", "relatedType": "Mailbox", "relatedId": "MB2", "metadata": {}}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    // AND filter on created.
    let req = single_call(
        "Metadata/changes",
        json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterRelatedType": "Email",
            "filterMetadataType": ["Annotation"]
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];
    let created: Vec<&str> = args["created"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        created.len(),
        1,
        "AND filter must yield exactly one id: {args}",
    );
    assert!(
        created.contains(&email_ann.as_ref()),
        "only Email + Annotation must survive: {args}",
    );

    // Destroy all four, then re-run /changes from `sinceState: "4"`
    // (the state after all four seeds, before any destroy) with the
    // same AND filter. The destroyed array must show only the
    // (Email, Annotation) id.
    //
    // We use `sinceState: "4"` rather than `"0"` because under the
    // RFC 8620 §5.2 SHOULD-conformant IdFate algorithm (bd:JMAP-826m.8)
    // a create followed by a destroy within the same /changes window
    // is omitted from both `created` and `destroyed`. The test's intent
    // is destroyed-array filtering (AND combination on a destroyed
    // record's `relatedType` and `@type`), not create-then-destroy
    // precedence, so the destroys-only window isolates the filter
    // behavior cleanly.
    for id in [&email_ann, &email_imap, &mailbox_ann, &mailbox_imap] {
        backend
            .destroy_object::<Metadata>(&(), &Id::from("acc1"), id)
            .await
            .expect("destroy must succeed");
    }

    let req = single_call(
        "Metadata/changes",
        json!({
            "accountId": "acc1",
            "sinceState": "4",
            "filterRelatedType": "Email",
            "filterMetadataType": ["Annotation"]
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let created: Vec<&str> = args["created"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let destroyed: Vec<&str> = args["destroyed"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        created.is_empty(),
        "destroys-only window must have no creates: {args}",
    );
    assert_eq!(
        destroyed.len(),
        1,
        "AND filter on destroyed must yield exactly one id: {args}",
    );
    assert!(
        destroyed.contains(&email_ann.as_ref()),
        "only (Email, Annotation) must survive on destroyed: {args}",
    );
}

// ---------------------------------------------------------------------------
// Metadata/query filter and sort tests (bd:JMAP-826m.4)
//
// MemoryBackend::query_objects previously discarded `_filter` and `_sort`,
// returning every stored object regardless of the client's filter. The
// fix walks the in-memory store, deserialises each entry into Metadata,
// and applies the typed filter / sort.
// ---------------------------------------------------------------------------

/// Oracle: §3.4.1 — `Metadata/query` with `filter: {relatedType: "Email"}`
/// returns only objects whose `relatedType` is `"Email"`. Before the fix,
/// the Mailbox-related annotation would have been incorrectly included.
#[tokio::test]
async fn query_filter_related_type_returns_only_matches() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let email_ann = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }),
    )
    .await;
    let _mailbox_ann = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Mailbox",
            "relatedId": "MB1"
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({
            "accountId": "acc1",
            "filter": {"relatedType": "Email"}
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids = args["ids"].as_array().expect("ids must be array");
    assert_eq!(
        ids.len(),
        1,
        "filter relatedType=Email must yield one id: {args}"
    );
    assert_eq!(ids[0], email_ann.as_ref());
}

/// Oracle: §3.4.1 — `filter: {"@type": ["Annotation"]}` returns only
/// objects whose `@type` is in the list.
#[tokio::test]
async fn query_filter_type_names_returns_only_matches() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let ann = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }),
    )
    .await;
    let _imap = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "ImapMetadata",
            "relatedType": "Mailbox",
            "relatedId": "MB1",
            "metadata": {"k": "v"}
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({
            "accountId": "acc1",
            "filter": {"@type": ["Annotation"]}
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids = args["ids"].as_array().expect("ids must be array");
    assert_eq!(
        ids.len(),
        1,
        "filter @type=[Annotation] must yield one id: {args}"
    );
    assert_eq!(ids[0], ann.as_ref());
}

/// Oracle: §3.4.1 — `filter: {relatedType: "Email", relatedIds: [...]}`
/// returns only objects whose `relatedId` is in the list AND whose
/// `relatedType` matches. The cross-field constraint is satisfied
/// (relatedIds with relatedType).
#[tokio::test]
async fn query_filter_related_ids_returns_only_matches() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let em1 = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }),
    )
    .await;
    let _em2 = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM2"
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({
            "accountId": "acc1",
            "filter": {"relatedType": "Email", "relatedIds": ["EM1"]}
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids = args["ids"].as_array().expect("ids must be array");
    assert_eq!(
        ids.len(),
        1,
        "filter relatedIds=[EM1] must yield one id: {args}"
    );
    assert_eq!(ids[0], em1.as_ref());
}

/// Oracle: §3.4.1 — `filter: {isPrivate: true}` returns only objects
/// whose `isPrivate` is `true`. Default-false objects are excluded.
#[tokio::test]
async fn query_filter_is_private_returns_only_matches() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let priv_ann = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": true
        }),
    )
    .await;
    let _pub_ann = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM2",
            "isPrivate": false
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({
            "accountId": "acc1",
            "filter": {"isPrivate": true}
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids = args["ids"].as_array().expect("ids must be array");
    assert_eq!(
        ids.len(),
        1,
        "filter isPrivate=true must yield one id: {args}"
    );
    assert_eq!(ids[0], priv_ann.as_ref());
}

/// Oracle: §3.4.1 — `filter: {textMatch: "review"}` matches the
/// vendor-string property containing the needle case-insensitively.
/// Annotations whose `.extra` does not contain the needle are excluded.
#[tokio::test]
async fn query_filter_text_match_searches_vendor_string_props() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let hit = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "acme.example.com:workflow": "Pending Review"
        }),
    )
    .await;
    let _miss = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM2",
            "acme.example.com:workflow": "approved"
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({
            "accountId": "acc1",
            "filter": {"textMatch": "REVIEW"}
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids = args["ids"].as_array().expect("ids must be array");
    assert_eq!(
        ids.len(),
        1,
        "case-insensitive textMatch must yield one id: {args}"
    );
    assert_eq!(ids[0], hit.as_ref());
}

/// Oracle: §3.4.2 — `sort: [{property: "relatedType", isAscending: true}]`
/// orders the results by `relatedType`. Hand-built oracle: seed three
/// records in non-alphabetical order, expect them back in alphabetical
/// order of `relatedType`.
#[tokio::test]
async fn query_sort_related_type_ascending() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    // Seed in non-alphabetical order to defeat any insertion-order luck.
    let mailbox = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Mailbox",
            "relatedId": "MB1"
        }),
    )
    .await;
    let calendar = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Calendar",
            "relatedId": "CAL1"
        }),
    )
    .await;
    let email = seed_metadata(
        &backend,
        "acc1",
        json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({
            "accountId": "acc1",
            "sort": [{"property": "relatedType", "isAscending": true}]
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids: Vec<&str> = args["ids"]
        .as_array()
        .expect("ids must be array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    // Calendar < Email < Mailbox alphabetically.
    assert_eq!(
        ids,
        vec![calendar.as_ref(), email.as_ref(), mailbox.as_ref()],
        "sort by relatedType asc must order Calendar < Email < Mailbox: {args}",
    );
}

/// Oracle: §3.4.2 — `sort: [{property: "relatedType", isAscending: false}]`
/// reverses the order. Negative control for the `isAscending` flag.
#[tokio::test]
async fn query_sort_related_type_descending() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mailbox = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Mailbox", "relatedId": "MB1"}),
    )
    .await;
    let email = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({
            "accountId": "acc1",
            "sort": [{"property": "relatedType", "isAscending": false}]
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids: Vec<&str> = args["ids"]
        .as_array()
        .expect("ids must be array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![mailbox.as_ref(), email.as_ref()],
        "descending must invert: {args}",
    );
}

/// Oracle: filter AND sort compose. Filter to relatedType=Email, sort by
/// relatedId ascending. Records that should not survive the filter must
/// not appear in the sort.
#[tokio::test]
async fn query_filter_and_sort_compose() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let _mailbox = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Mailbox", "relatedId": "MB1"}),
    )
    .await;
    let em2 = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM2"}),
    )
    .await;
    let em1 = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({
            "accountId": "acc1",
            "filter": {"relatedType": "Email"},
            "sort": [{"property": "relatedId", "isAscending": true}]
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids: Vec<&str> = args["ids"]
        .as_array()
        .expect("ids must be array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![em1.as_ref(), em2.as_ref()],
        "filter (Email only) + sort (relatedId asc) must yield EM1 then EM2: {args}",
    );
}

/// Oracle: empty filter returns all objects (pre-fix behaviour preserved
/// as a negative control). Sort default is ascending by id.
#[tokio::test]
async fn query_no_filter_no_sort_returns_all_ordered_by_id() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let a = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;
    let b = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Mailbox", "relatedId": "MB1"}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call("Metadata/query", json!({"accountId": "acc1"}), "c0");
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids = args["ids"].as_array().expect("ids must be array");
    assert_eq!(ids.len(), 2);
    // demo_next_id is monotonic so first-seeded id sorts before second-seeded.
    let mut expected = vec![a.as_ref(), b.as_ref()];
    expected.sort();
    let got: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        got, expected,
        "no filter / no sort returns all by id asc: {args}"
    );
}

// ---------------------------------------------------------------------------
// Metadata/query position handling (bd:JMAP-826m.5)
//
// RFC 8620 §5.5: `position` in the request may be negative to indicate an
// offset from the end of the result list. The response always echoes a
// non-negative `position` (the effective 0-based start index from the
// beginning of the full result list).
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §5.5 — `position: -1` against a result set of 3
/// returns a single-item ids array starting at the last entry, and the
/// response echoes `position: 2` (not -1).
#[tokio::test]
async fn query_negative_position_within_bounds_clamps_from_end() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let a = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;
    let b = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM2"}),
    )
    .await;
    let c = seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM3"}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({"accountId": "acc1", "position": -1}),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    // Default sort is id-asc; the last id in id-asc order is the
    // expected single result.
    let mut sorted = [a.as_ref(), b.as_ref(), c.as_ref()];
    sorted.sort_unstable();
    let last_id = sorted[2];

    let ids = args["ids"].as_array().expect("ids must be array");
    assert_eq!(
        ids.len(),
        1,
        "position=-1 with 3 results returns exactly one id: {args}"
    );
    assert_eq!(
        ids[0].as_str(),
        Some(last_id),
        "position=-1 with 3 results returns the last id: {args}"
    );
    assert_eq!(
        args["position"].as_u64(),
        Some(2),
        "response echoes effective non-negative position (2 for offset-1 of len-3): {args}"
    );
}

/// Oracle: RFC 8620 §5.5 — `position: -100` (|neg| > len) is clamped to
/// position 0; the full result list is returned, and the response echoes
/// `position: 0`.
#[tokio::test]
async fn query_negative_position_beyond_bounds_clamps_to_zero() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;
    seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM2"}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({"accountId": "acc1", "position": -100}),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids = args["ids"].as_array().expect("ids must be array");
    assert_eq!(
        ids.len(),
        2,
        "position=-100 (|neg| > len) returns the full list: {args}"
    );
    assert_eq!(
        args["position"].as_u64(),
        Some(0),
        "response echoes position=0 for over-large negative offset: {args}"
    );
}

/// Oracle: RFC 8620 §5.5 — `position: 100` (positive, > len) is clamped
/// to position == len; ids is empty; the response echoes the clamped
/// position.
#[tokio::test]
async fn query_positive_position_beyond_bounds_clamps_to_len() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM1"}),
    )
    .await;
    seed_metadata(
        &backend,
        "acc1",
        json!({"@type": "Annotation", "relatedType": "Email", "relatedId": "EM2"}),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/query",
        json!({"accountId": "acc1", "position": 100}),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    let ids = args["ids"].as_array().expect("ids must be array");
    assert!(
        ids.is_empty(),
        "position=100 (> len=2) returns no ids: {args}"
    );
    assert_eq!(
        args["position"].as_u64(),
        Some(2),
        "response echoes position=len=2 for over-large positive offset: {args}"
    );
}

// ---------------------------------------------------------------------------
// Filter contract regression (bd:JMAP-ayoz.38)
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-ayoz.38 — `validate_metadata_filter` silently
/// returns `Ok(())` when the filter passes the unknown-keys walk but
/// fails typed deserialize (a known field has the wrong VALUE shape,
/// e.g. `relatedIds: 42` instead of an array of Id strings). The
/// contract is that the downstream generic `/query` handler will
/// surface this as `unsupportedFilter` via its own `optional_arg` →
/// `serde_json::from_value` → error mapping at
/// `jmap_server::helpers::optional_arg` (cross-crate non-local
/// invariant).
///
/// This test pins that contract end-to-end through the registered
/// dispatcher. If a future refactor of `jmap_server::handlers::handle_query`
/// or `optional_arg` ever silent-OK's a wrong-type filter field (e.g.
/// via `#[serde(default)]` on `Filter<T>` or a switch to a value-tree
/// walk that ignores type mismatches on known fields), the silent-Ok in
/// `validate_metadata_filter` becomes a pass-through bug — and this
/// test fails loudly.
///
/// The `accountId` is known (so the account-existence check passes)
/// and the filter object carries no §3.4.1 cross-field violation; the
/// only failure mode is the wrong-type-on-a-known-field path.
#[tokio::test]
async fn query_filter_known_field_wrong_value_type_returns_unsupported_filter() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    // `relatedIds` is a recognised MetadataFilterCondition field
    // (passes the unknown-keys walk) but its expected wire shape is
    // an array of Id strings. Supplying an integer fails typed
    // deserialize.
    let req = single_call(
        "Metadata/query",
        json!({
            "accountId": "acc1",
            "filter": { "relatedIds": 42 }
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (method_name, args, _) = &resp.method_responses[0];
    assert_eq!(
        method_name, "error",
        "wrong-type filter field must produce an error invocation: {args}",
    );
    assert_eq!(
        args["type"], "unsupportedFilter",
        "wrong-type filter field MUST surface as `unsupportedFilter` \
         per the validate_metadata_filter contract (bd:JMAP-ayoz.38): \
         {args}",
    );
}

/// Oracle: bd:JMAP-ayoz.38 companion — same contract on
/// `Metadata/queryChanges`. The two methods share the
/// `validate_metadata_filter` precheck and the generic
/// `optional_arg`-based deserialize, so the contract must hold on
/// both.
#[tokio::test]
async fn query_changes_filter_known_field_wrong_value_type_returns_unsupported_filter() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Metadata/queryChanges",
        json!({
            "accountId": "acc1",
            "sinceQueryState": "0",
            "filter": { "relatedIds": 42 }
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (method_name, args, _) = &resp.method_responses[0];
    assert_eq!(
        method_name, "error",
        "wrong-type filter field must produce an error invocation: {args}",
    );
    assert_eq!(
        args["type"], "unsupportedFilter",
        "wrong-type filter field MUST surface as `unsupportedFilter` \
         per the validate_metadata_filter contract (bd:JMAP-ayoz.38): \
         {args}",
    );
}
