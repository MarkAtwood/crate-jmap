//! Property selector enum and [`jmap_types`] trait impls for [`crate::FileNode`].
//!
//! Defined here so `jmap-filenode-server` can use them without violating the
//! orphan rule (`JmapObject` is foreign; `FileNode` is local to this crate).

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enum
// ---------------------------------------------------------------------------

/// Property selector for [`crate::FileNode`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FileNodeProperty {
    /// The `id` property (draft-ietf-jmap-filenode-16 §3.1).
    Id,
    /// The `parentId` property (draft-ietf-jmap-filenode-16 §3.1).
    ParentId,
    /// The `@type` property (draft-ietf-jmap-filenode-16 §3.1).
    NodeType,
    /// The `blobId` property (draft-ietf-jmap-filenode-16 §3.1).
    BlobId,
    /// The `target` property (draft-ietf-jmap-filenode-16 §3.1).
    Target,
    /// The `size` property (draft-ietf-jmap-filenode-16 §3.1).
    Size,
    /// The `name` property (draft-ietf-jmap-filenode-16 §3.1).
    Name,
    /// The `mediaType` property (draft-ietf-jmap-filenode-16 §3.1).
    MediaType,
    /// The `created` property (draft-ietf-jmap-filenode-16 §3.1).
    Created,
    /// The `modified` property (draft-ietf-jmap-filenode-16 §3.1).
    Modified,
    /// The `accessed` property (draft-ietf-jmap-filenode-16 §3.1).
    Accessed,
    /// The `changed` property (draft-ietf-jmap-filenode-16 §3.1).
    Changed,
    /// The `executable` property (draft-ietf-jmap-filenode-16 §3.1).
    Executable,
    /// The `isSubscribed` property (draft-ietf-jmap-filenode-16 §3.1).
    IsSubscribed,
    /// The `myRights` property (draft-ietf-jmap-filenode-16 §3.1).
    MyRights,
    /// The `shareWith` property (draft-ietf-jmap-filenode-16 §3.1).
    ShareWith,
    /// The `role` property (draft-ietf-jmap-filenode-16 §3.1).
    Role,
}

// ---------------------------------------------------------------------------
// JmapObject / marker trait impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::FileNode {
    const TYPE_NAME: &'static str = "FileNode";
    type Property = FileNodeProperty;
}

impl GetObject for crate::FileNode {}

/// draft-ietf-jmap-filenode-16 §2 (lines 349, 362, 414, 431, 483): `@type`
/// is "(immutable)", `id` is "(immutable; server-set)", `changed`,
/// `myRights` and `size` are "(server-set)". draft-atwood-jmap-cid-00
/// §FileNode Extension: for `sha256` "the server MUST set this to the
/// SHA-256 digest of the stored blob content". draft-atwood-jmap-chat-
/// filenode-00 §The `spaceId` Extension Property: `spaceId` is "Supplied
/// by the client at creation and treated as immutable thereafter".
/// draft-ietf-jmap-metadata-04 §3.2: clients write `metadata` and
/// `privateMetadata` through `Foo/set`.
impl SetObject for crate::FileNode {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "changed", "myRights", "size", "sha256"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "@type",
        "parentId",
        "name",
        "created",
        "modified",
        "accessed",
        "isSubscribed",
        "shareWith",
        "blobId",
        "mediaType",
        "executable",
        "role",
        "target",
        "spaceId",
        "metadata",
        "privateMetadata",
    ];
    const IMMUTABLE: &'static [&'static str] = &["@type", "spaceId"];
}

impl QueryObject for crate::FileNode {
    type Filter = crate::FileNodeFilter;
    type Comparator = crate::FileNodeComparator;
}

#[cfg(test)]
mod tests {
    use jmap_types::{Id, UTCDate, check_property_classification};
    use serde_json::{Map, Value, json};
    use std::collections::BTreeMap;

    fn de<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).expect("hand-written fixture deserializes")
    }

    /// The struct literal makes a new field a compile error here until it is
    /// set, then a classification failure until the `SetObject` lists name
    /// it.
    #[test]
    fn file_node_properties_are_classified() {
        let rights = || -> crate::FilesRights {
            de(json!({
                "mayRead": true,
                "mayAddChildren": true,
                "mayRename": true,
                "mayDelete": true,
                "mayModifyContent": true,
                "mayShare": true,
            }))
        };
        let date = || Some(UTCDate::from("2026-01-01T00:00:00Z"));
        let obj = crate::FileNode {
            id: Id::from("f1"),
            parent_id: Some(Id::from("f0")),
            node_type: Some(de(json!("File"))),
            blob_id: Some(Id::from("b1")),
            target: Some(vec!["f2".into()]),
            size: Some(1),
            name: "n".into(),
            sha256: Some(de(json!(
                "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
            ))),
            media_type: Some("text/plain".into()),
            created: date(),
            modified: date(),
            accessed: date(),
            changed: date(),
            executable: Some(false),
            is_subscribed: Some(true),
            my_rights: Some(rights()),
            share_with: Some(BTreeMap::from([(Id::from("p1"), rights())])),
            role: Some(de(json!("chatspace"))),
            space_id: Some(Id::from("s1")),
            metadata: Some(de(json!({"acme.example.com": {"memo": "m"}}))),
            private_metadata: Some(de(json!({"acme.example.com": {"memo": "p"}}))),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }
}
