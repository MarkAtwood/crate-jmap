//! EmailPush through the memory backend and the dispatcher
//! (bd JMAP-r44n.12; r44n.3's revocation cases).
//!
//! Oracles: draft-ietf-jmap-emailpush-05 §5 Figures 2 and 3 (copied by
//! hand; Figure 2's RFC 8792-wrapped `p256dh` unwrapped), §3 ("delivered"
//! means the EmailDelivery conditions, not a JMAP method call) and §4
//! (removing a lost account from the map); RFC 8620 §5.3 echoes and
//! §7.2.2 verification; docs/design/DECISIONS.md decisions 9, 10 and 16.

use std::sync::Arc;

use jmap_mail_server::emailpush::EmailPushAccount;
use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::register_mail_handlers;
use jmap_server::{Dispatcher, register_push_subscription_handlers};
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "A1412";
const INBOX: &str = "674cc240-95db-49ce-a8a2-054f4f733095";

struct Server {
    b: MemoryBackend,
    d: Dispatcher<()>,
}

impl Server {
    fn new() -> Self {
        let b = MemoryBackend::new();
        b.register_account(&Id::from(ACCOUNT));
        let mut d = Dispatcher::new();
        register_mail_handlers(&mut d, Arc::new(b.clone()));
        register_push_subscription_handlers(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    async fn call(&self, method: &str, args: Value) -> Value {
        let req: JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail",
                      "urn:ietf:params:jmap:emailpush"],
            "methodCalls": [[method, args, "0"]]
        }))
        .unwrap();
        let resp = serde_json::to_value(self.d.dispatch(req, (), State::from("s")).await).unwrap();
        let inv = resp["methodResponses"][0].clone();
        assert_eq!(inv[0], method, "{inv}");
        inv[1].clone()
    }

    /// Creates a subscription with Figure 2's fields and `email_push`, and
    /// verifies it with the queued PushVerification. Returns its id and
    /// the `created` entry.
    async fn subscribe(&self, email_push: Value) -> (String, Value) {
        let r = self
            .call(
                "PushSubscription/set",
                json!({"create": {"4f29": {
                    "deviceClientId": "a889-ffea-910",
                    "url": "https://push.example.com/WmYD3enMWsSOE0nd",
                    "keys": {
                        "p256dh": "BM0HH37xD8sEh8NeiHu1hFuwxqeeruxPlX5S7XGWGjHY5UT5yZFQKAiihFQLP6GuEQPQOtD9z28HRyV7e1nndh8",
                        "auth": "uwPFKACOLBipGPTVz4UCDg"
                    },
                    "types": [],
                    "emailPush": email_push
                }}}),
            )
            .await;
        let created = r["created"]["4f29"].clone();
        let id = created["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}"))
            .to_owned();
        let v = self.b.take_push_verifications();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].push_subscription_id.as_ref(), id);
        let r = self
            .call(
                "PushSubscription/set",
                json!({"update": {&id: {"verificationCode": v[0].verification_code}}}),
            )
            .await;
        assert!(r["updated"].get(&id).is_some(), "{r}");
        (id, created)
    }

    async fn deliver(&self, subject: &str, mailbox: &str) -> (Id, Vec<Value>) {
        let raw = format!(
            "From: John Smith <john@example.com>\r\nTo: jane@example.com\r\n\
             Subject: {subject}\r\nMessage-ID: <{}@example.com>\r\n\r\nSee you there.\r\n",
            uuid_like(subject)
        );
        let (id, pushes) = self
            .b
            .deliver_email(
                &Id::from(ACCOUNT),
                raw.as_bytes(),
                &[Id::from(mailbox)],
                &[],
            )
            .await
            .unwrap();
        (
            id,
            pushes
                .into_iter()
                .map(|(_, o)| serde_json::to_value(&o.push).unwrap())
                .collect(),
        )
    }
}

