//! `Blob/get` (blobext-03 §5) against the memory backend: every case in
//! docs/design/blobext.md §10 "server, Blob/get".
//!
//! Oracles: digests are RFC 9404 §4.2.1 vectors or were computed outside
//! the code under test (`printf '%s' … | openssl dgst -sha256 -binary |
//! base64`); base64 bodies likewise come from `base64(1)`.

use std::collections::BTreeMap;

use jmap_blob_server::memory::MemoryBackend;
use jmap_blob_server::{BlobBackend, BlobCreate, BlobSource, handle_blob_get};
use jmap_blob_types::DigestAlgorithm;
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const BLOB: &str = "urn:ietf:params:jmap:blob";
const FOX: &str = "The quick brown fox jumped over the lazy dog.";

fn id(s: &str) -> Id {
    Id::new_validated(s).unwrap()
}

fn backend() -> MemoryBackend {
    MemoryBackend::new().with_account(&id("a1"))
}

fn using(uris: &[&str]) -> Vec<String> {
    let mut v = vec!["urn:ietf:params:jmap:core".to_owned()];
    v.extend(uris.iter().map(|u| (*u).to_owned()));
    v
}

async fn put(b: &MemoryBackend, bytes: &[u8]) -> Id {
    b.create_blob(
        &(),
        &id("a1"),
        BlobCreate::new(vec![BlobSource::Bytes(bytes.to_vec())], bytes.len() as u64),
    )
    .await
    .unwrap()
    .id
}

async fn call_with(
    b: &MemoryBackend,
    args: Value,
    created: &BTreeMap<Id, Id>,
    uris: &[&str],
) -> Result<Value, JmapError> {
    handle_blob_get(b, &(), args, created, &using(uris))
        .await
        .map(|out| out.primary)
}

async fn call(b: &MemoryBackend, args: Value) -> Result<Value, JmapError> {
    call_with(b, args, &BTreeMap::new(), &[BLOB2]).await
}

async fn ok(b: &MemoryBackend, args: Value) -> Value {
    call(b, args).await.unwrap()
}

/// The method-level error type of a failed call.
async fn err(b: &MemoryBackend, args: Value) -> String {
    error_type(call(b, args).await)
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

// ---- common start --------------------------------------------------------

#[tokio::test]
async fn default_properties_are_data_and_size() {
    let b = backend();
    let blob = put(&b, b"hello").await;
    let resp = ok(&b, json!({"accountId": "a1", "ids": [blob]})).await;
    assert_eq!(
        resp,
        json!({
            "accountId": "a1",
            "list": [{"id": blob, "data:asText": "hello", "size": 5}],
            "notFound": []
        })
    );
}

#[tokio::test]
async fn ids_null_or_absent_is_invalid_arguments() {
    let b = backend();
    assert_eq!(
        err(&b, json!({"accountId": "a1", "ids": null})).await,
        "invalidArguments"
    );
    assert_eq!(
        err(&b, json!({"accountId": "a1"})).await,
        "invalidArguments"
    );
}

#[tokio::test]
async fn offset_or_length_without_properties_is_invalid_arguments() {
    let b = backend();
    let blob = put(&b, b"hello").await;
    for extra in [json!({"offset": 1}), json!({"length": 1})] {
        let mut args = json!({"accountId": "a1", "ids": [blob]});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert_eq!(err(&b, args).await, "invalidArguments");
    }
    // Explicit nulls are "neither specified".
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "offset": null, "length": null}),
    )
    .await;
    assert_eq!(resp["list"][0]["data:asText"], "hello");
}

#[tokio::test]
async fn id_property_is_accepted() {
    let b = backend();
    let blob = put(&b, b"hello").await;
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["id", "size"]}),
    )
    .await;
    assert_eq!(resp["list"], json!([{"id": blob, "size": 5}]));
}

