//! Chat binding for the JMAP FileNode method handlers: the rules of
//! draft-atwood-jmap-chat-filenode-00 that apply when a server offers
//! `urn:ietf:params:jmap:chat:filenode` together with
//! `urn:ietf:params:jmap:chat` and `urn:ietf:params:jmap:filenode`.
//!
//! `jmap-filenode-server` and `jmap-chat-server` do not depend on each
//! other. A deployment that offers both implements
//! [`jmap_filenode_server::FileNodeBackend`] and
//! [`jmap_chat_server::ChatBackend`] and calls these functions from its
//! trait methods:
//!
//! | Backend method | Call |
//! |---|---|
//! | `FileNodeBackend::create_object::<FileNode>`, before the insert | [`check_root_create`] and [`check_space_create`] |
//! | `FileNodeBackend::update_object::<FileNode>`, before the write | [`check_space_update`] |
//! | `FileNodeBackend::destroy_object::<FileNode>`, before the removal | [`check_space_destroy`] |
//! | `FileNodeBackend::files_rights`, which `FileNode/get` and the `FileNode/set` and `FileNode/copy` `created` entries report as `myRights` | return [`space_files_rights`] |
//! | `JmapBackend::get_objects::<FileNode>` and the other FileNode reads, to leave out nodes with no flag | [`space_files_rights`] |
//! | The same, when advertising `maxSpaceStorageBytes` | [`check_space_quota`] |
//! | `ChatBackend::create_object::<Space>`, after the insert, when advertising `autoCreateFileTree: true` | [`create_space_file_tree`] |
//! | `ChatBackend::filenode_attachment` | [`resolve_filenode_attachment`] |
//! | `ChatBackend::on_space_destroyed` | [`destroy_space_file_tree`] |
//!
//! Each function takes the backends it reads, with their own caller
//! contexts and error types.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::fmt;

use jmap_chat_server::ChatBackend;
use jmap_chat_types::{Attachment, AttachmentCreate, Chat, ChatKind, ChatPermission, Space};
use jmap_filenode_server::FileNodeBackend;
use jmap_filenode_types::{FileNode, FileNodeFilterCondition, FilesRights, NodeType};
use jmap_server::{BackendQueryError, BackendSetError, QueryParams, SetError, SetErrorType};
use jmap_types::{Filter, Id, PatchObject};

/// A storage failure while checking a FileNode operation or a
/// `filenodeId` Attachment.
#[non_exhaustive]
#[derive(Debug)]
pub enum RootCheckError<C, F> {
    /// The Chat backend failed to read the Space.
    Chat(C),
    /// The FileNode backend failed to read the Space file tree.
    Files(BackendQueryError<F>),
}

impl<C, F> fmt::Display for RootCheckError<C, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Chat(_) => "reading the Space failed",
            Self::Files(_) => "reading the Space file tree failed",
        })
    }
}

impl<C, F> std::error::Error for RootCheckError<C, F>
where
    C: std::error::Error + 'static,
    F: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Chat(e) => Some(e),
            Self::Files(e) => Some(e),
        }
    }
}

/// Check draft-atwood-jmap-chat-filenode-00 §Creating a Space Root for a
/// `FileNode/set` create of `node`.
///
/// A node without `spaceId` passes unchecked, so this is safe to call
/// before every FileNode insert. A node with one is a Space root, "A
/// Directory FileNode whose `spaceId` matches the Space's `id`" (the
/// `@type: "Directory"` rule is already enforced by
/// `jmap-filenode-server`). In this order:
///
/// - "The Space identified by `spaceId` exists and the requesting user is
///   a member. If not, the server SHOULD return `forbidden` in both cases
///   — Space not found and user not a member — to avoid revealing Space
///   existence to non-members."
/// - "Creating, renaming, or destroying a Space root requires
///   `"manage_space"` permission, whatever the root's `myRights` report"
///   (§Permission Mapping); without it the create fails with `forbidden`.
///   The permissions are the union over the member's SpaceRoles plus the
///   implicit `@everyone` role's permissions from
///   [`ChatBackend::everyone_permissions`] (draft-atwood-jmap-chat-00
///   §Space Permission Resolution step 1). ChannelPermission overrides "do
///   not restrict file-tree access".
///
/// "A Space has zero, one, or more Space roots ... a server MAY limit the
/// number of Space roots per Space". This function sets no limit; a
/// deployment that wants one counts the roots with a `spaceId`
/// `FileNode/query` and returns `forbidden`, as the draft asks.
///
/// The requesting user is the owner of `account_id`, named by
/// `account_id` in Space member lists (draft-atwood-jmap-chat-00
/// §Terminology). `account_id` is the account of both the Space and the
/// FileNode. `files` and `files_caller` are not read.
///
/// Returns `Ok(Err(set_error))` when the create must be refused. Return
/// that SetError from `create_object` without inserting.
///
/// # Errors
///
/// Returns [`RootCheckError::Chat`] if reading the Space or its `@everyone`
/// permissions fails.
pub async fn check_root_create<C: ChatBackend, F: FileNodeBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    _files: &F,
    _files_caller: &F::CallerCtx,
    account_id: &Id,
    node: &FileNode,
) -> Result<Result<(), SetError>, RootCheckError<C::Error, F::Error>> {
    let Some(space_id) = &node.space_id else {
        return Ok(Ok(()));
    };
    let manages = manages_space(chat, chat_caller, account_id, space_id)
        .await
        .map_err(RootCheckError::Chat)?;
    Ok(match manages {
        None => Err(forbidden(
            "spaceId does not name a Space the caller is a member of",
        )),
        Some(false) => Err(forbidden(
            "creating a Space root requires the manage_space permission",
        )),
        Some(true) => Ok(()),
    })
}

