//! `#creationId` references in mail blobId-typed properties and arguments,
//! through the full dispatcher (bd:JMAP-tna1.8; docs/design/blobext.md §9).
//!
//! Oracles: RFC 8620 §5.3 (rfc8620.txt:1983-1996): the server "MUST keep a
//! simple map for the duration of the request of creation id to record id
//! for each newly created record, so it can substitute in the correct value
//! if necessary in later method calls"; draft-ietf-jmap-blobext-03 §8: a
//! blob created by Blob/upload, Blob/set or Blob/convert can be named by
//! `#creationId` in later method calls of the request.
//!
//! The first call of each request is a stand-in `Blob/upload`: it stores the
//! bytes in the mail backend's blob store and answers with a `created` map,
//! from which the dispatcher fills the request's creation id map, as it does
//! for the real method. Each test then names the blob as `#b1` and checks
//! that the method used the uploaded bytes, and that an unknown reference
//! fails with the error the method already gave a `#` value.

#![allow(async_fn_in_trait)]

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_mail_server::{MailBackend, register_mail_handlers};
use jmap_server::{CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput};
use jmap_types::{Id, JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "account1";
const UPLOADED: &str = "Gupload1";

/// A `Blob/upload` stand-in: stores `create.<cid>.data` (text) under the
/// fixed id [`UPLOADED`] and reports it in `created`.
fn register_upload(dispatcher: &mut Dispatcher<()>, backend: &MemoryBackend) {
    let handler = ClosureHandler::new(
        Arc::new(backend.clone()),
        |b: Arc<MemoryBackend>, cx: CallContext<()>| {
            Box::pin(async move {
                let mut created = serde_json::Map::new();
                for (cid, obj) in cx.args["create"].as_object().expect("create") {
                    let data = obj["data"].as_str().expect("data");
                    b.store_blob(&Id::from(UPLOADED), data.as_bytes().to_vec());
                    created.insert(cid.clone(), json!({"id": UPLOADED, "size": data.len()}));
                }
                Ok(HandlerOutput::new(
                    json!({"accountId": ACCOUNT, "created": created}),
                ))
            }) as HandlerFuture
        },
    );
    dispatcher.register("Blob/upload", Arc::new(handler));
}

/// Dispatch `Blob/upload` creating `b1` from `data`, then `calls`.
async fn dispatch(backend: MemoryBackend, data: &str, calls: Value) -> JmapResponse {
    let mut dispatcher = Dispatcher::new();
    register_upload(&mut dispatcher, &backend);
    #[cfg(feature = "sieve")]
    jmap_mail_server::register_sieve_handlers(&mut dispatcher, Arc::new(backend.clone()));
    register_mail_handlers(&mut dispatcher, Arc::new(backend));
    let mut method_calls = vec![json!(["Blob/upload", {
        "accountId": ACCOUNT,
        "create": {"b1": {"data": data}}
    }, "up"])];
    method_calls.extend(calls.as_array().expect("calls").iter().cloned());
    let request: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:mail",
            "urn:ietf:params:jmap:blob2",
            "urn:ietf:params:jmap:sieve"
        ],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    let resp = dispatcher.dispatch(request, (), State::from("s0")).await;
    assert_eq!(
        resp.method_responses[0].arguments["created"]["b1"]["id"], UPLOADED,
        "{:?}",
        resp.method_responses[0]
    );
    resp
}

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT));
    backend
}

const MESSAGE: &str = "Subject: Uploaded\r\nFrom: alice@example.com\r\n\r\nBody.";

/// The response arguments of method call `index` (0 is the upload).
fn args(resp: &JmapResponse, index: usize) -> &Value {
    &resp.method_responses[index].arguments
}