#[tokio::test]
async fn unknown_property_names_are_invalid_arguments() {
    let b = backend();
    let blob = put(&b, b"hello").await;
    assert_eq!(
        err(
            &b,
            json!({"accountId": "a1", "ids": [blob], "properties": ["type"]})
        )
        .await,
        "invalidArguments"
    );
    // dataSourceProperties is checked even without chunks.
    assert_eq!(
        err(
            &b,
            json!({"accountId": "a1", "ids": [blob], "properties": ["size"],
                   "dataSourceProperties": ["blobId", "nope"]})
        )
        .await,
        "invalidArguments"
    );
}

#[tokio::test]
async fn both_blob_capabilities_is_invalid_arguments() {
    let b = backend();
    let blob = put(&b, b"hello").await;
    let r = call_with(
        &b,
        json!({"accountId": "a1", "ids": [blob]}),
        &BTreeMap::new(),
        &[BLOB, BLOB2],
    )
    .await;
    assert_eq!(error_type(r), "invalidArguments");
}

#[tokio::test]
async fn unknown_account_is_account_not_found() {
    let b = backend();
    assert_eq!(
        err(&b, json!({"accountId": "zz", "ids": []})).await,
        "accountNotFound"
    );
}

#[tokio::test]
async fn too_many_ids_is_request_too_large() {
    let b = backend();
    let ids: Vec<String> = (0..501).map(|i| format!("B{i}")).collect();
    assert_eq!(
        err(&b, json!({"accountId": "a1", "ids": ids})).await,
        "requestTooLarge"
    );
}

#[tokio::test]
async fn missing_and_unresolved_ids_are_not_found() {
    let b = backend();
    let blob = put(&b, b"hello").await;
    let mut created = BTreeMap::new();
    created.insert(id("c1"), blob.clone());
    let resp = call_with(
        &b,
        json!({"accountId": "a1", "ids": ["#c1", "nope", "#c2"], "properties": ["size"]}),
        &created,
        &[BLOB2],
    )
    .await
    .unwrap();
    // `#c1` resolves to the blob created earlier in the request.
    assert_eq!(resp["list"], json!([{"id": blob, "size": 5}]));
    assert_eq!(resp["notFound"], json!(["nope", "#c2"]));
}

// ---- ranges and data -----------------------------------------------------

/// The `isTruncated` table of blobext-03 §5, through the handler.
#[tokio::test]
async fn is_truncated_table() {
    let b = backend();
    let blob = put(&b, b"0123456789").await;
    let cases: &[(Value, Value, &str, bool)] = &[
        (json!(10), Value::Null, "", false),
        (json!(11), Value::Null, "", true),
        (json!(4), Value::Null, "456789", false),
        (json!(4), json!(6), "456789", false),
        (json!(4), json!(7), "456789", true),
        (json!(1), json!(u64::MAX), "123456789", true),
        (json!(u64::MAX), json!(u64::MAX), "", true),
    ];
    for (offset, length, text, truncated) in cases {
        let resp = ok(
            &b,
            json!({"accountId": "a1", "ids": [blob], "properties": ["data:asText"],
                   "offset": offset, "length": length}),
        )
        .await;
        let item = &resp["list"][0];
        assert_eq!(
            item["data:asText"], *text,
            "offset {offset} length {length}"
        );
        assert_eq!(
            item.get("isTruncated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            *truncated,
            "offset {offset} length {length}"
        );
    }
}

#[tokio::test]
async fn split_utf8_is_an_encoding_problem() {
    let b = backend();
    // "a€" is 61 E2 82 AC; offset 0 length 3 cuts the euro sign.
    let blob = put(&b, "a€".as_bytes()).await;
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["data:asText"],
               "offset": 0, "length": 3}),
    )
    .await;
    assert_eq!(
        resp["list"][0],
        json!({"id": blob, "data:asText": null, "isEncodingProblem": true})
    );
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["data"],
               "offset": 0, "length": 3}),
    )
    .await;
    assert_eq!(
        resp["list"][0],
        json!({"id": blob, "data:asBase64": "YeKC", "isEncodingProblem": true})
    );
    // The whole blob is valid UTF-8; data:asBase64 never is a problem.
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["data", "data:asBase64"]}),
    )
    .await;
    assert_eq!(
        resp["list"][0],
        json!({"id": blob, "data:asText": "a€", "data:asBase64": "YeKCrA=="})
    );
}

