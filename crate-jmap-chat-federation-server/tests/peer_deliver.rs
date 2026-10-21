//! `Peer/deliver` validation (draft-atwood-jmap-chat-federation-00
//! §New Message Validation, §Edit and Reaction Validation, §Response,
//! §Direct Chat Simultaneous Initiation).
//!
//! "fed Lnnn" cites draft-atwood-jmap-chat-federation-00.md and "chat
//! Lnnn" draft-atwood-jmap-chat-00.md, both at jmap-chat-spec be46208.
//! Oracle: the quoted draft text, hand-written JSON records, and the
//! FIPS 180-2 Appendix B.1 SHA-256 digest of "abc".

mod common;

use std::collections::HashSet;

use common::*;
use jmap_chat_federation_server::AuthenticatedPeer;
use jmap_chat_server::ChatLimits;
use jmap_peer_auth::PeerDomain;
use jmap_types::{Id, UTCDate};
use serde_json::{json, Value};

fn message(sender_msg_id: &str) -> Value {
    json!({
        "senderMsgId": sender_msg_id,
        "body": "hello",
        "bodyType": "text/plain",
        "sentAt": PAST,
        "attachments": [],
    })
}

fn deliver_args(chat_id: &str, chat_kind: &str, message: Value) -> Value {
    json!({
        "accountId": ACCOUNT,
        "chatId": chat_id,
        "senderUserId": BOB,
        "chatKind": chat_kind,
        "message": message,
    })
}

fn abc_attachment(filename: &str, sha256: &str) -> Value {
    json!({
        "blobId": "remote-blob-1",
        "filename": filename,
        "contentType": "text/plain",
        "size": 3,
        "sha256": sha256,
        "fetchUrl": FETCH_URL,
    })
}

// ---------------------------------------------------------------------------
// Step 1
// ---------------------------------------------------------------------------

/// fed L306: "Callers without the `"peer"` role MUST receive `forbidden`
/// for all methods in this section."
#[tokio::test]
async fn step1_unauthenticated_caller_is_forbidden() {
    let mut fed = Fed::new();
    fed.peer = None;
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", message("M1")))
            .await,
        "forbidden",
    );
    assert!(fed.messages().await.is_empty());
}

/// fed L402: "Verify the request signature and the identity binding".
/// fed L244: "If the peer identity has a host, that host MUST equal the
/// authenticated peer domain."
#[tokio::test]
async fn step1_identity_not_bound_to_peer_domain_is_forbidden() {
    let mut fed = Fed::new();
    fed.peer = Some(AuthenticatedPeer::new(
        PeerDomain::parse(PEER_DOMAIN).expect("domain"),
        CAROL,
    ));
    let mut args = deliver_args(LOW, "direct", message("M1"));
    args["senderUserId"] = json!(CAROL);
    assert_error(fed.deliver(args).await, "forbidden");
    assert!(fed.messages().await.is_empty());
}

/// The did:key example of the W3C did:key Method specification §Format.
const DID_KEY: &str = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";

fn did_key_peer() -> AuthenticatedPeer {
    AuthenticatedPeer::new(PeerDomain::parse(PEER_DOMAIN).expect("domain"), DID_KEY)
}

/// fed L256 at jmap-chat-spec 6bcd9d8: "If the peer identity has no host
/// (for example, a `did:key` DID), the peer domain does not vouch for it.
/// The local server MUST NOT accept such an identity on the strength of
/// peer authentication alone."
#[tokio::test]
async fn step1_host_less_did_without_did_verification_is_forbidden() {
    let mut fed = Fed::new();
    fed.peer = Some(did_key_peer());
    let mut args = deliver_args(LOW, "direct", message("M1"));
    args["senderUserId"] = json!(DID_KEY);
    assert_error(fed.deliver(args).await, "forbidden");
    assert!(fed.messages().await.is_empty());
}

/// fed L256 at jmap-chat-spec 6bcd9d8: "A DID-form identity is accepted
/// only after the DID-bound verification of {{JMAP-CHAT-DID}} succeeds";
/// did L211: "{{JMAP-CHAT-FED}} accepts it only after the DID-bound
/// verification defined here succeeds."
#[tokio::test]
async fn step1_host_less_did_with_did_verification_is_accepted() {
    let mut fed = Fed::new();
    fed.peer = Some(did_key_peer().with_did_verified());
    let mut args = deliver_args(LOW, "direct", message("M1"));
    args["senderUserId"] = json!(DID_KEY);
    fed.deliver(args).await.expect("accepted");
    assert_eq!(fed.messages().await.len(), 1);
}

