//! Integration tests for jmap-filenode-types.
//!
//! All JSON fixtures are hand-written from draft-ietf-jmap-filenode-13 or
//! constructed directly from the spec field descriptions.  No expected value is
//! derived from the code under test.
//!
//! Structs are constructed exclusively via serde_json deserialization because all
//! public structs carry `#[non_exhaustive]`, which prevents struct literal syntax
//! outside the defining crate.

use jmap_filenode_types::{
    FileNode, FileNodeCapability, FileNodeFilterCondition, FilesRights, NodeRole, NodeType,
    JMAP_FILENODE_URI,
};
use jmap_types::Id;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn id(s: &str) -> Id {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).unwrap()
}

fn parse_rights(json: &str) -> FilesRights {
    serde_json::from_str(json).unwrap()
}

// ---------------------------------------------------------------------------
// NodeType enum
// ---------------------------------------------------------------------------

#[test]
fn node_type_known_values_deserialize() {
    let file: NodeType = serde_json::from_str("\"file\"").unwrap();
    assert_eq!(file, NodeType::File);

    let dir: NodeType = serde_json::from_str("\"directory\"").unwrap();
    assert_eq!(dir, NodeType::Directory);

    let sym: NodeType = serde_json::from_str("\"symlink\"").unwrap();
    assert_eq!(sym, NodeType::Symlink);
}

#[test]
fn node_type_unknown_becomes_other() {
    let other: NodeType = serde_json::from_str("\"hardlink\"").unwrap();
    assert_eq!(other, NodeType::Other("hardlink".to_owned()));
}

#[test]
fn node_type_roundtrip() {
    for val in &[
        NodeType::File,
        NodeType::Directory,
        NodeType::Symlink,
        NodeType::Other("future-type".to_owned()),
    ] {
        let json = serde_json::to_string(val).unwrap();
        let back: NodeType = serde_json::from_str(&json).unwrap();
        assert_eq!(val, &back);
    }
}

#[test]
fn node_type_wire_strings() {
    assert_eq!(NodeType::File.to_wire_str(), "file");
    assert_eq!(NodeType::Directory.to_wire_str(), "directory");
    assert_eq!(NodeType::Symlink.to_wire_str(), "symlink");
    assert_eq!(
        NodeType::Other("x-custom".to_owned()).to_wire_str(),
        "x-custom"
    );
}

// ---------------------------------------------------------------------------
// NodeRole enum
// ---------------------------------------------------------------------------

#[test]
fn node_role_known_values_deserialize() {
    let cases: &[(&str, NodeRole)] = &[
        ("\"root\"", NodeRole::Root),
        ("\"home\"", NodeRole::Home),
        ("\"temp\"", NodeRole::Temp),
        ("\"trash\"", NodeRole::Trash),
        ("\"documents\"", NodeRole::Documents),
        ("\"downloads\"", NodeRole::Downloads),
        ("\"music\"", NodeRole::Music),
        ("\"pictures\"", NodeRole::Pictures),
        ("\"videos\"", NodeRole::Videos),
    ];
    for (json, expected) in cases {
        let got: NodeRole = serde_json::from_str(json).unwrap();
        assert_eq!(&got, expected, "failed for {json}");
    }
}

#[test]
fn node_role_unknown_becomes_other() {
    let other: NodeRole = serde_json::from_str("\"bookmarks\"").unwrap();
    assert_eq!(other, NodeRole::Other("bookmarks".to_owned()));
}

#[test]
fn node_role_roundtrip() {
    for val in &[
        NodeRole::Root,
        NodeRole::Trash,
        NodeRole::Downloads,
        NodeRole::Other("x-vendor-role".to_owned()),
    ] {
        let json = serde_json::to_string(val).unwrap();
        let back: NodeRole = serde_json::from_str(&json).unwrap();
        assert_eq!(val, &back);
    }
}

// ---------------------------------------------------------------------------
// FilesRights struct
// ---------------------------------------------------------------------------