// ---- digests -------------------------------------------------------------

/// RFC 9404 §4.2.1: the fox blob, whole and at offset 4 length 9.
#[tokio::test]
async fn rfc9404_digest_vectors() {
    let b = backend();
    let blob = put(&b, FOX.as_bytes()).await;
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["digest:sha", "size"]}),
    )
    .await;
    assert_eq!(
        resp["list"][0],
        json!({"id": blob, "digest:sha": "wIVPufsDxBzOOALLDSIFKebu+U4=", "size": 45})
    );
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob],
               "properties": ["data:asText", "digest:sha", "digest:sha-256"],
               "offset": 4, "length": 9}),
    )
    .await;
    assert_eq!(
        resp["list"][0],
        json!({
            "id": blob,
            "data:asText": "quick bro",
            "digest:sha": "QiRAPtfyX8K6tm1iOAtZ87Xj3Ww=",
            "digest:sha-256": "gdg9INW7lwHK6OQ9u0dwDz2ZY/gubi0En0xlFpKt0OA="
        })
    );
}

/// The digest covers exactly the octets returned, here a truncated range;
/// the whole-blob sha-256 the memory backend stores must not be used.
#[tokio::test]
async fn digest_over_truncated_range() {
    let b = backend();
    let blob = put(&b, FOX.as_bytes()).await;
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["digest:sha-256"],
               "offset": 10, "length": 1000}),
    )
    .await;
    assert_eq!(
        resp["list"][0],
        json!({
            "id": blob,
            "digest:sha-256": "W8FYqC96YVNXUMiyxKe7rHb1QCGCfdPd1YGlegUjNUU=",
            "isTruncated": true
        })
    );
    // Offset past the end: the digest of no octets (RFC 9530 App. B.2).
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["digest:sha-256"],
               "offset": 46}),
    )
    .await;
    assert_eq!(
        resp["list"][0]["digest:sha-256"],
        "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
    );
}

#[tokio::test]
async fn unadvertised_digest_is_invalid_arguments() {
    let b = backend();
    let blob = put(&b, b"hello").await;
    assert_eq!(
        err(
            &b,
            json!({"accountId": "a1", "ids": [blob], "properties": ["digest:md5"]})
        )
        .await,
        "invalidArguments"
    );
    assert_eq!(
        err(
            &b,
            json!({"accountId": "a1", "ids": [blob], "properties": ["size"],
                   "dataSourceProperties": ["digest:sha-512"]})
        )
        .await,
        "invalidArguments"
    );
}

#[tokio::test]
async fn advertised_digest_without_digester_is_server_fail() {
    let b = backend();
    let mut cap = MemoryBackend::default_capability();
    cap.supported_digest_algorithms.push(DigestAlgorithm::Md5);
    b.advertise_for_test(&id("a1"), cap);
    let blob = put(&b, b"hello").await;
    assert_eq!(
        err(
            &b,
            json!({"accountId": "a1", "ids": [blob], "properties": ["digest:md5"]})
        )
        .await,
        "serverFail"
    );
}

// ---- imageData and chunks ------------------------------------------------

#[tokio::test]
async fn image_data_is_null_when_unknown() {
    let b = backend();
    let blob = put(&b, b"not an image").await;
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["imageData"]}),
    )
    .await;
    assert_eq!(resp["list"][0], json!({"id": blob, "imageData": null}));
}

/// A non-chunked blob is one chunk covering itself; the default
/// dataSourceProperties are blobId, offset and length.
#[tokio::test]
async fn single_chunk_with_default_data_source_properties() {
    let b = backend();
    let blob = put(&b, b"hello").await;
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["chunks"]}),
    )
    .await;
    assert_eq!(
        resp["list"][0],
        json!({"id": blob, "chunks": [{"blobId": blob, "offset": 0, "length": 5}]})
    );
}

