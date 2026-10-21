//! `space_files_rights` and the `check_space_*` gates: the Space role to
//! `FilesRights` mapping.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! 6bcd9d8 ("filenode Lnnn"):
//!
//! - L153: "FileNode/changes responses SHOULD be scoped to file trees the
//!   requesting user has access to, so that a user who is not a member of
//!   a Space receives no change notifications for that Space's file tree."
//! - L159: "Users who are not members of the Space ... SHOULD NOT be
//!   granted access to the Space file tree".
//! - L161: "Each row includes the grants of the rows above it. A member
//!   holding multiple roles receives the union of all applicable grants.
//!   A flag no applicable row grants is `false`. ... `mayAddChildren` is
//!   always `false` on nodes whose `@type` does not permit children, and
//!   `mayModifyContent` is `false` on a Directory, including the tree
//!   root."
//! - L165-168, the table: `"view"` `mayRead`; `"send"` "The above, plus
//!   `mayAddChildren`"; `"manage_channels"` "The above, plus `mayDelete`,
//!   `mayRename`, and `mayModifyContent` on every node except the tree
//!   root"; `"manage_space"` "The above, plus `mayDelete` and `mayRename`
//!   on the tree root".
//! - L170: "No role is granted `mayShare`. ... the server rejects a
//!   `FileNode/set` update that changes `shareWith` on a node in a Space
//!   file tree with a `forbidden` SetError."
//! - L176: "Creating the Space file tree root ... requires
//!   `"manage_space"` permission, and so, by the table above, does
//!   destroying it."
//!
//! draft-ietf-jmap-filenode §FileNode properties defines `mayRename` as
//! "rename or move this node" and `mayModifyContent` as "update the
//! content-related properties of this node (`blobId`, `mediaType`,
//! `target`, `modified`, `accessed`, `executable`)"; draft-atwood-jmap-
//! chat-00 §Space Permission Resolution step 1 includes "the implicit
//! `@everyone` role" in the union.
//!
//! Records and expected rights are hand-written JSON from those lines.

use std::collections::HashSet;

use jmap_chat_filenode_server::{
    check_space_create, check_space_destroy, check_space_update, space_files_rights,
};
use jmap_chat_server::memory::MemoryBackend as ChatMemory;
use jmap_filenode_server::memory::MemoryBackend as FilesMemory;
use jmap_filenode_types::FileNode;
use jmap_server::JmapBackend;
use jmap_types::{Id, PatchObject};
use serde_json::{json, Value};

const ACCOUNT: &str = "A1";
const SPACE: &str = "S1";
const ROLE: &str = "chatspace";

struct Fixture {
    chat: ChatMemory,
    files: FilesMemory,
    everyone: HashSet<jmap_chat_types::ChatPermission>,
}

impl Fixture {
    /// Space S1 with role R1 holding `permissions`, held by member A1, and
    /// the FileNode trees:
    ///
    /// - `root` (Space S1 root) / `docs` / `f1` (file)
    /// - `root-s2` (root of Space S2, which A1 cannot read) / `other`
    /// - `loose` (top-level directory outside any Space tree)
    fn new(permissions: &[&str]) -> Self {
        Self::with_roles(
            json!([{ "id": "R1", "name": "Role", "permissions": permissions, "position": 1 }]),
            &["R1"],
        )
    }

    fn with_roles(roles: Value, member_role_ids: &[&str]) -> Self {
        let chat = ChatMemory::new();
        chat.register_account(&Id::from(ACCOUNT));
        let space = json!({
            "id": SPACE,
            "name": "Space",
            "roles": roles,
            "members": [{ "id": ACCOUNT, "roleIds": member_role_ids,
                          "joinedAt": "2026-01-01T00:00:00Z" }],
            "categories": [],
            "uncategorizedChannelIds": [],
            "createdAt": "2026-01-01T00:00:00Z",
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "memberCount": 1
        });
        chat.insert_object_for_test("Space", ACCOUNT, SPACE, space);

        let files = FilesMemory::new().with_account(ACCOUNT);
        for node in [
            json!({ "id": "root", "@type": "Directory", "name": "S1", "parentId": null,
                    "role": ROLE, "spaceId": SPACE }),
            json!({ "id": "docs", "@type": "Directory", "name": "docs", "parentId": "root" }),
            json!({ "id": "f1", "@type": "File", "name": "f1.txt", "parentId": "docs",
                    "blobId": "b1", "size": 3, "mediaType": "text/plain" }),
            json!({ "id": "root-s2", "@type": "Directory", "name": "S2", "parentId": null,
                    "role": ROLE, "spaceId": "S2" }),
            json!({ "id": "other", "@type": "Directory", "name": "other", "parentId": "root-s2" }),
            json!({ "id": "loose", "@type": "Directory", "name": "loose", "parentId": null }),
        ] {
            files.seed_node(
                ACCOUNT,
                serde_json::from_value(node).expect("FileNode fixture"),
            );
        }
        Self {
            chat,
            files,
            everyone: HashSet::new(),
        }
    }