#[test]
fn files_rights_deserialize_from_spec() {
    // Hand-written JSON matching the §3.1 myRights field description.
    let rights = parse_rights(
        r#"{
        "mayRead": true,
        "mayAddChildren": false,
        "mayRename": true,
        "mayDelete": false,
        "mayModifyContent": true,
        "mayShare": false
    }"#,
    );
    assert!(rights.may_read);
    assert!(!rights.may_add_children);
    assert!(rights.may_rename);
    assert!(!rights.may_delete);
    assert!(rights.may_modify_content);
    assert!(!rights.may_share);
}

#[test]
fn files_rights_default_all_false() {
    let r = FilesRights::default();
    assert!(!r.may_read);
    assert!(!r.may_add_children);
    assert!(!r.may_rename);
    assert!(!r.may_delete);
    assert!(!r.may_modify_content);
    assert!(!r.may_share);
}

#[test]
fn files_rights_roundtrip() {
    // Build via serde; verify the round-trip is stable.
    let json = r#"{
        "mayRead": true,
        "mayAddChildren": true,
        "mayRename": false,
        "mayDelete": false,
        "mayModifyContent": true,
        "mayShare": true
    }"#;
    let r: FilesRights = serde_json::from_str(json).unwrap();
    let back: FilesRights = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(r, back);
}

// ---------------------------------------------------------------------------
// FileNode — file node (§3.1)
// ---------------------------------------------------------------------------

#[test]
fn filenode_file_deserialize() {
    // Constructed from §3.1 field descriptions for a file node.
    // parentId, blobId, target, size, type, shareWith are required-and-nullable.
    let json = r#"{
        "id": "fn1",
        "parentId": "dir1",
        "nodeType": "file",
        "blobId": "blob1",
        "target": null,
        "size": 4096,
        "name": "readme.txt",
        "type": "text/plain",
        "created": "2024-01-15T10:00:00Z",
        "modified": "2024-03-01T12:00:00Z",
        "accessed": "2024-03-10T08:00:00Z",
        "changed": "2024-03-01T12:00:00Z",
        "executable": false,
        "isSubscribed": true,
        "myRights": {
            "mayRead": true,
            "mayAddChildren": false,
            "mayRename": true,
            "mayDelete": true,
            "mayModifyContent": true,
            "mayShare": false
        },
        "shareWith": null,
        "role": null
    }"#;

    let node: FileNode = serde_json::from_str(json).unwrap();
    assert_eq!(node.id, id("fn1"));
    assert_eq!(node.parent_id, Some(id("dir1")));
    assert_eq!(node.node_type, Some(NodeType::File));
    assert_eq!(node.blob_id, Some(id("blob1")));
    assert_eq!(node.target, None);
    assert_eq!(node.size, Some(4096));
    assert_eq!(node.name, "readme.txt");
    assert_eq!(node.media_type, Some("text/plain".to_owned()));
    assert_eq!(node.created, Some("2024-01-15T10:00:00Z".to_owned()));
    assert_eq!(node.modified, Some("2024-03-01T12:00:00Z".to_owned()));
    assert_eq!(node.accessed, Some("2024-03-10T08:00:00Z".to_owned()));
    assert_eq!(node.changed, Some("2024-03-01T12:00:00Z".to_owned()));
    assert_eq!(node.executable, Some(false));
    assert_eq!(node.is_subscribed, Some(true));
    assert!(node.my_rights.as_ref().unwrap().may_read);
    assert!(!node.my_rights.as_ref().unwrap().may_add_children);
    assert_eq!(node.share_with, None);
    assert_eq!(node.role, None);
}

#[test]
fn filenode_directory_deserialize() {
    // §3.1: directory node — blobId null, target null, size null, type null.
    let json = r#"{
        "id": "dir1",
        "parentId": null,
        "blobId": null,
        "target": null,
        "size": null,
        "name": "My Files",
        "type": null,
        "shareWith": null,
        "role": "home"
    }"#;

    let node: FileNode = serde_json::from_str(json).unwrap();
    assert_eq!(node.id, id("dir1"));
    assert_eq!(node.parent_id, None);
    assert_eq!(node.node_type, None); // absent → None
    assert_eq!(node.blob_id, None);
    assert_eq!(node.target, None);
    assert_eq!(node.size, None);
    assert_eq!(node.name, "My Files");
    assert_eq!(node.media_type, None);
    assert_eq!(node.share_with, None);
    assert_eq!(node.role, Some(NodeRole::Home));
}

