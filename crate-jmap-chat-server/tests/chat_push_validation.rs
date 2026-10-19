//! chatPush validation, capability strip and the StateChange/ChatMessagePush
//! split (bd:JMAP-nbk3.5).
//!
//! Oracle: draft-atwood-jmap-chat-push-00 at jmap-chat-spec commit cefc217,
//! section "PushSubscription Extension" / "ChatPushConfig", plus the
//! approved JMAP-nbk3 plan decisions D4 (bad urgency is `invalidProperties`
//! naming `chatPush/<accountId>/<field>`), D9 (unknown `kinds` and
//! `properties` values are `invalidProperties`) and D10 (an account with a
//! live `chatPush` entry gets no `Message` type in its `StateChange`). Each
//! test quotes the sentence it enforces.

use std::collections::HashMap;

use jmap_chat_server::{chat_state_change_for, strip_unsupported_chat_push, validate_chat_push};
use jmap_chat_types::ChatPushCapability;
use jmap_server::SetErrorType;
use jmap_types::{Id, PushSubscription, State};
use serde_json::{json, Map, Value};

const PUSH_ACCOUNT: &str = "acct-push";
const PLAIN_ACCOUNT: &str = "acct-plain";

fn id(s: &str) -> Id {
    serde_json::from_value(json!(s)).expect("valid Id")
}

fn state(s: &str) -> State {
    serde_json::from_value(json!(s)).expect("valid State")
}

/// `PUSH_ACCOUNT` advertises `urn:ietf:params:jmap:chat:push` with the
/// minimum urgency set the draft requires ("normal" and "high");
/// `PLAIN_ACCOUNT` does not advertise it.
fn caps() -> HashMap<Id, ChatPushCapability> {
    let cap: ChatPushCapability = serde_json::from_value(json!({
        "maxSnippetBytes": 256,
        "supportedUrgencyValues": ["normal", "high"],
    }))
    .expect("valid ChatPushCapability");
    HashMap::from([(id(PUSH_ACCOUNT), cap)])
}

