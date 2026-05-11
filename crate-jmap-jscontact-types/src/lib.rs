//! JSContact (RFC 9553) typed sub-types for the jmap-* crate family.
//!
//! Normative reference: RFC 9553 (JSContact).
//!
//! These are sub-object types that have no JMAP identity of their own.
//! They are embedded within `ContactCard` (from `jmap-contacts-types`).
//!
//! ## Crate family position
//!
//! ```text
//! (no JMAP dep)
//!     └── jmap-jscontact-types  ← this crate (RFC 9553 typed sub-types)
//!             └── jmap-contacts-types (consumes via path-dep + re-export)
//! ```
//!
//! ## Design: optional fields and `Option<...>`
//!
//! RFC 9553 marks most fields optional, and JMAP `properties` arguments
//! permit partial responses. Every optional field is `Option<...>` with
//! `#[serde(skip_serializing_if = "Option::is_none")]` so partial inputs
//! round-trip unchanged. Mandatory fields per the RFC are kept as bare
//! types (not `Option`) to express the requirement at the type level —
//! callers building a fresh sub-object must populate them.
//!
//! ## Design: `@type` discriminator
//!
//! Every RFC 9553 sub-object carries an `@type` discriminator on the
//! wire. The Rust field is named `at_type: String` and renamed to
//! `"@type"` via serde attributes. The field is mandatory per spec but
//! modelled as `String` (not an enum) to preserve forward-compatibility
//! with new sub-object types.
//!
//! ## Design: `Resource`-derived types
//!
//! RFC 9553 §1.4.4 defines the abstract `Resource` common fields
//! (`@type`, `kind`, `uri`, `mediaType`, `contexts`, `pref`, `label`).
//! Five concrete types extend `Resource`:
//! [`Calendar`], [`CryptoKey`], [`Directory`], [`Link`], [`Media`].
//! Each embeds the common fields directly because the RFC defines the
//! inheritance for documentation only — the wire format is a flat
//! object per concrete type.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── Common helpers ────────────────────────────────────────────────────────────

/// JSContact `Id` data type (RFC 9553 §1.4.1).
///
/// A string of 1–255 octets containing only base64url-safe characters
/// (`A-Z`, `a-z`, `0-9`, `-`, `_`). JSContact `Id` values are NOT
/// JMAP `Id` values: validation of the character set is left to the
/// caller because this crate has no JMAP dependency.
///
/// Modelled as a transparent newtype around `String` so that wire JSON
/// for fields typed `Id` looks identical to a bare `String`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JsContactId(pub String);

impl From<String> for JsContactId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for JsContactId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for JsContactId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ── Name and NameComponent (RFC 9553 §2.2.1) ──────────────────────────────────

/// The name of the entity represented by a Card (RFC 9553 §2.2.1).
///
/// At least one of [`components`](Self::components) or [`full`](Self::full)
/// must be set per the RFC; this is not enforced at the type level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name {
    /// Object type discriminator; always `"Name"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The components making up this name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<NameComponent>>,

    /// Whether the components are ordered (default `false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_ordered: Option<bool>,

    /// The default separator to insert between component values when
    /// concatenating them; only valid when `is_ordered` is `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_separator: Option<String>,

    /// The full name representation of the Name. Must be set if
    /// `components` is not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,

    /// Sort-as overrides: `kind` → verbatim string to compare.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_as: Option<HashMap<String, String>>,

    /// The script used by the `phonetic` property on components.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic_script: Option<String>,

    /// The phonetic system used by the `phonetic` property on components.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic_system: Option<String>,
}

/// A single component of a [`Name`] (RFC 9553 §2.2.1.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameComponent {
    /// Object type discriminator; always `"NameComponent"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The component value (e.g. `"Vincent"`).
    pub value: String,

    /// The kind of name component: `"title"`, `"given"`, `"given2"`,
    /// `"surname"`, `"surname2"`, `"credential"`, `"generation"`, or
    /// `"separator"`.
    pub kind: String,

    /// Phonetic pronunciation of the component. If set, the parent
    /// [`Name`] must set at least one of `phonetic_script` / `phonetic_system`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic: Option<String>,
}

