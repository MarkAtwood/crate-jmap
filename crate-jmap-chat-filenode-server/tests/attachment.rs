//! `resolve_filenode_attachment`: an Attachment linked by `filenodeId`.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! 0cd3af0 ("filenode Lnnn"):
//!
//! - L152-157, the §Permission Mapping table: `"view"` grants "Read",
//!   `"send"` "Read + Create", `"manage_channels"` "Read + Create +
//!   Delete", `"manage_space"` "Read + Create + Delete + Administer".
//! - L172: "When present in a `Message/set` create request for a channel
//!   Chat ..., the server uses that FileNode's blob as the attachment
//!   blob. When `filenodeId` is present on an Attachment, the fields
//!   `blobId`, `filename`, `contentType`, `size`, and `sha256` become
//!   OPTIONAL for that Attachment entry; the server MUST populate any
//!   absent fields from the referenced FileNode's properties before
//!   storage. ... If the client supplies these fields, the server MAY
//!   validate them against the FileNode and MAY reject the create with an
//!   `invalidProperties` SetError naming `attachments` if they conflict."
//! - L174: "When `filenodeId` is present, the server MUST verify that the
//!   referenced FileNode exists and belongs to the same Space. If
//!   validation fails, the server MUST return a SetError of type
//!   `invalidProperties` naming `filenodeId`."
//! - L176: "The sender has at least read access to the FileNode under the
//!   mapping in {{permissions}}."
//! - L178: "The FileNode is not modified when linked as an attachment."
//! - L217: "Servers SHOULD NOT permit a `filenodeId` referencing a
//!   FileNode from a different Space or from outside any Space file tree,
//!   as this could allow cross-Space data exfiltration".
//!
//! Records are hand-written JSON from the drafts' field lists;
//! SHA-256("abc") is the FIPS 180-2 Appendix B.1 / RFC 6234 §8.5 TEST1
//! vector.

use std::collections::HashSet;

use jmap_chat_filenode_server::resolve_filenode_attachment;
use jmap_chat_server::memory::MemoryBackend as ChatMemory;
use jmap_chat_types::{AttachmentCreate, Chat};
use jmap_filenode_server::memory::MemoryBackend as FilesMemory;
use jmap_filenode_types::FileNode;
use jmap_server::JmapBackend;
use jmap_types::{Id, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "A1";
const SPACE: &str = "S1";
const ROLE: &str = "chatspace";
/// SHA-256("abc"), FIPS 180-2 Appendix B.1 / RFC 6234 §8.5 TEST1.
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

struct Fixture {
    chat: ChatMemory,
    files: FilesMemory,
}

impl Fixture {
    /// Space S1 whose member A1 holds role R1 with `permissions`, and the
    /// FileNode trees:
    ///
    /// - `root-s1` (Space S1 root) / `docs` / `f1` (file, blob `b1`)
    /// - `root-s2` (Space S2 root) / `f2` (file, blob `b2`)
    /// - `loose` (top-level file outside any Space tree)
    fn new(permissions: &[&str]) -> Self {
        let chat = ChatMemory::new();
        chat.set_everyone_permissions(HashSet::new());
        chat.register_account(&Id::from(ACCOUNT));
        let space = json!({
            "id": SPACE,
            "name": "Space",
            "roles": [{ "id": "R1", "name": "Role", "permissions": permissions, "position": 1 }],
            "members": [{ "id": ACCOUNT, "roleIds": ["R1"], "joinedAt": "2026-01-01T00:00:00Z" }],
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
            json!({ "id": "root-s1", "@type": "Directory", "name": "S1", "parentId": null,
                    "role": ROLE, "spaceId": SPACE }),
            json!({ "id": "docs", "@type": "Directory", "name": "docs", "parentId": "root-s1" }),
            file("f1", "docs", "b1"),
            json!({ "id": "root-s2", "@type": "Directory", "name": "S2", "parentId": null,
                    "role": ROLE, "spaceId": "S2" }),
            file("f2", "root-s2", "b2"),
            json!({ "id": "loose", "@type": "File", "name": "loose.txt", "parentId": null,
                    "blobId": "b3", "size": 3, "sha256": SHA256_ABC, "mediaType": "text/plain" }),
        ] {
            files.seed_node(
                ACCOUNT,
                serde_json::from_value(node).expect("FileNode fixture"),
            );
        }
        Self { chat, files }
    }

    /// Resolve `attachment` for a message in `chat`; the filled
    /// Attachment or the SetError, as wire JSON.
    async fn resolve_in(&self, chat: &Chat, attachment: Value) -> Result<Option<Value>, Value> {
        let attachment: AttachmentCreate =
            serde_json::from_value(attachment).expect("AttachmentCreate fixture");
        resolve_filenode_attachment(
            &self.chat,
            &(),
            &self.files,
            &(),
            &Id::from(ACCOUNT),
            chat,
            &attachment,
        )
        .await
        .expect("storage")
        .map(|a| a.map(|a| serde_json::to_value(a).expect("Attachment serializes")))
        .map_err(|e| serde_json::to_value(e).expect("SetError serializes"))
    }

    async fn resolve(&self, attachment: Value) -> Result<Option<Value>, Value> {
        self.resolve_in(&channel(Some(SPACE)), attachment).await
    }
}

/// A File FileNode named `{id}.txt` holding "abc" as `text/plain`.
fn file(id: &str, parent: &str, blob: &str) -> Value {
    json!({ "id": id, "@type": "File", "name": format!("{id}.txt"), "parentId": parent,
            "blobId": blob, "size": 3, "sha256": SHA256_ABC, "mediaType": "text/plain" })
}

/// A Chat with `kind: "channel"` in `space_id`, or a group Chat if `None`.
fn channel(space_id: Option<&str>) -> Chat {
    let chat = match space_id {
        Some(s) => json!({ "id": "C1", "kind": "channel", "name": "general", "spaceId": s,
                           "createdAt": "2026-01-01T00:00:00Z", "unreadCount": 0 }),
        None => json!({ "id": "C1", "kind": "group", "name": "group",
                        "createdAt": "2026-01-01T00:00:00Z", "unreadCount": 0 }),
    };
    serde_json::from_value(chat).expect("Chat fixture")
}

fn assert_invalid(err: &Value, property: &str) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!([property]), "{err}");
}

