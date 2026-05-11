//! RFC 9610 §3 — ContactCard object and filter types.
//!
//! A ContactCard is a JMAP wrapper around a JSContact Card (RFC 9553 §2),
//! with two JMAP-specific additions: `id` and `addressBookIds`.
//!
//! Complex JSContact sub-objects (addresses, phones, emails, etc.) are
//! represented as [`serde_json::Value`] because their nested structure is
//! large and their schema is still evolving.  Callers that need typed access
//! to sub-objects should deserialize the relevant `Value` field directly.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, UTCDate};
use serde::{Deserialize, Serialize};

/// A JMAP ContactCard object (RFC 9610 §3).
///
/// Wraps the JSContact Card format (RFC 9553 §2) with two JMAP-specific
/// additions: [`id`](ContactCard::id) and
/// [`address_book_ids`](ContactCard::address_book_ids).
///
/// All fields use `Option` because:
/// - RFC 9553 makes most fields optional.
/// - JMAP `properties` argument allows partial responses; absent fields must
///   not fail deserialization.
///
/// Complex sub-object maps (addresses, phones, emails, etc.) use
/// [`serde_json::Value`] to avoid coupling this crate to every nested
/// JSContact type.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactCard {
    // ── JMAP additions (RFC 9610 §3) ─────────────────────────────────
    /// Server-assigned immutable identifier (JMAP addition).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// Set of AddressBook ids this card belongs to.  Each value MUST be
    /// `true`; the set is encoded as `{ "<id>": true, ... }`.
    /// (JMAP addition — wire name: `addressBookIds`.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_book_ids: Option<HashMap<Id, bool>>,

    // ── RFC 9553 §2.1 Metadata ───────────────────────────────────────────
    /// JSContact version; MUST be `"1.0"` when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Date and time when the Card was created (UTCDateTime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<UTCDate>,

    /// Kind of entity: `"individual"`, `"group"`, `"org"`, `"location"`,
    /// `"device"`, `"application"`, or a vendor-specific value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// BCP 47 language tag for the primary language of the Card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// For group cards (`kind = "group"`): set of member UIDs → `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<HashMap<String, bool>>,

    /// Identifier for the product that created the Card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prod_id: Option<String>,

    /// Cards related to this one.  Map of uid → Relation object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_to: Option<serde_json::Value>,

    /// Globally unique identifier for this Card (often a UUID URN).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,

    /// Date and time when the Card was last modified (UTCDateTime; server-set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<UTCDate>,

    // ── RFC 9553 §2.2 Name and Organization ─────────────────────────────
    /// The name of the entity.  JSContact Name object (complex sub-object).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<serde_json::Value>,

    /// Nicknames map: id → Nickname object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nicknames: Option<serde_json::Value>,

    /// Organizations map: id → Organization object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organizations: Option<serde_json::Value>,

    /// How to address the entity in spoken or written language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speak_to_as: Option<serde_json::Value>,

    /// Titles map: id → Title object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub titles: Option<serde_json::Value>,

    // ── RFC 9553 §2.3 Contact ────────────────────────────────────────────
    /// Email addresses map: id → EmailAddress object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emails: Option<serde_json::Value>,

    /// Online services map: id → OnlineService object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_services: Option<serde_json::Value>,

    /// Phone numbers map: id → Phone object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phones: Option<serde_json::Value>,

    /// Preferred languages map: id → LanguagePref object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_languages: Option<serde_json::Value>,

    // ── RFC 9553 §2.4 Calendaring ────────────────────────────────────────
    /// Calendars map: id → Calendar object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendars: Option<serde_json::Value>,

    /// Scheduling addresses map: id → SchedulingAddress object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduling_addresses: Option<serde_json::Value>,

    // ── RFC 9553 §2.5 Address ────────────────────────────────────────────
    /// Postal addresses map: id → Address object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addresses: Option<serde_json::Value>,

    // ── RFC 9553 §2.6 Resources ──────────────────────────────────────────
    /// Cryptographic keys map: id → CryptoKey object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crypto_keys: Option<serde_json::Value>,

    /// Directories map: id → Directory object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directories: Option<serde_json::Value>,

    /// Links map: id → Link object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<serde_json::Value>,

    /// Media map: id → Media object (may include JMAP `blobId` extension).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<serde_json::Value>,

    // ── RFC 9553 §2.7 Multilingual ───────────────────────────────────────
    /// Localization patches map: BCP 47 language-tag →
    /// [`PatchObject`].
    ///
    /// Wire format is byte-identical to a plain JSON object via
    /// `#[serde(transparent)]` on `PatchObject`. The typed shape enforces the
    /// outer object-map structure at the type system level; inner patch
    /// values remain opaque JSON (per RFC 8620 §5.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localizations: Option<HashMap<String, PatchObject>>,

    // ── RFC 9553 §2.8 Additional ─────────────────────────────────────────
    /// Anniversaries map: id → Anniversary object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anniversaries: Option<serde_json::Value>,

    /// Keywords set: keyword → `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<serde_json::Value>,

    /// Notes map: id → Note object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<serde_json::Value>,

    /// Personal information map: id → PersonalInfo object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personal_info: Option<serde_json::Value>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Filter condition for `ContactCard/query` (RFC 9610 §3.3.1).
