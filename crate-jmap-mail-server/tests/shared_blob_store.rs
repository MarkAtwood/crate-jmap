//! The memory backend over a shared blob store (bd JMAP-tna1.3.15):
//! `with_blob_store` reads and writes through it, the reference methods
//! count current objects only, and `blob_destroyed` drops the history that
//! used the blob.
//!
//! Oracles: RFC 8621 §4.8 (Email/import reads an uploaded blob), blobext-03
//! §6 (a Mailbox or Thread references the blobs of its Emails),
//! object-history-03 §3.1 (non-current versions are not references; a
//! version whose blob was discarded "MUST NOT" be returned).

#![cfg(feature = "memory")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use jmap_mail_server::memory::{ExternalBlobStore, MemoryBackend};
use jmap_mail_server::register_mail_handlers_with_history;
use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const A: &str = "abc";

#[derive(Default)]
struct Store(Mutex<HashMap<(Id, Id), Vec<u8>>>);

impl ExternalBlobStore for Store {
    fn read(&self, account_id: &Id, blob_id: &Id) -> Option<Vec<u8>> {
        self.0
            .lock()
            .unwrap()
            .get(&(account_id.clone(), blob_id.clone()))
            .cloned()
    }
    fn store(&self, account_id: &Id, blob_id: &Id, bytes: Vec<u8>, _media_type: &str) {
        self.0
            .lock()
            .unwrap()
            .insert((account_id.clone(), blob_id.clone()), bytes);
    }
}

async fn call(d: &Dispatcher<()>, method: &str, mut args: Value) -> Value {
    args["accountId"] = json!(A);
    let req: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail",
                  "urn:ietf:params:jmap:object-history"],
        "methodCalls": [[method, args, "0"]]
    }))
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let r = resp["methodResponses"][0].clone();
    assert_eq!(r[0], method, "{r}");
    r[1].clone()
}

#[tokio::test]
async fn shared_store_references_and_destroy() {
    let acc = Id::from(A);
    let store = Arc::new(Store::default());
    let b = MemoryBackend::new()
        .with_blob_store(Arc::clone(&store) as Arc<dyn ExternalBlobStore>)
        .with_account(&acc)
        .with_object_history(
            serde_json::from_value(
                json!({"maxHistoryDuration": null, "supportedTypes": ["Email", "Mailbox"]}),
            )
            .unwrap(),
            HistoryRetention::new(),
        );
    let mut d = Dispatcher::new();
    register_mail_handlers_with_history(&mut d, Arc::new(b.clone()));

    let mb = call(
        &d,
        "Mailbox/set",
        json!({"create": {"m": {"name": "Inbox"}}}),
    )
    .await;
    let mb = mb["created"]["m"]["id"].as_str().unwrap().to_owned();
    let blob = Id::from("up-1");
    store.store(
        &acc,
        &blob,
        b"Subject: Hi\r\nMessage-ID: <s@x>\r\n\r\nbody\r\n".to_vec(),
        "message/rfc822",
    );
    let imp = call(
        &d,
        "Email/import",
        json!({"emails": {"i": {"blobId": blob, "mailboxIds": {&mb: true}}}}),
    )
    .await;
    let email = imp["created"]["i"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{imp}"))
        .to_owned();
    let thread = imp["created"]["i"]["threadId"].as_str().unwrap().to_owned();

    let ids = [blob.clone()];
    assert_eq!(
        b.current_blob_references(&acc, "Email", &ids)[&blob],
        [Id::from(email.as_str())]
    );
    assert_eq!(
        b.current_blob_references(&acc, "Mailbox", &ids)[&blob],
        [Id::from(mb.as_str())]
    );
    assert_eq!(
        b.current_blob_references(&acc, "Thread", &ids)[&blob],
        [Id::from(thread.as_str())]
    );
    assert!(b.blob_has_current_reference(&acc, &blob));

    // A created Email's message lands in the shared store.
    let made = call(
        &d,
        "Email/set",
        json!({"create": {"e": {
            "mailboxIds": {&mb: true}, "subject": "Mine", "bodyValues": {"1": {"value": "x"}},
            "textBody": [{"partId": "1", "type": "text/plain"}]
        }}}),
    )
    .await;
    let made_blob = Id::from(made["created"]["e"]["blobId"].as_str().unwrap());
    assert!(store.read(&acc, &made_blob).is_some());

    // Destroyed: no longer a reference, but its history keeps it.
    call(&d, "Email/set", json!({"destroy": [&email]})).await;
    assert!(b.current_blob_references(&acc, "Email", &ids)[&blob].is_empty());
    assert!(!b.blob_has_current_reference(&acc, &blob));
    let kept = call(
        &d,
        "Email/get",
        json!({"ids": [&email], "includeDestroyed": true}),
    )
    .await;
    assert_eq!(kept["list"].as_array().unwrap().len(), 1, "{kept}");

    // The blob is destroyed: the versions using it go.
    b.blob_destroyed(&acc, &blob);
    let gone = call(
        &d,
        "Email/get",
        json!({"ids": [&email], "includeDestroyed": true}),
    )
    .await;
    assert_eq!(gone["notFound"], json!([&email]), "{gone}");
}