/// Check draft-atwood-jmap-chat-filenode-00 `maxSpaceStorageBytes` for a
/// `FileNode/set` create of `node`: "When a `FileNode/set` create would
/// cause the Space file tree to exceed this limit, the server MUST return
/// an `overQuota` SetError".
///
/// The limit covers the "total blob storage in bytes across all FileNodes
/// in a single Space's file tree, counting all of its Space roots": the
/// `size` of every Space root of the Space `node` is created in, and of
/// every node below them, plus `node.size`. The Space is that of the
/// nearest Space root among the new parent and its ancestors. Call it with
/// `node` as the backend will store it, after setting the server-set
/// `size` from the blob. A node with no Space root above it passes
/// unchecked, as does a top-level node.
///
/// Like the backend's other create pre-checks, this is not atomic with
/// the insert: a backend with concurrent writers re-checks under the
/// insert's lock or transaction.
///
/// Returns `Ok(Err(set_error))` when the create must be refused; return
/// that SetError from `create_object` without inserting.
///
/// # Errors
///
/// Returns the storage error of the first FileNode read that fails.
pub async fn check_space_quota<F: FileNodeBackend>(
    files: &F,
    caller: &F::CallerCtx,
    account_id: &Id,
    node: &FileNode,
    max_space_storage_bytes: u64,
) -> Result<Result<(), SetError>, BackendQueryError<F::Error>> {
    let Some(parent_id) = &node.parent_id else {
        return Ok(Ok(()));
    };
    let parent_ids = std::slice::from_ref(parent_id);
    let (parents, _) = files
        .get_objects::<FileNode>(caller, account_id, Some(parent_ids), None)
        .await
        .map_err(BackendQueryError::Other)?;
    let Some(parent) = parents.into_iter().find(|n| &n.id == parent_id) else {
        return Ok(Ok(()));
    };
    let ancestors = files
        .get_ancestors(caller, account_id, parent_ids)
        .await
        .map_err(BackendQueryError::Other)?;
    let Some(space_id) = space_of(&parent, &ancestors) else {
        return Ok(Ok(()));
    };

    let mut condition = FileNodeFilterCondition::default();
    condition.space_id = Some(space_id);
    let filter = Filter::Condition(condition);
    let roots = files
        .query_objects::<FileNode>(caller, account_id, QueryParams::new().filter(Some(&filter)))
        .await?
        .ids;
    // ponytail: reads every tree of the Space on every create, O(tree
    // size). Upgrade path: a per-Space usage total the backend keeps with
    // each write.
    let below = files
        .query_subtree(caller, account_id, &roots, u64::MAX)
        .await?;
    // A Space root inside another one's tree counts once.
    let ids: Vec<Id> = roots
        .into_iter()
        .chain(below)
        .collect::<HashSet<Id>>()
        .into_iter()
        .collect();
    let (tree, _) = files
        .get_objects::<FileNode>(caller, account_id, Some(&ids), None)
        .await
        .map_err(BackendQueryError::Other)?;
    let total = tree
        .iter()
        .chain([node])
        .fold(0u64, |sum, n| sum.saturating_add(n.size.unwrap_or(0)));
    Ok(if total > max_space_storage_bytes {
        Err(SetError::new(SetErrorType::OverQuota)
            .with_description("the create would exceed maxSpaceStorageBytes for the Space"))
    } else {
        Ok(())
    })
}

