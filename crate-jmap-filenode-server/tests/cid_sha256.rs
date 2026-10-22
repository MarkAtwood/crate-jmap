//! `FileNode/set` and `FileNode/copy` fill `sha256` when `urn:ietf:params:jmap:cid` is
//! advertised (draft-atwood-jmap-cid-00 §FileNode Extension).
//!
//! Oracle: the draft text at jmap-chat-spec/draft-atwood-jmap-cid-00.md,
//! quoted with line numbers at each assertion, and the SHA-256 example
//! digests published with FIPS 180-4 (NIST CSRC "Examples with
//! Intermediate Values", SHA-256): the one-block message "abc" and the
//! two-block message "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq".
//! `MemoryBackend::seed_blob_sha256` stands in for a blob store holding
//! those messages.

mod common;

use common::MemoryBackend;
use jmap_cid_types::Sha256;
use jmap_filenode_server::{handle_filenode_copy, handle_filenode_get, handle_filenode_set};
use jmap_filenode_types::FileNode;
use jmap_server::HandlerOutput;
use serde_json::{json, Value};

/// FIPS 180-4 example: SHA-256("abc").
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
/// FIPS 180-4 example: SHA-256 of the 448-bit two-block message.
const SHA256_TWO_BLOCK: &str = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";

fn digest(hex: &str) -> Sha256 {
    Sha256::from_hex(hex).expect("FIPS 180-4 vector is 64 lowercase hex digits")
}

async fn set(backend: &MemoryBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_filenode_set(backend, &(), args)
        .await
        .expect("set must not return a top-level error");
    primary
}

async fn get_one(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary, .. } =
        handle_filenode_get(backend, &(), json!({ "accountId": "acc1", "ids": [id] }))
            .await
            .expect("get must not return a top-level error");
    primary["list"][0].clone()
}

fn create_file(blob_id: &str, extra: Value) -> Value {
    let mut file = json!({
        "@type": "File",
        "name": "msg.txt",
        "parentId": null,
        "blobId": blob_id,
        "mediaType": "text/plain",
    });
    if let (Value::Object(m), Value::Object(e)) = (&mut file, extra) {
        m.extend(e);
    }
    json!({ "accountId": "acc1", "create": { "c1": file } })
}

fn seeded_file(blob_id: &str, sha256: Option<&str>) -> FileNode {
    serde_json::from_value(json!({
        "id": "f1",
        "parentId": null,
        "@type": "File",
        "name": "msg.txt",
        "blobId": blob_id,
        "mediaType": "text/plain",
        "sha256": sha256,
    }))
    .expect("hand-written File node deserializes")
}

/// L215-217: "For file FileNodes, the server MUST set this to the SHA-256
/// digest of the stored blob content, encoded as defined in {{upload}}, at
/// the time the FileNode was created or last updated."
#[tokio::test]
async fn create_with_cid_sets_sha256_of_blob() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));

    let resp = set(&backend, create_file("blob-abc", json!({}))).await;
    let created = &resp["created"]["c1"];
    assert_eq!(created["sha256"], SHA256_ABC, "{resp}");

    let id = created["id"].as_str().expect("created id");
    assert_eq!(get_one(&backend, id).await["sha256"], SHA256_ABC);
}

/// L215-217 (as above): the value is the digest of the stored blob, so a
/// client-supplied `sha256` does not survive.
#[tokio::test]
async fn create_with_cid_replaces_client_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));

    let resp = set(
        &backend,
        create_file("blob-abc", json!({ "sha256": SHA256_TWO_BLOCK })),
    )
    .await;
    assert_eq!(resp["created"]["c1"]["sha256"], SHA256_ABC, "{resp}");
}

/// L206-208: "When a server advertises both `urn:ietf:params:jmap:cid` and
/// the {{JMAP-FILENODE}} capability, FileNode objects MUST include the
/// following additional property". Without the capability there is no
/// `sha256`, whatever the client sent.
#[tokio::test]
async fn create_without_cid_has_no_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");

    let resp = set(
        &backend,
        create_file("blob-abc", json!({ "sha256": SHA256_ABC })),
    )
    .await;
    let created = &resp["created"]["c1"];
    assert!(created.get("sha256").is_none(), "{resp}");

    let id = created["id"].as_str().expect("created id");
    assert!(get_one(&backend, id).await.get("sha256").is_none());
}

