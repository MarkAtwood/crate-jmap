//! `Blob/lookup` (blobext-03 §6) against the memory backend: every case in
//! docs/design/blobext.md §10 "server, Blob/lookup".
//!
//! Oracles: the §9.3 example and the §6 rules as quoted in each test.

use std::collections::BTreeMap;

use jmap_blob_server::memory::MemoryBackend;
use jmap_blob_server::{BlobBackend, BlobCreate, BlobSource, handle_blob_lookup};
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const BLOB: &str = "urn:ietf:params:jmap:blob";
const MAIL: &str = "urn:ietf:params:jmap:mail";

fn id(s: &str) -> Id {
    Id::new_validated(s).unwrap()
}

/// Account `account` advertising `Mailbox`, `Email`, `Identity` (cannot
/// reference blobs) and `NoSuchType` (not in the registry) as lookup types.
fn backend_for(account: &str) -> MemoryBackend<()> {
    let b = MemoryBackend::<()>::default().with_account(&id(account));
    let mut cap = MemoryBackend::default_capability();
    cap.supported_type_names = ["Mailbox", "Email", "Identity", "NoSuchType"]
        .map(str::to_owned)
        .to_vec();
    b.advertise_for_test(&id(account), cap);
    b
}

fn backend() -> MemoryBackend<()> {
    backend_for("a1")
}

fn using(uris: &[&str]) -> Vec<String> {
    let mut v = vec!["urn:ietf:params:jmap:core".to_owned()];
    v.extend(uris.iter().map(|u| (*u).to_owned()));
    v
}

async fn put_in(b: &MemoryBackend<()>, account: &str, bytes: &[u8]) -> Id {
    b.create_blob(
        &(),
        &id(account),
        BlobCreate::new(vec![BlobSource::Bytes(bytes.to_vec())], bytes.len() as u64),
    )
    .await
    .unwrap()
    .id
}

async fn put(b: &MemoryBackend<()>, bytes: &[u8]) -> Id {
    put_in(b, "a1", bytes).await
}

fn refer(b: &MemoryBackend<()>, type_name: &str, object: &str, blob: &Id) {
    b.add_reference_for_test(&id("a1"), type_name, &id(object), blob);
}

async fn call_with(
    b: &MemoryBackend<()>,
    args: Value,
    created: &BTreeMap<Id, Id>,
    uris: &[&str],
) -> Result<Value, JmapError> {
    handle_blob_lookup(b, &(), args, created, &using(uris))
        .await
        .map(|out| out.primary)
}

async fn call(b: &MemoryBackend<()>, args: Value) -> Result<Value, JmapError> {
    call_with(b, args, &BTreeMap::new(), &[BLOB2, MAIL]).await
}

async fn ok(b: &MemoryBackend<()>, args: Value) -> Value {
    call(b, args).await.unwrap()
}

fn error_type(r: Result<Value, JmapError>) -> String {
    match r {
        Err(e) => serde_json::to_value(&e).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned(),
        Ok(v) => panic!("expected an error, got {v}"),
    }
}

async fn err(b: &MemoryBackend<()>, args: Value) -> String {
    error_type(call(b, args).await)
}

// ---- the -03 example -----------------------------------------------------

/// -03 §9.3, verbatim except the blob ids, which the memory backend mints
/// (`B1` for `Bfirst`, `B2` for `Bsecond`). The backend owns containment,
/// so the test records the Mailbox reference directly.
#[tokio::test]
async fn example_9_3() {
    let b = backend_for("abc");
    let first = put_in(&b, "abc", b"first").await;
    let second = put_in(&b, "abc", b"second").await;
    assert_eq!((first.as_ref(), second.as_ref()), ("B1", "B2"));
    for (t, o) in [("Mailbox", "M1"), ("Email", "E1"), ("Email", "E2")] {
        b.add_reference_for_test(&id("abc"), t, &id(o), &first);
    }
    let resp = ok(
        &b,
        json!({
            "accountId": "abc",
            "typeNames": ["Mailbox", "Email"],
            "ids": ["B1", "B2"]
        }),
    )
    .await;
    assert_eq!(
        resp,
        json!({
            "accountId": "abc",
            "list": [
                {
                    "id": "B1",
                    "matchedIds": {
                        "Mailbox": ["M1"],
                        "Email": ["E1", "E2"]
                    }
                },
                {
                    "id": "B2",
                    "matchedIds": {
                        "Mailbox": [],
                        "Email": []
                    }
                }
            ]
        })
    );
}