/// fed L255 at jmap-chat-spec 6bcd9d8: "If the peer identity has a host,
/// that host MUST equal the authenticated peer domain." A DID-bound
/// verification does not lift that rule.
#[tokio::test]
async fn step1_did_verification_does_not_bind_another_host() {
    let mut fed = Fed::new();
    fed.peer = Some(
        AuthenticatedPeer::new(PeerDomain::parse(PEER_DOMAIN).expect("domain"), CAROL)
            .with_did_verified(),
    );
    let mut args = deliver_args(LOW, "direct", message("M1"));
    args["senderUserId"] = json!(CAROL);
    assert_error(fed.deliver(args).await, "forbidden");
    assert!(fed.messages().await.is_empty());
}

// ---------------------------------------------------------------------------
// Step 2 and identity mapping
// ---------------------------------------------------------------------------

/// fed L403: "Confirm `senderUserId` matches the peer identity. Reject
/// immediately if they differ."
#[tokio::test]
async fn step2_sender_user_id_must_equal_peer_identity() {
    let fed = Fed::new();
    let mut args = deliver_args(LOW, "direct", message("M1"));
    args["senderUserId"] = json!("mallory@peer.example");
    assert_error(fed.deliver(args).await, "forbidden");
    assert!(fed.messages().await.is_empty());
    assert!(fed.chats().await.is_empty());
}

/// fed L316: "Otherwise, the receiving server MUST create a placeholder
/// ChatContact with that `identity` and a server-assigned `id`, and the
/// identity refers to the placeholder."
#[tokio::test]
async fn step2_unknown_sender_gets_a_placeholder_contact() {
    let mut fed = Fed::new();
    fed.peer = Some(AuthenticatedPeer::new(
        PeerDomain::parse("other.example").expect("domain"),
        CAROL,
    ));
    let mut args = deliver_args(LOW, "direct", message("M1"));
    args["senderUserId"] = json!(CAROL);
    fed.deliver(args).await.expect("accepted");
    let carol = fed
        .contacts()
        .await
        .into_iter()
        .find(|c| c.identity == CAROL)
        .expect("placeholder created");
    let messages = fed.messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].sender_id, carol.id);
}

/// fed L318: "A receiving server creates placeholders only as part of a
/// request it accepts; a rejected request creates none."
#[tokio::test]
async fn rejected_request_creates_no_placeholder() {
    let fed = Fed::new();
    let mut m = message("M1");
    m["mentions"] = json!([{"id": CAROL}]);
    m["bodyType"] = json!("application/x-unsupported");
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "invalidArguments",
    );
    assert!(fed.contacts().await.iter().all(|c| c.identity != CAROL));
}

// ---------------------------------------------------------------------------
/// fed L875: "Servers MAY limit how many placeholder ChatContacts a peer
/// can cause to be created. A server that applies such a limit rejects,
/// with `rateLimited`, any request that would exceed it, and that request
/// creates no placeholders." Bob is the one ChatContact; a cap of two
/// leaves room for one placeholder, and the message names two unknown
/// identities.
#[tokio::test]
async fn placeholder_limit_rejects_with_rate_limited_and_creates_none() {
    let mut fed = Fed::new();
    fed.contact_cap = Some(2);
    let mut m = message("M1");
    m["mentions"] = json!([{"id": CAROL}, {"id": "dave@peer.example"}]);
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "rateLimited",
    );
    let identities: Vec<String> = fed
        .contacts()
        .await
        .into_iter()
        .map(|c| c.identity)
        .collect();
    assert_eq!(identities, [BOB]);
    assert!(fed.messages().await.is_empty());
    assert!(fed.chats().await.is_empty());
}

/// fed L875: only a request "that would exceed" the limit is rejected; a
/// cap with room for both placeholders accepts the same message.
#[tokio::test]
async fn placeholder_limit_accepts_a_request_within_it() {
    let mut fed = Fed::new();
    fed.contact_cap = Some(3);
    let mut m = message("M1");
    m["mentions"] = json!([{"id": CAROL}, {"id": "dave@peer.example"}]);
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("accepted");
    assert_eq!(fed.contacts().await.len(), 3);
    assert_eq!(fed.messages().await.len(), 1);
}

// Step 3
// ---------------------------------------------------------------------------

/// fed L404: "if a chat with this sender already exists, confirm `chatId`
/// matches its stored id; otherwise create a new Chat record with this
/// chatId and set `contactId` to the sender's ChatContact `id`".
#[tokio::test]
async fn step3_direct_chat_is_created_with_the_incoming_chat_id() {
    let fed = Fed::new();
    let response = fed
        .deliver(deliver_args(LOW, "direct", message("M1")))
        .await
        .expect("accepted");
    let chats = fed.chats().await;
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].id, Id::from(LOW));
    assert_eq!(chats[0].contact_id, Some(Id::from(BOB_ID)));
    let messages = fed.messages().await;
    assert_eq!(messages[0].chat_id, Id::from(LOW));
    // fed L439: canonicalChatId is "Absent in all other cases."
    assert!(response.get("canonicalChatId").is_none());
}