/// L222-224: "When a `FileNode/set` ({{JMAP-FILENODE}}) create or update
/// changes the associated blob, the server MUST update `sha256` to reflect
/// the new blob content before returning the response."
#[tokio::test]
async fn update_blob_with_cid_updates_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));
    backend.seed_blob_sha256("acc1", "blob-two", digest(SHA256_TWO_BLOCK));
    backend.seed_node("acc1", seeded_file("blob-abc", Some(SHA256_ABC)));

    let resp = set(
        &backend,
        json!({ "accountId": "acc1", "update": { "f1": { "blobId": "blob-two" } } }),
    )
    .await;
    // RFC 8620 §5.3: the updated value includes "any properties that
    // changed that were not explicitly requested by the client".
    assert_eq!(resp["updated"]["f1"]["sha256"], SHA256_TWO_BLOCK, "{resp}");
    assert_eq!(get_one(&backend, "f1").await["sha256"], SHA256_TWO_BLOCK);
}

/// L222-224 (as above): `sha256` must reflect the new blob, so a digest of
/// the old blob is dropped when the new blob has none (L206-208: no
/// capability, no property).
#[tokio::test]
async fn update_blob_without_cid_drops_old_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_node("acc1", seeded_file("blob-abc", Some(SHA256_ABC)));

    let resp = set(
        &backend,
        json!({ "accountId": "acc1", "update": { "f1": { "blobId": "blob-other" } } }),
    )
    .await;
    assert!(resp["updated"].get("f1").is_some(), "{resp}");
    let node = get_one(&backend, "f1").await;
    assert_eq!(node["blobId"], "blob-other");
    assert!(node.get("sha256").is_none(), "{node}");
}

/// L215-217: "the server MUST set this to the SHA-256 digest of the stored
/// blob content", and RFC 8620 §5.3: a server-set property in a patch whose
/// value differs from the current one means "the update MUST be rejected
/// with an `invalidProperties` SetError". Here the capability is off, so
/// the stored node has no `sha256` (L206-208) and a client digest differs.
#[tokio::test]
async fn update_client_sha256_without_cid_is_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_node("acc1", seeded_file("blob-abc", None));

    let resp = set(
        &backend,
        json!({ "accountId": "acc1", "update": { "f1": { "sha256": SHA256_ABC } } }),
    )
    .await;
    let err = &resp["notUpdated"]["f1"];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["sha256"]), "{resp}");
    assert!(get_one(&backend, "f1").await.get("sha256").is_none());
}

/// As above, with the capability on: a client digest that differs from the
/// stored blob's is rejected and the stored digest is kept.
#[tokio::test]
async fn update_client_sha256_differing_is_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));
    backend.seed_node("acc1", seeded_file("blob-abc", Some(SHA256_ABC)));

    let resp = set(
        &backend,
        json!({ "accountId": "acc1", "update": { "f1": { "sha256": SHA256_TWO_BLOCK } } }),
    )
    .await;
    let err = &resp["notUpdated"]["f1"];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["sha256"]), "{resp}");
    assert_eq!(get_one(&backend, "f1").await["sha256"], SHA256_ABC);
}

/// RFC 8620 §5.3: "Any server-set properties MAY be included in the patch
/// if their value is identical to the current server value".
#[tokio::test]
async fn update_client_sha256_identical_is_accepted() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));
    backend.seed_node("acc1", seeded_file("blob-abc", Some(SHA256_ABC)));

    let resp = set(
        &backend,
        json!({ "accountId": "acc1", "update": { "f1": { "sha256": SHA256_ABC, "name": "renamed.txt" } } }),
    )
    .await;
    assert!(resp["updated"].get("f1").is_some(), "{resp}");
    let node = get_one(&backend, "f1").await;
    assert_eq!(node["name"], "renamed.txt");
    assert_eq!(node["sha256"], SHA256_ABC);
}

/// L212-213: `sha256` "MUST be `null` for directory FileNodes (FileNodes
/// with no associated blob)", whatever the client sent.
#[tokio::test]
async fn create_directory_drops_client_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");

    let resp = set(
        &backend,
        json!({ "accountId": "acc1", "create": { "d1": {
            "@type": "Directory",
            "name": "docs",
            "parentId": null,
            "sha256": SHA256_ABC,
        } } }),
    )
    .await;
    let id = resp["created"]["d1"]["id"].as_str().expect("created id");
    assert!(
        get_one(&backend, id).await.get("sha256").is_none(),
        "{resp}"
    );
}

fn seeded_directory() -> FileNode {
    serde_json::from_value(json!({
        "id": "d1",
        "parentId": null,
        "@type": "Directory",
        "name": "docs",
    }))
    .expect("hand-written Directory node deserializes")
}