    async fn node(&self, id: &str) -> FileNode {
        let (mut nodes, _) = self
            .files
            .get_objects::<FileNode>(&(), &Id::from(ACCOUNT), Some(&[Id::from(id)]), None)
            .await
            .expect("get");
        nodes.pop().expect("seeded node")
    }

    /// `space_files_rights` of stored node `id`, as wire JSON.
    async fn rights(&self, id: &str) -> Option<Value> {
        let node = self.node(id).await;
        space_files_rights(
            &self.chat,
            &(),
            &self.files,
            &(),
            &Id::from(ACCOUNT),
            &node,
            &self.everyone,
        )
        .await
        .expect("storage")
        .map(|r| serde_json::to_value(r).expect("FilesRights serializes"))
    }

    /// The SetError type of a create of `node`, or `None` if it may proceed.
    async fn create(&self, node: Value) -> Option<String> {
        let node: FileNode = serde_json::from_value(node).expect("FileNode fixture");
        let result = check_space_create(
            &self.chat,
            &(),
            &self.files,
            &(),
            &Id::from(ACCOUNT),
            &node,
            &self.everyone,
        )
        .await
        .expect("storage");
        error_type(result)
    }

    /// The SetError type of an update of `id` by `patch`, or `None`.
    async fn update(&self, id: &str, patch: Value) -> Option<String> {
        let node = self.node(id).await;
        let Value::Object(patch) = patch else {
            panic!("patch fixture must be an object");
        };
        let result = check_space_update(
            &self.chat,
            &(),
            &self.files,
            &(),
            &Id::from(ACCOUNT),
            &node,
            &PatchObject::from_map(patch),
            &self.everyone,
        )
        .await
        .expect("storage");
        error_type(result)
    }

    /// The SetError type of a destroy of `id`, or `None`.
    async fn destroy(&self, id: &str) -> Option<String> {
        let node = self.node(id).await;
        let result = check_space_destroy(
            &self.chat,
            &(),
            &self.files,
            &(),
            &Id::from(ACCOUNT),
            &node,
            &self.everyone,
        )
        .await
        .expect("storage");
        error_type(result)
    }
}

fn error_type(result: Result<(), jmap_server::SetError>) -> Option<String> {
    result.err().map(|e| {
        serde_json::to_value(e).expect("SetError serializes")["type"]
            .as_str()
            .expect("type")
            .to_owned()
    })
}

/// FilesRights with exactly the flags in `granted` true.
fn rights(granted: &[&str]) -> Value {
    let mut r = json!({
        "mayRead": false, "mayAddChildren": false, "mayRename": false,
        "mayDelete": false, "mayModifyContent": false, "mayShare": false,
    });
    for flag in granted {
        r[*flag] = json!(true);
    }
    r
}

/// The table row by row, on the tree root, a directory and a file, with
/// L161's `@type` rules applied. `mayShare` is never granted (L170).
#[tokio::test]
async fn table_rows_on_root_directory_and_file() {
    let read = "mayRead";
    let add = "mayAddChildren";
    let del = "mayDelete";
    let ren = "mayRename";
    let content = "mayModifyContent";
    for (permission, root, dir, file) in [
        ("view", vec![read], vec![read], vec![read]),
        ("send", vec![read, add], vec![read, add], vec![read]),
        (
            "manage_channels",
            vec![read, add],
            vec![read, add, del, ren],
            vec![read, del, ren, content],
        ),
        (
            "manage_space",
            vec![read, add, del, ren],
            vec![read, add, del, ren],
            vec![read, del, ren, content],
        ),
    ] {
        let f = Fixture::new(&[permission]);
        assert_eq!(
            f.rights("root").await,
            Some(rights(&root)),
            "{permission} root"
        );
        assert_eq!(
            f.rights("docs").await,
            Some(rights(&dir)),
            "{permission} docs"
        );
        assert_eq!(f.rights("f1").await, Some(rights(&file)), "{permission} f1");
    }
}

