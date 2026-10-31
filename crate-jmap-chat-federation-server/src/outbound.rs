//! The sending side of draft-atwood-jmap-chat-federation-00: building
//! `Peer/*` arguments from the owner's stored records and the
//! `jmap_chat_server::OutboundEvent`s, aggregating `deliveryState`, and
//! telling a transient delivery failure from a permanent one.
//!
//! Everything here is pure except [`outbound_reference`], which reads one
//! Message through the backend. Queueing, discovery of the peer's
//! `accountId` and the HTTP call stay with the deployment.

use std::fmt;

use jmap_chat_federation_types::{
    GroupUpdateAction as PeerAction, PeerAvatar, PeerChatMember, PeerGroupUpdateArgs,
    PeerRoleUpdate, contact_server_url,
};
use jmap_chat_server::{ChatBackend, GroupUpdateAction};
use jmap_chat_types::CHAT_CAP_TASK;
use jmap_chat_types::{
    Chat, ChatContact, ChatKind, ChatMember, Clearable, DeliveryState, Message, MessageAction,
    Space,
};
use jmap_peer_auth::{IdentityHost, PeerDomain, identity_host};
use jmap_types::Id;
use serde_json::Value;

/// The `senderMsgId` to send for a locally stored `replyTo` or
/// `threadRootId` (§Outbound Message Preparation).
///
/// Reads the Message `local_id` names and returns its stored
/// `senderMsgId`: for a Message the owner composed that equals its `id`,
/// for one received from a peer it is the id the peer assigned. Returns
/// `None`, so the field is omitted from the payload, when `local_id` is
/// `None` or names no stored Message; the local id itself is never sent.
///
/// # Errors
///
/// The backend's error when the read fails.
pub async fn outbound_reference<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    local_id: Option<&Id>,
) -> Result<Option<Id>, B::Error> {
    let Some(local_id) = local_id else {
        return Ok(None);
    };
    let (found, _) = backend
        .get_objects::<Message>(
            caller,
            account_id,
            Some(std::slice::from_ref(local_id)),
            None,
        )
        .await?;
    Ok(found
        .into_iter()
        .find(|m| m.id == *local_id)
        .map(|m| m.sender_msg_id))
}

/// The ChatContact ids of every participant of `chat` other than the
/// owner `account_id`: the recipients of `Peer/deliver` and of
/// `Peer/retract` (§Peer/retract "Sender Behavior": "the server of every
/// other participant in the Chat").
///
/// A direct Chat's participant is its `contactId`, a group Chat's are its
/// `members`, and a channel's are the `members` of its Space, passed as
/// `space` (draft-atwood-jmap-chat-00 §SpaceMember). A channel with no
/// `space`, or a Chat kind this crate does not know, has none.
pub fn other_participants(chat: &Chat, space: Option<&Space>, account_id: &Id) -> Vec<Id> {
    let ids: Vec<&Id> = match chat.kind {
        ChatKind::Direct => chat.contact_id.iter().collect(),
        ChatKind::Channel => space
            .map(|s| s.members.iter().map(|m| &m.id).collect())
            .unwrap_or_default(),
        ChatKind::Group => chat.members.iter().flatten().map(|m| &m.id).collect(),
        _ => Vec::new(),
    };
    ids.into_iter()
        .filter(|id| *id != account_id)
        .cloned()
        .collect()
}

/// The owner's local ids and the identity strings they map to
/// (§User References in Peer Methods), for building outbound arguments.
#[derive(Debug, Clone, Copy)]
pub struct SenderIdentities<'a> {
    account_id: &'a Id,
    owner_user_id: &'a str,
    contacts: &'a [ChatContact],
}

impl<'a> SenderIdentities<'a> {
    /// The owner of `account_id` is `owner_user_id` (the Session's
    /// `ownerUserId`); every other user is one of `contacts`.
    pub fn new(account_id: &'a Id, owner_user_id: &'a str, contacts: &'a [ChatContact]) -> Self {
        Self {
            account_id,
            owner_user_id,
            contacts,
        }
    }

    /// The identity string for a local user id: the accountId maps to
    /// `ownerUserId`, a ChatContact id to that contact's `identity`.
    ///
    /// # Errors
    ///
    /// [`OutboundError::UnknownUser`] when `id` names neither.
    pub fn identity(&self, id: &Id) -> Result<String, OutboundError> {
        if id == self.account_id {
            return Ok(self.owner_user_id.to_owned());
        }
        self.contacts
            .iter()
            .find(|c| c.id == *id)
            .map(|c| c.identity.clone())
            .ok_or_else(|| OutboundError::UnknownUser(id.clone()))
    }

