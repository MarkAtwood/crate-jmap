//! MailBackend trait and supporting types for RFC 8621 method handlers.
//!
//! Consumers implement [`MailBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations and mail-specific operations are here.
//!
//! Marker traits and property selector enums live in `jmap-types` and
//! `jmap-mail-types` respectively; they are re-exported here for convenience.

pub use jmap_mail_types::backend::{
    EmailProperty, EmailSubmissionProperty, IdentityProperty, MailboxProperty,
    SearchSnippetProperty, ThreadProperty, VacationResponseProperty,
};
pub use jmap_server::{
    AddedItem, BackendChangesError, ChangesResult, GetObject, JmapBackend, JmapObject,
    QueryChangesResult, QueryObject, QueryResult, SetObject,
};

// ---------------------------------------------------------------------------
// SetError and SetErrorType
// ---------------------------------------------------------------------------

/// A per-item error in a `/set` response (`notCreated`, `notUpdated`,
/// `notDestroyed` maps) (RFC 8620 §5.3).
///
/// Construct with [`SetError::new`] and chain the builder methods as needed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetError {
    /// The machine-readable error type.
    #[serde(rename = "type")]
    pub error_type: SetErrorType,
    /// Optional human-readable description of the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Property names that caused the error (for `invalidProperties`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<String>>,
    /// The existing object id (for `alreadyExists` — RFC 8621 §5.7).
    #[serde(rename = "existingId", skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<jmap_types::Id>,
    /// Maximum recipients allowed (for `tooManyRecipients` — RFC 8621 §7.5).
    #[serde(rename = "maxRecipients", skip_serializing_if = "Option::is_none")]
    pub max_recipients: Option<u64>,
    /// Invalid recipient addresses (for `invalidRecipients` — RFC 8621 §7.5).
    #[serde(rename = "invalidRecipients", skip_serializing_if = "Option::is_none")]
    pub invalid_recipients: Option<Vec<String>>,
    /// Missing blob IDs (for `blobNotFound` — RFC 8621 §5.5).
    #[serde(rename = "notFound", skip_serializing_if = "Option::is_none")]
    pub not_found: Option<Vec<jmap_types::Id>>,
    /// Maximum message size in octets (for `tooLarge` on EmailSubmission — RFC 8621 §7.5).
    #[serde(rename = "maxSize", skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,
}

impl SetError {
    /// Construct a [`SetError`] with the given type and all optional fields `None`.
    pub fn new(error_type: SetErrorType) -> Self {
        Self {
            error_type,
            description: None,
            properties: None,
            existing_id: None,
            max_recipients: None,
            invalid_recipients: None,
            not_found: None,
            max_size: None,
        }
    }

    /// Set the human-readable description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the list of property names that caused the error.
    pub fn with_properties<I, S>(mut self, props: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.properties = Some(props.into_iter().map(|s| s.into()).collect());
        self
    }

    /// Set the existing object id (used with `alreadyExists`).
    pub fn with_existing_id(mut self, id: jmap_types::Id) -> Self {
        self.existing_id = Some(id);
        self
    }

    /// Set the maximum recipients (used with `tooManyRecipients` — RFC 8621 §7.5).
    pub fn with_max_recipients(mut self, n: u64) -> Self {
        self.max_recipients = Some(n);
        self
    }

    /// Set the invalid recipient addresses (used with `invalidRecipients` — RFC 8621 §7.5).
    pub fn with_invalid_recipients<I, S>(mut self, addrs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.invalid_recipients = Some(addrs.into_iter().map(|s| s.into()).collect());
        self
    }

    /// Set the missing blob IDs (used with `blobNotFound` — RFC 8621 §5.5).
    pub fn with_not_found(mut self, ids: Vec<jmap_types::Id>) -> Self {
        self.not_found = Some(ids);
        self
    }

    /// Set the maximum message size in octets (used with `tooLarge` on EmailSubmission — RFC 8621 §7.5).
    pub fn with_max_size(mut self, n: u64) -> Self {
        self.max_size = Some(n);
        self
    }
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error_type)?;
        if let Some(ref desc) = self.description {
            write!(f, ": {desc}")?;
        }
        Ok(())
    }
}

