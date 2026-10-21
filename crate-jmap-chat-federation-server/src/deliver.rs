//! `Peer/deliver` (draft-atwood-jmap-chat-federation-00 §Peer/deliver):
//! the receiver's validation of a new message, an edit or a reaction
//! update, and the direct-chat simultaneous-initiation race.
//!
//! [`handle_peer_deliver`] runs the draft's §New Message Validation steps
//! over a [`FederationBackend`], which is a [`ChatBackend`] plus the few
//! things federation needs that the chat backend does not carry: the
//! authenticated peer, the `@everyone` permission floor, attachment
//! fetching, Chat records whose id the peer chose, and typing events
//! forwarded to the owner's clients.
//!
//! The identity resolution, peer check and placeholder creation here are
//! shared with the other `Peer/*` handlers of this crate.

use std::collections::{HashMap, HashSet};

use jmap_chat_federation_types::{
    is_jmap_object_uri, PeerAttachment, PeerAvatar, PeerDeliverArgs, PeerDeliverResponse, PeerEdit,
    PeerMention, PeerMessage, PeerReactionUpdate, ReactionAction,
};
use jmap_chat_server::{
    find_message_by_sender_msg_id, resolve_channel_permissions, store_server_message,
    BackendSetError, ChatBackend, ChatLimits, SetErrorType, StoreOutcome, MANAGE_CHANNELS, SEND,
};
use jmap_chat_types::vocabulary::CHAT_CAP_TASK;
use jmap_chat_types::{
    BroadcastMention, Chat, ChatContact, ChatKind, ChatTypingEvent, DeliveryState, Mention,
    Message, MessageAction, Space, SpaceBan, BROADCAST_MENTION_SCOPES,
};
use jmap_peer_auth::{bind_identity, IdentityBinding, PeerDomain};
use jmap_server::{now_utc_string, serialize_value, server_fail_from_backend, HandlerOutput};
use jmap_types::{Id, JmapError, PatchObject, UTCDate};
use serde_json::{json, Map, Value};
use subtle::ConstantTimeEq;

use crate::outbound::task_assignee_ids;

/// The `id` [`ChatBackend::create_object`] documents as the placeholder it
/// replaces with a server-assigned id.
pub(crate) const PLACEHOLDER_ID: &str = "placeholder";

/// Implementation-defined maximum byte length of `subject`, the value
/// `Message/set` applies (draft-atwood-jmap-chat-00 §Message `subject`
/// sets no limit).
const MAX_SUBJECT_BYTES: usize = 1024;

/// The peer that signed the request, as the transport authenticated it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedPeer {
    /// The authenticated peer domain: the domain of the verified `keyid`
    /// (§Request Signing).
    pub peer_domain: PeerDomain,
    /// The peer identity: the `ownerUserId` published at
    /// `peer_domain`, from the fetch that supplied the signing keys
    /// (§Identity Binding rule 1).
    pub peer_identity: String,
    /// Whether the DID-bound verification of draft-atwood-jmap-chat-did-00
    /// §DID-Bound Verification succeeded for `peer_identity`. Only a
    /// host-less DID identity needs it: §Identity Binding rule 3 accepts
    /// one "only after the DID-bound verification of {{JMAP-CHAT-DID}}
    /// succeeds". It has no effect on any other identity.
    pub did_verified: bool,
}

impl AuthenticatedPeer {
    /// The peer at `peer_domain` whose identity is `peer_identity`, with
    /// no DID-bound verification.
    pub fn new(peer_domain: PeerDomain, peer_identity: impl Into<String>) -> Self {
        Self {
            peer_domain,
            peer_identity: peer_identity.into(),
            did_verified: false,
        }
    }

    /// Mark the peer identity as DID-verified.
    ///
    /// Call this only after `jmap_chat_server::verify_did_binding`
    /// returned `Ok` for the DID `peer_identity` and the key that
    /// verified the request signature.
    #[must_use]
    pub fn with_did_verified(mut self) -> Self {
        self.did_verified = true;
        self
    }
}

/// An attachment blob fetched from its `fetchUrl` and stored locally.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedBlob {
    /// The local blob id the stored Attachment carries.
    pub blob_id: Id,
    /// The number of bytes fetched.
    pub size: u64,
    /// SHA-256 of the fetched bytes.
    pub sha256: [u8; 32],
}

impl FetchedBlob {
    /// A blob stored as `blob_id`, `size` bytes long, with digest `sha256`.
    pub const fn new(blob_id: Id, size: u64, sha256: [u8; 32]) -> Self {
        Self {
            blob_id,
            size,
            sha256,
        }
    }
}

/// What the `Peer/*` handlers need beyond [`ChatBackend`].
///
/// None of these methods has a default: each one carries data or an
/// action the draft requires and that no neutral value stands in for.
pub trait FederationBackend: ChatBackend {
    /// The authenticated peer of `caller`, or `None` when the caller is
    /// not a peer.
    ///
    /// Return `Some` only after the request passed §Request Signing
    /// checks 1 to 6 (for example through `jmap_peer_auth_io::authenticate`)
    /// and the peer domain published an `ownerUserId`. When that identity
    /// is a DID without a host, the handlers accept it only if
    /// [`AuthenticatedPeer::did_verified`] is set. `None` makes every
    /// `Peer/*` handler return `forbidden` (§Server-to-Server Methods:
    /// "Callers without the `"peer"` role MUST receive `forbidden`").
    fn authenticated_peer(&self, caller: &Self::CallerCtx) -> Option<AuthenticatedPeer>;

    /// The permissions of the implicit `@everyone` role in `space`
    /// (draft-atwood-jmap-chat-00 §Space Permission Resolution step 1).
    /// The draft does not say where they are stored.
    fn everyone_permissions(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        space: &Space,
    ) -> HashSet<String>;

    /// A fresh id in the form the backend assigns to a stored Message.
    ///
    /// A silently dropped new message (steps 4 and 7) answers with this
    /// id, so the response does not show the peer that it was dropped.
    fn new_message_id(&self) -> Id;

