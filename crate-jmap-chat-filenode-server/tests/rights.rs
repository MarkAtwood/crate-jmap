//! `space_files_rights` and the `check_space_*` gates: the Space role to
//! `FilesRights` mapping.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! ebdf95e ("filenode Lnnn"):
//!
//! - L153: "FileNode/changes responses SHOULD be scoped to file trees the
//!   requesting user has access to, so that a user who is not a member of
//!   a Space receives no change notifications for that Space's file tree."
//! - L159: "Users who are not members of the Space ... SHOULD NOT be
//!   granted access to the Space file tree".
//! - L161: "the `FilesRights` flags ... the server reports in the
//!   `myRights` of every Directory in the Space file tree, including the
//!   tree root, and enforces. ... a File or SymLink takes its rights from
//!   its parent Directory. Each row includes the grants of the rows above
//!   it. A member holding multiple roles receives the union of all
//!   applicable grants. A flag no applicable row grants is `false`."
//! - L165-167, the table: `"view"` `mayRead`; `"send"` "The above, plus
//!   `mayAddChildren`"; `"manage_channels"` or `"manage_space"` "The
//!   above, plus `mayDelete`, `mayRename`, and `mayModifyContent`".
//! - L169 (jmap-chat-spec 662085d): "A Space root reports these rights
//!   like any other Directory ... They do not decide who may destroy,
//!   rename, or move the Space root itself".
//! - L171: "No role is granted `mayShare`. ... the server rejects a
//!   `FileNode/set` update that changes `shareWith` on a node in a Space
//!   file tree with a `forbidden` SetError."
//! - L173: "Replacing an existing file ... here only `"manage_channels"`
//!   and above hold. A member with only `"send"` uploads a new file
//!   instead".
//! - L177 (jmap-chat-spec 662085d): "Creating, renaming, or destroying a
//!   Space root requires `"manage_space"` permission, whatever the root's
//!   `myRights` report; the server SHOULD reject these from other members
//!   with `forbidden`. ... the server SHOULD reject a `FileNode/set` update
//!   that changes a Space root's `parentId` with `forbidden`."
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
    RootCheckError, check_space_create, check_space_destroy, check_space_update, space_files_rights,
};
use jmap_chat_server::memory::MemoryBackend as ChatMemory;
use jmap_filenode_server::memory::MemoryBackend as FilesMemory;
use jmap_filenode_types::FileNode;
use jmap_server::JmapBackend;
use jmap_types::{Id, PatchObject};
use serde_json::{Value, json};

const ACCOUNT: &str = "A1";
const SPACE: &str = "S1";

struct Fixture {
    chat: ChatMemory,
    files: FilesMemory,
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
        chat.set_everyone_permissions(HashSet::new());
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

