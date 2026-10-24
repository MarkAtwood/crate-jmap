//! draft-ietf-jmap-metadata-04 on Principal in the in-memory backend opted
//! in with `MemoryBackend::with_metadata_info` (bd JMAP-s4kp.7).
//!
//! Oracle: the -04 text quoted on each test
//! (/home/mark/PROJECT/IETF/specs/draft-ietf-jmap-metadata-04.txt, the
//! revision of October 2026) and RFC 8620 §5.5, with hand-written requests
//! and expected responses. The capability below allows vendor namespaces
//! only, no private metadata, and `maxDepth` 2.

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_server::{Dispatcher, HandlerOutput};
use jmap_sharing_server::{
    handle_principal_changes, handle_principal_get, handle_principal_query,
    handle_principal_query_changes, handle_principal_set, register_sharing_handlers,
};
use jmap_types::{Id, JmapError, JmapRequest, State};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "a1";
const NS: &str = "acme.example.com";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:principals".to_owned(),
        "urn:ietf:params:jmap:metadata".to_owned(),
    ]
}

fn using_without_metadata() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:principals".to_owned(),
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
        .with_metadata_info("Principal", info())
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

async fn principal_set(backend: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } = handle_principal_set(backend, &(), args, using)
        .await
        .expect("Principal/set must not fail at method level");
    primary
}

/// Create an individual Principal named `name` carrying `metadata`.
async fn create(backend: &MemoryBackend, name: &str, metadata: Value) -> String {
    let create = json!({ "type": "individual", "name": name, "metadata": metadata });
    let resp = principal_set(backend, json!({ "create": { "c": create } }), &using()).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Principal/set create failed: {resp}"))
        .to_owned()
}

/// Apply a Principal/set update that must succeed.
async fn update(backend: &MemoryBackend, id: &str, patch: Value) {
    let resp = principal_set(backend, json!({ "update": { id: patch } }), &using()).await;
    assert!(resp["updated"].get(id).is_some(), "{resp}");
}

/// The SetError type of a Principal/set update that must fail.
async fn update_error(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let resp = principal_set(backend, json!({ "update": { id: patch } }), &using()).await;
    resp["notUpdated"][id]["type"].clone()
}

async fn property(backend: &MemoryBackend, id: &str, property: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id] });
    let HandlerOutput { primary, .. } = handle_principal_get(backend, &(), args, &using())
        .await
        .unwrap();
    primary["list"][0][property].clone()
}

async fn query(backend: &MemoryBackend, filter: Value) -> Result<Vec<String>, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "filter": filter });
    handle_principal_query(backend, &(), args, &using())
        .await
        .map(|o| {
            let mut ids: Vec<String> = o.primary["ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect();
            ids.sort();
            ids
        })
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}

async fn changes(
    backend: &MemoryBackend,
    args: Value,
    using: &[String],
) -> Result<Value, JmapError> {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_principal_changes(backend, &(), args, using)
        .await
        .map(|o| o.primary)
}

async fn state(backend: &MemoryBackend) -> String {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [] });
    let HandlerOutput { primary, .. } = handle_principal_get(backend, &(), args, &using())
        .await
        .unwrap();
    primary["state"].as_str().unwrap().to_owned()
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
    let b = backend();
    let tagged = create(&b, "Room 4B", json!({ NS: { "floor": 4 } })).await;
    let _empty_ns = create(&b, "Room 4C", json!({ NS: {} })).await;
    let _none = create(&b, "Projector", json!({})).await;

    assert_eq!(
        query(&b, json!({ "metadataExists": NS })).await.unwrap(),
        vec![tagged.clone()]
    );
    assert_eq!(
        query(&b, json!({ "metadataExists": format!("{NS}/floor") }))
            .await
            .unwrap(),
        vec![tagged]
    );
    assert!(
        query(&b, json!({ "metadataExists": format!("{NS}/wing") }))
            .await
            .unwrap()
            .is_empty()
    );
}

