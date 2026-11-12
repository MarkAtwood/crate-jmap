//! `check_root_create`: the Space root create checks.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! 662085d:
//!
//! - §Creating a Space Root: "When a client creates a FileNode with
//!   `spaceId`, its `@type` ... MUST be `"Directory"`. The server MUST
//!   validate:"
//!   - "The Space identified by `spaceId` exists and the requesting user
//!     is a member. If not, the server SHOULD return `forbidden` in both
//!     cases — Space not found and user not a member — to avoid revealing
//!     Space existence to non-members."
//!   - "Creating another root would not exceed any limit the server places
//!     on the number of Space roots per Space." This crate places none.
//!   - "The requesting user holds sufficient permission to create the root
//!     ... If not, the server SHOULD return `forbidden`."
//! - §Space File Tree: "A Space has zero, one, or more Space roots".
//! - §Permission Mapping: "Creating, renaming, or destroying a Space root
//!   requires `"manage_space"` permission, whatever the root's `myRights`
//!   report ... This is a higher bar than the `mayAddChildren` grant to
//!   `"send"`".
//!
//! and draft-atwood-jmap-chat-00.md L1705 ("chat L1705"): "Compute the
//! union of `permissions` across all SpaceRoles held by the member,
//! including the implicit `@everyone` role."
//!
//! Records are hand-written JSON from the drafts' field lists.

use std::collections::HashSet;

use jmap_chat_filenode_server::check_root_create;
use jmap_chat_server::memory::MemoryBackend as ChatMemory;
use jmap_filenode_server::handle_filenode_set;
use jmap_filenode_server::memory::MemoryBackend as FilesMemory;
use jmap_filenode_types::FileNode;
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "A1";
const SPACE: &str = "S1";

struct Fixture {
    chat: ChatMemory,
    files: FilesMemory,
}

impl Fixture {
    fn new() -> Self {
        let chat = ChatMemory::new();
        chat.set_everyone_permissions(HashSet::new());
        chat.register_account(&Id::from(ACCOUNT));
        Self {
            chat,
            files: FilesMemory::new().with_account(&Id::from(ACCOUNT)),
        }
    }

    /// Store Space `id` with one role `R1` holding `permissions` and the
    /// given `(member id, role ids)` (chat §Space, §SpaceRole,
    /// §SpaceMember field lists).
    fn seed_space(&self, id: &str, permissions: &[&str], members: &[(&str, &[&str])]) {
        let members: Vec<Value> = members
            .iter()
            .map(|(m, roles)| json!({ "id": m, "roleIds": roles, "joinedAt": "2026-01-01T00:00:00Z" }))
            .collect();
        let space = json!({
            "id": id,
            "name": "Space",
            "roles": [{ "id": "R1", "name": "Role", "permissions": permissions, "position": 1 }],
            "members": members,
            "categories": [],
            "uncategorizedChannelIds": [],
            "createdAt": "2026-01-01T00:00:00Z",
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "memberCount": members.len()
        });
        self.chat
            .insert_object_for_test("Space", ACCOUNT, id, space);
    }

    /// Create a Space root for `space_id` through `FileNode/set` and
    /// return its id.
    async fn create_root(&self, space_id: &str) -> String {
        let out = handle_filenode_set(
            &self.files,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT,
                "create": { "r": { "@type": "Directory", "name": space_id, "parentId": null,
                                   "spaceId": space_id } }
            }),
            &[],
        )
        .await
        .expect("FileNode/set");
        out.primary["created"]["r"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("root must be created: {}", out.primary))
            .to_owned()
    }

    /// Run the check on a create of `node` and return the SetError as
    /// wire JSON, or `None` if the create may proceed.
    async fn check(&self, node: Value) -> Option<Value> {
        let node: FileNode = serde_json::from_value(node).expect("FileNode fixture");
        check_root_create(&self.chat, &(), &self.files, &(), &Id::from(ACCOUNT), &node)
            .await
            .expect("storage")
            .err()
            .map(|e| serde_json::to_value(e).expect("SetError serializes"))
    }
}