        let files = FilesMemory::new().with_account(&Id::from(ACCOUNT));
        for node in [
            json!({ "id": "root", "@type": "Directory", "name": "S1", "parentId": null,
                    "spaceId": SPACE }),
            json!({ "id": "docs", "@type": "Directory", "name": "docs", "parentId": "root" }),
            json!({ "id": "f1", "@type": "File", "name": "f1.txt", "parentId": "docs",
                    "blobId": "b1", "size": 3, "mediaType": "text/plain" }),
            json!({ "id": "root-s2", "@type": "Directory", "name": "S2", "parentId": null,
                    "spaceId": "S2" }),
            json!({ "id": "other", "@type": "Directory", "name": "other", "parentId": "root-s2" }),
            json!({ "id": "loose", "@type": "Directory", "name": "loose", "parentId": null }),
        ] {
            files.seed_node(
                &Id::from(ACCOUNT),
                serde_json::from_value(node).expect("FileNode fixture"),
            );
        }
        Self { chat, files }
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
        space_files_rights(&self.chat, &(), &self.files, &(), &Id::from(ACCOUNT), &node)
            .await
            .expect("storage")
            .map(|r| serde_json::to_value(r).expect("FilesRights serializes"))
    }

    /// The SetError type of a create of `node`, or `None` if it may proceed.
    async fn create(&self, node: Value) -> Option<String> {
        let node: FileNode = serde_json::from_value(node).expect("FileNode fixture");
        let result =
            check_space_create(&self.chat, &(), &self.files, &(), &Id::from(ACCOUNT), &node)
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
        )
        .await
        .expect("storage");
        error_type(result)
    }

    /// The SetError type of a destroy of `id`, or `None`.
    async fn destroy(&self, id: &str) -> Option<String> {
        let node = self.node(id).await;
        let result =
            check_space_destroy(&self.chat, &(), &self.files, &(), &Id::from(ACCOUNT), &node)
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

/// The table row by row (L165-167). L161 and L169: the root reports the
/// rights "like any other Directory", and the file takes its parent
/// Directory's. `mayShare` is never granted (L171).
#[tokio::test]
async fn table_rows_on_root_directory_and_file() {
    let read = "mayRead";
    let add = "mayAddChildren";
    let manage = [
        "mayRead",
        "mayAddChildren",
        "mayDelete",
        "mayRename",
        "mayModifyContent",
    ];
    for (permission, granted) in [
        ("view", vec![read]),
        ("send", vec![read, add]),
        ("manage_channels", manage.to_vec()),
        ("manage_space", manage.to_vec()),
    ] {
        let f = Fixture::new(&[permission]);
        for id in ["root", "docs", "f1"] {
            assert_eq!(
                f.rights(id).await,
                Some(rights(&granted)),
                "{permission} {id}"
            );
        }
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
            "mayAddChildren",
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
    let f = Fixture::new(&[]);
    f.chat
        .set_everyone_permissions(HashSet::from([jmap_chat_types::ChatPermission::Send]));
    assert_eq!(
        f.rights("docs").await,
        Some(rights(&["mayRead", "mayAddChildren"]))
    );
}

/// The `@everyone` floor is per Space (chat L812 at jmap-chat-spec
/// 29e9a41: "the implicit `@everyone` role, which every member of a Space
/// holds"): a node takes the floor of the Space whose tree holds it. A1 is
/// a role-less member of both S1 and S2, whose floors differ.
#[tokio::test]
async fn each_trees_own_space_floor_applies() {
    let f = Fixture::new(&[]);
    f.chat.insert_object_for_test(
        "Space",
        ACCOUNT,
        "S2",
        json!({
            "id": "S2",
            "name": "Space 2",
            "roles": [],
            "members": [{ "id": ACCOUNT, "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }],
            "categories": [],
            "uncategorizedChannelIds": [],
            "createdAt": "2026-01-01T00:00:00Z",
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "memberCount": 1
        }),
    );
    f.chat.set_space_everyone_permissions(
        &Id::from(SPACE),
        HashSet::from([jmap_chat_types::ChatPermission::Send]),
    );
    f.chat.set_space_everyone_permissions(
        &Id::from("S2"),
        HashSet::from([jmap_chat_types::ChatPermission::View]),
    );
    // L165-166: `"send"` is `mayRead` "plus `mayAddChildren`"; `"view"` is
    // `mayRead`.
    assert_eq!(
        f.rights("docs").await,
        Some(rights(&["mayRead", "mayAddChildren"]))
    );
    assert_eq!(f.rights("other").await, Some(rights(&["mayRead"])));
}

/// A Chat backend that fails reading the Space is
/// `RootCheckError::Chat`, not a rights answer. The stored Space lacks
/// every required property, so the Chat MemoryBackend's read fails.
#[tokio::test]
async fn chat_backend_error_is_root_check_error_chat() {
    let f = Fixture::new(&["manage_space"]);
    f.chat
        .insert_object_for_test("Space", ACCOUNT, SPACE, json!({ "id": SPACE }));
    let node = f.node("docs").await;
    let result = space_files_rights(&f.chat, &(), &f.files, &(), &Id::from(ACCOUNT), &node).await;
    assert!(matches!(result, Err(RootCheckError::Chat(_))), "{result:?}");
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

/// L161: only a Directory has `myRights`; a File takes its parent's.
/// draft-ietf-jmap-filenode-20 §3.2.3 makes "a node the child of a node
/// whose @type does not permit children" an error, so no role adds a
/// child to a File.
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

/// L171: "No role is granted `mayShare`. ... the server rejects a
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

/// L171 rejects an update that "changes `shareWith`": restating the
/// stored `null` changes nothing.
#[tokio::test]
async fn share_with_unchanged_passes() {
    let f = Fixture::new(&["view"]);
    assert_eq!(f.update("f1", json!({ "shareWith": null })).await, None);
}

/// L167: renaming a non-root node needs `"manage_channels"`; `"send"`
/// cannot rename, even a file they could have uploaded (L173).
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

/// L177: renaming a Space root needs `"manage_space"`, which
/// `"manage_channels"` lacks although the root reports `mayRename` for it.
#[tokio::test]
async fn only_manage_space_renames_the_root() {
    let rename = json!({ "name": "files" });
    assert_eq!(
        Fixture::new(&["manage_channels"])
            .update("root", rename.clone())
            .await
            .as_deref(),
        Some("forbidden")
    );
    assert_eq!(
        Fixture::new(&["manage_space"]).update("root", rename).await,
        None
    );
}

/// L177: a changed `parentId` on a Space root is refused for every member.
#[tokio::test]
async fn nobody_moves_the_root() {
    for permission in ["manage_channels", "manage_space"] {
        assert_eq!(
            Fixture::new(&[permission])
                .update("root", json!({ "parentId": "loose" }))
                .await
                .as_deref(),
            Some("forbidden"),
            "{permission}"
        );
    }
}

/// L177 governs only a rename or move of the root. A patch restating the
/// root's stored `name` ("S1" in the fixture) and its `parentId` null
/// renames and moves nothing, so a member holding `mayRename` (L167) may
/// send it.
#[tokio::test]
async fn restating_root_name_and_parent_passes() {
    for permission in ["manage_channels", "manage_space"] {
        let f = Fixture::new(&[permission]);
        for patch in [
            json!({ "name": "S1" }),
            json!({ "parentId": null }),
            json!({ "name": "S1", "parentId": null }),
        ] {
            assert_eq!(f.update("root", patch).await, None, "{permission}");
        }
    }
}

/// L167: `mayModifyContent` is granted on Directories too, the root
/// included (L169), so `"manage_channels"` may set the root's `modified`.
#[tokio::test]
async fn root_content_change_needs_manage_channels() {
    let patch = json!({ "modified": "2026-02-01T00:00:00Z" });
    assert_eq!(
        Fixture::new(&["send"])
            .update("root", patch.clone())
            .await
            .as_deref(),
        Some("forbidden")
    );
    assert_eq!(
        Fixture::new(&["manage_channels"])
            .update("root", patch)
            .await,
        None
    );
}

/// L167 and L173: replacing a file's content (`blobId`) needs
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

/// L167: `"manage_channels"` may destroy any node below the root;
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

/// L177: destroying a Space root needs `"manage_space"`, "whatever the
/// root's `myRights` report": `"manage_channels"` holds `mayDelete` and
/// is still refused.
#[tokio::test]
async fn only_manage_space_destroys_the_root() {
    assert_eq!(
        Fixture::new(&["manage_channels"])
            .destroy("root")
            .await
            .as_deref(),
        Some("forbidden")
    );
    assert_eq!(Fixture::new(&["manage_space"]).destroy("root").await, None);
}
