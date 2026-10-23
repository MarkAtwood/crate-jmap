//! RFC 9610 §2 — AddressBook object and component types.
//!
//! Provides [`AddressBook`] and [`AddressBookRights`].
//!
//! Note: the spec defines no `AddressBook/query` method, so no
//! `AddressBookFilterCondition` type exists.

use std::collections::BTreeMap;

use jmap_types::Id;
use serde::{Deserialize, Serialize};

use crate::collision::{self, CollisionError};

/// Camel-case wire-format names of every typed field on
/// [`AddressBookRights`]. Used by
/// [`AddressBookRights::validate_extras`]. JMAP-glx8.25.
const ADDRESS_BOOK_RIGHTS_TYPED_FIELDS: &[&str] = &[
    "mayRead",
    "mayWrite",
    "mayShare",
    "mayDelete",
    "maySetMetadata",
];

/// Camel-case wire-format names of every typed field on [`AddressBook`].
/// Used by [`AddressBook::validate_extras`]. JMAP-glx8.25.
const ADDRESS_BOOK_TYPED_FIELDS: &[&str] = &[
    "id",
    "name",
    "description",
    "sortOrder",
    "isDefault",
    "isSubscribed",
    "shareWith",
    "myRights",
    "metadata",
    "privateMetadata",
];

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
    /// User may modify the shared metadata of this AddressBook and of the
    /// ContactCards in it (draft-ietf-jmap-metadata-04 §4.1).
    /// `None` when the server omits it: the right is present only when the
    /// metadata capability lists this type or a type it contains (§4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub may_set_metadata: Option<bool>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// See [`ContactCard::extra`](crate::ContactCard::extra) for the
    /// collision contract: programmatically inserting a key that
    /// matches a typed field name produces a duplicate JSON object key
    /// on serialize. Treat `extra` as a write-only catch-all for unknown
    /// keys discovered at deserialize. JMAP-glx8.19.
    ///
    /// See [`AddressBookRights::validate_extras`] for a runtime
    /// pre-serialize check that detects this hazard. JMAP-glx8.25.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl AddressBookRights {
    /// Check that no [`extra`](Self::extra) key shadows a typed
    /// wire-format field of this struct. Returns
    /// [`Err(CollisionError)`](CollisionError) listing any colliding
    /// keys; otherwise returns `Ok(())`. See
    /// [`ContactCard::validate_extras`](crate::ContactCard::validate_extras)
    /// for the design rationale. JMAP-glx8.25.
    ///
    /// # Errors
    ///
    /// Returns [`CollisionError`] when one or more keys in
    /// [`extra`](Self::extra) match one of the camelCase wire-format
    /// names of this struct's typed fields.
    pub fn validate_extras(&self) -> Result<(), CollisionError> {
        collision::check(&self.extra, ADDRESS_BOOK_RIGHTS_TYPED_FIELDS)
    }
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
    /// Defaults to 0 per RFC 9610 §2.
    #[serde(default)]
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
    pub share_with: Option<BTreeMap<Id, AddressBookRights>>,
    /// ACL rights the authenticated user has on this AddressBook; server-set.
    pub my_rights: AddressBookRights,

    /// Shared metadata, keyed by namespace identifier
    /// (draft-ietf-jmap-metadata-04 §2). Each value is a JSON object (§2.1).
    /// `None` when absent, which -04 §2 treats as `{}`; in an update a
    /// `null` value "is equivalent to {}" (-04 §3.2) and also decodes as
    /// `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,

    /// The authenticated user's own metadata, keyed by namespace
    /// identifier (draft-ietf-jmap-metadata-04 §2). Same shape and `None`
    /// handling as `metadata`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_metadata: Option<serde_json::Map<String, serde_json::Value>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// See [`ContactCard::extra`](crate::ContactCard::extra) for the
    /// collision contract: programmatically inserting a key that
    /// matches a typed field name produces a duplicate JSON object key
    /// on serialize. JMAP-glx8.19.
    ///
    /// See [`AddressBook::validate_extras`] for a runtime pre-serialize
    /// check that detects this hazard. JMAP-glx8.25.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl AddressBook {
    /// Check that no [`extra`](Self::extra) key shadows a typed
    /// wire-format field of this struct. Returns
    /// [`Err(CollisionError)`](CollisionError) listing any colliding
    /// keys; otherwise returns `Ok(())`. See
    /// [`ContactCard::validate_extras`](crate::ContactCard::validate_extras)
    /// for the design rationale. JMAP-glx8.25.
    ///
    /// # Errors
    ///
    /// Returns [`CollisionError`] when one or more keys in
    /// [`extra`](Self::extra) match one of the camelCase wire-format
    /// names of this struct's typed fields.
    pub fn validate_extras(&self) -> Result<(), CollisionError> {
        collision::check(&self.extra, ADDRESS_BOOK_TYPED_FIELDS)
    }
}
