//! MailBackend trait and supporting types for RFC 8621 method handlers.
//!
//! Consumers implement [`MailBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.

// ---------------------------------------------------------------------------
// Marker traits
// ---------------------------------------------------------------------------

/// Marker trait for all JMAP object types.
///
/// All types passed as type parameters to [`MailBackend`] methods must implement
/// this trait. The [`TYPE_NAME`](JmapObject::TYPE_NAME) constant is used in
/// error messages and capability checks.
pub trait JmapObject:
    serde::Serialize + for<'de> serde::Deserialize<'de> + Send + Sync + 'static
{
    /// The JMAP type name string (e.g. `"Email"`, `"Mailbox"`).
    const TYPE_NAME: &'static str;
    /// The property selector enum for this type (server-side only, no serde).
    type Property: Send + Sync + 'static;
}

/// Marker for object types that support `get` and `changes` operations.
pub trait GetObject: JmapObject {}

/// Marker for object types that support `set` (create/update/destroy) operations.
pub trait SetObject: JmapObject {
    /// The patch type for update operations.
    ///
    /// Typically [`serde_json::Value`] for open-ended JSON Merge Patch, or a
    /// typed struct if the backend wants structured patching.
    type Patch: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static;
}

/// Marker for object types that support `query` and `queryChanges` operations.
pub trait QueryObject: JmapObject {
    /// The filter condition type (e.g. [`jmap_mail_types::EmailFilterCondition`]).
    type Filter: serde::de::DeserializeOwned + Send + Sync + 'static;
    /// The comparator type (e.g. [`jmap_mail_types::EmailComparator`]).
    type Comparator: serde::de::DeserializeOwned + Send + Sync + 'static;
}

// ---------------------------------------------------------------------------
// SetError and SetErrorType
// ---------------------------------------------------------------------------

/// A per-item error in a `/set` response (`notCreated`, `notUpdated`,
/// `notDestroyed` maps) (RFC 8620 §5.3).
///
/// Construct with [`SetError::new`] and chain the builder methods as needed.
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
}

impl SetError {
    /// Construct a [`SetError`] with the given type and all optional fields `None`.
    pub fn new(error_type: SetErrorType) -> Self {
        Self {
            error_type,
            description: None,
            properties: None,
            existing_id: None,
        }
    }

    /// Set the human-readable description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the list of property names that caused the error.
    pub fn with_properties(mut self, props: Vec<String>) -> Self {
        self.properties = Some(props);
        self
    }

    /// Set the existing object id (used with `alreadyExists`).
    pub fn with_existing_id(mut self, id: jmap_types::Id) -> Self {
        self.existing_id = Some(id);
        self
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
        // Delegate to serde's camelCase rename_all mapping so the wire-format
        // string is defined in exactly one place.
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(s)) => f.write_str(&s),
            _ => f.write_str("unknown"),
        }
    }
}

// ---------------------------------------------------------------------------
// Backend error envelopes
// ---------------------------------------------------------------------------

/// Error type returned by [`MailBackend::get_changes`] and
/// [`MailBackend::query_changes`].
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendChangesError<E> {
    /// The `sinceState` is too old or the server cannot calculate the full set
    /// of intermediate states. Maps to `tooManyChanges` in the response with
    /// the given suggested limit.
    TooManyChanges { limit: u64 },
    /// An unexpected storage-layer error.
    Other(E),
}