    /// Fetch `attachment` from its `fetchUrl` and store it as a blob of
    /// `account_id` (step 11).
    ///
    /// The implementation applies §Blob Fetch and SSRF to the URL and
    /// fetches it with an HTTP GET signed as §Request Signing requires
    /// for "every HTTP request it sends to a peer ... to fetch a blob from
    /// a `fetchUrl`": one signature tagged `"jmap-chat-peer"` covering
    /// `@method` and `@target-uri` (a GET has no content, so no
    /// `content-digest`). A serving server rejects an unsigned fetch. It
    /// does not follow a redirect to another origin ("a fetching server
    /// does not follow such a redirect"). The handler compares the
    /// returned size and digest with the attachment's `size` and `sha256`,
    /// and rejects a mismatch with `invalidArguments`.
    ///
    /// - `Ok(Ok(blob))`: the fetched blob.
    /// - `Ok(Err(error))`: reject the request with `error`. Return
    ///   `JmapError::custom("rateLimited")` when the serving server
    ///   answers 429, and `invalidArguments` when it answers 403 or 404 or
    ///   the SSRF policy refuses the URL.
    /// - `Err(e)`: `serverFail`, which the sender retries. Use it when the
    ///   connection fails, the request times out, or the serving server
    ///   answers with a 5xx status.
    fn fetch_attachment(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        attachment: &PeerAttachment,
    ) -> impl std::future::Future<Output = Result<Result<FetchedBlob, JmapError>, Self::Error>> + Send;

    /// Fetch the `Peer/groupUpdate` `avatar` from its `fetchUrl` and store
    /// it as a blob of `account_id`.
    ///
    /// The implementation applies §Blob Fetch and SSRF to the URL and
    /// fetches it as [`FederationBackend::fetch_attachment`] does: a GET
    /// signed per §Request Signing that does not follow a redirect to
    /// another origin. The handler calls this only after the request's authorization and
    /// `closedAt` checks and its own `contentType` and `size` checks, and
    /// compares the returned size and digest with the avatar's `size` and
    /// `sha256`. The avatar is best-effort ("A failed avatar never causes
    /// the request to be rejected"), so every failure is `None`.
    fn fetch_avatar(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        avatar: &PeerAvatar,
    ) -> impl std::future::Future<Output = Option<FetchedBlob>> + Send;

    /// Store `chat` under its own `id`.
    ///
    /// `Peer/deliver` step 3 for a direct Chat: "create a new Chat record
    /// with this chatId"; `Peer/groupUpdate` `"create"` for a group Chat:
    /// "The receiver MUST create a new Chat record of `kind: "group"` with
    /// this chatId". [`ChatBackend::create_object`] assigns ids, so it
    /// cannot. Record the create in Chat state and changes as
    /// `create_object` does.
    fn create_chat(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        chat: Chat,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Give the direct Chat `from` the id `to`, keeping its other
    /// properties, and move its Messages to `to` with their ids unchanged.
    ///
    /// §Direct Chat Simultaneous Initiation: "The receiving server SHOULD
    /// migrate any messages stored under the non-canonical chatId to the
    /// canonical one". Called only when `to` sorts before `from` and no
    /// Chat has id `to`.
    fn move_direct_chat(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        from: &Id,
        to: &Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Send `event` to the connected clients of the owner of `account_id`.
    ///
    /// §Peer/typing "Receiver Behavior": "If all checks pass, the server
    /// MUST forward a typing push event (as defined in {{JMAP-CHAT}}) to
    /// the owner's connected clients." [`crate::handle_peer_typing`] calls
    /// this only after those checks pass. The event is not stored.
    ///
    /// The draft's per-peer rate limit ("no more than one `Peer/typing`
    /// call per peer per chat per 3 seconds; calls received above this
    /// rate MAY be silently discarded") belongs here, with the transport,
    /// as `jmap_chat_server`'s typing limits do.
    fn forward_typing(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        event: &ChatTypingEvent,
    ) -> impl std::future::Future<Output = ()> + Send;
}

/// Steps 1 and 2 of §New Message Validation, which every `Peer/*` method
/// applies to its caller-identity field: "Servers MUST validate all such
/// fields identically against the peer identity" (§Server-to-Server
/// Methods). The peer identity must be bound to the authenticated peer
/// domain (§Identity Binding), or be a host-less DID whose DID-bound
/// verification succeeded (rule 3), and equal `claimed` octet for octet.
pub(crate) fn authenticate<B: FederationBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    claimed: &str,
) -> Result<(), JmapError> {
    let peer = backend
        .authenticated_peer(caller)
        .ok_or_else(JmapError::forbidden)?;
    let bound = match bind_identity(&peer.peer_identity, &peer.peer_domain) {
        IdentityBinding::Bound => true,
        IdentityBinding::NeedsDidVerification => peer.did_verified,
        IdentityBinding::HostMismatch | IdentityBinding::NoHost => false,
    };
    if !bound || claimed != peer.peer_identity {
        return Err(JmapError::forbidden());
    }
    Ok(())
}

/// `accountNotFound` unless `account_id` exists.
pub(crate) async fn check_account<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
) -> Result<(), JmapError> {
    if backend
        .account_exists(caller, account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        Ok(())
    } else {
        Err(JmapError::account_not_found())
    }
}

/// The Chat `chat_id` of `account_id`, if it exists.
pub(crate) async fn get_chat<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    chat_id: &Id,
) -> Result<Option<Chat>, JmapError> {
    let (chats, _) = backend
        .get_objects::<Chat>(
            caller,
            account_id,
            Some(std::slice::from_ref(chat_id)),
            None,
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    Ok(chats.into_iter().find(|c| &c.id == chat_id))
}

/// Whether the ChatContact `contact_id` takes part in `chat`: the
/// direct Chat's `contactId`, a group member, or a member of a channel's
/// Space.
pub(crate) async fn is_participant<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    chat: &Chat,
    contact_id: &Id,
) -> Result<bool, JmapError> {
    Ok(match chat.kind {
        ChatKind::Direct => chat.contact_id.as_ref() == Some(contact_id),
        ChatKind::Group => chat.members.iter().flatten().any(|m| &m.id == contact_id),
        ChatKind::Channel => match &chat.space_id {
            Some(space_id) => backend
                .get_objects::<Space>(
                    caller,
                    account_id,
                    Some(std::slice::from_ref(space_id)),
                    None,
                )
                .await
                .map_err(|e| server_fail_from_backend(&e))?
                .0
                .iter()
                .any(|s| &s.id == space_id && s.members.iter().any(|m| &m.id == contact_id)),
            None => false,
        },
        _ => false,
    })
}

