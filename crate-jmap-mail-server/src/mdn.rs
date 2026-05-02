//! [`MdnBackend`] trait for `MDN/send` and `MDN/parse` operations (draft-ietf-jmap-mdn-17).
//!
//! This module is unconditionally compiled when the `mdn` feature is enabled on
//! `jmap-mail-server`. The feature gate lives in `lib.rs` (`#[cfg(feature = "mdn")]`),
//! not here — this file contains no `#[cfg(…)]` attributes.

/// Per-send-attempt result returned by [`MdnBackend::send_mdns`].
pub struct MdnSendResult {
    /// Successfully sent MDNs. Key = client creation ID. Value = [`jmap_mail_types::mdn::Mdn`]
    /// with server-set fields populated (finalRecipient, originalMessageId, etc.).
    pub sent: std::collections::HashMap<jmap_types::Id, jmap_mail_types::mdn::Mdn>,
    /// Failed send attempts. Key = client creation ID. Value = [`jmap_server::backend::SetError`].
    pub not_sent: std::collections::HashMap<jmap_types::Id, jmap_server::backend::SetError>,
}

/// Per-parse result returned by [`MdnBackend::parse_mdns`].
pub struct MdnParseResult {
    /// Successfully parsed MDN blobs. Key = blob ID. Value = [`jmap_mail_types::mdn::Mdn`].
    pub parsed: std::collections::HashMap<jmap_types::Id, jmap_mail_types::mdn::Mdn>,
    /// Blob IDs that were found but could not be parsed as an MDN.
    pub not_parsable: Vec<jmap_types::Id>,
    /// Blob IDs that were not found in the blob store.
    pub not_found: Vec<jmap_types::Id>,
}

/// Backend trait for `MDN/send` and `MDN/parse` operations.
///
/// Implementors also implement [`jmap_mail_server::MailBackend`] on the same
/// struct — the generic bounds on any future `register_mdn_handlers` helper
/// will require both. This separation keeps MDN opt-in: existing
/// `MailBackend` implementors do not need to change.
///
/// # Blob access
///
/// `MDN/parse` needs raw RFC 5322 bytes for each blob. Implementors are
/// expected to have direct access to the blob store (the same store used by
/// `MailBackend`). The required method [`get_blob_bytes`](MdnBackend::get_blob_bytes)
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
    /// For each entry in `send`:
    /// - Fetch the referenced email by `for_email_id`; place a `notFound`
    ///   [`SetError`](jmap_server::backend::SetError) in the result if the
    ///   email does not exist.
    /// - Verify the email has a `Disposition-Notification-To` header; if not,
    ///   place a `notFound` [`SetError`](jmap_server::backend::SetError)
    ///   (per draft §2.1).
    /// - Build and transmit the RFC 5322 MDN message.
    /// - Return server-set fields (`finalRecipient`, `originalMessageId`,
    ///   `mdnGateway`, `originalRecipient`, `error`) for sent entries.
    ///
    /// The caller (handler) performs the `$mdnsent` keyword stamp via
    /// `MailBackend::update_object` after this method returns — the backend
    /// does NOT stamp the keyword.
    ///
    /// Returns [`BackendSetError::Other`](jmap_server::backend::BackendSetError::Other)
    /// only for catastrophic storage failures; per-entry failures are reported
    /// inside [`MdnSendResult::not_sent`].
    fn send_mdns(
        &self,
        account_id: &jmap_types::Id,
        identity_id: &jmap_types::Id,
        send: std::collections::HashMap<jmap_types::Id, jmap_mail_types::mdn::Mdn>,
    ) -> impl std::future::Future<
        Output = Result<MdnSendResult, jmap_server::backend::BackendSetError<Self::Error>>,
    > + Send;

    /// Parse one or more raw RFC 5322 blobs as MDN messages.
    ///
    /// For each blob ID:
    /// - Fetch raw bytes via [`get_blob_bytes`](MdnBackend::get_blob_bytes).
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