// ── Nickname (RFC 9553 §2.2.2) ────────────────────────────────────────────────

/// A nickname for the entity represented by a Card (RFC 9553 §2.2.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nickname {
    /// Object type discriminator; always `"Nickname"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The nickname.
    pub name: String,

    /// Contexts in which to use the nickname (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,
}

// ── Organization and OrgUnit (RFC 9553 §2.2.3) ────────────────────────────────

/// A company or organization name associated with a Card (RFC 9553 §2.2.3).
///
/// At least one of [`name`](Self::name) or [`units`](Self::units) must be
/// set per the RFC; this is not enforced at the type level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    /// Object type discriminator; always `"Organization"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The name of the organization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Organizational units, ordered descending by hierarchy. If set,
    /// must contain at least one entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<Vec<OrgUnit>>,

    /// The verbatim string for lexicographic sort by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_as: Option<String>,

    /// Contexts in which association applies (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,
}

/// An organizational unit within an [`Organization`] (RFC 9553 §2.2.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgUnit {
    /// Object type discriminator; always `"OrgUnit"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The name of the unit.
    pub name: String,

    /// The verbatim string for lexicographic sort within this level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_as: Option<String>,
}

// ── SpeakToAs and Pronouns (RFC 9553 §2.2.4) ──────────────────────────────────

/// How to address or refer to the entity represented by a Card
/// (RFC 9553 §2.2.4).
///
/// At least one of [`grammatical_gender`](Self::grammatical_gender) or
/// [`pronouns`](Self::pronouns) must be set per the RFC.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakToAs {
    /// Object type discriminator; always `"SpeakToAs"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Grammatical gender to use in salutations: `"animate"`, `"common"`,
    /// `"feminine"`, `"inanimate"`, `"masculine"`, or `"neuter"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammatical_gender: Option<String>,

    /// Map of pronoun [`Id`](JsContactId) → [`Pronouns`] object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pronouns: Option<HashMap<String, Pronouns>>,
}

/// A pronouns entry (RFC 9553 §2.2.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pronouns {
    /// Object type discriminator; always `"Pronouns"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The pronouns (free-form, e.g. `"she/her"`, `"they/them/theirs"`).
    pub pronouns: String,

    /// Contexts in which to use these pronouns (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,
}

// ── Title (RFC 9553 §2.2.5) ───────────────────────────────────────────────────

/// A job title or functional position (RFC 9553 §2.2.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Title {
    /// Object type discriminator; always `"Title"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The title or role name.
    pub name: String,

    /// `"title"` (default) or `"role"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// The [`JsContactId`] of the organization in which this title is held.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<JsContactId>,
}

// ── EmailAddress (RFC 9553 §2.3.1) ────────────────────────────────────────────

/// An email address (RFC 9553 §2.3.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAddress {
    /// Object type discriminator; always `"EmailAddress"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The email address. Must be an RFC 5322 addr-spec.
    pub address: String,

    /// Contexts in which to use the address (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Custom label for the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── OnlineService (RFC 9553 §2.3.2) ───────────────────────────────────────────

/// An online service (messaging service, social media, etc.) (RFC 9553 §2.3.2).
///
/// At least one of [`uri`](Self::uri) or [`user`](Self::user) must be set
/// per the RFC; this is not enforced at the type level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineService {
    /// Object type discriminator; always `"OnlineService"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Name of the online service or protocol (e.g. `"GitHub"`, `"Mastodon"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    /// Identifier for the entity at this service. Must be a URI (RFC 3986).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Username at the service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Contexts in which to use the service (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Custom label for the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── Phone (RFC 9553 §2.3.3) ───────────────────────────────────────────────────

/// A phone number (RFC 9553 §2.3.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phone {
    /// Object type discriminator; always `"Phone"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The phone number, either as a URI (typically `tel:` or `sip:`) or
    /// as free text.
    pub number: String,

    /// Feature flags (key → `true`): `"mobile"`, `"voice"`, `"text"`,
    /// `"video"`, `"main-number"`, `"textphone"`, `"fax"`, `"pager"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<HashMap<String, bool>>,

    /// Contexts in which to use the number (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Custom label for the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── LanguagePref (RFC 9553 §2.3.4) ────────────────────────────────────────────

