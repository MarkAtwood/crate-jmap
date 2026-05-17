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
//! round-trip unchanged.
//!
//! Mandatory fields per the RFC are normally kept as bare types (not
//! `Option`) to express the requirement at the type level — callers
//! building a fresh sub-object must populate them. The exception is
//! the `Resource`-derived types ([`Calendar`], [`CryptoKey`],
//! [`Directory`], [`Link`], [`Media`]), whose RFC-mandatory `kind` and
//! `uri` fields are modelled as `Option` to permit partial-response
//! deserialization (a JMAP client requesting `properties: ["kind"]` on
//! a `Calendar` legitimately receives a JSON object with no `uri` and
//! must round-trip it unchanged).
//!
//! The trade-off: callers can construct, e.g.,
//! `Calendar { kind: None, uri: None, ... }` and serialize a wire
//! object that no spec-conformant peer can validate. The type system
//! does not catch this; the kit's posture is "types model the wire
//! shape; semantic validity is the consumer's job" (see the kit-vs-jig
//! section in the workspace `AGENTS.md`). Callers building a fresh
//! value for emission MUST populate the mandatory-on-wire fields
//! themselves before serializing.
//!
//! Mandatory-on-wire fields modelled as `Option` (8 sites across 5
//! types), gathered into one place so each consumer does not have to
//! re-derive them from the per-field rustdoc:
//!
//! | Struct | Mandatory field | RFC section |
//! |---|---|---|
//! | [`Calendar`] | `kind` | RFC 9553 §2.4.1 |
//! | [`Calendar`] | `uri` | RFC 9553 §1.4.4 |
//! | [`CryptoKey`] | `uri` | RFC 9553 §1.4.4 |
//! | [`Directory`] | `kind` | RFC 9553 §2.6.2 |
//! | [`Directory`] | `uri` | RFC 9553 §1.4.4 |
//! | [`Link`] | `uri` | RFC 9553 §1.4.4 |
//! | [`Media`] | `kind` | RFC 9553 §2.6.4 |
//! | [`Media`] | `uri` | RFC 9553 §1.4.4 |
//!
//! ## Design: cross-field invariants are not type-enforced
//!
//! Six structs carry a cross-field "at least one of X, Y must be set"
//! constraint at the rustdoc level that the Rust type system does not
//! enforce. The kit's posture is "types model the wire shape; semantic
//! validity is the consumer's job" (see the kit-vs-jig section in the
//! workspace `AGENTS.md`). Encoding these constraints in the type system
//! would diverge from that posture and force the partial-response
//! `Option` modelling into a corner.
//!
//! Callers building a fresh value for emission MUST validate the
//! constraint themselves before serializing. The constraints, gathered
//! into one place so each consumer does not have to re-derive them
//! from the per-struct rustdoc:
//!
//! | Struct | Constraint |
//! |---|---|
//! | [`Name`] | at least one of `components` or `full` |
//! | [`Organization`] | at least one of `name` or `units` |
//! | [`SpeakToAs`] | at least one of `grammatical_gender` or `pronouns` |
//! | [`OnlineService`] | at least one of `uri` or `user` |
//! | [`Address`] | at least one of `components`, `coordinates`, `country_code`, `full`, or `time_zone` |
//! | [`Author`] | at least one property other than `@type` |
//!
//! Deserialize does not reject inputs that violate these constraints —
//! the kit accepts partial-response inputs that legitimately omit
//! fields. The constraint applies only to emitting fresh values. See
//! `bd:JMAP-sgrr.30`.
//!
//! ## Design: `@type` discriminator
//!
//! Every RFC 9553 sub-object has an `@type` discriminator on the wire.
//! The Rust field is named `at_type: Option<String>` and renamed to
//! `"@type"` via serde attributes, with `default` and
//! `skip_serializing_if = "Option::is_none"`. The field is modelled as
//! `Option<String>` (not bare `String`) because RFC 9553 §1.3.4 permits
//! omitting `@type` whenever the type is implied by context — most
//! notably when the value is in a `defaultType` position (see
//! [`Anniversary::date`] / [`AnniversaryDate`] for the worked example).
//! The value type is `String` (not an enum) to preserve forward-
//! compatibility with new sub-object types.
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

// ── Name and NameComponent (RFC 9553 §2.2.1) ──────────────────────────────────

/// The name of the entity represented by a Card (RFC 9553 §2.2.1).
///
/// At least one of [`components`](Self::components) or [`full`](Self::full)
/// must be set per the RFC; this is not enforced at the type level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name {
    /// Object type discriminator; SHOULD be `"Name"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single component of a [`Name`] (RFC 9553 §2.2.1.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameComponent {
    /// Object type discriminator; SHOULD be `"NameComponent"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Nickname (RFC 9553 §2.2.2) ────────────────────────────────────────────────