/// Create the Space root for a new Space, for a server advertising
/// draft-atwood-jmap-chat-filenode-00 `autoCreateFileTree: true`: "the
/// server automatically creates one Space root for each new Space at Space
/// creation time and includes the new FileNode in subsequent
/// `FileNode/changes` responses."
///
/// The root is a top-level `Directory` with `spaceId` set to `space_id`,
/// named `space_id` since the draft leaves the name open. It has no
/// `role`: the draft identifies Space roots by `spaceId` alone, and
/// draft-ietf-jmap-filenode-20 §3.1.4 allows each role on only one
/// Directory per account. It is inserted with [`FileNodeBackend::create_object`],
/// whose contract records the create for `FileNode/changes`. Call it from
/// the Space insert with the Space's account; `caller` is a server context,
/// as no client creates this node.
///
/// Returns the new root's id and the stored node.
///
/// # Errors
///
/// Returns the backend's error from `create_object`.
// Same error type as `FileNodeBackend::create_object`, which it propagates.
pub async fn create_space_file_tree<F: FileNodeBackend>(
    files: &F,
    caller: &F::CallerCtx,
    account_id: &Id,
    space_id: &Id,
) -> Result<(Id, FileNode), BackendSetError<F::Error>> {
    let root = |id: &Id| -> FileNode {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "@type": "Directory",
            "name": space_id,
            "parentId": null,
            "spaceId": space_id,
        }))
        .expect("a Directory FileNode with every required field deserializes")
    };
    files
        .create_object(caller, account_id, "spaceFileTree", root)
        .await
}

/// A storage failure while destroying a Space file tree.
#[non_exhaustive]
#[derive(Debug)]
pub enum TreeDestroyError<E> {
    /// Finding the root or its descendants failed.
    Query(BackendQueryError<E>),
    /// Destroying a FileNode failed.
    Destroy(BackendSetError<E>),
}

impl<E> fmt::Display for TreeDestroyError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Query(_) => "reading the Space file tree failed",
            Self::Destroy(_) => "destroying a Space file tree node failed",
        })
    }
}

impl<E: std::error::Error + 'static> std::error::Error for TreeDestroyError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Query(e) => Some(e),
            Self::Destroy(e) => Some(e),
        }
    }
}

/// Destroy the file trees of a destroyed Space, per
/// draft-atwood-jmap-chat-filenode-00 §Space Deletion: "When a Space is
/// destroyed via `Space/set` destroy, the server SHOULD also destroy all
/// of its Space roots and all FileNodes within them." Call it from
/// `ChatBackend::on_space_destroyed`.
///
/// No grace period is offered (the draft's "MAY instead retain"), so the
/// trees go with the Space and no former member can reach them afterwards.
/// The Space roots are found with the `spaceId` `FileNode/query` filter;
/// each descendant is destroyed before its root, as `FileNode/set` does
/// for `onDestroyRemoveChildren`. Every node goes through
/// [`FileNodeBackend::destroy_object`], whose contract advances the
/// FileNode state, so `FileNode/changes` lists the destroyed ids and the
/// push channel can emit the `FileNode` state change the draft asks for
/// "at the moment the file tree is destroyed". A Space without a file
/// tree destroys nothing.
///
/// `caller` is a server context, as no client destroys these nodes, and
/// `account_id` is the Space's account. The destroy is not atomic: on an
/// error the nodes destroyed so far stay destroyed, and calling it again
/// destroys the rest.
///
/// Returns the destroyed ids, descendants before their root.
///
/// # Errors
///
/// Returns [`TreeDestroyError::Query`] if finding the nodes fails, and
/// [`TreeDestroyError::Destroy`] for the first destroy that fails.
// Carries `FileNodeBackend::destroy_object`'s error, as for
// `create_space_file_tree`.
pub async fn destroy_space_file_tree<F: FileNodeBackend>(
    files: &F,
    caller: &F::CallerCtx,
    account_id: &Id,
    space_id: &Id,
) -> Result<Vec<Id>, TreeDestroyError<F::Error>> {
    let mut condition = FileNodeFilterCondition::default();
    condition.space_id = Some(space_id.clone());
    let filter = Filter::Condition(condition);
    let roots = files
        .query_objects::<FileNode>(caller, account_id, QueryParams::new().filter(Some(&filter)))
        .await
        .map_err(TreeDestroyError::Query)?
        .ids;

    let mut destroyed = Vec::new();
    for root_id in roots {
        // A Space root inside an earlier root's tree is already gone.
        if destroyed.contains(&root_id) {
            continue;
        }
        let descendants = files
            .get_descendant_ids(caller, account_id, &root_id)
            .await
            .map_err(|e| TreeDestroyError::Query(BackendQueryError::Other(e)))?;
        for id in descendants.into_iter().chain([root_id]) {
            files
                .destroy_object::<FileNode>(caller, account_id, &id)
                .await
                .map_err(TreeDestroyError::Destroy)?;
            destroyed.push(id);
        }
    }
    Ok(destroyed)
}