/// The machine-readable type for a [`SetError`] (RFC 8620 §5.3 and RFC 8621).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SetErrorType {
    /// The action would violate an ACL or other access control policy.
    Forbidden,
    /// Creating or modifying the object would exceed a server quota.
    OverQuota,
    /// The object is too large to be stored by the server.
    TooLarge,
    /// The server is rate-limiting this client.
    RateLimit,
    /// The object to be updated or destroyed does not exist.
    NotFound,
    /// The patch object is not a valid JSON Merge Patch or cannot be applied.
    InvalidPatch,
    /// The client requested destruction of an object that will be destroyed
    /// implicitly when another object is destroyed.
    WillDestroy,
    /// One or more properties have invalid values.
    InvalidProperties,
    /// The object type is a singleton and cannot be created or destroyed.
    Singleton,
    /// RFC 8621 §2.5 — Mailbox has child mailboxes and cannot be destroyed.
    MailboxHasChild,
    /// RFC 8621 §2.5 — Mailbox contains emails and `onDestroyRemoveEmails` is false.
    MailboxHasEmail,
    /// RFC 8621 §5.7 — An email with the same Message-ID already exists.
    AlreadyExists,
    /// RFC 8621 §5.5 — Too many keywords on the Email.
    TooManyKeywords,
    /// RFC 8621 §5.5 — Email is in too many mailboxes.
    TooManyMailboxes,
    /// RFC 8621 §5.5 — A referenced blob was not found.
    BlobNotFound,
    /// RFC 8621 §6.3 — The `from` address is not permitted for this Identity.
    ForbiddenFrom,
    /// RFC 8621 §7.5 — The Email is invalid for submission.
    InvalidEmail,
    /// RFC 8621 §7.5 — Too many recipients.
    TooManyRecipients,
    /// RFC 8621 §7.5 — No recipients specified.
    NoRecipients,
    /// RFC 8621 §7.5 — One or more recipient addresses are invalid.
    InvalidRecipients,
    /// RFC 8621 §7.5 — The MAIL FROM address is not permitted.
    ForbiddenMailFrom,
    /// RFC 8621 §7.5 — The user does not have send permission.
    ForbiddenToSend,
    /// RFC 8621 §7.5 — The submission cannot be undone.
    CannotUnsend,
}

impl std::fmt::Display for SetErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `SetErrorType` is defined in this crate. Within the defining crate,
        // `#[non_exhaustive]` does NOT prevent exhaustive matching, so this
        // match is truly exhaustive — adding a variant without updating it is
        // a compile error.
        let s = match self {
            Self::Forbidden => "forbidden",
            Self::OverQuota => "overQuota",
            Self::TooLarge => "tooLarge",
            Self::RateLimit => "rateLimit",
            Self::NotFound => "notFound",
            Self::InvalidPatch => "invalidPatch",
            Self::WillDestroy => "willDestroy",
            Self::InvalidProperties => "invalidProperties",
            Self::Singleton => "singleton",
            Self::MailboxHasChild => "mailboxHasChild",
            Self::MailboxHasEmail => "mailboxHasEmail",
            Self::AlreadyExists => "alreadyExists",
            Self::TooManyKeywords => "tooManyKeywords",
            Self::TooManyMailboxes => "tooManyMailboxes",
            Self::BlobNotFound => "blobNotFound",
            Self::ForbiddenFrom => "forbiddenFrom",
            Self::InvalidEmail => "invalidEmail",
            Self::TooManyRecipients => "tooManyRecipients",
            Self::NoRecipients => "noRecipients",
            Self::InvalidRecipients => "invalidRecipients",
            Self::ForbiddenMailFrom => "forbiddenMailFrom",
            Self::ForbiddenToSend => "forbiddenToSend",
            Self::CannotUnsend => "cannotUnsend",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// BackendSetError
// ---------------------------------------------------------------------------

/// Error type returned by create/update/destroy backend methods.
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendSetError<E> {
    /// A well-typed JMAP [`SetError`] to place verbatim in the
    /// `notCreated`/`notUpdated`/`notDestroyed` map.
    SetError(SetError),
    /// An unexpected storage-layer error.
    Other(E),
}

impl<E: std::fmt::Display> std::fmt::Display for BackendSetError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetError(se) => write!(f, "set error: {}", se.error_type),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BackendSetError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(e) => Some(e),
            _ => None,
        }
    }
}

impl<E> From<E> for BackendSetError<E> {
    fn from(e: E) -> Self {
        Self::Other(e)
    }
}