/// Handle a `Peer/deliver` method call.
///
/// Runs draft-atwood-jmap-chat-federation-00 §New Message Validation for
/// a `message`, and §Edit and Reaction Validation for an `edit` or a
/// `reactionUpdate`, then stores the result and answers with the
/// §Response arguments.
///
/// - Steps 1 and 2: the [`FederationBackend::authenticated_peer`]
///   identity must be bound to its peer domain (§Identity Binding) and
///   equal `senderUserId` octet for octet.
/// - Step 3 resolves the Chat, including the direct-chat race: for a
///   direct chat already stored as Y, an incoming X adopts `min(X, Y)`
///   and the response carries it as `canonicalChatId`.
/// - Steps 4 and 7 drop silently: success, nothing stored.
/// - Steps 8 to 14 validate the payload. Step 11 fetches attachments
///   through [`FederationBackend::fetch_attachment`] after every other
///   check has passed; every check of steps 10 to 14 rejects with
///   `invalidArguments`, so running it last changes no response, and a
///   rejected request fetches nothing.
/// - Identities in `senderUserId`, `mentions` and the `assigneeIds` of a
///   `urn:ietf:params:jmap:chat:cap:task` action resolve per §User References in Peer
///   Methods. Placeholder ChatContacts are created
///   only once the request is accepted. A backend that limits them
///   rejects the create with `rateLimit`, which becomes `rateLimited`
///   (§Placeholder ChatContact Creation); the placeholders the request
///   already created are destroyed first.
/// - A `senderMsgId` already stored in the Chat answers with the stored
///   Message and stores nothing ("the receiver MAY silently discard the
///   duplicate").
/// - Expiry is stored on the Message (`senderExpiresAt`, `burnOnRead`,
///   `expiresAfterReadSeconds`) for [`jmap_chat_server::sweep_expired`].
///
/// ponytail: Chats, ChatContacts, SpaceBans and Messages are read in full
/// per call, and the writes are separate backend calls, not one
/// transaction. A rejection happens before the first write. A backend
/// failure between writes returns `serverFail`; the peer's retry reuses
/// what was written, because identities, the direct Chat and
/// `senderMsgId` are all looked up first. Move to indexed lookups and a
/// transactional commit hook if either becomes a problem.
///
/// # Errors
///
/// `invalidArguments`, `forbidden`, `rateLimited` and `serverFail`, the
/// error types of §Error Handling; `accountNotFound` for an unknown
/// `accountId`.
pub async fn handle_peer_deliver<B: FederationBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args: PeerDeliverArgs = serde_json::from_value(args)
        .map_err(|e| JmapError::invalid_arguments(format!("Peer/deliver arguments: {e}")))?;
    let payload = match (&args.message, &args.edit, &args.reaction_update) {
        (Some(m), None, None) => Payload::Message(m),
        (None, Some(e), None) => Payload::Edit(e),
        (None, None, Some(r)) => Payload::Reaction(r),
        _ => {
            return Err(JmapError::invalid_arguments(
                "exactly one of message, edit or reactionUpdate must be present",
            ))
        }
    };

    // Steps 1 and 2: the signature was verified by the transport; the
    // identity binding and `senderUserId` are checked here.
    authenticate(backend, caller, &args.sender_user_id)?;

    let account_id = &args.account_id;
    check_account(backend, caller, account_id).await?;
    let now = now_utc_string();
    let identities = Identities::load(backend, caller, account_id).await?;
    let sender = match identities.lookup(&args.sender_user_id) {
        // The owner never sends to its own mailbox through a peer.
        Who::Owner => return Err(JmapError::forbidden()),
        Who::Contact(c) => Some(c.clone()),
        Who::Unknown => None,
    };
    let sender_id = sender.as_ref().map(|c| c.id.clone());

    // Step 3.
    let target = resolve_chat(backend, caller, &args, sender_id.as_ref()).await?;
    let drop = SilentDrop {
        account_id,
        now: &now,
        canonical: target.canonical.clone(),
    };

    // Steps 4 to 6.
    if let (Some(space), Some(sender_id)) = (&target.space, &sender_id) {
        if is_banned(backend, caller, account_id, space, sender_id, &now).await? {
            return drop.respond(backend, caller, &payload, &target).await;
        }
        let permissions =
            channel_permissions(backend, caller, account_id, space, &target, sender_id);
        if !permissions.contains(SEND) {
            return Err(JmapError::forbidden());
        }
        check_slow_mode(
            backend,
            caller,
            account_id,
            &target,
            sender_id,
            &permissions,
            &now,
        )
        .await?;
    }

    // Step 7.
    if sender.as_ref().is_some_and(|c| c.blocked) {
        return drop.respond(backend, caller, &payload, &target).await;
    }

    let mut ctx = Commit::new(backend, caller, account_id, &now, identities);
    match payload {
        Payload::Message(m) => deliver_message(&mut ctx, &args, m, &target).await,
        Payload::Edit(e) => deliver_edit(&mut ctx, e, &target, sender_id.as_ref()).await,
        Payload::Reaction(r) => deliver_reaction(&mut ctx, r, &target, sender_id.as_ref()).await,
    }
}

