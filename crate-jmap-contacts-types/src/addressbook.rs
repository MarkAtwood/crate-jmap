//! draft-ietf-jmap-contacts-10 §2 — AddressBook object and component types.
//!
//! Provides [`AddressBook`], [`AddressBookRights`], and
//! [`AddressBookFilterCondition`].

use std::collections::HashMap;

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// Access rights a principal holds on an AddressBook (contacts-10 §2).
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
}

/// A JMAP AddressBook object (draft-ietf-jmap-contacts-10 §2).
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Required-and-nullable (contacts-10 §2 type: `Id[AddressBookRights]|null`):
    /// always present in wire JSON; serializes as `null` when not shared.
    /// The spec §4.1 example shows `"shareWith": null` explicitly — never absent.
    #[serde(default)]
    pub share_with: Option<HashMap<Id, AddressBookRights>>,
    /// ACL rights the authenticated user has on this AddressBook; server-set.
    pub my_rights: AddressBookRights,
}

/// Filter condition for `AddressBook/query`.
///
/// All fields are optional; a condition with no fields set matches every
/// AddressBook.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressBookFilterCondition {
    /// Matches AddressBooks whose name contains this string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Matches AddressBooks with this exact subscription state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_subscribed: Option<bool>,
}
