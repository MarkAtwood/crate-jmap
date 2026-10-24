//! `#creationId` references in mail `/set` Id-typed properties, through
//! the full dispatcher (bd:JMAP-j7uo).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1983-1996): "Some records may hold
//! references to other records (foreign keys). That reference may be set
//! (via create or update) in the same request as the referenced record is
//! created. To do this, the client refers to the new record using its
//! creation id prefixed with a "#". [...] the server MUST keep a simple map
//! for the duration of the request of creation id to record id for each
//! newly created record, so it can substitute in the correct value if
//! necessary in later method calls. In the case of records with
//! references to the same type, the server MUST order the creates and
//! updates within a single method call so that creates happen before their
//! creation ids are referenced by another create/update/destroy in the same
//! call." RFC 8620 §3.3 (rfc8620.txt:925-926): the server "looks it up in
//! the map and replaces the reference with the real id, if found."
//!
//! Each assertion compares the referencing property with the id the server
//! reported for the referenced record in the same response, the relation
//! the RFC defines, or with a hand-written SetError shape.

#![allow(async_fn_in_trait)]

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_mail_server::{MailBackend, register_mail_handlers};
use jmap_mail_types::{Identity, Mailbox, MailboxRights};
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "account1";

async fn dispatch(backend: MemoryBackend, method_calls: Value) -> JmapResponse {
    let mut dispatcher = Dispatcher::new();
    register_mail_handlers(&mut dispatcher, Arc::new(backend));
    let request: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:mail",
            "urn:ietf:params:jmap:submission"
        ],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT));
    backend
}

#[tokio::test]
async fn mailbox_parent_id_resolves_earlier_call_creation() {
    let resp = dispatch(
        backend(),
        json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"p": {"name": "Parent"}}}, "0"],
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {
                "c": {"name": "Child", "parentId": "#p"}
            }}, "1"]
        ]),
    )
    .await;
    let parent_id = &resp.method_responses[0].arguments["created"]["p"]["id"];
    assert!(parent_id.is_string(), "{:?}", resp.method_responses[0]);
    let child = &resp.method_responses[1].arguments["created"]["c"];
    assert_eq!(
        &child["parentId"], parent_id,
        "{:?}",
        resp.method_responses[1]
    );
}

#[tokio::test]
async fn mailbox_parent_id_resolves_same_call_creation_listed_after_child() {
    // The child sorts before its parent in the create map; §5.3 still
    // requires the parent to be created first.
    let resp = dispatch(
        backend(),
        json!([["Mailbox/set", {"accountId": ACCOUNT, "create": {
            "a-child": {"name": "Child", "parentId": "#z-parent"},
            "z-parent": {"name": "Parent"}
        }}, "0"]]),
    )
    .await;
    let args = &resp.method_responses[0].arguments;
    let parent_id = &args["created"]["z-parent"]["id"];
    assert!(parent_id.is_string(), "{args:?}");
    assert_eq!(
        &args["created"]["a-child"]["parentId"], parent_id,
        "{args:?}"
    );
}

#[tokio::test]
async fn mailbox_update_parent_id_resolves_creation() {
    let backend = backend();
    let (moved_id, _) = backend
        .create_object::<Mailbox, _>(&(), &Id::from(ACCOUNT), "seed", |id: &Id| {
            Mailbox::new(
                id.clone(),
                "Moved",
                0,
                0,
                0,
                0,
                0,
                MailboxRights::default(),
                true,
            )
        })
        .await
        .expect("seed Mailbox");
    let resp = dispatch(
        backend,
        json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"p": {"name": "Parent"}}}, "0"],
            ["Mailbox/set", {"accountId": ACCOUNT, "update": {
                moved_id.as_ref(): {"parentId": "#p"}
            }}, "1"],
            ["Mailbox/get", {"accountId": ACCOUNT, "ids": [moved_id.as_ref()],
                "properties": ["parentId"]}, "2"]
        ]),
    )
    .await;
    let parent_id = &resp.method_responses[0].arguments["created"]["p"]["id"];
    assert!(parent_id.is_string(), "{:?}", resp.method_responses[0]);
    assert!(
        resp.method_responses[1].arguments["updated"]
            .get(moved_id.as_ref())
            .is_some(),
        "{:?}",
        resp.method_responses[1]
    );
    assert_eq!(
        &resp.method_responses[2].arguments["list"][0]["parentId"], parent_id,
        "{:?}",
        resp.method_responses[2]
    );
}

