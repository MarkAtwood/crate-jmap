//! Server-set values and empty patches in mail `/set` updates
//! (bd:JMAP-t8ej.25.1).
//!
//! Oracle, RFC 8620 §5.3 (update): "Any server-set properties MAY be
//! included in the patch if their value is identical to the current server
//! value (before applying the patches to the object).  Otherwise, the
//! update MUST be rejected with an "invalidProperties" SetError." and "This
//! patch definition is designed such that an entire Foo object is also a
//! valid PatchObject."
//!
//! An empty patch `{}`, literal or left empty once the identical
//! server-set values are dropped, succeeds as a no-op: `updated` maps the
//! id to null, nothing is written, and the state is unchanged (decision
//! recorded on JMAP-t8ej.25).
#![allow(async_fn_in_trait)]

mod common;

use std::collections::BTreeMap;

use common::MemoryBackend;
use jmap_mail_server::{
    MailBackend, handle_email_get, handle_email_set, handle_identity_get, handle_identity_set,
    handle_mailbox_get, handle_mailbox_set, handle_submission_set, handle_vacation_set,
};
use jmap_mail_types::Identity;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "account1";

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT));
    backend
}

/// Assert `resp` reports `id` as a no-op update: `updated[id]` is null,
/// nothing failed, and the state did not move.
fn assert_noop(resp: &Value, id: &str) {
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let updated = resp["updated"]
        .as_object()
        .unwrap_or_else(|| panic!("{resp}"));
    assert_eq!(updated.get(id), Some(&Value::Null), "{resp}");
    assert_eq!(resp["oldState"], resp["newState"], "{resp}");
}

async fn import_email(backend: &MemoryBackend) -> Id {
    let blob_id = Id::from("blob-server-set");
    backend.store_blob(
        &blob_id,
        b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Hi\r\n\r\nBody.\r\n".to_vec(),
    );
    let (email_id, _) = backend
        .import_email(
            &(),
            &Id::from(ACCOUNT),
            &blob_id,
            &[Id::from("inbox")],
            &[],
            None,
        )
        .await
        .expect("import_email");
    email_id
}

async fn create_identity(backend: &MemoryBackend) -> Id {
    let identity = |id: &Id| Identity::new(id.clone(), "alice@example.com", true);
    let (id, _) = backend
        .create_object::<Identity, _>(&(), &Id::from(ACCOUNT), "i0", identity)
        .await
        .expect("create Identity");
    id
}

async fn mailbox_set(backend: &MemoryBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } =
        handle_mailbox_set(backend, &(), &BTreeMap::new(), args, &[])
            .await
            .expect("Mailbox/set");
    primary
}

async fn get_mailbox(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_mailbox_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
        &[],
    )
    .await
    .expect("Mailbox/get");
    primary["list"][0].clone()
}

async fn create_mailbox(backend: &MemoryBackend, name: &str) -> String {
    let resp = mailbox_set(
        backend,
        json!({ "accountId": ACCOUNT, "create": { "m": { "name": name } } }),
    )
    .await;
    resp["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Mailbox not created: {resp}"))
        .to_owned()
}