#[test]
fn filenode_symlink_deserialize() {
    // §3.1: symlink node — blobId null, target non-null, size null, type null.
    let json = r#"{
        "id": "sym1",
        "parentId": "dir1",
        "nodeType": "symlink",
        "blobId": null,
        "target": ["", "home", "alice", "docs"],
        "size": null,
        "name": "docs-link",
        "type": null,
        "shareWith": null
    }"#;

    let node: FileNode = serde_json::from_str(json).unwrap();
    assert_eq!(node.node_type, Some(NodeType::Symlink));
    assert_eq!(node.blob_id, None);
    assert_eq!(
        node.target,
        Some(vec![
            "".to_owned(),
            "home".to_owned(),
            "alice".to_owned(),
            "docs".to_owned()
        ])
    );
    assert_eq!(node.size, None);
    assert_eq!(node.media_type, None);
}

#[test]
fn filenode_unknown_node_type_becomes_other() {
    // §3.1: clients MUST NOT reject unrecognised nodeType values.
    let json = r#"{
        "id": "fn2",
        "parentId": null,
        "nodeType": "x-vendor-hardlink",
        "blobId": null,
        "target": null,
        "size": null,
        "name": "special",
        "type": null,
        "shareWith": null
    }"#;
    let node: FileNode = serde_json::from_str(json).unwrap();
    assert_eq!(
        node.node_type,
        Some(NodeType::Other("x-vendor-hardlink".to_owned()))
    );
}

// ---------------------------------------------------------------------------
// FileNode — nullable fields must serialize as null, not absent
// ---------------------------------------------------------------------------

#[test]
fn filenode_nullable_fields_serialize_as_null_not_absent() {
    // Deserialize a node where all nullable fields are null so that the Rust
    // fields hold None; then re-serialize and check that they appear as null.
    // `role` is typed `String|null` in §3.1 — required-nullable, must round-trip
    // as `null` not absent.
    let json = r#"{
        "id": "x",
        "parentId": null,
        "blobId": null,
        "target": null,
        "size": null,
        "name": "f",
        "type": null,
        "shareWith": null,
        "role": null
    }"#;
    let node: FileNode = serde_json::from_str(json).unwrap();

    let out = serde_json::to_string(&node).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let obj = v.as_object().unwrap();

    // Required-and-nullable: MUST be present as null.
    assert!(obj.contains_key("parentId"), "parentId must be present");
    assert!(obj["parentId"].is_null(), "parentId must be null");

    assert!(obj.contains_key("blobId"), "blobId must be present");
    assert!(obj["blobId"].is_null(), "blobId must be null");

    assert!(obj.contains_key("target"), "target must be present");
    assert!(obj["target"].is_null(), "target must be null");

    assert!(obj.contains_key("size"), "size must be present");
    assert!(obj["size"].is_null(), "size must be null");

    assert!(
        obj.contains_key("type"),
        "type (media_type) must be present"
    );
    assert!(obj["type"].is_null(), "type (media_type) must be null");

    assert!(obj.contains_key("shareWith"), "shareWith must be present");
    assert!(obj["shareWith"].is_null(), "shareWith must be null");

    // role is String|null (§3.1) — required-nullable, MUST appear as null.
    assert!(obj.contains_key("role"), "role must be present");
    assert!(obj["role"].is_null(), "role must be null");

    // Truly optional: MUST be absent when None.
    assert!(
        !obj.contains_key("nodeType"),
        "nodeType must be absent when None"
    );
    assert!(
        !obj.contains_key("created"),
        "created must be absent when None"
    );
    assert!(
        !obj.contains_key("modified"),
        "modified must be absent when None"
    );
    assert!(
        !obj.contains_key("accessed"),
        "accessed must be absent when None"
    );
    assert!(
        !obj.contains_key("changed"),
        "changed must be absent when None"
    );
    assert!(
        !obj.contains_key("executable"),
        "executable must be absent when None"
    );
    assert!(
        !obj.contains_key("isSubscribed"),
        "isSubscribed must be absent when None"
    );
    assert!(
        !obj.contains_key("myRights"),
        "myRights must be absent when None"
    );
}