/// The one payload of a `Peer/deliver` request.
enum Payload<'a> {
    Message(&'a PeerMessage),
    Edit(&'a PeerEdit),
    Reaction(&'a PeerReactionUpdate),
}

/// A user an identity string names (§User References in Peer Methods).
pub(crate) enum Who<'a> {
    /// The receiving server's own `ownerUserId`.
    Owner,
    /// The ChatContact with that `identity`.
    Contact(&'a ChatContact),
    /// No local user yet; a placeholder ChatContact stands in.
    Unknown,
}

/// The owner's identity and the account's ChatContacts.
pub(crate) struct Identities {
    owner: Option<String>,
    contacts: Vec<ChatContact>,
}

impl Identities {
    pub(crate) async fn load<B: ChatBackend>(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
    ) -> Result<Self, JmapError> {
        let owner = backend
            .owner_user_id(caller, account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        let (contacts, _) = backend
            .get_objects::<ChatContact>(caller, account_id, None, None)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        Ok(Self { owner, contacts })
    }

    /// "The receiving server compares identity strings octet for octet".
    pub(crate) fn lookup(&self, identity: &str) -> Who<'_> {
        if self.owner.as_deref() == Some(identity) {
            return Who::Owner;
        }
        match self.contacts.iter().find(|c| c.identity == identity) {
            Some(c) => Who::Contact(c),
            None => Who::Unknown,
        }
    }
}

/// How the delivery reaches its Chat.
enum ChatPlan {
    /// The Chat is stored under the id the delivery uses.
    Existing,
    /// No direct Chat with the sender exists: create one.
    Create,
    /// The direct Chat stored under `from` moves to the incoming id.
    Move { from: Id },
}

/// The outcome of step 3.
struct Target {
    /// The stored Chat, under its current id.
    chat: Option<Chat>,
    /// The channel's Space.
    space: Option<Space>,
    plan: ChatPlan,
    /// The Chat id the delivery is stored under.
    chat_id: Id,
    /// `canonicalChatId` for the response.
    canonical: Option<Id>,
}

/// Step 3 of §New Message Validation.
async fn resolve_chat<B: FederationBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: &PeerDeliverArgs,
    sender_id: Option<&Id>,
) -> Result<Target, JmapError> {
    let account_id = &args.account_id;
    let (chats, _) = backend
        .get_objects::<Chat>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let incoming = chats.iter().find(|c| c.id == args.chat_id);
    let target = |chat: &Chat, space: Option<Space>| Target {
        chat: Some(chat.clone()),
        space,
        plan: ChatPlan::Existing,
        chat_id: chat.id.clone(),
        canonical: None,
    };
    match &args.chat_kind {
        ChatKind::Direct => {
            let existing = sender_id.and_then(|s| {
                chats
                    .iter()
                    .find(|c| c.kind == ChatKind::Direct && c.contact_id.as_ref() == Some(s))
            });
            match existing {
                Some(y) if y.id == args.chat_id => Ok(target(y, None)),
                // §Direct Chat Simultaneous Initiation: "both servers MUST
                // adopt `min(X, Y)` (lexicographically smaller)".
                Some(y) if y.id.as_ref() < args.chat_id.as_ref() => Ok(Target {
                    canonical: Some(y.id.clone()),
                    ..target(y, None)
                }),
                Some(y) => {
                    if incoming.is_some() {
                        return Err(JmapError::forbidden());
                    }
                    Ok(Target {
                        chat: Some(y.clone()),
                        space: None,
                        plan: ChatPlan::Move { from: y.id.clone() },
                        chat_id: args.chat_id.clone(),
                        canonical: Some(args.chat_id.clone()),
                    })
                }
                None => {
                    // The chatId of another conversation is not taken
                    // over (§Chat ID Integrity).
                    if incoming.is_some() {
                        return Err(JmapError::forbidden());
                    }
                    Ok(Target {
                        chat: None,
                        space: None,
                        plan: ChatPlan::Create,
                        chat_id: args.chat_id.clone(),
                        canonical: None,
                    })
                }
            }
        }
        ChatKind::Group => {
            let chat = incoming
                .filter(|c| {
                    c.kind == ChatKind::Group
                        && sender_id.is_some_and(|s| c.members.iter().flatten().any(|m| &m.id == s))
                })
                .ok_or_else(JmapError::forbidden)?;
            if chat.closed_at.is_some() {
                return Err(JmapError::forbidden());
            }
            Ok(target(chat, None))
        }
        ChatKind::Channel => {
            let chat = incoming
                .filter(|c| c.kind == ChatKind::Channel)
                .ok_or_else(JmapError::forbidden)?;
            let space_id = chat.space_id.as_ref().ok_or_else(JmapError::forbidden)?;
            let (spaces, _) = backend
                .get_objects::<Space>(
                    caller,
                    account_id,
                    Some(std::slice::from_ref(space_id)),
                    None,
                )
                .await
                .map_err(|e| server_fail_from_backend(&e))?;
            let space = spaces
                .into_iter()
                .find(|s| &s.id == space_id)
                .filter(|s| sender_id.is_some_and(|id| s.members.iter().any(|m| &m.id == id)))
                .ok_or_else(JmapError::forbidden)?;
            if space.closed_at.is_some() {
                return Err(JmapError::forbidden());
            }
            Ok(target(chat, Some(space)))
        }
        _ => Err(JmapError::invalid_arguments(
            "chatKind must be \"direct\", \"group\" or \"channel\"",
        )),
    }
}

/// Step 4: an active SpaceBan for the sender in the channel's Space.
///
/// A ban whose `expiresAt` does not parse counts as active.
async fn is_banned<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    space: &Space,
    sender_id: &Id,
    now: &UTCDate,
) -> Result<bool, JmapError> {
    let (bans, _) = backend
        .get_objects::<SpaceBan>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    Ok(bans.iter().any(|b| {
        b.space_id == space.id
            && &b.user_id == sender_id
            && b.expires_at
                .as_ref()
                .is_none_or(|expires| !is_before(expires, now))
    }))
}

/// Step 5: the sender's permissions in the channel, per
/// draft-atwood-jmap-chat-00 §Space Permission Resolution.
fn channel_permissions<B: FederationBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    space: &Space,
    target: &Target,
    sender_id: &Id,
) -> HashSet<String> {
    let Some(member) = space.members.iter().find(|m| &m.id == sender_id) else {
        return HashSet::new();
    };
    let role_ids: HashSet<&str> = member.role_ids.iter().map(AsRef::as_ref).collect();
    let space_permissions: HashSet<String> = space
        .roles
        .iter()
        .filter(|r| role_ids.contains(r.id.as_ref()))
        .flat_map(|r| r.permissions.iter().map(|p| p.as_str().to_owned()))
        .chain(backend.everyone_permissions(caller, account_id, space))
        .collect();
    let overrides = target
        .chat
        .as_ref()
        .and_then(|c| c.permission_overrides.as_deref())
        .unwrap_or_default();
    resolve_channel_permissions(space_permissions, sender_id.as_ref(), &role_ids, overrides)
}

/// Step 6: slow mode, exempting holders of `"manage_channels"`.
async fn check_slow_mode<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    target: &Target,
    sender_id: &Id,
    permissions: &HashSet<String>,
    now: &UTCDate,
) -> Result<(), JmapError> {
    let slow = target
        .chat
        .as_ref()
        .and_then(|c| c.slow_mode_seconds)
        .unwrap_or(0);
    if slow == 0 || permissions.contains(MANAGE_CHANNELS) {
        return Ok(());
    }
    let (messages, _) = backend
        .get_objects::<Message>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let last = messages
        .iter()
        .filter(|m| m.chat_id == target.chat_id && &m.sender_id == sender_id)
        .filter_map(|m| epoch_seconds(&m.received_at))
        .max();
    let now = epoch_seconds(now).unwrap_or(i64::MAX);
    match last {
        Some(last) if now.saturating_sub(last) < i64::try_from(slow).unwrap_or(i64::MAX) => {
            Err(JmapError::custom("rateLimited"))
        }
        _ => Ok(()),
    }
}