/// A nickname for the entity represented by a Card (RFC 9553 §2.2.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nickname {
    /// Object type discriminator; SHOULD be `"Nickname"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    /// Object type discriminator; SHOULD be `"Organization"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An organizational unit within an [`Organization`] (RFC 9553 §2.2.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgUnit {
    /// Object type discriminator; SHOULD be `"OrgUnit"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The name of the unit.
    pub name: String,

    /// The verbatim string for lexicographic sort within this level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_as: Option<String>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    /// Object type discriminator; SHOULD be `"SpeakToAs"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Grammatical gender to use in salutations: `"animate"`, `"common"`,
    /// `"feminine"`, `"inanimate"`, `"masculine"`, or `"neuter"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammatical_gender: Option<String>,

    /// Map of pronoun Id (per RFC 9553 §1.4.1) → [`Pronouns`] object.
    /// Keys are bare `String` per the workspace policy that JSContact
    /// `Id` references on the wire are modelled as `String`; validation
    /// of the character set (`A-Z`, `a-z`, `0-9`, `-`, `_`, length
    /// 1–255) is the caller's responsibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pronouns: Option<HashMap<String, Pronouns>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A pronouns entry (RFC 9553 §2.2.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pronouns {
    /// Object type discriminator; SHOULD be `"Pronouns"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Title (RFC 9553 §2.2.5) ───────────────────────────────────────────────────

/// A job title or functional position (RFC 9553 §2.2.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Title {
    /// Object type discriminator; SHOULD be `"Title"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The title or role name.
    pub name: String,

    /// `"title"` (default) or `"role"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// The JSContact `Id` (per RFC 9553 §1.4.1) of the organization in
    /// which this title is held. Modelled as `String`; validation of
    /// the character set is the caller's responsibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── EmailAddress (RFC 9553 §2.3.1) ────────────────────────────────────────────

/// An email address (RFC 9553 §2.3.1).
///
/// Distinct from the JMAP Mail RFC 8621 §2 binding type
/// [`jmap_mail_types::EmailAddress`](https://docs.rs/jmap-mail-types):
/// that type carries an RFC 5322 mailbox (`name` + `email`) and appears
/// in `Email.from` / `Email.to` etc., whereas this `EmailAddress` is a
/// JSContact sub-object embedded in a `ContactCard.emails` map.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAddress {
    /// Object type discriminator; SHOULD be `"EmailAddress"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    /// Object type discriminator; SHOULD be `"OnlineService"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Phone (RFC 9553 §2.3.3) ───────────────────────────────────────────────────

/// A phone number (RFC 9553 §2.3.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phone {
    /// Object type discriminator; SHOULD be `"Phone"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── LanguagePref (RFC 9553 §2.3.4) ────────────────────────────────────────────

/// A preferred language (RFC 9553 §2.3.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguagePref {
    /// Object type discriminator; SHOULD be `"LanguagePref"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Calendar (RFC 9553 §2.4.1; extends Resource §1.4.4) ───────────────────────

/// A calendaring resource (RFC 9553 §2.4.1).
///
/// Extends the abstract [Resource](crate#design-resource-derived-types)
/// type with a mandatory `kind` value of either `"calendar"` or `"freeBusy"`.
///
/// `kind` and `uri` are mandatory on the wire but modelled as `Option`
/// to permit partial-response deserialization; callers building a fresh
/// `Calendar` MUST populate both fields. See the crate-level
/// [Design: optional fields and `Option<...>`](crate#design-optional-fields-and-option)
/// section.
///
/// Distinct from the JMAP Calendars binding object
/// [`jmap_calendars_types::Calendar`](https://docs.rs/jmap-calendars-types):
/// that type is a top-level JMAP wire object with `id`, `name`,
/// `myRights`, `shareWith`, etc., whereas this `Calendar` is a JSContact
/// resource sub-object embedded in a `ContactCard`. The two wire shapes
/// are unrelated.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    /// Object type discriminator; SHOULD be `"Calendar"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── SchedulingAddress (RFC 9553 §2.4.2) ───────────────────────────────────────

/// An iTIP scheduling address (RFC 9553 §2.4.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulingAddress {
    /// Object type discriminator; SHOULD be `"SchedulingAddress"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Address and AddressComponent (RFC 9553 §2.5.1) ────────────────────────────

/// A postal or geographic address (RFC 9553 §2.5.1).
///
/// At least one of `components`, `coordinates`, `country_code`, `full`,
/// or `time_zone` must be set per the RFC; this is not enforced at the
/// type level.
///
/// Distinct from the JMAP Mail RFC 8621 §3.2 submission-address type
/// [`jmap_mail_types::Address`](https://docs.rs/jmap-mail-types):
/// that type is an RFC 5321 SMTP envelope address with `email` and
/// `parameters`, whereas this `Address` is a JSContact postal-address
/// sub-object embedded in a `ContactCard.addresses` map.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    /// Object type discriminator; SHOULD be `"Address"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single component of an [`Address`] (RFC 9553 §2.5.1.2).
///
/// Enumerated `kind` values include `"room"`, `"apartment"`, `"floor"`,
/// `"building"`, `"number"`, `"name"`, `"block"`, `"subdistrict"`,
/// `"district"`, `"locality"`, `"region"`, `"postcode"`, `"country"`,
/// `"direction"`, `"landmark"`, `"postOfficeBox"`, `"separator"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressComponent {
    /// Object type discriminator; SHOULD be `"AddressComponent"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── CryptoKey (RFC 9553 §2.6.1; extends Resource §1.4.4) ──────────────────────