/// A preferred language (RFC 9553 §2.3.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguagePref {
    /// Object type discriminator; always `"LanguagePref"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// BCP 47 language tag.
    pub language: String,

    /// Contexts in which to use the language (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,
}

// ── Calendar (RFC 9553 §2.4.1; extends Resource §1.4.4) ───────────────────────

/// A calendaring resource (RFC 9553 §2.4.1).
///
/// Extends the abstract [Resource](crate#design-resource-derived-types)
/// type with a mandatory `kind` value of either `"calendar"` or `"freeBusy"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    /// Object type discriminator; always `"Calendar"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// `"calendar"` or `"freeBusy"`. Mandatory per RFC 9553 §2.4.1, but
    /// modelled as `Option` to permit partial-response deserialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// The resource URI (RFC 3986). Mandatory on the wire per
    /// RFC 9553 §1.4.4, but modelled as `Option` to permit
    /// partial-response deserialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// IANA media type of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Contexts in which to use the resource (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Custom label for the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── SchedulingAddress (RFC 9553 §2.4.2) ───────────────────────────────────────

/// An iTIP scheduling address (RFC 9553 §2.4.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulingAddress {
    /// Object type discriminator; always `"SchedulingAddress"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The scheduling URI (RFC 3986).
    pub uri: String,

    /// Contexts in which to use the scheduling address (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Custom label for the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── Address and AddressComponent (RFC 9553 §2.5.1) ────────────────────────────

/// A postal or geographic address (RFC 9553 §2.5.1).
///
/// At least one of `components`, `coordinates`, `country_code`, `full`,
/// or `time_zone` must be set per the RFC; this is not enforced at the
/// type level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    /// Object type discriminator; always `"Address"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The components making up this address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<AddressComponent>>,

    /// Whether components are ordered (default `false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_ordered: Option<bool>,

    /// ISO 3166-1 Alpha-2 country code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,

    /// `geo:` URI for the address (RFC 5870).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<String>,

    /// IANA time zone name for the address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,

    /// Contexts (key → `true`); extra keys beyond common contexts are
    /// `"billing"` and `"delivery"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// The full address as a single string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,

    /// Default separator between component values when concatenating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_separator: Option<String>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Phonetic script for component phonetics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic_script: Option<String>,

    /// Phonetic system for component phonetics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic_system: Option<String>,
}

/// A single component of an [`Address`] (RFC 9553 §2.5.1.2).
///
/// Enumerated `kind` values include `"room"`, `"apartment"`, `"floor"`,
/// `"building"`, `"number"`, `"name"`, `"block"`, `"subdistrict"`,
/// `"district"`, `"locality"`, `"region"`, `"postcode"`, `"country"`,
/// `"direction"`, `"landmark"`, `"postOfficeBox"`, `"separator"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressComponent {
    /// Object type discriminator; always `"AddressComponent"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The component value.
    pub value: String,

    /// The kind of address component (see type-level doc for enumerated values).
    pub kind: String,

    /// Phonetic pronunciation. If set, parent [`Address`] must set at
    /// least one of `phonetic_script` / `phonetic_system`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic: Option<String>,
}

// ── CryptoKey (RFC 9553 §2.6.1; extends Resource §1.4.4) ──────────────────────

/// A cryptographic key or certificate associated with a Card
/// (RFC 9553 §2.6.1). Extends the abstract Resource type.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoKey {
    /// Object type discriminator; always `"CryptoKey"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Kind of resource (optional for CryptoKey per RFC 9553 §2.6.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// The resource URI (RFC 3986). Mandatory per RFC 9553 §1.4.4 but
    /// modelled as `Option` for partial-response deserialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// IANA media type of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Contexts in which to use the resource (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Custom label for the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── Directory (RFC 9553 §2.6.2; extends Resource §1.4.4) ──────────────────────