/// A silent drop (steps 4 and 7): success with nothing stored.
struct SilentDrop<'a> {
    account_id: &'a Id,
    now: &'a UTCDate,
    canonical: Option<Id>,
}

impl SilentDrop<'_> {
    /// Answer as an accepted delivery would. A new message gets a fresh
    /// id. An edit or reaction gets the target's local id when it exists,
    /// which the peer learned from the original delivery. A race
    /// `canonicalChatId` is still reported, though nothing moves.
    async fn respond<B: FederationBackend>(
        self,
        backend: &B,
        caller: &B::CallerCtx,
        payload: &Payload<'_>,
        target: &Target,
    ) -> Result<HandlerOutput, JmapError> {
        let sender_msg_id = match payload {
            Payload::Message(_) => None,
            Payload::Edit(e) => Some(&e.sender_msg_id),
            Payload::Reaction(r) => Some(&r.sender_msg_id),
        };
        let existing = match (sender_msg_id, &target.chat) {
            (Some(sender_msg_id), Some(chat)) => find_message_by_sender_msg_id(
                backend,
                caller,
                self.account_id,
                &chat.id,
                sender_msg_id,
            )
            .await
            .map_err(|e| server_fail_from_backend(&e))?
            .map(|m| m.id),
            _ => None,
        };
        let id = existing.unwrap_or_else(|| backend.new_message_id());
        respond(self.account_id, id, self.now.clone(), self.canonical)
    }
}

/// The §Response arguments.
fn respond(
    account_id: &Id,
    received_msg_id: Id,
    received_at: UTCDate,
    canonical: Option<Id>,
) -> Result<HandlerOutput, JmapError> {
    let mut response = PeerDeliverResponse::new(account_id.clone(), received_msg_id, received_at);
    response.canonical_chat_id = canonical;
    Ok(HandlerOutput::new(serialize_value(&response)?))
}

/// Writes for an accepted request.
pub(crate) struct Commit<'a, B: FederationBackend> {
    pub(crate) backend: &'a B,
    pub(crate) caller: &'a B::CallerCtx,
    pub(crate) account_id: &'a Id,
    pub(crate) now: &'a UTCDate,
    pub(crate) identities: Identities,
    /// Placeholders created by this request, by identity.
    created: HashMap<String, Id>,
}

impl<'a, B: FederationBackend> Commit<'a, B> {
    pub(crate) fn new(
        backend: &'a B,
        caller: &'a B::CallerCtx,
        account_id: &'a Id,
        now: &'a UTCDate,
        identities: Identities,
    ) -> Self {
        Self {
            backend,
            caller,
            account_id,
            now,
            identities,
            created: HashMap::new(),
        }
    }

    /// The local id of the user `identity` names, creating a placeholder
    /// ChatContact for an unknown one (§User References in Peer Methods
    /// rules 1 to 3).
    pub(crate) async fn local_id(&mut self, identity: &str) -> Result<Id, JmapError> {
        match self.identities.lookup(identity) {
            Who::Owner => return Ok(self.account_id.clone()),
            Who::Contact(c) => return Ok(c.id.clone()),
            Who::Unknown => {}
        }
        if let Some(id) = self.created.get(identity) {
            return Ok(id.clone());
        }
        // draft-atwood-jmap-chat-00 §ChatContact `login`: "A non-empty
        // human-readable identifier ... The format is deployment-specific".
        let placeholder = ChatContact::new(
            Id::from(PLACEHOLDER_ID),
            identity,
            identity,
            self.now.clone(),
            self.now.clone(),
            false,
        );
        let created = self
            .backend
            .create_object::<ChatContact>(self.caller, self.account_id, identity, placeholder)
            .await;
        match created {
            Ok((id, _)) => {
                self.created.insert(identity.to_owned(), id.clone());
                Ok(id)
            }
            Err(e) => {
                self.discard_placeholders().await?;
                Err(set_failure(e))
            }
        }
    }

    /// Destroy the placeholders this request created. A placeholder
    /// limit "rejects, with `rateLimited`, any request that would exceed
    /// it, and that request creates no placeholders" (§Placeholder
    /// ChatContact Creation); placeholders are the first writes of a
    /// request, so nothing else is undone.
    async fn discard_placeholders(&mut self) -> Result<(), JmapError> {
        for (_, id) in self.created.drain() {
            self.backend
                .destroy_object::<ChatContact>(self.caller, self.account_id, &id)
                .await
                .map_err(|_| JmapError::server_fail("could not discard a placeholder"))?;
        }
        Ok(())
    }

    /// Map each Mention `id` from an identity string to a local id.
    async fn mentions(
        &mut self,
        mentions: Option<&[PeerMention]>,
    ) -> Result<Vec<Mention>, JmapError> {
        let mut out = Vec::new();
        for m in mentions.unwrap_or_default() {
            let mut mention = Mention::new(self.local_id(&m.id).await?);
            mention.extra = m.extra.clone();
            out.push(mention);
        }
        Ok(out)
    }

    /// Map each `assigneeIds` entry of a `urn:ietf:params:jmap:chat:cap:task` action
    /// from an identity string to a local id (draft-atwood-jmap-chat-tasks-00
    /// §Federation: "the receiving server maps each identity to its local
    /// id"). An action whose `uri` is a JMAP object URI is removed first,
    /// keeping the order of the rest (§User References in Peer Methods:
    /// "A receiving server removes every MessageAction ... whose `uri` is
    /// a JMAP object URI"); an empty result is an absent `actions`. Every
    /// other action, key and value is stored as received.
    async fn actions(
        &mut self,
        actions: Option<&[MessageAction]>,
    ) -> Result<Vec<MessageAction>, JmapError> {
        let mut out: Vec<MessageAction> = actions
            .unwrap_or_default()
            .iter()
            .filter(|a| !is_jmap_object_uri(&a.uri))
            .cloned()
            .collect();
        for action in &mut out {
            for entry in task_assignee_ids(action).into_iter().flatten() {
                if let Some(identity) = entry.as_str() {
                    *entry = Value::String(self.local_id(identity).await?.to_string());
                }
            }
        }
        Ok(out)
    }

