//! `Peer/receipt`, `Peer/typing`, `Peer/retract` and `Peer/groupUpdate`
//! (draft-atwood-jmap-chat-federation-00 §Peer/receipt, §Peer/typing,
//! §Peer/retract, §Peer/groupUpdate).
//!
//! "fed Lnnn" cites draft-atwood-jmap-chat-federation-00.md and "chat
//! Lnnn" draft-atwood-jmap-chat-00.md, both at jmap-chat-spec be46208.
//! Oracle: the quoted draft text and hand-written JSON records.

mod common;

use common::*;
use jmap_chat_types::{Chat, ChatContact, Message};
use serde_json::{json, Value};

const DAVE: &str = "dave@peer.example";
const ERIN: &str = "erin@far.example";
const SMID: &str = "01J00000000000000000000SM1";
const READ_AT: &str = "2001-01-01T00:00:00Z";
const DELIVERED_AT: &str = "2000-06-01T00:00:00Z";

fn ack() -> Value {
    json!({"accountId": ACCOUNT})
}

/// A group Chat whose members are `(ChatContact id, role)` pairs.
fn group(members: &[(&str, &str)]) -> Value {
    let mut chat = group_chat(&[], false);
    chat["members"] = members
        .iter()
        .map(|(id, role)| json!({"id": id, "role": role, "joinedAt": PAST}))
        .collect();
    chat
}

/// The owner's Message `SMID` in `chat_id`, not yet delivered.
fn owner_message(chat_id: &str) -> Value {
    let mut m = stored_message("m1", SMID, chat_id, ACCOUNT, PAST);
    m["deliveryState"] = json!("pending");
    m
}

fn presence_status(receipt_sharing: bool) -> Value {
    json!({
        "id": "presence",
        "presence": "online",
        "receiptSharing": receipt_sharing,
        "updatedAt": PAST,
    })
}

fn receipt_args(extra: Value) -> Value {
    let mut args = json!({
        "accountId": ACCOUNT,
        "senderMsgId": SMID,
        "readerUserId": BOB,
    });
    args.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    args
}

async fn message(fed: &Fed, id: &str) -> Value {
    let m = fed
        .messages()
        .await
        .into_iter()
        .find(|m: &Message| m.id.as_ref() == id)
        .expect("stored message");
    serde_json::to_value(m).expect("Message serializes")
}

async fn chat(fed: &Fed, id: &str) -> Option<Value> {
    fed.chats()
        .await
        .into_iter()
        .find(|c: &Chat| c.id.as_ref() == id)
        .map(|c| serde_json::to_value(c).expect("Chat serializes"))
}

/// A Fed whose authenticated peer is `identity` at [`PEER_DOMAIN`].
fn fed_as(identity: &str) -> Fed {
    let mut fed = Fed::new();
    fed.peer = Some(jmap_chat_federation_server::AuthenticatedPeer::new(
        jmap_peer_auth::PeerDomain::parse(PEER_DOMAIN).expect("domain fixture"),
        identity,
    ));
    fed
}

// ---------------------------------------------------------------------------
// All four methods
// ---------------------------------------------------------------------------

/// fed L306: "Callers without the `"peer"` role MUST receive `forbidden`
/// for all methods in this section."
#[tokio::test]
async fn unauthenticated_caller_is_forbidden_by_every_method() {
    let mut fed = Fed::new();
    fed.peer = None;
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed("Message", owner_message(LOW));
    assert_error(fed.receipt(receipt_args(json!({}))).await, "forbidden");
    assert_error(
        fed.typing(
            json!({"accountId": ACCOUNT, "chatId": LOW, "senderUserId": BOB, "typing": true}),
        )
        .await,
        "forbidden",
    );
    assert_error(
        fed.retract(
            json!({"accountId": ACCOUNT, "chatId": LOW, "senderUserId": BOB, "senderMsgId": SMID}),
        )
        .await,
        "forbidden",
    );
    assert_error(
        fed.group_update(
            json!({"accountId": ACCOUNT, "chatId": GROUP, "senderUserId": BOB, "action": "leave"}),
        )
        .await,
        "forbidden",
    );
}

// ---------------------------------------------------------------------------
// Peer/receipt
// ---------------------------------------------------------------------------

/// fed L472: "The `readerUserId` MUST match the peer identity of the
/// calling server; the receiver MUST reject the request with `forbidden`
/// if they differ."
#[tokio::test]
async fn receipt_reader_must_be_the_peer_identity() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "admin"), (BOB_ID, "member")]));
    fed.seed("Message", owner_message(GROUP));
    let mut args = receipt_args(json!({"readAt": READ_AT}));
    args["readerUserId"] = json!(CAROL);
    assert_error(fed.receipt(args).await, "forbidden");
    assert_eq!(message(&fed, "m1").await.get("deliveryReceipts"), None);
}

