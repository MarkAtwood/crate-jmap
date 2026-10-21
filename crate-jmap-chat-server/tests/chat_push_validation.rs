//! chatPush validation, capability strip and the StateChange/ChatMessagePush
//! split (bd:JMAP-nbk3.5).
//!
//! Oracle: draft-atwood-jmap-chat-push-00 at jmap-chat-spec commit cefc217,
//! section "PushSubscription Extension" / "ChatPushConfig", plus the
//! approved JMAP-nbk3 plan decisions D4 (bad urgency is `invalidProperties`
//! naming `chatPush/<accountId>/<field>`) and D9 (unknown `kinds` and
//! `properties` values are `invalidProperties`). The StateChange split
//! follows jmap-chat-spec commit 6bcd9d8, push:108. Each test quotes the
//! sentence it enforces.

use std::collections::{BTreeMap, HashMap, HashSet};

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

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension" (push:110):
/// "If a `PushSubscription/set` create or update includes a `chatPush`
/// entry for an account whose `accountCapabilities` does not include
/// `urn:ietf:params:jmap:chat:push`, the server MUST return an
/// `invalidProperties` SetError whose `properties` names the offending
/// entry (e.g., `chatPush/<id>`) and MUST NOT store the subscription
/// change."
/// The entry for such an account is rejected rather than stored.
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
/// server MUST return an `invalidProperties` SetError on
/// `PushSubscription/set` whose `properties` names the offending path
/// (e.g., `chatPush/<id>/urgency`) and MUST NOT store the configuration."
/// "low" is a valid RFC 8030 urgency
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
/// `supportedUrgencyValues`. ... The same `invalidProperties` rule applies
/// as for `urgency`, naming `chatPush/<id>/mentionUrgency`."
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

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension" (push:110):
/// "If a `PushSubscription/set` create or update includes a `chatPush`
/// entry for an account whose `accountCapabilities` does not include
/// `urn:ietf:params:jmap:chat:push`, the server MUST return an
/// `invalidProperties` SetError whose `properties` names the offending
/// entry (e.g., `chatPush/<id>`) and MUST NOT store the subscription
/// change."
/// The draft states no rule for an entry stored while the account had the
/// capability; this server strips it on `/get` once the account no longer
/// advertises it, because `should_push` then treats it as absent.
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

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension"
/// (6bcd9d8 push:108): "A `ChatMessagePush` stands in for the
/// `StateChange` that the arrival of the messages it carries would
/// otherwise cause. Every other change in the account is still announced
/// by a standard `StateChange`, as {{RFC8620}} Section 7 describes: this
/// includes Message changes that the payload does not carry [...] and
/// changes to every other data type."
///
/// Every Message change in `PUSH_ACCOUNT` was carried, so `Message` is
/// dropped from that account's TypeState; its other types and the other
/// account are kept.
#[test]
fn state_change_drops_message_for_chat_push_account() {
    let sub: PushSubscription = serde_json::from_value(json!({
        "deviceClientId": "device-1",
        "url": "https://push.example.com/endpoint/1",
        "types": null,
        "chatPush": { PUSH_ACCOUNT: {} },
    }))
    .expect("valid PushSubscription");
    let changed = BTreeMap::from([
        (
            id(PUSH_ACCOUNT),
            BTreeMap::from([
                ("Message".to_owned(), state("m1")),
                ("Chat".to_owned(), state("c1")),
            ]),
        ),
        (
            id(PLAIN_ACCOUNT),
            BTreeMap::from([("Message".to_owned(), state("m2"))]),
        ),
    ]);
    let carried = HashSet::from([id(PUSH_ACCOUNT)]);
    let sc = chat_state_change_for(&sub, &changed, &carried).expect("other types still change");
    let expected = BTreeMap::from([
        (
            id(PUSH_ACCOUNT),
            BTreeMap::from([("Chat".to_owned(), state("c1"))]),
        ),
        (
            id(PLAIN_ACCOUNT),
            BTreeMap::from([("Message".to_owned(), state("m2"))]),
        ),
    ]);
    assert_eq!(sc.changed, expected);
}

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension"
/// (6bcd9d8 push:108), as above: when the only change in the account is
/// carried Message arrivals, the `ChatMessagePush` "stands in for the
/// `StateChange`" and none is built.
#[test]
fn state_change_is_none_when_only_message_changed_in_chat_push_account() {
    let sub: PushSubscription = serde_json::from_value(json!({
        "deviceClientId": "device-1",
        "url": "https://push.example.com/endpoint/1",
        "types": null,
        "chatPush": { PUSH_ACCOUNT: {} },
    }))
    .expect("valid PushSubscription");
    let changed = BTreeMap::from([(
        id(PUSH_ACCOUNT),
        BTreeMap::from([("Message".to_owned(), state("m1"))]),
    )]);
    let carried = HashSet::from([id(PUSH_ACCOUNT)]);
    assert!(chat_state_change_for(&sub, &changed, &carried).is_none());
}

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension"
/// (6bcd9d8 push:108): "Every other change in the account is still
/// announced by a standard `StateChange`, as {{RFC8620}} Section 7
/// describes: this includes Message changes that the payload does not
/// carry (edits, destroys, the owner's own messages, and messages excluded
/// by `ChatPushConfig`, mute, or block)".
///
/// The account has a live `chatPush` entry, but its Message change (an
/// edit, say) was not carried by any `ChatMessagePush`, so `Message` stays.
#[test]
fn state_change_keeps_uncarried_message_for_chat_push_account() {
    let sub: PushSubscription = serde_json::from_value(json!({
        "deviceClientId": "device-1",
        "url": "https://push.example.com/endpoint/1",
        "types": null,
        "chatPush": { PUSH_ACCOUNT: {} },
    }))
    .expect("valid PushSubscription");
    let changed = BTreeMap::from([(
        id(PUSH_ACCOUNT),
        BTreeMap::from([("Message".to_owned(), state("m1"))]),
    )]);
    let sc = chat_state_change_for(&sub, &changed, &HashSet::new())
        .expect("uncarried Message change is announced");
    assert_eq!(sc.changed, changed);
}