#[test]
fn filenode_share_with_populated_map() {
    let json = r#"{
        "id": "fn3",
        "parentId": "dir1",
        "blobId": "blob2",
        "target": null,
        "size": 100,
        "name": "shared.pdf",
        "type": "application/pdf",
        "shareWith": {
            "user42": {
                "mayRead": true,
                "mayAddChildren": false,
                "mayRename": false,
                "mayDelete": false,
                "mayModifyContent": false,
                "mayShare": false
            }
        }
    }"#;
    let node: FileNode = serde_json::from_str(json).unwrap();
    let sw = node.share_with.as_ref().unwrap();
    assert_eq!(sw.len(), 1);
    let rights = sw.get(&id("user42")).unwrap();
    assert!(rights.may_read);
    assert!(!rights.may_share);
}

#[test]
fn filenode_roundtrip_file() {
    // Build via JSON deserialization, then serialize and deserialize again.
    // Verifies serde consistency for a fully-populated file node.
    let json = r#"{
        "id": "fn-rt",
        "parentId": "dir-rt",
        "nodeType": "file",
        "blobId": "b-rt",
        "target": null,
        "size": 2048,
        "name": "roundtrip.bin",
        "type": "application/octet-stream",
        "created": "2025-01-01T00:00:00Z",
        "modified": "2025-06-01T00:00:00Z",
        "changed": "2025-06-01T00:00:00Z",
        "executable": true,
        "isSubscribed": false,
        "myRights": {
            "mayRead": true,
            "mayAddChildren": false,
            "mayRename": true,
            "mayDelete": true,
            "mayModifyContent": true,
            "mayShare": true
        },
        "shareWith": {
            "u1": {
                "mayRead": true,
                "mayAddChildren": false,
                "mayRename": false,
                "mayDelete": false,
                "mayModifyContent": false,
                "mayShare": false
            }
        },
        "role": null
    }"#;

    let original: FileNode = serde_json::from_str(json).unwrap();
    let serialized = serde_json::to_string(&original).unwrap();
    let back: FileNode = serde_json::from_str(&serialized).unwrap();
    assert_eq!(original, back);
}

// ---------------------------------------------------------------------------
// FileNode — role field nullable semantics (§3.1 type: String|null)
// ---------------------------------------------------------------------------

#[test]
fn filenode_role_serializes_as_null() {
    // Oracle: §3.1 types `role` as `String|null` — required-nullable field.
    // A FileNode with role None must produce "role":null in wire JSON, not
    // absence of the key.
    let json = r#"{
        "id": "rn1",
        "parentId": null,
        "blobId": "blob-rn",
        "target": null,
        "size": 10,
        "name": "file.txt",
        "type": "text/plain",
        "shareWith": null,
        "role": null
    }"#;
    let node: FileNode = serde_json::from_str(json).expect("deserialize FileNode with role null");
    assert_eq!(node.role, None);

    let out = serde_json::to_string(&node).expect("serialize");
    let v: serde_json::Value = serde_json::from_str(&out).expect("parse output");
    let obj = v.as_object().expect("object");
    assert!(
        obj.contains_key("role"),
        "role must be present in serialized JSON"
    );
    assert!(obj["role"].is_null(), "role must serialize as null, not absent");
}

