//! EmailPush with object history in the memory backend (bd JMAP-r44n.16,
//! and the EmailPush half of JMAP-tna1.3.15).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3.1 (`objectHistory` is an
//! Email/get property only where Email has history) and §5.1 (a restore is
//! a method call); draft-ietf-jmap-emailpush-05 §3 (only deliveries push;
//! the pushed properties are a prefix of the list); docs/design/DECISIONS.md
//! decision 10.

use std::sync::Arc;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::register_mail_handlers_with_history;
use jmap_object_history_server::HistoryRetention;
use jmap_server::{Dispatcher, register_push_subscription_handlers};
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const A: &str = "A1";

fn server(types: &[&str]) -> (MemoryBackend, Dispatcher<()>) {
    let b = MemoryBackend::new().with_object_history(
        serde_json::from_value(json!({"maxHistoryDuration": null, "supportedTypes": types}))
            .unwrap(),
        HistoryRetention::new(),
    );
    b.register_account(&Id::from(A));
    let mut d = Dispatcher::new();
    register_mail_handlers_with_history(&mut d, Arc::new(b.clone()));
    register_push_subscription_handlers(&mut d, Arc::new(b.clone()));
    (b, d)
}

async fn call(d: &Dispatcher<()>, method: &str, args: Value) -> Value {
    let req: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail",
                  "urn:ietf:params:jmap:object-history", "urn:ietf:params:jmap:sieve",
                  "urn:ietf:params:jmap:emailpush"],
        "methodCalls": [[method, args, "0"]]
    }))
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = resp["methodResponses"][0].clone();
    assert_eq!(inv[0], method, "{inv}");
    inv[1].clone()
}

fn create(properties: Value) -> Value {
    json!({"create": {"c": {
        "deviceClientId": "d", "url": "https://push.example.com/x",
        "keys": {"p256dh": "k", "auth": "a"},
        "emailPush": {A: {"properties": properties}}
    }}})
}

/// Creates and verifies a subscription; its `created` entry.
async fn subscribe(b: &MemoryBackend, d: &Dispatcher<()>, properties: Value) -> Value {
    let r = call(d, "PushSubscription/set", create(properties)).await;
    let created = r["created"]["c"].clone();
    let id = created["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{r}"))
        .to_owned();
    let v = b.take_push_verifications();
    call(
        d,
        "PushSubscription/set",
        json!({"update": {&id: {"verificationCode": v[0].verification_code}}}),
    )
    .await;
    created
}

/// A new Mailbox's id.
async fn mailbox(d: &Dispatcher<()>) -> Id {
    let r = call(
        d,
        "Mailbox/set",
        json!({"accountId": A, "create": {"m": {"name": "Inbox"}}}),
    )
    .await;
    Id::from(
        r["created"]["m"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}")),
    )
}

async fn deliver(b: &MemoryBackend, mailbox: &Id, subject: &str) -> (Id, Vec<Value>) {
    let raw = format!("From: a@example.com\r\nSubject: {subject}\r\n\r\nx\r\n");
    let (id, pushes) = b
        .deliver_email(
            &Id::from(A),
            raw.as_bytes(),
            std::slice::from_ref(mailbox),
            &[],
        )
        .await
        .unwrap();
    let emails = pushes
        .into_iter()
        .flat_map(|(_, o)| o.push.emails)
        .map(Value::Object)
        .collect();
    (id, emails)
}

#[tokio::test]
async fn object_history_follows_the_capability() {
    let (b, d) = server(&["Email", "Mailbox"]);
    let created = subscribe(&b, &d, json!(["from", "objectHistory", "subject"])).await;
    assert_eq!(
        created["emailPush"][A]["properties"],
        json!(["from", "subject"]),
        "{created}"
    );
    let inbox = mailbox(&d).await;
    let (_, emails) = deliver(&b, &inbox, "Hi").await;
    assert_eq!(
        emails,
        [json!({"from": [{"email": "a@example.com"}], "subject": "Hi"})]
    );

    // Email not in supportedTypes: objectHistory is unsupported.
    let (_, d) = server(&["Mailbox"]);
    let r = call(
        &d,
        "PushSubscription/set",
        create(json!(["from", "objectHistory"])),
    )
    .await;
    assert_eq!(
        r["notCreated"]["c"]["properties"],
        json!(["emailPush", format!("emailPush/{A}/properties")]),
        "{r}"
    );
}

/// tna1.3.15: a restored Email is not a delivery, so it pushes nothing.
#[tokio::test]
async fn restore_sends_no_email_push() {
    let (b, d) = server(&["Email", "Mailbox"]);
    subscribe(&b, &d, json!(["id"])).await;
    let inbox = mailbox(&d).await;
    let (id, emails) = deliver(&b, &inbox, "Hi").await;
    assert_eq!(emails, [json!({"id": id.as_ref()})]);
    b.take_email_pushes_for_test();

    let r = call(
        &d,
        "Email/set",
        json!({"accountId": A, "destroy": [id.as_ref()]}),
    )
    .await;
    assert_eq!(r["destroyed"], json!([id.as_ref()]), "{r}");
    let r = call(
        &d,
        "Email/set",
        json!({"accountId": A, "restore": {id.as_ref(): null}}),
    )
    .await;
    assert!(r["restored"].get(id.as_ref()).is_some(), "{r}");
    assert!(b.take_email_pushes_for_test().is_empty());
}
