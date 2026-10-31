//! Caller-identity binding and recipient routing for every `Peer/*`
//! method (draft-atwood-jmap-chat-federation-00 §Identity Binding,
//! §Server-to-Server Methods, §Peer/subscribePresence, §Error Handling;
//! jmap-chat-compliance-tests 7.16 and 7.20).
//!
//! "fed Lnnn" cites draft-atwood-jmap-chat-federation-00.md and "7.nn"
//! jmap-chat-compliance-tests.md, both at jmap-chat-spec c08568e.
//! Oracle: the quoted draft and compliance-test text, the draft's
//! `forbidden` example (fed L1248-1275) and hand-written JSON records.

mod common;

use std::sync::Arc;

use common::*;
use jmap_chat_federation_server::{
    handle_peer_deliver, handle_peer_group_update, handle_peer_presence, handle_peer_receipt,
    handle_peer_retract, handle_peer_subscribe_presence, handle_peer_typing,
    handle_peer_unsubscribe_presence,
};
use jmap_chat_types::{ChatContact, Message};
use jmap_server::{CallContext, ClosureHandler, Dispatcher, HandlerFuture};
use jmap_types::{JmapError, JmapRequest};
use serde_json::{Value, json};

/// The did:key example of the W3C did:key Method specification §Format,
/// as in 7.16.
const DID_KEY: &str = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";
/// The local ChatContact id of the caller identity under test.
const CALLER_ID: &str = "contact-caller";
/// The owner's Message a `Peer/receipt` names.
const OWNER_SMID: &str = "01J00000000000000000000SM1";
/// The caller's Message a `Peer/retract` names.
const CALLER_SMID: &str = "01J00000000000000000000SM2";

/// Call the handler for `method`.
async fn call(fed: &Fed, method: &str, args: Value) -> Result<Value, JmapError> {
    match method {
        "Peer/deliver" => fed.deliver(args).await,
        "Peer/receipt" => fed.receipt(args).await,
        "Peer/typing" => fed.typing(args).await,
        "Peer/retract" => fed.retract(args).await,
        "Peer/groupUpdate" => fed.group_update(args).await,
        "Peer/subscribePresence" => fed.subscribe_presence(args).await,
        "Peer/unsubscribePresence" => fed.unsubscribe_presence(args).await,
        "Peer/presence" => fed.presence(args).await,
        _ => panic!("no such method {method}"),
    }
}

/// One call of each `Peer/*` method to [`OWNER`]'s mailbox, each naming
/// `identity` in its caller-identity field (fed L402: "`senderUserId`,
/// `readerUserId`, `subscriberId`, or `identity` depending on the
/// method").
fn calls(identity: &str) -> Vec<(&'static str, Value)> {
    let base = |extra: Value| {
        let mut args = json!({"accountId": PEER_ACCOUNT, "recipientUserId": OWNER});
        args.as_object_mut()
            .expect("object")
            .extend(extra.as_object().expect("object").clone());
        args
    };
    vec![
        (
            "Peer/deliver",
            base(json!({
                "chatId": LOW,
                "senderUserId": identity,
                "chatKind": "direct",
                "message": {"senderMsgId": "M1", "body": "hi", "bodyType": "text/plain",
                            "sentAt": PAST, "attachments": []},
            })),
        ),
        (
            "Peer/receipt",
            base(json!({"senderMsgId": OWNER_SMID, "readerUserId": identity, "readAt": FUTURE})),
        ),
        (
            "Peer/typing",
            base(json!({"chatId": LOW, "senderUserId": identity, "typing": true})),
        ),
        (
            "Peer/retract",
            base(json!({"chatId": LOW, "senderUserId": identity, "senderMsgId": CALLER_SMID})),
        ),
        (
            "Peer/groupUpdate",
            base(json!({"chatId": GROUP, "senderUserId": identity, "action": "leave"})),
        ),
        (
            "Peer/subscribePresence",
            base(json!({"subscriberId": identity})),
        ),
        (
            "Peer/unsubscribePresence",
            base(json!({"subscriberId": identity})),
        ),
        (
            "Peer/presence",
            base(json!({"identity": identity, "presence": "away"})),
        ),
    ]
}

