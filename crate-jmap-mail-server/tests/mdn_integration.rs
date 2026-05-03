//! MDN integration tests for jmap-mail-server (draft-ietf-jmap-mdn-17).
//!
//! All tests in this file are compiled and run only when `--features mdn` is passed.
//! Test vectors come from draft-ietf-jmap-mdn-17 §3.1, §3.3, and RFC 8098 §9.
#![cfg(feature = "mdn")]
#![allow(async_fn_in_trait)]

mod common;

use common::{MemoryBackend, INVALID_MDN_BLOB, VALID_MDN_BLOB};
use jmap_mail_server::{
    handle_mdn_parse, handle_mdn_send, mdn::MDN_PARSE_MAX_BLOB_IDS, JmapBackend, MailBackend,
};
use jmap_mail_types::Identity;
use jmap_types::Id;

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

/// Create one email suitable for MDN testing in `account_id`.
///
/// The email has:
/// - `Disposition-Notification-To: Jane Sender <Jane_Sender@example.org>`
/// - `Message-ID: <199509192301.23456@example.org>`
/// - `Subject: World domination`
/// - placed in a mailbox called "inbox-mdn" (not the role inbox — just a container)
/// - no keywords (in particular, no `$mdnsent`)
///
/// Returns the JMAP Id of the newly created email.
///
/// # Panics
///
/// Panics on any backend error — this is test fixture setup code.
async fn setup_mdn_email(backend: &MemoryBackend, account_id: &Id) -> Id {
    // Raw RFC 5322 bytes with the required MDN-related headers.
    // These specific header values are from draft-ietf-jmap-mdn-17 §3.1 example.
    let raw = b"From: Jane Sender <Jane_Sender@example.org>\r\n\
Message-ID: <199509192301.23456@example.org>\r\n\
To: Joe Recipient <Joe_Recipient@example.com>\r\n\
Subject: World domination\r\n\
Disposition-Notification-To: Jane Sender <Jane_Sender@example.org>\r\n\
\r\n\
The email body.\r\n";

    let blob_id = Id::from("blob-mdn-email");
    backend.store_blob(&blob_id, raw.to_vec());

    let mailbox_id = Id::from("inbox-mdn");
    let (email_id, _) = backend
        .import_email(account_id, &blob_id, &[mailbox_id], &[], None)
        .await
        .expect("setup_mdn_email: import_email must succeed");

    email_id
}

/// Create one Identity in `account_id` and return its Id.
///
/// # Panics
///
/// Panics on any backend error — this is test fixture setup code.
async fn setup_identity(backend: &MemoryBackend, account_id: &Id) -> Id {
    let identity = Identity::new(Id::from("placeholder"), "Jane_Sender@example.org", true);
    let (identity_id, _) = backend
        .create_object::<Identity>(account_id, "ident-mdn", identity)
        .await
        .expect("setup_identity: create_object must succeed");
    identity_id
}

// ---------------------------------------------------------------------------
// MDN/send tests
// ---------------------------------------------------------------------------