#[test]
fn filenode_role_null_round_trips() {
    // Oracle: §3.1 — role: null → deserialize → None → serialize → "role":null.
    let json = r#"{
        "id": "rn2",
        "parentId": null,
        "blobId": null,
        "target": null,
        "size": null,
        "name": "dir",
        "type": null,
        "shareWith": null,
        "role": null
    }"#;
    let node: FileNode = serde_json::from_str(json).expect("first deserialize");
    assert_eq!(node.role, None);

    let serialized = serde_json::to_string(&node).expect("serialize");
    let back: FileNode = serde_json::from_str(&serialized).expect("second deserialize");
    assert_eq!(back.role, None);

    let out2: serde_json::Value = serde_json::from_str(&serialized).expect("parse");
    assert!(
        out2.as_object().expect("object").contains_key("role"),
        "role must be present after round-trip"
    );
    assert!(out2["role"].is_null(), "role must remain null after round-trip");
}

// ---------------------------------------------------------------------------
// FileNodeCapability — §2.1 example
// ---------------------------------------------------------------------------

#[test]
fn capability_uri_constant() {
    assert_eq!(JMAP_FILENODE_URI, "urn:ietf:params:jmap:filenode");
}

#[test]
fn capability_deserialize_from_spec_example() {
    // Taken verbatim from draft-ietf-jmap-filenode-13 §2.1.1 capability example.
    let json = r#"{
        "maxFileNodeDepth": 50,
        "maxSizeFileNodeName": 255,
        "fileNodeQuerySortOptions": [
            "name", "type", "size", "created", "modified",
            "nodeType", "tree"
        ],
        "forbiddenNameChars": "/<>:\"\\|?*",
        "forbiddenNodeNames": [".", "..", "CON", "PRN", "AUX",
            "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5",
            "COM6", "COM7", "COM8", "COM9", "LPT0", "LPT1", "LPT2",
            "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"
        ],
        "caseInsensitiveNames": false,
        "mayCreateTopLevelFileNode": false,
        "webTrashUrl": "https://files.example.com/trash",
        "webUrlTemplate": "https://files.example.com/view/{id}",
        "webWriteUrlTemplate": "https://files.example.com/write/{id}"
    }"#;

    let cap: FileNodeCapability = serde_json::from_str(json).unwrap();
    assert_eq!(cap.max_file_node_depth, Some(50));
    assert_eq!(cap.max_size_file_node_name, 255);
    assert_eq!(
        cap.file_node_query_sort_options,
        vec!["name", "type", "size", "created", "modified", "nodeType", "tree"]
    );
    assert_eq!(cap.forbidden_name_chars, Some("/<>:\"\\|?*".to_owned()));
    assert!(cap
        .forbidden_node_names
        .as_ref()
        .unwrap()
        .contains(&"CON".to_owned()));
    assert!(cap
        .forbidden_node_names
        .as_ref()
        .unwrap()
        .contains(&".".to_owned()));
    assert!(!cap.case_insensitive_names);
    assert!(!cap.may_create_top_level_file_node);
    assert_eq!(
        cap.web_trash_url,
        Some("https://files.example.com/trash".to_owned())
    );
    assert_eq!(
        cap.web_url_template,
        Some("https://files.example.com/view/{id}".to_owned())
    );
    assert_eq!(
        cap.web_write_url_template,
        Some("https://files.example.com/write/{id}".to_owned())
    );
}

#[test]
fn capability_null_depth_and_nullables() {
    // maxFileNodeDepth: null means no limit.  Other nullable fields are also null.
    let json = r#"{
        "maxFileNodeDepth": null,
        "maxSizeFileNodeName": 100,
        "fileNodeQuerySortOptions": ["name"],
        "forbiddenNameChars": null,
        "forbiddenNodeNames": null,
        "caseInsensitiveNames": true,
        "mayCreateTopLevelFileNode": true,
        "webTrashUrl": null,
        "webUrlTemplate": null,
        "webWriteUrlTemplate": null
    }"#;

    let cap: FileNodeCapability = serde_json::from_str(json).unwrap();
    assert_eq!(cap.max_file_node_depth, None);
    assert_eq!(cap.forbidden_name_chars, None);
    assert_eq!(cap.forbidden_node_names, None);
    assert_eq!(cap.web_trash_url, None);
    assert_eq!(cap.web_url_template, None);
    assert_eq!(cap.web_write_url_template, None);
}

