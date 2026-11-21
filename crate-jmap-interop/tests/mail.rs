//! RFC 8621 (JMAP for Mail) facts checked with `jmap-mail-client` against
//! the configured server. Run with `-- --ignored`; see
//! docs/interop/README.md.
//!
//! Every test creates its own objects under [`unique`] names and removes
//! them at the end, so the tests can run in parallel on a shared server.

use std::collections::BTreeMap;
use std::time::Duration;

use jmap_base_client::{QueryParams, UploadBlobSessionParams};
use jmap_interop::{Credentials, InteropEnv, known_divergence, unique};
use jmap_mail_client::{
    EmailGetParams, EmailImportInput, EmailSubmissionSetParams, JmapMailExt, SessionClient,
};
use jmap_mail_types::{EmailFilterCondition, MailboxRole};
use jmap_types::{Filter, Id, PatchObject};
use serde_json::{Value, json};

async fn mail_session(who: impl FnOnce(&InteropEnv) -> Credentials) -> SessionClient {
    let env = InteropEnv::require();
    let client = env.client(&who(&env)).expect("client");
    let session = client.fetch_session().await.expect("Session");
    client.with_mail_session(session)
}

async fn alice() -> SessionClient {
    mail_session(|e| e.user.clone()).await
}

async fn bob() -> SessionClient {
    mail_session(|e| e.user2.clone()).await
}

/// The id of the `/set` creation `key`, which must have succeeded.
fn created_id(
    resp: &jmap_mail_client::SetResponse<jmap_mail_client::PartialObject>,
    key: &str,
) -> Id {
    assert!(
        resp.not_created.is_empty(),
        "notCreated: {:?}",
        resp.not_created
    );
    resp.created
        .get(key)
        .and_then(|o| o.id.clone())
        .unwrap_or_else(|| panic!("RFC 8620 §5.3: created[{key}] carries the server-set id"))
}

/// Create a top-level Mailbox named `name`.
async fn create_mailbox(mail: &SessionClient, name: &str) -> Id {
    let resp = mail
        .mailbox_set(
            Some(json!({"mb": {"name": name, "parentId": null}})),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Mailbox/set create");
    created_id(&resp, "mb")
}

/// Destroy a Mailbox and the Emails left in it.
async fn destroy_mailbox(mail: &SessionClient, id: &Id) {
    let resp = mail
        .mailbox_set(
            None,
            None,
            Some(vec![id.clone()]),
            None,
            Some(jmap_mail_client::MailboxSetParams {
                on_destroy_remove_emails: Some(true),
                ..Default::default()
            }),
            None,
        )
        .await
        .expect("Mailbox/set destroy");
    assert_eq!(resp.destroyed, vec![id.clone()], "{:?}", resp.not_destroyed);
}

fn in_mailbox(id: &Id) -> Filter<EmailFilterCondition> {
    let mut cond = EmailFilterCondition::default();
    cond.in_mailbox = Some(id.clone());
    Filter::Condition(cond)
}

fn subject_is(subject: &str) -> Filter<EmailFilterCondition> {
    let mut cond = EmailFilterCondition::default();
    cond.subject = Some(subject.to_owned());
    Filter::Condition(cond)
}

/// Whether `email` has `keyword` (RFC 8621 §4.1.1). Tests check only
/// the keywords they set or removed: a server may set others itself, such
/// as `$hasnoattachment`.
fn has_keyword(email: &jmap_mail_types::Email, keyword: &str) -> bool {
    let keywords = serde_json::to_value(&email.keywords).expect("keywords serialize");
    keywords.get(keyword) == Some(&Value::Bool(true))
}

/// RFC 8621 §2: Mailbox/get returns the account's Mailboxes with their
/// required properties; at most one Mailbox has each role, and new mail
/// goes to the one with role `inbox`, which a mail account has.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn mailbox_get_lists_roles() {
    let _server = jmap_interop::exclusive().await;
    let mail = alice().await;
    let resp = mail.mailbox_get(None, None).await.expect("Mailbox/get");
    assert_eq!(&resp.account_id, mail.mail_account_id().expect("account"));
    let roles: Vec<&MailboxRole> = resp.list.iter().filter_map(|m| m.role.as_ref()).collect();
    for role in &roles {
        assert_eq!(
            roles.iter().filter(|r| r == &role).count(),
            1,
            "RFC 8621 §2: no two Mailboxes may have the same role ({role:?})"
        );
    }
    assert!(roles.contains(&&MailboxRole::Inbox), "roles: {roles:?}");
    for m in &resp.list {
        assert!(!m.name.is_empty(), "RFC 8621 §2: name is non-empty");
    }
}

