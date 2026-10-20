//! Integration tests for jmap-filenode-types.
//!
//! All JSON fixtures are hand-written from draft-ietf-jmap-filenode-16 or
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
use jmap_types::{Id, UTCDate};

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
    // §3.1.1: the defined @type values are "File", "Directory", and "SymLink".
    let file: NodeType = serde_json::from_str("\"File\"").unwrap();
    assert_eq!(file, NodeType::File);

    let dir: NodeType = serde_json::from_str("\"Directory\"").unwrap();
    assert_eq!(dir, NodeType::Directory);

    let sym: NodeType = serde_json::from_str("\"SymLink\"").unwrap();
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
    assert_eq!(NodeType::File.to_wire_str(), "File");
    assert_eq!(NodeType::Directory.to_wire_str(), "Directory");
    assert_eq!(NodeType::Symlink.to_wire_str(), "SymLink");
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
    // Constructed from §3.1.1 and §3.1.2 field descriptions for a File node.
    let json = r#"{
        "@type": "File",
        "id": "fn1",
        "parentId": "dir1",
        "blobId": "blob1",
        "size": 4096,
        "name": "readme.txt",
        "mediaType": "text/plain",
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
        "shareWith": null
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
    assert_eq!(node.created, Some(UTCDate::from("2024-01-15T10:00:00Z")));
    assert_eq!(node.modified, Some(UTCDate::from("2024-03-01T12:00:00Z")));
    assert_eq!(node.accessed, Some(UTCDate::from("2024-03-10T08:00:00Z")));
    assert_eq!(node.changed, Some(UTCDate::from("2024-03-01T12:00:00Z")));
    assert_eq!(node.executable, Some(false));
    assert_eq!(node.is_subscribed, Some(true));
    assert!(node.my_rights.as_ref().unwrap().may_read);
    assert!(!node.my_rights.as_ref().unwrap().may_add_children);
    assert_eq!(node.share_with, None);
    assert_eq!(node.role, None);
}

#[test]
fn filenode_directory_deserialize() {
    // §3.1.3: a Directory carries no File or SymLink properties.
    let json = r#"{
        "@type": "Directory",
        "id": "dir1",
        "parentId": null,
        "name": "My Files",
        "shareWith": null,
        "role": "home"
    }"#;

    let node: FileNode = serde_json::from_str(json).unwrap();
    assert_eq!(node.id, id("dir1"));
    assert_eq!(node.parent_id, None);
    assert_eq!(node.node_type, Some(NodeType::Directory));
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
    // §3.1.4: a SymLink carries `target` and no File or Directory properties.
    let json = r#"{
        "@type": "SymLink",
        "id": "sym1",
        "parentId": "dir1",
        "target": ["", "home", "alice", "docs"],
        "name": "docs-link",
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
    // §3.1.1: clients MUST be prepared to receive unrecognised @type values.
    let json = r#"{
        "@type": "x-vendor-hardlink",
        "id": "fn2",
        "parentId": null,
        "name": "special",
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
    // §3.1.1: parentId and shareWith are `|null` common properties; §3.1.3: role
    // is `String|null` on a Directory.  All three must round-trip as `null`.
    // §3.1: File and SymLink properties MUST NOT appear on a Directory.
    let json = r#"{
        "@type": "Directory",
        "id": "x",
        "parentId": null,
        "name": "f",
        "shareWith": null,
        "role": null
    }"#;
    let node: FileNode = serde_json::from_str(json).unwrap();

    let out = serde_json::to_string(&node).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let obj = v.as_object().unwrap();

    for key in ["parentId", "shareWith", "role"] {
        assert!(obj.contains_key(key), "{key} must be present");
        assert!(obj[key].is_null(), "{key} must be null");
    }
    assert_eq!(obj["@type"], "Directory");
    // §3.1.1: isSubscribed is "Boolean" (default: true).
    assert_eq!(obj["isSubscribed"], true, "got: {out}");

    for key in [
        "blobId",
        "size",
        "mediaType",
        "executable",
        "target",
        "created",
        "modified",
        "accessed",
        "changed",
        "myRights",
    ] {
        assert!(!obj.contains_key(key), "{key} must be absent, got: {out}");
    }
}