/// Test 1: MDN/send success path.
///
/// Oracle: draft-ietf-jmap-mdn-17 §3.1 example request.
/// The MDN is sent for an email that has a Disposition-Notification-To header.
/// On success:
/// - `sent["k1546"]` is present and non-null.
/// - `notSent` is null.
/// - One extra `Email/set` invocation is returned.
/// - After the call, fetching the email shows `$mdnsent: true`.
#[tokio::test]
async fn mdn_send_success() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let email_id = setup_mdn_email(&backend, &account_id).await;
    let identity_id = setup_identity(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "identityId": identity_id.as_ref(),
        "send": {
            "k1546": {
                "forEmailId": email_id.as_ref(),
                "subject": "Read receipt for: World domination",
                "textBody": "This receipt shows that the message has been displayed.",
                "reportingUA": "joes-pc.cs.example.com; Foomail 97.1",
                "disposition": {
                    "actionMode": "manual-action",
                    "sendingMode": "mdn-sent-manually",
                    "type": "displayed"
                }
            }
        },
        "onSuccessUpdateEmail": {
            "#k1546": { "keywords/$mdnsent": true }
        }
    });

    let (resp, extra) = handle_mdn_send(&backend, args, "call1")
        .await
        .expect("mdn_send_success: handle_mdn_send must succeed");

    // Oracle §3.1: sent map has the creation id; notSent is null.
    assert!(
        !resp["sent"]["k1546"].is_null(),
        "sent[k1546] must be present; resp: {resp}"
    );
    assert!(
        resp["notSent"].is_null(),
        "notSent must be null on full success; resp: {resp}"
    );

    // Oracle §3.1: server sets finalRecipient (MemoryBackend always returns
    // "rfc822; test@example.com" for test purposes).
    assert!(
        !resp["sent"]["k1546"]["finalRecipient"].is_null(),
        "finalRecipient must be set by server; resp: {resp}"
    );

    // Oracle draft §2.1: onSuccessUpdateEmail causes one extra Email/set invocation.
    assert_eq!(
        extra.len(),
        1,
        "exactly one extra invocation expected; got {extra:?}"
    );
    let (method_name, _, extra_call_id) = &extra[0];
    assert_eq!(
        method_name, "Email/set",
        "extra invocation must be Email/set"
    );
    assert_eq!(
        extra_call_id, "call1",
        "extra invocation must share call_id"
    );

    // Oracle: after onSuccessUpdateEmail, fetching the email shows $mdnsent: true.
    let (emails, not_found) = backend
        .get_objects::<jmap_mail_types::Email>(&account_id, Some(&[email_id.clone()]), None)
        .await
        .expect("get_objects must succeed");
    assert!(
        not_found.is_empty(),
        "email must still be found after mdn send"
    );
    let email = &emails[0];
    assert_eq!(
        email.keywords.get("$mdnsent"),
        Some(&true),
        "email must have $mdnsent: true after successful mdn send"
    );
}

/// Test 2: MDN/send is rejected per-entry when the email already has `$mdnsent`.
///
/// Oracle: draft-ietf-jmap-mdn-17 §2.1 — the server MUST NOT send a second MDN
/// for an email that already has the `$mdnsent` keyword.
/// Wire error type: `mdnAlreadySent` (spec §2.1).
#[tokio::test]
async fn mdn_send_already_sent() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let email_id = setup_mdn_email(&backend, &account_id).await;
    let identity_id = setup_identity(&backend, &account_id).await;

    // Pre-set $mdnsent on the email via Email/set so the handler sees it.
    backend
        .update_object::<jmap_mail_types::Email>(
            &account_id,
            &email_id,
            serde_json::json!({ "keywords/$mdnsent": true }),
        )
        .await
        .expect("pre-set $mdnsent must succeed");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "identityId": identity_id.as_ref(),
        "send": {
            "k1546": {
                "forEmailId": email_id.as_ref(),
                "subject": "Read receipt for: World domination",
                "textBody": "This receipt shows that the message has been displayed.",
                "disposition": {
                    "actionMode": "manual-action",
                    "sendingMode": "mdn-sent-manually",
                    "type": "displayed"
                }
            }
        },
        "onSuccessUpdateEmail": {
            "#k1546": { "keywords/$mdnsent": true }
        }
    });

    let (resp, extra) = handle_mdn_send(&backend, args, "call2")
        .await
        .expect("mdn_send_already_sent: handle_mdn_send must return Ok");

    // Oracle: per-entry error mdnAlreadySent; sent is null; no extra invocations.
    assert_eq!(
        resp["notSent"]["k1546"]["type"].as_str(),
        Some("mdnAlreadySent"),
        "notSent[k1546].type must be mdnAlreadySent; resp: {resp}"
    );
    assert!(
        resp["sent"].is_null(),
        "sent must be null when all entries fail; resp: {resp}"
    );
    assert!(
        extra.is_empty(),
        "no extra invocations when nothing was sent; extra: {extra:?}"
    );
}