#[tokio::test]
async fn mailbox_unknown_parent_ref_is_invalid_properties() {
    let resp = dispatch(
        backend(),
        json!([["Mailbox/set", {"accountId": ACCOUNT, "create": {
            "c": {"name": "Child", "parentId": "#nope"}
        }}, "0"]]),
    )
    .await;
    let err = &resp.method_responses[0].arguments["notCreated"]["c"];
    assert_eq!(err["type"], "invalidProperties", "{err:?}");
    assert_eq!(err["properties"], json!(["parentId"]), "{err:?}");
}

/// Seed an Email in a seeded Mailbox; returns the Email id.
async fn seed_email(backend: &MemoryBackend) -> Id {
    let account = Id::from(ACCOUNT);
    let (seed_mailbox, _) = backend
        .create_object::<Mailbox, _>(&(), &account, "seed", |id: &Id| {
            Mailbox::new(
                id.clone(),
                "Seed",
                0,
                0,
                0,
                0,
                0,
                MailboxRights::default(),
                true,
            )
        })
        .await
        .expect("seed Mailbox");
    let blob_id = Id::from("blob-creation-refs");
    backend.store_blob(
        &blob_id,
        b"Subject: Draft\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nBody.".to_vec(),
    );
    let (email_id, _) = backend
        .import_email(&(), &account, &blob_id, &[seed_mailbox], &[], None)
        .await
        .expect("import_email");
    email_id
}

#[tokio::test]
async fn email_create_mailbox_ids_key_resolves_creation() {
    let resp = dispatch(
        backend(),
        json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "One"}}}, "0"],
            ["Email/set", {"accountId": ACCOUNT, "create": {
                "e": {"mailboxIds": {"#m": true}, "subject": "Hi"}
            }}, "1"],
            ["Email/get", {"accountId": ACCOUNT, "properties": ["mailboxIds"]}, "2"]
        ]),
    )
    .await;
    let m = resp.method_responses[0].arguments["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{:?}", resp.method_responses[0]))
        .to_owned();
    assert!(
        resp.method_responses[1].arguments["created"]["e"].is_object(),
        "{:?}",
        resp.method_responses[1]
    );
    assert_eq!(
        resp.method_responses[2].arguments["list"][0]["mailboxIds"],
        json!({ m.as_str(): true }),
        "{:?}",
        resp.method_responses[2]
    );
}

#[tokio::test]
async fn email_update_mailbox_ids_patch_path_and_value_resolve_creation() {
    for patch in [
        json!({"mailboxIds/#m": true}),
        json!({"mailboxIds": {"#m": true}}),
    ] {
        let backend = backend();
        let email_id = seed_email(&backend).await;
        let resp = dispatch(
            backend,
            json!([
                ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "One"}}}, "0"],
                ["Email/set", {"accountId": ACCOUNT, "update": {email_id.as_ref(): patch}}, "1"],
                ["Email/get", {"accountId": ACCOUNT, "ids": [email_id.as_ref()],
                    "properties": ["mailboxIds"]}, "2"]
            ]),
        )
        .await;
        let m = resp.method_responses[0].arguments["created"]["m"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{:?}", resp.method_responses[0]))
            .to_owned();
        assert!(
            resp.method_responses[1].arguments["updated"]
                .get(email_id.as_ref())
                .is_some(),
            "{patch}: {:?}",
            resp.method_responses[1]
        );
        let mailbox_ids = &resp.method_responses[2].arguments["list"][0]["mailboxIds"];
        assert_eq!(mailbox_ids[m.as_str()], true, "{patch}: {mailbox_ids:?}");
        assert!(mailbox_ids.get("#m").is_none(), "{patch}: {mailbox_ids:?}");
    }
}

#[tokio::test]
async fn email_unknown_mailbox_ref_is_invalid_properties() {
    let resp = dispatch(
        backend(),
        json!([["Email/set", {"accountId": ACCOUNT, "create": {
            "e": {"mailboxIds": {"#nope": true}, "subject": "Hi"}
        }}, "0"]]),
    )
    .await;
    let err = &resp.method_responses[0].arguments["notCreated"]["e"];
    assert_eq!(err["type"], "invalidProperties", "{err:?}");
    assert_eq!(err["properties"], json!(["mailboxIds"]), "{err:?}");
}