#[test]
fn filenode_share_with_populated_map() {
    let json = r#"{
        "@type": "File",
        "id": "fn3",
        "parentId": "dir1",
        "blobId": "blob2",
        "size": 100,
        "name": "shared.pdf",
        "mediaType": "application/pdf",
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
        "@type": "File",
        "id": "fn-rt",
        "parentId": "dir-rt",
        "blobId": "b-rt",
        "size": 2048,
        "name": "roundtrip.bin",
        "mediaType": "application/octet-stream",
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
        }
    }"#;

    let original: FileNode = serde_json::from_str(json).unwrap();
    let serialized = serde_json::to_string(&original).unwrap();
    let back: FileNode = serde_json::from_str(&serialized).unwrap();
    assert_eq!(original, back);
}

// ---------------------------------------------------------------------------
// FileNode — role field nullable semantics (§3.1.3 type: String|null)
// ---------------------------------------------------------------------------

#[test]
fn filenode_role_serializes_as_null() {
    // Oracle: §3.1.3 types a Directory's `role` as `String|null`.  A Directory
    // with role None must produce "role":null in wire JSON, not absence of the key.
    let json = r#"{
        "@type": "Directory",
        "id": "rn1",
        "parentId": null,
        "name": "docs",
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
    assert!(
        obj["role"].is_null(),
        "role must serialize as null, not absent"
    );
}

#[test]
fn filenode_role_null_round_trips() {
    // Oracle: §3.1.3 — role: null → deserialize → None → serialize → "role":null.
    let json = r#"{
        "@type": "Directory",
        "id": "rn2",
        "parentId": null,
        "name": "dir",
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
    assert!(
        out2["role"].is_null(),
        "role must remain null after round-trip"
    );
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
    // Taken verbatim from draft-ietf-jmap-filenode-16 §2.1.1 capability example.
    let json = r#"{
        "maxFileNodeDepth": 50,
        "maxSizeFileNodeName": 255,
        "fileNodeQuerySortOptions": [
            "name", "mediaType", "size", "created", "modified",
            "nodeType", "tree"
        ],
        "supportedNodeTypes": ["Directory", "File", "SymLink"],
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
        vec![
            "name",
            "mediaType",
            "size",
            "created",
            "modified",
            "nodeType",
            "tree"
        ]
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
fn capability_supported_node_types_from_spec_example() {
    // supportedNodeTypes value from the draft-ietf-jmap-filenode-16 §2.1.1 example.
    let json = r#"{
        "maxFileNodeDepth": 50,
        "maxSizeFileNodeName": 255,
        "fileNodeQuerySortOptions": ["name"],
        "supportedNodeTypes": ["Directory", "File", "SymLink"],
        "forbiddenNameChars": null,
        "forbiddenNodeNames": null,
        "caseInsensitiveNames": false,
        "mayCreateTopLevelFileNode": false,
        "webTrashUrl": null,
        "webUrlTemplate": null,
        "webWriteUrlTemplate": null
    }"#;
    let cap: FileNodeCapability = serde_json::from_str(json).unwrap();
    assert_eq!(
        cap.supported_node_types,
        vec![NodeType::Directory, NodeType::File, NodeType::Symlink]
    );
    let v = serde_json::to_value(&cap).unwrap();
    assert_eq!(
        v["supportedNodeTypes"],
        serde_json::json!(["Directory", "File", "SymLink"])
    );
}

#[test]
fn capability_supported_node_types_keeps_unrecognised_values() {
    // §2.1: "Clients MUST ignore values they do not recognise." An unknown
    // registry value must not make the capability fail to parse.
    let json = r#"{
        "maxFileNodeDepth": null,
        "maxSizeFileNodeName": 255,
        "fileNodeQuerySortOptions": ["name"],
        "supportedNodeTypes": ["Directory", "File", "VendorType"],
        "forbiddenNameChars": null,
        "forbiddenNodeNames": null,
        "caseInsensitiveNames": false,
        "mayCreateTopLevelFileNode": false,
        "webTrashUrl": null,
        "webUrlTemplate": null,
        "webWriteUrlTemplate": null
    }"#;
    let cap: FileNodeCapability = serde_json::from_str(json).unwrap();
    assert_eq!(
        cap.supported_node_types[2],
        NodeType::Other("VendorType".to_owned())
    );
}