fn root(space_id: &str) -> Value {
    json!({ "id": "new", "@type": "Directory", "name": "files", "parentId": null,
            "spaceId": space_id })
}

/// §Creating a Space Root scopes the checks to creates "with `spaceId`":
/// other nodes pass without a Space, whatever their `role`.
#[tokio::test]
async fn node_without_space_id_is_not_checked() {
    let f = Fixture::new();
    let node = json!({ "id": "new", "@type": "Directory", "name": "docs", "parentId": null,
                       "role": "chatspace" });
    assert_eq!(f.check(node).await, None);
}

/// "the server SHOULD return `forbidden` in both cases — Space not found
/// and user not a member".
#[tokio::test]
async fn unknown_space_is_forbidden() {
    let f = Fixture::new();
    let err = f.check(root("no-such-space")).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
}

/// "the requesting user is a member. If not, the server SHOULD return
/// `forbidden`".
#[tokio::test]
async fn non_member_is_forbidden() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[("other-account", &["R1"])]);
    let err = f.check(root(SPACE)).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
}

/// A non-member is not told a root exists.
#[tokio::test]
async fn non_member_is_forbidden_even_when_a_root_exists() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[("other-account", &["R1"])]);
    f.create_root(SPACE).await;
    let err = f.check(root(SPACE)).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
    assert!(err.get("existingId").is_none(), "{err}");
}

/// §Space File Tree: "A Space has zero, one, or more Space roots", and
/// this crate sets no limit, so a second root of the Space passes.
#[tokio::test]
async fn second_root_of_a_space_may_be_created() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[(ACCOUNT, &["R1"])]);
    f.create_root(SPACE).await;
    assert_eq!(f.check(root(SPACE)).await, None);
}

/// §Permission Mapping: `"send"` grants `mayAddChildren` but not root
/// creation, which "requires `"manage_space"` permission".
#[tokio::test]
async fn member_without_manage_space_is_forbidden() {
    let f = Fixture::new();
    f.seed_space(
        SPACE,
        &["view", "send", "manage_channels"],
        &[(ACCOUNT, &["R1"])],
    );
    let err = f.check(root(SPACE)).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
}

/// §Permission Mapping: a member whose role grants `"manage_space"` may
/// create a root.
#[tokio::test]
async fn member_with_manage_space_may_create() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[(ACCOUNT, &["R1"])]);
    assert_eq!(f.check(root(SPACE)).await, None);
}

/// §Permission Mapping: a role the member does not hold grants nothing.
#[tokio::test]
async fn unheld_role_grants_nothing() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[(ACCOUNT, &[])]);
    let err = f.check(root(SPACE)).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
}

/// chat L1705: the union includes "the implicit `@everyone` role".
#[tokio::test]
async fn everyone_floor_with_manage_space_may_create() {
    let f = Fixture::new();
    f.chat.set_everyone_permissions(HashSet::from([
        jmap_chat_types::ChatPermission::ManageSpace,
    ]));
    f.seed_space(SPACE, &[], &[(ACCOUNT, &[])]);
    assert_eq!(f.check(root(SPACE)).await, None);
}

/// chat L809: "Servers MUST ignore unrecognized permission names." A
/// hand-built `Other` spelling of `"manage_space"` is not the recognized
/// permission, so it grants nothing.
#[tokio::test]
async fn other_spelling_of_manage_space_grants_nothing() {
    let f = Fixture::new();
    f.chat
        .set_everyone_permissions(HashSet::from([jmap_chat_types::ChatPermission::Other(
            "manage_space".to_owned(),
        )]));
    f.seed_space(SPACE, &[], &[(ACCOUNT, &[])]);
    let err = f.check(root(SPACE)).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
}