/// A cryptographic key or certificate associated with a Card
/// (RFC 9553 §2.6.1). Extends the abstract Resource type.
///
/// `uri` is mandatory on the wire but modelled as `Option` to permit
/// partial-response deserialization; callers building a fresh
/// `CryptoKey` MUST populate it. See the crate-level
/// [Design: optional fields and `Option<...>`](crate#design-optional-fields-and-option)
/// section.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoKey {
    /// Object type discriminator; SHOULD be `"CryptoKey"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Directory (RFC 9553 §2.6.2; extends Resource §1.4.4) ──────────────────────

/// A directory service associated with a Card (RFC 9553 §2.6.2).
///
/// Extends the abstract Resource type with a mandatory `kind` value of
/// either `"directory"` or `"entry"`, and an extra `list_as` ordering hint.
///
/// `kind` and `uri` are mandatory on the wire but modelled as `Option`
/// to permit partial-response deserialization; callers building a fresh
/// `Directory` MUST populate both fields. See the crate-level
/// [Design: optional fields and `Option<...>`](crate#design-optional-fields-and-option)
/// section.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Directory {
    /// Object type discriminator; SHOULD be `"Directory"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Link (RFC 9553 §2.6.3; extends Resource §1.4.4) ───────────────────────────

/// A generic resource link associated with a Card (RFC 9553 §2.6.3).
///
/// Extends the abstract Resource type. The `kind` value is optional;
/// when set, the only enumerated value is `"contact"`.
///
/// `uri` is mandatory on the wire but modelled as `Option` to permit
/// partial-response deserialization; callers building a fresh `Link`
/// MUST populate it. See the crate-level
/// [Design: optional fields and `Option<...>`](crate#design-optional-fields-and-option)
/// section.
///
/// Distinct from JSCalendar's `Link` type ([`jmap_jscalendar_types::Link`](https://docs.rs/jmap-jscalendar-types));
/// the two are unrelated wire-format types.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// Object type discriminator; SHOULD be `"Link"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Media (RFC 9553 §2.6.4; extends Resource §1.4.4) ──────────────────────────

/// A media resource associated with a Card (RFC 9553 §2.6.4).
///
/// Extends the abstract Resource type with a mandatory `kind` value of
/// `"photo"`, `"sound"`, or `"logo"`.
///
/// `kind` and `uri` are mandatory on the wire but modelled as `Option`
/// to permit partial-response deserialization; callers building a fresh
/// `Media` MUST populate both fields. See the crate-level
/// [Design: optional fields and `Option<...>`](crate#design-optional-fields-and-option)
/// section.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    /// Object type discriminator; SHOULD be `"Media"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Anniversary, PartialDate, Timestamp (RFC 9553 §2.8.1) ─────────────────────

/// A complete or partial Gregorian calendar date (RFC 9553 §2.8.1).
///
/// Used by [`Anniversary`]. Any of `year`, `month`, `day` may be absent,
/// representing a partial date; `month` requires either `year` or `day`,
/// and `day` requires `month`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialDate {
    /// Object type discriminator; SHOULD be `"PartialDate"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A UTC point in time (RFC 9553 §2.8.1).
///
/// Used by [`Anniversary`] as one of the two alternative `date` value
/// shapes (the other being [`PartialDate`]).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timestamp {
    /// Object type discriminator; required to be `"Timestamp"` on the
    /// wire when the value is used as an `Anniversary.date` (because the
    /// default-type for that field is `PartialDate`; explicit `@type`
    /// is what selects this variant).
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The UTC date-time (RFC 9553 §1.4.5 `UTCDateTime`): an RFC 3339
    /// date-time string with the time-offset always `"Z"`, e.g.
    /// `"2022-05-22T03:30:00Z"`.
    ///
    /// Stored as bare `String` so deserialize accepts any peer-emitted
    /// value losslessly. The format is NOT validated at construction or
    /// deserialize time; callers that need the parsed value should pipe
    /// it through `chrono::DateTime::parse_from_rfc3339` or
    /// `time::OffsetDateTime::parse` themselves (per the workspace
    /// convention used by `jmap_types::UTCDate`, which carries the same
    /// parse-on-demand contract).
    pub utc: String,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
