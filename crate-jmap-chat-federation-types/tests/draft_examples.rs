//! The `Peer/*` request and response examples of
//! draft-atwood-jmap-chat-federation-00 at jmap-chat-spec c08568e
//! ("fed Lnnn" cites draft-atwood-jmap-chat-federation-00.md at that
//! revision).
//!
//! Oracle: each example's JSON text, copied verbatim from the draft. Each
//! test parses the whole request or response as the RFC 8620 envelope,
//! checks it serializes back to the same JSON, then parses the method
//! arguments into this crate's type and checks they serialize back to the
//! draft's arguments. Every type has a flattened `extra` catch-all that
//! would also round-trip a key a typed field fails to claim, so each test
//! also asserts `extra` holds only `recipientUserId` on the arguments and
//! nothing on every other parsed struct.
//!
//! `recipientUserId` (fed L404) has no typed field in the 0.4 argument
//! types; it survives the round trip through `extra`. Typed support and
//! recipient routing are JMAP-ucha (0.5).

use jmap_chat_federation_types::*;
use jmap_types::{JmapError, JmapRequest, JmapResponse, MethodErrorType};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Parse `json` as `T`, assert it serializes back to the same JSON value,
/// and return the parsed form.
fn round_trip<T: Serialize + DeserializeOwned>(wire: &Value) -> T {
    let parsed: T = serde_json::from_value(wire.clone()).expect("draft JSON parses");
    assert_eq!(&serde_json::to_value(&parsed).expect("serialize"), wire);
    parsed
}

/// Assert the only draft key no typed field claimed is `recipientUserId`.
fn only_recipient(extra: &serde_json::Map<String, Value>) {
    assert_eq!(extra.keys().collect::<Vec<_>>(), ["recipientUserId"]);
}

/// The arguments of the single method call `name` with call id `"c1"` in
/// the request example `json`, round-tripped through `T`.
fn request<T: Serialize + DeserializeOwned>(json: &str, name: &str) -> T {
    let wire: Value = serde_json::from_str(json).expect("draft JSON");
    let req: JmapRequest = round_trip(&wire);
    assert_eq!(
        req.using,
        ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]
    );
    let [call] = req.method_calls.as_slice() else {
        panic!("one method call");
    };
    assert_eq!((call.name.as_str(), call.call_id.as_str()), (name, "c1"));
    round_trip(&call.arguments)
}

/// The arguments of the single method response `name` with call id
/// `"c1"` and the session state `state` in the response example `json`,
/// round-tripped through `T`.
fn response<T: Serialize + DeserializeOwned>(json: &str, name: &str, state: &str) -> T {
    let wire: Value = serde_json::from_str(json).expect("draft JSON");
    let resp: JmapResponse = round_trip(&wire);
    assert_eq!(resp.session_state.as_ref(), state);
    let [call] = resp.method_responses.as_slice() else {
        panic!("one method response");
    };
    assert_eq!((call.name.as_str(), call.call_id.as_str()), (name, "c1"));
    round_trip(&call.arguments)
}

/// fed L222-L296: the signed Peer/deliver of a new message, and its
/// response.
#[test]
fn deliver_new_message() {
    let args: PeerDeliverArgs = request(DELIVER_MESSAGE_REQUEST, "Peer/deliver");
    only_recipient(&args.extra);
    assert_eq!(args.sender_user_id, "alice@alice.example");
    let message = args.message.expect("message");
    assert!(message.extra.is_empty());
    assert_eq!(message.sender_msg_id, "01M66W1ZNG06Y60M4QKTWMP8P8");
    assert!(message.attachments.is_empty());
    let mentions = message.mentions.expect("mentions");
    assert!(mentions.iter().all(|m| m.extra.is_empty()));
    assert_eq!(mentions[0].id, "bob@bob.example");
    assert!(args.edit.is_none() && args.reaction_update.is_none());

    let r: PeerDeliverResponse = response(DELIVER_MESSAGE_RESPONSE, "Peer/deliver", "b7q3");
    assert!(r.extra.is_empty());
    assert_eq!(r.received_msg_id, "01M66W22K8QSZ6YW5FNRGPAX4A");
    assert_eq!(r.received_at.as_ref(), "2026-10-30T17:00:01Z");
    assert!(r.canonical_chat_id.is_none());
}