/// Resolve draft-atwood-jmap-chat-filenode-00 §filenodeId for an
/// Attachment of a `Message/set` create in `message_chat`, filling the
/// fields the client left out. Call it from
/// `ChatBackend::filenode_attachment` and return its result.
///
/// An `attachment` without `filenodeId` gives `Ok(Ok(None))`. Otherwise,
/// in this order:
///
/// - "the server MUST verify that the referenced FileNode exists and
///   belongs to the same Space. If validation fails, the server MUST
///   return a SetError of type `invalidProperties` naming `filenodeId`."
///   The node's nearest Space root ancestor (a Directory with `spaceId`)
///   must have the Chat's `spaceId`; §Attachment filenodeId Validation: "Servers SHOULD NOT
///   permit a `filenodeId` referencing a FileNode from a different Space
///   or from outside any Space file tree". A message to a Chat that is
///   not a channel has no Space, so it fails the same way. A node without
///   a blob, such as a Directory, fails too, as "the server uses that
///   FileNode's blob as the attachment blob".
/// - "The sender has at least read access to the FileNode under the
///   mapping in {{permissions}}": every row of that table grants Read, so
///   the sender must be a Space member holding `view`, `send`,
///   `manage_channels` or `manage_space` through a SpaceRole or the
///   implicit `@everyone` role. Otherwise
///   the create fails with `forbidden`, which the draft does not name.
/// - "the server MUST populate any absent fields from the referenced
///   FileNode's properties": `blobId`, `filename`, `contentType`, `size`
///   and `sha256` from the node's `blobId`, `name`, `mediaType`, `size` and
///   `sha256`. "If the client supplies these fields, the server MAY
///   validate them against the FileNode and MAY reject the create with an
///   `invalidProperties` SetError naming `attachments` if they conflict":
///   a supplied `blobId`, `size` or `sha256` that differs from the node's
///   is rejected, as those describe the blob; a supplied `filename` or
///   `contentType` is kept. A field that is neither supplied nor on the
///   node is also `invalidProperties` naming `attachments`.
///
/// The sender is the owner of `account_id`, named by `account_id` in
/// Space member lists, and `account_id` is the account of both the Space
/// and the FileNode, as for [`check_root_create`]. The node is only read:
/// "The FileNode is not modified when linked as an attachment."
///
/// # Errors
///
/// Returns [`RootCheckError::Files`] if reading the node or its ancestors
/// fails, and [`RootCheckError::Chat`] if reading the Space or its
/// `@everyone` permissions fails.
pub async fn resolve_filenode_attachment<C: ChatBackend, F: FileNodeBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    files: &F,
    files_caller: &F::CallerCtx,
    account_id: &Id,
    message_chat: &Chat,
    attachment: &AttachmentCreate,
) -> Result<Result<Option<Attachment>, SetError>, RootCheckError<C::Error, F::Error>> {
    let Some(node_id) = &attachment.filenode_id else {
        return Ok(Ok(None));
    };
    let not_in_space = || {
        Ok(Err(SetError::new(SetErrorType::InvalidProperties)
            .with_properties(["filenodeId"])
            .with_description(
                "filenodeId does not name a file in the channel's Space file tree",
            )))
    };
    let space_id = match &message_chat.space_id {
        Some(id) if message_chat.kind == ChatKind::Channel => id,
        _ => return not_in_space(),
    };

    let node_ids = std::slice::from_ref(node_id);
    let files_err = |e| RootCheckError::Files(BackendQueryError::Other(e));
    let (nodes, _) = files
        .get_objects::<FileNode>(files_caller, account_id, Some(node_ids), None)
        .await
        .map_err(files_err)?;
    let Some(node) = nodes.into_iter().find(|n| &n.id == node_id) else {
        return not_in_space();
    };
    let Some(blob_id) = node.blob_id.clone() else {
        return not_in_space();
    };
    let ancestors = files
        .get_ancestors(files_caller, account_id, node_ids)
        .await
        .map_err(files_err)?;
    // The node is a File, so it is no Space root itself.
    if space_of(&node, &ancestors).as_ref() != Some(space_id) {
        return not_in_space();
    }

    let (spaces, _) = chat
        .get_objects::<Space>(
            chat_caller,
            account_id,
            Some(std::slice::from_ref(space_id)),
            None,
        )
        .await
        .map_err(RootCheckError::Chat)?;
    let space = spaces.iter().find(|s| &s.id == space_id);
    let may_read = member_permissions(chat, chat_caller, account_id, space)
        .await
        .map_err(RootCheckError::Chat)?
        .is_some_and(|p| TIERS.iter().any(|r| p.contains(r)));
    if !may_read {
        return Ok(Err(forbidden(
            "the sender has no read access to the Space file tree",
        )));
    }

    // The node's sha256 is readable by the sender (FileNode/get), so the
    // plain comparison leaks nothing.
    let conflicts = attachment.blob_id.as_ref().is_some_and(|b| *b != blob_id)
        || attachment.size.is_some_and(|s| node.size != Some(s))
        || attachment
            .sha256
            .as_ref()
            .is_some_and(|h| node.sha256.as_ref() != Some(h));
    let invalid = |description: &str| {
        Ok(Err(SetError::new(SetErrorType::InvalidProperties)
            .with_properties(["attachments"])
            .with_description(description)))
    };
    if conflicts {
        return invalid("blobId, size or sha256 conflicts with the FileNode");
    }

    let mut filled = serde_json::to_value(attachment).expect("AttachmentCreate serializes to JSON");
    if let Some(obj) = filled.as_object_mut() {
        for (key, value) in [
            ("blobId", Some(blob_id.as_ref().into())),
            ("filename", Some(node.name.clone().into())),
            ("contentType", node.media_type.clone().map(Into::into)),
            ("size", node.size.map(Into::into)),
            ("sha256", node.sha256.as_ref().map(|d| d.as_str().into())),
        ] {
            if let Some(v) = value {
                obj.entry(key).or_insert(v);
            }
        }
    }
    match serde_json::from_value(filled) {
        Ok(filled) => Ok(Ok(Some(filled))),
        Err(_) => invalid("the FileNode lacks a required Attachment field"),
    }
}