#[test]
fn capability_nullable_fields_serialize_as_null_not_absent() {
    // Deserialize a capability with all nullable fields set to null; verify
    // they re-serialize as `null` rather than being absent.
    let json = r#"{
        "maxFileNodeDepth": null,
        "maxSizeFileNodeName": 255,
        "fileNodeQuerySortOptions": ["name"],
        "forbiddenNameChars": null,
        "forbiddenNodeNames": null,
        "caseInsensitiveNames": false,
        "mayCreateTopLevelFileNode": false,
        "webTrashUrl": null,
        "webUrlTemplate": null,
        "webWriteUrlTemplate": null
    }"#;

    let cap: FileNodeCapability = serde_json::from_str(json).unwrap();
    let out = serde_json::to_string(&cap).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let obj = v.as_object().unwrap();

    // All of these are nullable → must be present as null.
    assert!(
        obj.contains_key("maxFileNodeDepth"),
        "maxFileNodeDepth must be present"
    );
    assert!(obj["maxFileNodeDepth"].is_null());

    assert!(
        obj.contains_key("forbiddenNameChars"),
        "forbiddenNameChars must be present"
    );
    assert!(obj["forbiddenNameChars"].is_null());

    assert!(
        obj.contains_key("forbiddenNodeNames"),
        "forbiddenNodeNames must be present"
    );
    assert!(obj["forbiddenNodeNames"].is_null());

    assert!(
        obj.contains_key("webTrashUrl"),
        "webTrashUrl must be present"
    );
    assert!(obj["webTrashUrl"].is_null());

    assert!(
        obj.contains_key("webUrlTemplate"),
        "webUrlTemplate must be present"
    );
    assert!(obj["webUrlTemplate"].is_null());

    assert!(
        obj.contains_key("webWriteUrlTemplate"),
        "webWriteUrlTemplate must be present"
    );
    assert!(obj["webWriteUrlTemplate"].is_null());
}

#[test]
fn capability_roundtrip() {
    let json = r#"{
        "maxFileNodeDepth": 100,
        "maxSizeFileNodeName": 255,
        "fileNodeQuerySortOptions": ["name", "size"],
        "forbiddenNameChars": "/\\",
        "forbiddenNodeNames": [".", ".."],
        "caseInsensitiveNames": false,
        "mayCreateTopLevelFileNode": true,
        "webTrashUrl": "https://example.com/trash",
        "webUrlTemplate": "https://example.com/view/{id}",
        "webWriteUrlTemplate": null
    }"#;
    let cap: FileNodeCapability = serde_json::from_str(json).unwrap();
    let serialized = serde_json::to_string(&cap).unwrap();
    let back: FileNodeCapability = serde_json::from_str(&serialized).unwrap();
    assert_eq!(cap, back);
}

// ---------------------------------------------------------------------------
// FileNodeFilterCondition — §3.2.5
// ---------------------------------------------------------------------------

#[test]
fn filter_condition_type_field_uses_rename() {
    // The wire field for media_type is literally "type" — verify the rename works.
    let json = r#"{"type": "text/plain"}"#;
    let cond: FileNodeFilterCondition = serde_json::from_str(json).unwrap();
    assert_eq!(cond.media_type, Some("text/plain".to_owned()));

    let out = serde_json::to_string(&cond).unwrap();
    assert!(
        out.contains("\"type\":"),
        "serialized key must be \"type\", got: {out}"
    );
    assert!(
        !out.contains("\"media_type\":"),
        "must not contain Rust field name"
    );
}