impl<E: std::error::Error> From<BackendChangesError<E>> for jmap_types::JmapError {
    fn from(e: BackendChangesError<E>) -> Self {
        match e {
            BackendChangesError::TooManyChanges { limit } => {
                jmap_types::JmapError::too_many_changes_with_limit(limit)
            }
            BackendChangesError::Other(inner) => {
                jmap_types::JmapError::server_fail(inner.to_string())
            }
        }
    }
}

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
// MailBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for RFC 8621 JMAP Mail method handlers.
///
/// Implementors provide the actual data access; the method handler modules
/// in this crate translate between JMAP wire protocol and backend calls.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl MailBackend>` when sharing across tasks.
pub trait MailBackend: Send + Sync + 'static {
    /// The error type returned by storage operations.
    type Error: std::error::Error + Send + Sync + 'static;

    // -----------------------------------------------------------------------
    // Generic CRUD
    // -----------------------------------------------------------------------

    /// Fetch objects by id (or all objects when `ids` is `None`).
    ///
    /// Returns `(found, not_found)` — objects that exist and ids that do not.
    fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        ids: Option<&[jmap_types::Id]>,
        properties: Option<&[<O as JmapObject>::Property]>,
    ) -> impl std::future::Future<Output = Result<(Vec<O>, Vec<jmap_types::Id>), Self::Error>> + Send;

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
    /// Returns the updated object if the backend wants to report server-set
    /// properties, or `None` if the client's patch was applied verbatim.
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

    // -----------------------------------------------------------------------
    // Query
    // -----------------------------------------------------------------------

    /// Execute a `/query` and return a page of matching ids.
    fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> impl std::future::Future<Output = Result<QueryResult, Self::Error>> + Send;

    /// Execute a `/queryChanges` and return deltas since `since_query_state`.
    fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        since_query_state: &jmap_types::State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&jmap_types::Id>,
    ) -> impl std::future::Future<
        Output = Result<QueryChangesResult, BackendChangesError<Self::Error>>,
    > + Send;

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
    /// Used by the `Email/set` create path to assign new emails to an existing
    /// thread when they reply to or reference a known message. Backends should
    /// maintain a message-id index to answer this in O(1); the default
    /// implementation performs a full scan and is provided only as a fallback
    /// for backends that do not yet have an index.
    fn find_thread_by_message_ids(
        &self,
        account_id: &jmap_types::Id,
        message_ids: &[&str],
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send;

    /// Return `true` if `blob_id` exists in `account_id`'s blob store.
    ///
    /// Used by `Email/parse` to distinguish RFC 8621 §5.8 error categories:
    /// a blob that exists but cannot be parsed → `notParsable`; one that does
    /// not exist → `notFound`. The default returns `true`, which preserves the
    /// pre-existing behaviour of routing all parse failures to `notParsable`.
    /// Backends should override this for correct RFC conformance.
    fn blob_exists(
        &self,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = bool> + Send {
        let _ = (account_id, blob_id);
        std::future::ready(true)
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
    ) -> impl std::future::Future<
        Output = Result<(jmap_types::Id, jmap_mail_types::Email), BackendSetError<Self::Error>>,
    > + Send;

    /// Return search snippets for the given Email ids (RFC 8621 §5 — `SearchSnippet/get`).
    fn search_snippets(
        &self,
        account_id: &jmap_types::Id,
        email_ids: &[jmap_types::Id],
        filter: Option<&jmap_mail_types::EmailFilterCondition>,
    ) -> impl std::future::Future<Output = Result<Vec<jmap_mail_types::SearchSnippet>, Self::Error>> + Send;

    /// Return `true` if this backend supports the given JMAP object type.
    ///
    /// Return `false` for [`jmap_mail_types::SearchSnippet`] to disable
    /// `SearchSnippet/get` for this backend instance.
    fn supports_type<O: JmapObject>(&self) -> bool;
}

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`jmap_mail_types::Mailbox`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq)]
pub enum MailboxProperty {
    Id,
    Name,
    ParentId,
    Role,
    SortOrder,
    TotalEmails,
    UnreadEmails,
    TotalThreads,
    UnreadThreads,
    MyRights,
    IsSubscribed,
}

/// Property selector for [`jmap_mail_types::Thread`] `/get`.
#[derive(Debug, Clone, PartialEq)]
pub enum ThreadProperty {
    Id,
    EmailIds,
}

/// Property selector for [`jmap_mail_types::Email`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq)]
pub enum EmailProperty {
    Id,
    BlobId,
    ThreadId,
    MailboxIds,
    Keywords,
    Size,
    ReceivedAt,
    MessageId,
    InReplyTo,
    References,
    Subject,
    From,
    To,
    Cc,
    Bcc,
    ReplyTo,
    Sender,
    SentAt,
    HasAttachment,
    Preview,
    BodyStructure,
    TextBody,
    HtmlBody,
    Attachments,
    BodyValues,
    Headers,
}