/// fed L472: "the receiver updates the per-recipient delivery receipt in
/// `deliveryReceipts` under that user's local ChatContact `id`"; fed
/// L469: "If `readAt` is present and `readDisposition` is absent, the
/// receiving server MUST store `"displayed"`"; fed L790: `"received"` —
/// "The message has been explicitly read by the recipient (indicated via
/// `Peer/receipt`)"; fed L505: "`Peer/receipt` SHOULD return a normal
/// success response".
#[tokio::test]
async fn receipt_read_is_stored_under_the_reader_contact_id() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "admin"), (BOB_ID, "member")]));
    fed.seed("Message", owner_message(GROUP));
    let response = fed
        .receipt(receipt_args(
            json!({"deliveredAt": DELIVERED_AT, "readAt": READ_AT}),
        ))
        .await
        .expect("accepted");
    assert_eq!(response, ack());
    let m = message(&fed, "m1").await;
    assert_eq!(
        m["deliveryReceipts"],
        json!({BOB_ID: {"deliveredAt": DELIVERED_AT, "readAt": READ_AT, "readDisposition": "displayed"}})
    );
    assert_eq!(m["deliveryState"], json!("received"));
    // chat L748: `deliveredAt` — "Time the first outbound delivery was
    // acknowledged."
    assert_eq!(m["deliveredAt"], json!(DELIVERED_AT));
}

/// fed L469: "Unrecognized values MUST be stored as-is." fed L463:
/// "Servers SHOULD store this value in `Message.deliveryReceipts`".
#[tokio::test]
async fn receipt_stores_unrecognized_disposition_and_device_time_as_is() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed("Message", owner_message(LOW));
    fed.receipt(receipt_args(json!({
        "deviceDeliveredAt": DELIVERED_AT,
        "readAt": READ_AT,
        "readDisposition": "x-skimmed",
    })))
    .await
    .expect("accepted");
    assert_eq!(
        message(&fed, "m1").await["deliveryReceipts"],
        json!({BOB_ID: {"deviceDeliveredAt": DELIVERED_AT, "readAt": READ_AT, "readDisposition": "x-skimmed"}})
    );
}

/// fed L494-499: "If the effective preference is `false`, the server
/// SHOULD NOT update `Message.deliveryReceipts` with the supplied `readAt`
/// or `readDisposition` values ... The server SHOULD NOT record
/// `deviceDeliveredAt` when the effective preference is `false`"; fed
/// L501: "The server MAY still record `deliveredAt` if present"; fed
/// L504-505: "The calling server is not informed of this suppression".
#[tokio::test]
async fn receipt_sharing_off_in_chat_keeps_only_delivered_at() {
    let fed = Fed::new();
    let mut chat = group(&[(ACCOUNT, "admin"), (BOB_ID, "member")]);
    chat["receiptSharing"] = json!(false);
    fed.seed("Chat", chat);
    fed.seed("Message", owner_message(GROUP));
    let response = fed
        .receipt(receipt_args(json!({
            "deliveredAt": DELIVERED_AT,
            "deviceDeliveredAt": DELIVERED_AT,
            "readAt": READ_AT,
            "readDisposition": "displayed",
        })))
        .await
        .expect("accepted");
    assert_eq!(response, ack());
    let m = message(&fed, "m1").await;
    assert_eq!(
        m["deliveryReceipts"],
        json!({BOB_ID: {"deliveredAt": DELIVERED_AT}})
    );
    // fed L788: `"delivered"` — "The recipient's server has acknowledged
    // receipt"; no read is recorded, so not `"received"`.
    assert_eq!(m["deliveryState"], json!("delivered"));
}

/// fed L476 (applied by L486-487): "if `Chat.receiptSharing` is present
/// for the relevant Chat, that value is used; otherwise the account-level
/// `PresenceStatus.receiptSharing` applies".
#[tokio::test]
async fn receipt_sharing_falls_back_to_presence_status() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed("Message", owner_message(LOW));
    fed.seed("PresenceStatus", presence_status(false));
    fed.receipt(receipt_args(json!({"readAt": READ_AT})))
        .await
        .expect("accepted");
    let m = message(&fed, "m1").await;
    assert_eq!(m.get("deliveryReceipts"), None);
    assert_eq!(m["deliveryState"], json!("pending"));
}

/// fed L476: the Chat value, when present, is used over the account one.
#[tokio::test]
async fn receipt_chat_override_beats_presence_status() {
    let fed = Fed::new();
    let mut chat = direct_chat(LOW, BOB_ID);
    chat["receiptSharing"] = json!(true);
    fed.seed("Chat", chat);
    fed.seed("Message", owner_message(LOW));
    fed.seed("PresenceStatus", presence_status(false));
    fed.receipt(receipt_args(json!({"readAt": READ_AT})))
        .await
        .expect("accepted");
    assert_eq!(
        message(&fed, "m1").await["deliveryReceipts"],
        json!({BOB_ID: {"readAt": READ_AT, "readDisposition": "displayed"}})
    );
}