/// Spec 6bcd9d8 L200-207: "When a server advertises both
/// `urn:ietf:params:jmap:cid` and the {{JMAP-FILENODE}} capability,
/// FileNode objects MUST include" `sha256`, which "MUST be `null` for
/// directory FileNodes (FileNodes with no associated blob)".
#[tokio::test]
async fn get_directory_with_cid_has_null_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));
    backend.seed_node("acc1", seeded_directory());

    let node = get_one(&backend, "d1").await;
    assert_eq!(node.get("sha256"), Some(&Value::Null), "{node}");
}

/// Spec 6bcd9d8 L210-212: "A server MAY return `null` for a file FileNode
/// whose digest was not computed at the time of creation"; the property is
/// still one the FileNode "MUST include" (L201).
#[tokio::test]
async fn get_legacy_file_with_cid_has_null_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));
    backend.seed_node("acc1", seeded_file("blob-legacy", None));

    let node = get_one(&backend, "f1").await;
    assert_eq!(node.get("sha256"), Some(&Value::Null), "{node}");
}

/// Spec 6bcd9d8 L200: the property exists only "When a server advertises"
/// the capability, so without it a directory has no `sha256`.
#[tokio::test]
async fn get_directory_without_cid_has_no_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_node("acc1", seeded_directory());

    let node = get_one(&backend, "d1").await;
    assert!(node.get("sha256").is_none(), "{node}");
}

/// RFC 8620 §5.1 `properties`: "If supplied, only the properties listed in
/// the array are returned for each object."
#[tokio::test]
async fn get_with_cid_respects_properties() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));
    backend.seed_node("acc1", seeded_directory());

    let HandlerOutput { primary, .. } = handle_filenode_get(
        &backend,
        &(),
        json!({ "accountId": "acc1", "ids": ["d1"], "properties": ["name"] }),
    )
    .await
    .expect("get must not return a top-level error");
    let node = &primary["list"][0];
    assert_eq!(node["name"], "docs", "{primary}");
    assert!(node.get("sha256").is_none(), "{primary}");
}

/// Spec 33702a5 L200-206: with the capability, FileNode objects "MUST
/// include" `sha256`, which "MUST be `null` for directory FileNodes", and
/// RFC 8620 §5.3: `created` holds "any properties that were omitted by the
/// client and thus set to a default by the server".
#[tokio::test]
async fn create_directory_with_cid_created_has_null_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));

    let resp = set(
        &backend,
        json!({ "accountId": "acc1", "create": { "d1": {
            "@type": "Directory",
            "name": "docs",
            "parentId": null,
        } } }),
    )
    .await;
    let created = &resp["created"]["d1"];
    assert_eq!(created.get("sha256"), Some(&Value::Null), "{resp}");
}

/// Spec 33702a5 L200-206 (as above) applies to every FileNode object in a
/// FileNode/get response, including the ancestor directories that
/// draft-ietf-jmap-filenode `fetchParents` appends to `list`.
#[tokio::test]
async fn get_fetch_parents_with_cid_ancestor_has_null_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));
    backend.seed_node("acc1", seeded_directory());
    let mut file = seeded_file("blob-abc", Some(SHA256_ABC));
    file.parent_id = Some("d1".into());
    backend.seed_node("acc1", file);

    let HandlerOutput { primary, .. } = handle_filenode_get(
        &backend,
        &(),
        json!({ "accountId": "acc1", "ids": ["f1"], "fetchParents": true }),
    )
    .await
    .expect("get must not return a top-level error");
    let list = primary["list"].as_array().expect("list");
    let dir = list
        .iter()
        .find(|n| n["id"] == "d1")
        .unwrap_or_else(|| panic!("ancestor d1 appended: {primary}"));
    assert_eq!(dir.get("sha256"), Some(&Value::Null), "{primary}");
    let leaf = list.iter().find(|n| n["id"] == "f1").expect("f1 listed");
    assert_eq!(leaf["sha256"], SHA256_ABC, "{primary}");
}

/// RFC 8620 §5.1 `properties`: "If supplied, only the properties listed in
/// the array are returned for each object"; `sha256` is listed, so the
/// directory's null (spec 33702a5 L205-206) is returned.
#[tokio::test]
async fn get_with_cid_properties_including_sha256() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));
    backend.seed_node("acc1", seeded_directory());

    let HandlerOutput { primary, .. } = handle_filenode_get(
        &backend,
        &(),
        json!({ "accountId": "acc1", "ids": ["d1"], "properties": ["sha256", "name"] }),
    )
    .await
    .expect("get must not return a top-level error");
    let node = &primary["list"][0];
    assert_eq!(node["name"], "docs", "{primary}");
    assert_eq!(node.get("sha256"), Some(&Value::Null), "{primary}");
}

