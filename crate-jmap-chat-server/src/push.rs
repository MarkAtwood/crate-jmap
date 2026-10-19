//! `chatPush` support for RFC 8620 push subscriptions
//! (draft-atwood-jmap-chat-push-00).
//!
//! The `chatPush` property lives in [`PushSubscription::extra`] as a map of
//! accountId to `ChatPushConfig`. A backend calls [`validate_chat_push`] on
//! every create and update, [`strip_unsupported_chat_push`] on every
//! subscription it returns from `list_push_subscriptions`, and builds
//! notifications with [`chat_state_change_for`].

use std::collections::HashMap;

use jmap_chat_types::{ChatKind, ChatPushCapability, ChatPushConfig, UrgencyLevel};
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

/// An `invalidProperties` SetError naming `property`.
fn invalid(property: String) -> SetError {
    SetError::new(SetErrorType::InvalidProperties).with_properties([property])
}