    /// Bring the Chat to the id the delivery uses (step 3 and
    /// §Direct Chat Simultaneous Initiation).
    async fn chat(&mut self, target: &Target, sender_id: &Id) -> Result<(), JmapError> {
        match &target.plan {
            ChatPlan::Existing => Ok(()),
            ChatPlan::Create => {
                let mut chat = Chat::new(
                    target.chat_id.clone(),
                    ChatKind::Direct,
                    self.now.clone(),
                    0,
                    Vec::new(),
                    false,
                    true,
                );
                chat.contact_id = Some(sender_id.clone());
                self.backend
                    .create_chat(self.caller, self.account_id, chat)
                    .await
                    .map_err(set_failure)
            }
            ChatPlan::Move { from } => self
                .backend
                .move_direct_chat(self.caller, self.account_id, from, &target.chat_id)
                .await
                .map_err(set_failure),
        }
    }

    /// The Message in `chat_id` whose `senderMsgId` is `sender_msg_id`,
    /// if one is stored.
    async fn find(&self, chat_id: &Id, sender_msg_id: &Id) -> Result<Option<Message>, JmapError> {
        find_message_by_sender_msg_id(
            self.backend,
            self.caller,
            self.account_id,
            chat_id,
            sender_msg_id,
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))
    }
}

/// Map a write failure: a placeholder limit (`rateLimit`) is `rateLimited`
/// (§Placeholder ChatContact Creation); anything else is `serverFail`.
pub(crate) fn set_failure<E: std::error::Error>(e: BackendSetError<E>) -> JmapError {
    match e {
        BackendSetError::SetError(s) if s.error_type == SetErrorType::RateLimit => {
            JmapError::custom("rateLimited")
        }
        BackendSetError::Other(e) => server_fail_from_backend(&e),
        _ => JmapError::server_fail("the backend refused the write"),
    }
}

/// Step 8: `body` byte length against `maxBodyBytes`. A Rust `String` is
/// UTF-8, so deserializing the arguments already validated the encoding.
fn check_body(body: &str, limits: &ChatLimits) -> Result<(), JmapError> {
    if u64::try_from(body.len()).map_or(true, |n| n > limits.max_body_bytes) {
        return Err(JmapError::invalid_arguments(format!(
            "body exceeds maxBodyBytes ({})",
            limits.max_body_bytes
        )));
    }
    Ok(())
}

/// Step 9: `bodyType` against `supportedBodyTypes`.
fn check_body_type(body_type: &str, limits: &ChatLimits) -> Result<(), JmapError> {
    if !limits.supported_body_types.contains(&body_type) {
        return Err(JmapError::invalid_arguments(
            "bodyType is not in supportedBodyTypes",
        ));
    }
    Ok(())
}

/// Step 13: each `scope` is `"everyone"`, `"here"` or `"admins"`.
fn check_broadcast_mentions(broadcast: Option<&[BroadcastMention]>) -> Result<(), JmapError> {
    if broadcast
        .unwrap_or_default()
        .iter()
        .any(|b| !BROADCAST_MENTION_SCOPES.contains(&b.scope.as_str()))
    {
        return Err(JmapError::invalid_arguments(
            "broadcastMentions scope must be \"everyone\", \"here\" or \"admins\"",
        ));
    }
    Ok(())
}

/// Each Mention `id` must be usable as a placeholder's `identity` and
/// `login`, which draft-atwood-jmap-chat-00 §ChatContact requires to
/// hold "at least one non-whitespace character".
fn check_mention_ids(mentions: Option<&[PeerMention]>) -> Result<(), JmapError> {
    if mentions
        .unwrap_or_default()
        .iter()
        .any(|m| m.id.trim().is_empty())
    {
        return Err(JmapError::invalid_arguments(
            "a mentions id is not an identity string",
        ));
    }
    Ok(())
}

/// Each `assigneeIds` entry of a `urn:ietf:params:jmap:chat:cap:task` action is a
/// user reference (draft-atwood-jmap-chat-tasks-00 §Federation) and, like
/// a Mention `id`, must be an identity string a placeholder can carry.
/// An action with a JMAP object URI is not checked: it is removed, not
/// stored (§User References in Peer Methods).
fn check_task_assignees(actions: Option<&[MessageAction]>) -> Result<(), JmapError> {
    for action in actions.unwrap_or_default() {
        if action.action_type != CHAT_CAP_TASK || is_jmap_object_uri(&action.uri) {
            continue;
        }
        let ids = match action.metadata.as_ref().and_then(|m| m.get("assigneeIds")) {
            None | Some(Value::Null) => continue,
            Some(ids) => ids,
        };
        let identities = ids.as_array().is_some_and(|ids| {
            ids.iter()
                .all(|i| i.as_str().is_some_and(|s| !s.trim().is_empty()))
        });
        if !identities {
            return Err(JmapError::invalid_arguments(
                "an assigneeIds entry is not an identity string",
            ));
        }
    }
    Ok(())
}

/// The name of the first field §New Message Validation forbids, "present
/// and non-empty", with an encrypted `bodyType`.
fn encrypted_conflict(m: &PeerMessage) -> Option<&'static str> {
    let non_empty = |s: &Option<String>| s.as_deref().is_some_and(|s| !s.is_empty());
    [
        ("attachments", !m.attachments.is_empty()),
        (
            "mentions",
            m.mentions.as_ref().is_some_and(|v| !v.is_empty()),
        ),
        (
            "broadcastMentions",
            m.broadcast_mentions.as_ref().is_some_and(|v| !v.is_empty()),
        ),
        ("actions", m.actions.as_ref().is_some_and(|v| !v.is_empty())),
        ("replyTo", m.reply_to.is_some()),
        ("threadRootId", m.thread_root_id.is_some()),
        ("topicId", non_empty(&m.topic_id)),
        ("subject", non_empty(&m.subject)),
        ("lastSeenSenderMsgId", m.last_seen_sender_msg_id.is_some()),
    ]
    .into_iter()
    .find_map(|(name, present)| present.then_some(name))
}

/// Step 10: each attachment's `filename`, `contentType` and `size` per
/// draft-atwood-jmap-chat-00 §Attachment, plus the `sha256` form step 11
/// compares against. `size` is bounded by `maxAttachmentBytes` before any
/// fetch (draft-atwood-jmap-chat-00 §Denial of Service: "Enforce
/// `maxBodyBytes` and `maxAttachmentBytes` at parse time, before any
/// fetch or storage").
fn check_attachments(attachments: &[PeerAttachment], limits: &ChatLimits) -> Result<(), JmapError> {
    if u64::try_from(attachments.len()).map_or(true, |n| n > limits.max_attachments_per_message) {
        return Err(JmapError::invalid_arguments(format!(
            "more than maxAttachmentsPerMessage ({}) attachments",
            limits.max_attachments_per_message
        )));
    }
    for a in attachments {
        let a = &a.attachment;
        let valid = !a.filename.contains(['/', '\\', '\0'])
            && valid_media_type(&a.content_type)
            && a.size <= limits.max_attachment_bytes
            && a.sha256.len() == 64
            && a.sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !valid {
            return Err(JmapError::invalid_arguments(format!(
                "attachment {} has an invalid filename, contentType, size or sha256",
                a.blob_id
            )));
        }
    }
    Ok(())
}