/// fed L558-L594: Alice edits the message.
#[test]
fn deliver_edit() {
    let args: PeerDeliverArgs = request(DELIVER_EDIT_REQUEST, "Peer/deliver");
    only_recipient(&args.extra);
    let edit = args.edit.expect("edit");
    assert!(edit.extra.is_empty());
    assert_eq!(edit.body, "Lunch at 12:30, Bob?");
    assert_eq!(edit.edited_at.as_ref(), "2026-10-30T17:01:05Z");
    assert!(args.message.is_none() && args.reaction_update.is_none());

    let r: PeerDeliverResponse = response(DELIVER_EDIT_RESPONSE, "Peer/deliver", "b7q4");
    assert!(r.extra.is_empty());
    assert_eq!(r.received_msg_id, "01M66W22K8QSZ6YW5FNRGPAX4A");
}

/// fed L596-L632: Bob adds U+1F44D THUMBS UP SIGN; on `alice.example`
/// `receivedMsgId` equals the `senderMsgId`.
#[test]
fn deliver_reaction_update() {
    let args: PeerDeliverArgs = request(DELIVER_REACTION_REQUEST, "Peer/deliver");
    only_recipient(&args.extra);
    let reaction = args.reaction_update.expect("reactionUpdate");
    assert!(reaction.extra.is_empty());
    assert_eq!(reaction.emoji, "\u{1F44D}");
    assert_eq!(reaction.action, ReactionAction::Add);
    assert!(reaction.custom_emoji_id.is_none());

    let r: PeerDeliverResponse = response(DELIVER_REACTION_RESPONSE, "Peer/deliver", "a4k8");
    assert!(r.extra.is_empty());
    assert_eq!(r.received_msg_id, reaction.sender_msg_id);
}

/// fed L699-L730: Bob read the message.
#[test]
fn receipt() {
    let args: PeerReceiptArgs = request(RECEIPT_REQUEST, "Peer/receipt");
    only_recipient(&args.extra);
    assert_eq!(args.reader_user_id, "bob@bob.example");
    assert_eq!(
        args.read_at.expect("readAt").as_ref(),
        "2026-10-30T17:01:30Z"
    );
    assert!(args.device_delivered_at.is_none());

    let r: PeerAck = response(RECEIPT_RESPONSE, "Peer/receipt", "a4k7");
    assert!(r.extra.is_empty());
    assert_eq!(r.account_id, "P9c1");
}

/// fed L790-L819: Alice starts typing.
#[test]
fn typing() {
    let args: PeerTypingArgs = request(TYPING_REQUEST, "Peer/typing");
    only_recipient(&args.extra);
    assert!(args.typing);

    let r: PeerAck = response(TYPING_RESPONSE, "Peer/typing", "b7q3");
    assert!(r.extra.is_empty());
    assert_eq!(r.account_id, "P5f2");
}

/// fed L862-L892: Alice retracts the message.
#[test]
fn retract() {
    let args: PeerRetractArgs = request(RETRACT_REQUEST, "Peer/retract");
    only_recipient(&args.extra);
    assert_eq!(args.sender_msg_id, "01M66W1ZNG06Y60M4QKTWMP8P8");

    let r: PeerRetractResponse = response(RETRACT_RESPONSE, "Peer/retract", "b7q5");
    assert!(r.extra.is_empty());
    assert_eq!(r.retracted_at.as_ref(), "2026-10-30T17:05:12Z");
}