    /// `actions` as sent in a `Peer/deliver` message: each `assigneeIds`
    /// entry of a `urn:ietf:params:jmap:chat:cap:task` action becomes the identity
    /// string of the local user it names (draft-atwood-jmap-chat-tasks-00
    /// §Federation: "the sending server maps each local id to the user's
    /// identity"). Every other action, key and value is unchanged.
    ///
    /// # Errors
    ///
    /// [`OutboundError::UnknownUser`] when an entry names neither the
    /// owner nor a stored ChatContact, or is not a string.
    pub fn actions(&self, actions: &[MessageAction]) -> Result<Vec<MessageAction>, OutboundError> {
        let mut out = actions.to_vec();
        for action in &mut out {
            for entry in task_assignee_ids(action).into_iter().flatten() {
                let local = entry
                    .as_str()
                    .map(Id::from)
                    .ok_or_else(|| OutboundError::UnknownUser(Id::from(entry.to_string())))?;
                *entry = Value::String(self.identity(&local)?);
            }
        }
        Ok(out)
    }

    fn member(&self, m: &ChatMember) -> Result<PeerChatMember, OutboundError> {
        let mut out =
            PeerChatMember::new(self.identity(&m.id)?, m.role.clone(), m.joined_at.clone());
        out.invited_by = m
            .invited_by
            .as_ref()
            .map(|i| self.identity(i))
            .transpose()?;
        Ok(out)
    }

    fn members(&self, list: &[ChatMember]) -> Result<Vec<PeerChatMember>, OutboundError> {
        list.iter().map(|m| self.member(m)).collect()
    }
}

/// The `assigneeIds` array of a `urn:ietf:params:jmap:chat:cap:task` MessageAction's
/// `metadata`, the user-reference field inside MessageAction metadata
/// that draft-atwood-jmap-chat-federation-00 §User References in Peer
/// Methods names. `None` for another action type, or when the key is
/// absent or not an array.
pub(crate) fn task_assignee_ids(action: &mut MessageAction) -> Option<&mut Vec<Value>> {
    if action.action_type != CHAT_CAP_TASK {
        return None;
    }
    action
        .metadata
        .as_mut()?
        .get_mut("assigneeIds")?
        .as_array_mut()
}

/// Why outbound arguments could not be built.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundError {
    /// A user id names neither the owner nor a stored ChatContact.
    UnknownUser(Id),
    /// A `GroupUpdateAction` this crate does not know how to send.
    UnknownAction,
    /// The group has an `avatarBlobId` but no `avatar` object was given
    /// for it.
    MissingAvatar,
    /// The ChatContact's `identity` has a host that is not a domain name
    /// (an IP literal, a host with a port), so no peer domain hosts it.
    InvalidHost(Id),
    /// The ChatContact's `identity` has no host and the ChatContact has no
    /// `serverUrl`, so there is nowhere to send its calls.
    NoServerUrl(Id),
}

impl fmt::Display for OutboundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownUser(id) => write!(f, "no identity string for user id {id}"),
            Self::UnknownAction => f.write_str("unknown group update action"),
            Self::MissingAvatar => f.write_str("no avatar object for the group avatarBlobId"),
            Self::InvalidHost(id) => write!(f, "identity of contact {id} has no peer domain host"),
            Self::NoServerUrl(id) => {
                write!(f, "identity of contact {id} has no host and no serverUrl")
            }
        }
    }
}

impl std::error::Error for OutboundError {}

/// Where the calls of one [`PeerBatch`] go.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerDestination {
    /// The peer domain that is the host of each recipient's identity.
    Domain(PeerDomain),
    /// The `serverUrl` of each recipient's ChatContact, for recipients
    /// whose identity has no host.
    ServerUrl(String),
}

/// The calls for one destination: one signed request, one method call
/// per recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerBatch {
    /// Where the request goes.
    pub destination: PeerDestination,
    /// Each recipient's ChatContact id and its identity, the call's
    /// `recipientUserId`, in the order they were passed.
    pub recipients: Vec<(Id, String)>,
}

