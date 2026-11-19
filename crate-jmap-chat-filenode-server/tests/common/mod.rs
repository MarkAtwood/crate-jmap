//! A deployment wired as the crate docs direct, for the integration tests.

use std::collections::HashSet;

use jmap_chat_filenode_server::{check_space_destroy, space_files_rights};
use jmap_chat_server::memory::MemoryBackend as ChatMemory;
use jmap_filenode_server::memory::{MemoryBackend as FilesMemory, MemoryError};
use jmap_filenode_server::{CaseFolding, FileNodeBackend};
use jmap_filenode_types::{FileNode, FilesRights};
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapAccounts, JmapObject, JmapReads, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, SetObject,
};
use jmap_types::{Id, State};
use serde_json::json;

pub const ACCOUNT: &str = "A1";
pub const SPACE: &str = "S1";

/// A deployment offering both drafts: FileNode storage delegated to
/// `FilesMemory`, wired as the `jmap_chat_filenode_server` crate docs
/// direct: `files_rights` returns `space_files_rights`, and
/// `destroy_object` and `may_destroy` apply `check_space_destroy`.
pub struct Deployment {
    pub chat: ChatMemory,
    pub files: FilesMemory,
}

impl Deployment {
    /// Space S1 whose only member A1 holds a role with `permissions`, and
    /// the FileNode trees `root` (Space S1 root) / `docs` / `f1` (file),
    /// plus `loose`, a top-level directory outside any Space tree.
    pub fn new(permissions: &[&str]) -> Self {
        let chat = ChatMemory::new();
        chat.set_everyone_permissions(HashSet::new());
        chat.register_account(&Id::from(ACCOUNT));
        let space = json!({
            "id": SPACE,
            "name": "Space",
            "roles": [{ "id": "R1", "name": "Role", "permissions": permissions, "position": 1 }],
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

        let files = FilesMemory::new().with_account(&Id::from(ACCOUNT));
        for node in [
            json!({ "id": "root", "@type": "Directory", "name": "S1", "parentId": null,
                    "spaceId": SPACE }),
            json!({ "id": "docs", "@type": "Directory", "name": "docs", "parentId": "root" }),
            json!({ "id": "f1", "@type": "File", "name": "f1.txt", "parentId": "docs",
                    "blobId": "b1", "size": 3, "mediaType": "text/plain" }),
            json!({ "id": "loose", "@type": "Directory", "name": "loose", "parentId": null }),
        ] {
            files.seed_node(
                &Id::from(ACCOUNT),
                serde_json::from_value(node).expect("FileNode fixture"),
            );
        }
        Self { chat, files }
    }

    /// The stored node `id`, if any.
    async fn stored(&self, account_id: &Id, id: &Id) -> Result<Option<FileNode>, MemoryError> {
        let (nodes, _) = self
            .files
            .get_objects::<FileNode>(&(), account_id, Some(std::slice::from_ref(id)), None)
            .await?;
        Ok(nodes.into_iter().next())
    }
}

impl JmapAccounts for Deployment {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        self.files.account_exists(&(), account_id).await
    }
}

impl JmapReads for Deployment {
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
        params: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.files.get_changes::<O>(&(), account_id, params).await
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

    async fn copy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        from_account_id: &Id,
        to_account_id: &Id,
        obj: O,
        create: &serde_json::Value,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        self.files
            .copy_object(&(), from_account_id, to_account_id, obj, create)
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
        // A memory store has no concurrent writer between this check and
        // the removal.
        if let Some(node) = self
            .stored(account_id, id)
            .await
            .map_err(BackendSetError::Other)?
        {
            check_space_destroy(&self.chat, &(), &self.files, &(), account_id, &node)
                .await
                .map_err(|e| BackendSetError::Other(MemoryError::new(e.to_string())))?
                .map_err(|e| BackendSetError::SetError(Box::new(e)))?;
        }
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

    async fn may_destroy(
        &self,
        _caller: &(),
        account_id: &Id,
        node: &FileNode,
    ) -> Result<bool, Self::Error> {
        check_space_destroy(&self.chat, &(), &self.files, &(), account_id, node)
            .await
            .map(|r| r.is_ok())
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