/// fed L792: "the best state any recipient has reached, ordered
/// `"pending"` < `"delivered"` < `"received"`".
#[tokio::test]
async fn receipt_never_lowers_delivery_state() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    let mut m = owner_message(LOW);
    m["deliveryState"] = json!("received");
    fed.seed("Message", m);
    fed.receipt(receipt_args(json!({"deliveredAt": DELIVERED_AT})))
        .await
        .expect("accepted");
    assert_eq!(
        message(&fed, "m1").await["deliveryState"],
        json!("received")
    );
}

/// chat L745: `deliveryReceipts` maps "each non-owner participant's
/// ChatContact.id"; a reader who is not in the Chat has no receipt.
#[tokio::test]
async fn receipt_from_a_non_participant_is_forbidden() {
    let fed = Fed::new();
    fed.seed("ChatContact", contact(CAROL_ID, CAROL, false));
    fed.seed("Chat", direct_chat(LOW, CAROL_ID));
    fed.seed("Message", owner_message(LOW));
    assert_error(
        fed.receipt(receipt_args(json!({"readAt": READ_AT}))).await,
        "forbidden",
    );
    assert_eq!(message(&fed, "m1").await.get("deliveryReceipts"), None);
}

/// fed L322: "a rejected request creates none" — an identity with no
/// ChatContact is in no Chat.
#[tokio::test]
async fn receipt_from_an_unknown_identity_creates_no_placeholder() {
    let fed = fed_as(DAVE);
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed("Message", owner_message(LOW));
    let mut args = receipt_args(json!({"readAt": READ_AT}));
    args["readerUserId"] = json!(DAVE);
    assert_error(fed.receipt(args).await, "forbidden");
    assert_eq!(fed.contacts().await.len(), 1);
}

/// fed L456: `senderMsgId` — "The sender-assigned ULID identifying the
/// message for which this receipt is being sent"; chat L745:
/// `deliveryReceipts` is "Present only when `senderId` is the accountId".
#[tokio::test]
async fn receipt_for_no_owner_message_is_invalid_arguments() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed("Message", stored_message("m2", SMID, LOW, BOB_ID, PAST));
    assert_error(
        fed.receipt(receipt_args(json!({"readAt": READ_AT}))).await,
        "invalidArguments",
    );
}

// ---------------------------------------------------------------------------
// Peer/typing
// ---------------------------------------------------------------------------

fn typing_args(chat_id: &str, typing: bool) -> Value {
    json!({"accountId": ACCOUNT, "chatId": chat_id, "senderUserId": BOB, "typing": typing})
}

fn forwarded(fed: &Fed) -> Vec<Value> {
    fed.typed()
        .into_iter()
        .map(|e| serde_json::to_value(e).expect("event serializes"))
        .collect()
}

/// fed L554-555: "If all checks pass, the server MUST forward a typing
/// push event (as defined in {{JMAP-CHAT}}) to the owner's connected
/// clients"; chat L1629 (spec 6bcd9d8): `{"chatId":"<id>","senderId":
/// "<contact-id>","typing":<bool>}`; wss L229-230: "`accountId` (String):
/// The id of the account that contains the Chat."; fed L534: "the
/// receiving server does not persist this event".
#[tokio::test]
async fn typing_is_forwarded_with_the_sender_contact_id() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    let response = fed.typing(typing_args(LOW, true)).await.expect("accepted");
    assert_eq!(response, ack());
    assert_eq!(
        forwarded(&fed),
        vec![json!({"accountId": ACCOUNT, "chatId": LOW, "senderId": BOB_ID, "typing": true})]
    );
    assert!(fed.messages().await.is_empty());
}

/// fed L522: `senderUserId` "MUST match the peer identity; the receiver
/// MUST reject the request if they differ."
#[tokio::test]
async fn typing_sender_must_be_the_peer_identity() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    let mut args = typing_args(LOW, true);
    args["senderUserId"] = json!(CAROL);
    assert_error(fed.typing(args).await, "forbidden");
    assert!(forwarded(&fed).is_empty());
}

/// fed L536-539: "The receiving server SHOULD verify that the ChatContact
/// that `senderUserId` resolves to ... is a current member of the
/// identified Chat before forwarding. If it is not a member, the server
/// SHOULD return a `forbidden` method error and SHOULD NOT deliver any
/// event."
#[tokio::test]
async fn typing_from_a_non_member_is_forbidden() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "admin")]));
    assert_error(fed.typing(typing_args(GROUP, true)).await, "forbidden");
    assert_error(fed.typing(typing_args(HIGH, true)).await, "forbidden");
    assert!(forwarded(&fed).is_empty());
}