/// The `FilesRights` the owner of `account_id` holds on `node`, per the
/// draft-atwood-jmap-chat-filenode-00 §Permission Mapping table, or `None`
/// if `node` is not in a Space file tree.
///
/// Return the result from [`FileNodeBackend::files_rights`], which
/// `FileNode/get` reports as a Directory's `myRights`: the table gives "the
/// `FilesRights` flags ... the server reports in the `myRights` of every
/// Directory in the Space file tree, including the tree root, and
/// enforces". "Each row includes the grants of the rows above it", and the
/// member's permissions are the union over their SpaceRoles plus the
/// `@everyone` role, as for [`check_root_create`]:
///
/// | Highest permission held | Flags `true` |
/// |---|---|
/// | `view` | `mayRead` |
/// | `send` | the above, plus `mayAddChildren` |
/// | `manage_channels` or `manage_space` | the above, plus `mayDelete`, `mayRename` and `mayModifyContent` |
///
/// "A flag no applicable row grants is `false`", so `mayShare` and any
/// flag a later draft adds are always `false`, and a non-member, who
/// "SHOULD NOT be granted access", gets no flag at all. Every Directory in
/// the tree, the root included, gets the same flags. "A File or SymLink
/// takes its rights from its parent Directory", so a File or SymLink gets
/// them too: these are the rights the `FileNode/set` gates check for it.
/// A Space root's flags "do not decide who may destroy, rename, or move the
/// Space root itself": [`check_space_update`] and [`check_space_destroy`]
/// decide that.
///
/// A node with no flag is not discoverable (draft-ietf-jmap-filenode
/// §Access Control), so the read path leaves it out: `FileNode/get`
/// reports it in `notFound`, `FileNode/query` omits it, and
/// `FileNode/changes` omits it from `created` and `updated`, as
/// "FileNode/changes responses SHOULD be scoped to file trees the
/// requesting user has access to".
///
/// `node` is a stored node: its tree is found from its ancestors. A node
/// is in the Space file tree of the nearest Space root (a node with a
/// `spaceId`) among itself and its ancestors.
///
/// # Errors
///
/// Returns [`RootCheckError::Files`] if reading the ancestors fails, and
/// [`RootCheckError::Chat`] if reading the Space or its `@everyone`
/// permissions fails.
pub async fn space_files_rights<C: ChatBackend, F: FileNodeBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    files: &F,
    files_caller: &F::CallerCtx,
    account_id: &Id,
    node: &FileNode,
) -> Result<Option<FilesRights>, RootCheckError<C::Error, F::Error>> {
    let ancestors = if node.space_id.is_none() && node.parent_id.is_some() {
        files
            .get_ancestors(files_caller, account_id, std::slice::from_ref(&node.id))
            .await
            .map_err(|e| RootCheckError::Files(BackendQueryError::Other(e)))?
    } else {
        Vec::new()
    };
    let Some(space_id) = space_of(node, &ancestors) else {
        return Ok(None);
    };

    let (spaces, _) = chat
        .get_objects::<Space>(
            chat_caller,
            account_id,
            Some(std::slice::from_ref(&space_id)),
            None,
        )
        .await
        .map_err(RootCheckError::Chat)?;
    let space = spaces.iter().find(|s| s.id == space_id);
    let tier = member_permissions(chat, chat_caller, account_id, space)
        .await
        .map_err(RootCheckError::Chat)?
        .and_then(|p| TIERS.iter().rposition(|t| p.contains(t)))
        .map_or(0, |i| i + 1);

    let manages = tier >= 3;
    let mut rights = FilesRights::default();
    rights.may_read = tier >= 1;
    rights.may_add_children = tier >= 2;
    rights.may_delete = manages;
    rights.may_rename = manages;
    rights.may_modify_content = manages;
    Ok(Some(rights))
}