/// The draft only extends Attachments that carry `filenodeId`; one
/// without it is left to the normal rules.
#[tokio::test]
async fn attachment_without_filenode_id_is_not_resolved() {
    let f = Fixture::new(&["view"]);
    let plain = json!({ "blobId": "b9", "filename": "x.txt", "contentType": "text/plain",
                        "size": 3, "sha256": SHA256_ABC });
    assert_eq!(f.resolve(plain).await, Ok(None));
}

/// L172: the five fields "become OPTIONAL" and "the server MUST populate
/// any absent fields from the referenced FileNode's properties"; the
/// FileNode's blob is "the attachment blob".
#[tokio::test]
async fn filenode_id_only_is_filled_from_the_node() {
    let f = Fixture::new(&["view"]);
    let filled = f
        .resolve(json!({ "filenodeId": "f1", "thumbnailBlobId": "t1" }))
        .await
        .expect("accepted")
        .expect("resolved");
    assert_eq!(
        filled,
        json!({ "blobId": "b1", "filename": "f1.txt", "contentType": "text/plain",
                "size": 3, "sha256": SHA256_ABC, "filenodeId": "f1",
                "thumbnailBlobId": "t1" })
    );
}

/// L172 fills only "absent fields": a supplied `filename` and
/// `contentType` are kept.
#[tokio::test]
async fn supplied_display_fields_are_kept() {
    let f = Fixture::new(&["view"]);
    let filled = f
        .resolve(json!({ "filenodeId": "f1", "filename": "renamed.txt",
                         "contentType": "text/markdown" }))
        .await
        .expect("accepted")
        .expect("resolved");
    assert_eq!(filled["filename"], "renamed.txt");
    assert_eq!(filled["contentType"], "text/markdown");
    assert_eq!(filled["blobId"], "b1");
}

/// L172: the server "MAY reject the create with an `invalidProperties`
/// SetError naming `attachments` if they conflict"; the attachment blob
/// is the FileNode's blob.
#[tokio::test]
async fn conflicting_blob_fields_are_invalid_attachments() {
    let f = Fixture::new(&["view"]);
    for conflict in [
        json!({ "filenodeId": "f1", "blobId": "other" }),
        json!({ "filenodeId": "f1", "size": 4 }),
        json!({ "filenodeId": "f1", "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" }),
    ] {
        let err = f.resolve(conflict.clone()).await.expect_err("refused");
        assert_invalid(&err, "attachments");
    }
    let matching = json!({ "filenodeId": "f1", "blobId": "b1", "size": 3, "sha256": SHA256_ABC });
    assert!(f.resolve(matching).await.expect("accepted").is_some());
}