/// fed L404, and fed L833 (§Chat ID Integrity): the check "prevents a sender from
/// injecting messages into a chatId that belongs to a different
/// conversation."
#[tokio::test]
async fn step3_direct_delivery_cannot_take_over_another_chat_id() {
    let fed = Fed::new();
    fed.seed("ChatContact", contact(CAROL_ID, CAROL, false));
    fed.seed("Chat", direct_chat(LOW, CAROL_ID));
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", message("M1")))
            .await,
        "forbidden",
    );
    assert!(fed.messages().await.is_empty());
}

/// fed L404: "For group and channel chats: confirm `chatId` matches a
/// known chat and the sender is a current member of that chat."
#[tokio::test]
async fn step3_group_sender_must_be_a_member() {
    let fed = Fed::new();
    fed.seed("Chat", group_chat(&[ACCOUNT], false));
    assert_error(
        fed.deliver(deliver_args(GROUP, "group", message("M1")))
            .await,
        "forbidden",
    );

    let fed = Fed::new();
    fed.seed("Chat", group_chat(&[ACCOUNT, BOB_ID], false));
    fed.deliver(deliver_args(GROUP, "group", message("M1")))
        .await
        .expect("member accepted");
    assert_eq!(fed.messages().await.len(), 1);
}

/// fed L404: "If the group chat's `closedAt` is set, or the channel's
/// Space has `closedAt` set ({{JMAP-CHAT}}), reject with `forbidden`."
#[tokio::test]
async fn step3_closed_group_or_space_is_forbidden() {
    let fed = Fed::new();
    fed.seed("Chat", group_chat(&[ACCOUNT, BOB_ID], true));
    assert_error(
        fed.deliver(deliver_args(GROUP, "group", message("M1")))
            .await,
        "forbidden",
    );

    let fed = Fed::new();
    fed.seed("Chat", channel(0, json!([])));
    fed.seed("Space", space(&["send"], true));
    assert_error(
        fed.deliver(deliver_args(CHANNEL, "channel", message("M1")))
            .await,
        "forbidden",
    );
    assert!(fed.messages().await.is_empty());
}

/// fed L404: for channels, "confirm `chatId` matches a known chat".
#[tokio::test]
async fn step3_unknown_channel_is_forbidden() {
    let fed = Fed::new();
    assert_error(
        fed.deliver(deliver_args(CHANNEL, "channel", message("M1")))
            .await,
        "forbidden",
    );
}

// ---------------------------------------------------------------------------
// Direct chat race
// ---------------------------------------------------------------------------

/// fed L841: "when a `Peer/deliver` arrives for a direct chat with an
/// unknown chatId X, and the receiving server already has a direct chat
/// chatId Y with the same sender, both servers SHOULD adopt `min(X, Y)`
/// (lexicographically smaller) as the canonical chatId. ... and SHOULD
/// return the canonical chatId in the `Peer/deliver` response".
/// Here Y = LOW < X = HIGH, so Y stays.
#[tokio::test]
async fn race_keeps_the_smaller_stored_chat_id() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    let response = fed
        .deliver(deliver_args(HIGH, "direct", message("M1")))
        .await
        .expect("accepted");
    assert_eq!(response["canonicalChatId"], json!(LOW));
    let chats = fed.chats().await;
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].id, Id::from(LOW));
    assert_eq!(fed.messages().await[0].chat_id, Id::from(LOW));
}

/// fed L841: "The receiving server SHOULD migrate any messages stored
/// under the non-canonical chatId to the canonical one". Here Y = HIGH >
/// X = LOW, so the stored Chat and its Message move to LOW.
#[tokio::test]
async fn race_migrates_to_the_smaller_incoming_chat_id() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(HIGH, BOB_ID));
    fed.seed(
        "Message",
        stored_message("m-old", "S0", HIGH, ACCOUNT, PAST),
    );
    let response = fed
        .deliver(deliver_args(LOW, "direct", message("M1")))
        .await
        .expect("accepted");
    assert_eq!(response["canonicalChatId"], json!(LOW));
    let chats = fed.chats().await;
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].id, Id::from(LOW));
    let messages = fed.messages().await;
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().all(|m| m.chat_id == LOW));
}

// ---------------------------------------------------------------------------
// Steps 4 to 7
// ---------------------------------------------------------------------------