/// A `PushSubscription.extra` map carrying `chatPush`.
fn extra_with(chat_push: Value) -> Map<String, Value> {
    match json!({ "chatPush": chat_push }) {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

fn assert_invalid(extra: &Map<String, Value>, property: &str) {
    let err = validate_chat_push(extra, &caps()).expect_err("config must be rejected");
    assert_eq!(err.error_type, SetErrorType::InvalidProperties);
    assert_eq!(err.properties, Some(vec![property.to_owned()]));
}

/// Positive control: a well-formed config for an account that advertises
/// the capability is accepted, so the rejections below are about the
/// offending field and not about the config as a whole.
#[test]
fn valid_config_is_accepted() {
    let extra = extra_with(json!({
        PUSH_ACCOUNT: {
            "kinds": ["direct", "group", "channel"],
            "properties": ["messageId", "chatId", "hasMention"],
            "urgency": "normal",
            "mentionUrgency": "high",
        }
    }));
    validate_chat_push(&extra, &caps()).expect("valid config is accepted");
}

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension" (push:108):
/// "Servers MUST NOT include `chatPush` entries for accounts whose
/// `accountCapabilities` does not include `urn:ietf:params:jmap:chat:push`."
/// On `/set` the entry for such an account is rejected rather than stored.
#[test]
fn chat_push_for_account_without_capability_is_rejected() {
    let extra = extra_with(json!({ PLAIN_ACCOUNT: { "urgency": "normal" } }));
    let err = validate_chat_push(&extra, &caps()).expect_err("entry must be rejected");
    assert_eq!(err.error_type, SetErrorType::InvalidProperties);
    let props = err.properties.expect("offending property is named");
    assert!(
        props.iter().any(|p| p.starts_with("chatPush")),
        "properties {props:?} do not name chatPush"
    );
}

/// draft-atwood-jmap-chat-push-00 §"ChatPushConfig" (push:124), on
/// `urgency`: "MUST be one of the values in `supportedUrgencyValues`. ...
/// If the client supplies a value not in `supportedUrgencyValues`, the
/// server MUST return an `invalidArguments` SetError on
/// `PushSubscription/set` and MUST NOT store the configuration."
///
/// Per plan decision D4 the SetError is `invalidProperties` naming
/// `chatPush/<accountId>/urgency` (RFC 8620 defines `invalidArguments` only
/// at method level, rfc8620.txt:1189). "low" is a valid RFC 8030 urgency
/// that this account does not support; "urgent" is not an urgency at all.
#[test]
fn urgency_outside_supported_values_is_rejected() {
    for bad in ["low", "urgent"] {
        let extra = extra_with(json!({ PUSH_ACCOUNT: { "urgency": bad } }));
        assert_invalid(&extra, &format!("chatPush/{PUSH_ACCOUNT}/urgency"));
    }
}

/// draft-atwood-jmap-chat-push-00 §"ChatPushConfig" (push:127), on
/// `mentionUrgency`: "MUST be one of the values in
/// `supportedUrgencyValues`. ... The same `invalidArguments` rule applies
/// as for `urgency`." Error code per plan decision D4.
#[test]
fn mention_urgency_outside_supported_values_is_rejected() {
    for bad in ["very-low", "urgent"] {
        let extra = extra_with(json!({
            PUSH_ACCOUNT: { "urgency": "normal", "mentionUrgency": bad }
        }));
        assert_invalid(&extra, &format!("chatPush/{PUSH_ACCOUNT}/mentionUrgency"));
    }
}

/// draft-atwood-jmap-chat-push-00 §"ChatPushConfig" (push:115), on
/// `kinds`: "Each entry is one of `\"direct\"`, `\"group\"`, or
/// `\"channel\"`." Per plan decision D9 any other value is
/// `invalidProperties`.
#[test]
fn unknown_kind_is_rejected() {
    let extra = extra_with(json!({ PUSH_ACCOUNT: { "kinds": ["direct", "forum"] } }));
    assert_invalid(&extra, &format!("chatPush/{PUSH_ACCOUNT}/kinds"));
}

/// draft-atwood-jmap-chat-push-00 §"ChatPushConfig" (push:121), on
/// `properties`: "The `ChatMessageEntry` fields to include in each payload
/// entry." Per plan decision D9 a name that is not a `ChatMessageEntry`
/// field is `invalidProperties`.
#[test]
fn unknown_entry_property_is_rejected() {
    let extra = extra_with(json!({
        PUSH_ACCOUNT: { "properties": ["messageId", "notAField"] }
    }));
    assert_invalid(&extra, &format!("chatPush/{PUSH_ACCOUNT}/properties"));
}

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension" (push:108):
/// "Servers MUST NOT include `chatPush` entries for accounts whose
/// `accountCapabilities` does not include `urn:ietf:params:jmap:chat:push`."
/// On `/get`, an entry stored while the account had the capability is
/// stripped once the account no longer advertises it.
#[test]
fn strip_removes_entries_for_accounts_without_capability() {
    let mut extra = extra_with(json!({
        PUSH_ACCOUNT: { "urgency": "high" },
        PLAIN_ACCOUNT: { "urgency": "normal" },
    }));
    strip_unsupported_chat_push(&mut extra, &caps());
    assert_eq!(
        Value::Object(extra),
        json!({ "chatPush": { PUSH_ACCOUNT: { "urgency": "high" } } })
    );
}

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension" (push:106):
/// "When present for an account, the server delivers `ChatMessagePush`
/// payloads to the push endpoint for new inbound messages in that account
/// that match the configuration. When absent or null for an account, the
/// server delivers standard `StateChange` notifications for that account
/// without inline message content."
///
/// Per plan decision D10, the `ChatMessagePush` replaces the `Message`
/// state change for a live `chatPush` account: `Message` is dropped from
/// that account's TypeState, its other types and other accounts are kept.
#[test]
fn state_change_drops_message_for_chat_push_account() {
    let sub: PushSubscription = serde_json::from_value(json!({
        "deviceClientId": "device-1",
        "url": "https://push.example.com/endpoint/1",
        "types": null,
        "chatPush": { PUSH_ACCOUNT: {} },
    }))
    .expect("valid PushSubscription");
    let changed = HashMap::from([
        (
            id(PUSH_ACCOUNT),
            HashMap::from([
                ("Message".to_owned(), state("m1")),
                ("Chat".to_owned(), state("c1")),
            ]),
        ),
        (
            id(PLAIN_ACCOUNT),
            HashMap::from([("Message".to_owned(), state("m2"))]),
        ),
    ]);
    let sc = chat_state_change_for(&sub, &changed, &caps()).expect("other types still change");
    let expected = HashMap::from([
        (
            id(PUSH_ACCOUNT),
            HashMap::from([("Chat".to_owned(), state("c1"))]),
        ),
        (
            id(PLAIN_ACCOUNT),
            HashMap::from([("Message".to_owned(), state("m2"))]),
        ),
    ]);
    assert_eq!(sc.changed, expected);
}

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension" (push:106),
/// as above: when only `Message` changed in a live `chatPush` account, the
/// `ChatMessagePush` is the whole notification and no `StateChange` is
/// built (plan decision D10).
#[test]
fn state_change_is_none_when_only_message_changed_in_chat_push_account() {
    let sub: PushSubscription = serde_json::from_value(json!({
        "deviceClientId": "device-1",
        "url": "https://push.example.com/endpoint/1",
        "types": null,
        "chatPush": { PUSH_ACCOUNT: {} },
    }))
    .expect("valid PushSubscription");
    let changed = HashMap::from([(
        id(PUSH_ACCOUNT),
        HashMap::from([("Message".to_owned(), state("m1"))]),
    )]);
    assert!(chat_state_change_for(&sub, &changed, &caps()).is_none());
}
