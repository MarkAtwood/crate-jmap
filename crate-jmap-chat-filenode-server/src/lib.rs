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
use jmap_server::{BackendQueryError, SetError, SetErrorType};
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

/// A `forbidden` SetError with `description`.
fn forbidden(description: &str) -> SetError {
    SetError::new(SetErrorType::Forbidden).with_description(description)
}