/// A directory service associated with a Card (RFC 9553 §2.6.2).
///
/// Extends the abstract Resource type with a mandatory `kind` value of
/// either `"directory"` or `"entry"`, and an extra `list_as` ordering hint.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Directory {
    /// Object type discriminator; always `"Directory"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// `"directory"` or `"entry"`. Mandatory per RFC 9553 §2.6.2,
    /// modelled as `Option` for partial-response deserialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// The resource URI (RFC 3986). Mandatory per RFC 9553 §1.4.4 but
    /// modelled as `Option` for partial-response deserialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// IANA media type of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Contexts in which to use the resource (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Custom label for the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Position in the list of Directory objects of the same `kind`
    /// (RFC 9553 §2.6.2). Must be > 0 when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_as: Option<u32>,
}

// ── Link (RFC 9553 §2.6.3; extends Resource §1.4.4) ───────────────────────────

/// A generic resource link associated with a Card (RFC 9553 §2.6.3).
///
/// Extends the abstract Resource type. The `kind` value is optional;
/// when set, the only enumerated value is `"contact"`.
///
/// Distinct from JSCalendar's `Link` type ([`jmap_jscalendar_types::Link`](https://docs.rs/jmap-jscalendar-types));
/// the two are unrelated wire-format types.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// Object type discriminator; always `"Link"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Optional `"contact"` discriminator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// The resource URI (RFC 3986). Mandatory per RFC 9553 §1.4.4 but
    /// modelled as `Option` for partial-response deserialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// IANA media type of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Contexts in which to use the resource (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Custom label for the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── Media (RFC 9553 §2.6.4; extends Resource §1.4.4) ──────────────────────────

/// A media resource associated with a Card (RFC 9553 §2.6.4).
///
/// Extends the abstract Resource type with a mandatory `kind` value of
/// `"photo"`, `"sound"`, or `"logo"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    /// Object type discriminator; always `"Media"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// `"photo"`, `"sound"`, or `"logo"`. Mandatory per RFC 9553 §2.6.4,
    /// modelled as `Option` for partial-response deserialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// The resource URI (RFC 3986). Mandatory per RFC 9553 §1.4.4 but
    /// modelled as `Option` for partial-response deserialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// IANA media type of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Contexts in which to use the resource (key → `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    /// Preference order in 1..=100 (lower = more preferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    /// Custom label for the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── Anniversary, PartialDate, Timestamp (RFC 9553 §2.8.1) ─────────────────────

/// A complete or partial Gregorian calendar date (RFC 9553 §2.8.1).
///
/// Used by [`Anniversary`]. Any of `year`, `month`, `day` may be absent,
/// representing a partial date; `month` requires either `year` or `day`,
/// and `day` requires `month`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialDate {
    /// Object type discriminator; always `"PartialDate"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Calendar year.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,

    /// Calendar month, 1..=12.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub month: Option<u32>,

    /// Calendar day of month, 1..=31.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<u32>,

    /// Calendar system (lowercase CLDR name or vendor-specific value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_scale: Option<String>,
}

/// A UTC point in time (RFC 9553 §2.8.1).
///
/// Used by [`Anniversary`] as one of the two alternative `date` value
/// shapes (the other being [`PartialDate`]).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timestamp {
    /// Object type discriminator; required to be `"Timestamp"` on the
    /// wire when the value is used as an `Anniversary.date` (because the
    /// default-type for that field is `PartialDate`; explicit `@type`
    /// is what selects this variant).
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The UTC date-time.
    pub utc: String,
}

/// The date value of an [`Anniversary`] — either a [`PartialDate`] or a
/// [`Timestamp`] (RFC 9553 §2.8.1).
///
/// Wire selection follows JSContact's `@type` discriminator rules
/// (RFC 9553 §1.3.4): for the default type ([`PartialDate`]), the
/// `@type` discriminator may be omitted; for [`Timestamp`], it must be
/// `"Timestamp"`.
///
/// An [`Unknown`](Self::Unknown) variant preserves any other shape as
/// raw JSON for round-trip fidelity.
///
/// Serde is implemented manually because `#[serde(tag = "@type", other)]`
/// with tuple variants is not supported by the derive macro.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum AnniversaryDate {
    /// A partial Gregorian date (the default per RFC 9553 §2.8.1).
    PartialDate(PartialDate),
    /// An absolute UTC timestamp.
    Timestamp(Timestamp),
    /// Any other shape; preserved opaquely.
    Unknown(serde_json::Value),
}

