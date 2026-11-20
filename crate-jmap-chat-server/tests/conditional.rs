//! Conditional chat Foo/set (draft-ietf-jmap-conditional-02) through
//! `register_chat_conditional`, `register_push_subscription_conditional`
//! and the memory backend's `ScopedState` scopes (bd JMAP-y1yu.9;
//! docs/design/conditional.md §5.1, §6, §7).
//!
//! Oracles: conditional-02 §3.1, §4.2, §4.3 ("defer irreversible external
//! effects"), §9.1; draft-atwood-jmap-chat-00 §CustomEmoji (one name per
//! scope) and draft-atwood-jmap-chat-federation-00 L605 (`deletedForAll`
//! sends `Peer/retract`); RFC 8620 §7.2 (`url` and `keys` are never
//! readable) and the memory backend's push subscription cap.

mod common;

use std::sync::Arc;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::memory::MAX_PUSH_SUBSCRIPTIONS;
use jmap_chat_server::{register_chat_conditional, register_chat_handlers};
use jmap_server::{
    Dispatcher, register_push_subscription_conditional, register_push_subscription_handlers,
};
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:chat",
    "urn:ietf:params:jmap:conditional",
];

fn backend() -> Arc<MemoryBackend<()>> {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from(ACCOUNT_ID));
    Arc::new(b)
}

fn dispatcher(b: &Arc<MemoryBackend<()>>) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    register_chat_handlers(&mut d, Arc::clone(b));
    register_push_subscription_handlers(&mut d, Arc::clone(b));
    register_chat_conditional(&mut d, Arc::clone(b));
    register_push_subscription_conditional(&mut d, Arc::clone(b)).unwrap();
    d
}

/// Run `method` with `args` (accountId added unless `None`); the response
/// name and arguments.
async fn call(d: &Dispatcher<()>, method: &str, mut args: Value) -> (String, Value) {
    if !method.starts_with("PushSubscription/") {
        args["accountId"] = json!(ACCOUNT_ID);
    }
    let req: JmapRequest =
        serde_json::from_value(json!({"using": USING, "methodCalls": [[method, args, "0"]]}))
            .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

fn seed_emoji(b: &MemoryBackend<()>, id: &str, name: &str) {
    b.insert_object_for_test(
        "CustomEmoji",
        ACCOUNT_ID,
        id,
        json!({"id": id, "name": name, "blobId": "blob1", "createdBy": ACCOUNT_ID,
               "createdAt": "2026-01-01T00:00:00Z"}),
    );
}

async fn emoji_names(d: &Dispatcher<()>) -> Vec<(String, String)> {
    let (_, r) = call(d, "CustomEmoji/get", json!({})).await;
    let mut names: Vec<_> = r["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["id"].as_str().unwrap().to_owned(),
                e["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn precondition_on_an_emoji_name() {
    let b = backend();
    seed_emoji(&b, "e1", "catjam");
    let d = dispatcher(&b);
    let rename_if = |expected: &str| json!({"ifUnchangedBy": {"e1": {"name": expected}}, "update": {"e1": {"name": "dogjam"}}});
    let (name, r) = call(&d, "CustomEmoji/set", rename_if("partyparrot")).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["e1"]}))
    );
    let (name, r) = call(&d, "CustomEmoji/set", rename_if("catjam")).await;
    assert_eq!(name, "CustomEmoji/set", "{r}");
    assert_eq!(emoji_names(&d).await, [("e1".into(), "dogjam".into())]);
}