/// fed L405: "verify that no active SpaceBan exists for the sender in the
/// Space containing the identified channel. If a SpaceBan exists,
/// silently drop the message and return success."
#[tokio::test]
async fn step4_banned_sender_is_dropped_silently() {
    let fed = Fed::new();
    fed.seed("Chat", channel(0, json!([])));
    fed.seed("Space", space(&["send"], false));
    fed.seed(
        "SpaceBan",
        json!({"id": "ban-1", "spaceId": SPACE, "userId": BOB_ID, "bannedBy": ACCOUNT, "createdAt": PAST}),
    );
    let response = fed
        .deliver(deliver_args(CHANNEL, "channel", message("M1")))
        .await
        .expect("success");
    assert_eq!(response["accountId"], json!(ACCOUNT));
    assert!(response["receivedMsgId"].is_string());
    assert!(fed.messages().await.is_empty());
}

/// chat L905 (§SpaceBan `expiresAt`): "If present, the ban expires at
/// this time and the server MUST restore the user's access." An expired
/// ban is not active (fed L405).
#[tokio::test]
async fn step4_expired_ban_does_not_drop() {
    let fed = Fed::new();
    fed.seed("Chat", channel(0, json!([])));
    fed.seed("Space", space(&["send"], false));
    fed.seed(
        "SpaceBan",
        json!({"id": "ban-1", "spaceId": SPACE, "userId": BOB_ID, "bannedBy": ACCOUNT,
               "createdAt": PAST, "expiresAt": PAST}),
    );
    fed.deliver(deliver_args(CHANNEL, "channel", message("M1")))
        .await
        .expect("accepted");
    assert_eq!(fed.messages().await.len(), 1);
}

/// fed L406: "verify that the sender holds the `"send"` permission in the
/// identified channel per the Space permission resolution ... If the
/// sender lacks `"send"`, reject with `forbidden`."
#[tokio::test]
async fn step5_sender_needs_send_permission() {
    let fed = Fed::new();
    fed.seed("Chat", channel(0, json!([])));
    fed.seed("Space", space(&["view"], false));
    assert_error(
        fed.deliver(deliver_args(CHANNEL, "channel", message("M1")))
            .await,
        "forbidden",
    );

    // chat L1621: "Compute the union of `permissions` across all
    // SpaceRoles held by the member, including the implicit `@everyone`
    // role."
    let mut fed = Fed::new();
    fed.everyone = HashSet::from([jmap_chat_types::ChatPermission::Send]);
    fed.seed("Chat", channel(0, json!([])));
    fed.seed("Space", space(&["view"], false));
    fed.deliver(deliver_args(CHANNEL, "channel", message("M1")))
        .await
        .expect("send from @everyone");
}

/// chat §Space Permission Resolution step 4: "Apply `deny` entries from
/// any ChannelPermission record matching the member directly".
#[tokio::test]
async fn step5_member_override_denies_send() {
    let fed = Fed::new();
    fed.seed(
        "Chat",
        channel(
            0,
            json!([{"targetId": BOB_ID, "targetType": "member", "allow": [], "deny": ["send"]}]),
        ),
    );
    fed.seed("Space", space(&["send"], false));
    assert_error(
        fed.deliver(deliver_args(CHANNEL, "channel", message("M1")))
            .await,
        "forbidden",
    );
}

/// fed L407: "If the sender's last accepted message in this channel was
/// received within `slowModeSeconds` seconds and the sender does not hold
/// `"manage_channels"` permission, reject with `rateLimited`."
#[tokio::test]
async fn step6_slow_mode_rate_limits_the_sender() {
    let fed = Fed::new();
    fed.seed("Chat", channel(3600, json!([])));
    fed.seed("Space", space(&["send"], false));
    fed.deliver(deliver_args(CHANNEL, "channel", message("M1")))
        .await
        .expect("first message");
    assert_error(
        fed.deliver(deliver_args(CHANNEL, "channel", message("M2")))
            .await,
        "rateLimited",
    );
    assert_eq!(fed.messages().await.len(), 1);
}

/// fed L407: a sender holding `"manage_channels"` is not rate-limited.
#[tokio::test]
async fn step6_manage_channels_is_exempt() {
    let fed = Fed::new();
    fed.seed("Chat", channel(3600, json!([])));
    fed.seed("Space", space(&["send", "manage_channels"], false));
    fed.deliver(deliver_args(CHANNEL, "channel", message("M1")))
        .await
        .expect("first message");
    fed.deliver(deliver_args(CHANNEL, "channel", message("M2")))
        .await
        .expect("exempt");
    assert_eq!(fed.messages().await.len(), 2);
}