/// fed L541-544: "If `senderUserId` corresponds to a ChatContact whose
/// `blocked` field is `true` ... the server silently drops the event".
#[tokio::test]
async fn typing_from_a_blocked_contact_is_dropped_silently() {
    let fed = Fed::new();
    fed.seed("ChatContact", contact(BOB_ID, BOB, true));
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    let response = fed.typing(typing_args(LOW, true)).await.expect("success");
    assert_eq!(response, ack());
    assert!(forwarded(&fed).is_empty());
}

/// fed L546-549: "if `false` for the identified Chat (and the Chat kind is
/// `"direct"` or `"group"`), the event is silently dropped and not
/// forwarded to any of the owner's clients."
#[tokio::test]
async fn typing_with_indicators_off_in_a_group_is_dropped_silently() {
    let fed = Fed::new();
    let mut chat = group(&[(ACCOUNT, "admin"), (BOB_ID, "member")]);
    chat["receiveTypingIndicators"] = json!(false);
    fed.seed("Chat", chat);
    let response = fed
        .typing(typing_args(GROUP, false))
        .await
        .expect("success");
    assert_eq!(response, ack());
    assert!(forwarded(&fed).is_empty());
}

/// chat L636: for channel chats, servers "MUST NOT use it to suppress
/// typing events".
#[tokio::test]
async fn typing_with_indicators_off_in_a_channel_is_forwarded() {
    let fed = Fed::new();
    let mut chat = channel(0, json!([]));
    chat["receiveTypingIndicators"] = json!(false);
    fed.seed("Chat", chat);
    let mut s = space(&[], false);
    s["members"] = json!([
        {"id": ACCOUNT, "roleIds": [], "joinedAt": PAST},
        {"id": BOB_ID, "roleIds": [], "joinedAt": PAST},
    ]);
    fed.seed("Space", s);
    fed.typing(typing_args(CHANNEL, true))
        .await
        .expect("accepted");
    assert_eq!(
        forwarded(&fed),
        vec![json!({"accountId": ACCOUNT, "chatId": CHANNEL, "senderId": BOB_ID, "typing": true})]
    );
}

// ---------------------------------------------------------------------------
// Peer/retract
// ---------------------------------------------------------------------------

fn retract_args(chat_id: &str) -> Value {
    json!({"accountId": ACCOUNT, "chatId": chat_id, "senderUserId": BOB, "senderMsgId": SMID})
}

fn bobs_message(sender_id: &str) -> Value {
    let mut m = stored_message("m1", SMID, LOW, sender_id, PAST);
    m["attachments"] = json!([{
        "blobId": "b1",
        "filename": "a.txt",
        "contentType": "text/plain",
        "size": 3,
        "sha256": ABC_SHA256,
    }]);
    m["editedAt"] = json!(PAST);
    m["editHistory"] = json!([{"body": "older", "bodyType": "text/plain", "editedAt": PAST}]);
    m
}

/// fed L590: "On success, the server MUST: clear `body` to an empty
/// string, clear `attachments` to an empty array, remove `editHistory`,
/// set `deletedAt` to the current time, and set `deletedForAll` to
/// `true`. The message record is retained as a tombstone." fed L597-598:
/// `retractedAt` — "The time at which the retraction was applied."
#[tokio::test]
async fn retract_tombstones_the_senders_message() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed("Message", bobs_message(BOB_ID));
    let response = fed.retract(retract_args(LOW)).await.expect("accepted");
    let m = message(&fed, "m1").await;
    assert_eq!(m["body"], json!(""));
    assert_eq!(m["attachments"], json!([]));
    assert_eq!(m.get("editHistory"), None);
    assert_eq!(m["deletedForAll"], json!(true));
    assert_eq!(response["accountId"], json!(ACCOUNT));
    assert_eq!(response["retractedAt"], m["deletedAt"]);
    assert_ne!(m["deletedAt"], json!(PAST));
}

/// A retried retraction answers with the time it was first applied.
#[tokio::test]
async fn retract_retry_answers_with_the_first_retraction_time() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    let mut m = bobs_message(BOB_ID);
    m["body"] = json!("");
    m["deletedAt"] = json!(PAST);
    m["deletedForAll"] = json!(true);
    fed.seed("Message", m);
    let response = fed.retract(retract_args(LOW)).await.expect("accepted");
    assert_eq!(response, json!({"accountId": ACCOUNT, "retractedAt": PAST}));
}

/// fed L588: "It MUST verify that the stored message's `senderId` is the
/// `id` of the ChatContact that `senderUserId` resolves to ... If the
/// sender does not match, the server MUST reject the request." fed L846:
/// "a peer cannot retract messages sent by other users".
#[tokio::test]
async fn retract_of_another_senders_message_is_rejected() {
    for sender in [CAROL_ID, ACCOUNT] {
        let fed = Fed::new();
        fed.seed("ChatContact", contact(CAROL_ID, CAROL, false));
        fed.seed("Chat", direct_chat(LOW, BOB_ID));
        fed.seed("Message", bobs_message(sender));
        assert_error(fed.retract(retract_args(LOW)).await, "forbidden");
        let m = message(&fed, "m1").await;
        assert_eq!(m["body"], json!("old"));
        assert_eq!(m.get("deletedForAll"), None);
    }
}