/// RFC 8621 §7 (rfc8621.txt:4138, 4151) types EmailSubmission
/// `identityId` and `emailId` as "Id", so a draft created by Email/set
/// earlier in the request is submitted by its creation id.
#[tokio::test]
async fn submission_email_id_resolves_creation_and_unknown_ref_fails() {
    let backend = backend();
    let (identity_id, _) = backend
        .create_object::<Identity, _>(&(), &Id::from(ACCOUNT), "i0", |id: &Id| {
            Identity::new(id.clone(), "alice@example.com", true)
        })
        .await
        .expect("create Identity");
    let resp = dispatch(
        backend,
        json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"d": {"name": "Drafts"}}}, "0"],
            ["Email/set", {"accountId": ACCOUNT, "create": {"e": {
                "mailboxIds": {"#d": true},
                "from": [{"email": "alice@example.com"}],
                "to": [{"email": "bob@example.com"}],
                "subject": "Hi"
            }}}, "1"],
            ["EmailSubmission/set", {"accountId": ACCOUNT, "create": {
                "ok": {
                    "identityId": identity_id.as_ref(),
                    "emailId": "#e",
                    "envelope": {
                        "mailFrom": {"email": "alice@example.com"},
                        "rcptTo": [{"email": "bob@example.com"}]
                    }
                },
                "bad-email": {"identityId": identity_id.as_ref(), "emailId": "#nope"},
                "bad-identity": {"identityId": "#nope", "emailId": "#e"}
            }}, "2"]
        ]),
    )
    .await;
    let email_id = &resp.method_responses[1].arguments["created"]["e"]["id"];
    assert!(email_id.is_string(), "{:?}", resp.method_responses[1]);
    let args = &resp.method_responses[2].arguments;
    assert_eq!(&args["created"]["ok"]["emailId"], email_id, "{args:?}");
    for (create_id, property) in [("bad-email", "emailId"), ("bad-identity", "identityId")] {
        let err = &args["notCreated"][create_id];
        assert_eq!(err["type"], "invalidProperties", "{create_id}: {err:?}");
        assert_eq!(err["properties"], json!([property]), "{create_id}: {err:?}");
    }
}

/// The id the server reported for creation id `m` in a Mailbox/set
/// response.
fn created_mailbox_id(resp: &JmapResponse, index: usize) -> String {
    resp.method_responses[index].arguments["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{:?}", resp.method_responses[index]))
        .to_owned()
}

/// RFC 8621 §4.8 (rfc8621.txt:3005) types EmailImport `mailboxIds`
/// as "Id[Boolean]", so its keys take creation id references.
#[tokio::test]
async fn email_import_mailbox_ids_key_resolves_creation_and_unknown_ref_fails() {
    let backend = backend();
    let blob_id = Id::from("blob-import-refs");
    backend.store_blob(
        &blob_id,
        b"Subject: Hi\r\nFrom: alice@example.com\r\n\r\nBody.".to_vec(),
    );
    let resp = dispatch(
        backend,
        json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "One"}}}, "0"],
            ["Email/import", {"accountId": ACCOUNT, "emails": {
                "ok": {"blobId": blob_id.as_ref(), "mailboxIds": {"#m": true}},
                "bad": {"blobId": blob_id.as_ref(), "mailboxIds": {"#nope": true}}
            }}, "1"],
            ["Email/get", {"accountId": ACCOUNT, "properties": ["mailboxIds"]}, "2"]
        ]),
    )
    .await;
    let m = created_mailbox_id(&resp, 0);
    let args = &resp.method_responses[1].arguments;
    assert!(args["created"]["ok"].is_object(), "{args:?}");
    let err = &args["notCreated"]["bad"];
    assert_eq!(err["type"], "invalidProperties", "{err:?}");
    assert_eq!(err["properties"], json!(["mailboxIds"]), "{err:?}");
    assert_eq!(
        resp.method_responses[2].arguments["list"],
        json!([{"id": args["created"]["ok"]["id"], "mailboxIds": {m.as_str(): true}}]),
        "{:?}",
        resp.method_responses[2]
    );
}

