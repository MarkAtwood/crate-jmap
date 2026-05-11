//! RFC 9610 §2 — AddressBook object and component types.
//!
//! Provides [`AddressBook`] and [`AddressBookRights`].
//!
//! Note: the spec defines no `AddressBook/query` method, so no
//! `AddressBookFilterCondition` type exists.

use std::collections::HashMap;

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// Access rights a principal holds on an AddressBook (RFC 9610 §2).
///
/// All four rights are booleans.  `Default` produces all-false, which is the
/// most restrictive valid value and a safe starting point when constructing
/// rights in tests or server code.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressBookRights {
    /// User may fetch the ContactCards in this AddressBook.
    pub may_read: bool,
    /// User may create, modify, or destroy all ContactCards in this AddressBook,
    /// or move them to or from this AddressBook.
    pub may_write: bool,
    /// User may modify the `shareWith` property for this AddressBook.
    pub may_share: bool,
    /// User may delete the AddressBook itself.
    pub may_delete: bool,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A JMAP AddressBook object (RFC 9610 §2).
///
/// An AddressBook is a named collection of ContactCards.  All ContactCards
/// belong to one or more AddressBooks.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressBook {
    /// Server-assigned immutable identifier.
    pub id: Id,
    /// User-visible name; MUST NOT be empty and MUST NOT exceed 255 UTF-8 octets.
    pub name: String,
    /// Optional longer-form description (default: null).
    ///
    /// Required-and-nullable per RFC 9553 + RFC 9610 §4.1: always present on
    /// the wire as `"description": null` when unset.  Must NOT use
    /// `skip_serializing_if`.
    #[serde(default)]
    pub description: Option<String>,
    /// UI sort order; lower values display first.  Range: [0, 2^31).
    pub sort_order: u32,
    /// True for at most one AddressBook per account; server-set.
    pub is_default: bool,
    /// True if the user has subscribed to this AddressBook.
    pub is_subscribed: bool,
    /// Map of principal id → rights for principals this AddressBook is shared
    /// with.
    ///
    /// Required-and-nullable (RFC 9610 §2 type: `Id[AddressBookRights]|null`):
    /// always present in wire JSON; serializes as `null` when not shared.
    /// The spec §4.1 example shows `"shareWith": null` explicitly — never absent.
    #[serde(default)]
    pub share_with: Option<HashMap<Id, AddressBookRights>>,
    /// ACL rights the authenticated user has on this AddressBook; server-set.
    pub my_rights: AddressBookRights,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