///
/// # Deserialize dispatch contract
///
/// - `@type` absent (no field at all) and `@type: "PartialDate"` both
///   route to the [`PartialDate`](Self::PartialDate) variant. Per RFC
///   9553 §1.3.4 the wire forms are interchangeable for the default
///   type; this enum treats them as a single variant.
/// - `@type: "Timestamp"` routes to the [`Timestamp`](Self::Timestamp)
///   variant.
/// - Any other `@type` string value (including the literal empty
///   string, which RFC 9553 does not define) routes to
///   [`Unknown`](Self::Unknown) with the original `serde_json::Value`
///   preserved for round-trip.
///
/// # Error context
///
/// Errors parsing the inner `PartialDate` or `Timestamp` body surface
/// as `serde::de::Error::custom` strings. They do not include the
/// path within a parent [`Anniversary`]; callers debugging a deeply-
/// nested `ContactCard` (from the consumer `jmap-contacts-types`
/// crate) should wrap the parse and add context at the call site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnniversaryDate {
    /// A partial Gregorian date (the default per RFC 9553 §2.8.1).
    PartialDate(PartialDate),
    /// An absolute UTC timestamp.
    Timestamp(Timestamp),
    /// Any other shape; preserved opaquely.
    ///
    /// Carries the raw `serde_json::Value` so a future-spec `@type`
    /// variant a JSContact extension server may emit round-trips
    /// losslessly through this crate, even though the kit cannot
    /// dispatch on it. Matches the workspace extras-preservation
    /// posture (see workspace `AGENTS.md`).
    ///
    /// **Do not remove this variant** to "simplify" the enum into
    /// `{PartialDate, Timestamp}` only — that would force a
    /// deserialize error on any spec-conformant input carrying a
    /// future `@type` value, silently losing data. The variant exists
    /// specifically to prevent that bug. See `bd:JMAP-sgrr.10`.
    ///
    /// # Construction precondition (Rust-side callers)
    ///
    /// Lossless round-trip through this variant requires that the
    /// wrapped [`serde_json::Value`] is **either**:
    ///
    /// 1. not a JSON object (any scalar, array, or `null`), **or**
    /// 2. a JSON object whose `@type` field is set to a string value
    ///    *outside* the set the deserializer dispatches on —
    ///    currently `{"PartialDate", "Timestamp"}` plus implicit
    ///    `@type` absence (which routes to `PartialDate` per
    ///    RFC 9553 §2.8.1's `defaultType` rule).
    ///
    /// Wrapping a `Value` that is an object **with** `@type` set to
    /// `"PartialDate"` or `"Timestamp"`, **or** an object with no
    /// `@type` at all (a bare `PartialDate` shape), will not survive
    /// a serialize → deserialize round trip as `Unknown`: the
    /// deserializer will re-dispatch on the recognised `@type` (or on
    /// `defaultType = PartialDate` if `@type` is absent) and produce
    /// the corresponding typed variant. Variant identity is lost,
    /// though the underlying field data is preserved through the
    /// typed shape.
    ///
    /// This is intentional: the dispatch contract documented above is
    /// driven entirely by `@type`; the variant is for shapes the
    /// dispatcher does not recognise. No spec-conformant RFC 9553
    /// emitter produces wire input that triggers this asymmetry — the
    /// concern is composing callers who hand-construct `Unknown(v)`
    /// without first checking `v`. See `bd:JMAP-sgrr.28`.
    Unknown(serde_json::Value),
}

impl Serialize for AnniversaryDate {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            AnniversaryDate::PartialDate(d) => d.serialize(s),
            // When emitting a `Timestamp` in `AnniversaryDate` position the
            // `@type` discriminator is what distinguishes the variant from
            // the default `PartialDate`; if the caller left `at_type` as
            // `None` we re-stamp it on the way out so the wire output is
            // unambiguous and survives a serialize→deserialize round trip.
            // A caller-provided `at_type` (even an odd value) is preserved
            // verbatim for forward-compat / faithful echo. See bd:JMAP-sgrr.29.
            AnniversaryDate::Timestamp(t) if t.at_type.is_none() => {
                let mut stamped = t.clone();
                stamped.at_type = Some("Timestamp".to_owned());
                stamped.serialize(s)
            }
            AnniversaryDate::Timestamp(t) => t.serialize(s),
            AnniversaryDate::Unknown(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for AnniversaryDate {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Deserialize into an intermediate Value, then dispatch on @type.
        let v = serde_json::Value::deserialize(d)?;
        // Non-object Values (scalar, array, null) cannot be a PartialDate
        // or Timestamp — both are JSON objects with @type. Route them to
        // Unknown so any AnniversaryDate constructed in Rust as
        // Unknown(non-object) survives a serialize→deserialize round
        // trip. RFC 9553 itself never emits a non-object date, so this
        // branch is unreachable from spec-conformant wire input.
        if !v.is_object() {
            return Ok(AnniversaryDate::Unknown(v));
        }
        // Match on Option<&str> so absent @type (None) and any concrete
        // @type value (Some(_)) are dispatched without conflating absent
        // with a literal empty string. RFC 9553 §1.3.4 makes @type
        // omissible in defaultType positions but does not define an
        // empty-string @type, so empty strings are routed to Unknown
        // along with any other unrecognised @type value.
        match v.get("@type").and_then(|t| t.as_str()) {
            // RFC 9553 §2.8.1: PartialDate is the default type when @type
            // is absent (or explicitly set to "PartialDate").
            None | Some("PartialDate") => {
                let d: PartialDate = serde_json::from_value(v).map_err(serde::de::Error::custom)?;
                Ok(AnniversaryDate::PartialDate(d))
            }
            Some("Timestamp") => {
                let t: Timestamp = serde_json::from_value(v).map_err(serde::de::Error::custom)?;
                Ok(AnniversaryDate::Timestamp(t))
            }
            Some(_) => Ok(AnniversaryDate::Unknown(v)),
        }
    }
}

/// A memorable date or event (RFC 9553 §2.8.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anniversary {
    /// Object type discriminator; SHOULD be `"Anniversary"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Note and Author (RFC 9553 §2.8.3) ─────────────────────────────────────────

/// A free-text note associated with a Card (RFC 9553 §2.8.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Object type discriminator; SHOULD be `"Note"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// The free-text value of this note.
    pub note: String,

