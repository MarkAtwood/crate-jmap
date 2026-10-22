//! A deployment whose `FileNodeBackend::files_rights` returns
//! `space_files_rights` reports the Space role mapping in `FileNode/get`
//! `myRights`.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! 33702a5 ("filenode Lnnn"):
//!
//! - L161: "the following table maps role permissions to the `FilesRights`
//!   flags ({{JMAP-FILENODE}}) the server reports in `myRights` and
//!   enforces on each FileNode in the Space file tree. Each row includes
//!   the grants of the rows above it. ... A flag no applicable row grants
//!   is `false`. ... `mayAddChildren` is always `false` on nodes whose
//!   `@type` does not permit children".
//! - L165-166: `"view"` "`mayRead` on every node"; `"send"` "The above,
//!   plus `mayAddChildren`".
//!
//! Records and expected rights are hand-written JSON from those lines.

use std::collections::HashSet;

use jmap_chat_filenode_server::space_files_rights;
use jmap_chat_server::memory::MemoryBackend as ChatMemory;
use jmap_filenode_server::filenode::handle_filenode_get;
use jmap_filenode_server::memory::{MemoryBackend as FilesMemory, MemoryError};
use jmap_filenode_server::{
    BackendChangesError, BackendQueryError, BackendSetError, CaseFolding, ChangesResult,
    FileNodeBackend, GetObject, JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult,
    QueryObject, QueryParams, QueryResult, SetObject,
};
use jmap_filenode_types::{FileNode, FilesRights};
use jmap_types::{Id, State};
use serde_json::{json, Value};

const ACCOUNT: &str = "A1";
const SPACE: &str = "S1";

/// A deployment offering both drafts: FileNode storage delegated to
/// `FilesMemory`, with `files_rights` wired to `space_files_rights`.
struct Deployment {
    chat: ChatMemory,
    files: FilesMemory,
}

impl Deployment {
    /// Space S1 whose only member A1 holds a role with `"send"`, and the
    /// FileNode trees `root` (Space S1 root) / `docs` / `f1` (file), plus
    /// `loose`, a top-level directory outside any Space tree.
    fn with_send_member() -> Self {
        let chat = ChatMemory::new();
        chat.set_everyone_permissions(HashSet::new());
        chat.register_account(&Id::from(ACCOUNT));
        let space = json!({
            "id": SPACE,
            "name": "Space",
            "roles": [{ "id": "R1", "name": "Role", "permissions": ["send"], "position": 1 }],
            "members": [{ "id": ACCOUNT, "roleIds": ["R1"],
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
                    "role": "chatspace", "spaceId": SPACE }),
            json!({ "id": "docs", "@type": "Directory", "name": "docs", "parentId": "root" }),
            json!({ "id": "f1", "@type": "File", "name": "f1.txt", "parentId": "docs",
                    "blobId": "b1", "size": 3, "mediaType": "text/plain" }),
            json!({ "id": "loose", "@type": "Directory", "name": "loose", "parentId": null }),
        ] {
            files.seed_node(
                ACCOUNT,
                serde_json::from_value(node).expect("FileNode fixture"),
            );
        }
        Self { chat, files }
    }
}

impl JmapBackend for Deployment {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        self.files.account_exists(&(), account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        self.files
            .get_objects::<O>(&(), account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.files.get_state::<O>(&(), account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.files
            .get_changes::<O>(&(), account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        self.files.query_objects::<O>(&(), account_id, params).await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.files.query_changes::<O>(&(), account_id, params).await
    }
}

impl FileNodeBackend for Deployment {
    async fn create_object<O, F>(
        &self,
        _caller: &(),
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<Self::Error>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        self.files
            .create_object(&(), account_id, create_id, build)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        self.files
            .update_object::<O>(&(), account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        self.files.destroy_object::<O>(&(), account_id, id).await
    }

    fn supports_type<O: JmapObject>(&self, caller: &(), account_id: &Id) -> bool {
        self.files.supports_type::<O>(caller, account_id)
    }

    async fn get_ancestors(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: &[Id],
    ) -> Result<Vec<FileNode>, Self::Error> {
        self.files.get_ancestors(&(), account_id, ids).await
    }

    async fn get_descendant_ids(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<Vec<Id>, Self::Error> {
        self.files.get_descendant_ids(&(), account_id, id).await
    }

    async fn blob_exists(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<bool, Self::Error> {
        self.files.blob_exists(&(), account_id, blob_id).await
    }

    async fn files_rights(
        &self,
        _caller: &(),
        account_id: &Id,
        node: &FileNode,
    ) -> Result<Option<FilesRights>, Self::Error> {
        space_files_rights(&self.chat, &(), &self.files, &(), account_id, node)
            .await
            .map_err(|e| MemoryError::new(e.to_string()))
    }

    async fn find_sibling_by_name(
        &self,
        _caller: &(),
        account_id: &Id,
        parent_id: Option<&Id>,
        name: &str,
        folding: CaseFolding,
    ) -> Result<Option<Id>, Self::Error> {
        self.files
            .find_sibling_by_name(&(), account_id, parent_id, name, folding)
            .await
    }
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

/// L165-166 for a `"send"` member: `mayRead` and `mayAddChildren` on the
/// root and on a directory, `mayRead` alone on a file (L161, no children).
/// The ancestors `fetchParents` appends carry their rights too. `loose` is
/// outside every Space tree, so it keeps its stored `myRights`.
#[tokio::test]
async fn get_reports_space_role_rights() {
    let deployment = Deployment::with_send_member();
    let args = json!({ "accountId": ACCOUNT, "ids": ["f1", "loose"], "fetchParents": true });
    let resp = handle_filenode_get(&deployment, &(), args)
        .await
        .expect("get")
        .primary;
    let my_rights = |id: &str| {
        resp["list"]
            .as_array()
            .expect("list")
            .iter()
            .find(|n| n["id"] == id)
            .unwrap_or_else(|| panic!("{id} listed: {resp}"))["myRights"]
            .clone()
    };
    let read = "mayRead";
    let add = "mayAddChildren";
    assert_eq!(my_rights("root"), rights(&[read, add]), "{resp}");
    assert_eq!(my_rights("docs"), rights(&[read, add]), "{resp}");
    assert_eq!(my_rights("f1"), rights(&[read]), "{resp}");
    assert_eq!(my_rights("loose"), Value::Null, "{resp}");
}