/// Property selector for [`jmap_mail_types::Identity`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq)]
pub enum IdentityProperty {
    Id,
    Name,
    Email,
    ReplyTo,
    Bcc,
    TextSignature,
    HtmlSignature,
    MayDelete,
}

/// Property selector for [`jmap_mail_types::EmailSubmission`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq)]
pub enum EmailSubmissionProperty {
    Id,
    IdentityId,
    EmailId,
    ThreadId,
    Envelope,
    SendAt,
    UndoStatus,
    DeliveryStatus,
    DsnBlobIds,
    MdnBlobIds,
}

/// Property selector for [`jmap_mail_types::VacationResponse`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq)]
pub enum VacationResponseProperty {
    Id,
    IsEnabled,
    FromDate,
    ToDate,
    Subject,
    TextBody,
    HtmlBody,
}

/// Property selector for [`jmap_mail_types::SearchSnippet`] `/get`.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchSnippetProperty {
    EmailId,
    Subject,
    Preview,
}

// ---------------------------------------------------------------------------
// JmapObject impls for all RFC 8621 mail types
// ---------------------------------------------------------------------------

impl JmapObject for jmap_mail_types::Mailbox {
    const TYPE_NAME: &'static str = "Mailbox";
    type Property = MailboxProperty;
}

impl GetObject for jmap_mail_types::Mailbox {}

impl SetObject for jmap_mail_types::Mailbox {
    type Patch = serde_json::Value;
}

impl QueryObject for jmap_mail_types::Mailbox {
    /// RFC 8621 §2.3 — Mailbox/query has no defined filter condition object;
    /// reuse [`jmap_mail_types::EmailFilter`] as the closest available type.
    type Filter = jmap_mail_types::EmailFilter;
    type Comparator = serde_json::Value;
}

impl JmapObject for jmap_mail_types::Thread {
    const TYPE_NAME: &'static str = "Thread";
    type Property = ThreadProperty;
}

impl GetObject for jmap_mail_types::Thread {}

impl JmapObject for jmap_mail_types::Email {
    const TYPE_NAME: &'static str = "Email";
    type Property = EmailProperty;
}

impl GetObject for jmap_mail_types::Email {}

impl SetObject for jmap_mail_types::Email {
    type Patch = serde_json::Value;
}

impl QueryObject for jmap_mail_types::Email {
    type Filter = jmap_mail_types::EmailFilter;
    type Comparator = jmap_mail_types::EmailComparator;
}

impl JmapObject for jmap_mail_types::Identity {
    const TYPE_NAME: &'static str = "Identity";
    type Property = IdentityProperty;
}

impl GetObject for jmap_mail_types::Identity {}

impl SetObject for jmap_mail_types::Identity {
    type Patch = serde_json::Value;
}

impl JmapObject for jmap_mail_types::EmailSubmission {
    const TYPE_NAME: &'static str = "EmailSubmission";
    type Property = EmailSubmissionProperty;
}

impl GetObject for jmap_mail_types::EmailSubmission {}

impl SetObject for jmap_mail_types::EmailSubmission {
    type Patch = serde_json::Value;
}

impl QueryObject for jmap_mail_types::EmailSubmission {
    type Filter = jmap_mail_types::EmailSubmissionFilter;
    type Comparator = serde_json::Value;
}

impl JmapObject for jmap_mail_types::VacationResponse {
    const TYPE_NAME: &'static str = "VacationResponse";
    type Property = VacationResponseProperty;
}

impl GetObject for jmap_mail_types::VacationResponse {}

impl SetObject for jmap_mail_types::VacationResponse {
    type Patch = serde_json::Value;
}

impl JmapObject for jmap_mail_types::SearchSnippet {
    const TYPE_NAME: &'static str = "SearchSnippet";
    type Property = SearchSnippetProperty;
}