    /// UTC date-time when the note was created (RFC 9553 §1.4.5
    /// `UTCDateTime`): an RFC 3339 date-time string with the time-offset
    /// always `"Z"`, e.g. `"2022-05-22T03:30:00Z"`.
    ///
    /// Stored as bare `String` and not validated; callers that need the
    /// parsed value should use `chrono::DateTime::parse_from_rfc3339` or
    /// `time::OffsetDateTime::parse`. Same contract as [`Timestamp::utc`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,

    /// The author of this note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The author of a [`Note`] (RFC 9553 §2.8.3).
///
/// At least one property other than `@type` must be set per the RFC;
/// this is not enforced at the type level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    /// Object type discriminator; SHOULD be `"Author"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Name of the author.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// URI that identifies the author.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── PersonalInfo (RFC 9553 §2.8.4) ────────────────────────────────────────────

/// Personal information such as an expertise, hobby, or interest
/// (RFC 9553 §2.8.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalInfo {
    /// Object type discriminator; SHOULD be `"PersonalInfo"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Relation (RFC 9553 §2.1.8) ────────────────────────────────────────────────

/// A relationship to another Card (RFC 9553 §2.1.8).
///
/// This is the value type for the `relatedTo` property on a `ContactCard`.
/// Each map key is the `uid` of the related Card; each value is a
/// `Relation` object describing the relationship.
///
/// Distinct from the JSCalendar RFC 8984 §1.4.10 type
/// [`jmap_jscalendar_types::Relation`](https://docs.rs/jmap-jscalendar-types):
/// that type relates JSCalendar entries (events, tasks) via UID and
/// has a different `relation` enumeration; this `Relation` relates
/// JSContact Cards.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    /// Object type discriminator; SHOULD be `"Relation"` when present per RFC 9553 §1.3.4 (may be omitted in defaultType positions).
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Set of relation types (key → `true`). Initial enumerated values:
    /// `"acquaintance"`, `"agent"`, `"child"`, `"co-resident"`,
    /// `"co-worker"`, `"colleague"`, `"contact"`, `"crush"`, `"date"`,
    /// `"emergency"`, `"friend"`, `"kin"`, `"me"`, `"met"`, `"muse"`,
    /// `"neighbor"`, `"parent"`, `"sibling"`, `"spouse"`, `"sweetheart"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<HashMap<String, bool>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    //!
    //! ## Do not generate fixtures programmatically
    //!
    //! It is tempting to reduce repetition by replacing the hand-typed
    //! JSON literals with something like
    //! `serde_json::to_value(Name::default())`. Do not. A fixture
    //! generated from the code under test is not an independent oracle —
    //! it would only verify that `to_value(from_value(x)) == to_value(x)`,
    //! which is a tautology. The figure-numbered fixtures verify that
    //! the typed shape matches the wire shape the RFC says the wire
    //! shape is, which is the only check that catches a misnamed serde
    //! attribute or a renamed field.
    //!
    //! When RFC 9553 errata land, the figure-of-record citations in
    //! each test name (`figure_16`, `figure_19`, etc.) are the audit
    //! trail: re-check the erratum against the corresponding figure
    //! before changing the fixture. See workspace `AGENTS.md` "Test
    //! Integrity" and this crate's `PLAN.md` §"Round-trip test policy".

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

    #[test]
    fn anniversary_date_unknown_non_object_round_trips() {
        // Regression test for bd:JMAP-sgrr.9: a non-object Value wrapped
        // in Unknown must survive serialize→deserialize.
        //
        // Independent oracle: each case is a hand-built JSON literal
        // (scalar string, array, null) chosen because no real RFC 9553
        // emitter would produce it on the wire. The test verifies the
        // round-trip invariant for Rust-constructable values; the wire
        // never exercises this branch.
        for value in [
            serde_json::Value::String("opaque-scalar".into()),
            serde_json::json!([1, 2, 3]),
            serde_json::Value::Null,
        ] {
            let original = AnniversaryDate::Unknown(value.clone());
            let on_wire = serde_json::to_value(&original).unwrap();
            assert_eq!(on_wire, value, "serialize forwards verbatim");
            let back: AnniversaryDate = serde_json::from_value(on_wire).unwrap();
            assert!(
                matches!(back, AnniversaryDate::Unknown(_)),
                "non-object Value must deserialize back to Unknown, got: {back:?}",
            );
            // Extract the inner Value and verify it equals the original.
            let AnniversaryDate::Unknown(inner) = back else {
                unreachable!("matches! above already guards this")
            };
            assert_eq!(inner, value, "Unknown payload preserved across round trip");
        }
    }

