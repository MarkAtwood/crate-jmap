//! `chatPush` support for RFC 8620 push subscriptions
//! (draft-atwood-jmap-chat-push-00).
//!
//! The `chatPush` property lives in [`PushSubscription::extra`] as a map of
//! accountId to `ChatPushConfig`. A backend calls [`validate_chat_push`] on
//! every create and update, [`strip_unsupported_chat_push`] on every
//! subscription it returns from `list_push_subscriptions`, builds
//! notifications with [`chat_state_change_for`], and builds the
//! `ChatMessagePush` for each new inbound message with
//! [`build_chat_message_push`] after [`should_push`] allows it, fitted to
//! the platform size limit with [`fit_chat_message_push`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use jmap_chat_types::{
    BodyType, BroadcastScope, Chat, ChatContact, ChatKind, ChatMessageEntry, ChatMessagePush,
    ChatPushCapability, ChatPushConfig, Message, Space, SpaceMember, UrgencyLevel,
};
use jmap_server::state_change_for;
use jmap_types::{Id, PushSubscription, State, StateChange, UTCDate, Urgency};
use serde_json::{Map, Value};

use crate::{ChatBackend, SetError, SetErrorType};

/// The `PushSubscription` property this extension adds.
const CHAT_PUSH: &str = "chatPush";

/// The `ChatMessageEntry` fields a server MUST support in `properties`
/// (draft-atwood-jmap-chat-push-00 §ChatPushConfig). `unreadCount` is a
/// `ChatMessagePush` property, not an entry field, so it is rejected like
/// any unknown name.
const ENTRY_PROPERTIES: [&str; 13] = [
    "messageId",
    "chatId",
    "chatKind",
    "chatName",
    "spaceId",
    "spaceName",
    "senderId",
    "senderDisplayName",
    "sentAt",
    "hasMention",
    "mentionScopes",
    "encrypted",
    "bodySnippet",
];

/// The `ChatMessageEntry` fields sent when `properties` is absent
/// (draft-atwood-jmap-chat-push-00 §ChatPushConfig).
const DEFAULT_ENTRY_PROPERTIES: [&str; 10] = [
    "messageId",
    "chatId",
    "chatKind",
    "senderId",
    "senderDisplayName",
    "sentAt",
    "hasMention",
    "mentionScopes",
    "encrypted",
    "bodySnippet",
];

/// Check the `chatPush` property in a `PushSubscription`'s `extra` map
/// before it is stored.
///
/// `caps` maps each account that advertises
/// `urn:ietf:params:jmap:chat:push` to its account-level capability object.
/// An absent or `null` `chatPush`, and a `null` entry for an account, are
/// accepted. Otherwise the first offending value gets an
/// `invalidProperties` SetError naming it:
///
/// - `chatPush`: the value is not an object.
/// - `chatPush/<accountId>`: the account is not in `caps`, or the entry is
///   not a `ChatPushConfig`. draft-atwood-jmap-chat-push-00
///   §PushSubscription Extension: "If a `PushSubscription/set` create or
///   update includes a `chatPush` entry for an account whose
///   `accountCapabilities` does not include
///   `urn:ietf:params:jmap:chat:push`, the server MUST return an
///   `invalidProperties` SetError whose `properties` names the offending
///   entry (e.g., `chatPush/<id>`) and MUST NOT store the subscription
///   change."
/// - `chatPush/<accountId>/kinds`: a value other than `"direct"`,
///   `"group"` or `"channel"`.
/// - `chatPush/<accountId>/properties`: a name that is not a
///   `ChatMessageEntry` field the server supports.
/// - `chatPush/<accountId>/urgency` and `.../mentionUrgency`: a value not
///   in the account's `supportedUrgencyValues`, including any value outside
///   RFC 8030 §5.3, which no capability can list. draft-atwood-jmap-chat-push-00
///   §ChatPushConfig: "the server MUST return an `invalidProperties`
///   SetError on `PushSubscription/set` whose `properties` names the
///   offending path (e.g., `chatPush/<id>/urgency`)".
pub fn validate_chat_push(
    extra: &Map<String, Value>,
    caps: &HashMap<Id, ChatPushCapability>,
) -> Result<(), Box<SetError>> {
    let entries = match extra.get(CHAT_PUSH) {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::Object(entries)) => entries,
        Some(_) => return Err(Box::new(invalid(CHAT_PUSH.to_owned()))),
    };
    for (account, entry) in entries.iter().filter(|(_, e)| !e.is_null()) {
        let path = format!("{CHAT_PUSH}/{account}");
        let Some(cap) = caps.get(account.as_str()) else {
            return Err(Box::new(invalid(path)));
        };
        let cfg: ChatPushConfig =
            serde_json::from_value(entry.clone()).map_err(|_| invalid(path.clone()))?;
        if cfg
            .kinds
            .iter()
            .flatten()
            .any(|k| matches!(k, ChatKind::Other(_)))
        {
            return Err(Box::new(invalid(format!("{path}/kinds"))));
        }
        if cfg
            .properties
            .iter()
            .flatten()
            .any(|p| !ENTRY_PROPERTIES.contains(&p.as_str()))
        {
            return Err(Box::new(invalid(format!("{path}/properties"))));
        }
        let unsupported = |u: &Option<UrgencyLevel>| {
            u.as_ref().is_some_and(|u| {
                u.to_urgency()
                    .is_none_or(|u| !cap.supported_urgency_values.contains(&u))
            })
        };
        if unsupported(&cfg.urgency) {
            return Err(Box::new(invalid(format!("{path}/urgency"))));
        }
        if unsupported(&cfg.mention_urgency) {
            return Err(Box::new(invalid(format!("{path}/mentionUrgency"))));
        }
    }
    Ok(())
}

