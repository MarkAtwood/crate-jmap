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
//! | `FileNodeBackend::create_object::<FileNode>`, before the insert | [`check_root_create`] |
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

use jmap_chat_server::{ChatBackend, MANAGE_CHANNELS, MANAGE_SPACE, SEND, VIEW};
use jmap_chat_types::{Attachment, AttachmentCreate, Chat, ChatKind, Space};
use jmap_filenode_server::FileNodeBackend;
use jmap_filenode_types::{FileNode, FileNodeFilterCondition, NodeRole};
use jmap_server::{BackendQueryError, BackendSetError, SetError, SetErrorType};
use jmap_types::Id;

/// A storage failure while checking a FileNode create or a `filenodeId`
/// Attachment.
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

/// Check draft-atwood-jmap-chat-filenode-00 §Creating the File Tree Root
/// for a `FileNode/set` create of `node`.
///
/// A node whose `role` is not `chatspace` passes
/// unchecked, so this is safe to call before every FileNode insert. For a
/// Space file tree root, in this order:
///
/// - `spaceId` must be present: the client "MUST supply ... `spaceId`".
///   Without it the create fails with `invalidProperties` naming
///   `spaceId`. (`@type: "Directory"` is already enforced by
///   `jmap-filenode-server`, which allows `role` on directories only.)
/// - "The Space identified by `spaceId` exists and the requesting user is
///   a member. If not, the server MUST return `forbidden`." Both cases
///   give the same error, as the draft recommends "to avoid revealing
///   Space existence to non-members".
/// - "No file tree root already exists for that Space ... If one exists,
///   the server MUST return an `alreadyExists` SetError ... with
///   `existingId` set to the id of the existing root." The root is found
///   with the `role` and `spaceId` `FileNode/query` filter the draft
///   requires servers to support.
/// - Creating the root "requires `"manage_space"` permission"
///   (§Permission Mapping); without it the create fails with `forbidden`.
///   The permissions are the union over the member's SpaceRoles plus
///   `everyone`, the implicit `@everyone` role's permissions, which the
///   draft leaves to the deployment (draft-atwood-jmap-chat-00 §Space
///   Permission Resolution step 1). ChannelPermission overrides "do not
///   apply to file tree access".
///
/// The requesting user is the owner of `account_id`, named by
/// `account_id` in Space member lists (draft-atwood-jmap-chat-00
/// §Terminology). `account_id` is the account of both the Space and the
/// FileNode.
///
/// Returns `Ok(Err(set_error))` when the create must be refused. Return
/// that SetError from `create_object` without inserting: "On any of these
/// failures, the server MUST NOT create the FileNode."
///
/// # Errors
///
/// Returns [`RootCheckError::Chat`] if reading the Space fails, and
/// [`RootCheckError::Files`] if the root query fails.
pub async fn check_root_create<C: ChatBackend, F: FileNodeBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    files: &F,
    files_caller: &F::CallerCtx,
    account_id: &Id,
    node: &FileNode,
    everyone: &HashSet<String>,
) -> Result<Result<(), SetError>, RootCheckError<C::Error, F::Error>> {
    if node.role != Some(NodeRole::ChatSpace) {
        return Ok(Ok(()));
    }
    let Some(space_id) = &node.space_id else {
        return Ok(Err(SetError::new(SetErrorType::InvalidProperties)
            .with_properties(["spaceId"])
            .with_description("a Space file tree root requires spaceId")));
    };

    let (spaces, _) = chat
        .get_objects::<Space>(
            chat_caller,
            account_id,
            Some(std::slice::from_ref(space_id)),
            None,
        )
        .await
        .map_err(RootCheckError::Chat)?;
    let Some(permissions) = spaces
        .iter()
        .find(|s| &s.id == space_id)
        .and_then(|s| member_permissions(s, account_id, everyone))
    else {
        return Ok(Err(forbidden(
            "spaceId does not name a Space the caller is a member of",
        )));
    };

    let mut filter = FileNodeFilterCondition::default();
    filter.role = Some(NodeRole::ChatSpace.to_wire_str().to_owned());
    filter.space_id = Some(space_id.clone());
    let existing = files
        .query_objects::<FileNode>(
            files_caller,
            account_id,
            Some(&filter),
            None,
            Some(1),
            0,
            None,
            0,
        )
        .await
        .map_err(RootCheckError::Files)?;
    if let Some(existing_id) = existing.ids.into_iter().next() {
        return Ok(Err(SetError::new(SetErrorType::AlreadyExists)
            .with_existing_id(existing_id)
            .with_description("the Space already has a file tree root")));
    }

    Ok(if permissions.contains(MANAGE_SPACE) {
        Ok(())
    } else {
        Err(forbidden(
            "creating a Space file tree root requires the manage_space permission",
        ))
    })
}