/// fed L965-L1005: Alice creates a group with Bob and Carol.
#[test]
fn group_update_create() {
    let args: PeerGroupUpdateArgs = request(GROUP_UPDATE_REQUEST, "Peer/groupUpdate");
    only_recipient(&args.extra);
    assert_eq!(args.action, GroupUpdateAction::Create);
    let members = args.members.expect("members");
    assert!(members.iter().all(|m| m.extra.is_empty()));
    assert_eq!(members.len(), 3);
    assert!(members[0].invited_by.is_none());
    assert_eq!(
        members[2].invited_by.as_deref(),
        Some("alice@alice.example")
    );
    assert!(args.avatar.is_none());

    let r: PeerAck = response(GROUP_UPDATE_RESPONSE, "Peer/groupUpdate", "b7q6");
    assert!(r.extra.is_empty());
    assert_eq!(r.account_id, "P5f2");
}

/// fed L1053-L1084: Bob subscribes to Alice's presence for an hour.
#[test]
fn subscribe_presence() {
    let args: PeerSubscribePresenceArgs =
        request(SUBSCRIBE_PRESENCE_REQUEST, "Peer/subscribePresence");
    only_recipient(&args.extra);
    assert_eq!(args.ttl, Some(3600));

    let r: PeerSubscribePresenceResponse = response(
        SUBSCRIBE_PRESENCE_RESPONSE,
        "Peer/subscribePresence",
        "a4k8",
    );
    assert!(r.extra.is_empty());
    assert_eq!(r.ttl, 3600);
    assert!(r.status_text.is_none() && r.status_emoji.is_none());
}

/// fed L1108-L1135: Bob cancels the subscription.
#[test]
fn unsubscribe_presence() {
    let args: PeerUnsubscribePresenceArgs =
        request(UNSUBSCRIBE_PRESENCE_REQUEST, "Peer/unsubscribePresence");
    only_recipient(&args.extra);
    assert_eq!(args.subscriber_id, "bob@bob.example");

    let r: PeerAck = response(
        UNSUBSCRIBE_PRESENCE_RESPONSE,
        "Peer/unsubscribePresence",
        "a4k8",
    );
    assert!(r.extra.is_empty());
    assert_eq!(r.account_id, "P9c1");
}

/// fed L1177-L1207: Alice's presence changes; `statusEmoji` is absent,
/// which leaves it unchanged (fed L1164).
#[test]
fn presence() {
    let args: PeerPresenceArgs = request(PRESENCE_REQUEST, "Peer/presence");
    only_recipient(&args.extra);
    assert_eq!(args.identity, "alice@alice.example");
    assert!(args.status_emoji.is_none());

    let r: PeerAck = response(PRESENCE_RESPONSE, "Peer/presence", "b7q6");
    assert!(r.extra.is_empty());
    assert_eq!(r.account_id, "P5f2");
}

/// fed L1248-L1275: a call for a mailbox the server does not host is
/// rejected with `forbidden`, with no `description`.
#[test]
fn forbidden_unknown_recipient() {
    let args: PeerTypingArgs = request(FORBIDDEN_REQUEST, "Peer/typing");
    only_recipient(&args.extra);
    assert!(args.typing);

    let e: JmapError = response(FORBIDDEN_RESPONSE, "error", "b7q3");
    assert_eq!(e.error_type, MethodErrorType::Forbidden);
    assert!(e.description.is_none());
}

/// fed L241-L261.
const DELIVER_MESSAGE_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/deliver", {
      "accountId": "P5f2",
      "recipientUserId": "bob@bob.example",
      "chatId": "01M63F1WH01Y0REYVGR2JXN81Y",
      "senderUserId": "alice@alice.example",
      "chatKind": "direct",
      "message": {
        "senderMsgId": "01M66W1ZNG06Y60M4QKTWMP8P8",
        "body": "Lunch at noon, Bob?",
        "bodyType": "text/plain",
        "sentAt": "2026-10-30T16:59:58Z",
        "attachments": [],
        "mentions": [{"id": "bob@bob.example"}]
      }
    }, "c1"]
  ]
}
"#;