// ---- unknownDataType -----------------------------------------------------

/// §6: "Only type names listed in the server's supportedTypeNames
/// capability, and whose "JMAP Data Types" registry entry has "Can
/// Reference Blobs" set to "Yes" … are valid. If a type name is not known
/// by the server or the associated capability has not been requested in
/// using, the server MUST return an "unknownDataType" error."
#[tokio::test]
async fn unknown_data_type() {
    let b = backend();
    let blob = put(&b, b"x").await;
    let args = |t: &str| json!({"accountId": "a1", "typeNames": ["Email", t], "ids": [blob]});
    // In the registry and able to reference blobs, but not advertised.
    assert_eq!(err(&b, args("Thread")).await, "unknownDataType");
    // Advertised, but the registry says it cannot reference blobs.
    assert_eq!(err(&b, args("Identity")).await, "unknownDataType");
    // Advertised, but in no registry.
    assert_eq!(err(&b, args("NoSuchType")).await, "unknownDataType");
    // Advertised and able, but mail is not in using.
    let r = call_with(&b, args("Mailbox"), &BTreeMap::new(), &[BLOB2]).await;
    assert_eq!(error_type(r), "unknownDataType");
    // The description names the offending type.
    let e = call(&b, args("Thread")).await.unwrap_err();
    assert!(e.description.as_deref().unwrap().contains("Thread"));
}

// ---- matching ------------------------------------------------------------

/// §6: "the server MUST NOT return the id of an object that the user could
/// not discover by querying."
#[tokio::test]
async fn hidden_object_is_omitted() {
    let b = backend();
    let blob = put(&b, b"x").await;
    refer(&b, "Email", "E1", &blob);
    refer(&b, "Email", "E2", &blob);
    b.hide_object_for_test(&id("a1"), "Email", &id("E1"));
    let resp = ok(
        &b,
        json!({"accountId": "a1", "typeNames": ["Email"], "ids": [blob]}),
    )
    .await;
    assert_eq!(resp["list"][0]["matchedIds"], json!({"Email": ["E2"]}));
}

/// §6: "if a blob is not visible to the authenticated user or does not
/// exist on the server at all, the server MUST still return an empty array
/// for each type name." There is no `notFound`.
#[tokio::test]
async fn unknown_or_invisible_blob_gets_empty_arrays() {
    let b = backend();
    // A blob of another account, referenced there, is invisible in a1.
    b.register_account(&id("a2"));
    let other = put_in(&b, "a2", b"other").await;
    b.add_reference_for_test(&id("a2"), "Email", &id("E9"), &other);
    let resp = ok(
        &b,
        json!({
            "accountId": "a1",
            "typeNames": ["Mailbox", "Email"],
            "ids": ["B99", other, "#nope"]
        }),
    )
    .await;
    let empty = json!({"Mailbox": [], "Email": []});
    assert_eq!(
        resp,
        json!({
            "accountId": "a1",
            "list": [
                {"id": "B99", "matchedIds": empty},
                {"id": other, "matchedIds": empty},
                // An unresolved `#creationId` is echoed as sent.
                {"id": "#nope", "matchedIds": empty}
            ]
        })
    );
}

/// `list` is in request order with one entry per requested id, so a
/// repeated id repeats its entry; a repeated type name is one key.
#[tokio::test]
async fn request_order_and_duplicates() {
    let b = backend();
    let x = put(&b, b"x").await;
    let y = put(&b, b"y").await;
    refer(&b, "Email", "E1", &x);
    refer(&b, "Email", "E2", &y);
    let resp = ok(
        &b,
        json!({"accountId": "a1", "typeNames": ["Email", "Email"], "ids": [y, x, y]}),
    )
    .await;
    assert_eq!(
        resp["list"],
        json!([
            {"id": y, "matchedIds": {"Email": ["E2"]}},
            {"id": x, "matchedIds": {"Email": ["E1"]}},
            {"id": y, "matchedIds": {"Email": ["E2"]}}
        ])
    );
}