#[tokio::test]
async fn email_import_blob_id_resolves_upload_and_unknown_ref_fails() {
    let backend = backend();
    let resp = dispatch(
        backend.clone(),
        MESSAGE,
        json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "In"}}}, "0"],
            ["Email/import", {"accountId": ACCOUNT, "emails": {
                "ok": {"blobId": "#b1", "mailboxIds": {"#m": true}},
                "bad": {"blobId": "#nope", "mailboxIds": {"#m": true}}
            }}, "1"],
            ["Email/get", {"accountId": ACCOUNT, "properties": ["subject"]}, "2"]
        ]),
    )
    .await;
    let import = args(&resp, 2);
    assert_eq!(import["created"]["ok"]["blobId"], UPLOADED, "{import:?}");
    let err = &import["notCreated"]["bad"];
    assert_eq!(err["type"], "invalidProperties", "{err:?}");
    assert_eq!(err["properties"], json!(["blobId"]), "{err:?}");
    let list = &args(&resp, 3)["list"];
    assert_eq!(
        list,
        &json!([{"id": import["created"]["ok"]["id"], "subject": "Uploaded"}])
    );
}

/// The bytes of the Email `created` names, as stored.
async fn stored(backend: &MemoryBackend, created: &Value) -> String {
    let blob = Id::from(created["blobId"].as_str().expect("blobId"));
    let bytes = backend
        .read_blob(&(), &Id::from(ACCOUNT), &blob)
        .await
        .expect("read_blob")
        .expect("stored message");
    String::from_utf8(bytes).expect("UTF-8")
}

/// `attachments` and a part nested in `bodyStructure.subParts` both take the
/// uploaded blob; base64 of `%PDF-1` is `JVBERi0x`.
#[tokio::test]
async fn email_set_body_part_blob_ids_resolve_upload() {
    let backend = backend();
    let pdf = |blob_id: &str| {
        json!({"blobId": blob_id, "type": "application/pdf", "name": "a.pdf",
               "disposition": "attachment"})
    };
    let resp = dispatch(
        backend.clone(),
        "%PDF-1",
        json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "Drafts"}}}, "0"],
            ["Email/set", {"accountId": ACCOUNT, "create": {
                "flat": {
                    "mailboxIds": {"#m": true},
                    "textBody": [{"partId": "t", "type": "text/plain"}],
                    "attachments": [pdf("#b1")],
                    "bodyValues": {"t": {"value": "See attached."}}
                },
                "nested": {
                    "mailboxIds": {"#m": true},
                    "bodyStructure": {"type": "multipart/mixed", "subParts": [
                        {"partId": "t", "type": "text/plain"},
                        pdf("#b1")
                    ]},
                    "bodyValues": {"t": {"value": "See attached."}}
                }
            }}, "1"]
        ]),
    )
    .await;
    let set = args(&resp, 2);
    for cid in ["flat", "nested"] {
        let created = &set["created"][cid];
        assert!(created.is_object(), "{cid}: {set:?}");
        let message = stored(&backend, created).await;
        assert!(message.contains("JVBERi0x"), "{cid}: {message}");
    }
}

/// An unknown reference fails as any `#` blobId did before:
/// `invalidProperties` on the property that holds the part.
#[tokio::test]
async fn email_set_unknown_blob_ref_is_invalid_properties() {
    let resp = dispatch(
        backend(),
        "%PDF-1",
        json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "Drafts"}}}, "0"],
            ["Email/set", {"accountId": ACCOUNT, "create": {"bad": {
                "mailboxIds": {"#m": true},
                "attachments": [{"blobId": "#nope", "type": "application/pdf"}]
            }}}, "1"]
        ]),
    )
    .await;
    let err = &args(&resp, 2)["notCreated"]["bad"];
    assert_eq!(err["type"], "invalidProperties", "{err:?}");
    assert_eq!(err["properties"], json!(["attachments"]), "{err:?}");
}