/// Remove `chatPush` entries for accounts not in `caps` from a stored
/// subscription's `extra` map before it is returned.
///
/// An entry stored while the account advertised
/// `urn:ietf:params:jmap:chat:push` stops being returned once it no longer
/// does. draft-atwood-jmap-chat-push-00 rejects such an entry only on
/// `/set` (§PushSubscription Extension) and states no rule for one stored
/// before the capability was withdrawn. This server strips it so that
/// `/get` matches delivery: [`should_push`] treats the entry as absent, and
/// the draft says "When `chatPush` is absent or null for an account, the
/// server delivers standard `StateChange` notifications for that account
/// without inline message content."
pub fn strip_unsupported_chat_push(
    extra: &mut Map<String, Value>,
    caps: &HashMap<Id, ChatPushCapability>,
) {
    if let Some(Value::Object(entries)) = extra.get_mut(CHAT_PUSH) {
        entries.retain(|account, _| caps.contains_key(account.as_str()));
    }
}

/// The `StateChange` to push to `sub` for the types in `changed`, or `None`
/// when nothing is left to send.
///
/// Wraps [`state_change_for`] (which filters by `types`) and drops the
/// `Message` type for each account in `carried`. The caller puts an
/// account in `carried` only when every Message change for it in `changed`
/// arrived in a `ChatMessagePush` delivered to `sub`; an edit, a destroy,
/// the owner's own message or a message [`should_push`] filtered out keeps
/// the account out, and `Message` stays in its `StateChange`.
/// draft-atwood-jmap-chat-push-00 §PushSubscription Extension: "A
/// `ChatMessagePush` stands in for the `StateChange` that the arrival of
/// the messages it carries would otherwise cause. Every other change in
/// the account is still announced by a standard `StateChange`, as
/// {{RFC8620}} Section 7 describes: this includes Message changes that the
/// payload does not carry (edits, destroys, the owner's own messages, and
/// messages excluded by `ChatPushConfig`, mute, or block) and changes to
/// every other data type."
///
/// A consumer falling back to a plain `StateChange` under rate limiting
/// (§Rate Limiting and Fallback) calls [`state_change_for`] directly.
pub fn chat_state_change_for(
    sub: &PushSubscription,
    changed: &BTreeMap<Id, BTreeMap<String, State>>,
    carried: &HashSet<Id>,
) -> Option<StateChange> {
    let filtered: BTreeMap<Id, BTreeMap<String, State>> = changed
        .iter()
        .map(|(account, types)| {
            let mut types = types.clone();
            if carried.contains(account) {
                types.remove("Message");
            }
            (account.clone(), types)
        })
        .collect();
    state_change_for(sub, &filtered)
}