#[test]
fn capability_supported_node_types_is_required() {
    // §2.1: the object "MUST contain" supportedNodeTypes.
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
    assert!(serde_json::from_str::<FileNodeCapability>(json).is_err());
}

#[test]
fn capability_null_depth_and_nullables() {
    // maxFileNodeDepth: null means no limit.  Other nullable fields are also null.
    let json = r#"{
        "maxFileNodeDepth": null,
        "maxSizeFileNodeName": 100,
        "fileNodeQuerySortOptions": ["name"],
        "supportedNodeTypes": ["Directory", "File"],
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
        "supportedNodeTypes": ["Directory", "File"],
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
        "supportedNodeTypes": ["Directory", "File", "SymLink"],
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
fn filter_condition_media_type_wire_names() {
    // §3.2.5: the media type conditions are `mediaType` and `mediaTypeMatch`.
    let json = r#"{"mediaType": "text/plain", "mediaTypeMatch": "text/*"}"#;
    let cond: FileNodeFilterCondition = serde_json::from_str(json).unwrap();
    assert_eq!(cond.media_type, Some("text/plain".to_owned()));
    assert_eq!(cond.media_type_match, Some("text/*".to_owned()));

    let out: serde_json::Value = serde_json::to_value(&cond).unwrap();
    assert_eq!(
        out,
        serde_json::json!({"mediaType": "text/plain", "mediaTypeMatch": "text/*"})
    );
    let out = out.to_string();
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
        "nodeType": "File",
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
        "mediaType": "text/plain",
        "mediaTypeMatch": "text/*",
        "body": "hello world",
        "text": "search term"
    }"#;

    let cond: FileNodeFilterCondition = serde_json::from_str(json).unwrap();
    assert_eq!(cond.is_top_level, Some(true));
    assert_eq!(cond.parent_id, Some(id("dir1")));
    assert_eq!(cond.ancestor_id, Some(id("root1")));
    assert_eq!(cond.descendant_id, Some(id("child1")));
    assert_eq!(cond.node_type, Some("File".to_owned()));
    assert_eq!(cond.role, Some("home".to_owned()));
    assert_eq!(cond.has_any_role, Some(false));
    assert_eq!(cond.blob_id, Some(id("blob1")));
    assert_eq!(cond.is_executable, Some(true));
    assert_eq!(
        cond.created_before,
        Some(UTCDate::from("2025-01-01T00:00:00Z"))
    );
    assert_eq!(
        cond.created_after,
        Some(UTCDate::from("2020-01-01T00:00:00Z"))
    );
    assert_eq!(
        cond.modified_before,
        Some(UTCDate::from("2025-06-01T00:00:00Z"))
    );
    assert_eq!(
        cond.modified_after,
        Some(UTCDate::from("2021-01-01T00:00:00Z"))
    );
    assert_eq!(
        cond.accessed_before,
        Some(UTCDate::from("2025-12-01T00:00:00Z"))
    );
    assert_eq!(
        cond.accessed_after,
        Some(UTCDate::from("2022-01-01T00:00:00Z"))
    );
    assert_eq!(cond.min_size, Some(1024));
    assert_eq!(cond.max_size, Some(1048576));
    assert_eq!(cond.name, Some("readme.txt".to_owned()));
    assert_eq!(cond.name_match, Some("*.txt".to_owned()));
    assert_eq!(cond.media_type, Some("text/plain".to_owned()));
    assert_eq!(cond.media_type_match, Some("text/*".to_owned()));
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
        "nodeType": "File",
        "minSize": 512,
        "mediaType": "image/png",
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

// ── Extras-preservation policy tests (JMAP-lbdy.7) ──────────────────────────

/// `FilesRights.extra` captures vendor fields and preserves them across
/// deserialize/serialize round-trip.
#[test]
fn files_rights_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "mayRead": true,
        "mayAddChildren": false,
        "mayRename": false,
        "mayDelete": false,
        "mayModifyContent": false,
        "mayShare": false,
        "acmeCorpMayPin": true
    });
    let r: FilesRights = serde_json::from_value(raw).unwrap();
    assert_eq!(
        r.extra.get("acmeCorpMayPin").and_then(|v| v.as_bool()),
        Some(true)
    );
    let back = serde_json::to_value(&r).unwrap();
    assert_eq!(back["acmeCorpMayPin"], true);
}