impl Serialize for AnniversaryDate {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            AnniversaryDate::PartialDate(d) => d.serialize(s),
            AnniversaryDate::Timestamp(t) => t.serialize(s),
            AnniversaryDate::Unknown(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for AnniversaryDate {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Deserialize into an intermediate Value, then dispatch on @type.
        let v = serde_json::Value::deserialize(d)?;
        let tag = v.get("@type").and_then(|t| t.as_str()).unwrap_or("");
        match tag {
            "Timestamp" => {
                let t: Timestamp = serde_json::from_value(v).map_err(serde::de::Error::custom)?;
                Ok(AnniversaryDate::Timestamp(t))
            }
            // RFC 9553 §2.8.1: PartialDate is the default type when @type is
            // absent. Treat empty or "PartialDate" tags as PartialDate;
            // anything else is preserved opaquely.
            "" | "PartialDate" => {
                let d: PartialDate = serde_json::from_value(v).map_err(serde::de::Error::custom)?;
                Ok(AnniversaryDate::PartialDate(d))
            }
            _ => Ok(AnniversaryDate::Unknown(v)),
        }
    }
}

/// A memorable date or event (RFC 9553 §2.8.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anniversary {
    /// Object type discriminator; always `"Anniversary"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// `"birth"`, `"death"`, or `"wedding"`.
    pub kind: String,

    /// The date of the anniversary — either a [`PartialDate`] or a
    /// [`Timestamp`].
    pub date: AnniversaryDate,

    /// An associated address (e.g. place of birth or death).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<Address>,
}

// ── Note and Author (RFC 9553 §2.8.3) ─────────────────────────────────────────

/// A free-text note associated with a Card (RFC 9553 §2.8.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Object type discriminator; always `"Note"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The free-text value of this note.
    pub note: String,

    /// UTC date-time when the note was created (RFC 9553 §1.4.5 UTCDateTime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,

    /// The author of this note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,
}

/// The author of a [`Note`] (RFC 9553 §2.8.3).
///
/// At least one property other than `@type` must be set per the RFC;
/// this is not enforced at the type level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    /// Object type discriminator; always `"Author"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Name of the author.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// URI that identifies the author.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

// ── PersonalInfo (RFC 9553 §2.8.4) ────────────────────────────────────────────

/// Personal information such as an expertise, hobby, or interest
/// (RFC 9553 §2.8.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalInfo {
    /// Object type discriminator; always `"PersonalInfo"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// `"expertise"`, `"hobby"`, or `"interest"`.
    pub kind: String,

    /// The actual information value.
    pub value: String,

    /// Level of engagement: `"high"`, `"medium"`, or `"low"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,

    /// Position in the list of PersonalInfo entries of the same kind.
    /// Must be > 0 when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_as: Option<u32>,

    /// Custom label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── Relation (RFC 9553 §2.1.8) ────────────────────────────────────────────────

