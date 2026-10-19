//! `chatPush` support for RFC 8620 push subscriptions
//! (draft-atwood-jmap-chat-push-00).
//!
//! The `chatPush` property lives in [`PushSubscription::extra`] as a map of
//! accountId to `ChatPushConfig`. A backend calls [`validate_chat_push`] on
//! every create and update, [`strip_unsupported_chat_push`] on every
//! subscription it returns from `list_push_subscriptions`, builds
//! notifications with [`chat_state_change_for`], and builds the
//! `ChatMessagePush` for each new inbound message with
//! [`build_chat_message_push`].

use std::collections::HashMap;

use jmap_chat_types::{
    BodyType, Chat, ChatContact, ChatKind, ChatMessageEntry, ChatMessagePush, ChatPushCapability,
    ChatPushConfig, Message, Space, SpaceMember, UrgencyLevel,
};
use jmap_server::state_change_for;
use jmap_types::{Id, PushSubscription, State, StateChange};
use serde_json::{Map, Value};

use crate::{SetError, SetErrorType};

/// The `PushSubscription` property this extension adds.
const CHAT_PUSH: &str = "chatPush";

/// The `ChatMessageEntry` fields a server MUST support in `properties`
/// (draft-atwood-jmap-chat-push-00 §ChatPushConfig). `unreadCount` is not
/// in that list and is not built, so it is rejected like any unknown name.
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
///   §PushSubscription Extension: "Servers MUST NOT include `chatPush`
///   entries for accounts whose `accountCapabilities` does not include
///   `urn:ietf:params:jmap:chat:push`."
/// - `chatPush/<accountId>/kinds`: a value other than `"direct"`,
///   `"group"` or `"channel"`.
/// - `chatPush/<accountId>/properties`: a name that is not a
///   `ChatMessageEntry` field the server supports.
/// - `chatPush/<accountId>/urgency` and `.../mentionUrgency`: a value not
///   in the account's `supportedUrgencyValues`. The draft asks for an
///   `invalidArguments` SetError here; RFC 8620 defines `invalidArguments`
///   only as a method-level error (rfc8620.txt:1189), so this returns
///   `invalidProperties` instead.
#[allow(clippy::result_large_err)]
pub fn validate_chat_push(
    extra: &Map<String, Value>,
    caps: &HashMap<Id, ChatPushCapability>,
) -> Result<(), SetError> {
    let entries = match extra.get(CHAT_PUSH) {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::Object(entries)) => entries,
        Some(_) => return Err(invalid(CHAT_PUSH.to_owned())),
    };
    for (account, entry) in entries.iter().filter(|(_, e)| !e.is_null()) {
        let path = format!("{CHAT_PUSH}/{account}");
        let Some(cap) = caps.get(account.as_str()) else {
            return Err(invalid(path));
        };
        let cfg: ChatPushConfig =
            serde_json::from_value(entry.clone()).map_err(|_| invalid(path.clone()))?;
        if cfg
            .kinds
            .iter()
            .flatten()
            .any(|k| matches!(k, ChatKind::Other(_)))
        {
            return Err(invalid(format!("{path}/kinds")));
        }
        if cfg
            .properties
            .iter()
            .flatten()
            .any(|p| !ENTRY_PROPERTIES.contains(&p.as_str()))
        {
            return Err(invalid(format!("{path}/properties")));
        }
        let unsupported = |u: &Option<UrgencyLevel>| {
            u.as_ref()
                .is_some_and(|u| !cap.supported_urgency_values.contains(u))
        };
        if unsupported(&cfg.urgency) {
            return Err(invalid(format!("{path}/urgency")));
        }
        if unsupported(&cfg.mention_urgency) {
            return Err(invalid(format!("{path}/mentionUrgency")));
        }
    }
    Ok(())
}

/// Remove `chatPush` entries for accounts not in `caps` from a stored
/// subscription's `extra` map before it is returned.
///
/// An entry stored while the account advertised
/// `urn:ietf:params:jmap:chat:push` stops being returned once it no longer
/// does. draft-atwood-jmap-chat-push-00 §PushSubscription Extension:
/// "Servers MUST NOT include `chatPush` entries for accounts whose
/// `accountCapabilities` does not include `urn:ietf:params:jmap:chat:push`."
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
/// `Message` type for every account with a live `chatPush` entry: one that
/// is non-null and whose account is in `caps`. For those accounts the
/// `ChatMessagePush` replaces the `Message` state change.
/// draft-atwood-jmap-chat-push-00 §PushSubscription Extension: "When
/// present for an account, the server delivers `ChatMessagePush` payloads
/// to the push endpoint for new inbound messages in that account that match
/// the configuration. When absent or null for an account, the server
/// delivers standard `StateChange` notifications for that account without
/// inline message content."
///
/// A consumer falling back to a plain `StateChange` under rate limiting
/// (§Rate Limiting and Fallback) calls [`state_change_for`] directly.
pub fn chat_state_change_for(
    sub: &PushSubscription,
    changed: &HashMap<Id, HashMap<String, State>>,
    caps: &HashMap<Id, ChatPushCapability>,
) -> Option<StateChange> {
    let live = |account: &Id| {
        caps.contains_key(account)
            && sub
                .extra
                .get(CHAT_PUSH)
                .and_then(|entries| entries.get(account.as_ref()))
                .is_some_and(|entry| !entry.is_null())
    };
    let filtered: HashMap<Id, HashMap<String, State>> = changed
        .iter()
        .map(|(account, types)| {
            let mut types = types.clone();
            if live(account) {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OwnerFlags {
    /// The owner satisfies the deployment's "active" predicate.
    pub active: bool,
    /// The owner has administrative authority in the Chat.
    pub admin: bool,
}

/// The stored records one `ChatMessageEntry` is built from.
#[derive(Debug, Clone, Copy)]
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

/// A built `ChatMessagePush` and the Web Push urgency to deliver it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltPush {
    /// The payload to deliver to the push endpoint.
    pub payload: ChatMessagePush,
    /// The Web Push urgency for this delivery.
    pub urgency: UrgencyLevel,
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
/// `hasMention` or `mentionScopes`.
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
    let plaintext = [BodyType::Plain, BodyType::Markdown, BodyType::Rich]
        .iter()
        .any(|t| t.as_str() == body_type);

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
        .clone()
        .filter(|_| mentioned)
        .or_else(|| cfg.urgency.clone())
        .unwrap_or(UrgencyLevel::Normal);
    // ponytail: one message per payload, which §Delivery allows ("Contains
    // exactly one entry for a single new message") and §Batching and Retry
    // prefers ("at most one `ChatMessagePush` per message"). The upgrade
    // path is batching, chunked by `maxMessagesPerPush`.
    BuiltPush {
        payload: ChatMessagePush::new(account_id.clone(), state, vec![entry]),
        urgency,
    }
}

/// The deduplicated `Message.broadcastMentions[]` scopes that target the
/// owner, in first-seen order. `"everyone"` always targets a Chat member;
/// unknown scopes never do.
fn mention_scopes(msg: &Message, owner: OwnerFlags) -> Vec<String> {
    let mut scopes: Vec<String> = Vec::new();
    for scope in msg.broadcast_mentions.iter().map(|b| b.scope.as_str()) {
        let targets_owner = match scope {
            "everyone" => true,
            "here" => owner.active,
            "admins" => owner.admin,
            _ => false,
        };
        if targets_owner && !scopes.iter().any(|s| s == scope) {
            scopes.push(scope.to_owned());
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