/// The account owner's standing in the Chat at delivery time, which
/// decides the broadcast scopes that target them.
///
/// draft-atwood-jmap-chat-push-00 §ChatMessageEntry `mentionScopes`:
/// "`"here"` is included only when the owner satisfies the
/// deployment-defined "active" predicate at delivery time; `"admins"` is
/// included only when the owner satisfies the deployment-defined
/// "administrative authority" predicate at delivery time." Both predicates
/// are deployment-defined, so the caller evaluates them.
///
/// Build it with [`OwnerFlags::new`]; more flags may be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct OwnerFlags {
    /// The owner satisfies the deployment's "active" predicate.
    pub active: bool,
    /// The owner has administrative authority in the Chat.
    pub admin: bool,
}

impl OwnerFlags {
    /// The owner's flags from the two deployment-defined predicates.
    pub const fn new(active: bool, admin: bool) -> Self {
        Self { active, admin }
    }
}

/// The stored records one `ChatMessageEntry` is built from.
///
/// Build it with [`PushItem::new`] and set the optional fields after; more
/// fields may be added.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct PushItem<'a> {
    /// The new inbound message.
    pub message: &'a Message,
    /// The Chat containing the message.
    pub chat: &'a Chat,
    /// The sender's `ChatContact` on the account owner's contact list.
    pub sender: &'a ChatContact,
    /// The sender's `SpaceMember` record in the Space containing the
    /// Chat, when the Chat is a channel and the record exists. Ignored for
    /// other kinds.
    pub sender_member: Option<&'a SpaceMember>,
    /// The Space containing the Chat, when the Chat is a channel. Its
    /// `name` becomes `spaceName`. Ignored for other kinds.
    pub space: Option<&'a Space>,
}

impl<'a> PushItem<'a> {
    /// The required records, with `sender_member` and `space` unset.
    pub const fn new(message: &'a Message, chat: &'a Chat, sender: &'a ChatContact) -> Self {
        Self {
            message,
            chat,
            sender,
            sender_member: None,
            space: None,
        }
    }
}

/// A built `ChatMessagePush` and the Web Push urgency to deliver it with.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuiltPush {
    /// The payload to deliver to the push endpoint.
    pub payload: ChatMessagePush,
    /// The Web Push urgency for this delivery.
    pub urgency: Urgency,
}

