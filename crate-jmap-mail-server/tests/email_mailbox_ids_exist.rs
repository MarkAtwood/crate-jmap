//! Every Mailbox id an Email write adds to `mailboxIds` names a Mailbox
//! of the account (bd:JMAP-91dd).
//!
//! Oracles: RFC 8621 §4.1.1 (`mailboxIds` is "The set of Mailbox ids
//! this Email belongs to"), §4.6 (an update may replace the whole set or
//! "add/remove individual ones using the JMAP patch syntax"), §4.8 ("If
//! the "blobId", "mailboxIds", or "keywords" properties are invalid
//! (e.g., missing, wrong type, id not found), the server MUST reject the
//! import with an "invalidProperties" SetError") and RFC 8620 §5.3
//! (`invalidProperties`, with `properties` naming the invalid ones; a
//! `#creationId` stands for the id the server assigned earlier in the
//! request). Stalwart 1.1.1 answers the same.

mod common;

use std::sync::Arc;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::register_mail_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc";
/// A Mailbox id no account in these tests has.
const NO_SUCH: &str = "zz9-no-such";

struct Server {
    b: MemoryBackend<()>,
    d: Dispatcher<()>,
}

impl Server {
    /// Account `acc` with the Mailboxes `inbox` and `archive`.
    fn new() -> Self {
        let b = MemoryBackend::<()>::default();
        b.register_account(&Id::from(ACCOUNT));
        common::seed_mailboxes(&b, &Id::from(ACCOUNT), &["inbox", "archive"]);
        let mut d = Dispatcher::new();
        register_mail_handlers(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    /// Run `calls` as one request; returns the method responses.
    async fn request(&self, calls: Value) -> Vec<Value> {
        let req: JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": calls
        }))
        .unwrap();
        let resp = serde_json::to_value(self.d.dispatch(req, (), State::from("s")).await).unwrap();
        resp["methodResponses"].as_array().unwrap().clone()
    }

    /// Run one method call; returns its response arguments.
    async fn call(&self, method: &str, args: Value) -> Value {
        let inv = self.request(json!([[method, args, "0"]])).await.remove(0);
        assert_eq!(inv[0], method, "{inv}");
        inv[1].clone()
    }

    async fn email_set(&self, args: Value) -> Value {
        let mut args = args;
        args["accountId"] = json!(ACCOUNT);
        self.call("Email/set", args).await
    }

    /// Create an Email in `inbox`; returns its id.
    async fn create_in_inbox(&self) -> String {
        let r = self
            .email_set(json!({"create": {"e": {
                "mailboxIds": {"inbox": true}, "subject": "hello"}}}))
            .await;
        r["created"]["e"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}"))
            .to_owned()
    }

    /// The stored `mailboxIds` of Email `id`.
    async fn mailbox_ids(&self, id: &str) -> Value {
        let r = self
            .call(
                "Email/get",
                json!({"accountId": ACCOUNT, "ids": [id], "properties": ["mailboxIds"]}),
            )
            .await;
        r["list"][0]["mailboxIds"].clone()
    }
}

/// Assert `error` is `invalidProperties` naming exactly `mailboxIds`.
fn assert_invalid_mailbox_ids(error: &Value) {
    assert_eq!(error["type"], "invalidProperties", "{error}");
    assert_eq!(error["properties"], json!(["mailboxIds"]), "{error}");
}

#[tokio::test]
async fn create_naming_an_unknown_mailbox_is_invalid_properties() {
    let s = Server::new();
    for mailbox_ids in [
        json!({NO_SUCH: true}),
        json!({"inbox": true, NO_SUCH: true}),
    ] {
        let r = s
            .email_set(json!({"create": {"x": {"mailboxIds": mailbox_ids, "subject": "s"}}}))
            .await;
        assert!(r["created"].is_null(), "{r}");
        assert_invalid_mailbox_ids(&r["notCreated"]["x"]);
        assert_eq!(r["oldState"], r["newState"], "nothing was stored: {r}");
    }
}

#[tokio::test]
async fn update_replacing_mailbox_ids_with_an_unknown_mailbox_is_invalid_properties() {
    let s = Server::new();
    let id = s.create_in_inbox().await;
    let r = s
        .email_set(json!({"update": {&id: {"mailboxIds": {"inbox": true, NO_SUCH: true}}}}))
        .await;
    assert_invalid_mailbox_ids(&r["notUpdated"][&id]);
    assert_eq!(s.mailbox_ids(&id).await, json!({"inbox": true}));
}

