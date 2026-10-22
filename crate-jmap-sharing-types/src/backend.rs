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
    type Filter = crate::PrincipalFilter;
    // RFC 9670 §2.4 names no Principal sort properties, so there is no
    // typed property enum to give this comparator.
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
    type Filter = crate::ShareNotificationFilter;
    type Comparator = crate::ShareNotificationComparator;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: RFC 8620 §5.5 (line 2368) types the /query `filter` as
    /// "FilterOperator|FilterCondition|null"; RFC 9670 §2.4 and §3.4 say
    /// Principal/query and ShareNotification/query are "a standard
    /// "/query" method", with the `accountIds` (§2.4.1) and `after`
    /// (§3.4.1) conditions. A hand-written NOT tree decodes as each filter.
    #[test]
    fn query_filters_are_operator_trees() {
        /// Decodes `json` as `O`'s filter, which must be the full tree.
        fn decodes_as_operator<O, C>(json: serde_json::Value) -> bool
        where
            O: QueryObject<Filter = jmap_types::Filter<C>>,
            C: serde::de::DeserializeOwned,
        {
            let f: O::Filter = serde_json::from_value(json).expect("operator tree must decode");
            matches!(f, jmap_types::Filter::Operator(_))
        }
        let not = |cond: serde_json::Value| serde_json::json!({ "operator": "NOT", "conditions": [cond] });
        assert!(decodes_as_operator::<crate::Principal, _>(not(
            serde_json::json!({ "accountIds": ["a1"] })
        )));
        assert!(decodes_as_operator::<crate::ShareNotification, _>(not(
            serde_json::json!({ "after": "2026-01-01T00:00:00Z" })
        )));
    }
}
