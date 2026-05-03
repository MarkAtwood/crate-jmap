//! [`MdnBackend`] trait for `MDN/send` and `MDN/parse` operations (draft-ietf-jmap-mdn-17).
//!
//! This module is unconditionally compiled when the `mdn` feature is enabled on
//! `jmap-mail-server`. The feature gate lives in `lib.rs` (`#[cfg(feature = "mdn")]`),
//! not here — this file contains no `#[cfg(…)]` attributes.

use std::collections::HashMap;

use jmap_mail_types::{
    mdn::{Mdn, MdnParseRequest, MdnSendRequest},
    Email, Identity,
};
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MailBackend, SetError, SetErrorType};
use crate::helpers::{find_immutable_patch_key, set_error_value};

/// Per-send-attempt result returned by [`MdnBackend::send_mdns`].
#[non_exhaustive]
#[derive(Debug)]
pub struct MdnSendResult {
    /// Successfully sent MDNs. Key = client creation ID. Value = [`Mdn`]
    /// with server-set fields populated (finalRecipient, originalMessageId, etc.).
    pub sent: HashMap<Id, Mdn>,
    /// Failed send attempts. Key = client creation ID. Value = [`SetError`].
    pub not_sent: HashMap<Id, SetError>,
}

impl MdnSendResult {
    /// Construct an `MdnSendResult`.
    ///
    /// Required because the struct is `#[non_exhaustive]` — external crates
    /// cannot use struct-literal syntax.
    pub fn new(sent: HashMap<Id, Mdn>, not_sent: HashMap<Id, SetError>) -> Self {
        Self { sent, not_sent }
    }
}

/// Per-parse result returned by [`MdnBackend::parse_mdns`].
#[non_exhaustive]
#[derive(Debug)]
pub struct MdnParseResult {
    /// Successfully parsed MDN blobs. Key = blob ID. Value = [`Mdn`].
    pub parsed: HashMap<Id, Mdn>,
    /// Blob IDs that were found but could not be parsed as an MDN.
    pub not_parsable: Vec<Id>,
    /// Blob IDs that were not found in the blob store.
    pub not_found: Vec<Id>,
}

impl MdnParseResult {
    /// Construct an `MdnParseResult`.
    ///
    /// Required because the struct is `#[non_exhaustive]` — external crates
    /// cannot use struct-literal syntax.
    pub fn new(parsed: HashMap<Id, Mdn>, not_parsable: Vec<Id>, not_found: Vec<Id>) -> Self {
        Self {
            parsed,
            not_parsable,
            not_found,
        }
    }
}

