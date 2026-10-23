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

/// RFC 9670 §2 (rfc9670.txt:257, 287, 292): `id` is "(immutable;
/// server-set)"; `capabilities` and `accounts` are "(server-set)".
/// draft-ietf-jmap-metadata-04 §3.2: clients write `metadata` and
/// `privateMetadata` through `Foo/set`.
impl SetObject for crate::Principal {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "capabilities", "accounts"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "type",
        "name",
        "description",
        "email",
        "timeZone",
        "metadata",
        "privateMetadata",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
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

/// RFC 9670 §3 (rfc9670.txt:405-443): every property is "(immutable;
/// server-set)"; §3.3: "Only destroy is supported".
impl SetObject for crate::ShareNotification {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "created",
        "changedBy",
        "objectType",
        "objectAccountId",
        "objectId",
        "oldRights",
        "newRights",
        "name",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] = &[];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl QueryObject for crate::ShareNotification {
    type Filter = crate::ShareNotificationFilter;
    type Comparator = crate::ShareNotificationComparator;
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_types::{Id, UTCDate, check_property_classification};
    use serde_json::{Map, Value, json};
    use std::collections::BTreeMap;

    fn de<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).expect("hand-written fixture deserializes")
    }

    // Each classification test builds a fully populated object with a
    // struct literal, so a new field is a compile error until it is set,
    // then a classification failure until the `SetObject` lists name it.

    #[test]
    fn principal_properties_are_classified() {
        let obj = crate::Principal {
            id: Id::from("p1"),
            principal_type: de(json!("individual")),
            name: "n".into(),
            description: Some("d".into()),
            email: Some("a@example.com".into()),
            time_zone: Some("Etc/UTC".into()),
            capabilities: BTreeMap::new(),
            accounts: Some(BTreeMap::new()),
            metadata: Some(de(json!({"acme.example.com": {"memo": "m"}}))),
            private_metadata: Some(de(json!({"acme.example.com": {"memo": "p"}}))),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn share_notification_properties_are_classified() {
        let rights = || Some(BTreeMap::from([("mayRead".to_owned(), true)]));
        let obj = crate::ShareNotification {
            id: Id::from("n1"),
            created: UTCDate::from("2026-01-01T00:00:00Z"),
            changed_by: de(json!({"name": "A", "email": "a@example.com", "principalId": "p1"})),
            object_type: "Calendar".into(),
            object_account_id: Id::from("a1"),
            object_id: Id::from("c1"),
            old_rights: rights(),
            new_rights: rights(),
            name: "n".into(),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

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