/// blobext-03 §9.2: a blob stored as two chunks, with sizes, positions
/// and per-chunk sha-256 digests over the octets each chunk contributes.
#[tokio::test]
async fn section_9_2_chunks() {
    let b = backend();
    let c1 = put(&b, b"0123456789").await;
    let c2 = put(&b, b"abcdef").await;
    let whole = b
        .create_blob(
            &(),
            &id("a1"),
            BlobCreate::new(
                vec![
                    BlobSource::Blob {
                        blob_id: c1.clone(),
                        range: 2..10,
                    },
                    BlobSource::Blob {
                        blob_id: c2.clone(),
                        range: 0..6,
                    },
                ],
                14,
            ),
        )
        .await
        .unwrap()
        .id;
    let resp = ok(
        &b,
        json!({
            "accountId": "a1",
            "ids": [whole],
            "properties": ["size", "chunks"],
            "dataSourceProperties": [
                "blobId", "size", "offset", "length", "position",
                "digest:sha-256"
            ]
        }),
    )
    .await;
    assert_eq!(
        resp,
        json!({
            "accountId": "a1",
            "list": [{
                "id": whole,
                "size": 14,
                "chunks": [
                    {
                        "blobId": c1,
                        "size": 10,
                        "offset": 2,
                        "length": 8,
                        "position": 0,
                        // sha-256("23456789")
                        "digest:sha-256": "8U8obKQ10fo7nYBB6PBqoK96so6o7c1+Ef1IWhALYys="
                    },
                    {
                        "blobId": c2,
                        "size": 6,
                        "offset": 0,
                        "length": 6,
                        "position": 8,
                        // sha-256("abcdef")
                        "digest:sha-256": "vvV+x/U6bUC+tkCngKY5yDvCmsipgW8fxsXG3Nk8RyE="
                    }
                ]
            }],
            "notFound": []
        })
    );
    // The data reads back through the chunks; chunks ignore the range.
    let resp = ok(
        &b,
        json!({"accountId": "a1", "ids": [whole], "properties": ["data", "chunks"],
               "offset": 6, "length": 4, "dataSourceProperties": ["position"]}),
    )
    .await;
    assert_eq!(
        resp["list"][0],
        json!({"id": whole, "data:asText": "89ab",
               "chunks": [{"position": 0}, {"position": 8}]})
    );
}

// ---- RFC 9404 mode (full conformance tests: bd JMAP-tna1.2.11) -------------

#[tokio::test]
async fn rfc9404_mode_basics() {
    let b = backend();
    let blob = put(&b, FOX.as_bytes()).await;
    let none = BTreeMap::new();
    // RFC 9404 §4.2: offset/length without properties default to data+size.
    let resp = call_with(
        &b,
        json!({"accountId": "a1", "ids": [blob], "offset": 4, "length": 9}),
        &none,
        &[BLOB],
    )
    .await
    .unwrap();
    assert_eq!(
        resp["list"][0],
        json!({"id": blob, "data:asText": "quick bro", "size": 45})
    );
    // chunks and imageData are not RFC 9404 properties.
    for p in ["chunks", "imageData"] {
        let r = call_with(
            &b,
            json!({"accountId": "a1", "ids": [blob], "properties": [p]}),
            &none,
            &[BLOB],
        )
        .await;
        assert_eq!(error_type(r), "invalidArguments", "{p}");
    }
    // dataSourceProperties is an unknown argument there, so it is ignored.
    let resp = call_with(
        &b,
        json!({"accountId": "a1", "ids": [blob], "properties": ["size"],
               "dataSourceProperties": ["nope"]}),
        &none,
        &[BLOB],
    )
    .await
    .unwrap();
    assert_eq!(resp["list"][0], json!({"id": blob, "size": 45}));
}