/// RFC 8621 §2.5 Mailbox/set create and destroy, §2.2 Mailbox/changes
/// reporting both, RFC 8620 §5.1 notFound for the destroyed id.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn mailbox_create_changes_destroy() {
    let _server = jmap_interop::exclusive().await;
    let mail = alice().await;
    let before = mail
        .mailbox_get(Some(&[]), None)
        .await
        .expect("Mailbox/get state");

    let name = unique("interop-box");
    let id = create_mailbox(&mail, &name).await;

    let got = mail
        .mailbox_get(Some(std::slice::from_ref(&id)), None)
        .await
        .expect("Mailbox/get");
    assert_eq!(got.list.len(), 1);
    let mb = &got.list[0];
    assert_eq!(mb.id, id);
    assert_eq!(mb.name, name);
    assert_eq!(mb.parent_id, None);
    assert_eq!(mb.role, None);
    assert_eq!(mb.total_emails, 0);

    let changes = mail
        .mailbox_changes(&before.state, None)
        .await
        .expect("Mailbox/changes");
    assert_eq!(changes.old_state, before.state);
    assert!(
        changes.created.contains(&id),
        "created: {:?}",
        changes.created
    );
    assert_eq!(
        changes.new_state, got.state,
        "RFC 8620 §5.2: newState is the current state"
    );

    destroy_mailbox(&mail, &id).await;

    let after = mail
        .mailbox_changes(&got.state, None)
        .await
        .expect("Mailbox/changes after destroy");
    assert!(
        after.destroyed.contains(&id),
        "destroyed: {:?}",
        after.destroyed
    );
    let gone = mail
        .mailbox_get(Some(std::slice::from_ref(&id)), None)
        .await
        .expect("Mailbox/get destroyed");
    assert!(gone.list.is_empty());
    assert_eq!(gone.not_found, vec![id]);
}

/// RFC 8620 §5.3: destroying an id that does not exist is reported in
/// notDestroyed with SetError type `notFound`.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn destroy_unknown_mailbox_is_not_found() {
    let _server = jmap_interop::exclusive().await;
    let mail = alice().await;
    let ghost = Id::from("zz9-no-such-mailbox");
    let resp = mail
        .mailbox_set(None, None, Some(vec![ghost.clone()]), None, None, None)
        .await
        .expect("Mailbox/set");
    let err = resp
        .not_destroyed
        .get(&ghost)
        .unwrap_or_else(|| panic!("notDestroyed lacks the id: {resp:?}"));
    assert_eq!(err.error_type.as_str(), "notFound");
}

/// RFC 8621 §7.5 with RFC 8620 §5.3: EmailSubmission/set destroy of an
/// unknown id is `notFound`. RFC 8620 §5.3 makes the response's
/// `newState` mandatory; `jmap-mail-client` tolerates a server that
/// omits it (decision 67) and decodes it as the empty state, which is
/// still reported as a divergence.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn destroy_unknown_submission_is_not_found() {
    let _server = jmap_interop::exclusive().await;
    let mail = alice().await;
    let ghost = Id::from("zz9-no-such-submission");
    let resp = mail
        .email_submission_set(None, None, Some(vec![ghost.clone()]), None, None, None)
        .await
        .expect("EmailSubmission/set");
    let err = resp
        .not_destroyed
        .get(&ghost)
        .unwrap_or_else(|| panic!("notDestroyed lacks the id: {resp:?}"));
    assert_eq!(err.error_type.as_str(), "notFound");
    if resp.new_state.as_ref().is_empty() {
        known_divergence(
            "RFC 8620 §5.3: EmailSubmission/set response has no newState \
             (decoded as the empty state)",
        );
    }
}

