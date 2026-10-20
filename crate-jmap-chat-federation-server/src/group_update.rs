//! `Peer/groupUpdate` (draft-atwood-jmap-chat-federation-00
//! §Peer/groupUpdate): a new group Chat, or a change to the members,
//! roles or metadata of one.

use std::collections::HashSet;

use jmap_chat_federation_types::{GroupUpdateAction, PeerAck, PeerChatMember, PeerGroupUpdateArgs};
use jmap_chat_types::{Chat, ChatKind, ChatMember, ChatMemberRole};
use jmap_server::{now_utc_string, serialize_value, HandlerOutput};
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{json, Map, Value};

use crate::deliver::{authenticate, check_account, get_chat, set_failure, Commit, Identities, Who};
use crate::FederationBackend;

/// Handle a `Peer/groupUpdate` method call.
///
/// - `senderUserId` must be the peer identity, else `forbidden`.
/// - `"create"`: `members` is required and must list `senderUserId`,
///   else `forbidden`. The group Chat is stored under `chatId` through
///   [`FederationBackend::create_chat`], with each member's `id` and
///   `invitedBy` resolved per §User References in Peer Methods
///   (placeholder ChatContacts for unknown identities), and `name`,
///   `description` and `avatarBlobId` as sent. A `chatId` already in use
///   is `forbidden`, except by a group Chat the sender is a member of,
///   which answers a retried create without writing.
/// - Every other action needs a stored group Chat with this id, else
///   `forbidden`. "Once the local Chat record has `closedAt` set, the
///   receiving server MUST reject every subsequent `Peer/groupUpdate` for
///   that chat with `forbidden`." The sender must be a member, and for
///   every action but `"leave"` an admin, else `forbidden`.
/// - `"addMembers"` adds the `addedMembers` not yet members, resolved as
///   for `"create"`. `"removeMembers"` and `"updateRoles"` act on the
///   named identities that are members and skip the others; an identity
///   with no local user is no member, so they create no placeholders.
///   `"updateMetadata"` sets the `name`, `description` and `avatarBlobId`
///   present. `"leave"` removes the sender. `"close"` removes the sender
///   and sets `closedAt`.
/// - Every rejection, including a missing required array or an empty
///   identity string, happens before the first write.
///
/// `avatarBlobId` names a blob on the sending server; it is stored as
/// sent, and fetching it ("the receiver MAY fetch this blob") is left to
/// the deployment.
///
/// # Errors
///
/// `invalidArguments`, `forbidden`, `rateLimited` (a placeholder limit)
/// and `serverFail`; `accountNotFound` for an unknown `accountId`.
pub async fn handle_peer_group_update<B: FederationBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args: PeerGroupUpdateArgs = serde_json::from_value(args)
        .map_err(|e| JmapError::invalid_arguments(format!("Peer/groupUpdate arguments: {e}")))?;
    authenticate(backend, caller, &args.sender_user_id)?;
    let account_id = &args.account_id;
    check_account(backend, caller, account_id).await?;

    let now = now_utc_string();
    let identities = Identities::load(backend, caller, account_id).await?;
    let stored = get_chat(backend, caller, account_id, &args.chat_id).await?;
    let sender_id = match identities.lookup(&args.sender_user_id) {
        Who::Contact(c) => Some(c.id.clone()),
        Who::Owner | Who::Unknown => None,
    };
    let mut ctx = Commit::new(backend, caller, account_id, &now, identities);
    let ack = HandlerOutput::new(serialize_value(PeerAck::new(account_id.clone()))?);

    if args.action == GroupUpdateAction::Create {
        let members = required(args.members.as_deref(), "members")?;
        check_members(members)?;
        if !members.iter().any(|m| m.id == args.sender_user_id) {
            return Err(JmapError::forbidden());
        }
        if let Some(chat) = stored {
            return if chat.kind == ChatKind::Group && is_member(&chat, sender_id.as_ref()) {
                Ok(ack)
            } else {
                Err(JmapError::forbidden())
            };
        }
        let mut chat = Chat::new(
            args.chat_id.clone(),
            ChatKind::Group,
            now.clone(),
            0,
            Vec::new(),
            false,
            true,
        );
        chat.members = Some(add_members(&mut ctx, Vec::new(), members).await?);
        chat.name = args.name.clone();
        chat.description = args.description.clone();
        chat.avatar_blob_id = args.avatar_blob_id.clone();
        backend
            .create_chat(caller, account_id, chat)
            .await
            .map_err(set_failure)?;
        return Ok(ack);
    }

    let chat = stored
        .filter(|c| c.kind == ChatKind::Group && c.closed_at.is_none())
        .ok_or_else(JmapError::forbidden)?;
    let mut members = chat.members.clone().unwrap_or_default();
    let sender = sender_id
        .as_ref()
        .and_then(|id| members.iter().find(|m| &m.id == id))
        .ok_or_else(JmapError::forbidden)?;
    if args.action != GroupUpdateAction::Leave && sender.role != ChatMemberRole::Admin {
        return Err(JmapError::forbidden());
    }
    let sender = sender.id.clone();

    let mut patch = Map::new();
    match args.action {
        GroupUpdateAction::AddMembers => {
            let added = required(args.added_members.as_deref(), "addedMembers")?;
            check_members(added)?;
            members = add_members(&mut ctx, members, added).await?;
        }
        GroupUpdateAction::RemoveMembers => {
            let removed = required(args.removed_member_ids.as_deref(), "removedMemberIds")?;
            let removed: HashSet<Id> = removed.iter().filter_map(|i| known(&ctx, i)).collect();
            members.retain(|m| !removed.contains(&m.id));
        }
        GroupUpdateAction::UpdateRoles => {
            for update in required(args.updated_roles.as_deref(), "updatedRoles")? {
                let id = known(&ctx, &update.id);
                if let Some(m) = members.iter_mut().find(|m| Some(&m.id) == id.as_ref()) {
                    m.role = update.role.clone();
                }
            }
        }
        GroupUpdateAction::UpdateMetadata => {
            if let Some(name) = &args.name {
                patch.insert("name".to_owned(), json!(name));
            }
            if let Some(description) = &args.description {
                patch.insert("description".to_owned(), json!(description));
            }
            if let Some(avatar) = &args.avatar_blob_id {
                patch.insert("avatarBlobId".to_owned(), json!(avatar));
            }
        }
        GroupUpdateAction::Leave => members.retain(|m| m.id != sender),
        GroupUpdateAction::Close => {
            members.retain(|m| m.id != sender);
            patch.insert("closedAt".to_owned(), json!(now));
        }
        _ => return Err(JmapError::invalid_arguments("unknown action")),
    }
    if Some(&members) != chat.members.as_ref() {
        patch.insert("members".to_owned(), serialize_value(&members)?);
    }
    if !patch.is_empty() {
        backend
            .update_object::<Chat>(caller, account_id, &chat.id, PatchObject::from_map(patch))
            .await
            .map_err(set_failure)?;
    }
    Ok(ack)
}