/// fed L286-L295.
const DELIVER_MESSAGE_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/deliver", {
      "accountId": "P5f2",
      "receivedMsgId": "01M66W22K8QSZ6YW5FNRGPAX4A",
      "receivedAt": "2026-10-30T17:00:01Z"
    }, "c1"]
  ],
  "sessionState": "b7q3"
}
"#;

/// fed L561-L580.
const DELIVER_EDIT_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/deliver", {
      "accountId": "P5f2",
      "recipientUserId": "bob@bob.example",
      "chatId": "01M63F1WH01Y0REYVGR2JXN81Y",
      "senderUserId": "alice@alice.example",
      "chatKind": "direct",
      "edit": {
        "senderMsgId": "01M66W1ZNG06Y60M4QKTWMP8P8",
        "body": "Lunch at 12:30, Bob?",
        "bodyType": "text/plain",
        "editedAt": "2026-10-30T17:01:05Z",
        "mentions": [{"id": "bob@bob.example"}]
      }
    }, "c1"]
  ]
}
"#;

/// fed L584-L593.
const DELIVER_EDIT_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/deliver", {
      "accountId": "P5f2",
      "receivedMsgId": "01M66W22K8QSZ6YW5FNRGPAX4A",
      "receivedAt": "2026-10-30T17:01:06Z"
    }, "c1"]
  ],
  "sessionState": "b7q4"
}
"#;

/// fed L599-L618.
const DELIVER_REACTION_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/deliver", {
      "accountId": "P9c1",
      "recipientUserId": "alice@alice.example",
      "chatId": "01M63F1WH01Y0REYVGR2JXN81Y",
      "senderUserId": "bob@bob.example",
      "chatKind": "direct",
      "reactionUpdate": {
        "senderMsgId": "01M66W1ZNG06Y60M4QKTWMP8P8",
        "senderReactionId": "01M66W53907GDA344GN3NHT6HN",
        "emoji": "\ud83d\udc4d",
        "action": "add",
        "sentAt": "2026-10-30T17:01:40Z"
      }
    }, "c1"]
  ]
}
"#;

/// fed L622-L631.
const DELIVER_REACTION_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/deliver", {
      "accountId": "P9c1",
      "receivedMsgId": "01M66W1ZNG06Y60M4QKTWMP8P8",
      "receivedAt": "2026-10-30T17:01:41Z"
    }, "c1"]
  ],
  "sessionState": "a4k8"
}
"#;

/// fed L704-L718.
const RECEIPT_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/receipt", {
      "accountId": "P9c1",
      "recipientUserId": "alice@alice.example",
      "senderMsgId": "01M66W1ZNG06Y60M4QKTWMP8P8",
      "deliveredAt": "2026-10-30T17:00:01Z",
      "readAt": "2026-10-30T17:01:30Z",
      "readDisposition": "displayed",
      "readerUserId": "bob@bob.example"
    }, "c1"]
  ]
}
"#;

/// fed L722-L729.
const RECEIPT_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/receipt", {
      "accountId": "P9c1"
    }, "c1"]
  ],
  "sessionState": "a4k7"
}
"#;

/// fed L795-L807.
const TYPING_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/typing", {
      "accountId": "P5f2",
      "recipientUserId": "bob@bob.example",
      "chatId": "01M63F1WH01Y0REYVGR2JXN81Y",
      "senderUserId": "alice@alice.example",
      "typing": true
    }, "c1"]
  ]
}
"#;

/// fed L811-L818.
const TYPING_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/typing", {
      "accountId": "P5f2"
    }, "c1"]
  ],
  "sessionState": "b7q3"
}
"#;

/// fed L867-L879.
const RETRACT_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/retract", {
      "accountId": "P5f2",
      "recipientUserId": "bob@bob.example",
      "chatId": "01M63F1WH01Y0REYVGR2JXN81Y",
      "senderUserId": "alice@alice.example",
      "senderMsgId": "01M66W1ZNG06Y60M4QKTWMP8P8"
    }, "c1"]
  ]
}
"#;