/// fed L588: "The receiving server MUST look up the message by
/// `senderMsgId` within `chatId`."
#[tokio::test]
async fn retract_looks_up_the_message_within_the_chat() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed("Message", bobs_message(BOB_ID));
    assert_error(fed.retract(retract_args(HIGH)).await, "invalidArguments");
    assert_eq!(message(&fed, "m1").await["body"], json!("old"));
}

// ---------------------------------------------------------------------------
// Peer/groupUpdate
// ---------------------------------------------------------------------------

fn group_args(action: &str, extra: Value) -> Value {
    let mut args = json!({
        "accountId": ACCOUNT,
        "chatId": GROUP,
        "senderUserId": BOB,
        "action": action,
    });
    args.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    args
}

fn peer_member(identity: &str, role: &str) -> Value {
    json!({"id": identity, "role": role, "joinedAt": PAST})
}

fn member_ids(chat: &Value) -> Vec<String> {
    chat["members"]
        .as_array()
        .expect("members")
        .iter()
        .map(|m| m["id"].as_str().expect("id").to_owned())
        .collect()
}

/// fed L620: "The receiver MUST create a new Chat record of `kind:
/// "group"` with this chatId." fed L629: "Each ChatMember `id` and
/// `invitedBy` carries an identity string. The receiver resolves each one
/// per {{identity-mapping}}, creating a placeholder ChatContact for each
/// identity it does not know." fed L318: "An identity equal to the
/// receiving server's own `ownerUserId` refers to the mailbox owner, whose
/// local id is the owner's `accountId`."
#[tokio::test]
async fn group_create_stores_the_group_with_local_member_ids() {
    let fed = Fed::new();
    let mut erin = peer_member(ERIN, "member");
    erin["invitedBy"] = json!(BOB);
    let response = fed
        .group_update(group_args(
            "create",
            json!({
                "members": [peer_member(OWNER, "member"), peer_member(BOB, "admin"), erin],
                "name": "Friends",
                "description": "d",
            }),
        ))
        .await
        .expect("accepted");
    assert_eq!(response, ack());
    let placeholder: ChatContact = fed
        .contacts()
        .await
        .into_iter()
        .find(|c| c.identity == ERIN)
        .expect("fed L320: MUST create a placeholder ChatContact");
    let chat = chat(&fed, GROUP).await.expect("group stored");
    assert_eq!(chat["kind"], json!("group"));
    assert_eq!(chat["name"], json!("Friends"));
    assert_eq!(chat["description"], json!("d"));
    assert_eq!(
        chat["members"],
        json!([
            {"id": ACCOUNT, "role": "member", "joinedAt": PAST},
            {"id": BOB_ID, "role": "admin", "joinedAt": PAST},
            {"id": placeholder.id, "role": "member", "joinedAt": PAST, "invitedBy": BOB_ID},
        ])
    );
}

/// fed L651: "For `"create"`, it MUST verify that `senderUserId` is one of
/// the members listed in `members`." fed L322: "a rejected request
/// creates none."
#[tokio::test]
async fn group_create_by_an_unlisted_sender_is_forbidden() {
    let fed = Fed::new();
    let args = group_args(
        "create",
        json!({"members": [peer_member(OWNER, "admin"), peer_member(ERIN, "member")]}),
    );
    assert_error(fed.group_update(args).await, "forbidden");
    assert_eq!(chat(&fed, GROUP).await, None);
    assert_eq!(fed.contacts().await.len(), 1);
}

/// fed L875: a placeholder limit "rejects, with `rateLimited`, any
/// request that would exceed it, and that request creates no
/// placeholders." Bob is the one ChatContact; a cap of two leaves room
/// for Erin's placeholder but not Dave's.
#[tokio::test]
async fn group_create_over_the_placeholder_limit_is_rate_limited() {
    let mut fed = Fed::new();
    fed.contact_cap = Some(2);
    let args = group_args(
        "create",
        json!({"members": [
            peer_member(BOB, "admin"),
            peer_member(ERIN, "member"),
            peer_member(DAVE, "member"),
        ]}),
    );
    assert_error(fed.group_update(args).await, "rateLimited");
    assert_eq!(chat(&fed, GROUP).await, None);
    assert_eq!(fed.contacts().await.len(), 1);
}

/// fed L628: `members` (ChatMember[], "required for `"create"`").
#[tokio::test]
async fn group_create_without_members_is_invalid_arguments() {
    let fed = Fed::new();
    assert_error(
        fed.group_update(group_args("create", json!({}))).await,
        "invalidArguments",
    );
}

