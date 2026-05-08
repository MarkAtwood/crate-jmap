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
    Id,
    Type,
    Name,
    Description,
    Email,
    TimeZone,
    Capabilities,
    Accounts,
}

/// Property selector for [`crate::ShareNotification`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ShareNotificationProperty {
    Id,
    Created,
    ChangedBy,
    ObjectType,
    ObjectAccountId,
    ObjectId,
    OldRights,
    NewRights,
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