/// RFC 8620 §6.1 upload, RFC 8621 §4.8 Email/import, §4.4 Email/query
/// with `inMailbox`, §4.1 Email/get of the parsed header fields, §3
/// Thread/get, §4.3 Email/changes across create, keyword update and
/// destroy.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn email_import_query_get_changes() {
    let _server = jmap_interop::exclusive().await;
    let mail = alice().await;
    let account = mail.mail_account_id().expect("account").clone();
    let mailbox = create_mailbox(&mail, &unique("interop-import")).await;
    let subject = unique("interop import");
    let message = format!(
        "From: Interop Sender <sender@example.net>\r\n\
         To: alice@example.org\r\n\
         Subject: {subject}\r\n\
         Date: Sat, 21 Nov 2026 12:00:00 +0000\r\n\
         Message-ID: <{subject_id}@example.net>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         Hello from the crate-jmap interop tests.\r\n",
        subject_id = subject.replace(' ', "-"),
    );

    let upload = mail
        .client()
        .upload_blob_session(
            mail.session(),
            UploadBlobSessionParams {
                account_id: &account,
                content_type: "message/rfc822",
                data: bytes::Bytes::from(message.clone()),
            },
        )
        .await
        .expect("upload");
    assert_eq!(upload.account_id, account);
    assert_eq!(
        upload.size,
        message.len() as u64,
        "RFC 8620 §6.1: size in octets"
    );

    let state0 = mail
        .email_get(Some(&[]), None, None)
        .await
        .expect("Email/get state")
        .state;

    let mut emails = BTreeMap::new();
    emails.insert(
        "imp".to_owned(),
        EmailImportInput {
            blob_id: &upload.blob_id,
            mailbox_ids: std::slice::from_ref(&mailbox),
            keywords: None,
            received_at: None,
            extra: Default::default(),
        },
    );
    let imported = mail
        .email_import(&emails, None)
        .await
        .expect("Email/import");
    assert!(
        imported.not_created.as_ref().is_none_or(|m| m.is_empty()),
        "{imported:?}"
    );
    let created = &imported.created.as_ref().expect("created")["imp"];
    let id = created.id.clone();
    assert_eq!(created.size, message.len() as u64);

    let query = mail
        .email_query(
            Some(&in_mailbox(&mailbox)),
            None,
            &QueryParams::default(),
            None,
        )
        .await
        .expect("Email/query");
    assert_eq!(query.ids, vec![id.clone()]);

    let got = mail
        .email_get(
            Some(std::slice::from_ref(&id)),
            None,
            Some(EmailGetParams {
                fetch_text_body_values: Some(true),
                ..Default::default()
            }),
        )
        .await
        .expect("Email/get");
    assert_eq!(got.list.len(), 1);
    let email = &got.list[0];
    assert_eq!(email.id, id);
    assert_eq!(email.thread_id, created.thread_id);
    assert_eq!(email.subject.as_deref(), Some(subject.as_str()));
    let from = email.from.as_ref().expect("from");
    assert_eq!(from.len(), 1);
    assert_eq!(from[0].email, "sender@example.net");
    assert_eq!(from[0].name.as_deref(), Some("Interop Sender"));
    assert_eq!(email.mailbox_ids.keys().collect::<Vec<_>>(), vec![&mailbox]);
    assert!(
        !has_keyword(email, "$seen") && !has_keyword(email, "$draft"),
        "RFC 8621 §4.8: no keywords were given"
    );
    let part = email.text_body.first().expect("a text body part");
    let part_id = part.part_id.clone().expect("partId");
    assert_eq!(
        email.body_values[&part_id].value.trim_end(),
        "Hello from the crate-jmap interop tests."
    );

    let threads = mail
        .thread_get(Some(std::slice::from_ref(&email.thread_id)), None)
        .await
        .expect("Thread/get");
    assert!(
        threads.list[0].email_ids.contains(&id),
        "RFC 8621 §3: the thread lists the Email"
    );

    let c1 = mail
        .email_changes(&state0, None)
        .await
        .expect("Email/changes");
    assert!(c1.created.contains(&id), "created: {:?}", c1.created);

    // RFC 8621 §4.6: keywords are patched by path.
    let mut patch = PatchObject::new();
    patch
        .as_map_mut()
        .insert("keywords/$seen".into(), json!(true));
    let upd = mail
        .email_set(
            None,
            Some(BTreeMap::from([(id.clone(), patch)])),
            None,
            None,
            None,
        )
        .await
        .expect("Email/set update");
    assert!(upd.updated.contains_key(&id), "{upd:?}");
    let c2 = mail
        .email_changes(&c1.new_state, None)
        .await
        .expect("Email/changes");
    assert!(c2.updated.contains(&id), "updated: {:?}", c2.updated);
    let seen = mail
        .email_get(Some(std::slice::from_ref(&id)), None, None)
        .await
        .expect("Email/get");
    assert!(has_keyword(&seen.list[0], "$seen"));

    let del = mail
        .email_set(None, None, Some(vec![id.clone()]), None, None)
        .await
        .expect("Email/set destroy");
    assert_eq!(del.destroyed, vec![id.clone()]);
    let c3 = mail
        .email_changes(&c2.new_state, None)
        .await
        .expect("Email/changes");
    assert!(c3.destroyed.contains(&id), "destroyed: {:?}", c3.destroyed);

    destroy_mailbox(&mail, &mailbox).await;
}