/// L161: "A member holding multiple roles receives the union of all
/// applicable grants."
#[tokio::test]
async fn union_over_held_roles() {
    let f = Fixture::with_roles(
        json!([
            { "id": "R1", "name": "Viewer", "permissions": ["view"], "position": 1 },
            { "id": "R2", "name": "Mod", "permissions": ["manage_channels"], "position": 2 },
        ]),
        &["R1", "R2"],
    );
    assert_eq!(
        f.rights("f1").await,
        Some(rights(&[
            "mayRead",
            "mayDelete",
            "mayRename",
            "mayModifyContent"
        ]))
    );
}

/// chat §Space Permission Resolution: the union includes the implicit
/// `@everyone` role.
#[tokio::test]
async fn everyone_permissions_count() {
    let mut f = Fixture::new(&[]);
    f.everyone.insert(jmap_chat_types::ChatPermission::Send);
    assert_eq!(
        f.rights("docs").await,
        Some(rights(&["mayRead", "mayAddChildren"]))
    );
}

/// L161: "A flag no applicable row grants is `false`": a member whose
/// roles hold none of the four permissions gets no flag.
#[tokio::test]
async fn member_without_table_permission_gets_nothing() {
    let f = Fixture::new(&["ban"]);
    assert_eq!(f.rights("f1").await, Some(rights(&[])));
}

/// L153 and L159: a non-member is granted nothing on the tree (S2 has no
/// Space record A1 is a member of), so the read path hides the tree and
/// its changes from them.
#[tokio::test]
async fn non_member_gets_no_flag() {
    let f = Fixture::new(&["manage_space"]);
    assert_eq!(f.rights("root-s2").await, Some(rights(&[])));
    assert_eq!(f.rights("other").await, Some(rights(&[])));
}

/// The table applies "on each FileNode in the Space file tree" only.
#[tokio::test]
async fn node_outside_a_space_tree_is_unmapped() {
    let f = Fixture::new(&["manage_space"]);
    assert_eq!(f.rights("loose").await, None);
}

/// L166: `"send"` adds `mayAddChildren`, "upload new files and
/// directories"; `"view"` alone does not.
#[tokio::test]
async fn create_needs_may_add_children_on_the_parent() {
    let node = json!({ "id": "new", "@type": "Directory", "name": "n", "parentId": "docs" });
    assert_eq!(Fixture::new(&["send"]).create(node.clone()).await, None);
    assert_eq!(
        Fixture::new(&["view"]).create(node).await.as_deref(),
        Some("forbidden")
    );
}

/// L161: `mayAddChildren` is `false` on a node whose `@type` does not
/// permit children, whatever the role.
#[tokio::test]
async fn create_under_a_file_is_forbidden() {
    let f = Fixture::new(&["manage_space"]);
    let node = json!({ "id": "new", "@type": "Directory", "name": "n", "parentId": "f1" });
    assert_eq!(f.create(node).await.as_deref(), Some("forbidden"));
}

/// L159: a non-member may not add to another Space's tree.
#[tokio::test]
async fn non_member_create_is_forbidden() {
    let f = Fixture::new(&["manage_space"]);
    let node = json!({ "id": "new", "@type": "Directory", "name": "n", "parentId": "root-s2" });
    assert_eq!(f.create(node).await.as_deref(), Some("forbidden"));
}

/// Outside a Space tree the table does not apply.
#[tokio::test]
async fn create_outside_a_space_tree_is_unchecked() {
    let f = Fixture::new(&[]);
    let node = json!({ "id": "new", "@type": "Directory", "name": "n", "parentId": "loose" });
    assert_eq!(f.create(node).await, None);
}

/// L170: "No role is granted `mayShare`. ... the server rejects a
/// `FileNode/set` update that changes `shareWith` on a node in a Space
/// file tree with a `forbidden` SetError", even for `"manage_space"`.
#[tokio::test]
async fn share_with_change_is_forbidden_for_every_role() {
    let f = Fixture::new(&["manage_space"]);
    for id in ["root", "docs", "f1"] {
        assert_eq!(
            f.update(id, json!({ "shareWith": { "U2": rights(&["mayRead"]) } }))
                .await
                .as_deref(),
            Some("forbidden"),
            "{id}"
        );
        assert_eq!(
            f.update(id, json!({ "shareWith/U2": rights(&["mayRead"]) }))
                .await
                .as_deref(),
            Some("forbidden"),
            "{id}"
        );
    }
}

