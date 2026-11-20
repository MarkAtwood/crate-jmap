//! EmailSubmission/set: the derived envelope and the onSuccess* side
//! effects (RFC 8621 §7, §7.5).
//!
//! Oracles:
//! - RFC 8621 §7 envelope "mailFrom": "The email address in the Sender
//!   header field, if present; otherwise, it's the email address in the
//!   From header field ... If the address found from this is not allowed by
//!   the Identity associated with this submission, the "email" property
//!   from the Identity MUST be used instead."
//! - RFC 8621 §7 envelope "rcptTo": "The deduplicated set of email
//!   addresses from the To, Cc, and Bcc header fields".
//! - RFC 8621 §7.5 onSuccessDestroyEmail: the Email is destroyed "if the
//!   create/update/destroy succeeds".

mod common;

use std::collections::BTreeMap;

use common::MemoryBackend;
use jmap_mail_server::{MailBackend, handle_email_get, handle_submission_set};
use jmap_mail_types::Identity;
use jmap_types::Id;
use serde_json::{Value, json};

const MESSAGE: &[u8] = b"From: Jane Sender <Jane_Sender@example.org>\r\n\
Message-ID: <x1@example.org>\r\n\
To: Joe <joe@example.com>\r\n\
Cc: Joe Again <JOE@example.com>, Ann <ann@example.com>\r\n\
Subject: Hi\r\n\
\r\n\
Body.\r\n";

async fn email(b: &MemoryBackend<()>, account: &Id) -> Id {
    let blob = Id::from("blob-e");
    b.register_account(account);
    b.store_blob(&blob, MESSAGE.to_vec());
    b.import_email(&(), account, &blob, &[Id::from("mb1")], &[], None)
        .await
        .unwrap()
        .0
}

async fn identity(b: &MemoryBackend<()>, account: &Id, addr: &'static str) -> Id {
    b.create_object::<Identity, _>(&(), account, "i", move |id: &Id| {
        Identity::new(id.clone(), addr, true)
    })
    .await
    .unwrap()
    .0
}

async fn submit(b: &MemoryBackend<()>, args: Value) -> jmap_server::HandlerOutput {
    handle_submission_set(b, &(), &BTreeMap::new(), args, "c0")
        .await
        .unwrap()
}

/// The derived envelope of a submission of [`MESSAGE`] as `identity_addr`.
async fn derived_envelope(identity_addr: &'static str) -> Value {
    let b = MemoryBackend::<()>::default();
    let account = Id::from("acc");
    let e = email(&b, &account).await;
    let i = identity(&b, &account, identity_addr).await;
    let out = submit(
        &b,
        json!({"accountId": "acc", "create": {"s": {"identityId": i.as_ref(), "emailId": e.as_ref()}}}),
    )
    .await;
    out.primary["created"]["s"]["envelope"].clone()
}

#[tokio::test]
async fn wildcard_identity_uses_the_from_address() {
    let env = derived_envelope("*@example.org").await;
    assert_eq!(env["mailFrom"]["email"], "Jane_Sender@example.org", "{env}");
}

#[tokio::test]
async fn disallowed_from_falls_back_to_the_identity() {
    let env = derived_envelope("other@example.net").await;
    assert_eq!(env["mailFrom"]["email"], "other@example.net", "{env}");
}

#[tokio::test]
async fn rcpt_to_is_deduplicated() {
    let env = derived_envelope("Jane_Sender@example.org").await;
    let rcpt: Vec<&str> = env["rcptTo"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["email"].as_str().unwrap())
        .collect();
    assert_eq!(rcpt, ["joe@example.com", "ann@example.com"], "{env}");
}

/// A call with no create, update or destroy names an existing submission
/// in onSuccessDestroyEmail: nothing succeeded, so its Email stays.
#[tokio::test]
async fn on_success_ignores_submissions_the_call_did_not_touch() {
    let b = MemoryBackend::<()>::default();
    let account = Id::from("acc");
    let e = email(&b, &account).await;
    let i = identity(&b, &account, "Jane_Sender@example.org").await;
    let out = submit(
        &b,
        json!({"accountId": "acc", "create": {"s": {"identityId": i.as_ref(), "emailId": e.as_ref()}}}),
    )
    .await;
    let sid = out.primary["created"]["s"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let out = submit(
        &b,
        json!({"accountId": "acc", "onSuccessDestroyEmail": [sid]}),
    )
    .await;
    assert!(
        out.extra_invocations.is_empty(),
        "{:?}",
        out.extra_invocations
    );
    let got = handle_email_get(
        &b,
        &(),
        json!({"accountId": "acc", "ids": [e.as_ref()], "properties": ["id"]}),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(got.primary["list"][0]["id"], e.as_ref(), "{}", got.primary);
}