/// Check a `FileNode/set` create of `node` against the
/// draft-atwood-jmap-chat-filenode-00 §Permission Mapping table: a node
/// created in a Space file tree needs `mayAddChildren` on its parent
/// ("upload new files and directories"), which `"send"` and above hold.
///
/// A Space root (a node with `spaceId`) is left to [`check_root_create`],
/// as is a node whose parent is not stored or not in a Space file tree. Without the right the create fails with
/// `forbidden`; return that SetError from `create_object` without
/// inserting.
///
/// # Errors
///
/// As for [`space_files_rights`], plus [`RootCheckError::Files`] if
/// reading the parent fails.
pub async fn check_space_create<C: ChatBackend, F: FileNodeBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    files: &F,
    files_caller: &F::CallerCtx,
    account_id: &Id,
    node: &FileNode,
) -> Result<Result<(), SetError>, RootCheckError<C::Error, F::Error>> {
    if node.space_id.is_some() {
        return Ok(Ok(()));
    }
    let Some(parent_id) = &node.parent_id else {
        return Ok(Ok(()));
    };
    let may_add = may_add_children(
        chat,
        chat_caller,
        files,
        files_caller,
        account_id,
        parent_id,
    )
    .await?;
    Ok(if may_add {
        Ok(())
    } else {
        Err(forbidden(
            "creating a node in this Space file tree directory requires mayAddChildren",
        ))
    })
}