/// fed L408: "If `blocked` is `true` on the sender's ChatContact record,
/// silently drop the message and return success (to avoid disclosing
/// block status to the sender)."
#[tokio::test]
async fn step7_blocked_sender_is_dropped_silently() {
    let fed = Fed::new();
    fed.seed("ChatContact", contact(BOB_ID, BOB, true));
    let response = fed
        .deliver(deliver_args(LOW, "direct", message("M1")))
        .await
        .expect("success");
    assert!(response["receivedMsgId"].is_string());
    assert!(response["receivedAt"].is_string());
    assert!(fed.messages().await.is_empty());
    assert!(fed.chats().await.is_empty());
}

// ---------------------------------------------------------------------------
// Steps 8 to 14
// ---------------------------------------------------------------------------

/// fed L409: "Validate `body` byte length against `maxBodyBytes`; reject
/// with `invalidArguments` if exceeded."
#[tokio::test]
async fn step8_body_over_max_body_bytes_is_rejected() {
    let fed = Fed::new();
    fed.chat
        .set_limits_for_test(Some(ChatLimits::default().with_max_body_bytes(4)));
    // "hello" is 5 bytes.
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", message("M1")))
            .await,
        "invalidArguments",
    );
    assert!(fed.messages().await.is_empty());
    assert!(fed.chats().await.is_empty());
}

/// fed L410: "Validate `bodyType` against the server's
/// `supportedBodyTypes`; reject with `invalidArguments` if not present."
#[tokio::test]
async fn step9_unsupported_body_type_is_rejected() {
    let fed = Fed::new();
    let mut m = message("M1");
    m["bodyType"] = json!("application/x-unsupported");
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "invalidArguments",
    );
    assert!(fed.messages().await.is_empty());
}

/// fed L411: "Validate each attachment `filename` (MUST NOT contain `/`,
/// `\`, or null bytes), `contentType` (MUST be a syntactically valid MIME
/// type string), and `size`." Nothing is fetched for a rejected message
/// (fed L419: "no side effects").
#[tokio::test]
async fn step10_invalid_attachment_metadata_is_rejected_before_fetch() {
    let fed = Fed::new();
    let mut m = message("M1");
    m["attachments"] = json!([abc_attachment("../etc/passwd", ABC_SHA256)]);
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "invalidArguments",
    );

    let mut m = message("M2");
    let mut a = abc_attachment("a.txt", ABC_SHA256);
    a["contentType"] = json!("not a mime type");
    m["attachments"] = json!([a]);
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "invalidArguments",
    );

    assert!(fed.fetched.lock().expect("lock").is_empty());
    assert!(fed.messages().await.is_empty());
}

/// fed L412: "Fetch each attachment blob from its `fetchUrl`; verify the
/// byte count against `size` and the content against `sha256`. Reject
/// with `invalidArguments` if either check fails."
#[tokio::test]
async fn step11_fetched_blob_must_match_size_and_sha256() {
    let fed = Fed::new();
    let wrong = "0".repeat(64);
    let mut m = message("M1");
    m["attachments"] = json!([abc_attachment("a.txt", &wrong)]);
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "invalidArguments",
    );

    let mut m = message("M2");
    let mut a = abc_attachment("a.txt", ABC_SHA256);
    a["size"] = json!(4);
    m["attachments"] = json!([a]);
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "invalidArguments",
    );
    assert!(fed.messages().await.is_empty());

    let mut m = message("M3");
    m["attachments"] = json!([abc_attachment("a.txt", ABC_SHA256)]);
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("matching blob accepted");
    let messages = fed.messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].attachments[0].blob_id, Id::from("local-blob-1"));
    assert_eq!(fed.fetched.lock().expect("lock").as_slice(), [FETCH_URL; 3]);
}

/// fed L413: "Take mentions only from the `mentions` array ... The
/// receiving server MUST NOT parse `body`, including a rich body's spans,
/// to find mentions". fed L314-316: an identity equal to the owner's
/// `ownerUserId` is the owner's `accountId`; a ChatContact's identity is
/// that ChatContact; any other gets a placeholder.
#[tokio::test]
async fn step12_mentions_come_from_the_array_and_map_to_local_ids() {
    let fed = Fed::new();
    let mut m = message("M1");
    m["body"] = json!("hi @dave");
    m["mentions"] = json!([{"id": OWNER}, {"id": BOB}, {"id": CAROL}]);
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("accepted");
    let carol = fed
        .contacts()
        .await
        .into_iter()
        .find(|c| c.identity == CAROL)
        .expect("placeholder for the mentioned identity");
    let mentioned: Vec<Id> = fed.messages().await[0]
        .mentions
        .iter()
        .map(|m| m.id.clone())
        .collect();
    assert_eq!(mentioned, [Id::from(ACCOUNT), Id::from(BOB_ID), carol.id]);
}