/// Build the `ChatMessagePush` for one new inbound message under one
/// subscription's `ChatPushConfig` for `account_id`.
///
/// The caller decides whether to deliver at all (draft-atwood-jmap-chat-push-00
/// §Delivery) and passes `state`, the account's `Message` state after this
/// message was stored. §ChatMessagePush: "The server MUST set this field to
/// the post-delivery state".
///
/// The entry holds exactly the fields in `cfg.properties`, or the default
/// set when it is absent. §ChatPushConfig: "The `properties` list is
/// authoritative: the server returns exactly the requested fields." Each
/// field is derived as §ChatMessageEntry says, with these exceptions:
///
/// - `encrypted` comes from `bodyType`: `false` for `text/plain`,
///   `text/markdown` and `application/jmap-chat-rich`, otherwise `true`
///   (§End-to-End Encrypted Deployments). An encrypted entry never has
///   `hasMention`, `mentionScopes` or `bodySnippet`.
/// - `bodySnippet` is the body with control characters removed, cut to at
///   most `cap.maxSnippetBytes` bytes on a UTF-8 character boundary.
///   Markdown markup is left in, which misses the §ChatMessageEntry SHOULD
///   for a plaintext approximation. For `application/jmap-chat-rich` the
///   snippet is omitted even when requested: there is no span type to
///   render plaintext from.
/// - `senderDisplayName` treats an empty `login` as absent.
///
/// The urgency is `cfg.mentionUrgency` when it is set and the owner is
/// mentioned directly or by a targeting broadcast scope, otherwise
/// `cfg.urgency`, otherwise `"normal"` (§Urgency). The mention test uses
/// the computed mention, whether or not `properties` asks for
/// `hasMention` or `mentionScopes`. A chosen value outside RFC 8030
/// (`UrgencyLevel::Other`, which [`validate_chat_push`] rejects before a
/// config is stored) is also sent as `"normal"`, the urgency of a push
/// without the header (RFC 8030 §5.3).
pub fn build_chat_message_push(
    account_id: &Id,
    state: State,
    cfg: &ChatPushConfig,
    cap: &ChatPushCapability,
    owner: OwnerFlags,
    item: PushItem<'_>,
) -> BuiltPush {
    let msg = item.message;
    let kind = item.chat.kind.clone();
    let body_type = msg.body_type.as_str();
    let plaintext = is_plaintext(body_type);

    let mut mentioned = false;
    let mut entry = if plaintext {
        let has_mention = msg.mentions.iter().any(|m| m.id == *account_id);
        let scopes = mention_scopes(msg, owner);
        mentioned = has_mention || !scopes.is_empty();
        let snippet = (body_type != BodyType::Rich.as_str())
            .then(|| snippet(&msg.body, cap.max_snippet_bytes));
        ChatMessageEntry::plaintext(
            msg.id.clone(),
            msg.chat_id.clone(),
            kind.clone(),
            msg.sender_id.clone(),
            msg.sent_at.clone(),
            has_mention,
            scopes,
            snippet,
        )
    } else {
        ChatMessageEntry::encrypted(
            msg.id.clone(),
            msg.chat_id.clone(),
            kind.clone(),
            msg.sender_id.clone(),
            msg.sent_at.clone(),
        )
    };
    if kind != ChatKind::Direct {
        entry.chat_name = item.chat.name.clone();
    }
    if kind == ChatKind::Channel {
        entry.space_id = item.chat.space_id.clone();
        entry.space_name = item.space.map(|s| s.name.clone());
    }
    entry.sender_display_name = Some(sender_display_name(&kind, &item));

    let wanted = |name: &str| match &cfg.properties {
        Some(properties) => properties.iter().any(|p| p == name),
        None => DEFAULT_ENTRY_PROPERTIES.contains(&name),
    };
    entry.message_id = entry.message_id.take().filter(|_| wanted("messageId"));
    entry.chat_id = entry.chat_id.take().filter(|_| wanted("chatId"));
    entry.chat_kind = entry.chat_kind.take().filter(|_| wanted("chatKind"));
    entry.chat_name = entry.chat_name.take().filter(|_| wanted("chatName"));
    entry.space_id = entry.space_id.take().filter(|_| wanted("spaceId"));
    entry.space_name = entry.space_name.take().filter(|_| wanted("spaceName"));
    entry.sender_id = entry.sender_id.take().filter(|_| wanted("senderId"));
    entry.sender_display_name = entry
        .sender_display_name
        .take()
        .filter(|_| wanted("senderDisplayName"));
    entry.sent_at = entry.sent_at.take().filter(|_| wanted("sentAt"));
    entry.has_mention = entry.has_mention.take().filter(|_| wanted("hasMention"));
    entry.mention_scopes = entry
        .mention_scopes
        .take()
        .filter(|_| wanted("mentionScopes"));
    entry.encrypted = entry.encrypted.take().filter(|_| wanted("encrypted"));
    entry.body_snippet = entry.body_snippet.take().filter(|_| wanted("bodySnippet"));

    let urgency = cfg
        .mention_urgency
        .as_ref()
        .filter(|_| mentioned)
        .or(cfg.urgency.as_ref())
        .and_then(UrgencyLevel::to_urgency)
        .unwrap_or(Urgency::Normal);
    // ponytail: one message per payload, which §Delivery allows ("Contains
    // exactly one entry for a single new message") and §Batching and Retry
    // prefers ("at most one `ChatMessagePush` per message"). The upgrade
    // path is batching, chunked by `maxMessagesPerPush`.
    BuiltPush {
        payload: ChatMessagePush::new(account_id.clone(), state, vec![entry]),
        urgency,
    }
}