/// §4.2 and §4.3 on the main type: an atomic Message/set with one failing
/// change keeps the other unapplied, and the `Peer/retract` its
/// `deletedForAll` would owe is never queued.
#[tokio::test]
async fn atomic_failure_rolls_back_message_and_outbound_event() {
    let b = backend();
    b.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        "m1",
        json!({
            "id": "m1", "senderMsgId": "smid-m1", "senderId": ACCOUNT_ID, "chatId": "dm",
            "body": "hello", "bodyType": "text/plain", "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z", "receivedAt": "2026-01-01T00:00:00Z",
            "deliveryState": "delivered"
        }),
    );
    let d = dispatcher(&b);
    let (_, before) = call(&d, "Message/get", json!({"ids": ["m1"]})).await;

    let (name, r) = call(
        &d,
        "Message/set",
        json!({
            "atomic": true,
            "update": {
                "m1": {"deletedAt": "2026-01-02T00:00:00Z", "deletedForAll": true},
                "nope": {"body": "x"}
            }
        }),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert_eq!(r["notUpdated"]["nope"]["type"], "notFound", "{r}");

    let (_, after) = call(&d, "Message/get", json!({"ids": ["m1"]})).await;
    assert_eq!(after, before, "m1 and the state string rolled back");
    assert!(
        b.take_outbound_events_for_test().is_empty(),
        "no Peer/retract"
    );

    // Without the failing change the retract is queued as before.
    let (name, r) = call(
        &d,
        "Message/set",
        json!({"atomic": true, "update": {"m1": {"deletedAt": "2026-01-02T00:00:00Z", "deletedForAll": true}}}),
    )
    .await;
    assert_eq!(name, "Message/set", "{r}");
    assert_eq!(b.take_outbound_events_for_test().len(), 1);
}

/// Emoji names are unique per scope; under atomic the check runs on the
/// final state, so a swap commits, while a create that leaves a duplicate
/// is rejected at commit.
#[tokio::test]
async fn atomic_emoji_names_checked_on_final_state() {
    let swap = |atomic: bool| json!({"atomic": atomic, "update": {"e1": {"name": "b"}, "e2": {"name": "a"}}});

    let b = backend();
    seed_emoji(&b, "e1", "a");
    seed_emoji(&b, "e2", "b");
    let d = dispatcher(&b);
    let (_, r) = call(&d, "CustomEmoji/set", swap(false)).await;
    assert!(
        r["notUpdated"].as_object().is_some_and(|m| !m.is_empty()),
        "{r}"
    );

    let b = backend();
    seed_emoji(&b, "e1", "a");
    seed_emoji(&b, "e2", "b");
    let d = dispatcher(&b);
    let (name, r) = call(&d, "CustomEmoji/set", swap(true)).await;
    assert_eq!(name, "CustomEmoji/set", "{r}");
    assert_eq!(
        emoji_names(&d).await,
        [("e1".into(), "b".into()), ("e2".into(), "a".into())]
    );

    let (name, r) = call(
        &d,
        "CustomEmoji/set",
        json!({"atomic": true, "create": {"n": {"name": "a", "blobId": "blob1"}}}),
    )
    .await;
    assert_eq!(
        (name.as_str(), r),
        (
            "error",
            json!({"type": "atomicFailure", "notCreated": {
                "n": {"type": "alreadyExists", "existingId": "e2"}
            }})
        )
    );
    assert_eq!(emoji_names(&d).await.len(), 2);
}

fn subscription(n: usize) -> Value {
    json!({"deviceClientId": format!("d{n}"), "url": format!("https://example.com/{n}")})
}

/// A per-user cap met only by the final state: at the push subscription
/// cap, replacing one needs `atomic`, because PushSubscription/set runs
/// creates before destroys.
#[tokio::test]
async fn atomic_push_cap_checked_on_final_state() {
    let b = backend();
    let d = dispatcher(&b);
    let create: serde_json::Map<String, Value> = (0..MAX_PUSH_SUBSCRIPTIONS)
        .map(|n| (format!("c{n}"), subscription(n)))
        .collect();
    let (_, r) = call(&d, "PushSubscription/set", json!({"create": create})).await;
    let first = r["created"]["c0"]["id"].as_str().unwrap().to_owned();

    let replace = |atomic: bool| json!({"atomic": atomic, "create": {"new": subscription(99)}, "destroy": [&first]});
    let (_, r) = call(&d, "PushSubscription/set", replace(false)).await;
    assert_eq!(r["notCreated"]["new"]["type"], "overQuota", "{r}");
    // Each change stood alone: the destroy ran, so the user lost a
    // subscription without gaining one.
    assert_eq!(r["destroyed"], json!([first]), "{r}");
}