/// fed L834: servers confirm membership "for which the sender need only
/// be listed in `members`"; a create does not take over the chatId of a
/// Chat the sender is not in (fed L830-832, §Chat ID Integrity).
#[tokio::test]
async fn group_create_cannot_take_over_an_existing_chat_id() {
    let fed = Fed::new();
    let mut taken = direct_chat(GROUP, CAROL_ID);
    taken["kind"] = json!("direct");
    fed.seed("Chat", taken);
    let args = group_args(
        "create",
        json!({"members": [peer_member(OWNER, "member"), peer_member(BOB, "admin")]}),
    );
    assert_error(fed.group_update(args).await, "forbidden");
    assert_eq!(
        chat(&fed, GROUP).await.expect("kept")["kind"],
        json!("direct")
    );
}

/// fed L651: "For all actions other than `"create"` and `"leave"`, the
/// receiving server MUST verify that the ChatContact that `senderUserId`
/// resolves to ... holds an admin role in the named group on the
/// receiving server's local Chat record before applying any changes."
#[tokio::test]
async fn group_changes_by_a_non_admin_are_forbidden() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "admin"), (BOB_ID, "member")]));
    for (action, extra) in [
        (
            "addMembers",
            json!({"addedMembers": [peer_member(ERIN, "member")]}),
        ),
        ("removeMembers", json!({"removedMemberIds": [OWNER]})),
        (
            "updateRoles",
            json!({"updatedRoles": [{"id": BOB, "role": "admin"}]}),
        ),
        ("updateMetadata", json!({"name": "x"})),
        ("close", json!({})),
    ] {
        assert_error(
            fed.group_update(group_args(action, extra)).await,
            "forbidden",
        );
    }
    let chat = chat(&fed, GROUP).await.expect("group");
    assert_eq!(member_ids(&chat), vec![ACCOUNT, BOB_ID]);
    assert_eq!(chat["name"], json!("g"));
    assert_eq!(chat["members"][1]["role"], json!("member"));
    assert_eq!(fed.contacts().await.len(), 1);
}

/// fed L621: `"addMembers"` — "One or more members were added to the
/// group"; fed L632: "Each ChatMember `id` and `invitedBy` is resolved as
/// for `members`, including placeholder creation."
#[tokio::test]
async fn group_add_members_by_an_admin() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "member"), (BOB_ID, "admin")]));
    fed.group_update(group_args(
        "addMembers",
        json!({"addedMembers": [peer_member(ERIN, "member"), peer_member(BOB, "admin")]}),
    ))
    .await
    .expect("accepted");
    let erin = fed
        .contacts()
        .await
        .into_iter()
        .find(|c| c.identity == ERIN)
        .expect("placeholder");
    let chat = chat(&fed, GROUP).await.expect("group");
    assert_eq!(member_ids(&chat), vec![ACCOUNT, BOB_ID, erin.id.as_ref()]);
}

/// fed L622: `"removeMembers"`; fed L635: `removedMemberIds` — "The
/// identity strings of the members who were removed".
#[tokio::test]
async fn group_remove_members_by_an_admin() {
    let fed = Fed::new();
    fed.seed("ChatContact", contact(CAROL_ID, CAROL, false));
    fed.seed(
        "Chat",
        group(&[(ACCOUNT, "member"), (BOB_ID, "admin"), (CAROL_ID, "member")]),
    );
    fed.group_update(group_args(
        "removeMembers",
        json!({"removedMemberIds": [CAROL, ERIN]}),
    ))
    .await
    .expect("accepted");
    let chat = chat(&fed, GROUP).await.expect("group");
    assert_eq!(member_ids(&chat), vec![ACCOUNT, BOB_ID]);
}

/// fed L638: each `updatedRoles` entry has "`id` (String, the identity
/// string of the member ...) and `role` (String, the new role value:
/// `"admin"` or `"member"`)"; fed L318: the owner's identity maps to the
/// `accountId`.
#[tokio::test]
async fn group_update_roles_by_an_admin() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "member"), (BOB_ID, "admin")]));
    fed.group_update(group_args(
        "updateRoles",
        json!({"updatedRoles": [{"id": OWNER, "role": "admin"}]}),
    ))
    .await
    .expect("accepted");
    let chat = chat(&fed, GROUP).await.expect("group");
    assert_eq!(
        chat["members"][0],
        json!({"id": ACCOUNT, "role": "admin", "joinedAt": PAST})
    );
}

/// A `Peer/groupUpdate` `avatar` object (fed 6bcd9d8 L668: "the
/// `contentType`, `size`, and `sha256` fields of an Attachment ... plus
/// `fetchUrl`") for the "abc" blob the test backend serves at `FETCH_URL`.
fn abc_avatar() -> Value {
    json!({"contentType": "image/png", "size": 3, "sha256": ABC_SHA256, "fetchUrl": FETCH_URL})
}

/// The group, `ACCOUNT` a member and Bob an admin, with avatar `blob_id`.
fn group_with_avatar(blob_id: &str) -> Value {
    let mut g = group(&[(ACCOUNT, "member"), (BOB_ID, "admin")]);
    g["avatarBlobId"] = json!(blob_id);
    g
}

