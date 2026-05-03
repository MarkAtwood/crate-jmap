//! Shared backend infrastructure for all JMAP server crates.
//!
//! Re-exports the marker traits from `jmap-types` and adds the result types,
//! `BackendChangesError`, and [`JmapBackend`] supertrait. Domain crates add
//! their write-side methods and domain-specific error variants on top.

pub use jmap_types::{GetObject, JmapObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// SetError — RFC 8620 §5.3 per-object set-method error
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
///
/// Extension crates define their own error strings via [`SetErrorType::custom`]
/// rather than adding variants here. This keeps the base crate stable as new
/// JMAP extension crates (calendar, contacts, etc.) are added.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
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
    /// An object with the same unique key already exists.
    AlreadyExists,
    /// RFC 8621 §2.5 — Mailbox has child mailboxes and cannot be destroyed.
    MailboxHasChild,
    /// RFC 8621 §2.5 — Mailbox contains emails and `onDestroyRemoveEmails` is false.
    MailboxHasEmail,
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
    /// An extension-defined error type not covered by the variants above.
    /// Serializes as the inner string directly (e.g. `"mdnAlreadySent"`).
    Custom(String),
}

impl SetErrorType {
    /// Construct a [`SetErrorType::Custom`] from any string.
    ///
    /// Use this in extension crates to emit domain-specific error types
    /// without adding variants to this enum.
    pub fn custom(s: impl Into<String>) -> Self {
        Self::Custom(s.into())
    }
}

impl std::fmt::Display for SetErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &str = match self {
            Self::Forbidden => "forbidden",
            Self::OverQuota => "overQuota",
            Self::TooLarge => "tooLarge",
            Self::RateLimit => "rateLimit",
            Self::NotFound => "notFound",
            Self::InvalidPatch => "invalidPatch",
            Self::WillDestroy => "willDestroy",
            Self::InvalidProperties => "invalidProperties",
            Self::Singleton => "singleton",
            Self::AlreadyExists => "alreadyExists",
            Self::MailboxHasChild => "mailboxHasChild",
            Self::MailboxHasEmail => "mailboxHasEmail",
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
            Self::Custom(s) => s.as_str(),
        };
        f.write_str(s)
    }
}

impl serde::Serialize for SetErrorType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for SetErrorType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = SetErrorType;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JMAP SetError type string")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(match v {
                    "forbidden" => SetErrorType::Forbidden,
                    "overQuota" => SetErrorType::OverQuota,
                    "tooLarge" => SetErrorType::TooLarge,
                    "rateLimit" => SetErrorType::RateLimit,
                    "notFound" => SetErrorType::NotFound,
                    "invalidPatch" => SetErrorType::InvalidPatch,
                    "willDestroy" => SetErrorType::WillDestroy,
                    "invalidProperties" => SetErrorType::InvalidProperties,
                    "singleton" => SetErrorType::Singleton,
                    "alreadyExists" => SetErrorType::AlreadyExists,
                    "mailboxHasChild" => SetErrorType::MailboxHasChild,
                    "mailboxHasEmail" => SetErrorType::MailboxHasEmail,
                    "tooManyKeywords" => SetErrorType::TooManyKeywords,
                    "tooManyMailboxes" => SetErrorType::TooManyMailboxes,
                    "blobNotFound" => SetErrorType::BlobNotFound,
                    "forbiddenFrom" => SetErrorType::ForbiddenFrom,
                    "invalidEmail" => SetErrorType::InvalidEmail,
                    "tooManyRecipients" => SetErrorType::TooManyRecipients,
                    "noRecipients" => SetErrorType::NoRecipients,
                    "invalidRecipients" => SetErrorType::InvalidRecipients,
                    "forbiddenMailFrom" => SetErrorType::ForbiddenMailFrom,
                    "forbiddenToSend" => SetErrorType::ForbiddenToSend,
                    "cannotUnsend" => SetErrorType::CannotUnsend,
                    other => SetErrorType::Custom(other.to_owned()),
                })
            }
        }
        d.deserialize_str(Visitor)
    }
}

/// Error type returned by create/update/destroy backend methods.
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
            Self::SetError(se) => write!(f, "set error: {se}"),
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

impl<E> From<SetError> for BackendSetError<E> {
    fn from(e: SetError) -> Self {
        Self::SetError(e)
    }
}

// ---------------------------------------------------------------------------
// Backend error envelopes
// ---------------------------------------------------------------------------

/// Error type returned by [`JmapBackend::get_changes`] and
/// [`JmapBackend::query_changes`].
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendChangesError<E> {
    /// The `sinceState` is too old or the server cannot calculate the full set
    /// of intermediate states. Maps to `tooManyChanges` in the response with
    /// the given suggested limit. Use `limit: 0` for `cannotCalculateChanges`.
    TooManyChanges { limit: u64 },
    /// An unexpected storage-layer error.
    Other(E),
}