/// Test 3: MDN/send with a non-existent `forEmailId`.
///
/// Oracle: draft-ietf-jmap-mdn-17 §2.1 — the server places a `notFound` SetError
/// in `notSent` for any entry whose referenced email does not exist.
#[tokio::test]
async fn mdn_send_email_not_found() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let identity_id = setup_identity(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "identityId": identity_id.as_ref(),
        "send": {
            "k1546": {
                "forEmailId": "nonexistent-id",
                "disposition": {
                    "actionMode": "manual-action",
                    "sendingMode": "mdn-sent-manually",
                    "type": "displayed"
                }
            }
        },
        "onSuccessUpdateEmail": {
            "#k1546": { "keywords/$mdnsent": true }
        }
    });

    let (resp, extra) = handle_mdn_send(&backend, args, "call3")
        .await
        .expect("mdn_send_email_not_found: handle_mdn_send must return Ok");

    // Oracle: notFound per-entry error; sent is null; no extra invocations.
    assert_eq!(
        resp["notSent"]["k1546"]["type"].as_str(),
        Some("notFound"),
        "notSent[k1546].type must be notFound; resp: {resp}"
    );
    assert!(
        resp["sent"].is_null(),
        "sent must be null when all entries fail; resp: {resp}"
    );
    assert!(
        extra.is_empty(),
        "no extra invocations when nothing was sent; extra: {extra:?}"
    );
}

/// Test 4: MDN/send where `onSuccessUpdateEmail` does not set `keywords/$mdnsent: true`.
///
/// Oracle: draft-ietf-jmap-mdn-17 §2.1 — the server MUST reject any MDN/send
/// where `onSuccessUpdateEmail` is present but does not stamp `$mdnsent: true`
/// for each entry in `send`.  The whole request is rejected with `invalidArguments`.
#[tokio::test]
async fn mdn_send_missing_mdnsent_patch() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let email_id = setup_mdn_email(&backend, &account_id).await;
    let identity_id = setup_identity(&backend, &account_id).await;

    // Patch changes subject but does NOT set keywords/$mdnsent.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "identityId": identity_id.as_ref(),
        "send": {
            "k1546": {
                "forEmailId": email_id.as_ref(),
                "disposition": {
                    "actionMode": "manual-action",
                    "sendingMode": "mdn-sent-manually",
                    "type": "displayed"
                }
            }
        },
        "onSuccessUpdateEmail": {
            "#k1546": { "subject": "changed" }
        }
    });

    let result = handle_mdn_send(&backend, args, "call4").await;

    // Oracle: the whole request must be rejected with an Err (invalidArguments).
    assert!(
        result.is_err(),
        "handle_mdn_send must return Err when onSuccessUpdateEmail lacks $mdnsent patch"
    );
    let err = result.unwrap_err();
    // The JmapError type should indicate invalidArguments.
    let err_str = format!("{err:?}");
    assert!(
        err_str.contains("invalidArguments") || err_str.contains("InvalidArguments"),
        "error must be invalidArguments; got: {err_str}"
    );
}

/// Test 5: MDN/send where `onSuccessUpdateEmail` is null and `send` is non-empty.
///
/// Oracle: draft-ietf-jmap-mdn-17 §2.1 — `onSuccessUpdateEmail` is required when
/// `send` is non-empty.  The whole request is rejected with `invalidArguments`.
#[tokio::test]
async fn mdn_send_null_on_success() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let email_id = setup_mdn_email(&backend, &account_id).await;
    let identity_id = setup_identity(&backend, &account_id).await;

    // JSON null for onSuccessUpdateEmail — deserializes as None in MdnSendRequest.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "identityId": identity_id.as_ref(),
        "send": {
            "k1546": {
                "forEmailId": email_id.as_ref(),
                "disposition": {
                    "actionMode": "manual-action",
                    "sendingMode": "mdn-sent-manually",
                    "type": "displayed"
                }
            }
        },
        "onSuccessUpdateEmail": null
    });

    let result = handle_mdn_send(&backend, args, "call5").await;

    // Oracle: the whole request must be rejected with Err (invalidArguments).
    assert!(
        result.is_err(),
        "handle_mdn_send must return Err when onSuccessUpdateEmail is null and send is non-empty"
    );
    let err = result.unwrap_err();
    let err_str = format!("{err:?}");
    assert!(
        err_str.contains("invalidArguments") || err_str.contains("InvalidArguments"),
        "error must be invalidArguments; got: {err_str}"
    );
}