    #[test]
    fn anniversary_date_unknown_object_with_recognised_at_type_loses_variant_identity() {
        // Pin test for bd:JMAP-sgrr.28.
        //
        // The Unknown variant's round-trip contract holds only when
        // the wrapped Value's @type is OUTSIDE the dispatcher's
        // recognised set ({"PartialDate", "Timestamp"}) or the Value
        // is not an object. When a caller wraps a Value whose @type
        // IS recognised — or an object with no @type at all (which
        // routes to PartialDate by RFC 9553 §2.8.1's defaultType
        // rule) — variant identity is lost across serialize →
        // deserialize: the deserializer re-dispatches on the @type
        // and produces the typed variant.
        //
        // This is documented behaviour, not a bug. The test pins it
        // so a future contributor does not accidentally "fix" the
        // dispatch contract and break the documented preservation
        // posture for genuinely-unknown shapes.
        //
        // Independent oracle: hand-built JSON literals chosen to
        // exercise each recognised dispatch path.
        let cases: &[(&str, serde_json::Value, fn(&AnniversaryDate) -> bool)] = &[
            (
                "object with @type = PartialDate",
                serde_json::json!({"@type": "PartialDate", "year": 2000}),
                |d| matches!(d, AnniversaryDate::PartialDate(_)),
            ),
            (
                "object with @type = Timestamp",
                serde_json::json!({"@type": "Timestamp", "utc": "2000-01-01T00:00:00Z"}),
                |d| matches!(d, AnniversaryDate::Timestamp(_)),
            ),
            (
                "object with no @type (defaultType = PartialDate)",
                serde_json::json!({"year": 2000}),
                |d| matches!(d, AnniversaryDate::PartialDate(_)),
            ),
        ];
        for (label, value, expected) in cases {
            let original = AnniversaryDate::Unknown(value.clone());
            let wire = serde_json::to_value(&original).expect("serialize");
            assert_eq!(&wire, value, "{label}: serialize forwards verbatim");
            let back: AnniversaryDate = serde_json::from_value(wire).expect("deserialize");
            assert!(
                expected(&back),
                "{label}: contract — recognised @type re-dispatches off Unknown, got {back:?}",
            );
        }
    }

    #[test]
    fn anniversary_date_timestamp_at_type_none_round_trips_as_timestamp() {
        // Regression test for bd:JMAP-sgrr.29.
        //
        // A Rust-side caller can construct
        // `AnniversaryDate::Timestamp(Timestamp { at_type: None, .. })`
        // because `Timestamp::at_type` is `Option<String>` (a Timestamp
        // standing alone outside an Anniversary may omit `@type` per
        // workspace `@type` convention). When that value is emitted in
        // `AnniversaryDate` position the Serialize impl re-stamps
        // `@type = "Timestamp"` so the wire output is unambiguous and
        // deserializes back to the `Timestamp` variant rather than
        // silently routing to the default `PartialDate`.
        //
        // Independent oracle: hand-built `Timestamp` with `at_type:
        // None` (the construction path the bug report describes).
        let original = AnniversaryDate::Timestamp(Timestamp {
            at_type: None,
            utc: "2022-05-22T03:30:00Z".to_owned(),
            extra: serde_json::Map::new(),
        });
        let wire = serde_json::to_value(&original).expect("serialize");
        // Wire must carry the @type discriminator so a peer can dispatch.
        assert_eq!(
            wire.get("@type").and_then(|t| t.as_str()),
            Some("Timestamp"),
            "Serialize must re-stamp @type when caller left it None: {wire}",
        );
        let back: AnniversaryDate = serde_json::from_value(wire).expect("deserialize");
        match back {
            AnniversaryDate::Timestamp(t) => {
                assert_eq!(t.utc, "2022-05-22T03:30:00Z");
                assert!(t.extra.is_empty(), "no stray flatten-extras");
            }
            other => panic!("variant identity lost: expected Timestamp, got {other:?}"),
        }
    }