/// L174: "verify that the referenced FileNode exists".
#[tokio::test]
async fn unknown_filenode_is_invalid_filenode_id() {
    let f = Fixture::new(&["view"]);
    let err = f
        .resolve(json!({ "filenodeId": "nope" }))
        .await
        .expect_err("refused");
    assert_invalid(&err, "filenodeId");
}

/// L174 "belongs to the same Space" and L217 "SHOULD NOT permit a
/// `filenodeId` referencing a FileNode from a different Space".
#[tokio::test]
async fn node_in_another_spaces_tree_is_invalid_filenode_id() {
    let f = Fixture::new(&["view"]);
    let err = f
        .resolve(json!({ "filenodeId": "f2" }))
        .await
        .expect_err("refused");
    assert_invalid(&err, "filenodeId");
}

/// L217: "or from outside any Space file tree".
#[tokio::test]
async fn node_outside_any_space_tree_is_invalid_filenode_id() {
    let f = Fixture::new(&["view"]);
    let err = f
        .resolve(json!({ "filenodeId": "loose" }))
        .await
        .expect_err("refused");
    assert_invalid(&err, "filenodeId");
}

/// L172: "the server uses that FileNode's blob as the attachment blob";
/// a Directory has none.
#[tokio::test]
async fn directory_is_invalid_filenode_id() {
    let f = Fixture::new(&["view"]);
    let err = f
        .resolve(json!({ "filenodeId": "docs" }))
        .await
        .expect_err("refused");
    assert_invalid(&err, "filenodeId");
}

/// L172 scopes `filenodeId` to "a channel Chat"; a group Chat has no
/// Space for the node to belong to (L174).
#[tokio::test]
async fn non_channel_chat_is_invalid_filenode_id() {
    let f = Fixture::new(&["view"]);
    let err = f
        .resolve_in(&channel(None), json!({ "filenodeId": "f1" }))
        .await
        .expect_err("refused");
    assert_invalid(&err, "filenodeId");
}

/// L176 with the L152-157 table: each of `"view"`, `"send"`,
/// `"manage_channels"` and `"manage_space"` grants Read.
#[tokio::test]
async fn every_mapped_permission_grants_read() {
    for permission in ["view", "send", "manage_channels", "manage_space"] {
        let f = Fixture::new(&[permission]);
        let got = f.resolve(json!({ "filenodeId": "f1" })).await;
        assert!(matches!(got, Ok(Some(_))), "{permission}: {got:?}");
    }
}

/// L176: without a permission from the L152-157 table the sender has no
/// read access, and the create is refused.
#[tokio::test]
async fn sender_without_read_access_is_refused() {
    let f = Fixture::new(&["pin"]);
    let err = f
        .resolve(json!({ "filenodeId": "f1" }))
        .await
        .expect_err("refused");
    assert_eq!(err["type"], "forbidden", "{err}");
}

/// L176 with the implicit `@everyone` role: a permission granted to
/// everyone counts toward read access.
#[tokio::test]
async fn everyone_view_grants_read() {
    let f = Fixture::new(&[]);
    f.chat
        .set_everyone_permissions(HashSet::from([jmap_chat_types::ChatPermission::View]));
    assert!(
        f.resolve(json!({ "filenodeId": "f1" }))
            .await
            .expect("accepted")
            .is_some()
    );
}

/// The stored `f1` and the FileNode state.
async fn read_f1(files: &FilesMemory) -> (Vec<FileNode>, State) {
    let account = Id::from(ACCOUNT);
    let (nodes, _) = files
        .get_objects::<FileNode>(&(), &account, Some(&[Id::from("f1")]), None)
        .await
        .expect("FileNode read");
    let state = files
        .get_state::<FileNode>(&(), &account)
        .await
        .expect("FileNode state");
    (nodes, state)
}

/// L178: "The FileNode is not modified when linked as an attachment."
#[tokio::test]
async fn linked_node_is_not_modified() {
    let f = Fixture::new(&["view"]);
    let before = read_f1(&f.files).await;
    f.resolve(json!({ "filenodeId": "f1" }))
        .await
        .expect("accepted");
    assert_eq!(read_f1(&f.files).await, before);
}