/// §5.3: the Mailbox as `Mailbox/get` returns it, server-set counts and
/// `myRights` included, with only `name` changed, updates.
#[tokio::test]
async fn mailbox_update_with_identical_server_set_values_is_updated() {
    let backend = backend();
    let id = create_mailbox(&backend, "Work").await;
    let mut whole = get_mailbox(&backend, &id).await;
    for key in ["id", "totalEmails", "unreadEmails", "myRights"] {
        assert!(
            whole.get(key).is_some(),
            "Mailbox/get returns {key}: {whole}"
        );
    }
    whole["name"] = json!("Renamed");

    let resp = mailbox_set(
        &backend,
        json!({ "accountId": ACCOUNT, "update": { &id: whole } }),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
    assert_eq!(get_mailbox(&backend, &id).await["name"], json!("Renamed"));
}

/// `{}` and a patch of identical server-set values alone are no-ops; a
/// changed server-set value is still `invalidProperties` (§5.3).
#[tokio::test]
async fn mailbox_empty_patch_is_noop() {
    let backend = backend();
    let a = create_mailbox(&backend, "A").await;
    let b = create_mailbox(&backend, "B").await;
    let c = create_mailbox(&backend, "C").await;
    let stored_b = get_mailbox(&backend, &b).await;

    let resp = mailbox_set(
        &backend,
        json!({ "accountId": ACCOUNT, "update": {
            &a: {},
            &b: { "id": &b, "totalEmails": stored_b["totalEmails"] },
        } }),
    )
    .await;
    assert_noop(&resp, &a);
    assert_noop(&resp, &b);

    let resp = mailbox_set(
        &backend,
        json!({ "accountId": ACCOUNT, "update": { &c: { "totalEmails": 99 } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&c]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notUpdated"][&c]["properties"], json!(["totalEmails"]));
}

/// RFC 8620 §5.3 notFound: "The id given to update/destroy cannot be
/// found." An empty patch does not hide a missing id.
#[tokio::test]
async fn mailbox_empty_patch_of_missing_id_is_not_found() {
    let backend = backend();
    let resp = mailbox_set(
        &backend,
        json!({ "accountId": ACCOUNT, "update": { "nope": {} } }),
    )
    .await;
    assert_eq!(resp["notUpdated"]["nope"]["type"], "notFound", "{resp}");
    assert!(resp["updated"].is_null(), "{resp}");
}

async fn email_set(backend: &MemoryBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_email_set(backend, &(), &BTreeMap::new(), args, &[])
        .await
        .expect("Email/set");
    primary
}

async fn get_email(backend: &MemoryBackend, id: &Id) -> Value {
    let HandlerOutput { primary, .. } = handle_email_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT, "ids": [id.as_ref()] }),
        &[],
    )
    .await
    .expect("Email/get");
    primary["list"][0].clone()
}

/// §5.3: server-set `id`, `blobId`, `threadId`, `size` and immutable
/// `receivedAt` repeated with their stored values beside a keyword change
/// update the Email.
#[tokio::test]
async fn email_update_with_identical_server_set_values_is_updated() {
    let backend = backend();
    let id = import_email(&backend).await;
    let stored = get_email(&backend, &id).await;
    let mut patch = serde_json::Map::new();
    for key in ["id", "blobId", "threadId", "size", "receivedAt"] {
        let value = stored.get(key).unwrap_or_else(|| panic!("{key}: {stored}"));
        patch.insert(key.to_owned(), value.clone());
    }
    patch.insert("keywords/$seen".to_owned(), json!(true));

    let resp = email_set(
        &backend,
        json!({ "accountId": ACCOUNT, "update": { id.as_ref(): patch } }),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert!(resp["updated"].get(id.as_ref()).is_some(), "{resp}");
    assert_eq!(
        get_email(&backend, &id).await["keywords"]["$seen"],
        json!(true)
    );
}

#[tokio::test]
async fn email_empty_patch_is_noop() {
    let backend = backend();
    let id = import_email(&backend).await;
    let resp = email_set(
        &backend,
        json!({ "accountId": ACCOUNT, "update": { id.as_ref(): {} } }),
    )
    .await;
    assert_noop(&resp, id.as_ref());
}

async fn identity_set(backend: &MemoryBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_identity_set(backend, &(), args)
        .await
        .expect("Identity/set");
    primary
}

async fn get_identity(backend: &MemoryBackend, id: &Id) -> Value {
    let HandlerOutput { primary, .. } = handle_identity_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT, "ids": [id.as_ref()] }),
    )
    .await
    .expect("Identity/get");
    primary["list"][0].clone()
}