/// fed L883-L891.
const RETRACT_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/retract", {
      "accountId": "P5f2",
      "retractedAt": "2026-10-30T17:05:12Z"
    }, "c1"]
  ],
  "sessionState": "b7q5"
}
"#;

/// fed L970-L993.
const GROUP_UPDATE_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/groupUpdate", {
      "accountId": "P5f2",
      "recipientUserId": "bob@bob.example",
      "chatId": "01M66WMBJ06WNHVQK3HVZ7ZF52",
      "senderUserId": "alice@alice.example",
      "action": "create",
      "name": "Lunch crew",
      "members": [
        {"id": "alice@alice.example", "role": "admin",
         "joinedAt": "2026-10-30T17:10:00Z"},
        {"id": "bob@bob.example", "role": "member",
         "joinedAt": "2026-10-30T17:10:00Z",
         "invitedBy": "alice@alice.example"},
        {"id": "carol@carol.example", "role": "member",
         "joinedAt": "2026-10-30T17:10:00Z",
         "invitedBy": "alice@alice.example"}
      ]
    }, "c1"]
  ]
}
"#;

/// fed L997-L1004.
const GROUP_UPDATE_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/groupUpdate", {
      "accountId": "P5f2"
    }, "c1"]
  ],
  "sessionState": "b7q6"
}
"#;

/// fed L1058-L1069.
const SUBSCRIBE_PRESENCE_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/subscribePresence", {
      "accountId": "P9c1",
      "recipientUserId": "alice@alice.example",
      "subscriberId": "bob@bob.example",
      "ttl": 3600
    }, "c1"]
  ]
}
"#;

/// fed L1073-L1083.
const SUBSCRIBE_PRESENCE_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/subscribePresence", {
      "accountId": "P9c1",
      "presence": "online",
      "lastActiveAt": "2026-10-30T17:11:20Z",
      "ttl": 3600
    }, "c1"]
  ],
  "sessionState": "a4k8"
}
"#;

/// fed L1113-L1123.
const UNSUBSCRIBE_PRESENCE_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/unsubscribePresence", {
      "accountId": "P9c1",
      "recipientUserId": "alice@alice.example",
      "subscriberId": "bob@bob.example"
    }, "c1"]
  ]
}
"#;

/// fed L1127-L1134.
const UNSUBSCRIBE_PRESENCE_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/unsubscribePresence", {
      "accountId": "P9c1"
    }, "c1"]
  ],
  "sessionState": "a4k8"
}
"#;

/// fed L1182-L1195.
const PRESENCE_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/presence", {
      "accountId": "P5f2",
      "recipientUserId": "bob@bob.example",
      "identity": "alice@alice.example",
      "presence": "away",
      "lastActiveAt": "2026-10-30T17:25:00Z",
      "statusText": "At lunch"
    }, "c1"]
  ]
}
"#;

/// fed L1199-L1206.
const PRESENCE_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["Peer/presence", {
      "accountId": "P5f2"
    }, "c1"]
  ],
  "sessionState": "b7q6"
}
"#;

/// fed L1251-L1263.
const FORBIDDEN_REQUEST: &str = r#"
{
  "using": ["urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat"],
  "methodCalls": [
    ["Peer/typing", {
      "accountId": "P5f2",
      "recipientUserId": "nobody@bob.example",
      "chatId": "01M63F1WH01Y0REYVGR2JXN81Y",
      "senderUserId": "alice@alice.example",
      "typing": true
    }, "c1"]
  ]
}
"#;

/// fed L1267-L1274.
const FORBIDDEN_RESPONSE: &str = r#"
{
  "methodResponses": [
    ["error", {
      "type": "forbidden"
    }, "c1"]
  ],
  "sessionState": "b7q3"
}
"#;