/// fed 6bcd9d8 L645: `"updateMetadata"` — "The group's `name`,
/// `description`, or avatar (`avatarBlobId` in {{JMAP-CHAT}}) was
/// changed." L678: "When `avatar` is an Object and its fetch and checks
/// succeeded, it stores the fetched blob and sets the Chat's
/// `avatarBlobId` to the local blobId".
#[tokio::test]
async fn group_update_metadata_by_an_admin() {
    let fed = Fed::new();
    fed.seed("Chat", group_with_avatar("old-avatar"));
    fed.group_update(group_args(
        "updateMetadata",
        json!({"name": "New", "avatar": abc_avatar()}),
    ))
    .await
    .expect("accepted");
    let chat = chat(&fed, GROUP).await.expect("group");
    assert_eq!(chat["name"], json!("New"));
    assert_eq!(chat["avatarBlobId"], json!("local-blob-1"));
    assert_eq!(member_ids(&chat), vec![ACCOUNT, BOB_ID]);
    assert_eq!(*fed.fetched.lock().expect("lock"), vec![FETCH_URL]);
}

/// fed 6bcd9d8 L678: "when `avatar` is `null`, it removes
/// `avatarBlobId`"; L668: "absent leaves it unchanged."
#[tokio::test]
async fn group_update_metadata_null_avatar_removes_it() {
    let fed = Fed::new();
    fed.seed("Chat", group_with_avatar("old-avatar"));
    fed.group_update(group_args("updateMetadata", json!({"name": "New"})))
        .await
        .expect("accepted");
    let stored = chat(&fed, GROUP).await.expect("group");
    assert_eq!(stored["avatarBlobId"], json!("old-avatar"));

    fed.group_update(group_args("updateMetadata", json!({"avatar": null})))
        .await
        .expect("accepted");
    let stored = chat(&fed, GROUP).await.expect("group");
    assert!(stored.get("avatarBlobId").is_none_or(Value::is_null));
    assert_eq!(stored["name"], json!("New"));
}

/// fed 6bcd9d8 L676: "if the fetch or any of these checks fails, the
/// receiving server still applies the rest of the request, and leaves the
/// Chat ... with its current avatar (for `"updateMetadata"`). ... A failed
/// avatar never causes the request to be rejected". The checks are
/// `contentType` and `size` "per Attachment in {{JMAP-CHAT}}" before the
/// fetch, then "the byte count against `size` and the content against
/// `sha256`" after it.
#[tokio::test]
async fn group_update_metadata_failed_avatar_keeps_the_current_one() {
    let mut not_a_type = abc_avatar();
    not_a_type["contentType"] = json!("png");
    let mut too_big = abc_avatar();
    // chat 6bcd9d8 L319 `maxAttachmentBytes`, L1758: "Enforce ...
    // `maxAttachmentBytes` at parse time, before any fetch". The test
    // backend's limit is 50000000.
    too_big["size"] = json!(50_000_001_u64);
    let mut wrong_size = abc_avatar();
    wrong_size["size"] = json!(4);
    let mut wrong_digest = abc_avatar();
    wrong_digest["sha256"] = json!("0".repeat(64));
    let mut unfetchable = abc_avatar();
    unfetchable["fetchUrl"] = json!("https://peer.example/blob/missing");
    for (avatar, fetches) in [
        (not_a_type, 0),
        (too_big, 0),
        (wrong_size, 1),
        (wrong_digest, 1),
        (unfetchable, 1),
    ] {
        let fed = Fed::new();
        fed.seed("Chat", group_with_avatar("old-avatar"));
        let response = fed
            .group_update(group_args(
                "updateMetadata",
                json!({"name": "New", "avatar": avatar}),
            ))
            .await
            .expect("a failed avatar never rejects the request");
        assert_eq!(response, ack());
        let chat = chat(&fed, GROUP).await.expect("group");
        assert_eq!(chat["name"], json!("New"));
        assert_eq!(chat["avatarBlobId"], json!("old-avatar"));
        assert_eq!(fed.fetched.lock().expect("lock").len(), fetches);
    }
}

/// fed 6bcd9d8 L676: "After the authorization and `closedAt` checks above
/// succeed, and only then, if `avatar` is an Object, the receiving server
/// ... fetches the blob". A non-admin's or a closed group's update fetches
/// nothing.
#[tokio::test]
async fn group_avatar_is_not_fetched_before_the_checks_pass() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "admin"), (BOB_ID, "member")]));
    assert_error(
        fed.group_update(group_args(
            "updateMetadata",
            json!({"avatar": abc_avatar()}),
        ))
        .await,
        "forbidden",
    );
    let mut closed = group(&[(ACCOUNT, "member"), (BOB_ID, "admin")]);
    closed["closedAt"] = json!(PAST);
    let fed_closed = Fed::new();
    fed_closed.seed("Chat", closed);
    assert_error(
        fed_closed
            .group_update(group_args(
                "updateMetadata",
                json!({"avatar": abc_avatar()}),
            ))
            .await,
        "forbidden",
    );
    let unlisted = Fed::new();
    assert_error(
        unlisted
            .group_update(group_args(
                "create",
                json!({"members": [peer_member(OWNER, "admin")], "avatar": abc_avatar()}),
            ))
            .await,
        "forbidden",
    );
    assert!(fed.fetched.lock().expect("lock").is_empty());
    assert!(fed_closed.fetched.lock().expect("lock").is_empty());
    assert!(unlisted.fetched.lock().expect("lock").is_empty());
}