/// §5.3: the Identity as `Identity/get` returns it, with server-set `id`
/// and `mayDelete` and immutable `email`, and `name` changed, updates.
#[tokio::test]
async fn identity_update_with_identical_server_set_values_is_updated() {
    let backend = backend();
    let id = create_identity(&backend).await;
    let mut whole = get_identity(&backend, &id).await;
    for key in ["id", "mayDelete", "email"] {
        assert!(
            whole.get(key).is_some(),
            "Identity/get returns {key}: {whole}"
        );
    }
    whole["name"] = json!("Alice");

    let resp = identity_set(
        &backend,
        json!({ "accountId": ACCOUNT, "update": { id.as_ref(): whole } }),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert!(resp["updated"].get(id.as_ref()).is_some(), "{resp}");
    assert_eq!(get_identity(&backend, &id).await["name"], json!("Alice"));
}

#[tokio::test]
async fn identity_empty_patch_is_noop() {
    let backend = backend();
    let id = create_identity(&backend).await;
    let resp = identity_set(
        &backend,
        json!({ "accountId": ACCOUNT, "update": { id.as_ref(): {} } }),
    )
    .await;
    assert_noop(&resp, id.as_ref());
}

/// VacationResponse `id` is always "singleton" (RFC 8621 §8): `{}` and
/// `{"id": "singleton"}` are both no-ops.
#[tokio::test]
async fn vacation_empty_patch_is_noop() {
    let backend = backend();
    for patch in [json!({}), json!({ "id": "singleton" })] {
        let HandlerOutput { primary: resp, .. } = handle_vacation_set(
            &backend,
            &(),
            json!({ "accountId": ACCOUNT, "update": { "singleton": patch } }),
        )
        .await
        .expect("VacationResponse/set");
        assert_noop(&resp, "singleton");
    }
}

/// Create an EmailSubmission for a fresh Email; `extra(email_id)` is
/// merged into the request, for `onSuccessUpdateEmail`. Returns the
/// submission id, the Email id and the response.
async fn submit(
    backend: &MemoryBackend,
    extra: impl Fn(&Id) -> Value,
) -> (String, Id, HandlerOutput) {
    let identity_id = create_identity(backend).await;
    let email_id = import_email(backend).await;
    let mut args = json!({
        "accountId": ACCOUNT,
        "create": { "s0": {
            "identityId": identity_id.as_ref(),
            "emailId": email_id.as_ref(),
        } },
    });
    if let (Some(args), Value::Object(extra)) = (args.as_object_mut(), extra(&email_id)) {
        args.extend(extra);
    }
    let out = handle_submission_set(backend, &(), &BTreeMap::new(), args, "c1")
        .await
        .expect("EmailSubmission/set");
    let id = out.primary["created"]["s0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("submission not created: {}", out.primary))
        .to_owned();
    (id, email_id, out)
}

/// The implicit `Email/set` response of `out`.
fn implicit_email_set(out: &HandlerOutput) -> &Value {
    let inv = out
        .extra_invocations
        .iter()
        .find(|inv| inv.name == "Email/set")
        .unwrap_or_else(|| panic!("no implicit Email/set: {:?}", out.extra_invocations));
    &inv.arguments
}

#[tokio::test]
async fn submission_empty_patch_is_noop() {
    let backend = backend();
    let (id, _, _) = submit(&backend, |_| json!({})).await;
    let HandlerOutput { primary: resp, .. } = handle_submission_set(
        &backend,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT, "update": { &id: {} } }),
        "c2",
    )
    .await
    .expect("EmailSubmission/set");
    assert_noop(&resp, &id);
}

/// §5.3 applies to the implicit Email/set of `onSuccessUpdateEmail`
/// (RFC 8621 §7.5): an identical `id` beside a keyword change updates.
#[tokio::test]
async fn submission_on_success_update_with_identical_id_is_updated() {
    let backend = backend();
    let (_, email_id, out) = submit(&backend, |id| {
        json!({ "onSuccessUpdateEmail": {
            "#s0": { "id": id.as_ref(), "keywords/$seen": true },
        } })
    })
    .await;
    let email_set = implicit_email_set(&out);
    assert!(email_set["notUpdated"].is_null(), "{email_set}");
    assert!(
        email_set["updated"].get(email_id.as_ref()).is_some(),
        "{email_set}"
    );
    assert_eq!(
        get_email(&backend, &email_id).await["keywords"]["$seen"],
        json!(true)
    );
}

#[tokio::test]
async fn submission_on_success_empty_patch_is_noop() {
    let backend = backend();
    let (_, email_id, out) = submit(
        &backend,
        |_| json!({ "onSuccessUpdateEmail": { "#s0": {} } }),
    )
    .await;
    assert_noop(implicit_email_set(&out), email_id.as_ref());
}

#[cfg(feature = "sieve")]
mod sieve {
    use super::*;
    use common::VALID_SIEVE_SCRIPT;
    use jmap_mail_server::handle_sieve_set;