/// `FileNode.extra` captures vendor fields and preserves them across
/// deserialize/serialize round-trip.
#[test]
fn file_node_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "@type": "Directory",
        "id": "n1",
        "parentId": null,
        "name": "Documents",
        "shareWith": null,
        "role": null,
        "acmeCorpStorageClass": "warm"
    });
    let n: FileNode = serde_json::from_value(raw).unwrap();
    assert_eq!(
        n.extra.get("acmeCorpStorageClass").and_then(|v| v.as_str()),
        Some("warm")
    );
    let back = serde_json::to_value(&n).unwrap();
    assert_eq!(back["acmeCorpStorageClass"], "warm");
}

// ---------------------------------------------------------------------------
// draft-ietf-jmap-filenode-16 §3.1 / §3.1.5 — @type and type-specific properties
// ---------------------------------------------------------------------------

/// The §3.1.5 example array, copied verbatim from draft-ietf-jmap-filenode-16.
const SPEC_3_1_5_EXAMPLE: &str = r#"[
  {
    "@type": "Directory",
    "id": "n1",
    "parentId": null,
    "name": "Documents",
    "role": "documents"
  },
  {
    "@type": "File",
    "id": "n3",
    "parentId": "n1",
    "name": "report.pdf",
    "blobId": "Bfa61c0",
    "size": 48576,
    "mediaType": "application/pdf",
    "executable": false
  },
  {
    "@type": "SymLink",
    "id": "n7",
    "parentId": null,
    "name": "latest-report",
    "target": ["Documents", "report.pdf"]
  }
]"#;

#[test]
fn spec_3_1_5_example_deserializes() {
    let nodes: Vec<FileNode> = serde_json::from_str(SPEC_3_1_5_EXAMPLE).unwrap();
    assert_eq!(nodes.len(), 3);

    assert_eq!(nodes[0].node_type, Some(NodeType::Directory));
    assert_eq!(nodes[0].role, Some(NodeRole::Documents));

    assert_eq!(nodes[1].node_type, Some(NodeType::File));
    assert_eq!(nodes[1].parent_id, Some(id("n1")));
    assert_eq!(nodes[1].blob_id, Some(id("Bfa61c0")));
    assert_eq!(nodes[1].size, Some(48576));
    assert_eq!(nodes[1].media_type.as_deref(), Some("application/pdf"));
    assert_eq!(nodes[1].executable, Some(false));

    assert_eq!(nodes[2].node_type, Some(NodeType::Symlink));
    assert_eq!(
        nodes[2].target,
        Some(vec!["Documents".to_owned(), "report.pdf".to_owned()])
    );
    for node in &nodes {
        assert!(node.extra.is_empty(), "no property may fall into extras");
    }
}

#[test]
fn spec_3_1_5_example_round_trips() {
    // The example omits common properties "for brevity" (§3.1.5); the ones
    // this crate always emits are the required-and-nullable `shareWith` and
    // `isSubscribed`, "Boolean" (default: true) per §3.1.1.
    let expected: Vec<serde_json::Value> = serde_json::from_str(SPEC_3_1_5_EXAMPLE).unwrap();
    let nodes: Vec<FileNode> = serde_json::from_str(SPEC_3_1_5_EXAMPLE).unwrap();
    for (node, want) in nodes.iter().zip(expected) {
        let mut want = want;
        want.as_object_mut()
            .unwrap()
            .insert("shareWith".to_owned(), serde_json::Value::Null);
        want.as_object_mut()
            .unwrap()
            .entry("isSubscribed")
            .or_insert(serde_json::Value::Bool(true));
        assert_eq!(serde_json::to_value(node).unwrap(), want);
    }
}