/// A Fed in which `identity` is a known ChatContact of [`OWNER`] with a
/// direct Chat, a Message of its own, a Message of the owner's it can
/// acknowledge, and membership of a group, so that each call of
/// [`calls`] can succeed when `identity` is a peer identity.
fn fed_knowing(identity: &str) -> Fed {
    let fed = Fed::new();
    fed.seed("ChatContact", contact(CALLER_ID, identity, false));
    fed.seed("Chat", direct_chat(LOW, CALLER_ID));
    fed.seed("Chat", group_chat(&[ACCOUNT, CALLER_ID], false));
    fed.seed(
        "Message",
        stored_message("m-owner", OWNER_SMID, LOW, ACCOUNT, PAST),
    );
    fed.seed(
        "Message",
        stored_message("m-caller", CALLER_SMID, LOW, CALLER_ID, PAST),
    );
    fed
}

/// Everything a call could write: both mailboxes' records, the presence
/// subscriptions, and the events sent to the owners' clients.
async fn state(fed: &Fed) -> Value {
    let mut out = json!({});
    for account in [ACCOUNT, ACCOUNT2] {
        out[account] = json!({
            "messages": fed.all_in::<Message>(account).await,
            "contacts": fed.all_in::<ChatContact>(account).await,
            "chats": fed.all_in::<jmap_chat_types::Chat>(account).await,
        });
    }
    let mut subs: Vec<_> = fed.subscriptions().into_iter().collect();
    subs.sort();
    out["subscriptions"] = json!(subs);
    out["typed"] = json!(fed.typed());
    out["presences"] = json!(fed.presences());
    out
}

/// Every call of [`calls`]`(identity)` is `forbidden` and changes nothing.
async fn assert_all_forbidden(fed: &Fed, identity: &str) {
    for (method, args) in calls(identity) {
        let before = state(fed).await;
        assert_error(call(fed, method, args).await, "forbidden");
        assert_eq!(state(fed).await, before, "{method} wrote state");
    }
}