/// Check draft-atwood-jmap-chat-filenode-00 `maxSpaceStorageBytes` for a
/// `FileNode/set` create of `node`: "When a `FileNode/set` create would
/// cause the Space file tree to exceed this limit, the server MUST return
/// an `overQuota` SetError".
///
/// The limit covers the "total blob storage in bytes across all FileNodes
/// in a single Space's file tree": the `size` of every node under the
/// tree root plus `node.size`. Call it with `node` as the backend will
/// store it, after setting the server-set `size` from the blob. A node
/// whose top-level ancestor is not a Space file tree root (role
/// `chatspace`) passes unchecked, as does a top-level
/// node.
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
    let (mut chain, _) = files
        .get_objects::<FileNode>(caller, account_id, Some(parent_ids), None)
        .await
        .map_err(BackendQueryError::Other)?;
    chain.extend(
        files
            .get_ancestors(caller, account_id, parent_ids)
            .await
            .map_err(BackendQueryError::Other)?,
    );
    let Some(root) = chain
        .into_iter()
        .find(|n| n.parent_id.is_none() && n.role == Some(NodeRole::ChatSpace))
    else {
        return Ok(Ok(()));
    };

    // ponytail: reads the whole tree on every create, O(tree size). Upgrade
    // path: a per-Space usage total the backend keeps with each write.
    let ids = files
        .query_subtree(caller, account_id, std::slice::from_ref(&root.id), u64::MAX)
        .await?;
    let (tree, _) = files
        .get_objects::<FileNode>(caller, account_id, Some(&ids), None)
        .await
        .map_err(BackendQueryError::Other)?;
    let total = tree
        .iter()
        .chain([&root, node])
        .fold(0u64, |sum, n| sum.saturating_add(n.size.unwrap_or(0)));
    Ok(if total > max_space_storage_bytes {
        Err(SetError::new(SetErrorType::OverQuota)
            .with_description("the create would exceed maxSpaceStorageBytes for the Space"))
    } else {
        Ok(())
    })
}

/// Create the file tree root for a new Space, for a server advertising
/// draft-atwood-jmap-chat-filenode-00 `autoCreateFileTree: true`: "the
/// server automatically creates a Space file tree root for each new Space
/// at Space creation time and includes the new FileNode in subsequent
/// `FileNode/changes` responses."
///
/// The root is a top-level `Directory` with role
/// `chatspace` and `spaceId` set to `space_id`, named
/// `space_id` since the draft leaves the name open and at most one root
/// exists per Space. It is inserted with [`FileNodeBackend::create_object`],
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
#[allow(clippy::result_large_err)]
pub async fn create_space_file_tree<F: FileNodeBackend>(
    files: &F,
    caller: &F::CallerCtx,
    account_id: &Id,
    space_id: &Id,
) -> Result<(Id, FileNode), BackendSetError<F::Error>> {
    let root: FileNode = serde_json::from_value(serde_json::json!({
        "id": space_id,
        "@type": "Directory",
        "name": space_id,
        "parentId": null,
        "role": NodeRole::ChatSpace.to_wire_str(),
        "spaceId": space_id,
    }))
    .expect("a Directory FileNode with every required field deserializes");
    files
        .create_object(caller, account_id, "spaceFileTree", root)
        .await
}

/// A storage failure while destroying a Space file tree.
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