/// Check a `FileNode/set` update of the stored `node` by `patch` against
/// the draft-atwood-jmap-chat-filenode-00 §Permission Mapping table.
///
/// For a node in a Space file tree, each patched property needs a right
/// from [`space_files_rights`], as draft-ietf-jmap-filenode `myRights`
/// defines them:
///
/// - `shareWith`: refused for every member, as "No role is granted
///   `mayShare`" and "the server rejects a `FileNode/set` update that
///   changes `shareWith` on a node in a Space file tree with a `forbidden`
///   SetError". A patch setting the whole of `shareWith` to its stored
///   value changes nothing and passes.
/// - `name` and `parentId`: `mayRename`, "rename or move this node". On
///   a Space root a changed `name` instead needs the `"manage_space"`
///   permission: "Creating, renaming, or destroying a Space root requires
///   `"manage_space"` permission, whatever the root's `myRights` report".
///   A changed `parentId` on a Space root is refused for every member:
///   "the server SHOULD reject a `FileNode/set` update that changes a
///   Space root's `parentId` with `forbidden`".
/// - `blobId`, `mediaType`, `target`, `modified`, `accessed` and
///   `executable`: `mayModifyContent`, "the content-related properties of
///   this node".
/// - any other property: `mayRead`, so a non-member changes nothing.
///
/// A `parentId` naming a new parent in a Space file tree also needs
/// `mayAddChildren` on that parent, as a node moved there is a new child
/// of it. Without a needed right the update fails with `forbidden`; return
/// that SetError from `update_object` without writing.
///
/// # Errors
///
/// As for [`space_files_rights`], plus [`RootCheckError::Files`] if
/// reading the new parent fails.
pub async fn check_space_update<C: ChatBackend, F: FileNodeBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    files: &F,
    files_caller: &F::CallerCtx,
    account_id: &Id,
    node: &FileNode,
    patch: &PatchObject,
) -> Result<Result<(), SetError>, RootCheckError<C::Error, F::Error>> {
    let rights =
        space_files_rights(chat, chat_caller, files, files_caller, account_id, node).await?;
    if let Some(rights) = rights {
        let share_with =
            serde_json::to_value(&node.share_with).expect("shareWith serializes to JSON");
        let parent_id = node.parent_id.as_ref().map(Id::as_ref);
        let renames_root = node.space_id.as_ref().filter(|_| {
            patch
                .as_map()
                .get("name")
                .is_some_and(|v| v.as_str() != Some(node.name.as_str()))
        });
        let manages_root = match renames_root {
            Some(space_id) => manages_space(chat, chat_caller, account_id, space_id)
                .await
                .map_err(RootCheckError::Chat)?
                .unwrap_or(false),
            None => false,
        };
        let is_root = node.space_id.is_some();
        for (key, value) in patch.as_map() {
            let allowed = match key.split('/').next().unwrap_or(key) {
                "shareWith" => key == "shareWith" && *value == share_with,
                "name" if is_root => value.as_str() == Some(node.name.as_str()) || manages_root,
                "parentId" if is_root => value.as_str() == parent_id,
                "name" | "parentId" => rights.may_rename,
                "blobId" | "mediaType" | "target" | "modified" | "accessed" | "executable" => {
                    rights.may_modify_content
                }
                _ => rights.may_read,
            };
            if !allowed {
                return Ok(Err(forbidden(
                    "the Space permissions held do not grant this change",
                )));
            }
        }
    }

    if let Some(new_parent) = patch.as_map().get("parentId").and_then(|v| v.as_str()) {
        let new_parent_id = match jmap_server::parse_wire_id(new_parent, "parentId") {
            Ok(id) => id,
            Err(err) => return Ok(Err(*err)),
        };
        if node.parent_id.as_ref().map(Id::as_ref) != Some(new_parent)
            && !may_add_children(
                chat,
                chat_caller,
                files,
                files_caller,
                account_id,
                &new_parent_id,
            )
            .await?
        {
            return Ok(Err(forbidden(
                "moving a node into this Space file tree directory requires mayAddChildren",
            )));
        }
    }
    Ok(Ok(()))
}

/// Check a `FileNode/set` destroy of the stored `node` against the
/// draft-atwood-jmap-chat-filenode-00 §Permission Mapping table: a node in
/// a Space file tree needs `mayDelete` from [`space_files_rights`], which
/// `"manage_channels"` and `"manage_space"` hold. A Space root instead
/// needs the `"manage_space"` permission, "whatever the root's `myRights`
/// report"; "Destroying a Space root follows {{JMAP-FILENODE}} for its
/// children; a Space may be left with no Space roots".
/// [`destroy_space_file_tree`] destroys in a server context and does not
/// go through this check.
///
/// This covers the `onExists` replacements of draft-ietf-jmap-filenode,
/// which "require `mayDelete` on the existing node, and fail with
/// "forbidden" without it", as `FileNode/set` destroys the replaced node
/// through `destroy_object`. Without the right the destroy fails with
/// `forbidden`; return that SetError from `destroy_object` without
/// removing the node.
///
/// # Errors
///
/// As for [`space_files_rights`].
pub async fn check_space_destroy<C: ChatBackend, F: FileNodeBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    files: &F,
    files_caller: &F::CallerCtx,
    account_id: &Id,
    node: &FileNode,
) -> Result<Result<(), SetError>, RootCheckError<C::Error, F::Error>> {
    if let Some(space_id) = &node.space_id {
        let manages = manages_space(chat, chat_caller, account_id, space_id)
            .await
            .map_err(RootCheckError::Chat)?;
        return Ok(if manages == Some(true) {
            Ok(())
        } else {
            Err(forbidden(
                "destroying a Space root requires the manage_space permission",
            ))
        });
    }
    let rights =
        space_files_rights(chat, chat_caller, files, files_caller, account_id, node).await?;
    Ok(match rights {
        Some(r) if !r.may_delete => Err(forbidden(
            "destroying this Space file tree node requires mayDelete",
        )),
        _ => Ok(()),
    })
}

