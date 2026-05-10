//! RFC 9670 §3 ShareNotification object and related types.
//!
//! Provides [`ShareNotification`], [`ChangedBy`], and
//! [`ShareNotificationFilterCondition`].
//!
//! A ShareNotification records when the authenticated user's permissions to
//! access a shared object change.  Notifications are created only by the server;
//! clients may only query and destroy them.

use std::collections::HashMap;

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// Identifies the entity that made a permission change (RFC 9670 §3).
///
/// Called "Entity" in the RFC; named `ChangedBy` here for clarity.
///
/// ## Nullable fields
///
/// `email` and `principal_id` are required-but-nullable per the RFC.  They
/// MUST serialize as `null` when `None`, not be absent from the wire.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedBy {
    /// Display name of the entity that made the change.
    pub name: String,

    /// Email of the entity that made the change, or `null` if unavailable.
    ///
    /// Serializes as `null` when `None` (required-but-nullable per RFC 9670 §3).
    pub email: Option<String>,

    /// Id of the Principal corresponding to this entity, or `null` if none.
    ///
    /// Serializes as `null` when `None` (required-but-nullable per RFC 9670 §3).
    pub principal_id: Option<Id>,
}

/// A JMAP ShareNotification object (RFC 9670 §3).
///
/// Records a change in the authenticated user's permissions on a shared object.
/// All fields are server-set and immutable; clients may only destroy these objects.
///
/// ## Nullable fields
///
/// `old_rights` and `new_rights` are required-but-nullable.  A `null` `old_rights`
/// means the user was newly granted access; a `null` `new_rights` means access was
/// revoked entirely.  Both serialize as `null` when `None`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareNotification {
    /// Server-assigned immutable identifier.
    pub id: Id,

    /// Time this notification was created (server-set, immutable).
    pub created: UTCDate,

    /// Who made the permission change (server-set, immutable).
    pub changed_by: ChangedBy,

    /// Name of the JMAP data type for the shared object, e.g., `"Mailbox"`.
    pub object_type: String,

    /// Id of the Account where the shared object lives.
    pub object_account_id: Id,

    /// Id of the shared object.
    pub object_id: Id,

    /// `myRights` on the object before the change, or `null` if newly granted.
    ///
    /// Serializes as `null` when `None` (required-but-nullable per RFC 9670 §3).
    pub old_rights: Option<HashMap<String, bool>>,

    /// `myRights` on the object after the change, or `null` if access removed.
    ///
    /// Serializes as `null` when `None` (required-but-nullable per RFC 9670 §3).
    pub new_rights: Option<HashMap<String, bool>>,

    /// Name of the shared object at the time of the notification.
    pub name: String,
}

