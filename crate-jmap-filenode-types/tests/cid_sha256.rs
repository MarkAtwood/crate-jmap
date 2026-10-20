//! `FileNode.sha256` tests (draft-atwood-jmap-cid-00 §FileNode Extension).
//!
//! Oracle: the draft text at jmap-chat-spec/draft-atwood-jmap-cid-00.md,
//! quoted with line numbers at each assertion, and hand-written JSON. The
//! digest value is the draft's own example (L158), the SHA-256 of `"test"`.

use jmap_filenode_types::{FileNode, NodeType};
use serde_json::{json, Value};

/// draft-atwood-jmap-cid-00 L158: the `sha256` example value.
const DRAFT_SHA256: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

fn file_node(sha256: Value) -> Value {
    json!({
        "id": "fn1",
        "parentId": "dir1",
        "@type": "File",
        "blobId": "blob1",
        "size": 4,
        "sha256": sha256,
        "name": "test.txt",
        "mediaType": "text/plain",
        "executable": false,
        "isSubscribed": true,
        "shareWith": null
    })
}

/// L210-212: "`sha256` (String|null): The SHA-256 digest of the blob
/// associated with this FileNode, encoded as a lowercase hexadecimal string
/// of exactly 64 characters."
#[test]
fn file_sha256_round_trips() {
    let wire = file_node(json!(DRAFT_SHA256));
    let node: FileNode = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(node.sha256.as_ref().map(|d| d.as_str()), Some(DRAFT_SHA256));
    assert!(node.extra.is_empty(), "sha256 must be typed, not an extra");
    assert_eq!(serde_json::to_value(&node).unwrap(), wire);
}

/// L106: "sha256-value = 64( %x30-39 / %x61-66 )"; L211-212: "a lowercase
/// hexadecimal string of exactly 64 characters".
#[test]
fn malformed_sha256_is_rejected() {
    let upper = DRAFT_SHA256.to_ascii_uppercase();
    let short = &DRAFT_SHA256[..63];
    let non_hex = format!("{}g", &DRAFT_SHA256[..63]);
    for bad in [upper.as_str(), short, non_hex.as_str(), ""] {
        let r = serde_json::from_value::<FileNode>(file_node(json!(bad)));
        assert!(r.is_err(), "accepted malformed sha256 {bad:?}");
    }
    assert!(serde_json::from_value::<FileNode>(file_node(json!(42))).is_err());
}

/// L212-213: "MUST be `null` for directory FileNodes (FileNodes with no
/// associated blob)."
#[test]
fn directory_null_sha256_deserializes_to_none() {
    let node: FileNode = serde_json::from_value(json!({
        "id": "d1", "parentId": null, "@type": "Directory", "name": "docs",
        "role": null, "shareWith": null, "sha256": null
    }))
    .unwrap();
    assert_eq!(node.node_type, Some(NodeType::Directory));
    assert_eq!(node.sha256, None);
    assert!(node.extra.is_empty());
}

/// L212-213: "MUST be `null` for directory FileNodes (FileNodes with no
/// associated blob)." A digest on a blob-less node is never emitted.
#[test]
fn sha256_not_emitted_for_blobless_node_types() {
    for (ty, extra) in [
        ("Directory", json!({ "role": null })),
        ("SymLink", json!({ "target": ["a"] })),
    ] {
        let mut wire = json!({
            "id": "n1", "parentId": null, "@type": ty, "name": "n",
            "shareWith": null, "sha256": DRAFT_SHA256
        });
        for (k, v) in extra.as_object().unwrap() {
            wire[k] = v.clone();
        }
        let node: FileNode = serde_json::from_value(wire).unwrap();
        let out = serde_json::to_value(&node).unwrap();
        assert!(out.get("sha256").is_none(), "{ty}: {out}");
    }
}

/// L216-217: "A server MAY return `null` for a file FileNode whose digest
/// was not computed at the time of creation". L206-208 makes the property
/// conditional on "a server advertises both `urn:ietf:params:jmap:cid` and
/// the {{JMAP-FILENODE}} capability", so an absent `sha256` is valid too.
#[test]
fn legacy_null_or_absent_sha256_is_none_and_omitted() {
    let node: FileNode = serde_json::from_value(file_node(Value::Null)).unwrap();
    assert_eq!(node.sha256, None);

    let mut absent = file_node(Value::Null);
    absent.as_object_mut().unwrap().remove("sha256");
    let node: FileNode = serde_json::from_value(absent.clone()).unwrap();
    assert_eq!(node.sha256, None);
    assert_eq!(serde_json::to_value(&node).unwrap(), absent);
}