/// A `#creationId` resolves through the request's earlier creations and
/// reports the blobId it names.
#[tokio::test]
async fn creation_reference_resolves() {
    let b = backend();
    let blob = put(&b, b"x").await;
    refer(&b, "Mailbox", "M1", &blob);
    let created = BTreeMap::from([(id("c1"), blob.clone())]);
    let resp = call_with(
        &b,
        json!({"accountId": "a1", "typeNames": ["Mailbox"], "ids": ["#c1"]}),
        &created,
        &[BLOB2, MAIL],
    )
    .await
    .unwrap();
    assert_eq!(
        resp["list"],
        json!([{"id": blob, "matchedIds": {"Mailbox": ["M1"]}}])
    );
}

/// Empty `typeNames` gives empty `matchedIds` objects; empty `ids` an
/// empty `list`.
#[tokio::test]
async fn empty_arguments() {
    let b = backend();
    let blob = put(&b, b"x").await;
    let resp = ok(
        &b,
        json!({"accountId": "a1", "typeNames": [], "ids": [blob]}),
    )
    .await;
    assert_eq!(resp["list"], json!([{"id": blob, "matchedIds": {}}]));
    let resp = ok(
        &b,
        json!({"accountId": "a1", "typeNames": ["Email"], "ids": []}),
    )
    .await;
    assert_eq!(resp, json!({"accountId": "a1", "list": []}));
}

// ---- method-level errors -------------------------------------------------

#[tokio::test]
async fn malformed_arguments() {
    let b = backend();
    for args in [
        json!({"accountId": "a1", "ids": []}),
        json!({"accountId": "a1", "typeNames": null, "ids": []}),
        json!({"accountId": "a1", "typeNames": "Email", "ids": []}),
        json!({"accountId": "a1", "typeNames": [1], "ids": []}),
        json!({"accountId": "a1", "typeNames": []}),
        json!({"accountId": "a1", "typeNames": [], "ids": null}),
        json!({"accountId": "a1", "typeNames": [], "ids": ["not an id"]}),
        json!({"accountId": "a1", "typeNames": [], "ids": [7]}),
        json!({"typeNames": [], "ids": []}),
    ] {
        assert_eq!(err(&b, args.clone()).await, "invalidArguments", "{args}");
    }
}

#[tokio::test]
async fn account_not_found() {
    let b = backend();
    let r = err(&b, json!({"accountId": "zz", "typeNames": [], "ids": []})).await;
    assert_eq!(r, "accountNotFound");
}

#[tokio::test]
async fn request_too_large() {
    let b = backend();
    let ids: Vec<String> = (0..501).map(|n| format!("B{n}")).collect();
    let r = err(&b, json!({"accountId": "a1", "typeNames": [], "ids": ids})).await;
    assert_eq!(r, "requestTooLarge");
}

/// -03 §2.1: "a client MUST NOT include both" capabilities.
#[tokio::test]
async fn both_capabilities_is_invalid_arguments() {
    let b = backend();
    let r = call_with(
        &b,
        json!({"accountId": "a1", "typeNames": [], "ids": []}),
        &BTreeMap::new(),
        &[BLOB, BLOB2],
    )
    .await;
    assert_eq!(error_type(r), "invalidArguments");
}

/// RFC 9404 §4.3 has the same rules, and an unknown blob gets empty
/// arrays there too (decision 21(ii)), not the §4.3.1 example's
/// `notFound`.
#[tokio::test]
async fn rfc9404_mode_matches() {
    let b = backend();
    let blob = put(&b, b"x").await;
    refer(&b, "Email", "E1", &blob);
    let args = json!({"accountId": "a1", "typeNames": ["Email"], "ids": [blob, "B99"]});
    let legacy = call_with(&b, args.clone(), &BTreeMap::new(), &[BLOB, MAIL])
        .await
        .unwrap();
    assert_eq!(legacy, ok(&b, args).await);
    assert_eq!(
        legacy["list"],
        json!([
            {"id": blob, "matchedIds": {"Email": ["E1"]}},
            {"id": "B99", "matchedIds": {"Email": []}}
        ])
    );
    assert!(legacy.get("notFound").is_none());
}