/// fed 6bcd9d8 L676-678 for `"create"`: a fetched avatar becomes the new
/// Chat's `avatarBlobId`; a failed one "leaves the Chat without an
/// avatar".
#[tokio::test]
async fn group_create_fetches_the_avatar() {
    let members = json!([peer_member(OWNER, "member"), peer_member(BOB, "admin")]);
    let fed = Fed::new();
    fed.group_update(group_args(
        "create",
        json!({"members": members, "avatar": abc_avatar()}),
    ))
    .await
    .expect("accepted");
    let stored = chat(&fed, GROUP).await.expect("group stored");
    assert_eq!(stored["avatarBlobId"], json!("local-blob-1"));

    let mut unfetchable = abc_avatar();
    unfetchable["fetchUrl"] = json!("https://peer.example/blob/missing");
    let fed = Fed::new();
    fed.group_update(group_args(
        "create",
        json!({"members": members, "avatar": unfetchable}),
    ))
    .await
    .expect("a failed avatar never rejects the request");
    let stored = chat(&fed, GROUP).await.expect("group stored");
    assert!(stored.get("avatarBlobId").is_none_or(Value::is_null));
    assert_eq!(member_ids(&stored), vec![ACCOUNT, BOB_ID]);
}

/// fed L625: `"leave"` — "The receiver MUST remove the sender from the
/// group's member list without requiring admin authorization."
#[tokio::test]
async fn group_leave_by_a_member() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "admin"), (BOB_ID, "member")]));
    fed.group_update(group_args("leave", json!({})))
        .await
        .expect("accepted");
    let chat = chat(&fed, GROUP).await.expect("group");
    assert_eq!(member_ids(&chat), vec![ACCOUNT]);
    assert_eq!(chat.get("closedAt"), None);
}

/// fed L651: "For `"leave"`, it MUST verify that that ChatContact is a
/// current member."
#[tokio::test]
async fn group_leave_by_a_non_member_is_forbidden() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "admin")]));
    assert_error(
        fed.group_update(group_args("leave", json!({}))).await,
        "forbidden",
    );
}

/// fed L626: `"close"` — "The receiver MUST remove the sender from the
/// group's member list and set the local Chat record's `closedAt`." fed
/// L653: "Once the local Chat record has `closedAt` set, the receiving
/// server MUST reject every subsequent `Peer/groupUpdate` for that chat
/// with `forbidden`."
#[tokio::test]
async fn group_close_then_every_update_is_forbidden() {
    let fed = Fed::new();
    fed.seed(
        "Chat",
        group(&[(ACCOUNT, "member"), (BOB_ID, "admin"), (CAROL_ID, "member")]),
    );
    fed.group_update(group_args("close", json!({})))
        .await
        .expect("accepted");
    let closed = chat(&fed, GROUP).await.expect("group");
    assert_eq!(member_ids(&closed), vec![ACCOUNT, CAROL_ID]);
    assert!(closed["closedAt"].is_string());

    let fed = Fed::new();
    let mut chat_value = group(&[(ACCOUNT, "member"), (BOB_ID, "admin")]);
    chat_value["closedAt"] = json!(PAST);
    fed.seed("Chat", chat_value);
    for (action, extra) in [
        (
            "create",
            json!({"members": [peer_member(OWNER, "member"), peer_member(BOB, "admin")]}),
        ),
        ("leave", json!({})),
        ("updateMetadata", json!({"name": "x"})),
        (
            "addMembers",
            json!({"addedMembers": [peer_member(ERIN, "member")]}),
        ),
    ] {
        assert_error(
            fed.group_update(group_args(action, extra)).await,
            "forbidden",
        );
    }
    assert_eq!(
        member_ids(&chat(&fed, GROUP).await.expect("group")),
        vec![ACCOUNT, BOB_ID]
    );
}

/// fed L618: `action` is "One of the following values"; any other is not
/// a valid argument.
#[tokio::test]
async fn group_unknown_action_is_invalid_arguments() {
    let fed = Fed::new();
    fed.seed("Chat", group(&[(ACCOUNT, "member"), (BOB_ID, "admin")]));
    assert_error(
        fed.group_update(group_args("rename", json!({}))).await,
        "invalidArguments",
    );
}