impl<E: std::fmt::Display> std::fmt::Display for BackendChangesError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyChanges { limit: 0 } => write!(f, "cannot calculate changes"),
            Self::TooManyChanges { limit } => write!(f, "too many changes (limit: {limit})"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BackendChangesError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(e) => Some(e),
            _ => None,
        }
    }
}

impl<E> From<E> for BackendChangesError<E> {
    fn from(e: E) -> Self {
        Self::Other(e)
    }
}

impl<E: std::error::Error> From<BackendChangesError<E>> for jmap_types::JmapError {
    fn from(e: BackendChangesError<E>) -> Self {
        match e {
            BackendChangesError::TooManyChanges { limit: 0 } => {
                jmap_types::JmapError::cannot_calculate_changes()
            }
            BackendChangesError::TooManyChanges { limit } => {
                jmap_types::JmapError::too_many_changes_with_limit(limit)
            }
            BackendChangesError::Other(inner) => {
                jmap_types::JmapError::server_fail(inner.to_string())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of a `/changes` call (RFC 8620 §5.2).
#[derive(Debug)]
#[non_exhaustive]
pub struct ChangesResult {
    /// Ids of objects that were created since `sinceState`.
    pub created: Vec<jmap_types::Id>,
    /// Ids of objects that were updated since `sinceState`.
    pub updated: Vec<jmap_types::Id>,
    /// Ids of objects that were destroyed since `sinceState`.
    pub destroyed: Vec<jmap_types::Id>,
    /// `true` if there are more changes beyond this batch.
    pub has_more_changes: bool,
    /// The current state token after applying all reported changes.
    pub new_state: jmap_types::State,
}

impl ChangesResult {
    /// Construct a [`ChangesResult`].
    pub fn new(
        created: Vec<jmap_types::Id>,
        updated: Vec<jmap_types::Id>,
        destroyed: Vec<jmap_types::Id>,
        has_more_changes: bool,
        new_state: jmap_types::State,
    ) -> Self {
        Self {
            created,
            updated,
            destroyed,
            has_more_changes,
            new_state,
        }
    }
}

/// Result of a `/query` call (RFC 8620 §5.5).
#[derive(Debug)]
#[non_exhaustive]
pub struct QueryResult {
    /// The ordered list of matching object ids.
    pub ids: Vec<jmap_types::Id>,
    /// The 0-based index of the first returned id in the complete result list.
    pub position: i64,
    /// Total number of results, if the backend can calculate it.
    pub total: Option<u64>,
    /// Opaque query state token for subsequent `/queryChanges` calls.
    pub query_state: jmap_types::State,
    /// Whether the backend supports `/queryChanges` for this query.
    pub can_calculate_changes: bool,
}

impl QueryResult {
    /// Construct a [`QueryResult`].
    pub fn new(
        ids: Vec<jmap_types::Id>,
        position: i64,
        total: Option<u64>,
        query_state: jmap_types::State,
        can_calculate_changes: bool,
    ) -> Self {
        Self {
            ids,
            position,
            total,
            query_state,
            can_calculate_changes,
        }
    }
}

/// One entry in the `added` list of a `/queryChanges` response (RFC 8620 §5.6).
#[derive(Debug)]
#[non_exhaustive]
pub struct AddedItem {
    /// The id of the newly-added object.
    pub id: jmap_types::Id,
    /// Its 0-based position in the result list after applying all changes.
    pub index: u64,
}

impl AddedItem {
    /// Construct an [`AddedItem`].
    pub fn new(id: jmap_types::Id, index: u64) -> Self {
        Self { id, index }
    }
}

/// Result of a `/queryChanges` call (RFC 8620 §5.6).
#[derive(Debug)]
#[non_exhaustive]
pub struct QueryChangesResult {
    /// The query state token supplied by the client in `sinceQueryState`.
    pub old_query_state: jmap_types::State,
    /// The current query state token.
    pub new_query_state: jmap_types::State,
    /// Total number of results in the new query, if the backend can calculate it.
    pub total: Option<u64>,
    /// Ids removed from the result set since `oldQueryState`.
    pub removed: Vec<jmap_types::Id>,
    /// Ids added to the result set since `oldQueryState`, with their positions.
    pub added: Vec<AddedItem>,
}

impl QueryChangesResult {
    /// Construct a [`QueryChangesResult`].
    pub fn new(
        old_query_state: jmap_types::State,
        new_query_state: jmap_types::State,
        total: Option<u64>,
        removed: Vec<jmap_types::Id>,
        added: Vec<AddedItem>,
    ) -> Self {
        Self {
            old_query_state,
            new_query_state,
            total,
            removed,
            added,
        }
    }
}

// ---------------------------------------------------------------------------
// JmapBackend — the read-side supertrait
// ---------------------------------------------------------------------------

/// Read-side backend supertrait shared by all JMAP server crates.
///
/// Domain-specific backend traits (`MailBackend`, `ChatBackend`, etc.) require
/// this trait as a supertrait and add write-side methods on top.
///
/// Only the read operations that have an identical signature across all JMAP
/// object types belong here. Write operations (`create_object`, `update_object`,
/// `destroy_object`) and domain-specific operations remain in the domain crate.
///
/// The `collapse_threads` parameter on `query_changes` is included for
/// `Email/queryChanges` (RFC 8621 §4.5). Non-mail backends should pass `false`
/// and may ignore the parameter.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl JmapBackend>` when sharing across tasks.
pub trait JmapBackend: Send + Sync + 'static {
    /// The error type returned by storage operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Fetch objects by id (or all objects when `ids` is `None`).
    ///
    /// `properties` is the list of property names requested by the client
    /// (RFC 8620 §5.1). `None` means the client did not send a `properties`
    /// field; the backend should return all properties. When `Some`, the backend
    /// MAY filter the response to only the named properties, but is not required
    /// to — implementations that always return all properties are correct.
    ///
    /// Returns `(found, not_found)` — objects that exist and ids that do not.
    fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        ids: Option<&[jmap_types::Id]>,
        properties: Option<&[String]>,
    ) -> impl std::future::Future<Output = Result<(Vec<O>, Vec<jmap_types::Id>), Self::Error>> + Send;

    /// Return the current state token for an object type in the given account.
    fn get_state<O: JmapObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<jmap_types::State, Self::Error>> + Send;

    /// Return changes since `since_state`, up to `max_changes` entries.
    fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        since_state: &jmap_types::State,
        max_changes: Option<u64>,
    ) -> impl std::future::Future<Output = Result<ChangesResult, BackendChangesError<Self::Error>>> + Send;

    /// Execute a `/query` and return a page of matching ids.
    ///
    /// `position` may be negative — negative values are relative to the end of
    /// the result set per RFC 8620 §5.5 (e.g. -1 means the last result).
    fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> impl std::future::Future<Output = Result<QueryResult, Self::Error>> + Send;

    /// Execute a `/queryChanges` and return deltas since `since_query_state`.
    ///
    /// `collapse_threads` is only meaningful for `Email/queryChanges`
    /// (RFC 8621 §4.5). Pass `false` for all other object types.
    #[allow(clippy::too_many_arguments)]
    fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        since_query_state: &jmap_types::State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&jmap_types::Id>,
        collapse_threads: bool,
    ) -> impl std::future::Future<
        Output = Result<QueryChangesResult, BackendChangesError<Self::Error>>,
    > + Send;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: BackendChangesError::TooManyChanges { limit: 0 } must map to
    /// cannotCalculateChanges (RFC 8620 §5.6), not tooManyChanges with limit 0.
    ///
    /// limit=0 is the convention for "cannot calculate".
    #[test]
    fn backend_changes_error_limit_zero_maps_to_cannot_calculate() {
        let err = jmap_types::JmapError::from(
            BackendChangesError::<std::convert::Infallible>::TooManyChanges { limit: 0 },
        );
        assert_eq!(
            err.error_type.as_str(),
            "cannotCalculateChanges",
            "limit=0 must produce cannotCalculateChanges; got: {:?}",
            err.error_type
        );
    }