/// L170 rejects an update that "changes `shareWith`": restating the
/// stored `null` changes nothing.
#[tokio::test]
async fn share_with_unchanged_passes() {
    let f = Fixture::new(&["view"]);
    assert_eq!(f.update("f1", json!({ "shareWith": null })).await, None);
}

/// L167: renaming a non-root node needs `"manage_channels"`; `"send"`
/// cannot rename, even a file they could have uploaded (L172).
#[tokio::test]
async fn rename_needs_manage_channels() {
    let patch = json!({ "name": "renamed.txt" });
    assert_eq!(
        Fixture::new(&["send"])
            .update("f1", patch.clone())
            .await
            .as_deref(),
        Some("forbidden")
    );
    assert_eq!(
        Fixture::new(&["manage_channels"]).update("f1", patch).await,
        None
    );
}

/// L167-168: `mayRename` on the tree root needs `"manage_space"`.
#[tokio::test]
async fn renaming_the_root_needs_manage_space() {
    let patch = json!({ "name": "files" });
    assert_eq!(
        Fixture::new(&["manage_channels"])
            .update("root", patch.clone())
            .await
            .as_deref(),
        Some("forbidden")
    );
    assert_eq!(
        Fixture::new(&["manage_space"]).update("root", patch).await,
        None
    );
}

/// L167 and L172: replacing a file's content (`blobId`) needs
/// `"manage_channels"`: "A member with only `"send"` uploads a new file
/// instead".
#[tokio::test]
async fn content_change_needs_manage_channels() {
    let patch = json!({ "blobId": "b2" });
    assert_eq!(
        Fixture::new(&["send"])
            .update("f1", patch.clone())
            .await
            .as_deref(),
        Some("forbidden")
    );
    assert_eq!(
        Fixture::new(&["manage_channels"]).update("f1", patch).await,
        None
    );
}

/// L159: a non-member changes nothing in another Space's tree.
#[tokio::test]
async fn non_member_update_is_forbidden() {
    let f = Fixture::new(&["manage_space"]);
    assert_eq!(
        f.update("other", json!({ "isSubscribed": false }))
            .await
            .as_deref(),
        Some("forbidden")
    );
}

/// L167: "rename, move": a move within the tree is a `mayRename` change
/// plus a new child of a directory that has `mayAddChildren`.
#[tokio::test]
async fn move_within_the_tree_needs_manage_channels() {
    let patch = json!({ "parentId": "root" });
    assert_eq!(
        Fixture::new(&["send"])
            .update("f1", patch.clone())
            .await
            .as_deref(),
        Some("forbidden")
    );
    assert_eq!(
        Fixture::new(&["manage_channels"]).update("f1", patch).await,
        None
    );
}

/// L159: moving a node into a Space tree the caller is not a member of
/// adds a child where they hold no `mayAddChildren`.
#[tokio::test]
async fn move_into_a_non_member_tree_is_forbidden() {
    let f = Fixture::new(&["manage_space"]);
    assert_eq!(
        f.update("f1", json!({ "parentId": "other" }))
            .await
            .as_deref(),
        Some("forbidden")
    );
}

/// L167: `"manage_channels"` may destroy any node except the root;
/// `"send"` may not.
#[tokio::test]
async fn destroy_needs_manage_channels() {
    assert_eq!(
        Fixture::new(&["send"]).destroy("f1").await.as_deref(),
        Some("forbidden")
    );
    assert_eq!(Fixture::new(&["manage_channels"]).destroy("f1").await, None);
    assert_eq!(
        Fixture::new(&["manage_channels"]).destroy("docs").await,
        None
    );
}

/// L176: destroying the root "requires `"manage_space"` permission".
#[tokio::test]
async fn destroying_the_root_needs_manage_space() {
    assert_eq!(
        Fixture::new(&["manage_channels"])
            .destroy("root")
            .await
            .as_deref(),
        Some("forbidden")
    );
    assert_eq!(Fixture::new(&["manage_space"]).destroy("root").await, None);
}