/// Every call of [`calls`]`(identity)` succeeds.
async fn assert_all_accepted(fed: &Fed, identity: &str) {
    for (method, args) in calls(identity) {
        if let Err(e) = call(fed, method, args).await {
            panic!("{method} from {identity}: {e:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// 7.16 Caller identity host must equal peer domain
// ---------------------------------------------------------------------------

/// 7.16: "Send correctly signed `Peer/deliver` calls from `a.example` with
/// `senderUserId` `alice@A.Example` and then `carol@a.example`, two users
/// of one peer domain. Verify both succeed." fed L335: "Hosts are compared
/// as peer domains are", and fed L162: "A peer domain is compared in
/// lowercase A-label form". fed L340: the ChatContact's `identity` is "the
/// same string that the user's own server uses".
#[tokio::test]
async fn two_users_of_one_peer_domain_are_both_accepted() {
    let mut fed = Fed::new();
    fed.peer = Some(peer_at("a.example"));
    for (identity, chat_id, smid) in [
        ("alice@A.Example", LOW, "M1"),
        ("carol@a.example", HIGH, "M2"),
    ] {
        let args = json!({
            "accountId": PEER_ACCOUNT,
            "recipientUserId": OWNER,
            "chatId": chat_id,
            "senderUserId": identity,
            "chatKind": "direct",
            "message": {"senderMsgId": smid, "body": "hi", "bodyType": "text/plain",
                        "sentAt": PAST, "attachments": []},
        });
        fed.deliver(args).await.expect(identity);
    }
    let contacts = fed.contacts().await;
    let mut senders: Vec<String> = fed
        .messages()
        .await
        .into_iter()
        .map(|m| {
            contacts
                .iter()
                .find(|c| c.id == m.sender_id)
                .expect("sender contact")
                .identity
                .clone()
        })
        .collect();
    senders.sort();
    assert_eq!(senders, ["alice@A.Example", "carol@a.example"]);
}

/// The calls of [`calls`] are valid apart from the identity: from the
/// identity's own peer domain every one succeeds. This keeps the
/// `forbidden` results below attributable to identity binding.
#[tokio::test]
async fn every_call_succeeds_from_the_identitys_own_domain() {
    let mut fed = fed_knowing("bob@b.example");
    fed.peer = Some(peer_at("b.example"));
    assert_all_accepted(&fed, "bob@b.example").await;
}

/// 7.16: "Send one with `senderUserId: \"bob@b.example\"`. Verify
/// `forbidden` and no placeholder ChatContact. ... Send `Peer/receipt`
/// with `readerUserId: \"bob@b.example\"` and `Peer/typing` with
/// `senderUserId: \"bob@b.example\"` from `a.example`. Verify `forbidden`
/// for each." fed L402: "Servers MUST check every such field by
/// {{identity-binding}}". Every caller-identity field of every method is
/// checked, even though the identity is a known ChatContact.
#[tokio::test]
async fn another_domains_identity_is_forbidden_in_every_method() {
    let mut fed = fed_knowing("bob@b.example");
    fed.peer = Some(peer_at("a.example"));
    assert_all_forbidden(&fed, "bob@b.example").await;

    let mut unknown = Fed::new();
    unknown.peer = Some(peer_at("a.example"));
    assert_all_forbidden(&unknown, "bob@b.example").await;
}

/// 7.16: "Repeat with `senderUserId`
/// `did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK` and no DID
/// companion support. Verify `forbidden`." fed L336: "The local server
/// MUST NOT accept such an identity on the strength of peer authentication
/// alone." The harness, like the trait default, verifies no DID.
#[tokio::test]
async fn host_less_did_is_forbidden_by_default_in_every_method() {
    let mut fed = fed_knowing(DID_KEY);
    fed.peer = Some(peer_at("a.example"));
    assert_all_forbidden(&fed, DID_KEY).await;
}

/// fed L336: "A DID-form identity is accepted only after the DID-bound
/// verification of {{JMAP-CHAT-DID}} succeeds". Once
/// `verify_did_identity` reports success, every method accepts it.
#[tokio::test]
async fn host_less_did_is_accepted_after_did_verification() {
    let mut fed = fed_knowing(DID_KEY);
    fed.peer = Some(peer_at("a.example"));
    fed.verified_dids.insert(DID_KEY.to_owned());
    assert_all_accepted(&fed, DID_KEY).await;
}

// ---------------------------------------------------------------------------
// 7.20 Peer/* calls name the recipient by identity
// ---------------------------------------------------------------------------

/// A dispatcher with every `Peer/*` handler over `fed`.
fn dispatcher(fed: Arc<Fed>) -> Dispatcher<()> {
    macro_rules! register {
        ($d:ident, $method:literal, $handler:path) => {
            $d.register(
                $method,
                Arc::new(ClosureHandler::new(
                    Arc::clone(&fed),
                    |b: Arc<Fed>, cx: CallContext<()>| -> HandlerFuture {
                        Box::pin(async move { $handler(&*b, &cx.caller, cx.args).await })
                    },
                )),
            );
        };
    }
    let mut d = Dispatcher::new();
    register!(d, "Peer/deliver", handle_peer_deliver);
    register!(d, "Peer/receipt", handle_peer_receipt);
    register!(d, "Peer/typing", handle_peer_typing);
    register!(d, "Peer/retract", handle_peer_retract);
    register!(d, "Peer/groupUpdate", handle_peer_group_update);
    register!(d, "Peer/subscribePresence", handle_peer_subscribe_presence);
    register!(
        d,
        "Peer/unsubscribePresence",
        handle_peer_unsubscribe_presence
    );
    register!(d, "Peer/presence", handle_peer_presence);
    d
}

/// Dispatch the JMAP Request `request` and return the JMAP Response.
async fn dispatch(fed: Arc<Fed>, request: Value, session_state: &str) -> Value {
    let request: JmapRequest = serde_json::from_value(request).expect("JMAP Request");
    let response = dispatcher(fed)
        .dispatch(request, (), session_state.into())
        .await;
    serde_json::to_value(response).expect("JMAP Response serializes")
}

fn new_direct_message(recipient: &str, chat_id: &str, smid: &str) -> Value {
    json!({
        "accountId": PEER_ACCOUNT,
        "recipientUserId": recipient,
        "chatId": chat_id,
        "senderUserId": BOB,
        "chatKind": "direct",
        "message": {"senderMsgId": smid, "body": smid, "bodyType": "text/plain",
                    "sentAt": PAST, "attachments": []},
    })
}

/// 7.20: "Send one request holding two `Peer/deliver` method calls with
/// that account's id as `accountId`: one with `recipientUserId:
/// \"u1@<server>\"`, one with `recipientUserId: \"u2@<server>\"`, each a
/// new direct message with a distinct `senderMsgId`. Verify both succeed
/// and return that `accountId`. ... Verify each mailbox holds only the
/// message addressed to it." fed L404: "the `accountId` argument of every
/// Peer/* call is the id of the single peer-role account in the caller's
/// session, and every response returns it."
#[tokio::test]
async fn one_request_delivers_to_two_mailboxes() {
    let fed = Arc::new(Fed::new());
    let response = dispatch(
        Arc::clone(&fed),
        json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
            "methodCalls": [
                ["Peer/deliver", new_direct_message(OWNER, LOW, "M-U1"), "c1"],
                ["Peer/deliver", new_direct_message(OWNER2, HIGH, "M-U2"), "c2"],
            ],
        }),
        "s1",
    )
    .await;
    let responses = response["methodResponses"].as_array().expect("responses");
    assert_eq!(responses.len(), 2);
    for (r, call_id) in responses.iter().zip(["c1", "c2"]) {
        assert_eq!(r[0], "Peer/deliver", "{r}");
        assert_eq!(r[1]["accountId"], PEER_ACCOUNT, "{r}");
        assert_eq!(r[2], call_id);
    }
    let smids = |account| {
        let fed = Arc::clone(&fed);
        async move {
            fed.all_in::<Message>(account)
                .await
                .into_iter()
                .map(|m| m.sender_msg_id)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(smids(ACCOUNT).await, ["M-U1"]);
    assert_eq!(smids(ACCOUNT2).await, ["M-U2"]);
}

/// 7.20: "Send `Peer/deliver` with `recipientUserId: \"nobody@<server>\"`.
/// Verify `forbidden` and that nothing is stored and no ChatContact is
/// created." fed L404: "If no mailbox on the receiving server has that
/// identity, the receiving server rejects the call with `forbidden`." Dave
/// has a ChatContact in neither mailbox, and the message mentions Carol,
/// so an accepted call would create placeholders.
#[tokio::test]
async fn unknown_recipient_is_forbidden_and_writes_nothing() {
    let fed = Fed::new();
    let before = state(&fed).await;
    let mut args = new_direct_message("nobody@home.example", LOW, "M1");
    args["senderUserId"] = json!("dave@peer.example");
    args["message"]["mentions"] = json!([{"id": CAROL}]);
    let err = fed.deliver(args).await.expect_err("forbidden");
    assert_eq!(err, JmapError::forbidden());
    assert_eq!(state(&fed).await, before);
}

/// fed L404: "compared octet for octet". An identity that differs from an
/// owner's only in case names no mailbox.
#[tokio::test]
async fn recipient_is_matched_octet_for_octet() {
    let fed = Fed::new();
    let before = state(&fed).await;
    let err = fed
        .deliver(new_direct_message("Owner@home.example", LOW, "M1"))
        .await
        .expect_err("forbidden");
    assert_eq!(err, JmapError::forbidden());
    assert_eq!(state(&fed).await, before);
}

/// 7.20: "Send `Peer/typing` with `recipientUserId: \"u1@<server>\"` and
/// a `chatId` the sender is not a member of. Verify `forbidden`, and that
/// its error object equals the `nobody@<server>` one apart from any
/// per-request value (SHOULD)." fed L404: "It SHOULD give that rejection
/// the same form, including any `description`, as a `forbidden` rejection
/// of a call to an existing mailbox".
#[tokio::test]
async fn unknown_recipient_error_equals_a_non_member_error() {
    let fed = Fed::new();
    fed.seed("Chat", group_chat(&[ACCOUNT], false));
    let typing = |recipient: &str| {
        json!({"accountId": PEER_ACCOUNT, "recipientUserId": recipient, "chatId": GROUP,
               "senderUserId": BOB, "typing": true})
    };
    let non_member = fed.typing(typing(OWNER)).await.expect_err("non-member");
    let nobody = fed
        .typing(typing("nobody@home.example"))
        .await
        .expect_err("nobody");
    assert_eq!(non_member, nobody);
    let deliver_nobody = fed
        .deliver(new_direct_message("nobody@home.example", LOW, "M1"))
        .await
        .expect_err("nobody");
    assert_eq!(deliver_nobody, nobody);
    assert!(fed.typed().is_empty());
}

/// fed L404: "the `accountId` argument of every Peer/* call is the id of
/// the single peer-role account in the caller's session". 7.20, common
/// failure: "routes by `accountId` and ignores `recipientUserId`". A
/// mailbox's own account id is not the peer's account, so the call is
/// `accountNotFound` (RFC 8620 §3.6.2) and writes nothing.
#[tokio::test]
async fn account_other_than_the_peer_account_is_account_not_found() {
    for account in [ACCOUNT, ACCOUNT2, "X9"] {
        let fed = Fed::new();
        let before = state(&fed).await;
        let mut args = new_direct_message(OWNER, LOW, "M1");
        args["accountId"] = json!(account);
        assert_error(fed.deliver(args).await, "accountNotFound");
        assert_eq!(state(&fed).await, before, "{account}");
    }
}

// ---------------------------------------------------------------------------
// Presence subscriptions per recipient
// ---------------------------------------------------------------------------

fn presence_args(recipient: &str) -> Value {
    json!({"accountId": PEER_ACCOUNT, "recipientUserId": recipient, "subscriberId": BOB})
}

/// fed L1049: "A subscription is identified by its `subscriberId` and
/// `recipientUserId`." One subscriber's subscriptions to two mailboxes are
/// two subscriptions; cancelling one leaves the other.
#[tokio::test]
async fn subscriptions_to_two_mailboxes_are_distinct() {
    let fed = Fed::new();
    for recipient in [OWNER, OWNER2] {
        let r = fed
            .subscribe_presence(presence_args(recipient))
            .await
            .expect("subscribed");
        assert_eq!(r["accountId"], PEER_ACCOUNT);
    }
    let mut keys: Vec<_> = fed.subscriptions().into_keys().collect();
    keys.sort();
    assert_eq!(keys, [sub(ACCOUNT, BOB), sub(ACCOUNT2, BOB)]);

    fed.unsubscribe_presence(presence_args(OWNER2))
        .await
        .expect("unsubscribed");
    let keys: Vec<_> = fed.subscriptions().into_keys().collect();
    assert_eq!(keys, [sub(ACCOUNT, BOB)]);
}

/// fed L1049: "Which subscriptions a server accepts is
/// implementation-defined; a server that refuses one rejects the call with
/// `forbidden`."
#[tokio::test]
async fn refused_subscription_is_forbidden() {
    let mut fed = Fed::new();
    fed.refuse_subscriptions = true;
    assert_error(
        fed.subscribe_presence(presence_args(OWNER)).await,
        "forbidden",
    );
    assert!(fed.subscriptions().is_empty());
}

/// fed L404: an unknown recipient is `forbidden` for the presence methods
/// too, and records nothing.
#[tokio::test]
async fn subscription_to_unknown_recipient_is_forbidden() {
    let fed = Fed::new();
    assert_error(
        fed.subscribe_presence(presence_args("nobody@home.example"))
            .await,
        "forbidden",
    );
    assert!(fed.subscriptions().is_empty());
}

// ---------------------------------------------------------------------------
// The draft's example
// ---------------------------------------------------------------------------

/// fed L1248-1275: "`bob.example` hosts no mailbox for
/// `nobody@bob.example`, so it rejects this call with `forbidden`". The
/// request and response are the draft's, verbatim: the server here hosts
/// mailboxes for other owners, `alice.example` signed the request, and
/// `P5f2` is its peer-role account (fed L408).
#[tokio::test]
async fn draft_forbidden_example() {
    let mut fed = Fed::new();
    fed.peer = Some(peer_at("alice.example"));
    let request = json!({
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
    });
    let expected = json!({
      "methodResponses": [
        ["error", {
          "type": "forbidden"
        }, "c1"]
      ],
      "sessionState": "b7q3"
    });
    assert_eq!(dispatch(Arc::new(fed), request, "b7q3").await, expected);
}