// ---------------------------------------------------------------------------
// MDN/parse tests
// ---------------------------------------------------------------------------

/// Test 6: MDN/parse with a valid MDN blob.
///
/// Oracle: draft-ietf-jmap-mdn-17 §3.3 + RFC 8098 §9 example.
/// `VALID_MDN_BLOB` is the hand-written fixture from `common/mod.rs`, derived
/// from the RFC 8098 §9 example — it is the independent oracle.
///
/// The blob contains:
/// - `Disposition: manual-action/MDN-sent-manually; displayed`
/// - `Final-Recipient: rfc822;Joe_Recipient@example.com`
/// - `Reporting-UA: joes-pc.cs.example.com; Foomail 97.1`
#[tokio::test]
async fn mdn_parse_valid() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let blob_id = Id::from("blob-valid-mdn");
    backend.store_blob(&blob_id, VALID_MDN_BLOB.to_vec());

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobIds": [blob_id.as_ref()]
    });

    let (resp, extra) = handle_mdn_parse(&backend, args, MDN_PARSE_MAX_BLOB_IDS)
        .await
        .expect("mdn_parse_valid: handle_mdn_parse must succeed");

    // Oracle: exactly one parsed entry; no errors.
    assert!(
        !resp["parsed"][blob_id.as_ref()].is_null(),
        "parsed[blob_id] must be present; resp: {resp}"
    );
    assert!(
        resp["notParsable"].is_null(),
        "notParsable must be null for a valid blob; resp: {resp}"
    );
    assert!(
        resp["notFound"].is_null(),
        "notFound must be null for a stored blob; resp: {resp}"
    );

    // Oracle RFC 8098 §9 / draft §3.3: disposition fields from the fixture.
    // Fixture Disposition line: manual-action/MDN-sent-manually; displayed
    // MemoryBackend parse_mdns lowercases all values.
    let parsed_entry = &resp["parsed"][blob_id.as_ref()];
    assert_eq!(
        parsed_entry["disposition"]["actionMode"].as_str(),
        Some("manual-action"),
        "actionMode must be manual-action; entry: {parsed_entry}"
    );
    assert_eq!(
        parsed_entry["disposition"]["sendingMode"].as_str(),
        Some("mdn-sent-manually"),
        "sendingMode must be mdn-sent-manually; entry: {parsed_entry}"
    );
    assert_eq!(
        parsed_entry["disposition"]["type"].as_str(),
        Some("displayed"),
        "type must be displayed; entry: {parsed_entry}"
    );

    // Oracle RFC 8098 §9: Final-Recipient header is present in the fixture.
    assert!(
        !parsed_entry["finalRecipient"].is_null(),
        "finalRecipient must be set from blob; entry: {parsed_entry}"
    );

    // Oracle: MDN/parse has no side effects — no extra invocations.
    assert!(
        extra.is_empty(),
        "MDN/parse must produce no extra invocations"
    );
}