    #[test]
    fn anniversary_date_timestamp_preserves_caller_at_type() {
        // Sibling of `anniversary_date_timestamp_at_type_none_round_trips_as_timestamp`.
        // When the caller DID set `at_type` we must preserve it verbatim
        // — the re-stamp logic only applies on the `None` path. This
        // guards against an over-eager fix that would overwrite caller
        // input.
        let original = AnniversaryDate::Timestamp(Timestamp {
            at_type: Some("Timestamp".to_owned()),
            utc: "2022-05-22T03:30:00Z".to_owned(),
            extra: serde_json::Map::new(),
        });
        let wire = serde_json::to_value(&original).expect("serialize");
        assert_eq!(
            wire.get("@type").and_then(|t| t.as_str()),
            Some("Timestamp"),
        );
        let back: AnniversaryDate = serde_json::from_value(wire).expect("deserialize");
        assert!(matches!(back, AnniversaryDate::Timestamp(_)));
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

    // ── Extras-preservation policy tests (JMAP-lbdy.5, JMAP-lbdy.12) ─────
    //
    // One round-trip preservation test per migrated type. Each asserts
    // that an unknown vendor / site / private-extension field survives
    // deserialize/serialize unchanged.
    //
    // The four formerly Hash-derived types (NameComponent,
    // AddressComponent, PartialDate, Timestamp) had their Hash derive
    // dropped under JMAP-lbdy.12 option A so the extras-preservation
    // policy applies to them uniformly.

    /// Generic helper: assert a vendor field round-trips through the
    /// given type's `extra` field.
    ///
    /// Asserts full object equality after the round-trip — not just the
    /// vendor key. The stronger assertion catches subtle bugs where a
    /// typed field unexpectedly lands in `extra` (or a vendor field
    /// unexpectedly captures a typed field's value) due to a serde
    /// `flatten` + `rename` interaction. See `bd:JMAP-sgrr.8`.
    fn assert_extras_roundtrip<T>(
        mut raw: serde_json::Value,
        vendor_key: &str,
        vendor_val: serde_json::Value,
    ) where
        T: serde::de::DeserializeOwned + Serialize,
    {
        raw[vendor_key] = vendor_val;
        let de: T = serde_json::from_value(raw.clone()).unwrap();
        let back = serde_json::to_value(&de).unwrap();
        assert_eq!(
            back, raw,
            "full round-trip must preserve typed fields AND the vendor extra"
        );
    }

    #[test]
    fn name_preserves_vendor_extras() {
        assert_extras_roundtrip::<Name>(
            json!({"full": "Alice"}),
            "acmeCorpNameSource",
            json!("hr"),
        );
    }

    // ── @type-vs-extras serde-interaction probe (bd:JMAP-sgrr.6) ──────────
    //
    // Every wire-format struct combines `#[serde(rename = "@type", ...)]`
    // on `at_type` with `#[serde(flatten)]` on `extra`. The intended
    // behavior is that an `@type` field on the wire populates `at_type`
    // and `extra` stays empty; the pathological alternative is that
    // flatten captures `@type` into `extra` and `at_type` stays None.
    // Workspace policy reasons from serde docs that the explicit rename
    // takes priority over flatten, but no test verified the behaviour
    // empirically. These probes lock it in so a future serde release
    // that subtly changes the interaction would fail loudly here rather
    // than silently break every JSContact emitter.

    #[test]
    fn name_at_type_populates_at_type_not_extras() {
        // Probe 1: bare @type. Independent oracle: a hand-built JSON
        // literal with @type set; assert the typed field captures it
        // and extras stays empty; assert full round-trip equality.
        let wire = json!({"@type": "Name", "full": "Alice"});
        let de: Name = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            de.at_type.as_deref(),
            Some("Name"),
            "@type must populate at_type, not flow into extras"
        );
        assert!(
            de.extra.is_empty(),
            "@type must NOT leak into extras; found: {:?}",
            de.extra
        );
        let back = serde_json::to_value(&de).unwrap();
        assert_eq!(back, wire, "round-trip must preserve @type on the wire");
    }

    #[test]
    fn name_at_type_and_vendor_extras_coexist_separately() {
        // Probe 2: @type AND a vendor extra. Independent oracle: a
        // hand-built JSON literal carrying both; assert at_type captures
        // @type, extras captures the vendor key only, and full
        // round-trip preserves the wire shape byte-for-byte.
        let wire = json!({
            "@type": "Name",
            "full": "Alice",
            "acmeCorpNameSource": "hr",
        });
        let de: Name = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(de.at_type.as_deref(), Some("Name"));
        assert_eq!(
            de.extra.get("acmeCorpNameSource"),
            Some(&json!("hr")),
            "vendor key must land in extras"
        );
        assert!(
            !de.extra.contains_key("@type"),
            "@type must NOT also appear in extras; found: {:?}",
            de.extra
        );
        let back = serde_json::to_value(&de).unwrap();
        assert_eq!(back, wire);
    }

    #[test]
    fn nickname_preserves_vendor_extras() {
        assert_extras_roundtrip::<Nickname>(
            json!({"name": "Al"}),
            "acmeCorpScope",
            json!("internal"),
        );
    }

    #[test]
    fn organization_preserves_vendor_extras() {
        assert_extras_roundtrip::<Organization>(
            json!({"name": "Acme"}),
            "acmeCorpDept",
            json!("eng"),
        );
    }

    #[test]
    fn org_unit_preserves_vendor_extras() {
        assert_extras_roundtrip::<OrgUnit>(
            json!({"name": "Platform"}),
            "acmeCorpCostCenter",
            json!("cc-42"),
        );
    }

    #[test]
    fn speak_to_as_preserves_vendor_extras() {
        assert_extras_roundtrip::<SpeakToAs>(
            json!({"grammaticalGender": "feminine"}),
            "acmeCorpFormality",
            json!("informal"),
        );
    }

    #[test]
    fn pronouns_preserves_vendor_extras() {
        assert_extras_roundtrip::<Pronouns>(
            json!({"pronouns": "she/her"}),
            "acmeCorpAccessibilityHint",
            json!("screen-reader"),
        );
    }

    #[test]
    fn title_preserves_vendor_extras() {
        assert_extras_roundtrip::<Title>(json!({"name": "Engineer"}), "acmeCorpLevel", json!(5));
    }