///
/// All fields are optional; a condition with no fields set matches every
/// ContactCard.
///
/// Note: several field names use forward-slash separators on the wire
/// (e.g., `"name/given"`, `"name/surname"`, `"name/surname2"`).  These are
/// encoded using explicit `#[serde(rename = "...")]` attributes.
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactCardFilterCondition {
    /// Card must belong to this AddressBook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_address_book: Option<Id>,

    /// Card must have exactly this uid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,

    /// Card must have a `members` property containing this uid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_member: Option<String>,

    /// Card `kind` must equal this string exactly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Card `created` must be before this UTCDateTime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<UTCDate>,

    /// Card `created` must be equal to or after this UTCDateTime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<UTCDate>,

    /// Card `updated` must be before this UTCDateTime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_before: Option<UTCDate>,

    /// Card `updated` must be equal to or after this UTCDateTime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_after: Option<UTCDate>,

    /// Full-text search across all text in the card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Matches any NameComponent value or the `full` property in `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Matches NameComponents with `kind = "given"`.
    /// Wire name: `name/given`.
    #[serde(rename = "name/given", skip_serializing_if = "Option::is_none")]
    pub name_given: Option<String>,

    /// Matches NameComponents with `kind = "surname"`.
    /// Wire name: `name/surname`.
    #[serde(rename = "name/surname", skip_serializing_if = "Option::is_none")]
    pub name_surname: Option<String>,

    /// Matches NameComponents with `kind = "surname2"`.
    /// Wire name: `name/surname2`.
    #[serde(rename = "name/surname2", skip_serializing_if = "Option::is_none")]
    pub name_surname2: Option<String>,

    /// Matches any Nickname `name` in the `nicknames` property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,

    /// Matches any Organization `name` in the `organizations` property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,

    /// Matches any EmailAddress `address` or `label` in the `emails` property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Matches any Phone `number` or `label` in the `phones` property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,

    /// Matches any OnlineService `service`, `uri`, `user`, or `label` in
    /// `onlineServices`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_service: Option<String>,

    /// Matches any AddressComponent value or the `full` property in `addresses`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,

    /// Matches any Note `note` in the `notes` property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Comparator for `ContactCard/query` sort order (RFC 9610 §3.3.2).
///
/// The `property` field holds the sort key string.  Required values:
/// `"created"`, `"updated"`.  Recommended values: `"name/given"`,
/// `"name/surname"`, `"name/surname2"`.
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field, and its `property`
/// field is consumed by backend dispatch to determine sort order. See
/// [`ContactCardFilterCondition`] for the rationale and for the two
/// recommended paths (`draft-ietf-jmap-metadata`, bd JMAP-06zp; or the
/// pre-IETF sloppy-value escape).
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactCardComparator {
    /// Sort key string.
    pub property: String,
    /// Sort direction; `true` = ascending (default), `false` = descending.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Optional collation identifier (RFC 4790).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

fn default_true() -> bool {
    true
}