/// Test 7: MDN/parse with a non-existent blob ID.
///
/// Oracle: draft-ietf-jmap-mdn-17 §3.3 — blob IDs not found in the account
/// appear in `notFound`; `parsed` and `notParsable` are absent (null).
#[tokio::test]
async fn mdn_parse_not_found() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let missing_id = "nonexistent-blob-id";
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobIds": [missing_id]
    });

    let (resp, extra) = handle_mdn_parse(&backend, args, MDN_PARSE_MAX_BLOB_IDS)
        .await
        .expect("mdn_parse_not_found: handle_mdn_parse must succeed");

    // Oracle §3.3: notFound contains the id; other fields are null.
    let not_found = resp["notFound"]
        .as_array()
        .expect("notFound must be an array");
    assert!(
        not_found.iter().any(|v| v.as_str() == Some(missing_id)),
        "notFound must contain {missing_id}; resp: {resp}"
    );
    assert!(
        resp["parsed"].is_null(),
        "parsed must be null when nothing was parsed; resp: {resp}"
    );
    assert!(
        resp["notParsable"].is_null(),
        "notParsable must be null; resp: {resp}"
    );

    assert!(
        extra.is_empty(),
        "MDN/parse must produce no extra invocations"
    );
}

/// Test 8: MDN/parse with a blob that is not a valid MDN message.
///
/// Oracle: draft-ietf-jmap-mdn-17 §3.3 — blobs that cannot be parsed as MDN
/// appear in `notParsable`; `parsed` and `notFound` are absent (null).
///
/// `INVALID_MDN_BLOB` (from `common/mod.rs`) is a plain text string with no
/// `Disposition:` field, which the MemoryBackend heuristic classifies as
/// not parsable.
#[tokio::test]
async fn mdn_parse_not_parsable() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let blob_id = Id::from("blob-invalid-mdn");
    backend.store_blob(&blob_id, INVALID_MDN_BLOB.to_vec());

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobIds": [blob_id.as_ref()]
    });

    let (resp, extra) = handle_mdn_parse(&backend, args, MDN_PARSE_MAX_BLOB_IDS)
        .await
        .expect("mdn_parse_not_parsable: handle_mdn_parse must succeed");

    // Oracle §3.3: notParsable contains the id; other fields are null.
    let not_parsable = resp["notParsable"]
        .as_array()
        .expect("notParsable must be an array");
    assert!(
        not_parsable
            .iter()
            .any(|v| v.as_str() == Some(blob_id.as_ref())),
        "notParsable must contain {blob_id}; resp: {resp}"
    );
    assert!(
        resp["parsed"].is_null(),
        "parsed must be null when the blob is not parsable; resp: {resp}"
    );
    assert!(
        resp["notFound"].is_null(),
        "notFound must be null; resp: {resp}"
    );

    assert!(
        extra.is_empty(),
        "MDN/parse must produce no extra invocations"
    );
}

/// Test 9: MDN/send with a null/absent `forEmailId` produces per-entry
/// `invalidProperties`, not a whole-request error.
///
/// Oracle: draft-ietf-jmap-mdn-17 §2 — "forEmailId MUST NOT be null for MDN/send."
/// A null/absent forEmailId must produce per-entry invalidProperties, not a whole-request error.
#[tokio::test]
async fn mdn_send_null_for_email_id() {
    let backend = MemoryBackend::default();
    let account_id = Id::from("account1");

    // Identity must exist so the request passes the identity check and reaches
    // the per-entry forEmailId validation in step 4 of the handler.
    let identity_id = setup_identity(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "identityId": identity_id.as_ref(),
        "send": {
            "k1": {
                // forEmailId deliberately omitted → None after deserialization
                "disposition": {
                    "actionMode": "manual-action",
                    "sendingMode": "mdn-sent-manually",
                    "type": "displayed"
                }
            }
        },
        "onSuccessUpdateEmail": {
            "#k1": { "keywords/$mdnsent": true }
        }
    });
    let (resp, extra) = handle_mdn_send(&backend, args, "call1")
        .await
        .expect("null forEmailId should not cause a whole-request error");
    // Oracle: per-entry invalidProperties, not a request-level Err
    assert_eq!(
        resp["notSent"]["k1"]["type"], "invalidProperties",
        "null forEmailId must return invalidProperties per draft §2"
    );
    assert!(
        resp["sent"].is_null() || resp["sent"] == serde_json::json!(null),
        "sent must be null when all entries have errors"
    );
    assert!(
        extra.is_empty(),
        "no Email/set companion when nothing was sent"
    );
}