/// A Message-ID local part unique per subject.
fn uuid_like(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn figure_2_email_push() -> Value {
    json!({
      "A1412": {
        "filter": {
          "operator": "OR",
          "conditions": [
            { "inMailbox": INBOX },
            { "hasKeyword": "$notify" }
          ]
        },
        "properties": ["id", "from", "subject"]
      }
    })
}

/// §5 end to end: the IMAP-with-ObjectID account, Figure 2's request, a
/// delivery to the inbox, Figure 3's EmailPush (with this server's id).
#[tokio::test]
async fn section_5_end_to_end() {
    let s = Server::new();
    s.b.set_email_push_account_for_test(
        &Id::from(ACCOUNT),
        Some(EmailPushAccount::new(false).with_object_ids(true)),
    );
    let (_, created) = s.subscribe(figure_2_email_push()).await;
    // Decision 9: the default urgency is filled and echoed.
    let mut stored = figure_2_email_push();
    stored["A1412"]["urgency"] = json!("normal");
    assert_eq!(created["emailPush"], stored, "{created}");

    let (id, pushes) = s
        .deliver("Surprise birthday party for Jane tonight", INBOX)
        .await;
    assert_eq!(
        pushes,
        [json!({
          "@type": "EmailPush",
          "accountId": "A1412",
          "emails": [{
            "id": id.as_ref(),
            "from": [{ "name": "John Smith", "email": "john@example.com" }],
            "subject": "Surprise birthday party for Jane tonight"
          }],
          "state": null
        })]
    );
    // Not in the inbox and no $notify: nothing.
    let (_, pushes) = s.deliver("Elsewhere", "archive").await;
    assert!(pushes.is_empty());
}

#[tokio::test]
async fn unverified_subscription_gets_nothing() {
    let s = Server::new();
    let r = s
        .call(
            "PushSubscription/set",
            json!({"create": {"c": {
                "deviceClientId": "d", "url": "https://push.example.com/x",
                "keys": {"p256dh": "k", "auth": "a"},
                "emailPush": {"A1412": {}}
            }}}),
        )
        .await;
    assert!(r["created"]["c"]["id"].is_string(), "{r}");
    let (_, pushes) = s.deliver("Hi", INBOX).await;
    assert!(pushes.is_empty());
}

/// With mail (the default account), `state` is the Email state after the
/// delivery.
#[tokio::test]
async fn mail_account_state_is_the_email_state() {
    let s = Server::new();
    s.subscribe(json!({"A1412": {"properties": ["subject"]}}))
        .await;
    let (_, pushes) = s.deliver("Hi", INBOX).await;
    let got = s
        .call("Email/get", json!({"accountId": ACCOUNT, "ids": []}))
        .await;
    assert_eq!(pushes[0]["state"], got["state"]);
    assert_eq!(pushes[0]["emails"], json!([{"subject": "Hi"}]));
}

/// §3: an Email created by a JMAP method call is not delivered.
#[tokio::test]
async fn import_and_set_create_push_nothing() {
    let s = Server::new();
    s.subscribe(json!({"A1412": {}})).await;
    let raw = b"From: a@example.com\r\nSubject: imported\r\n\r\nx\r\n".to_vec();
    s.b.store_blob(&Id::from("blob1"), raw);
    let r = s
        .call(
            "Email/import",
            json!({"accountId": ACCOUNT, "emails": {"i": {
                "blobId": "blob1", "mailboxIds": {INBOX: true}}}}),
        )
        .await;
    assert!(r["created"]["i"]["id"].is_string(), "{r}");
    let r = s
        .call(
            "Email/set",
            json!({"accountId": ACCOUNT, "create": {"c": {
                "mailboxIds": {INBOX: true}, "subject": "created"}}}),
        )
        .await;
    assert!(r["created"]["c"]["id"].is_string(), "{r}");
    assert!(s.b.take_email_pushes_for_test().is_empty());
    s.deliver("delivered", INBOX).await;
    assert_eq!(s.b.take_email_pushes_for_test().len(), 1);
}

/// Decision 10.
#[tokio::test]
async fn object_history_property() {
    let s = Server::new();
    let props = json!({"A1412": {"properties": ["from", "objectHistory", "subject"]}});
    let r = s
        .call(
            "PushSubscription/set",
            json!({"create": {"c": {
                "deviceClientId": "d", "url": "https://push.example.com/x",
                "keys": {"p256dh": "k", "auth": "a"}, "emailPush": props
            }}}),
        )
        .await;
    assert_eq!(
        r["notCreated"]["c"]["properties"],
        json!(["emailPush", "emailPush/A1412/properties"]),
        "{r}"
    );
    s.b.set_email_push_account_for_test(
        &Id::from(ACCOUNT),
        Some(
            EmailPushAccount::new(true)
                .with_object_ids(true)
                .with_email_history(true),
        ),
    );
    let (_, created) = s.subscribe(props).await;
    assert_eq!(
        created["emailPush"]["A1412"]["properties"],
        json!(["from", "subject"])
    );
}

/// r44n.3 after revoking R.
#[tokio::test]
async fn revoked_account() {
    let s = Server::new();
    s.b.register_account(&Id::from("R"));
    let normal = json!({"filter": null, "properties": [], "urgency": "normal"});
    let (id, _) = s.subscribe(json!({"A1412": {}, "R": {}})).await;
    s.b.set_email_push_account_for_test(&Id::from("R"), None);

    // /get lacks R.
    let r = s
        .call(
            "PushSubscription/get",
            json!({"ids": [&id], "properties": ["emailPush"]}),
        )
        .await;
    assert_eq!(r["list"][0]["emailPush"], json!({"A1412": normal}), "{r}");

    // A patch through R: invalidPatch (its parent is gone), and a
    // config for R: the unknown account.
    let r = s
        .call(
            "PushSubscription/set",
            json!({"update": {&id: {"emailPush/R/urgency": "high"}}}),
        )
        .await;
    assert_eq!(r["notUpdated"][&id]["type"], "invalidPatch", "{r}");
    let r = s
        .call(
            "PushSubscription/set",
            json!({"update": {&id: {"emailPush/R": {}}}}),
        )
        .await;
    assert_eq!(
        r["notUpdated"][&id]["properties"],
        json!(["emailPush", "emailPush/R"]),
        "{r}"
    );

    // An update of types only succeeds, and stores the pruned map.
    let r = s
        .call(
            "PushSubscription/set",
            json!({"update": {&id: {"types": ["Email"]}}}),
        )
        .await;
    assert!(r["updated"].get(&id).is_some(), "{r}");
    s.b.set_email_push_account_for_test(&Id::from("R"), Some(EmailPushAccount::new(true)));
    let r = s
        .call(
            "PushSubscription/get",
            json!({"ids": [&id], "properties": ["emailPush"]}),
        )
        .await;
    assert_eq!(
        r["list"][0]["emailPush"],
        json!({"A1412": normal}),
        "R not restored: {r}"
    );

    // Deliveries to a revoked account push nothing.
    s.b.set_email_push_account_for_test(&Id::from(ACCOUNT), None);
    let (_, pushes) = s.deliver("Hi", INBOX).await;
    assert!(pushes.is_empty());
}

/// Splitting through the size knob: an oversized Email is cut to a prefix.
#[tokio::test]
async fn size_limit() {
    let s = Server::new();
    s.subscribe(json!({"A1412": {"properties": ["id", "subject", "from"]}}))
        .await;
    s.b.set_push_max_bytes_for_test(200);
    let long = "s".repeat(300);
    let (id, pushes) = s.deliver(&long, INBOX).await;
    assert_eq!(pushes.len(), 1);
    assert_eq!(pushes[0]["emails"], json!([{"id": id.as_ref()}]));
    assert!(serde_json::to_vec(&pushes[0]).unwrap().len() <= 200);
    // A limit that cannot hold `{}` sends nothing.
    s.b.set_push_max_bytes_for_test(10);
    let (_, pushes) = s.deliver("short", INBOX).await;
    assert!(pushes.is_empty());
}

/// Thread keywords come from the Thread's current Emails: a reply into a
/// Thread whose first Email is `$muted` matches `noneInThreadHaveKeyword`
/// no longer.
#[tokio::test]
async fn thread_conditions_see_the_thread() {
    let s = Server::new();
    s.subscribe(json!({"A1412": {"filter": {"noneInThreadHaveKeyword": "$muted"}}}))
        .await;
    let first =
        b"From: a@example.com\r\nSubject: Plan\r\nMessage-ID: <first@example.com>\r\n\r\nx\r\n";
    let (first_id, pushes) =
        s.b.deliver_email(&Id::from(ACCOUNT), first, &[Id::from(INBOX)], &[])
            .await
            .unwrap();
    assert_eq!(pushes.len(), 1);
    let r = s
        .call(
            "Email/set",
            json!({"accountId": ACCOUNT, "update": {first_id.as_ref(): {"keywords/$muted": true}}}),
        )
        .await;
    assert!(r["updated"].get(first_id.as_ref()).is_some(), "{r}");
    let reply = b"From: b@example.com\r\nSubject: Re: Plan\r\nMessage-ID: <reply@example.com>\r\n\
                  In-Reply-To: <first@example.com>\r\n\r\ny\r\n";
    let (_, pushes) =
        s.b.deliver_email(&Id::from(ACCOUNT), reply, &[Id::from(INBOX)], &[])
            .await
            .unwrap();
    assert!(pushes.is_empty(), "{pushes:?}");
    let other =
        b"From: c@example.com\r\nSubject: Other\r\nMessage-ID: <other@example.com>\r\n\r\nz\r\n";
    let (_, pushes) =
        s.b.deliver_email(&Id::from(ACCOUNT), other, &[Id::from(INBOX)], &[])
            .await
            .unwrap();
    assert_eq!(pushes.len(), 1);
}