/// RFC 8621 §4.6: Email/set create from JMAP properties with a text body
/// value; the server sets id, blobId, threadId and size.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn email_set_create_draft() {
    let _server = jmap_interop::exclusive().await;
    let mail = alice().await;
    let mailbox = create_mailbox(&mail, &unique("interop-draft")).await;
    let subject = unique("interop draft");
    let resp = mail
        .email_set(
            Some(json!({"d": {
                "mailboxIds": {mailbox.as_ref(): true},
                "keywords": {"$draft": true},
                "from": [{"name": "Alice", "email": "alice@example.org"}],
                "to": [{"email": "bob@example.org"}],
                "subject": subject,
                "bodyValues": {"t": {"value": "Draft body."}},
                "textBody": [{"partId": "t", "type": "text/plain"}],
            }})),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Email/set create");
    let id = created_id(&resp, "d");
    let echoed = &resp.created["d"].extra;
    for key in ["blobId", "threadId", "size"] {
        assert!(
            echoed.contains_key(key),
            "RFC 8621 §4.6: created has {key}: {echoed:?}"
        );
    }

    let got = mail
        .email_get(
            Some(std::slice::from_ref(&id)),
            None,
            Some(EmailGetParams {
                fetch_text_body_values: Some(true),
                ..Default::default()
            }),
        )
        .await
        .expect("Email/get");
    let email = &got.list[0];
    assert_eq!(email.subject.as_deref(), Some(subject.as_str()));
    assert!(has_keyword(email, "$draft"));
    let to = email.to.as_ref().expect("to");
    assert_eq!(to[0].email, "bob@example.org");
    let part_id = email.text_body[0].part_id.clone().expect("partId");
    assert_eq!(email.body_values[&part_id].value, "Draft body.");

    destroy_mailbox(&mail, &mailbox).await;
}