/// -04 §3.6: `metadataTextContains` matches a substring "under the
/// "i;unicode-casemap" collation"; `metadataTextEquals` compares "Unicode
/// code points without case folding". Both compose through "the usual
/// AND/OR/NOT operators (Section 5.5 of [RFC8620])"; RFC 8620 §5.5: NOT
/// means "none of the conditions must match".
#[tokio::test]
async fn query_text_conditions_in_operator_tree() {
    let b = backend();
    let alpha = create(&b, "A", json!({ NS: { "team": "Alpha Squad" } })).await;
    let beta = create(&b, "B", json!({ NS: { "team": "beta" } })).await;
    let none = create(&b, "C", json!({})).await;

    let contains =
        json!({ "metadataTextContains": { "path": format!("{NS}/team"), "value": "ALPHA" } });
    assert_eq!(
        query(&b, contains.clone()).await.unwrap(),
        vec![alpha.clone()]
    );

    let equals_upper =
        json!({ "metadataTextEquals": { "path": format!("{NS}/team"), "value": "Beta" } });
    assert!(query(&b, equals_upper).await.unwrap().is_empty());
    let equals = json!({ "metadataTextEquals": { "path": format!("{NS}/team"), "value": "beta" } });
    assert_eq!(query(&b, equals.clone()).await.unwrap(), vec![beta.clone()]);

    let or = json!({ "operator": "OR", "conditions": [contains.clone(), equals] });
    assert_eq!(
        query(&b, or).await.unwrap(),
        sorted(vec![alpha.clone(), beta.clone()])
    );
    let not = json!({ "operator": "NOT", "conditions": [contains] });
    assert_eq!(query(&b, not).await.unwrap(), sorted(vec![beta, none]));
}

/// -04 §3.6: a malformed path "MUST be rejected with an
/// "invalidArguments" error"; "Any privateMetadata* condition ... MUST be
/// rejected with an "unsupportedFilter" error if supportsPrivate is false".
/// RFC 8620 §5.5: "A server MUST reject the call with an unsupportedFilter
/// error if it cannot process the given filter"; this backend evaluates no
/// RFC 9670 §2.4.1 condition, so one next to a metadata condition is
/// rejected, also inside an operator and with no Principal stored.
#[tokio::test]
async fn invalid_conditions_are_rejected() {
    let b = backend();
    let bad_path = json!({ "metadataExists": format!("{NS}/a~2b") });
    assert_eq!(
        error_type(query(&b, bad_path).await.unwrap_err()),
        json!("invalidArguments")
    );
    let private = json!({ "privateMetadataExists": NS });
    assert_eq!(
        error_type(query(&b, private).await.unwrap_err()),
        json!("unsupportedFilter")
    );
    let mixed_leaf = json!({ "name": "Room", "metadataExists": NS });
    assert_eq!(
        error_type(query(&b, mixed_leaf).await.unwrap_err()),
        json!("unsupportedFilter")
    );
    let mixed_tree = json!({ "operator": "AND", "conditions": [
        { "metadataExists": NS }, { "type": "individual" }
    ] });
    assert_eq!(
        error_type(query(&b, mixed_tree).await.unwrap_err()),
        json!("unsupportedFilter")
    );
}

/// -04 §1.2.1: a type not listed in dataTypes "does not gain the metadata
/// or privateMetadata properties", so the backend cannot process a
/// metadata condition (RFC 8620 §5.5 `unsupportedFilter`).
#[tokio::test]
async fn unlisted_type_rejects_metadata_condition() {
    let b = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    assert_eq!(
        error_type(
            query(&b, json!({ "metadataExists": NS }))
                .await
                .unwrap_err()
        ),
        json!("unsupportedFilter")
    );
}

// ---------------------------------------------------------------------------
// /set (-04 §3, §3.2)
// ---------------------------------------------------------------------------