#[tokio::test]
async fn update_patch_adding_an_unknown_mailbox_is_invalid_properties() {
    let s = Server::new();
    let id = s.create_in_inbox().await;
    let r = s
        .email_set(json!({"update": {&id: {format!("mailboxIds/{NO_SUCH}"): true}}}))
        .await;
    assert_invalid_mailbox_ids(&r["notUpdated"][&id]);
    assert_eq!(s.mailbox_ids(&id).await, json!({"inbox": true}));
}

/// Adding an existing Mailbox, replacing the set with existing ones and
/// removing an id still pass.
#[tokio::test]
async fn updates_naming_existing_mailboxes_succeed() {
    let s = Server::new();
    let id = s.create_in_inbox().await;
    let r = s
        .email_set(json!({"update": {&id: {"mailboxIds/archive": true}}}))
        .await;
    assert!(r["notUpdated"].is_null(), "{r}");
    assert_eq!(
        s.mailbox_ids(&id).await,
        json!({"inbox": true, "archive": true})
    );
    let r = s
        .email_set(json!({"update": {&id: {"mailboxIds/inbox": null}}}))
        .await;
    assert!(r["notUpdated"].is_null(), "{r}");
    let r = s
        .email_set(json!({"update": {&id: {"mailboxIds": {"inbox": true}}}}))
        .await;
    assert!(r["notUpdated"].is_null(), "{r}");
    assert_eq!(s.mailbox_ids(&id).await, json!({"inbox": true}));
}

/// The Mailbox must exist in the account the copy goes to; one of the
/// same id in the source account does not count.
#[tokio::test]
async fn copy_naming_a_mailbox_the_target_lacks_is_invalid_properties() {
    let s = Server::new();
    let src = s.create_in_inbox().await;
    s.b.register_account(&Id::from("dst"));
    common::seed_mailboxes(&s.b, &Id::from("dst"), &["archive"]);
    let r = s
        .call(
            "Email/copy",
            json!({"fromAccountId": ACCOUNT, "accountId": "dst", "create": {
                "bad": {"id": &src, "mailboxIds": {"inbox": true}},
                "good": {"id": &src, "mailboxIds": {"archive": true}}}}),
        )
        .await;
    assert_invalid_mailbox_ids(&r["notCreated"]["bad"]);
    assert!(r["created"]["good"]["id"].is_string(), "{r}");
}

#[tokio::test]
async fn import_naming_an_unknown_mailbox_is_invalid_properties() {
    let s = Server::new();
    let blob = Id::from("blob-import");
    s.b.store_blob(&blob, b"Subject: import\r\n\r\nbody\r\n".to_vec());
    let r = s
        .call(
            "Email/import",
            json!({"accountId": ACCOUNT, "emails": {
                "bad": {"blobId": blob.as_ref(), "mailboxIds": {NO_SUCH: true}}}}),
        )
        .await;
    assert!(r["created"].is_null(), "{r}");
    assert_invalid_mailbox_ids(&r["notCreated"]["bad"]);
}

/// `#m` names the Mailbox an earlier call of the same request created,
/// as a `mailboxIds` key in a create and as a `mailboxIds/#m` patch path;
/// each reference resolves before the existence check.
#[tokio::test]
async fn creation_reference_to_a_mailbox_created_earlier_in_the_request_works() {
    let s = Server::new();
    let old = s.create_in_inbox().await;
    let responses = s
        .request(json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "New"}}}, "0"],
            ["Email/set", {"accountId": ACCOUNT, "create": {
                "e": {"mailboxIds": {"#m": true}, "subject": "filed"}}}, "1"],
            ["Email/set", {"accountId": ACCOUNT, "update": {&old: {"mailboxIds/#m": true}}}, "2"],
        ]))
        .await;
    let mailbox = responses[0][1]["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{responses:?}"));
    let email = responses[1][1]["created"]["e"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{responses:?}"));
    assert!(responses[2][1]["notUpdated"].is_null(), "{responses:?}");
    assert_eq!(s.mailbox_ids(email).await, json!({mailbox: true}));
    assert_eq!(
        s.mailbox_ids(&old).await,
        json!({"inbox": true, mailbox: true})
    );
}