/// tasks L254 (draft-atwood-jmap-chat-tasks-00.md) and fed L312-322, all
/// at jmap-chat-spec 63e9d08. tasks L254: "Between servers it carries
/// identity strings, not local ids ... the receiving server maps each
/// identity to its local id." fed L312: the rule covers "fields inside MessageAction metadata (for
/// example, `assigneeIds` in the task metadata ...)"; fed L318-320 resolve
/// the owner, a known ChatContact and an unknown identity (placeholder).
/// fed L322: "A server that does not recognise a field passes it through
/// unmapped": other keys and other action types are stored as sent.
#[tokio::test]
async fn task_action_assignee_ids_map_to_local_ids() {
    let fed = Fed::new();
    let mut m = message("M1");
    m["actions"] = json!([
        {
            "type": "urn:ietf:params:jmap:chat:cap:task",
            "uri": "https://tracker.example/T-1",
            "metadata": {"assigneeIds": [OWNER, BOB, CAROL], "priority": 1},
        },
        {
            "type": "urn:example:other",
            "uri": "https://other.example/1",
            "metadata": {"assigneeIds": [CAROL]},
        },
    ]);
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("accepted");
    let carol = fed
        .contacts()
        .await
        .into_iter()
        .find(|c| c.identity == CAROL)
        .expect("placeholder for the assigned identity");
    let actions = &fed.messages().await[0].actions;
    assert_eq!(
        actions[0].metadata,
        Some(json!({
            "assigneeIds": [ACCOUNT, BOB_ID, carol.id.to_string()],
            "priority": 1,
        }))
    );
    assert_eq!(actions[1].metadata, Some(json!({"assigneeIds": [CAROL]})));
}

/// tasks L202 (jmap-chat-spec 63e9d08): `assigneeIds` is "String[]" and
/// "Between servers, entries carry identities". fed L320: an unknown
/// identity gets a placeholder ChatContact, which carries it as `login`,
/// "at least one non-whitespace character" (chat L508), as for a Mention
/// `id`. fed L322: "a rejected request creates none".
#[tokio::test]
async fn task_action_assignee_ids_must_be_identity_strings() {
    for ids in [json!([" "]), json!([7]), json!(CAROL)] {
        let fed = Fed::new();
        let mut m = message("M1");
        m["actions"] = json!([{
            "type": "urn:ietf:params:jmap:chat:cap:task",
            "uri": "https://tracker.example/T-1",
            "metadata": {"assigneeIds": ids},
        }]);
        assert_error(
            fed.deliver(deliver_args(LOW, "direct", m)).await,
            "invalidArguments",
        );
        assert!(fed.messages().await.is_empty());
        assert!(fed.contacts().await.iter().all(|c| c.identity != CAROL));
    }
}

/// fed L337 (jmap-chat-spec 6bcd9d8): "A receiving server removes every
/// MessageAction ... whose `uri` is a JMAP object URI (a URI with the
/// scheme `jmap`) from data it receives from a peer, whether in a Peer/*
/// call ... before storing the data ... The remaining entries keep their
/// order, and an array left empty is stored as absent. MessageActions and
/// Endpoints with other URIs, such as `https` URLs, are kept." fed L367:
/// "MessageActions whose `uri` is a JMAP object URI are removed". A
/// removed task action is not stored, so its `assigneeIds` (here not
/// identity strings) do not reject the delivery; a kept one is still
/// mapped. object-uri L92: "the scheme name is case-insensitive".
#[tokio::test]
async fn jmap_object_uri_actions_are_removed() {
    let fed = Fed::new();
    let mut m = message("M1");
    m["actions"] = json!([
        {
            "type": "urn:ietf:params:jmap:chat:cap:task",
            "uri": "jmap:task:A9:T1",
            "metadata": {"assigneeIds": [7]},
        },
        {
            "type": "urn:ietf:params:jmap:chat:cap:task",
            "uri": "https://tracker.example/T-1",
            "metadata": {"assigneeIds": [OWNER]},
        },
        {"type": "urn:ietf:params:jmap:chat:cap:calendar-event", "uri": "JMAP:calendarevent:A9:E1"},
        {"type": "urn:example:other", "uri": "https://other.example/1"},
    ]);
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("accepted");
    let actions = &fed.messages().await[0].actions;
    let uris: Vec<&str> = actions.iter().map(|a| a.uri.as_str()).collect();
    assert_eq!(
        uris,
        ["https://tracker.example/T-1", "https://other.example/1"]
    );
    assert_eq!(actions[0].metadata, Some(json!({"assigneeIds": [ACCOUNT]})));

    let mut m = message("M2");
    m["actions"] = json!([{"type": "urn:example:other", "uri": "jmap:email:A9:M1"}]);
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("accepted");
    let messages = fed.messages().await;
    let stored = messages
        .iter()
        .find(|m| m.sender_msg_id == "M2")
        .expect("stored");
    assert!(stored.actions.is_empty());
}