/// RFC 2045 §5.1 `type "/" subtype`, each an RFC 2045 `token`, as
/// `Message/set` checks it.
///
/// ponytail: parameters after `;` are not parsed, as in `Message/set`.
pub(crate) fn valid_media_type(s: &str) -> bool {
    let is_token = |t: &str| {
        !t.is_empty()
            && t.bytes()
                .all(|b| b.is_ascii_graphic() && !b"()<>@,;:\\\"/[]?=".contains(&b))
    };
    let essence = s.split(';').next().unwrap_or_default().trim();
    essence
        .split_once('/')
        .is_some_and(|(t, sub)| is_token(t) && is_token(sub))
}

/// `topicId` "MUST be non-empty and MUST NOT exceed 255 bytes";
/// `subject` "MUST be absent when `topicId` is absent"
/// (draft-atwood-jmap-chat-00 §Message).
fn check_topic(m: &PeerMessage) -> Result<(), JmapError> {
    if m.topic_id
        .as_ref()
        .is_some_and(|t| t.is_empty() || t.len() > 255)
    {
        return Err(JmapError::invalid_arguments("invalid topicId"));
    }
    if m.subject
        .as_ref()
        .is_some_and(|s| m.topic_id.is_none() || s.len() > MAX_SUBJECT_BYTES)
    {
        return Err(JmapError::invalid_arguments("invalid subject"));
    }
    Ok(())
}

/// Seconds since the epoch of `d`, ignoring fractional seconds.
///
/// [`now_utc_string`] writes milliseconds, which
/// [`UTCDate::to_epoch_seconds`] rejects (bd:JMAP-jfia.23), so the
/// conversion reads only `YYYY-MM-DDTHH:MM:SS`. RFC 8620 §1.4 fixes the
/// offset to `Z`.
fn epoch_seconds(d: &UTCDate) -> Option<i64> {
    let s: &str = d.as_ref();
    let whole = format!("{}Z", s.get(..19)?);
    UTCDate::from(whole.as_str()).to_epoch_seconds().ok()
}

/// Whether `a` is earlier than `b`, to the second. A value that does not
/// parse is never earlier.
fn is_before(a: &UTCDate, b: &UTCDate) -> bool {
    match (epoch_seconds(a), epoch_seconds(b)) {
        (Some(a), Some(b)) => a < b,
        _ => false,
    }
}

/// Whether the 64-hex-digit `hex` encodes `digest`, compared in constant
/// time as `Message/set` compares attachment digests.
pub(crate) fn sha256_hex_matches(hex: &str, digest: &[u8; 32]) -> bool {
    let mut decoded = [0u8; 32];
    for (i, out) in decoded.iter_mut().enumerate() {
        let Some(byte) = hex
            .get(2 * i..2 * i + 2)
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        else {
            return false;
        };
        *out = byte;
    }
    hex.len() == 64 && bool::from(decoded.ct_eq(digest))
}

/// Steps 8 to 14 for a new message, then the store.
async fn deliver_message<B: FederationBackend>(
    ctx: &mut Commit<'_, B>,
    args: &PeerDeliverArgs,
    m: &PeerMessage,
    target: &Target,
) -> Result<HandlerOutput, JmapError> {
    let limits = ctx.backend.limits(ctx.caller, ctx.account_id);
    check_body(&m.body, &limits)?;
    check_body_type(&m.body_type, &limits)?;
    let encrypted = limits.encrypted_body_types.contains(&m.body_type.as_str());
    if encrypted {
        if let Some(field) = encrypted_conflict(m) {
            return Err(JmapError::invalid_arguments(format!(
                "{field} must be absent with an encrypted bodyType"
            )));
        }
    } else {
        check_attachments(&m.attachments, &limits)?;
        // Step 12: mentions come from `mentions` only; `body` is not read.
        check_mention_ids(m.mentions.as_deref())?;
        check_broadcast_mentions(m.broadcast_mentions.as_deref())?;
        check_task_assignees(m.actions.as_deref())?;
    }
    check_topic(m)?;
    // Step 14.
    if m.sender_expires_at
        .as_ref()
        .is_some_and(|expires| !is_before(ctx.now, expires))
    {
        return Err(JmapError::invalid_arguments(
            "senderExpiresAt must be in the future",
        ));
    }

    if let Some(chat) = &target.chat {
        if let Some(existing) = ctx.find(&chat.id, &m.sender_msg_id).await? {
            return respond(
                ctx.account_id,
                existing.id,
                existing.received_at,
                target.canonical.clone(),
            );
        }
    }

    // Step 11.
    let mut attachments = Vec::with_capacity(m.attachments.len());
    for a in &m.attachments {
        let blob = ctx
            .backend
            .fetch_attachment(ctx.caller, ctx.account_id, a)
            .await
            .map_err(|e| server_fail_from_backend(&e))??;
        if blob.size != a.attachment.size || !sha256_hex_matches(&a.attachment.sha256, &blob.sha256)
        {
            return Err(JmapError::invalid_arguments(format!(
                "attachment {} does not match the fetched blob's size or sha256",
                a.attachment.blob_id
            )));
        }
        let mut stored = a.attachment.clone();
        stored.blob_id = blob.blob_id;
        attachments.push(stored);
    }

    let sender_id = ctx.local_id(&args.sender_user_id).await?;
    let mentions = ctx.mentions(m.mentions.as_deref()).await?;
    let actions = ctx.actions(m.actions.as_deref()).await?;
    ctx.chat(target, &sender_id).await?;
    let chat_id = target.chat_id.clone();
    // "If the receiver cannot resolve a `replyTo` or `threadRootId`
    // senderMsgId to a local message id ... the receiver MUST store the
    // message with `replyTo` and/or `threadRootId` set to null."
    let reply_to = match &m.reply_to {
        Some(r) => ctx.find(&chat_id, r).await?.map(|m| m.id),
        None => None,
    };
    let thread_root_id = match &m.thread_root_id {
        Some(r) => ctx.find(&chat_id, r).await?.map(|m| m.id),
        None => None,
    };

    // A message this mailbox accepted from the sender's server is
    // delivered (draft-atwood-jmap-chat-00 §Message `deliveryState`:
    // "remote server accepted the message").
    let mut msg = Message::new(
        Id::from(PLACEHOLDER_ID),
        m.sender_msg_id.clone(),
        sender_id,
        chat_id,
        m.body.clone(),
        m.body_type.clone(),
        m.sent_at.clone(),
        ctx.now.clone(),
        DeliveryState::Delivered,
    );
    msg.attachments = attachments;
    msg.mentions = mentions;
    msg.broadcast_mentions = m.broadcast_mentions.clone().unwrap_or_default();
    msg.actions = actions;
    msg.reply_to = reply_to;
    msg.thread_root_id = thread_root_id;
    msg.topic_id = m.topic_id.clone();
    msg.subject = m.subject.clone();
    msg.sender_expires_at = m.sender_expires_at.clone();
    msg.burn_on_read = m.burn_on_read;
    msg.expires_after_read_seconds = m.expires_after_read_seconds;
    let id = match store_server_message(ctx.backend, ctx.caller, ctx.account_id, msg)
        .await
        .map_err(set_failure)?
    {
        StoreOutcome::Stored(id) | StoreOutcome::Duplicate(id) => id,
        _ => return Err(JmapError::server_fail("unknown store outcome")),
    };
    respond(
        ctx.account_id,
        id,
        ctx.now.clone(),
        target.canonical.clone(),
    )
}