/// Filter condition for `ShareNotification/query` (RFC 9670 §3.4.1).
///
/// All fields are optional; absent fields match unconditionally.
/// When multiple fields are set, all must match (logical AND).
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field. Filter clauses the
/// server does not understand are a query-correctness hazard — silently
/// preserving an unrecognised clause and round-tripping it back to the
/// client can return the wrong set of records with no error signal.
///
/// ## What to do instead
///
/// **IETF-track path.** Vendors who need both capability-level declaration
/// and filterability for custom fields should use
/// `draft-ietf-jmap-metadata` (capability URI
/// `urn:ietf:params:jmap:metadata`), which defines a filterable
/// `Metadata` / `Annotation` companion object. Workspace implementation
/// tracker: bd JMAP-06zp.
///
/// **Pre-IETF escape.** Vendors who cannot wait for the metadata draft can
/// either escape the filter tree to `serde_json::Value` or fork the
/// `FilterCondition` type. See `crate-jmap-calendars-types/PLAN.md` for
/// the hybrid sloppy-value pattern.
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareNotificationFilterCondition {
    /// Notifications created on or after this date match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<UTCDate>,

    /// Notifications created before this date match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<UTCDate>,

    /// The `objectType` of the notification must exactly match this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,

    /// The `objectAccountId` of the notification must exactly match this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_account_id: Option<Id>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: hand-written from RFC 9670 §3 field descriptions.
    fn rfc_notification_new_share_json() -> serde_json::Value {
        json!({
            "id": "notif1",
            "created": "2024-03-15T10:00:00Z",
            "changedBy": {
                "name": "Alice Smith",
                "email": "alice@example.com",
                "principalId": "P123"
            },
            "objectType": "Mailbox",
            "objectAccountId": "acc1",
            "objectId": "mb1",
            "oldRights": null,
            "newRights": {
                "mayReadItems": true,
                "mayAddItems": false
            },
            "name": "Team Inbox"
        })
    }

    /// Oracle for access-revoked notification: newRights is null.
    fn rfc_notification_revoke_json() -> serde_json::Value {
        json!({
            "id": "notif2",
            "created": "2024-03-16T08:30:00Z",
            "changedBy": {
                "name": "Bob Jones",
                "email": null,
                "principalId": null
            },
            "objectType": "Calendar",
            "objectAccountId": "acc2",
            "objectId": "cal1",
            "oldRights": {
                "mayReadItems": true
            },
            "newRights": null,
            "name": "Shared Calendar"
        })
    }

    #[test]
    fn deserialize_new_share_notification() {
        let json = rfc_notification_new_share_json();
        let n: ShareNotification =
            serde_json::from_value(json).expect("deserialize ShareNotification");

        assert_eq!(n.id, "notif1");
        assert_eq!(n.created, "2024-03-15T10:00:00Z");
        assert_eq!(n.changed_by.name, "Alice Smith");
        assert_eq!(n.changed_by.email.as_deref(), Some("alice@example.com"));
        assert_eq!(
            n.changed_by.principal_id.as_ref().map(|id| id.as_ref()),
            Some("P123")
        );
        assert_eq!(n.object_type, "Mailbox");
        assert_eq!(n.object_account_id, "acc1");
        assert_eq!(n.object_id, "mb1");
        assert!(n.old_rights.is_none()); // newly granted — was null
        let new_rights = n.new_rights.as_ref().expect("newRights should be Some");
        assert_eq!(new_rights.get("mayReadItems"), Some(&true));
        assert_eq!(new_rights.get("mayAddItems"), Some(&false));
        assert_eq!(n.name, "Team Inbox");
    }

    #[test]
    fn deserialize_revoke_notification() {
        let json = rfc_notification_revoke_json();
        let n: ShareNotification =
            serde_json::from_value(json).expect("deserialize revoke notification");

        assert!(n.new_rights.is_none()); // revoked — is null
        let old_rights = n.old_rights.as_ref().expect("oldRights should be Some");
        assert_eq!(old_rights.get("mayReadItems"), Some(&true));
    }

    /// Nullable fields must serialize as `null`, not be absent.
    #[test]
    fn nullable_rights_serialize_as_null() {
        let json = rfc_notification_new_share_json();
        let n: ShareNotification = serde_json::from_value(json).expect("deserialize");
        let serialized = serde_json::to_value(&n).expect("serialize");

        assert_eq!(serialized["oldRights"], serde_json::Value::Null);
        // newRights is present as a map, not null
        assert!(serialized["newRights"].is_object());
    }

    #[test]
    fn nullable_changed_by_fields_serialize_as_null() {
        let json = rfc_notification_revoke_json();
        let n: ShareNotification = serde_json::from_value(json).expect("deserialize");
        let serialized = serde_json::to_value(&n).expect("serialize");

        assert_eq!(serialized["changedBy"]["email"], serde_json::Value::Null);
        assert_eq!(
            serialized["changedBy"]["principalId"],
            serde_json::Value::Null
        );
        assert_eq!(serialized["newRights"], serde_json::Value::Null);
    }

    #[test]
    fn notification_roundtrip() {
        let json = rfc_notification_new_share_json();
        let n: ShareNotification = serde_json::from_value(json.clone()).expect("deserialize");
        let serialized = serde_json::to_value(&n).expect("serialize");
        let n2: ShareNotification = serde_json::from_value(serialized).expect("deserialize again");
        assert_eq!(n, n2);
    }

    // --- ShareNotificationFilterCondition tests ---

    #[test]
    fn notification_filter_default_is_empty() {
        let fc = ShareNotificationFilterCondition::default();
        let json = serde_json::to_value(&fc).expect("serialize empty filter");
        assert_eq!(json, json!({}));
    }

    #[test]
    fn notification_filter_after_and_before() {
        let json = json!({
            "after": "2024-01-01T00:00:00Z",
            "before": "2024-12-31T23:59:59Z"
        });
        let fc: ShareNotificationFilterCondition =
            serde_json::from_value(json).expect("deserialize filter");
        assert_eq!(
            fc.after.as_ref().map(|d| d.as_ref()),
            Some("2024-01-01T00:00:00Z")
        );
        assert_eq!(
            fc.before.as_ref().map(|d| d.as_ref()),
            Some("2024-12-31T23:59:59Z")
        );
    }

    #[test]
    fn notification_filter_roundtrip() {
        let json = json!({
            "objectType": "Mailbox",
            "objectAccountId": "acc1"
        });
        let fc: ShareNotificationFilterCondition =
            serde_json::from_value(json.clone()).expect("deserialize");
        let reserialized = serde_json::to_value(&fc).expect("serialize");
        assert_eq!(reserialized["objectType"], json["objectType"]);
        assert_eq!(reserialized["objectAccountId"], json["objectAccountId"]);
    }

    /// Explicit JSON `null` for `after` and `before` deserializes without error.
    ///
    /// Oracle: RFC 9670 §3.4.1 — `after` and `before` are `UTCDate|null`; an
    /// explicit `null` value is valid JSON and must be accepted.
    #[test]
    fn notification_filter_null_after_before() {
        let json = json!({"after": null, "before": null});
        let fc: ShareNotificationFilterCondition =
            serde_json::from_value(json).expect("explicit null after/before must deserialize");
        assert!(fc.after.is_none(), "after must be None when JSON null");
        assert!(fc.before.is_none(), "before must be None when JSON null");
    }

    /// `ShareNotificationFilterCondition::default()` serializes to `{}`.
    ///
    /// Oracle: all fields are optional and absent when None per `skip_serializing_if`.
    #[test]
    fn notification_filter_default_serializes_to_empty_object() {
        let fc = ShareNotificationFilterCondition::default();
        let out = serde_json::to_value(&fc).expect("must serialize");
        assert_eq!(
            out,
            json!({}),
            "default must serialize to empty object, got: {out}"
        );
    }
}