// ---------------------------------------------------------------------------
// MailBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for RFC 8621 JMAP Mail method handlers.
///
/// Implementors provide the actual data access; the method handler modules
/// in this crate translate between JMAP wire protocol and backend calls.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are inherited from [`JmapBackend`].
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl MailBackend>` when sharing across tasks.
pub trait MailBackend: JmapBackend {
    // -----------------------------------------------------------------------
    // Write operations
    // -----------------------------------------------------------------------

    /// Create a new object.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply a partial update (patch) to an existing object.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    ///
    /// **Callers must handle the `Some` case.** When the return value is
    /// `Some(O)`, the handler should serialize the updated object and include
    /// the server-modified fields in the `updated` map of the `/set` response
    /// (RFC 8620 §5.3). Discarding the return value causes server-modified
    /// fields to be silently omitted from the response. To use per-request
    /// auth context in an update handler, implement [`jmap_server::JmapHandler`] directly
    /// rather than using `register_mail_handlers`.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an existing object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    // -----------------------------------------------------------------------
    // Mail-specific methods
    // -----------------------------------------------------------------------

    /// Import a raw message blob as an Email (RFC 8621 §5.7).
    ///
    /// The blob must already be stored (uploaded via JMAP blob upload). Returns
    /// the assigned id and the created Email object.
    fn import_email(
        &self,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
        mailbox_ids: &[jmap_types::Id],
        keywords: &[jmap_mail_types::Keyword],
        received_at: Option<&jmap_types::UTCDate>,
    ) -> impl std::future::Future<
        Output = Result<(jmap_types::Id, jmap_mail_types::Email), BackendSetError<Self::Error>>,
    > + Send;

    /// Return the thread id of the first stored [`Email`](jmap_mail_types::Email) whose
    /// `messageId` list intersects `message_ids`, or `None` if no match exists.
    ///
    /// **Persistent backends MUST override this method.** The default `next_id`
    /// generator used when this returns `None` is seeded from system-clock
    /// nanoseconds at process startup. Two processes that start within the same
    /// nanosecond (common in containers and test harnesses) will produce
    /// identical ID sequences, silently corrupting thread graphs across
    /// restarts. A persistent backend must derive thread IDs from durable
    /// storage — for example, by looking up a content-addressed hash of the
    /// message-id header — so that thread identity survives process boundaries.
    fn find_thread_by_message_ids(
        &self,
        account_id: &jmap_types::Id,
        message_ids: &[&str],
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send;

    /// Return `true` if `blob_id` exists in `account_id`'s blob store.
    ///
    /// Used by `Email/parse` to distinguish `notFound` (blob absent) from
    /// `notParsable` (blob present but uninterpretable as a message).
    ///
    /// **Override this in every backend that stores blobs.** The default
    /// returns `false`, so every blob lookup will be reported as `notFound`.
    /// The default exists only to satisfy the trait bound for backends that
    /// do not implement blob storage; it does not represent a meaningful
    /// runtime value.
    fn blob_exists(
        &self,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = bool> + Send {
        let _ = (account_id, blob_id);
        std::future::ready(false)
    }

    /// Parse a raw message blob and return an Email object without storing it
    /// (RFC 8621 §5.8 — `Email/parse`).
    fn parse_email(
        &self,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<jmap_mail_types::Email, Self::Error>> + Send;

    /// Copy an Email from one account to another (RFC 8620 §6.3).
    ///
    /// Returns the new id and the created Email in `to_account_id`.
    fn copy_email(
        &self,
        from_account_id: &jmap_types::Id,
        email_id: &jmap_types::Id,
        to_account_id: &jmap_types::Id,
        mailbox_ids: &[jmap_types::Id],
        keywords: &[jmap_mail_types::Keyword],
        received_at: Option<&jmap_types::UTCDate>,
    ) -> impl std::future::Future<
        Output = Result<(jmap_types::Id, jmap_mail_types::Email), BackendSetError<Self::Error>>,
    > + Send;

    /// Return search snippets for the given Email ids (RFC 8621 §5.9 — `SearchSnippet/get`).
    fn search_snippets(
        &self,
        account_id: &jmap_types::Id,
        email_ids: &[jmap_types::Id],
        filter: Option<&jmap_mail_types::EmailFilterCondition>,
    ) -> impl std::future::Future<Output = Result<Vec<jmap_mail_types::SearchSnippet>, Self::Error>> + Send;

    /// Return `true` if this backend supports the given JMAP object type.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Maximum bytes of body value text to return per `EmailBodyPart`.
    ///
    /// A value of `0` means unlimited. Used with `maxBodyValueBytes` in
    /// `Email/get` and `Email/parse`. Override in your implementation to
    /// enforce per-account limits.
    fn max_body_value_bytes(&self, _account_id: &jmap_types::Id) -> u64 {
        0 // unlimited by default
    }

    /// Maximum seconds in the future that `sendAt` may be in an `EmailSubmission`.
    ///
    /// A value of `0` means no delayed send support. Used to validate `sendAt`
    /// in `EmailSubmission/set`. Override in your implementation to advertise
    /// this server capability.
    fn max_delayed_send_seconds(&self, _account_id: &jmap_types::Id) -> u64 {
        0 // no delayed send by default
    }

    /// Return `true` if this backend can compute `Mailbox/queryChanges` for
    /// the given account (RFC 8620 §5.6 — `canCalculateChanges`).
    ///
    /// The default is `false` because the in-process query filter in
    /// `handle_mailbox_query` cannot guarantee that the backend tracks
    /// per-query result sets. Override to `true` only if the backend
    /// maintains a stable, query-result-aware change log for Mailbox objects.
    fn can_calculate_mailbox_query_changes(&self, _account_id: &jmap_types::Id) -> bool {
        false
    }
}