#[test]
fn filenode_type_foreign_properties_are_omitted() {
    // §3.1: properties defined for one type MUST NOT appear on a FileNode of
    // another type; they are omitted rather than set to null.  Each fixture
    // carries every other type's properties; serialization must drop them.
    let cases = [
        (
            serde_json::json!({
                "@type": "File", "id": "f", "parentId": null, "name": "a",
                "blobId": "b1", "size": 3, "mediaType": "text/plain", "executable": true,
                "role": "documents", "target": ["x"]
            }),
            &["blobId", "size", "mediaType", "executable"][..],
            &["role", "target"][..],
        ),
        (
            serde_json::json!({
                "@type": "Directory", "id": "d", "parentId": null, "name": "a",
                "blobId": "b1", "size": 3, "mediaType": "text/plain", "executable": true,
                "role": "documents", "target": ["x"]
            }),
            &["role"][..],
            &["blobId", "size", "mediaType", "executable", "target"][..],
        ),
        (
            serde_json::json!({
                "@type": "SymLink", "id": "s", "parentId": null, "name": "a",
                "blobId": "b1", "size": 3, "mediaType": "text/plain", "executable": true,
                "role": "documents", "target": ["x"]
            }),
            &["target"][..],
            &["blobId", "size", "mediaType", "executable", "role"][..],
        ),
    ];
    for (input, present, absent) in cases {
        let node: FileNode = serde_json::from_value(input.clone()).unwrap();
        let out = serde_json::to_value(&node).unwrap();
        for key in present {
            assert_eq!(out[key], input[key], "{key} must be kept: {out}");
        }
        for key in absent {
            assert!(out.get(key).is_none(), "{key} must be omitted: {out}");
        }
    }
}

#[test]
fn filenode_null_type_foreign_properties_are_accepted() {
    // §3.1: a null value for a property not defined for the node's @type is
    // ignored, so a client may send the same property set for any type.
    let node: FileNode = serde_json::from_value(serde_json::json!({
        "@type": "File", "id": "f", "parentId": null, "name": "a",
        "blobId": "b1", "role": null, "target": null
    }))
    .unwrap();
    assert_eq!(node.role, None);
    assert_eq!(node.target, None);
    let out = serde_json::to_value(&node).unwrap();
    assert!(
        out.get("role").is_none() && out.get("target").is_none(),
        "{out}"
    );
}

#[test]
fn filenode_old_wire_names_are_not_recognised() {
    // §3.1.1, §3.1.2: the -16 names are `@type` and `mediaType`.  The -14
    // `nodeType` / `type` keys are not FileNode properties any more and are
    // preserved only as unknown extras.
    let node: FileNode = serde_json::from_value(serde_json::json!({
        "id": "f", "parentId": null, "name": "a",
        "nodeType": "file", "type": "text/plain"
    }))
    .unwrap();
    assert_eq!(node.node_type, None);
    assert_eq!(node.media_type, None);
    assert_eq!(node.extra.len(), 2);
}

/// Oracle: draft-ietf-jmap-filenode-16 §3.1.2 `mediaType: "String" (default:
/// "application/octet-stream")` and `executable: "Boolean" (default: false)`;
/// "If not provided on creation, the server sets it to
/// "application/octet-stream"."
#[test]
fn filenode_file_unset_defaulted_properties_serialize_defaults() {
    let node: FileNode = serde_json::from_value(serde_json::json!({
        "@type": "File", "id": "f", "parentId": null, "name": "a", "blobId": "b1"
    }))
    .unwrap();
    let out = serde_json::to_value(&node).unwrap();
    assert_eq!(
        out["mediaType"],
        serde_json::json!("application/octet-stream"),
        "{out}"
    );
    assert_eq!(out["executable"], serde_json::json!(false), "{out}");
}

/// Oracle: workspace map-ordering policy (bd:JMAP-pdux) — wire-format maps
/// serialize in sorted key order. Input keys are deliberately out of order;
/// expected bytes are hand-written.
#[test]
fn share_with_serializes_in_sorted_key_order() {
    let rights = r#"{"mayRead":true,"mayAddChildren":false,"mayRename":false,"mayDelete":false,"mayModifyContent":false,"mayShare":false}"#;
    let input = format!(
        r#"{{"id":"n1","parentId":null,"name":"doc","shareWith":{{"u2":{rights},"u1":{rights}}}}}"#
    );
    let node: FileNode = serde_json::from_str(&input).expect("deserialize");
    let j = serde_json::to_string(&node).expect("serialize");
    assert!(
        j.contains(&format!(r#""shareWith":{{"u1":{rights},"u2":{rights}}}"#)),
        "shareWith keys must be sorted: {j}"
    );
}