/// The array an action requires, or `invalidArguments` naming it.
fn required<'a, T>(value: Option<&'a [T]>, name: &str) -> Result<&'a [T], JmapError> {
    value.ok_or_else(|| JmapError::invalid_arguments(format!("{name} is required")))
}

/// Each member `id` and `invitedBy` must be usable as a placeholder's
/// `identity` and `login`, which draft-atwood-jmap-chat-00 §ChatContact
/// requires to hold "at least one non-whitespace character".
fn check_members(members: &[PeerChatMember]) -> Result<(), JmapError> {
    let blank = |s: &str| s.trim().is_empty();
    if members
        .iter()
        .any(|m| blank(&m.id) || m.invited_by.as_deref().is_some_and(blank))
    {
        return Err(JmapError::invalid_arguments(
            "a member id or invitedBy is not an identity string",
        ));
    }
    Ok(())
}

/// Whether the ChatContact `id` is a member of `chat`.
fn is_member(chat: &Chat, id: Option<&Id>) -> bool {
    id.is_some_and(|id| chat.members.iter().flatten().any(|m| &m.id == id))
}

/// The local id of an identity that already names a local user.
fn known<B: FederationBackend>(ctx: &Commit<'_, B>, identity: &str) -> Option<Id> {
    match ctx.identities.lookup(identity) {
        Who::Owner => Some(ctx.account_id.clone()),
        Who::Contact(c) => Some(c.id.clone()),
        Who::Unknown => None,
    }
}

/// `members` with each of `added` not yet a member appended, its `id` and
/// `invitedBy` resolved to local ids.
async fn add_members<B: FederationBackend>(
    ctx: &mut Commit<'_, B>,
    mut members: Vec<ChatMember>,
    added: &[PeerChatMember],
) -> Result<Vec<ChatMember>, JmapError> {
    for m in added {
        let id = ctx.local_id(&m.id).await?;
        if members.iter().any(|x| x.id == id) {
            continue;
        }
        let mut member = ChatMember::new(id, m.role.clone(), m.joined_at.clone());
        member.invited_by = match &m.invited_by {
            Some(identity) => Some(ctx.local_id(identity).await?),
            None => None,
        };
        member.extra = m.extra.clone();
        members.push(member);
    }
    Ok(members)
}