/// fed L414: "Reject with `invalidArguments` if any entry's `scope` is not
/// one of `"everyone"`, `"here"`, or `"admins"`."
#[tokio::test]
async fn step13_unknown_broadcast_scope_is_rejected() {
    let fed = Fed::new();
    let mut m = message("M1");
    m["broadcastMentions"] = json!([{"scope": "all"}]);
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "invalidArguments",
    );

    let mut m = message("M2");
    m["broadcastMentions"] = json!([{"scope": "everyone"}, {"scope": "here"}, {"scope": "admins"}]);
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("the three scopes are accepted");
}

/// fed L415: "If `senderExpiresAt` is present, confirm it is strictly in
/// the future; reject with `invalidArguments` if it is in the past ...
/// Schedule hard deletion at that time. ... if `burnOnRead` is `true`,
/// register a trigger to hard-delete the message when `readAt` is set".
#[tokio::test]
async fn step14_sender_expires_at_must_be_in_the_future() {
    let fed = Fed::new();
    let mut m = message("M1");
    m["senderExpiresAt"] = json!(PAST);
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "invalidArguments",
    );
    assert!(fed.messages().await.is_empty());

    let mut m = message("M2");
    m["senderExpiresAt"] = json!(FUTURE);
    m["burnOnRead"] = json!(true);
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("accepted");
    let stored = &fed.messages().await[0];
    assert_eq!(stored.sender_expires_at, Some(UTCDate::from(FUTURE)));
    assert_eq!(stored.burn_on_read, Some(true));
}

/// fed L417: with an encrypted `bodyType` the server "MUST, at step 10,
/// reject with `invalidArguments` any message in which `attachments`,
/// `mentions`, ... is present and non-empty, and MUST skip steps 11
/// through 13."
#[tokio::test]
async fn encrypted_body_type_rejects_plaintext_metadata() {
    let fed = Fed::new();
    let mut m = message("M1");
    m["bodyType"] = json!("application/mls-ciphertext");
    m["mentions"] = json!([{"id": CAROL}]);
    assert_error(
        fed.deliver(deliver_args(LOW, "direct", m)).await,
        "invalidArguments",
    );

    let mut m = message("M2");
    m["bodyType"] = json!("application/mls-ciphertext");
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("bare ciphertext accepted");
    assert!(fed.fetched.lock().expect("lock").is_empty());
}

// ---------------------------------------------------------------------------
// Request shape, idempotency and threading
// ---------------------------------------------------------------------------

/// fed L322: "Exactly one of `message`, `edit`, or `reactionUpdate` MUST
/// be present in a given request."
#[tokio::test]
async fn exactly_one_payload_is_required() {
    let fed = Fed::new();
    let mut args = deliver_args(LOW, "direct", message("M1"));
    args["edit"] =
        json!({"senderMsgId": "M1", "body": "x", "bodyType": "text/plain", "editedAt": PAST});
    assert_error(fed.deliver(args).await, "invalidArguments");

    let mut args = deliver_args(LOW, "direct", message("M1"));
    args.as_object_mut().expect("object").remove("message");
    assert_error(fed.deliver(args).await, "invalidArguments");
}

/// fed L341: "Functions as an idempotency key; if this value is already
/// known for the given chat, the receiver MAY silently discard the
/// duplicate." fed L433: `receivedMsgId` is "the receiver's assigned ULID
/// for the stored message".
#[tokio::test]
async fn duplicate_sender_msg_id_returns_the_stored_message() {
    let fed = Fed::new();
    let first = fed
        .deliver(deliver_args(LOW, "direct", message("M1")))
        .await
        .expect("accepted");
    let second = fed
        .deliver(deliver_args(LOW, "direct", message("M1")))
        .await
        .expect("duplicate accepted");
    let messages = fed.messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(first["receivedMsgId"], json!(messages[0].id));
    assert_eq!(second["receivedMsgId"], first["receivedMsgId"]);
}