/// -04 §3.2: patch paths "MAY continue to any depth below" a namespace;
/// "Setting a patch value to null removes the targeted key"; "In an
/// update, null is equivalent to {}".
#[tokio::test]
async fn patch_within_namespace_then_null() {
    let b = backend();
    let id = create(&b, "Room 4B", json!({ NS: { "floor": 4, "wing": "E" } })).await;

    update(&b, &id, json!({ format!("metadata/{NS}/floor"): 5 })).await;
    assert_eq!(
        property(&b, &id, "metadata").await,
        json!({ NS: { "floor": 5, "wing": "E" } })
    );
    update(&b, &id, json!({ format!("metadata/{NS}/wing"): null })).await;
    assert_eq!(
        property(&b, &id, "metadata").await,
        json!({ NS: { "floor": 5 } })
    );
    update(&b, &id, json!({ "metadata": null })).await;
    assert_eq!(property(&b, &id, "metadata").await, json!({}));
}

/// -04 §3: without the capability in `using`, "Foo/set MUST reject any
/// write to them with an "invalidProperties" SetError". -04 §3.2: on
/// create, "A value of null for either property MUST be rejected with an
/// "invalidProperties" SetError".
#[tokio::test]
async fn handler_checks_using_and_create_null() {
    let b = backend();
    let id = create(&b, "Room 4B", json!({})).await;

    let patch = json!({ "update": { &id: { "metadata": { NS: { "k": 1 } } } } });
    let resp = principal_set(&b, patch, &using_without_metadata()).await;
    assert_eq!(
        resp["notUpdated"][&id]["type"],
        json!("invalidProperties"),
        "{resp}"
    );
    let create_null =
        json!({ "create": { "c": { "type": "individual", "name": "X", "metadata": null } } });
    let resp = principal_set(&b, create_null, &using()).await;
    assert_eq!(
        resp["notCreated"]["c"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );
}

/// -04 §3.2 rules 2 to 4: privateMetadata while "supportsPrivate is false"
/// (rule 2), a namespace "not supported on this data type" (rule 3; the
/// registered-style name `photography` is not listed and is not a domain
/// name), and a value that "would exceed maxDepth" (rule 4; -04 §2.1
/// gives `{"a": {"b": {"c": 1}}}` depth 3, over maxDepth 2) are each
/// `invalidProperties`; none of them is stored.
#[tokio::test]
async fn invalid_metadata_writes_are_invalid_properties() {
    let b = backend();
    let id = create(&b, "Room 4B", json!({})).await;
    let before = state(&b).await;

    for patch in [
        json!({ "privateMetadata": { NS: { "k": 1 } } }),
        json!({ "metadata/photography": { "iso": 400 } }),
        json!({ format!("metadata/{NS}"): { "a": { "b": { "c": 1 } } } }),
    ] {
        assert_eq!(
            update_error(&b, &id, patch.clone()).await,
            json!("invalidProperties"),
            "{patch}"
        );
    }
    assert_eq!(state(&b).await, before);
    assert_eq!(property(&b, &id, "metadata").await, json!({}));
}

/// -04 §3.2: "Patch errors that can be determined from the request alone
/// (for example, two paths where one is a prefix of the other) MUST be
/// reported with "invalidPatch" before rule 1 is evaluated", and rule 1
/// ("If the user cannot read the related object, reject with "notFound"")
/// comes before the namespace rule 3.
#[tokio::test]
async fn rule_order_invalid_patch_then_not_found() {
    let b = backend();
    let prefix = json!({
        format!("metadata/{NS}"): { "k": 1 },
        format!("metadata/{NS}/k"): 2
    });
    assert_eq!(
        update_error(&b, "missing", prefix).await,
        json!("invalidPatch")
    );
    assert_eq!(
        update_error(&b, "missing", json!({ "metadata/photography": {} })).await,
        json!("notFound")
    );
}

/// -04 §1.2.1: an unlisted type "does not gain the metadata or
/// privateMetadata properties", so a write is `invalidProperties` on
/// create and on update.
#[tokio::test]
async fn unlisted_type_rejects_metadata_write() {
    let b = MemoryBackend::new().with_account(&Id::from(ACCOUNT_ID));
    let create_meta = json!({ "create": { "c": {
        "type": "individual", "name": "X", "metadata": { NS: {} }
    } } });
    let resp = principal_set(&b, create_meta, &using()).await;
    assert_eq!(
        resp["notCreated"]["c"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );

    let resp = principal_set(
        &b,
        json!({ "create": { "c": { "type": "individual", "name": "X" } } }),
        &using(),
    )
    .await;
    let id = resp["created"]["c"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        update_error(&b, &id, json!({ "metadata": { NS: {} } })).await,
        json!("invalidProperties")
    );
}

// ---------------------------------------------------------------------------
// /changes (-04 §3, §3.3)
// ---------------------------------------------------------------------------

/// -04 §3.3: with ignoreMetadataOnlyChanges the server "MUST exclude from
/// the updated array any id whose only changes since sinceState ... are
/// confined to metadata"; otherwise `updatedProperties` "MUST be set to a
/// list containing only those property names". -04 §3: a request without
/// the capability "MUST behave as if ignoreMetadataOnlyChanges were true".
#[tokio::test]
async fn metadata_only_change() {
    let b = backend();
    let id = create(&b, "Room 4B", json!({})).await;
    let since = state(&b).await;
    update(&b, &id, json!({ format!("metadata/{NS}"): { "k": 1 } })).await;

    let reported = changes(&b, json!({ "sinceState": since }), &using())
        .await
        .unwrap();
    assert_eq!(reported["updated"], json!([id]), "{reported}");
    assert_eq!(reported["updatedProperties"], json!(["metadata"]));

    let ignored = changes(
        &b,
        json!({ "sinceState": since, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(ignored["updated"], json!([]), "{ignored}");
    assert_ne!(
        ignored["newState"],
        json!(since),
        "the state still advances"
    );

    let legacy = changes(
        &b,
        json!({ "sinceState": since }),
        &using_without_metadata(),
    )
    .await
    .unwrap();
    assert_eq!(legacy["updated"], json!([]), "{legacy}");
}

/// -04 §3.3: "Ids with any other change remain in updated", and
/// `updatedProperties` "MUST be null" if "any other property ... may also
/// have changed".
#[tokio::test]
async fn mixed_change_is_reported_without_updated_properties() {
    let b = backend();
    let id = create(&b, "Room 4B", json!({})).await;
    let since = state(&b).await;
    update(
        &b,
        &id,
        json!({ "name": "Room 4C", format!("metadata/{NS}"): { "k": 1 } }),
    )
    .await;

    for args in [
        json!({ "sinceState": since }),
        json!({ "sinceState": since, "ignoreMetadataOnlyChanges": true }),
    ] {
        let resp = changes(&b, args, &using()).await.unwrap();
        assert_eq!(resp["updated"], json!([id]), "{resp}");
        assert_eq!(resp["updatedProperties"], Value::Null, "{resp}");
    }
}

/// -04 §3.3: "Excluded ids do not count toward maxChanges".
#[tokio::test]
async fn excluded_ids_do_not_count_toward_max_changes() {
    let b = backend();
    let p1 = create(&b, "P1", json!({})).await;
    let p2 = create(&b, "P2", json!({})).await;
    let since = state(&b).await;
    update(&b, &p1, json!({ format!("metadata/{NS}"): { "k": 1 } })).await;
    update(&b, &p2, json!({ format!("metadata/{NS}"): { "k": 2 } })).await;

    let ignored = changes(
        &b,
        json!({ "sinceState": since, "maxChanges": 1, "ignoreMetadataOnlyChanges": true }),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(ignored["updated"], json!([]), "{ignored}");
    // Reported, the two updates exceed maxChanges 1: RFC 8620 §5.2 "the
    // server SHOULD generate an update to take the client to an
    // intermediate state".
    let reported = changes(
        &b,
        json!({ "sinceState": since, "maxChanges": 1 }),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(reported["updated"], json!([p1]), "{reported}");
    assert_eq!(reported["hasMoreChanges"], json!(true), "{reported}");
}

// ---------------------------------------------------------------------------
// Dispatcher wiring (-04 §3)
// ---------------------------------------------------------------------------

/// `register_sharing_handlers` hands the request's `using` to
/// Principal/set: the same write is `invalidProperties` without
/// `urn:ietf:params:jmap:metadata` (-04 §3) and succeeds with it.
#[tokio::test]
async fn dispatcher_forwards_using() {
    let b = backend();
    let id = create(&b, "Room 4B", json!({})).await;
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::new(b));

    let call = json!([["Principal/set", {
        "accountId": ACCOUNT_ID,
        "update": { &id: { format!("metadata/{NS}"): { "k": 1 } } }
    }, "0"]]);
    for (using, expect_ok) in [(using_without_metadata(), false), (using(), true)] {
        let request: JmapRequest =
            serde_json::from_value(json!({ "using": using, "methodCalls": call })).unwrap();
        let resp = dispatcher.dispatch(request, (), State::from("s0")).await;
        let args = &resp.method_responses[0].arguments;
        if expect_ok {
            assert!(args["updated"].get(&id).is_some(), "{args}");
        } else {
            assert_eq!(
                args["notUpdated"][&id]["type"],
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

/// Run Principal/get with `properties` on `id` and return its one list entry.
async fn principal_get_one(
    b: &MemoryBackend,
    id: &str,
    properties: Value,
    using: &[String],
) -> Result<Value, JmapError> {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": properties });
    handle_principal_get(b, &(), args, using)
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
async fn principal_get_default_properties_follow_capability() {
    let b = backend();
    let bare = create(&b, "bare", json!({})).await;
    let tagged = create(&b, "tagged", json!({ NS: { "k": 1 } })).await;

    let got = principal_get_one(&b, &bare, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({}), "{got}");
    assert!(got.get("privateMetadata").is_none(), "{got}");
    let got = principal_get_one(&b, &tagged, Value::Null, &using())
        .await
        .unwrap();
    assert_eq!(got["metadata"], json!({ NS: { "k": 1 } }), "{got}");

    let without = using_without_metadata();
    let got = principal_get_one(&b, &tagged, Value::Null, &without)
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");
    let props = json!(["id", "metadata", "privateMetadata"]);
    let got = principal_get_one(&b, &tagged, props, &without)
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
async fn principal_get_metadata_subselectors() {
    let b = backend();
    let id = create(&b, "tagged", json!({ NS: { "k": 1 }, NS2: { "k": 2 } })).await;
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
        let got = principal_get_one(&b, &id, props.clone(), &using())
            .await
            .unwrap();
        assert_eq!(got["metadata"], want, "{props}: {got}");
    }

    let props = json!(["metadata/acme.example.com"]);
    let got = principal_get_one(&b, &id, props, &using_without_metadata())
        .await
        .unwrap();
    assert!(got.get("metadata").is_none(), "{got}");

    for props in [
        json!(["metadata/"]),
        json!(["metadata/acme.example.com/k"]),
        json!(["privateMetadata/"]),
    ] {
        let err = principal_get_one(&b, &id, props.clone(), &using())
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
async fn principal_metadata_filter_requires_capability() {
    let b = backend();
    let without = using_without_metadata();
    for filter in [
        json!({ "metadataExists": NS }),
        json!({ "operator": "NOT", "conditions": [json!({ "metadataExists": NS })] }),
    ] {
        let args = json!({ "accountId": ACCOUNT_ID, "filter": filter.clone() });
        let err = handle_principal_query(&b, &(), args, &without)
            .await
            .unwrap_err();
        assert_eq!(
            method_error_type(&err),
            json!("unsupportedFilter"),
            "{filter}"
        );
        let args =
            json!({ "accountId": ACCOUNT_ID, "filter": filter.clone(), "sinceQueryState": "0" });
        let err = handle_principal_query_changes(&b, &(), args, &without)
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
        handle_principal_query(&b, &(), args, &using())
            .await
            .is_ok()
    );
}
