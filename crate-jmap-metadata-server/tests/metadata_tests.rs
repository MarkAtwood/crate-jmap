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

    // Destroy all four, then re-run /changes from "0" with the same
    // AND filter. The destroyed array must show only the
    // (Email, Annotation) id.
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
            "sinceState": "0",
            "filterRelatedType": "Email",
            "filterMetadataType": ["Annotation"]
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    // Created and destroyed for the same id within the same /changes
    // window collapse to destroyed (later supersedes earlier) per the
    // override's create-then-destroy precedence. So the (Email,
    // Annotation) id must appear in destroyed, NOT in created.
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
        "no surviving creates when every id is also destroyed: {args}",
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