/// RFC 8620 §5.1 `properties`: "If null, all properties of the object are
/// returned", so the directory's null `sha256` (spec 33702a5 L205-206) is
/// returned.
#[tokio::test]
async fn get_with_cid_properties_null() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_blob_sha256("acc1", "blob-abc", digest(SHA256_ABC));
    backend.seed_node("acc1", seeded_directory());

    let HandlerOutput { primary, .. } = handle_filenode_get(
        &backend,
        &(),
        json!({ "accountId": "acc1", "ids": ["d1"], "properties": null }),
    )
    .await
    .expect("get must not return a top-level error");
    let node = &primary["list"][0];
    assert_eq!(node.get("sha256"), Some(&Value::Null), "{primary}");
}

/// Copies source node `id` from account `src` into account `dst` with the
/// given RFC 8620 §5.4 per-copy overrides.
async fn copy(backend: &MemoryBackend, id: &str, overrides: Value) -> Value {
    let mut entry = json!({ "id": id });
    if let (Value::Object(m), Value::Object(o)) = (&mut entry, overrides) {
        m.extend(o);
    }
    let HandlerOutput { primary, .. } = handle_filenode_copy(
        backend,
        &(),
        json!({ "fromAccountId": "src", "accountId": "dst", "create": { "c1": entry } }),
        "c0",
    )
    .await
    .expect("copy must not return a top-level error");
    primary
}

/// Spec 33702a5 L208-214: "For file FileNodes, the server MUST set this to
/// the SHA-256 digest of the stored blob content ... MUST NOT return `null`
/// for any file FileNode created ... while this capability is active." A
/// copy is a create in the destination (RFC 8620 §5.4), so a source node
/// with no digest gets the destination blob's.
#[tokio::test]
async fn copy_file_with_cid_sets_sha256_of_blob() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");
    backend.seed_node("src", seeded_file("blob-abc", None));
    backend.seed_blob_sha256("dst", "blob-abc", digest(SHA256_ABC));

    let resp = copy(&backend, "f1", json!({})).await;
    let created = &resp["created"]["c1"];
    assert_eq!(created["sha256"], SHA256_ABC, "{resp}");
}

/// Spec 33702a5 L208-210 (as above): the value is the digest of the stored
/// blob, so an override's `sha256` does not survive the copy.
#[tokio::test]
async fn copy_file_with_cid_replaces_override_sha256() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");
    backend.seed_node("src", seeded_file("blob-abc", Some(SHA256_ABC)));
    backend.seed_blob_sha256("dst", "blob-abc", digest(SHA256_ABC));

    let resp = copy(&backend, "f1", json!({ "sha256": SHA256_TWO_BLOCK })).await;
    assert_eq!(resp["created"]["c1"]["sha256"], SHA256_ABC, "{resp}");
}

/// Spec 33702a5 L200-202: `sha256` exists only "When a server advertises
/// both `urn:ietf:params:jmap:cid` and the {{JMAP-FILENODE}} capability",
/// so a copy into an account without it carries none, even when the source
/// had one.
#[tokio::test]
async fn copy_file_without_cid_has_no_sha256() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");
    backend.seed_node("src", seeded_file("blob-abc", Some(SHA256_ABC)));

    let resp = copy(&backend, "f1", json!({})).await;
    let created = &resp["created"]["c1"];
    assert!(created.get("id").is_some(), "{resp}");
    assert!(created.get("sha256").is_none(), "{resp}");
}

/// Spec 33702a5 L200-207: with the capability, FileNode objects "MUST
/// include" `sha256`, which "MUST be `null` for directory FileNodes", and
/// RFC 8620 §5.4: `created` is as for /set, holding server-set properties.
#[tokio::test]
async fn copy_directory_with_cid_created_has_null_sha256() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");
    backend.seed_node("src", seeded_directory());
    backend.seed_blob_sha256("dst", "blob-abc", digest(SHA256_ABC));

    let resp = copy(&backend, "d1", json!({})).await;
    let created = &resp["created"]["c1"];
    assert_eq!(created.get("sha256"), Some(&Value::Null), "{resp}");
}