/// `parsed` is keyed by the `blobIds` entry as sent.
#[tokio::test]
async fn email_parse_blob_ids_resolve_upload() {
    let resp = dispatch(
        backend(),
        MESSAGE,
        json!([["Email/parse", {
            "accountId": ACCOUNT, "blobIds": ["#b1"], "properties": ["subject"]
        }, "0"]]),
    )
    .await;
    let parse = args(&resp, 1);
    assert_eq!(parse["parsed"]["#b1"]["subject"], "Uploaded", "{parse:?}");
    assert_eq!(parse["notFound"], Value::Null, "{parse:?}");
}

#[tokio::test]
async fn email_parse_unknown_blob_ref_is_invalid_arguments() {
    let resp = dispatch(
        backend(),
        MESSAGE,
        json!([["Email/parse", {"accountId": ACCOUNT, "blobIds": ["#nope"]}, "0"]]),
    )
    .await;
    assert_eq!(resp.method_responses[1].name, "error");
    assert_eq!(args(&resp, 1)["type"], "invalidArguments");
}

#[cfg(feature = "sieve")]
mod sieve {
    use super::*;

    /// A script stored from a literal blob; returns its id.
    async fn seed_script(backend: &MemoryBackend) -> String {
        backend.store_blob(&Id::from("script"), b"keep;".to_vec());
        let resp = dispatch(
            backend.clone(),
            "keep;",
            json!([["SieveScript/set", {"accountId": ACCOUNT, "create": {
                "s": {"name": "seeded", "blobId": "script"}
            }}, "0"]]),
        )
        .await;
        let id = &args(&resp, 1)["created"]["s"]["id"];
        id.as_str().expect("seeded script id").to_owned()
    }

    /// SieveScript/validate, and SieveScript/set create and update, take
    /// the uploaded script.
    #[tokio::test]
    async fn sieve_blob_ids_resolve_upload() {
        let backend = backend();
        let seeded = seed_script(&backend).await;
        let resp = dispatch(
            backend.clone(),
            "discard;",
            json!([
                ["SieveScript/validate", {"accountId": ACCOUNT, "blobId": "#b1"}, "0"],
                ["SieveScript/set", {"accountId": ACCOUNT,
                    "create": {"s": {"name": "uploaded", "blobId": "#b1"}},
                    "update": {seeded.as_str(): {"blobId": "#b1"}}
                }, "1"],
                ["SieveScript/get", {"accountId": ACCOUNT, "ids": [seeded.as_str()],
                    "properties": ["blobId"]}, "2"]
            ]),
        )
        .await;
        let validate = args(&resp, 1);
        assert_eq!(validate["error"], Value::Null, "{validate:?}");
        let set = args(&resp, 2);
        assert_eq!(set["created"]["s"]["blobId"], UPLOADED, "{set:?}");
        assert!(set["updated"].get(&seeded).is_some(), "{set:?}");
        assert_eq!(args(&resp, 3)["list"][0]["blobId"], UPLOADED);
    }

    #[tokio::test]
    async fn sieve_unknown_blob_ref_fails_as_before() {
        let backend = backend();
        let seeded = seed_script(&backend).await;
        let resp = dispatch(
            backend,
            "keep;",
            json!([
                ["SieveScript/validate", {"accountId": ACCOUNT, "blobId": "#nope"}, "0"],
                ["SieveScript/set", {"accountId": ACCOUNT,
                    "create": {"bad": {"name": "bad", "blobId": "#nope"}},
                    "update": {seeded.as_str(): {"blobId": "#nope"}}
                }, "1"]
            ]),
        )
        .await;
        assert_eq!(resp.method_responses[1].name, "error");
        assert_eq!(args(&resp, 1)["type"], "invalidArguments");
        let err = &args(&resp, 2)["notCreated"]["bad"];
        assert_eq!(err["type"], "invalidProperties", "{err:?}");
        assert_eq!(err["properties"], json!(["blobId"]), "{err:?}");
        let err = &args(&resp, 2)["notUpdated"][&seeded];
        assert_eq!(err["type"], "invalidProperties", "{err:?}");
        assert_eq!(err["properties"], json!(["blobId"]), "{err:?}");
    }
}
