//! `check_root_create`: the Space file tree root create checks.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! 33702a5 ("filenode Lnnn"):
//!
//! - L139: "When a client creates a FileNode with `role:
//!   "chatspace"`, it MUST supply `spaceId`, and its `@type` ... MUST be
//!   `"Directory"`. The server MUST validate:"
//! - L141: "The Space identified by `spaceId` exists and the requesting
//!   user is a member. If not, the server SHOULD return `forbidden` in
//!   both cases — Space not found and user not a member — to avoid
//!   revealing Space existence to non-members."
//! - L142: "No file tree root already exists for that Space ... If one
//!   exists, the server MUST return an `alreadyExists` SetError ... with
//!   `existingId` set to the id of the existing root."
//! - L143: "The requesting user holds sufficient permission to create the
//!   root ... If not, the server MUST return `forbidden`."
//! - L176: "Creating the Space file tree root ... requires
//!   `"manage_space"` permission ... This is a higher bar than the
//!   `mayAddChildren` grant to `"send"`".
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
const ROLE: &str = "chatspace";

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

    /// Create a Space file tree root for `space_id` through `FileNode/set`
    /// and return its id.
    async fn create_root(&self, space_id: &str) -> String {
        let out = handle_filenode_set(
            &self.files,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT,
                "create": { "r": { "@type": "Directory", "name": space_id, "parentId": null,
                                   "role": ROLE, "spaceId": space_id } }
            }),
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

fn root(space_id: Option<&str>) -> Value {
    let mut node = json!({ "id": "new", "@type": "Directory", "name": "files", "parentId": null, "role": ROLE });
    if let Some(s) = space_id {
        node["spaceId"] = json!(s);
    }
    node
}

/// L139 scopes the checks to creates "with `role:
/// "chatspace"`": other nodes pass without a Space.
#[tokio::test]
async fn non_root_node_is_not_checked() {
    let f = Fixture::new();
    let node = json!({ "id": "new", "@type": "Directory", "name": "docs", "parentId": null });
    assert_eq!(f.check(node).await, None);
}

/// L139: the client "MUST supply ... `spaceId`".
#[tokio::test]
async fn root_without_space_id_is_invalid_properties() {
    let f = Fixture::new();
    let err = f.check(root(None)).await.expect("refused");
    assert_eq!(err["type"], "invalidProperties");
    assert_eq!(err["properties"], json!(["spaceId"]));
}

/// L141: "the server SHOULD return `forbidden` in both cases — Space not
/// found and user not a member".
#[tokio::test]
async fn unknown_space_is_forbidden() {
    let f = Fixture::new();
    let err = f.check(root(Some("no-such-space"))).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
}

/// L141: "the requesting user is a member. If not, the server SHOULD
/// return `forbidden`".
#[tokio::test]
async fn non_member_is_forbidden() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[("other-account", &["R1"])]);
    let err = f.check(root(Some(SPACE))).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
}

/// L141 comes before L142: a non-member is not told a root exists.
#[tokio::test]
async fn non_member_is_forbidden_even_when_a_root_exists() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[("other-account", &["R1"])]);
    f.create_root(SPACE).await;
    let err = f.check(root(Some(SPACE))).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
    assert!(err.get("existingId").is_none(), "{err}");
}

/// L142: "If one exists, the server MUST return an `alreadyExists`
/// SetError ... with `existingId` set to the id of the existing root."
#[tokio::test]
async fn second_root_is_already_exists_with_existing_id() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[(ACCOUNT, &["R1"])]);
    let existing = f.create_root(SPACE).await;
    let err = f.check(root(Some(SPACE))).await.expect("refused");
    assert_eq!(err["type"], "alreadyExists");
    assert_eq!(err["existingId"], json!(existing));
}

/// L142: "No file tree root already exists for that Space": another
/// Space's root does not count.
#[tokio::test]
async fn another_spaces_root_does_not_block() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[(ACCOUNT, &["R1"])]);
    f.create_root("S2").await;
    assert_eq!(f.check(root(Some(SPACE))).await, None);
}

/// L143 and L176: `"send"` grants `mayAddChildren` but not root creation, which
/// "requires `"manage_space"` permission".
#[tokio::test]
async fn member_without_manage_space_is_forbidden() {
    let f = Fixture::new();
    f.seed_space(
        SPACE,
        &["view", "send", "manage_channels"],
        &[(ACCOUNT, &["R1"])],
    );
    let err = f.check(root(Some(SPACE))).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
}

/// L176: a member whose role grants `"manage_space"` may create the root.
#[tokio::test]
async fn member_with_manage_space_may_create() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[(ACCOUNT, &["R1"])]);
    assert_eq!(f.check(root(Some(SPACE))).await, None);
}

/// L176: a role the member does not hold grants nothing.
#[tokio::test]
async fn unheld_role_grants_nothing() {
    let f = Fixture::new();
    f.seed_space(SPACE, &["manage_space"], &[(ACCOUNT, &[])]);
    let err = f.check(root(Some(SPACE))).await.expect("refused");
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
    assert_eq!(f.check(root(Some(SPACE))).await, None);
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
    let err = f.check(root(Some(SPACE))).await.expect("refused");
    assert_eq!(err["type"], "forbidden");
}