/// RFC 8621 §4.7 makes Email/copy a standard RFC 8620 §5.4 /copy whose
/// create objects may override `mailboxIds`, an "Id[Boolean]" (§4.1.1,
/// rfc8621.txt:1331).
#[tokio::test]
async fn email_copy_mailbox_ids_key_resolves_creation_and_unknown_ref_fails() {
    const DEST: &str = "account2";
    let backend = backend();
    backend.register_account(&Id::from(DEST));
    let source_id = seed_email(&backend).await;
    let resp = dispatch(
        backend,
        json!([
            ["Mailbox/set", {"accountId": DEST, "create": {"m": {"name": "One"}}}, "0"],
            ["Email/copy", {"fromAccountId": ACCOUNT, "accountId": DEST, "create": {
                "ok": {"id": source_id.as_ref(), "mailboxIds": {"#m": true}},
                "bad": {"id": source_id.as_ref(), "mailboxIds": {"#nope": true}}
            }}, "1"],
            ["Email/get", {"accountId": DEST, "properties": ["mailboxIds"]}, "2"]
        ]),
    )
    .await;
    let m = created_mailbox_id(&resp, 0);
    let args = &resp.method_responses[1].arguments;
    assert!(args["created"]["ok"].is_object(), "{args:?}");
    let err = &args["notCreated"]["bad"];
    assert_eq!(err["type"], "invalidProperties", "{err:?}");
    assert_eq!(err["properties"], json!(["mailboxIds"]), "{err:?}");
    assert_eq!(
        resp.method_responses[2].arguments["list"],
        json!([{"id": args["created"]["ok"]["id"], "mailboxIds": {m.as_str(): true}}]),
        "{:?}",
        resp.method_responses[2]
    );
}

/// RFC 8621 §7.5 (rfc8621.txt:4525-4531): onSuccessUpdateEmail values are
/// "PatchObject"s applied by an implicit Email/set, so a `mailboxIds/#m`
/// path names the Mailbox created earlier in the request.
#[tokio::test]
async fn submission_on_success_update_email_resolves_mailbox_creation() {
    let backend = backend();
    let (identity_id, _) = backend
        .create_object::<Identity, _>(&(), &Id::from(ACCOUNT), "i0", |id: &Id| {
            Identity::new(id.clone(), "alice@example.com", true)
        })
        .await
        .expect("create Identity");
    let email_id = seed_email(&backend).await;
    let resp = dispatch(
        backend,
        json!([
            ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "Sent"}}}, "0"],
            ["EmailSubmission/set", {"accountId": ACCOUNT,
                "create": {"s": {
                    "identityId": identity_id.as_ref(),
                    "emailId": email_id.as_ref(),
                    "envelope": {
                        "mailFrom": {"email": "alice@example.com"},
                        "rcptTo": [{"email": "bob@example.com"}]
                    }
                }},
                "onSuccessUpdateEmail": {"#s": {"mailboxIds/#m": true}}
            }, "1"],
            ["Email/get", {"accountId": ACCOUNT, "ids": [email_id.as_ref()],
                "properties": ["mailboxIds"]}, "2"]
        ]),
    )
    .await;
    let m = created_mailbox_id(&resp, 0);
    let implicit = &resp.method_responses[2];
    assert_eq!(implicit.name, "Email/set", "{:?}", resp.method_responses);
    assert!(
        implicit.arguments["updated"]
            .get(email_id.as_ref())
            .is_some(),
        "{implicit:?}"
    );
    let mailbox_ids = &resp.method_responses[3].arguments["list"][0]["mailboxIds"];
    assert_eq!(mailbox_ids[m.as_str()], true, "{mailbox_ids:?}");
    assert!(mailbox_ids.get("#m").is_none(), "{mailbox_ids:?}");
}

/// An unknown reference in an onSuccessUpdateEmail patch fails that
/// Email update with the same SetError Email/set gives (RFC 8620 §5.3).
#[tokio::test]
async fn submission_on_success_update_email_unknown_mailbox_ref_fails() {
    let backend = backend();
    let (identity_id, _) = backend
        .create_object::<Identity, _>(&(), &Id::from(ACCOUNT), "i0", |id: &Id| {
            Identity::new(id.clone(), "alice@example.com", true)
        })
        .await
        .expect("create Identity");
    let email_id = seed_email(&backend).await;
    let resp = dispatch(
        backend,
        json!([["EmailSubmission/set", {"accountId": ACCOUNT,
            "create": {"s": {
                "identityId": identity_id.as_ref(),
                "emailId": email_id.as_ref(),
                "envelope": {
                    "mailFrom": {"email": "alice@example.com"},
                    "rcptTo": [{"email": "bob@example.com"}]
                }
            }},
            "onSuccessUpdateEmail": {"#s": {"mailboxIds/#nope": true}}
        }, "0"]]),
    )
    .await;
    let implicit = &resp.method_responses[1];
    assert_eq!(implicit.name, "Email/set", "{:?}", resp.method_responses);
    let err = &implicit.arguments["notUpdated"][email_id.as_ref()];
    assert_eq!(err["type"], "invalidProperties", "{err:?}");
    assert_eq!(err["properties"], json!(["mailboxIds"]), "{err:?}");
}