/// RFC 8621 §6.1: Identity/get lists an Identity for the user's address,
/// with every property present.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn identity_get_lists_own_address() {
    let _server = jmap_interop::exclusive().await;
    let env = InteropEnv::require();
    let mail = alice().await;
    let resp = mail.identity_get(None, None).await.expect("Identity/get");
    assert!(
        resp.list.iter().any(|i| i.email == env.user.username),
        "identities: {:?}",
        resp.list.iter().map(|i| &i.email).collect::<Vec<_>>()
    );
}

/// RFC 8621 §7.5: EmailSubmission/set sends a draft from alice to bob,
/// `onSuccessUpdateEmail` with the `#creationId` key patches the sent
/// Email, §7.1 EmailSubmission/get reads the submission back; bob's
/// account then holds the message.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn submission_delivers_to_second_account() {
    let _server = jmap_interop::exclusive().await;
    let env = InteropEnv::require();
    let mail = alice().await;
    let identity = mail
        .identity_get(None, None)
        .await
        .expect("Identity/get")
        .list
        .into_iter()
        .find(|i| i.email == env.user.username)
        .expect("an Identity for alice");

    let mailbox = create_mailbox(&mail, &unique("interop-outbox")).await;
    let subject = unique("interop submission");
    let draft = mail
        .email_set(
            Some(json!({"d": {
                "mailboxIds": {mailbox.as_ref(): true},
                "keywords": {"$draft": true},
                "from": [{"email": env.user.username}],
                "to": [{"email": env.user2.username}],
                "subject": subject,
                "bodyValues": {"t": {"value": "Sent by the interop tests."}},
                "textBody": [{"partId": "t", "type": "text/plain"}],
            }})),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Email/set create");
    let email_id = created_id(&draft, "d");

    let mut on_success = BTreeMap::new();
    let mut patch = PatchObject::new();
    patch
        .as_map_mut()
        .insert("keywords/$draft".into(), Value::Null);
    on_success.insert("#s".to_owned(), patch);
    let sub = mail
        .email_submission_set(
            Some(json!({"s": {"identityId": identity.id, "emailId": email_id}})),
            None,
            None,
            None,
            Some(EmailSubmissionSetParams {
                on_success_update_email: Some(on_success),
                ..Default::default()
            }),
            None,
        )
        .await
        .expect("EmailSubmission/set");
    let sub_id = created_id(&sub, "s");

    let got = mail
        .email_submission_get(Some(std::slice::from_ref(&sub_id)), None)
        .await
        .expect("EmailSubmission/get");
    assert_eq!(got.list.len(), 1);
    assert_eq!(got.list[0].email_id, email_id);
    assert_eq!(got.list[0].identity_id, identity.id);

    // The implicit Email/set of onSuccessUpdateEmail removed $draft.
    let sent = mail
        .email_get(Some(std::slice::from_ref(&email_id)), None, None)
        .await
        .expect("Email/get");
    assert!(!has_keyword(&sent.list[0], "$draft"));

    // Delivery is asynchronous: poll bob's account.
    let bob = bob().await;
    let mut found = Vec::new();
    for _ in 0..40 {
        found = bob
            .email_query(
                Some(&subject_is(&subject)),
                None,
                &QueryParams::default(),
                None,
            )
            .await
            .expect("bob Email/query")
            .ids;
        if !found.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(found.len(), 1, "bob did not receive the message");
    let received = bob
        .email_get(Some(&found), None, None)
        .await
        .expect("bob Email/get");
    let email = &received.list[0];
    assert_eq!(email.subject.as_deref(), Some(subject.as_str()));
    assert_eq!(
        email.from.as_ref().expect("from")[0].email,
        env.user.username
    );

    bob.email_set(None, None, Some(found), None, None)
        .await
        .expect("bob Email/set destroy");
    destroy_mailbox(&mail, &mailbox).await;
}