/// Backend trait for `MDN/send` and `MDN/parse` operations.
///
/// Implementors also implement [`MailBackend`] on the same struct — the
/// generic bounds on any future `register_mdn_handlers` helper will require
/// both. This separation keeps MDN opt-in: existing `MailBackend` implementors
/// do not need to change.
///
/// # Blob access
///
/// `MDN/parse` needs raw RFC 5322 bytes for each blob. Implementors are
/// expected to have direct access to the blob store (the same store used by
/// `MailBackend`). The required method [`MdnBackend::get_blob_bytes`]
/// exposes this access at the trait level so handlers need not assume a
/// concrete struct type.
pub trait MdnBackend: Send + Sync {
    /// The associated error type for storage-layer failures.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Fetch raw bytes for a blob by ID.
    ///
    /// Returns `Ok(Some(bytes))` if found, `Ok(None)` if the blob does not
    /// exist in this account, and `Err` for storage failures.
    fn get_blob_bytes(
        &self,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;

    /// Send one or more MDNs.
    ///
    /// The caller ([`handle_mdn_send`]) guarantees that every entry in `send`
    /// has `for_email_id = Some(…)` — entries with `None` are rejected before
    /// this method is called.
    ///
    /// For each entry in `send`:
    /// - Fetch the referenced email by `for_email_id`; place a `notFound`
    ///   [`SetError`] in the result if the
    ///   email does not exist.
    /// - Verify the email has a `Disposition-Notification-To` header; if not,
    ///   place a `notFound` [`SetError`]
    ///   (per draft §2.1).
    /// - Build and transmit the RFC 5322 MDN message.
    /// - Return server-set fields (`finalRecipient`, `originalMessageId`,
    ///   `mdnGateway`, `originalRecipient`, `error`) for sent entries.
    ///
    /// The caller (handler) performs the `$mdnsent` keyword stamp via
    /// `MailBackend::update_object` after this method returns — the backend
    /// does NOT stamp the keyword.
    ///
    /// Returns [`BackendSetError::Other`]
    /// only for catastrophic storage failures; per-entry failures are reported
    /// inside [`MdnSendResult::not_sent`].
    fn send_mdns(
        &self,
        account_id: &jmap_types::Id,
        identity_id: &jmap_types::Id,
        send: HashMap<Id, Mdn>,
    ) -> impl std::future::Future<
        Output = Result<MdnSendResult, jmap_server::backend::BackendSetError<Self::Error>>,
    > + Send;

    /// Parse one or more raw RFC 5322 blobs as MDN messages.
    ///
    /// For each blob ID:
    /// - Fetch raw bytes via [`MdnBackend::get_blob_bytes`].
    /// - Attempt to parse as `multipart/report` containing a
    ///   `message/disposition-notification` part.
    /// - Normalize `actionMode`, `sendingMode`, and `type` to lowercase
    ///   (RFC 8098 is case-insensitive; draft §2 requires lowercase in JMAP).
    /// - Populate `forEmailId` by correlating the `Original-Message-ID`
    ///   against known sent mail in `account_id`; may be `None` if correlation
    ///   cannot be done or the message is unknown.
    ///
    /// Returns `Err` only for catastrophic storage failures.
    fn parse_mdns(
        &self,
        account_id: &jmap_types::Id,
        blob_ids: Vec<jmap_types::Id>,
    ) -> impl std::future::Future<Output = Result<MdnParseResult, Self::Error>> + Send;
}

// ---------------------------------------------------------------------------
// MDN/send handler
// ---------------------------------------------------------------------------

/// Handle an `MDN/send` method call (draft-ietf-jmap-mdn-17 §3.1).
///
/// Returns `(response_args, extra_invocations)`. When `onSuccessUpdateEmail` is
/// present and MDNs are sent successfully, `extra_invocations` will contain one
/// `Email/set` invocation using the same `call_id`.
pub async fn handle_mdn_send<B: MailBackend + MdnBackend>(
    backend: &B,
    args: serde_json::Value,
    call_id: &str,
) -> Result<(serde_json::Value, Vec<Invocation>), JmapError> {
    // Step 1: Parse request.
    let req: MdnSendRequest = serde_json::from_value(args).map_err(|e| {
        JmapError::invalid_arguments(format!("failed to parse MDN/send arguments: {e}"))
    })?;

    // Step 2: Validate identityId — fetch identity, confirming both account and
    // identity existence in a single round-trip.  An unknown accountId produces
    // an empty `identities` list, which is caught by the identity-not-found check
    // below (RFC 8620 §5.1: unknown accountId → accountNotFound, but we surface
    // it as invalidArguments here per the spec for MDN/send).
    let (identities, _) = backend
        .get_objects::<Identity>(
            &req.account_id,
            Some(std::slice::from_ref(&req.identity_id)),
            None,
        )
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if identities.is_empty() {
        return Err(JmapError::invalid_arguments("identityId not found"));
    }

    // Step 3: Validate onSuccessUpdateEmail — per draft §3.1 the server MUST
    // reject any MDN/send where onSuccessUpdateEmail does not result in setting
    // keywords/$mdnsent: true for each entry in send.
    if !req.send.is_empty() {
        match &req.on_success_update_email {
            None => {
                return Err(JmapError::invalid_arguments(
                    "onSuccessUpdateEmail is required and must set keywords/$mdnsent: true for each send entry",
                ));
            }
            Some(patches) => {
                for creation_id in req.send.keys() {
                    let key = format!("#{creation_id}");
                    match patches.get(&key) {
                        None => {
                            return Err(JmapError::invalid_arguments(
                                "onSuccessUpdateEmail is required and must set keywords/$mdnsent: true for each send entry",
                            ));
                        }
                        Some(patch) => {
                            // The patch must contain "keywords/$mdnsent": true.
                            let sets_mdnsent = patch
                                .get("keywords/$mdnsent")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if !sets_mdnsent {
                                return Err(JmapError::invalid_arguments(
                                    "onSuccessUpdateEmail is required and must set keywords/$mdnsent: true for each send entry",
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 4: CRLF validation — per-entry, add to notSent rather than rejecting
    // the whole request.
    let mut not_sent: HashMap<String, Value> = HashMap::new();
    let mut send_map: HashMap<Id, Mdn> = HashMap::new();

    for (creation_id, mdn) in req.send {
        if mdn.for_email_id.is_none() {
            not_sent.insert(
                creation_id.clone(),
                serde_json::json!({"type": "invalidProperties", "properties": ["forEmailId"],
                                   "description": "forEmailId MUST NOT be null for MDN/send (draft §2)"}),
            );
            continue;
        }

        let mut crlf_bad = false;

        if let Some(ref s) = mdn.subject {
            if !crate::submission::check_no_crlf(s) {
                crlf_bad = true;
            }
        }
        if !crlf_bad {
            if let Some(ref s) = mdn.text_body {
                if !crate::submission::check_no_crlf(s) {
                    crlf_bad = true;
                }
            }
        }
        if !crlf_bad {
            if let Some(ref s) = mdn.reporting_ua {
                if !crate::submission::check_no_crlf(s) {
                    crlf_bad = true;
                }
            }
        }
        if !crlf_bad {
            if let Some(ref s) = mdn.final_recipient {
                if !crate::submission::check_no_crlf(s) {
                    crlf_bad = true;
                }
            }
        }
        if !crlf_bad {
            if let Some(ref fields) = mdn.extension_fields {
                for (k, v) in fields {
                    if !crate::submission::check_no_crlf(k) || !crate::submission::check_no_crlf(v)
                    {
                        crlf_bad = true;
                        break;
                    }
                }
            }
        }
        // mdn_gateway, original_message_id, original_recipient, and error
        // items are "server-set" in spec §2, but the Mdn struct is shared
        // between send and parse: a client CAN include them. Any client-supplied
        // value that ends up in a generated RFC 5322 header must be CRLF-clean
        // to prevent header injection (e.g. mdn_gateway → MDN-Gateway: header,
        // original_recipient → Original-Recipient:, error items → Error: headers).
        if !crlf_bad {
            if let Some(ref s) = mdn.mdn_gateway {
                if !crate::submission::check_no_crlf(s) {
                    crlf_bad = true;
                }
            }
        }
        if !crlf_bad {
            if let Some(ref s) = mdn.original_message_id {
                if !crate::submission::check_no_crlf(s) {
                    crlf_bad = true;
                }
            }
        }
        if !crlf_bad {
            if let Some(ref s) = mdn.original_recipient {
                if !crate::submission::check_no_crlf(s) {
                    crlf_bad = true;
                }
            }
        }
        if !crlf_bad {
            if let Some(ref errors) = mdn.error {
                for e in errors {
                    if !crate::submission::check_no_crlf(e) {
                        crlf_bad = true;
                        break;
                    }
                }
            }
        }

        if crlf_bad {
            not_sent.insert(
                creation_id,
                set_error_value(
                    &SetError::new(SetErrorType::InvalidProperties)
                        .with_description("CR or LF in MDN field"),
                ),
            );
        } else {
            send_map.insert(Id::from(creation_id.as_str()), mdn);
        }
    }

    // Step 5: Pre-check $mdnsent keyword — batch-fetch all referenced emails in
    // one round-trip instead of one call per entry, then check keywords locally.
    let email_ids: Vec<Id> = send_map
        .values()
        .filter_map(|mdn| mdn.for_email_id.clone())
        .collect();

    let emails_by_id: HashMap<Id, Email> = if email_ids.is_empty() {
        HashMap::new()
    } else {
        let (fetched, _) = backend
            .get_objects::<Email>(&req.account_id, Some(&email_ids), None)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;
        fetched.into_iter().map(|e| (e.id.clone(), e)).collect()
    };

    let mut remaining_send: HashMap<Id, Mdn> = HashMap::new();
    for (creation_id, mdn) in send_map {
        if let Some(ref for_email_id) = mdn.for_email_id {
            if let Some(email) = emails_by_id.get(for_email_id) {
                if email.keywords.get("$mdnsent") == Some(&true) {
                    not_sent.insert(
                        creation_id.as_ref().to_owned(),
                        set_error_value(&SetError::new(SetErrorType::MdnAlreadySent)),
                    );
                    continue;
                }
            }
        }
        remaining_send.insert(creation_id, mdn);
    }

    // Step 6: Call backend.send_mdns.
    let mut sent_mdns: HashMap<String, Mdn> = HashMap::new();

    if !remaining_send.is_empty() {
        let result = backend
            .send_mdns(&req.account_id, &req.identity_id, remaining_send)
            .await
            .map_err(|e| match e {
                BackendSetError::Other(inner) => JmapError::server_fail(inner.to_string()),
                BackendSetError::SetError(se) => JmapError::server_fail(se.to_string()),
            })?;

        for (id, se) in result.not_sent {
            not_sent.insert(id.as_ref().to_owned(), set_error_value(&se));
        }
        for (id, mdn) in result.sent {
            sent_mdns.insert(id.as_ref().to_owned(), mdn);
        }
    }

    // Step 7: Apply onSuccessUpdateEmail for each successfully sent entry.
    let mut extra_invocations: Vec<Invocation> = Vec::new();

    if let Some(ref patches) = req.on_success_update_email {
        if !sent_mdns.is_empty() {
            let email_old_state = backend
                .get_state::<Email>(&req.account_id)
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;

            let mut email_updated: serde_json::Map<String, Value> = serde_json::Map::new();
            let mut email_not_updated: serde_json::Map<String, Value> = serde_json::Map::new();

            for (creation_id_str, sent_mdn) in &sent_mdns {
                let patch_key = format!("#{creation_id_str}");
                let patch = match patches.get(&patch_key) {
                    Some(p) => p,
                    None => continue,
                };

                // Resolve the email ID from the sent MDN's forEmailId.
                let email_id = match sent_mdn.for_email_id.as_ref() {
                    Some(id) => id.clone(),
                    None => continue,
                };

                // Apply the same immutable-field guard as handle_email_set patches.
                if let Some(bad_field) = find_immutable_patch_key(patch) {
                    email_not_updated.insert(
                        email_id.as_ref().to_owned(),
                        json!({
                            "type": "invalidProperties",
                            "properties": [bad_field],
                        }),
                    );
                    continue;
                }

                match backend
                    .update_object::<Email>(&req.account_id, &email_id, patch.clone())
                    .await
                {
                    Ok(Some(obj)) => {
                        email_updated.insert(
                            email_id.as_ref().to_owned(),
                            serde_json::to_value(&obj).unwrap_or_else(
                                |e| json!({ "type": "serverFail", "description": e.to_string() }),
                            ),
                        );
                    }
                    Ok(None) => {
                        email_updated.insert(email_id.as_ref().to_owned(), Value::Null);
                    }
                    Err(BackendSetError::SetError(se)) => {
                        email_not_updated
                            .insert(email_id.as_ref().to_owned(), set_error_value(&se));
                    }
                    Err(BackendSetError::Other(e)) => {
                        email_not_updated.insert(
                            email_id.as_ref().to_owned(),
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                    }
                }
            }

            let any_email_ops = !email_updated.is_empty() || !email_not_updated.is_empty();
            if any_email_ops {
                let email_new_state = backend
                    .get_state::<Email>(&req.account_id)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;

                let email_set_resp = json!({
                    "accountId": req.account_id.as_ref(),
                    "oldState": email_old_state.as_ref(),
                    "newState": email_new_state.as_ref(),
                    "created": Value::Null,
                    "updated": if email_updated.is_empty() { Value::Null } else { Value::Object(email_updated) },
                    "destroyed": Value::Null,
                    "notCreated": Value::Null,
                    "notUpdated": if email_not_updated.is_empty() { Value::Null } else { Value::Object(email_not_updated) },
                    "notDestroyed": Value::Null,
                });
                extra_invocations.push((
                    "Email/set".to_owned(),
                    email_set_resp,
                    call_id.to_owned(),
                ));
            }
        }
    }

    // Step 8: Build MDN/send response.
    let sent_value: Value = if sent_mdns.is_empty() {
        Value::Null
    } else {
        serde_json::to_value(&sent_mdns).map_err(|e| JmapError::server_fail(e.to_string()))?
    };
    let not_sent_value: Value = if not_sent.is_empty() {
        Value::Null
    } else {
        Value::Object(not_sent.into_iter().collect())
    };

    let resp = json!({
        "accountId": req.account_id.as_ref(),
        "sent": sent_value,
        "notSent": not_sent_value,
    });

    Ok((resp, extra_invocations))
}

// ---------------------------------------------------------------------------
// MDN/parse handler
// ---------------------------------------------------------------------------

/// Default maximum blob IDs for a single `MDN/parse` request.
///
/// Pass this as `max_blob_ids` to [`handle_mdn_parse`] unless your deployment
/// has a specific policy.  16 is a conservative default matching typical email
/// client batch sizes; the draft spec mandates no limit.
pub const MDN_PARSE_MAX_BLOB_IDS: usize = 16;

/// Handle an `MDN/parse` method call (draft-ietf-jmap-mdn-17 §3.3).
///
/// `max_blob_ids` caps the number of blob IDs accepted in a single request.
/// Use [`MDN_PARSE_MAX_BLOB_IDS`] for the default.  Exceeding the limit
/// returns a method-level `requestTooLarge` error (RFC 8620 §5.1).
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are always
/// empty — `MDN/parse` is a read-only operation with no side effects.
pub async fn handle_mdn_parse<B: MailBackend + MdnBackend>(
    backend: &B,
    args: Value,
    max_blob_ids: usize,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // Step 1: deserialize the full request structure.
    let req: MdnParseRequest = serde_json::from_value(args).map_err(|e| {
        JmapError::invalid_arguments(format!("failed to parse MDN/parse arguments: {e}"))
    })?;

    // Step 2: `accountId` presence is enforced by MdnParseRequest deserialization
    // above — a missing field causes Step 1 to fail.  No separate backend
    // account-existence check exists in the MailBackend trait; other handlers
    // follow the same pattern and let storage errors surface naturally.

    // Step 3: enforce per-request blob count limit.
    if req.blob_ids.len() > max_blob_ids {
        return Err(JmapError::request_too_large());
    }

    // Step 4: delegate to the backend.
    let result = backend
        .parse_mdns(&req.account_id, req.blob_ids)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Step 5: build the response — omit each collection key when empty per spec §3.3.
    // We build JSON directly rather than constructing MdnParseResponse (which is
    // #[non_exhaustive] and therefore cannot be constructed outside its defining crate).
    let parsed_value: Value = if result.parsed.is_empty() {
        Value::Null
    } else {
        serde_json::to_value(&result.parsed).map_err(|e| JmapError::server_fail(e.to_string()))?
    };
    let not_parsable_value: Value = if result.not_parsable.is_empty() {
        Value::Null
    } else {
        serde_json::to_value(&result.not_parsable)
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    };
    let not_found_value: Value = if result.not_found.is_empty() {
        Value::Null
    } else {
        serde_json::to_value(&result.not_found)
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    };

    let response_json = json!({
        "accountId": req.account_id.as_ref(),
        "parsed": parsed_value,
        "notParsable": not_parsable_value,
        "notFound": not_found_value,
    });

    // Step 6: return with no extra invocations.
    Ok((response_json, vec![]))
}