#[tokio::test]
async fn atomic_push_replace_at_the_cap() {
    let b = backend();
    let d = dispatcher(&b);
    let create: serde_json::Map<String, Value> = (0..MAX_PUSH_SUBSCRIPTIONS)
        .map(|n| (format!("c{n}"), subscription(n)))
        .collect();
    let (_, r) = call(&d, "PushSubscription/set", json!({"create": create})).await;
    let first = r["created"]["c0"]["id"].as_str().unwrap().to_owned();

    let (name, r) = call(
        &d,
        "PushSubscription/set",
        json!({"atomic": true, "create": {"new": subscription(99)}, "destroy": [&first]}),
    )
    .await;
    assert_eq!(name, "PushSubscription/set", "{r}");
    assert_eq!(r["destroyed"], json!([first]));

    // Over the cap in the final state: the commit rejects the create.
    let (name, r) = call(
        &d,
        "PushSubscription/set",
        json!({"atomic": true, "create": {"extra": subscription(100)}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert_eq!(r["notCreated"]["extra"]["type"], "overQuota", "{r}");
    let (_, r) = call(&d, "PushSubscription/get", json!({})).await;
    assert_eq!(r["list"].as_array().unwrap().len(), MAX_PUSH_SUBSCRIPTIONS);
}

/// §9.1 on chat memory: `keys` is never readable, so a precondition on it
/// is `forbidden`.
#[tokio::test]
async fn push_precondition_on_keys_is_forbidden() {
    let b = backend();
    let d = dispatcher(&b);
    let (_, r) = call(
        &d,
        "PushSubscription/set",
        json!({"create": {"c": subscription(1)}}),
    )
    .await;
    let id = r["created"]["c"]["id"].as_str().unwrap().to_owned();
    let (name, r) = call(
        &d,
        "PushSubscription/set",
        json!({"ifUnchangedBy": {&id: {"keys": null}}, "destroy": [&id]}),
    )
    .await;
    assert_eq!(
        (name.as_str(), &r["type"]),
        ("error", &json!("forbidden")),
        "{r}"
    );
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = backend();
    let mut d = Dispatcher::new();
    register_chat_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps().len(), 10, "{:?}", d.conditional_gaps());
    register_chat_conditional(&mut d, Arc::clone(&b));
    assert!(
        d.conditional_gaps().is_empty(),
        "{:?}",
        d.conditional_gaps()
    );
    assert!(d.conditional_ready());

    register_push_subscription_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps(), ["PushSubscription/set"]);
    register_push_subscription_conditional(&mut d, b).unwrap();
    assert!(d.conditional_ready());
}

/// Creates run before updates, so a create may take an emoji name that an
/// update in the same call gives up only under `atomic`, where uniqueness
/// is judged on the final state.
#[tokio::test]
async fn atomic_create_takes_a_name_given_up_in_the_same_call() {
    let handover = |atomic: bool| {
        json!({
            "atomic": atomic,
            "create": {"n": {"name": "a", "blobId": "blob1"}},
            "update": {"e1": {"name": "old-a"}}
        })
    };
    let b = backend();
    seed_emoji(&b, "e1", "a");
    let d = dispatcher(&b);
    let (_, r) = call(&d, "CustomEmoji/set", handover(false)).await;
    assert_eq!(r["notCreated"]["n"]["type"], "alreadyExists", "{r}");

    let b = backend();
    seed_emoji(&b, "e1", "a");
    let d = dispatcher(&b);
    let (name, r) = call(&d, "CustomEmoji/set", handover(true)).await;
    assert_eq!(name, "CustomEmoji/set", "{r}");
    let mut names: Vec<String> = emoji_names(&d).await.into_iter().map(|(_, n)| n).collect();
    names.sort();
    assert_eq!(names, ["a", "old-a"]);
}