    #[test]
    fn email_address_preserves_vendor_extras() {
        assert_extras_roundtrip::<EmailAddress>(
            json!({"address": "alice@example.com"}),
            "acmeCorpVerified",
            json!(true),
        );
    }

    #[test]
    fn online_service_preserves_vendor_extras() {
        assert_extras_roundtrip::<OnlineService>(
            json!({"service": "GitHub", "uri": "https://github.com/alice"}),
            "acmeCorpScore",
            json!(0.95),
        );
    }

    #[test]
    fn phone_preserves_vendor_extras() {
        assert_extras_roundtrip::<Phone>(
            json!({"number": "tel:+1-555-0100"}),
            "acmeCorpRegion",
            json!("us"),
        );
    }

    #[test]
    fn language_pref_preserves_vendor_extras() {
        assert_extras_roundtrip::<LanguagePref>(
            json!({"language": "en"}),
            "acmeCorpProficiency",
            json!("native"),
        );
    }

    #[test]
    fn calendar_preserves_vendor_extras() {
        assert_extras_roundtrip::<Calendar>(
            json!({"kind": "calendar", "uri": "https://cal/example"}),
            "acmeCorpAccessLevel",
            json!("read-only"),
        );
    }

    #[test]
    fn scheduling_address_preserves_vendor_extras() {
        assert_extras_roundtrip::<SchedulingAddress>(
            json!({"uri": "mailto:alice@example.com"}),
            "acmeCorpReplyHint",
            json!("auto"),
        );
    }

    #[test]
    fn address_preserves_vendor_extras() {
        assert_extras_roundtrip::<Address>(
            json!({"full": "123 Main St"}),
            "acmeCorpGeocoded",
            json!(true),
        );
    }

    #[test]
    fn crypto_key_preserves_vendor_extras() {
        assert_extras_roundtrip::<CryptoKey>(
            json!({"uri": "https://example.com/key.pem"}),
            "acmeCorpKeyAlgorithm",
            json!("rsa-2048"),
        );
    }

    #[test]
    fn directory_preserves_vendor_extras() {
        assert_extras_roundtrip::<Directory>(
            json!({"kind": "directory", "uri": "ldap://example.com"}),
            "acmeCorpDirectoryNamespace",
            json!("internal"),
        );
    }

    #[test]
    fn link_preserves_vendor_extras() {
        assert_extras_roundtrip::<Link>(
            json!({"uri": "https://example.com"}),
            "acmeCorpLinkRel",
            json!("homepage"),
        );
    }

    #[test]
    fn media_preserves_vendor_extras() {
        assert_extras_roundtrip::<Media>(
            json!({"kind": "photo", "uri": "https://example.com/photo.jpg"}),
            "acmeCorpThumbnailUri",
            json!("https://example.com/photo.thumb.jpg"),
        );
    }

    #[test]
    fn anniversary_preserves_vendor_extras() {
        assert_extras_roundtrip::<Anniversary>(
            json!({"kind": "birth", "date": {"year": 2000, "month": 1, "day": 1}}),
            "acmeCorpReminderDays",
            json!(7),
        );
    }

    #[test]
    fn note_preserves_vendor_extras() {
        assert_extras_roundtrip::<Note>(
            json!({"note": "important"}),
            "acmeCorpClassification",
            json!("internal"),
        );
    }

    #[test]
    fn author_preserves_vendor_extras() {
        assert_extras_roundtrip::<Author>(
            json!({"name": "Alice"}),
            "acmeCorpAuthorRole",
            json!("manager"),
        );
    }

    #[test]
    fn personal_info_preserves_vendor_extras() {
        assert_extras_roundtrip::<PersonalInfo>(
            json!({"kind": "hobby", "value": "skiing"}),
            "acmeCorpInterestRank",
            json!(3),
        );
    }

    #[test]
    fn relation_preserves_vendor_extras() {
        assert_extras_roundtrip::<Relation>(
            json!({"relation": {"friend": true}}),
            "acmeCorpRelationStrength",
            json!("close"),
        );
    }

    // ── JMAP-lbdy.12: formerly Hash-derived types ─────────────────────────

    #[test]
    fn name_component_preserves_vendor_extras() {
        assert_extras_roundtrip::<NameComponent>(
            json!({"value": "Vincent", "kind": "given"}),
            "acmeCorpComponentSource",
            json!("hr"),
        );
    }

    #[test]
    fn address_component_preserves_vendor_extras() {
        assert_extras_roundtrip::<AddressComponent>(
            json!({"value": "123", "kind": "number"}),
            "acmeCorpVerified",
            json!(true),
        );
    }

    #[test]
    fn partial_date_preserves_vendor_extras() {
        assert_extras_roundtrip::<PartialDate>(
            json!({"year": 2000, "month": 1, "day": 1}),
            "acmeCorpDateSource",
            json!("self-reported"),
        );
    }

    #[test]
    fn timestamp_preserves_vendor_extras() {
        assert_extras_roundtrip::<Timestamp>(
            json!({"@type": "Timestamp", "utc": "2022-05-22T03:30:00Z"}),
            "acmeCorpTimezone",
            json!("UTC"),
        );
    }
}