/// Fit `push` to a platform payload limit of `max_bytes` serialized JSON
/// bytes, returning the payloads to deliver in order. A `None` element
/// stands for one message whose entry cannot fit; the caller delivers a
/// `StateChange` for the `Message` data type in its place.
///
/// The limit is the delivery platform's; for Web Push,
/// jmap-push-platform-guide §Payload size: "Servers MUST keep plaintext
/// under **3993 bytes**", so pass 3992 there.
///
/// draft-atwood-jmap-chat-push-00 §Delivery: "If a single
/// `ChatMessageEntry` does not fit, the server SHOULD shorten or omit that
/// entry's `bodySnippet`; if the entry still does not fit, the server
/// delivers a `StateChange` for the `Message` data type in place of a
/// `ChatMessagePush` for that message." A `push` that fits is returned
/// unchanged. Otherwise each entry goes in a payload of its own, with the
/// same `accountId`, `state`, `unreadCount` and extension fields, so no message is lost
/// (§Delivery: "the server MUST deliver additional `ChatMessagePush`
/// payloads to cover the remainder"). An entry that does not fit alone
/// loses `bodySnippet`; no other field is ever removed.
// ponytail: the snippet is omitted, not shortened, since JSON escaping
// makes the fitting length a search. Upgrade path: trim it on a UTF-8
// boundary until the payload fits.
pub fn fit_chat_message_push(
    mut push: ChatMessagePush,
    max_bytes: usize,
) -> Vec<Option<ChatMessagePush>> {
    let fits = |p: &ChatMessagePush| serde_json::to_vec(p).is_ok_and(|v| v.len() <= max_bytes);
    if fits(&push) {
        return vec![Some(push)];
    }
    std::mem::take(&mut push.messages)
        .into_iter()
        .map(|entry| {
            let mut single = push.clone();
            single.messages = vec![entry];
            if !fits(&single) {
                single.messages[0].body_snippet = None;
            }
            fits(&single).then_some(single)
        })
        .collect()
}

/// Whether a `ChatMessagePush` for the new inbound message `msg` goes to
/// a subscription whose `chatPush` entry for `account_id` is `cfg`.
///
/// `caps` maps each account that advertises
/// `urn:ietf:params:jmap:chat:push` to its capability object; a `cfg` for
/// an account not in `caps` counts as absent and gets `false`
/// (draft-atwood-jmap-chat-push-00 §PushSubscription Extension). `owner`
/// is the owner's standing at delivery time, as for
/// [`build_chat_message_push`].
///
/// Returns `true` only when the Chat matches `cfg` and none of the
/// §Delivery exclusions applies:
///
/// - `cfg.kinds` and `cfg.chatIds` both match when present. §ChatPushConfig:
///   "If both `kinds` and `chatIds` are present, a Chat matches only if its
///   id appears in `chatIds` and its kind appears in `kinds`."
/// - The sender is not the owner: `senderId` is not `account_id`.
/// - The owner is still a member of the Chat. The account must hold the
///   Chat; for a group Chat its `members` must have an entry whose id is
///   `account_id`, and for a channel Chat its Space's `members` must.
/// - The Chat is not muted, unless the owner is targeted by one of the message's broadcast
///   mentions. Direct mentions never bypass a mute, and neither does an
///   encrypted message (§End-to-End Encrypted Deployments: "never bypasses
///   mute suppression"). When `muteUntil` is set, the Chat is muted until
///   that time whatever `muted` says, because draft-atwood-jmap-chat-00
///   §Chat `muteUntil` says "Muting expires at this time" and clearing
///   `muted` afterwards is only a SHOULD. Otherwise the Chat is muted when
///   `muted` is `true`. A `muteUntil` that is not a seconds-precision
///   UTCDate, or a system clock before 1970, counts as an active mute.
/// - The sender's `ChatContact` is not blocked, per
///   [`ChatBackend::is_contact_blocked`].
///
/// # Messages stored before the subscription
///
/// §Delivery: "The server MUST NOT deliver a `ChatMessagePush` in any of
/// the following cases: The message was stored before the
/// `PushSubscription` was created." This function cannot see creation
/// order. The caller MUST read the set of subscriptions under the same lock
/// or transaction as the message write, and call this function only for
/// subscriptions in that set, including when delivery fans out
/// asynchronously afterwards.
///
/// # Errors
///
/// Returns the backend's error if reading the Chat, its Space or the
/// sender's blocked status fails.
pub async fn should_push<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    cfg: &ChatPushConfig,
    caps: &HashMap<Id, ChatPushCapability>,
    msg: &Message,
    owner: OwnerFlags,
) -> Result<bool, B::Error> {
    if !caps.contains_key(account_id) || msg.sender_id == *account_id {
        return Ok(false);
    }
    if cfg
        .chat_ids
        .as_ref()
        .is_some_and(|ids| !ids.contains(&msg.chat_id))
    {
        return Ok(false);
    }
    let (chats, _) = backend
        .get_objects::<Chat>(
            caller,
            account_id,
            Some(std::slice::from_ref(&msg.chat_id)),
            None,
        )
        .await?;
    let Some(chat) = chats.into_iter().next() else {
        return Ok(false);
    };
    if cfg
        .kinds
        .as_ref()
        .is_some_and(|kinds| !kinds.contains(&chat.kind))
    {
        return Ok(false);
    }
    if !owner_is_member(backend, caller, account_id, &chat).await? {
        return Ok(false);
    }
    let bypass = is_plaintext(msg.body_type.as_str()) && !mention_scopes(msg, owner).is_empty();
    let muted = match &chat.mute_until {
        Some(until) => mute_active(until),
        None => chat.muted,
    };
    if muted && !bypass {
        return Ok(false);
    }
    Ok(!backend
        .is_contact_blocked(caller, account_id, &msg.sender_id)
        .await?)
}