/// fed L350: "The receiver resolves this similarly. ... If the receiver
/// cannot resolve a `replyTo` or `threadRootId` senderMsgId to a local
/// message id ..., the receiver MUST store the message with `replyTo`
/// and/or `threadRootId` set to null. The message MUST NOT be rejected on
/// this basis."
#[tokio::test]
async fn reply_to_resolves_through_the_sender_msg_id_index() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed(
        "Message",
        stored_message("local-1", "S1", LOW, BOB_ID, PAST),
    );
    let mut m = message("M1");
    m["replyTo"] = json!("S1");
    m["threadRootId"] = json!("S-unknown");
    fed.deliver(deliver_args(LOW, "direct", m))
        .await
        .expect("accepted");
    let stored = fed
        .messages()
        .await
        .into_iter()
        .find(|m| m.sender_msg_id == "M1")
        .expect("stored");
    assert_eq!(stored.reply_to, Some(Id::from("local-1")));
    assert_eq!(stored.thread_root_id, None);
}

// ---------------------------------------------------------------------------
// Edit and reaction
// ---------------------------------------------------------------------------

fn edit_args(sender_msg_id: &str) -> Value {
    json!({
        "accountId": ACCOUNT,
        "chatId": LOW,
        "senderUserId": BOB,
        "chatKind": "direct",
        "edit": {"senderMsgId": sender_msg_id, "body": "new", "bodyType": "text/plain", "editedAt": FUTURE},
    })
}

/// fed L423: "Verify that the identified message exists in the given chat
/// and that its `senderId` is the sender's ChatContact `id`. Reject if the
/// sender does not match." fed L433: for an edit, `receivedMsgId` is "the
/// local `id` of the affected message".
#[tokio::test]
async fn edit_applies_only_to_the_senders_message() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed(
        "Message",
        stored_message("local-bob", "S1", LOW, BOB_ID, PAST),
    );
    fed.seed(
        "Message",
        stored_message("local-owner", "S2", LOW, ACCOUNT, PAST),
    );

    let response = fed.deliver(edit_args("S1")).await.expect("own message");
    assert_eq!(response["receivedMsgId"], json!("local-bob"));
    let messages = fed.messages().await;
    let bob = messages.iter().find(|m| m.id == "local-bob").expect("bob");
    assert_eq!(bob.body, "new");
    assert_eq!(bob.edited_at, Some(UTCDate::from(FUTURE)));

    assert_error(fed.deliver(edit_args("S2")).await, "forbidden");
    let owner = messages
        .iter()
        .find(|m| m.id == "local-owner")
        .expect("owner");
    assert_eq!(owner.body, "old");

    assert_error(
        fed.deliver(edit_args("S-missing")).await,
        "invalidArguments",
    );
}

fn reaction_args(action: &str, emoji: &str) -> Value {
    json!({
        "accountId": ACCOUNT,
        "chatId": LOW,
        "senderUserId": BOB,
        "chatKind": "direct",
        "reactionUpdate": {"senderMsgId": "S1", "senderReactionId": "R1", "emoji": emoji,
                           "action": action, "sentAt": PAST},
    })
}

/// fed L425: "Validate `emoji` as a non-empty string. For `"add"`, verify
/// the target message exists. For `"remove"`, verify the target reaction
/// exists and was originally added by the sender." fed L374 (`add`): "the
/// receiver stores the reaction in the `reactions` map using
/// `senderReactionId` as the map key."
#[tokio::test]
async fn reaction_add_and_remove() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    fed.seed(
        "Message",
        stored_message("local-1", "S1", LOW, ACCOUNT, PAST),
    );

    assert_error(
        fed.deliver(reaction_args("add", "")).await,
        "invalidArguments",
    );
    assert_error(
        fed.deliver(reaction_args("remove", "+1")).await,
        "invalidArguments",
    );

    fed.deliver(reaction_args("add", "+1")).await.expect("add");
    let reaction = fed.messages().await[0].reactions["R1"].clone();
    assert_eq!(reaction.emoji, "+1");
    assert_eq!(reaction.sender_id, Id::from(BOB_ID));

    fed.deliver(reaction_args("remove", "+1"))
        .await
        .expect("remove");
    assert!(fed.messages().await[0].reactions.is_empty());
}

/// fed L425: a reaction is removed only when it "was originally added by
/// the sender".
#[tokio::test]
async fn reaction_remove_by_another_sender_is_forbidden() {
    let fed = Fed::new();
    fed.seed("Chat", direct_chat(LOW, BOB_ID));
    let mut m = stored_message("local-1", "S1", LOW, ACCOUNT, PAST);
    m["reactions"] = json!({"R1": {"emoji": "+1", "senderId": ACCOUNT, "sentAt": PAST}});
    fed.seed("Message", m);
    assert_error(
        fed.deliver(reaction_args("remove", "+1")).await,
        "forbidden",
    );
    assert_eq!(fed.messages().await[0].reactions.len(), 1);
}