    /// Oracle: BackendChangesError::TooManyChanges { limit: N } (N > 0) maps to
    /// tooManyChanges with the suggested limit.
    #[test]
    fn backend_changes_error_nonzero_limit_maps_to_too_many_changes() {
        let err = jmap_types::JmapError::from(
            BackendChangesError::<std::convert::Infallible>::TooManyChanges { limit: 50 },
        );
        assert_eq!(
            err.error_type.as_str(),
            "tooManyChanges",
            "limit=50 must produce tooManyChanges; got: {:?}",
            err.error_type
        );
    }

    /// Oracle: SetErrorType::Custom("mdnAlreadySent") must serialize as the bare
    /// string "mdnAlreadySent" and deserialize back to Custom("mdnAlreadySent").
    /// Extension crates depend on this round-trip to emit domain-specific errors.
    #[test]
    fn set_error_type_custom_round_trips_as_bare_string() {
        let original = SetErrorType::custom("mdnAlreadySent");
        let serialized = serde_json::to_string(&original).expect("serialize");
        assert_eq!(serialized, r#""mdnAlreadySent""#, "Custom must serialize as bare string");
        let deserialized: SetErrorType = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(deserialized, original, "Custom must deserialize back to Custom");
    }

    /// Oracle: known SetErrorType variants (e.g. Singleton) must still
    /// serialize as their camelCase wire strings and deserialize back correctly.
    #[test]
    fn set_error_type_known_variant_round_trips() {
        let original = SetErrorType::Singleton;
        let serialized = serde_json::to_string(&original).expect("serialize");
        assert_eq!(serialized, r#""singleton""#, "Singleton must serialize as \"singleton\"");
        let deserialized: SetErrorType = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(deserialized, original, "Singleton must deserialize back to Singleton");
    }
}
