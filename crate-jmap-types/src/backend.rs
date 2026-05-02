//! Backend marker traits shared across all JMAP server crates.
//!
//! These traits are defined here in `jmap-types` so that implementations can
//! go in the type crates (`jmap-mail-types`, `jmap-chat-types`) without
//! violating the orphan rule.

// ---------------------------------------------------------------------------
// Marker traits
// ---------------------------------------------------------------------------

/// Marker trait for all JMAP object types.
///
/// Implement this in the *types* crate for each first-class JMAP object (e.g.
/// `Mailbox`, `Email`, `Chat`). The [`TYPE_NAME`](JmapObject::TYPE_NAME) constant
/// is used in error messages and capability checks in the server crates.
pub trait JmapObject:
    serde::Serialize + for<'de> serde::Deserialize<'de> + Send + Sync + 'static
{
    /// The JMAP type name string (e.g. `"Email"`, `"Mailbox"`, `"Chat"`).
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
    /// The filter condition type.
    ///
    /// Must implement both `Serialize` and `DeserializeOwned` so that backends
    /// can inspect or forward the filter.
    type Filter: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static;
    /// The comparator type for sort operations.
    type Comparator: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static;
}
