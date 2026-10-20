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
//!
//! Each function takes the backends it reads, with their own caller
//! contexts and error types.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::fmt;

use jmap_chat_server::{ChatBackend, MANAGE_SPACE};
use jmap_chat_types::Space;
use jmap_filenode_server::FileNodeBackend;
use jmap_filenode_types::{FileNode, FileNodeFilterCondition, NodeRole};
use jmap_server::{BackendQueryError, BackendSetError, SetError, SetErrorType};
use jmap_types::Id;

/// A storage failure while checking a FileNode create.
#[derive(Debug)]
pub enum RootCheckError<C, F> {
    /// The Chat backend failed to read the Space.
    Chat(C),
    /// The FileNode backend failed the `role` and `spaceId` query.
    Files(BackendQueryError<F>),
}

impl<C, F> fmt::Display for RootCheckError<C, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Chat(_) => "reading the Space failed",
            Self::Files(_) => "querying the Space file tree root failed",
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
/// A node whose `role` is not `urn:jmap:chat:filenode:space` passes
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
    let Some(member) = spaces
        .iter()
        .find(|s| &s.id == space_id)
        .and_then(|s| Some((s, s.members.iter().find(|m| &m.id == account_id)?)))
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

    let (space, member) = member;
    let role_ids: HashSet<&Id> = member.role_ids.iter().collect();
    let may_manage = everyone.contains(MANAGE_SPACE)
        || space
            .roles
            .iter()
            .filter(|r| role_ids.contains(&r.id))
            .any(|r| r.permissions.iter().any(|p| p == MANAGE_SPACE));
    Ok(if may_manage {
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
/// `urn:jmap:chat:filenode:space`) passes unchecked, as does a top-level
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
/// `urn:jmap:chat:filenode:space` and `spaceId` set to `space_id`, named
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

/// A `forbidden` SetError with `description`.
fn forbidden(description: &str) -> SetError {
    SetError::new(SetErrorType::Forbidden).with_description(description)
}
