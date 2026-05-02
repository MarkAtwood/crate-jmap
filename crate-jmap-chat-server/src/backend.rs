//! ChatBackend trait and supporting types for JMAP Chat method handlers.
//!
//! Consumers implement [`ChatBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations are here.
//!
//! Marker traits and property selector enums live in `jmap-types` and
//! `jmap-chat-types` respectively; they are re-exported here for convenience.

pub use jmap_chat_types::backend::{
    ChatContactProperty, ChatProperty, MessageProperty, ReadPositionProperty, SpaceProperty,
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
    /// The existing object id (for `alreadyExists`).
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
}

/// The machine-readable type for a [`SetError`] (RFC 8620 §5.3).
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
    /// An object with the same unique key already exists.
    AlreadyExists,
}

impl std::fmt::Display for SetErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
            Self::AlreadyExists => "alreadyExists",
        };
        f.write_str(s)
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

impl<E> From<SetError> for BackendSetError<E> {
    fn from(e: SetError) -> Self {
        Self::SetError(e)
    }
}

// ---------------------------------------------------------------------------
// ChatBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for JMAP Chat method handlers.
///
/// Implementors provide the actual data access; the method handler modules
/// in this crate translate between JMAP wire protocol and backend calls.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are defined on the [`JmapBackend`]
/// supertrait. Only write operations and type introspection are here.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl ChatBackend>` when sharing across tasks.
pub trait ChatBackend: JmapBackend {
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

    /// Return `true` if this backend supports the given JMAP object type.
    ///
    /// Return `false` to disable the corresponding method group for this
    /// backend instance.
    fn supports_type<O: JmapObject>(&self) -> bool;
}