/// A relationship to another Card (RFC 9553 §2.1.8).
///
/// This is the value type for the `relatedTo` property on a `ContactCard`.
/// Each map key is the `uid` of the related Card; each value is a
/// `Relation` object describing the relationship.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    /// Object type discriminator; always `"Relation"` on the wire.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Set of relation types (key → `true`). Initial enumerated values:
    /// `"acquaintance"`, `"agent"`, `"child"`, `"co-resident"`,
    /// `"co-worker"`, `"colleague"`, `"contact"`, `"crush"`, `"date"`,
    /// `"emergency"`, `"friend"`, `"kin"`, `"me"`, `"met"`, `"muse"`,
    /// `"neighbor"`, `"parent"`, `"sibling"`, `"spouse"`, `"sweetheart"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<HashMap<String, bool>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! Round-trip tests using RFC 9553 example JSON as the oracle.
    //!
    //! Each test loads a hand-typed JSON fixture taken verbatim from a
    //! figure in RFC 9553, parses it into the typed struct, re-serializes,
    //! and checks the round-trip preserves the data. The RFC is the
    //! oracle — expected values are never derived from the code under
    //! test.

    use super::*;
    use serde_json::json;

    fn assert_roundtrip<T>(value: serde_json::Value)
    where
        T: serde::de::DeserializeOwned + Serialize,
    {
        let typed: T = serde_json::from_value(value.clone())
            .unwrap_or_else(|e| panic!("deserialize failed: {e}\ninput: {value}"));
        let back = serde_json::to_value(&typed).unwrap_or_else(|e| panic!("serialize failed: {e}"));
        assert_eq!(back, value, "round-trip mismatch");
    }

    // ── Name + NameComponent (RFC 9553 Figure 16 / §2.2.1) ────────────────

    #[test]
    fn name_roundtrip_figure_16() {
        // RFC 9553 Figure 16: "Vincent van Gogh"
        let v = json!({
            "components": [
                { "kind": "given", "value": "Vincent" },
                { "kind": "surname", "value": "van Gogh" }
            ],
            "isOrdered": true
        });
        assert_roundtrip::<Name>(v);
    }

    #[test]
    fn name_roundtrip_figure_19() {
        // RFC 9553 Figure 19: sortAs example
        let v = json!({
            "components": [
                { "kind": "given", "value": "Robert" },
                { "kind": "given2", "value": "Pau" },
                { "kind": "surname", "value": "Shou Chang" }
            ],
            "sortAs": {
                "surname": "Pau Shou Chang",
                "given": "Robert"
            },
            "isOrdered": true
        });
        assert_roundtrip::<Name>(v);
    }

    // ── Nickname (RFC 9553 Figure 21 / §2.2.2) ────────────────────────────

    #[test]
    fn nickname_roundtrip_figure_21() {
        let v = json!({
            "name": "Johnny"
        });
        assert_roundtrip::<Nickname>(v);
    }

    // ── Organization + OrgUnit (RFC 9553 Figure 22 / §2.2.3) ──────────────

    #[test]
    fn organization_roundtrip_figure_22() {
        let v = json!({
            "name": "ABC, Inc.",
            "units": [
                { "name": "North American Division" },
                { "name": "Marketing" }
            ],
            "sortAs": "ABC"
        });
        assert_roundtrip::<Organization>(v);
    }

    // ── SpeakToAs + Pronouns (RFC 9553 Figure 23 / §2.2.4) ────────────────

    #[test]
    fn speak_to_as_roundtrip_figure_23() {
        let v = json!({
            "grammaticalGender": "neuter",
            "pronouns": {
                "k19": {
                    "pronouns": "they/them",
                    "pref": 2
                },
                "k32": {
                    "pronouns": "xe/xir",
                    "pref": 1
                }
            }
        });
        assert_roundtrip::<SpeakToAs>(v);
    }

    // ── Title (RFC 9553 Figure 24 / §2.2.5) ───────────────────────────────

    #[test]
    fn title_roundtrip_figure_24_title() {
        let v = json!({
            "kind": "title",
            "name": "Research Scientist"
        });
        assert_roundtrip::<Title>(v);
    }

    #[test]
    fn title_roundtrip_figure_24_role() {
        let v = json!({
            "kind": "role",
            "name": "Project Leader",
            "organizationId": "o2"
        });
        assert_roundtrip::<Title>(v);
    }

    // ── EmailAddress (RFC 9553 Figure 25 / §2.3.1) ────────────────────────

    #[test]
    fn email_address_roundtrip_figure_25_work() {
        let v = json!({
            "contexts": { "work": true },
            "address": "jqpublic@xyz.example.com"
        });
        assert_roundtrip::<EmailAddress>(v);
    }

    #[test]
    fn email_address_roundtrip_figure_25_pref() {
        let v = json!({
            "address": "jane_doe@example.com",
            "pref": 1
        });
        assert_roundtrip::<EmailAddress>(v);
    }

    // ── OnlineService (RFC 9553 Figure 26 / §2.3.2) ───────────────────────

    #[test]
    fn online_service_roundtrip_figure_26() {
        let v = json!({
            "service": "Mastodon",
            "user": "@alice@example2.com",
            "uri": "https://example2.com/@alice"
        });
        assert_roundtrip::<OnlineService>(v);
    }

    // ── Phone (RFC 9553 Figure 27 / §2.3.3) ───────────────────────────────

    #[test]
    fn phone_roundtrip_figure_27() {
        let v = json!({
            "contexts": { "private": true },
            "features": { "voice": true },
            "number": "tel:+1-555-555-5555;ext=5555",
            "pref": 1
        });
        assert_roundtrip::<Phone>(v);
    }

    // ── LanguagePref (RFC 9553 Figure 28 / §2.3.4) ────────────────────────

    #[test]
    fn language_pref_roundtrip_figure_28() {
        let v = json!({
            "language": "en",
            "contexts": { "work": true },
            "pref": 1
        });
        assert_roundtrip::<LanguagePref>(v);
    }

    // ── Calendar (RFC 9553 Figure 29 / §2.4.1) ────────────────────────────

    #[test]
    fn calendar_roundtrip_figure_29_calendar() {
        let v = json!({
            "kind": "calendar",
            "uri": "webcal://calendar.example.com/calA.ics"
        });
        assert_roundtrip::<Calendar>(v);
    }

    #[test]
    fn calendar_roundtrip_figure_29_freebusy() {
        let v = json!({
            "kind": "freeBusy",
            "uri": "https://calendar.example.com/busy/project-a"
        });
        assert_roundtrip::<Calendar>(v);
    }

    // ── SchedulingAddress (RFC 9553 Figure 30 / §2.4.2) ───────────────────

    #[test]
    fn scheduling_address_roundtrip_figure_30() {
        let v = json!({
            "uri": "mailto:janedoe@example.com"
        });
        assert_roundtrip::<SchedulingAddress>(v);
    }

    // ── Address (RFC 9553 Figure 31 / §2.5.1) ─────────────────────────────

    #[test]
    fn address_roundtrip_figure_31() {
        let v = json!({
            "contexts": { "work": true },
            "components": [
                { "kind": "number", "value": "54321" },
                { "kind": "separator", "value": " " },
                { "kind": "name", "value": "Oak St" },
                { "kind": "locality", "value": "Reston" },
                { "kind": "region", "value": "VA" },
                { "kind": "separator", "value": " " },
                { "kind": "postcode", "value": "20190" },
                { "kind": "country", "value": "USA" }
            ],
            "countryCode": "US",
            "defaultSeparator": ", ",
            "isOrdered": true
        });
        assert_roundtrip::<Address>(v);
    }

    // ── CryptoKey (RFC 9553 Figure 34 / §2.6.1) ───────────────────────────

    #[test]
    fn crypto_key_roundtrip_figure_34() {
        let v = json!({
            "uri": "https://www.example.com/keys/jdoe.cer"
        });
        assert_roundtrip::<CryptoKey>(v);
    }

    // ── Directory (RFC 9553 Figure 36 / §2.6.2) ───────────────────────────

    #[test]
    fn directory_roundtrip_figure_36_entry() {
        let v = json!({
            "kind": "entry",
            "uri": "https://dir.example.com/addrbook/jdoe/Jean%20Dupont.vcf"
        });
        assert_roundtrip::<Directory>(v);
    }

    #[test]
    fn directory_roundtrip_figure_36_directory() {
        let v = json!({
            "kind": "directory",
            "uri": "ldap://ldap.example/o=Example%20Tech,ou=Engineering",
            "pref": 1
        });
        assert_roundtrip::<Directory>(v);
    }

    // ── Link (RFC 9553 Figure 37 / §2.6.3) ────────────────────────────────

    #[test]
    fn link_roundtrip_figure_37() {
        let v = json!({
            "kind": "contact",
            "uri": "mailto:contact@example.com",
            "pref": 1
        });
        assert_roundtrip::<Link>(v);
    }

    // ── Media (RFC 9553 Figure 38 / §2.6.4) ───────────────────────────────

    #[test]
    fn media_roundtrip_figure_38_sound() {
        let v = json!({
            "kind": "sound",
            "uri": "CID:JOHNQ.part8.19960229T080000.xyzMail@example.com"
        });
        assert_roundtrip::<Media>(v);
    }

    #[test]
    fn media_roundtrip_figure_38_logo() {
        let v = json!({
            "kind": "logo",
            "uri": "https://www.example.com/pub/logos/abccorp.jpg"
        });
        assert_roundtrip::<Media>(v);
    }

    // ── Anniversary + PartialDate + Timestamp (RFC 9553 Figure 41 / §2.8.1) ──

    #[test]
    fn anniversary_roundtrip_figure_41_partial_date() {
        // Figure 41, k8 entry. PartialDate is the default; @type omitted.
        let v = json!({
            "kind": "birth",
            "date": {
                "year": 1953,
                "month": 4,
                "day": 15
            }
        });
        assert_roundtrip::<Anniversary>(v);
    }

    #[test]
    fn anniversary_roundtrip_figure_41_timestamp() {
        // Figure 41, k9 entry. Timestamp requires explicit @type.
        let v = json!({
            "kind": "death",
            "date": {
                "@type": "Timestamp",
                "utc": "2019-10-15T23:10:00Z"
            },
            "place": {
                "full": "4445 Tree Street\nNew England, ND 58647\nUSA"
            }
        });
        assert_roundtrip::<Anniversary>(v);
    }

    #[test]
    fn anniversary_date_unknown_preserves_opaque() {
        // A future-spec @type value the crate doesn't recognise must be
        // preserved opaquely so a Card object containing it round-trips.
        let v = json!({
            "kind": "birth",
            "date": {
                "@type": "FuturisticDateShape",
                "stardate": 41153.7
            }
        });
        let anniv: Anniversary = serde_json::from_value(v.clone()).unwrap();
        assert!(matches!(anniv.date, AnniversaryDate::Unknown(_)));
        let back = serde_json::to_value(&anniv).unwrap();
        assert_eq!(back, v);
    }

    // ── Note + Author (RFC 9553 Figure 43 / §2.8.3) ───────────────────────

    #[test]
    fn note_roundtrip_figure_43() {
        let v = json!({
            "note": "Open office hours are 1600 to 1715 EST, Mon-Fri",
            "created": "2022-11-23T15:01:32Z",
            "author": {
                "name": "John"
            }
        });
        assert_roundtrip::<Note>(v);
    }

    // ── PersonalInfo (RFC 9553 Figure 44 / §2.8.4) ────────────────────────

    #[test]
    fn personal_info_roundtrip_figure_44_expertise() {
        let v = json!({
            "kind": "expertise",
            "value": "chemistry",
            "level": "high"
        });
        assert_roundtrip::<PersonalInfo>(v);
    }

    #[test]
    fn personal_info_roundtrip_figure_44_hobby() {
        let v = json!({
            "kind": "hobby",
            "value": "reading",
            "level": "high"
        });
        assert_roundtrip::<PersonalInfo>(v);
    }

    // ── Relation (RFC 9553 Figure 13 / §2.1.8) ────────────────────────────

    #[test]
    fn relation_roundtrip_figure_13_friend() {
        let v = json!({
            "relation": { "friend": true }
        });
        assert_roundtrip::<Relation>(v);
    }

    #[test]
    fn relation_roundtrip_figure_13_empty() {
        let v = json!({
            "relation": {}
        });
        assert_roundtrip::<Relation>(v);
    }

    // ── JsContactId transparent newtype ───────────────────────────────────

    #[test]
    fn jscontact_id_is_transparent_string() {
        let id = JsContactId::from("abc123");
        let v = serde_json::to_value(&id).unwrap();
        assert_eq!(v, json!("abc123"));
        let back: JsContactId = serde_json::from_value(v).unwrap();
        assert_eq!(back, id);
    }
}