/// Whether the owner of `account_id` may add children to the stored node
/// `parent_id`: `mayAddChildren` from [`space_files_rights`] on a
/// Directory, as only a node whose `@type` permits children can have
/// them, or `true` when the node is not stored or not in a Space file
/// tree.
async fn may_add_children<C: ChatBackend, F: FileNodeBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    files: &F,
    files_caller: &F::CallerCtx,
    account_id: &Id,
    parent_id: &Id,
) -> Result<bool, RootCheckError<C::Error, F::Error>> {
    let (parents, _) = files
        .get_objects::<FileNode>(
            files_caller,
            account_id,
            Some(std::slice::from_ref(parent_id)),
            None,
        )
        .await
        .map_err(|e| RootCheckError::Files(BackendQueryError::Other(e)))?;
    let Some(parent) = parents.iter().find(|n| &n.id == parent_id) else {
        return Ok(true);
    };
    let rights =
        space_files_rights(chat, chat_caller, files, files_caller, account_id, parent).await?;
    let is_directory = parent.node_type == Some(NodeType::Directory);
    Ok(rights.is_none_or(|r| r.may_add_children && is_directory))
}

/// The `spaceId` of the nearest Space root among `node` and its
/// `ancestors` (the unordered [`FileNodeBackend::get_ancestors`] result for
/// `node`), or `None` if neither `node` nor any ancestor is one.
///
/// draft-atwood-jmap-chat-filenode-00 §Terminology: a Space root is "A
/// Directory FileNode whose `spaceId` matches the Space's `id`", and the
/// Space file tree is "its Space roots and every FileNode below them".
fn space_of(node: &FileNode, ancestors: &[FileNode]) -> Option<Id> {
    let by_id: std::collections::HashMap<&Id, &FileNode> =
        ancestors.iter().map(|n| (&n.id, n)).collect();
    let mut current = Some(node);
    // Each step moves up one ancestor, so a cycle ends after all of them.
    for _ in 0..=ancestors.len() {
        let n = current?;
        if n.space_id.is_some() {
            return n.space_id.clone();
        }
        current = n.parent_id.as_ref().and_then(|p| by_id.get(p).copied());
    }
    None
}

/// Whether the owner of `account_id` holds `"manage_space"` in the Space
/// `space_id`, or `None` if the Space does not exist or they are not a
/// member. The permissions are those of [`member_permissions`].
async fn manages_space<C: ChatBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    account_id: &Id,
    space_id: &Id,
) -> Result<Option<bool>, C::Error> {
    let (spaces, _) = chat
        .get_objects::<Space>(
            chat_caller,
            account_id,
            Some(std::slice::from_ref(space_id)),
            None,
        )
        .await?;
    let space = spaces.iter().find(|s| &s.id == space_id);
    Ok(member_permissions(chat, chat_caller, account_id, space)
        .await?
        .map(|p| p.contains(&ChatPermission::ManageSpace)))
}

/// The rows of draft-atwood-jmap-chat-filenode-00 §Permission Mapping,
/// lowest first: "Each row includes the grants of the rows above it".
const TIERS: [ChatPermission; 4] = [
    ChatPermission::View,
    ChatPermission::Send,
    ChatPermission::ManageChannels,
    ChatPermission::ManageSpace,
];

/// The Space permissions `account_id` holds as a member of `space`: the
/// union over the member's SpaceRoles plus the implicit `@everyone`
/// role's permissions from [`ChatBackend::everyone_permissions`]
/// (draft-atwood-jmap-chat-00 §Space Permission Resolution step 1).
/// `None` if `space` is `None` or `account_id` is not a member, without
/// asking the backend. ChannelPermission overrides are not applied: they
/// "do not restrict file-tree access" (draft-atwood-jmap-chat-filenode-00
/// §Permission Mapping). Permissions compare by variant, so a
/// `ChatPermission::Other` spelling of a known name grants nothing, as in
/// `resolve_channel_permissions`.
async fn member_permissions<C: ChatBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    account_id: &Id,
    space: Option<&Space>,
) -> Result<Option<HashSet<ChatPermission>>, C::Error> {
    let Some((space, member)) =
        space.and_then(|s| Some((s, s.members.iter().find(|m| &m.id == account_id)?)))
    else {
        return Ok(None);
    };
    let role_ids: HashSet<&Id> = member.role_ids.iter().collect();
    let mut permissions = chat
        .everyone_permissions(chat_caller, account_id, space)
        .await?;
    permissions.extend(
        space
            .roles
            .iter()
            .filter(|r| role_ids.contains(&r.id))
            .flat_map(|r| r.permissions.iter().cloned()),
    );
    Ok(Some(permissions))
}

/// A `forbidden` SetError with `description`.
fn forbidden(description: &str) -> SetError {
    SetError::new(SetErrorType::Forbidden).with_description(description)
}