/// The stored Message an edit or reaction targets, and the sender check
/// both apply.
async fn target_message<B: FederationBackend>(
    ctx: &Commit<'_, B>,
    target: &Target,
    sender_msg_id: &Id,
) -> Result<Message, JmapError> {
    let not_found = || JmapError::invalid_arguments("no message with this senderMsgId in the chat");
    let chat = target.chat.as_ref().ok_or_else(not_found)?;
    ctx.find(&chat.id, sender_msg_id)
        .await?
        .ok_or_else(not_found)
}

/// §Edit and Reaction Validation for an `edit`, then the update.
async fn deliver_edit<B: FederationBackend>(
    ctx: &mut Commit<'_, B>,
    e: &PeerEdit,
    target: &Target,
    sender_id: Option<&Id>,
) -> Result<HandlerOutput, JmapError> {
    let limits = ctx.backend.limits(ctx.caller, ctx.account_id);
    check_body(&e.body, &limits)?;
    check_body_type(&e.body_type, &limits)?;
    if limits.encrypted_body_types.contains(&e.body_type.as_str()) {
        let mentions = e.mentions.as_ref().is_some_and(|v| !v.is_empty());
        let broadcast = e.broadcast_mentions.as_ref().is_some_and(|v| !v.is_empty());
        if mentions || broadcast {
            return Err(JmapError::invalid_arguments(
                "mentions and broadcastMentions must be absent with an encrypted bodyType",
            ));
        }
    } else {
        check_mention_ids(e.mentions.as_deref())?;
        check_broadcast_mentions(e.broadcast_mentions.as_deref())?;
    }
    let msg = target_message(ctx, target, &e.sender_msg_id).await?;
    if Some(&msg.sender_id) != sender_id {
        return Err(JmapError::forbidden());
    }

    let mentions = ctx.mentions(e.mentions.as_deref()).await?;
    ctx.chat(target, &msg.sender_id).await?;
    let mut patch = Map::new();
    patch.insert("body".to_owned(), json!(e.body));
    patch.insert("bodyType".to_owned(), json!(e.body_type));
    patch.insert("editedAt".to_owned(), json!(e.edited_at));
    patch.insert("mentions".to_owned(), serialize_value(&mentions)?);
    patch.insert(
        "broadcastMentions".to_owned(),
        serialize_value(e.broadcast_mentions.clone().unwrap_or_default())?,
    );
    ctx.backend
        .update_object::<Message>(
            ctx.caller,
            ctx.account_id,
            &msg.id,
            PatchObject::from_map(patch),
        )
        .await
        .map_err(set_failure)?;
    respond(
        ctx.account_id,
        msg.id,
        ctx.now.clone(),
        target.canonical.clone(),
    )
}

/// §Edit and Reaction Validation for a `reactionUpdate`, then the update.
async fn deliver_reaction<B: FederationBackend>(
    ctx: &mut Commit<'_, B>,
    r: &PeerReactionUpdate,
    target: &Target,
    sender_id: Option<&Id>,
) -> Result<HandlerOutput, JmapError> {
    if r.emoji.is_empty() {
        return Err(JmapError::invalid_arguments(
            "emoji must be a non-empty string",
        ));
    }
    let msg = target_message(ctx, target, &r.sender_msg_id).await?;
    // A stored Chat with this sender implies a known sender (step 3).
    let sender_id = sender_id.ok_or_else(JmapError::forbidden)?;
    let existing = msg.reactions.get(r.sender_reaction_id.as_ref());
    let value = match r.action {
        ReactionAction::Add => {
            // Another sender's reaction under the same key is not replaced.
            if existing.is_some_and(|x| &x.sender_id != sender_id) {
                return Err(JmapError::forbidden());
            }
            let mut reaction = json!({
                "emoji": r.emoji,
                "senderId": sender_id,
                "sentAt": r.sent_at,
            });
            if let Some(custom) = &r.custom_emoji_id {
                reaction["customEmojiId"] = json!(custom);
            }
            reaction
        }
        ReactionAction::Remove => {
            let existing = existing.ok_or_else(|| {
                JmapError::invalid_arguments("no reaction with this senderReactionId")
            })?;
            if &existing.sender_id != sender_id {
                return Err(JmapError::forbidden());
            }
            Value::Null
        }
        _ => {
            return Err(JmapError::invalid_arguments(
                "action must be \"add\" or \"remove\"",
            ))
        }
    };

    ctx.chat(target, sender_id).await?;
    let mut patch = Map::new();
    patch.insert(format!("reactions/{}", r.sender_reaction_id), value);
    ctx.backend
        .update_object::<Message>(
            ctx.caller,
            ctx.account_id,
            &msg.id,
            PatchObject::from_map(patch),
        )
        .await
        .map_err(set_failure)?;
    respond(
        ctx.account_id,
        msg.id,
        ctx.now.clone(),
        target.canonical.clone(),
    )
}