    async fn sieve_set(backend: &MemoryBackend, args: Value) -> Value {
        let HandlerOutput { primary, .. } = handle_sieve_set(backend, &(), args)
            .await
            .expect("SieveScript/set");
        primary
    }

    async fn create_script(backend: &MemoryBackend) -> String {
        backend.store_blob(&Id::from("script-blob"), VALID_SIEVE_SCRIPT.to_vec());
        let resp = sieve_set(
            backend,
            json!({ "accountId": ACCOUNT, "create": {
                "s": { "name": "one", "blobId": "script-blob" },
            } }),
        )
        .await;
        resp["created"]["s"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("script not created: {resp}"))
            .to_owned()
    }

    /// §5.3: server-set `id` and `isActive` (RFC 9661 §2) repeated with
    /// their stored values beside a `name` change update the script.
    #[tokio::test]
    async fn sieve_update_with_identical_server_set_values_is_updated() {
        let backend = backend();
        let id = create_script(&backend).await;
        let resp = sieve_set(
            &backend,
            json!({ "accountId": ACCOUNT, "update": {
                &id: { "id": &id, "isActive": false, "name": "two" },
            } }),
        )
        .await;
        assert!(resp["notUpdated"].is_null(), "{resp}");
        assert!(resp["updated"].get(&id).is_some(), "{resp}");
    }

    #[tokio::test]
    async fn sieve_empty_patch_is_noop() {
        let backend = backend();
        let id = create_script(&backend).await;
        let resp = sieve_set(
            &backend,
            json!({ "accountId": ACCOUNT, "update": { &id: {} } }),
        )
        .await;
        assert_noop(&resp, &id);
    }
}

#[cfg(feature = "mdn")]
mod mdn {
    use super::*;
    use jmap_mail_server::mdn::handle_mdn_send;

    /// Send one MDN for a fresh Email with `patch` as its
    /// `onSuccessUpdateEmail` entry; returns the Email id and the
    /// implicit Email/set response.
    async fn send(backend: &MemoryBackend, patch: impl Fn(&Id) -> Value) -> (Id, Value) {
        let blob_id = Id::from("blob-mdn");
        backend.store_blob(
            &blob_id,
            b"From: Jane Sender <Jane_Sender@example.org>\r\n\
Message-ID: <199509192301.23456@example.org>\r\n\
To: Joe Recipient <Joe_Recipient@example.com>\r\n\
Subject: World domination\r\n\
Disposition-Notification-To: Jane Sender <Jane_Sender@example.org>\r\n\
\r\n\
The email body.\r\n"
                .to_vec(),
        );
        let (email_id, _) = backend
            .import_email(
                &(),
                &Id::from(ACCOUNT),
                &blob_id,
                &[Id::from("inbox")],
                &[],
                None,
            )
            .await
            .expect("import_email");
        let identity_id = create_identity(backend).await;
        let out = handle_mdn_send(
            backend,
            &(),
            json!({
                "accountId": ACCOUNT,
                "identityId": identity_id.as_ref(),
                "send": { "k1": {
                    "forEmailId": email_id.as_ref(),
                    "disposition": {
                        "actionMode": "manual-action",
                        "sendingMode": "mdn-sent-manually",
                        "type": "displayed",
                    },
                } },
                "onSuccessUpdateEmail": { "#k1": patch(&email_id) },
            }),
            "c1",
        )
        .await
        .expect("MDN/send");
        let email_set = implicit_email_set(&out).clone();
        (email_id, email_set)
    }

    /// §5.3 in the implicit Email/set of MDN/send (RFC 9007 §2.1).
    #[tokio::test]
    async fn mdn_on_success_update_with_identical_id_is_updated() {
        let backend = backend();
        let (email_id, email_set) = send(
            &backend,
            |id| json!({ "id": id.as_ref(), "keywords/$mdnsent": true }),
        )
        .await;
        assert!(email_set["notUpdated"].is_null(), "{email_set}");
        assert!(
            email_set["updated"].get(email_id.as_ref()).is_some(),
            "{email_set}"
        );
        assert_eq!(
            get_email(&backend, &email_id).await["keywords"]["$mdnsent"],
            json!(true)
        );
    }
}