/// Groups the ChatContacts `recipients` into one [`PeerBatch`] per
/// destination, in the order each destination first appears.
///
/// "The sending server sends the call to the peer domain that is the host
/// of `recipientUserId`, as {{identity-binding}} defines the host of an
/// identity; how it finds the server for an identity that has no host is
/// not defined by this document" (§Server-to-Server Methods, fed b1cd625
/// L404). Hosts are grouped as §Identity Binding compares them (L335), so
/// `bob@B.Example` and `carol@b.example` share a batch. For an identity
/// with no host, such as a `did:key` DID, this crate's choice is the
/// ChatContact's `serverUrl`. A sending server with calls for several
/// mailboxes on one peer domain "MAY send them in one request, one method
/// call per recipient" (L406); a deployment that sends each call alone
/// iterates the recipients of each batch.
///
/// # Errors
///
/// [`OutboundError::UnknownUser`] when a recipient is not one of
/// `contacts` (the owner is never a recipient),
/// [`OutboundError::InvalidHost`] when its identity's host is not a
/// domain name, and [`OutboundError::NoServerUrl`] when its identity has
/// no host and it has no `serverUrl`.
pub fn peer_batches(
    contacts: &[ChatContact],
    recipients: &[Id],
) -> Result<Vec<PeerBatch>, OutboundError> {
    let mut batches: Vec<PeerBatch> = Vec::new();
    for id in recipients {
        let contact = contacts
            .iter()
            .find(|c| c.id == *id)
            .ok_or_else(|| OutboundError::UnknownUser(id.clone()))?;
        let destination = match identity_host(&contact.identity) {
            IdentityHost::Domain(domain) => PeerDestination::Domain(domain),
            IdentityHost::HostlessDid | IdentityHost::NoHost => contact_server_url(&contact.extra)
                .map(|url| PeerDestination::ServerUrl(url.to_owned()))
                .ok_or_else(|| OutboundError::NoServerUrl(id.clone()))?,
            _ => return Err(OutboundError::InvalidHost(id.clone())),
        };
        let entry = (id.clone(), contact.identity.clone());
        // ponytail: linear scans, O(recipients x (contacts + batches));
        // index contacts and batches in maps if groups grow to thousands.
        match batches.iter_mut().find(|b| b.destination == destination) {
            Some(batch) => batch.recipients.push(entry),
            None => batches.push(PeerBatch {
                destination,
                recipients: vec![entry],
            }),
        }
    }
    Ok(batches)
}

/// The `Peer/groupUpdate` arguments for one
/// `OutboundEvent::GroupUpdate` action on the stored group `chat`, sent
/// to the receiving server's `peer_account_id` (§Peer/groupUpdate), for
/// the mailbox of the member whose identity string is `recipient_user_id`
/// (`recipientUserId`, §Server-to-Server Methods).
///
/// `senderUserId` is the owner's `ownerUserId`, and every ChatMember
/// `id` and `invitedBy` becomes an identity string. `"create"` carries
/// the full `members` list with `name`, `description` and `avatar`;
/// `"updateMetadata"` carries those three; `"addMembers"`,
/// `"removeMembers"` and `"updateRoles"` carry `addedMembers`,
/// `removedMemberIds` and `updatedRoles`; `"leave"` and `"close"` carry
/// nothing more.
///
/// `avatar` is the `avatar` object for the chat's `avatarBlobId`, used
/// only for `"create"` and `"updateMetadata"` and only when the chat has
/// one. The deployment builds it from the local blob's `contentType`,
/// `size` and `sha256`, minting a `fetchUrl` and recording the grant to
/// the receiving peer domain first: the serving server "SHOULD serve the
/// blob only to a peer domain to which it has sent a payload carrying
/// that `fetchUrl`, recording the grant before it sends the payload"
/// (§Blob Fetch Authorization). A chat without an `avatarBlobId` sends
/// `avatar: null` for `"updateMetadata"` ("`null` removes the group
/// avatar") and no `avatar` for `"create"`.
///
/// # Errors
///
/// [`OutboundError::UnknownUser`] when a member id has no identity
/// string, [`OutboundError::UnknownAction`] for an action this crate does
/// not know, [`OutboundError::MissingAvatar`] when `"create"` or
/// `"updateMetadata"` is built for a chat with an `avatarBlobId` and
/// `avatar` is `None`.
pub fn peer_group_update_args(
    identities: &SenderIdentities<'_>,
    peer_account_id: Id,
    recipient_user_id: impl Into<String>,
    chat: &Chat,
    action: &GroupUpdateAction,
    avatar: Option<PeerAvatar>,
) -> Result<PeerGroupUpdateArgs, OutboundError> {
    let peer_action = match action {
        GroupUpdateAction::Create => PeerAction::Create,
        GroupUpdateAction::AddMembers { .. } => PeerAction::AddMembers,
        GroupUpdateAction::RemoveMembers { .. } => PeerAction::RemoveMembers,
        GroupUpdateAction::UpdateRoles { .. } => PeerAction::UpdateRoles,
        GroupUpdateAction::UpdateMetadata => PeerAction::UpdateMetadata,
        GroupUpdateAction::Leave => PeerAction::Leave,
        GroupUpdateAction::Close => PeerAction::Close,
        _ => return Err(OutboundError::UnknownAction),
    };
    let mut args = PeerGroupUpdateArgs::new(
        peer_account_id,
        recipient_user_id,
        chat.id.clone(),
        identities.owner_user_id,
        peer_action,
    );
    match action {
        GroupUpdateAction::Create => {
            args.members = Some(identities.members(chat.members.as_deref().unwrap_or_default())?);
            set_metadata(&mut args, chat, avatar)?;
        }
        GroupUpdateAction::UpdateMetadata => {
            set_metadata(&mut args, chat, avatar)?;
            args.avatar.get_or_insert(Clearable::Clear);
        }
        GroupUpdateAction::AddMembers { added } => {
            args.added_members = Some(identities.members(added)?);
        }
        GroupUpdateAction::RemoveMembers { removed_ids } => {
            args.removed_member_ids = Some(
                removed_ids
                    .iter()
                    .map(|id| identities.identity(id))
                    .collect::<Result<_, _>>()?,
            );
        }
        GroupUpdateAction::UpdateRoles { updated } => {
            args.updated_roles = Some(
                updated
                    .iter()
                    .map(|m| {
                        Ok(PeerRoleUpdate::new(
                            identities.identity(&m.id)?,
                            m.role.clone(),
                        ))
                    })
                    .collect::<Result<_, OutboundError>>()?,
            );
        }
        _ => {}
    }
    Ok(args)
}