#[test]
fn filter_condition_all_fields_deserialize() {
    // Hand-written JSON covering every field of FileNodeFilterCondition.
    let json = r#"{
        "isTopLevel": true,
        "parentId": "dir1",
        "ancestorId": "root1",
        "descendantId": "child1",
        "nodeType": "file",
        "role": "home",
        "hasAnyRole": false,
        "blobId": "blob1",
        "isExecutable": true,
        "createdBefore": "2025-01-01T00:00:00Z",
        "createdAfter": "2020-01-01T00:00:00Z",
        "modifiedBefore": "2025-06-01T00:00:00Z",
        "modifiedAfter": "2021-01-01T00:00:00Z",
        "accessedBefore": "2025-12-01T00:00:00Z",
        "accessedAfter": "2022-01-01T00:00:00Z",
        "minSize": 1024,
        "maxSize": 1048576,
        "name": "readme.txt",
        "nameMatch": "*.txt",
        "type": "text/plain",
        "typeMatch": "text/*",
        "body": "hello world",
        "text": "search term"
    }"#;

    let cond: FileNodeFilterCondition = serde_json::from_str(json).unwrap();
    assert_eq!(cond.is_top_level, Some(true));
    assert_eq!(cond.parent_id, Some(id("dir1")));
    assert_eq!(cond.ancestor_id, Some(id("root1")));
    assert_eq!(cond.descendant_id, Some(id("child1")));
    assert_eq!(cond.node_type, Some("file".to_owned()));
    assert_eq!(cond.role, Some("home".to_owned()));
    assert_eq!(cond.has_any_role, Some(false));
    assert_eq!(cond.blob_id, Some(id("blob1")));
    assert_eq!(cond.is_executable, Some(true));
    assert_eq!(cond.created_before, Some("2025-01-01T00:00:00Z".to_owned()));
    assert_eq!(cond.created_after, Some("2020-01-01T00:00:00Z".to_owned()));
    assert_eq!(
        cond.modified_before,
        Some("2025-06-01T00:00:00Z".to_owned())
    );
    assert_eq!(cond.modified_after, Some("2021-01-01T00:00:00Z".to_owned()));
    assert_eq!(
        cond.accessed_before,
        Some("2025-12-01T00:00:00Z".to_owned())
    );
    assert_eq!(cond.accessed_after, Some("2022-01-01T00:00:00Z".to_owned()));
    assert_eq!(cond.min_size, Some(1024));
    assert_eq!(cond.max_size, Some(1048576));
    assert_eq!(cond.name, Some("readme.txt".to_owned()));
    assert_eq!(cond.name_match, Some("*.txt".to_owned()));
    assert_eq!(cond.media_type, Some("text/plain".to_owned()));
    assert_eq!(cond.type_match, Some("text/*".to_owned()));
    assert_eq!(cond.body, Some("hello world".to_owned()));
    assert_eq!(cond.text, Some("search term".to_owned()));
}

#[test]
fn filter_condition_empty_deserialize() {
    // A filter condition with no fields set matches everything.
    let cond: FileNodeFilterCondition = serde_json::from_str("{}").unwrap();
    assert_eq!(cond.is_top_level, None);
    assert_eq!(cond.parent_id, None);
    assert_eq!(cond.media_type, None);
}

#[test]
fn filter_condition_optional_fields_absent_when_none() {
    // All-None condition should serialize to `{}` (no fields present).
    let cond: FileNodeFilterCondition = serde_json::from_str("{}").unwrap();
    let json = serde_json::to_string(&cond).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = v.as_object().unwrap();
    assert!(
        obj.is_empty(),
        "default filter condition should serialize to empty object, got: {json}"
    );
}

#[test]
fn filter_condition_roundtrip() {
    let json = r#"{
        "isTopLevel": false,
        "parentId": "p1",
        "nodeType": "file",
        "minSize": 512,
        "type": "image/png",
        "nameMatch": "*.png"
    }"#;
    let cond: FileNodeFilterCondition = serde_json::from_str(json).unwrap();
    let serialized = serde_json::to_string(&cond).unwrap();
    let back: FileNodeFilterCondition = serde_json::from_str(&serialized).unwrap();

    assert_eq!(cond.is_top_level, back.is_top_level);
    assert_eq!(cond.parent_id, back.parent_id);
    assert_eq!(cond.node_type, back.node_type);
    assert_eq!(cond.min_size, back.min_size);
    assert_eq!(cond.media_type, back.media_type);
    assert_eq!(cond.name_match, back.name_match);
    assert_eq!(cond.ancestor_id, back.ancestor_id); // None
}
