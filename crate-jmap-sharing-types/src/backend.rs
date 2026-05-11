//! Property selector enums and [`jmap_types::JmapObject`] impls for JMAP Sharing types.
//!
//! These are defined here so that `jmap-sharing-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the sharing types are
//! local to this crate).

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::Principal`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PrincipalProperty {
    /// The `id` property (RFC 9670 §2).
    Id,
    /// The `type` property (RFC 9670 §2).
    Type,
    /// The `name` property (RFC 9670 §2).
    Name,
    /// The `description` property (RFC 9670 §2).
    Description,
    /// The `email` property (RFC 9670 §2).
    Email,
    /// The `timeZone` property (RFC 9670 §2).
    TimeZone,
    /// The `capabilities` property (RFC 9670 §2).
    Capabilities,
    /// The `accounts` property (RFC 9670 §2).
    Accounts,
}

/// Property selector for [`crate::ShareNotification`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ShareNotificationProperty {
    /// The `id` property (RFC 9670 §3).
    Id,
    /// The `created` property (RFC 9670 §3).
    Created,
    /// The `changedBy` property (RFC 9670 §3).
    ChangedBy,
    /// The `objectType` property (RFC 9670 §3).
    ObjectType,
    /// The `objectAccountId` property (RFC 9670 §3).
    ObjectAccountId,
    /// The `objectId` property (RFC 9670 §3).
    ObjectId,
    /// The `oldRights` property (RFC 9670 §3).
    OldRights,
    /// The `newRights` property (RFC 9670 §3).
    NewRights,
    /// The `name` property (RFC 9670 §3).
    Name,
}

// ---------------------------------------------------------------------------
// JmapObject impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::Principal {
    const TYPE_NAME: &'static str = "Principal";
    type Property = PrincipalProperty;
}

impl GetObject for crate::Principal {}

impl SetObject for crate::Principal {
    type Patch = PatchObject;
}

impl QueryObject for crate::Principal {
    type Filter = crate::PrincipalFilterCondition;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::ShareNotification {
    const TYPE_NAME: &'static str = "ShareNotification";
    type Property = ShareNotificationProperty;
}

impl GetObject for crate::ShareNotification {}

impl SetObject for crate::ShareNotification {
    type Patch = PatchObject;
}

impl QueryObject for crate::ShareNotification {
    type Filter = crate::ShareNotificationFilterCondition;
    type Comparator = serde_json::Value;
}