/// Destroy the file tree of a destroyed Space, per
/// draft-atwood-jmap-chat-filenode-00 §Space Deletion: "When a Space is
/// destroyed via `Space/set` destroy, the server SHOULD also destroy the
/// associated file tree and all FileNodes within it." Call it from
/// `ChatBackend::on_space_destroyed`.
///
/// No grace period is offered (the draft's "MAY instead retain"), so the
/// tree goes with the Space and no former member can reach it afterwards.
/// The root is found with the `role` and `spaceId` `FileNode/query`
/// filter; each descendant is destroyed before the root, as `FileNode/set`
/// does for `onDestroyRemoveChildren`. Every node goes through
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
#[allow(clippy::result_large_err)]
pub async fn destroy_space_file_tree<F: FileNodeBackend>(
    files: &F,
    caller: &F::CallerCtx,
    account_id: &Id,
    space_id: &Id,
) -> Result<Vec<Id>, TreeDestroyError<F::Error>> {
    let mut filter = FileNodeFilterCondition::default();
    filter.role = Some(NodeRole::ChatSpace.to_wire_str().to_owned());
    filter.space_id = Some(space_id.clone());
    let roots = files
        .query_objects::<FileNode>(caller, account_id, Some(&filter), None, None, 0, None, 0)
        .await
        .map_err(TreeDestroyError::Query)?
        .ids;

    let mut destroyed = Vec::new();
    for root_id in roots {
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
///   The node must have an ancestor that is the Space file tree root
///   (role `chatspace`, `spaceId` the Chat's
///   `spaceId`); §Attachment filenodeId Validation: "Servers SHOULD NOT
///   permit a `filenodeId` referencing a FileNode from a different Space
///   or from outside any Space file tree". A message to a Chat that is
///   not a channel has no Space, so it fails the same way. A node without
///   a blob, such as a Directory, fails too, as "the server uses that
///   FileNode's blob as the attachment blob".
/// - "The sender has at least read access to the FileNode under the
///   mapping in {{permissions}}": every row of that table grants Read, so
///   the sender must be a Space member holding `view`, `send`,
///   `manage_channels` or `manage_space` through a SpaceRole or
///   `everyone`, the implicit `@everyone` role's permissions. Otherwise
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
/// fails, and [`RootCheckError::Chat`] if reading the Space fails.
#[allow(clippy::too_many_arguments)]
pub async fn resolve_filenode_attachment<C: ChatBackend, F: FileNodeBackend>(
    chat: &C,
    chat_caller: &C::CallerCtx,
    files: &F,
    files_caller: &F::CallerCtx,
    account_id: &Id,
    message_chat: &Chat,
    attachment: &AttachmentCreate,
    everyone: &HashSet<String>,
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
    if !ancestors.iter().any(|n| {
        n.parent_id.is_none()
            && n.role == Some(NodeRole::ChatSpace)
            && n.space_id.as_ref() == Some(space_id)
    }) {
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
    let may_read = spaces
        .iter()
        .find(|s| &s.id == space_id)
        .and_then(|s| member_permissions(s, account_id, everyone))
        .is_some_and(|p| {
            [VIEW, SEND, MANAGE_CHANNELS, MANAGE_SPACE]
                .iter()
                .any(|r| p.contains(r))
        });
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
            .is_some_and(|h| node.sha256.as_ref().map(|d| d.as_str()) != Some(h.as_str()));
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

/// The Space permissions `account_id` holds as a member of `space`: the
/// union over the member's SpaceRoles plus `everyone`, the implicit
/// `@everyone` role's permissions (draft-atwood-jmap-chat-00 §Space
/// Permission Resolution step 1). `None` if `account_id` is not a member.
/// ChannelPermission overrides are not applied: they "do not restrict
/// file-tree access" (draft-atwood-jmap-chat-filenode-00 §Permission
/// Mapping).
fn member_permissions<'a>(
    space: &'a Space,
    account_id: &Id,
    everyone: &'a HashSet<String>,
) -> Option<HashSet<&'a str>> {
    let member = space.members.iter().find(|m| &m.id == account_id)?;
    let role_ids: HashSet<&Id> = member.role_ids.iter().collect();
    Some(
        space
            .roles
            .iter()
            .filter(|r| role_ids.contains(&r.id))
            .flat_map(|r| r.permissions.iter().map(|p| p.as_str()))
            .chain(everyone.iter().map(String::as_str))
            .collect(),
    )
}

/// A `forbidden` SetError with `description`.
fn forbidden(description: &str) -> SetError {
    SetError::new(SetErrorType::Forbidden).with_description(description)
}