/// Whether the owner of `account_id` is a current member of `chat`, its
/// own Chat record: a group lists the owner in `members`, a channel's
/// Space lists the owner, and a direct Chat always has its owner
/// (draft-atwood-jmap-chat-00 §Terminology: the owner's member entry is
/// the accountId).
pub(crate) async fn owner_is_member<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    chat: &Chat,
) -> Result<bool, B::Error> {
    let is_owner = |id: &Id| id == account_id;
    Ok(match chat.kind {
        ChatKind::Group => chat.members.iter().flatten().any(|m| is_owner(&m.id)),
        ChatKind::Channel => match &chat.space_id {
            Some(space_id) => backend
                .get_objects::<Space>(
                    caller,
                    account_id,
                    Some(std::slice::from_ref(space_id)),
                    None,
                )
                .await?
                .0
                .iter()
                .any(|space| space.members.iter().any(|m| is_owner(&m.id))),
            None => false,
        },
        _ => true,
    })
}

/// Whether a mute ending at `until` is still in effect: `until` is later
/// than now. draft-atwood-jmap-chat-00 §Chat `muteUntil`: "Muting expires
/// at this time." An unparseable `until` or clock counts as in effect.
fn mute_active(until: &UTCDate) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok());
    match (until.to_epoch_seconds(), now) {
        (Ok(until), Some(now)) => now < until,
        _ => true,
    }
}

/// Whether `body_type` is one of the plaintext body types that
/// draft-atwood-jmap-chat-push-00 §End-to-End Encrypted Deployments says
/// "MUST set `encrypted` to `false`"; every other type is encrypted.
fn is_plaintext(body_type: &str) -> bool {
    [BodyType::Plain, BodyType::Markdown, BodyType::Rich]
        .iter()
        .any(|t| t.as_str() == body_type)
}

/// The deduplicated `Message.broadcastMentions[]` scopes that target the
/// owner, in first-seen order. `"everyone"` always targets a Chat member;
/// unknown scopes never do.
fn mention_scopes(msg: &Message, owner: OwnerFlags) -> Vec<BroadcastScope> {
    let mut scopes: Vec<BroadcastScope> = Vec::new();
    for scope in msg.broadcast_mentions.iter().map(|b| &b.scope) {
        let targets_owner = match scope {
            BroadcastScope::Everyone => true,
            BroadcastScope::Here => owner.active,
            BroadcastScope::Admins => owner.admin,
            _ => false,
        };
        if targets_owner && !scopes.contains(scope) {
            scopes.push(scope.clone());
        }
    }
    scopes
}

/// `body` without control characters, cut to at most `max_bytes` bytes on
/// a UTF-8 character boundary.
fn snippet(body: &str, max_bytes: u64) -> String {
    let mut text: String = body.chars().filter(|c| !c.is_control()).collect();
    let mut end = usize::try_from(max_bytes).map_or(text.len(), |max| max.min(text.len()));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

/// The `senderDisplayName` chain from draft-atwood-jmap-chat-push-00
/// §ChatMessageEntry: the `SpaceMember.nick` (channels only), then
/// `ChatContact.displayName`, then a non-empty `ChatContact.login`, then
/// `ChatContact.identity`.
fn sender_display_name(kind: &ChatKind, item: &PushItem<'_>) -> String {
    let nick = item
        .sender_member
        .filter(|_| *kind == ChatKind::Channel)
        .and_then(|m| m.nick.clone());
    let login = Some(&item.sender.login).filter(|l| !l.is_empty()).cloned();
    nick.or_else(|| item.sender.display_name.clone())
        .or(login)
        .unwrap_or_else(|| item.sender.identity.clone())
}

/// An `invalidProperties` SetError naming `property`.
fn invalid(property: String) -> SetError {
    SetError::new(SetErrorType::InvalidProperties).with_properties([property])
}