fn set_metadata(
    args: &mut PeerGroupUpdateArgs,
    chat: &Chat,
    avatar: Option<PeerAvatar>,
) -> Result<(), OutboundError> {
    args.name.clone_from(&chat.name);
    args.description.clone_from(&chat.description);
    if chat.avatar_blob_id.is_some() {
        args.avatar = Some(Clearable::Set(avatar.ok_or(OutboundError::MissingAvatar)?));
    }
    Ok(())
}

/// The aggregate `deliveryState` of a Message from each recipient's
/// state (§Delivery State Transitions).
///
/// The best state any recipient has reached, ordered `"pending"` <
/// `"delivered"` < `"received"`; `"failed"` only when every recipient has
/// failed permanently. A state this crate does not know counts as
/// `"pending"`. With no recipients the Message stays `"pending"`, the
/// initial state.
pub fn aggregate_delivery_state<'a>(
    recipients: impl IntoIterator<Item = &'a DeliveryState>,
) -> DeliveryState {
    let mut any = false;
    let best = recipients
        .into_iter()
        .inspect(|_| any = true)
        .filter_map(|s| match s {
            DeliveryState::Failed => None,
            DeliveryState::Received => Some(2),
            DeliveryState::Delivered => Some(1),
            _ => Some(0),
        })
        .max();
    match best {
        Some(2) => DeliveryState::Received,
        Some(1) => DeliveryState::Delivered,
        None if any => DeliveryState::Failed,
        _ => DeliveryState::Pending,
    }
}

/// How one outbound `Peer/*` call failed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryFailure<'a> {
    /// The peer answered with a JMAP method-level error of this `type`.
    MethodError(&'a str),
    /// The peer answered with this non-success HTTP status.
    HttpStatus(u16),
    /// No connection could be established, or no response arrived within
    /// the deployment's timeout.
    NoResponse,
}

/// `true` when the failure is transient and the call is retried with
/// backoff; `false` when it is permanent (§Retry and Backoff, §Error
/// Handling).
///
/// Transient: the method errors `rateLimited` and `serverFail`, an HTTP
/// 5xx status, and no response. Every other failure, including
/// `invalidArguments`, `forbidden` and any error type the draft does not
/// list, is permanent: the draft counts an attempt as failed, and so
/// retried, only for a transient error, a failed connection or a timeout.
pub fn is_transient(failure: DeliveryFailure<'_>) -> bool {
    match failure {
        DeliveryFailure::MethodError(error_type) => {
            matches!(error_type, "rateLimited" | "serverFail")
        }
        DeliveryFailure::HttpStatus(status) => (500..=599).contains(&status),
        DeliveryFailure::NoResponse => true,
    }
}
