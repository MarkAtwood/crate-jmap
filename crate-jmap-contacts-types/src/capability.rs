//! draft-ietf-jmap-contacts-10 §1.4 — JMAP Contacts capability objects.
//!
//! Provides [`ContactsCapability`] (session-level, empty object) and
//! [`ContactsAccountCapability`] (account-level, with server limits).

use serde::{Deserialize, Serialize};

/// JMAP Contacts capability URI.
///
/// This is the key used in both the session `capabilities` object and in an
/// account's `accountCapabilities` object.
pub const JMAP_CONTACTS_URI: &str = "urn:ietf:params:jmap:contacts";

/// Session-level contacts capability (contacts-10 §1.4.1).
///
/// The value of the `urn:ietf:params:jmap:contacts` property in the JMAP
/// Session `capabilities` object is an empty JSON object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactsCapability {}

/// Account-level contacts capability (contacts-10 §1.4.1).
///
/// The value of `urn:ietf:params:jmap:contacts` in an account's
/// `accountCapabilities` object.  Contains server limits and permissions for
/// the account.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactsAccountCapability {
    /// Maximum number of AddressBooks that can be assigned to a single
    /// ContactCard.  `null` means no limit.
    pub max_address_books_per_card: Option<u32>,
    /// `true` if the user may create an AddressBook in this account.
    pub may_create_address_book: bool,
}
