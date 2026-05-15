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

use crate::collision::{self, CollisionError};

/// Camel-case wire-format names of every typed field on [`ContactCard`].
///
/// Used by [`ContactCard::validate_extras`] to detect vendor-extras
/// key collisions before serialize. Order is not significant; the
/// helper sorts collisions alphabetically. JMAP-glx8.25.
const CONTACT_CARD_TYPED_FIELDS: &[&str] = &[
    "id",
    "addressBookIds",
    "version",
    "created",
    "kind",
    "language",
    "members",
    "prodId",
    "relatedTo",
    "uid",
    "updated",
    "name",
    "nicknames",
    "organizations",
    "speakToAs",
    "titles",
    "emails",
    "onlineServices",
    "phones",
    "preferredLanguages",
    "calendars",
    "schedulingAddresses",
    "addresses",
    "cryptoKeys",
    "directories",
    "links",
    "media",
    "localizations",
    "anniversaries",
    "keywords",
    "notes",
    "personalInfo",
];

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
///
/// # Construction
///
/// [`ContactCard::default()`] returns a card with every field unset,
/// useful for incremental construction (e.g. building a `ContactCard/set
/// create` argument by patching the wanted fields). It is **not** a valid
/// wire payload on its own: RFC 9610 §3 requires `id` (server-set) and
/// `addressBookIds` on full-object responses, and RFC 9553 §2.1 requires
/// `@type` (implicit) plus `version` and `uid` on creation. The handler
/// layer (`jmap-contacts-server`) is responsible for rejecting partial
/// cards on `/set` create; servers building `/get` responses MUST fill in
/// `id` and `addressBookIds` before serializing. JMAP-glx8.16.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactCard {
    // ── JMAP additions (RFC 9610 §3) ─────────────────────────────────
    /// Server-assigned immutable identifier (JMAP addition).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// Set of AddressBook ids this card belongs to (JMAP addition; wire
    /// name `addressBookIds`).
    ///
    /// Represented as `HashMap<Id, bool>` because the JMAP wire format
    /// uses a JSON object with boolean values (RFC 9610 §3). Values are
    /// always `true` in full-object responses; per RFC 9610 §3, "each
    /// value MUST be true" — the type does not enforce that constraint,
    /// so callers (and `jmap-contacts-server`) MUST reject `false`
    /// values with `invalidProperties` per RFC 8620 §5.3. The map shape
    /// is also used in PatchObject updates (RFC 8620 §5.3) where a
    /// `null` value removes an entry.
    ///
    /// This shape matches the canonical `Email.mailbox_ids` precedent
    /// in `jmap-mail-types`. JMAP-glx8.11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_book_ids: Option<HashMap<Id, bool>>,

    // ── RFC 9553 §2.1 Metadata ───────────────────────────────────────────
    /// JSContact version; MUST be `"1.0"` when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Date and time when the Card was created (RFC 8620 UTCDate format:
    /// `YYYY-MM-DDTHH:MM:SS[.SSS]Z`, exactly the `Z` UTC suffix).
    ///
    /// [`UTCDate`] is a transparent `String` newtype in `jmap-types`; it
    /// performs **no parse validation** at construction. Callers building a
    /// `ContactCard/set` argument MUST supply a syntactically valid
    /// UTCDate; the handler layer rejects malformed values with
    /// `invalidProperties`. JMAP-glx8.18.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<UTCDate>,

    /// Kind of entity: `"individual"`, `"group"`, `"org"`, `"location"`,
    /// `"device"`, `"application"`, or a vendor-specific value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// BCP 47 language tag for the primary language of the Card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// For group cards (`kind = "group"`): set of member UIDs → `true`
    /// (RFC 9553 §2.1.6).
    ///
    /// Values are always `true`; per RFC 9553 §2.1.6, "each value MUST
    /// be true" — the type does not enforce that constraint, so callers
    /// (and `jmap-contacts-server`) MUST reject `false` values with
    /// `invalidProperties` per RFC 8620 §5.3. The map shape is also
    /// used in PatchObject updates (RFC 8620 §5.3) where a `null` value
    /// removes an entry. JMAP-glx8.11.
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

    /// Date and time when the Card was last modified (UTCDate; server-set).
    ///
    /// See [`created`](Self::created) for the format and validation
    /// contract. JMAP-glx8.18.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<UTCDate>,

    // ── RFC 9553 §2.2 Name and Organization ─────────────────────────────
    //
    // The 22 sub-object fields below are `Option<serde_json::Value>` per
    // the workspace Sloppy-Value pattern (see workspace AGENTS.md). RFC
    // 9553 §2.2-§2.8 requires that each value be a **JSON object** of
    // the relevant sub-type shape, but the wire type accepts any JSON
    // (string, number, array, bool, null, object) and round-trips it
    // unchanged. A caller submitting a non-object value produces a
    // wire-format violation; the handler layer (`jmap-contacts-server`)
    // is responsible for rejecting non-object values with
    // `invalidProperties` per RFC 8620 §5.3. Typed access from
    // `jmap-jscontact-types` (re-exported at this crate's root) MUST
    // wrap `serde_json::from_value` in a `Result` to handle non-object
    // wire shapes gracefully. JMAP-glx8.13.
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

    /// Keywords set: keyword → `true` (RFC 9553 §2.8.2 `String[Boolean]`).
    ///
    /// Typed because the wire shape is closed by spec and matches the
    /// sibling `members` and `address_book_ids` fields. JMAP-glx8.3.
    /// Values are always `true`; the type does not enforce that
    /// constraint, so callers MUST reject `false` values per RFC 9553
    /// §2.8.2. The map shape is also used in PatchObject updates
    /// (RFC 8620 §5.3) where a `null` value removes an entry.
    /// JMAP-glx8.11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<HashMap<String, bool>>,

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
    ///
    /// # Collision contract
    ///
    /// `extra` MUST NOT contain a key that matches one of the typed
    /// wire-format field names (`id`, `addressBookIds`, `version`,
    /// `created`, `kind`, `language`, `members`, `prodId`, `relatedTo`,
    /// `uid`, `updated`, `name`, `nicknames`, `organizations`,
    /// `speakToAs`, `titles`, `emails`, `onlineServices`, `phones`,
    /// `preferredLanguages`, `calendars`, `schedulingAddresses`,
    /// `addresses`, `cryptoKeys`, `directories`, `links`, `media`,
    /// `localizations`, `anniversaries`, `keywords`, `notes`,
    /// `personalInfo`). On deserialize, `#[serde(flatten)]` consumes
    /// matching keys into their typed fields first; truly unknown keys
    /// land in `extra`. On serialize, a colliding key in `extra` is
    /// emitted as a **duplicate JSON object key** alongside the typed
    /// field. RFC 8259 §4 leaves duplicate-key handling
    /// implementation-defined; JMAP servers MAY accept, reject, or
    /// last-wins.
    ///
    /// In short: treat `extra` as a write-only catch-all for unknown
    /// keys discovered at deserialize, and do not programmatically
    /// insert keys that match a typed field. JMAP-glx8.19.
    ///
    /// See [`ContactCard::validate_extras`] for a runtime pre-serialize
    /// check that detects this hazard. JMAP-glx8.25.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ContactCard {
    /// Check that no [`extra`](Self::extra) key shadows a typed
    /// wire-format field of this struct. Returns
    /// [`Err(CollisionError)`](CollisionError) listing any colliding
    /// keys; otherwise returns `Ok(())`.
    ///
    /// Recommended pre-serialize hook for producers who construct
    /// `ContactCard` values programmatically and need to guarantee
    /// that the resulting JSON does not contain duplicate object keys.
    /// See the [`extra`](Self::extra) collision contract for the
    /// underlying hazard. JMAP-glx8.25.
    ///
    /// # Errors
    ///
    /// Returns [`CollisionError`] when one or more keys in
    /// [`extra`](Self::extra) match one of the camelCase wire-format
    /// names of this struct's typed fields.
    pub fn validate_extras(&self) -> Result<(), CollisionError> {
        collision::check(&self.extra, CONTACT_CARD_TYPED_FIELDS)
    }
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
/// # Field semantics
///
/// Two classes of filter field, with different matching contracts:
///
/// - **Exact-match fields** ([`in_address_book`](Self::in_address_book),
///   [`uid`](Self::uid), [`has_member`](Self::has_member),
///   [`kind`](Self::kind)) test value equality (string equality for the
///   `String`-typed fields, [`Id`] equality for `in_address_book`).
///   These are unambiguous and portable across servers.
///
/// - **Range fields** ([`created_before`](Self::created_before),
///   [`created_after`](Self::created_after), and the `updated_*` siblings)
///   test [`UTCDate`] ordering — `*_before` is strictly less than, `*_after`
///   is greater than or equal to, per RFC 8620 §5.6.
///
/// - **Text-match fields** ([`text`](Self::text), [`name`](Self::name),
///   [`name_given`](Self::name_given), [`name_surname`](Self::name_surname),
///   [`name_surname2`](Self::name_surname2), [`nickname`](Self::nickname),
///   [`organization`](Self::organization), [`email`](Self::email),
///   [`phone`](Self::phone), [`online_service`](Self::online_service),
///   [`address`](Self::address), [`note`](Self::note)) — **matching
///   semantics are server-defined**. RFC 9610 §3.3.1 does NOT specify
///   case-sensitivity, substring-vs-prefix-vs-exact, Unicode
///   normalization (NFC/NFD), accent folding, or word-boundary handling.
///   A caller writing `filter.name = Some("müller".into())` may get
///   different result sets from different servers running identical
///   queries. Portable callers SHOULD treat text-match fields as
///   approximate full-text-search filters, not exact value tests.
///   JMAP-glx8.17.
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
/// `Metadata` / `Annotation` companion object. Implemented in `jmap-metadata-types`,
/// `jmap-metadata-server`, and `jmap-metadata-client` (bd JMAP-06zp).
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

    /// Card `created` must be strictly before this UTCDate
    /// (`YYYY-MM-DDTHH:MM:SS[.SSS]Z`).
    ///
    /// [`UTCDate`] is a transparent `String` newtype with no parse
    /// validation at construction; the server rejects syntactically
    /// invalid values per RFC 8620 §5.6. JMAP-glx8.18.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<UTCDate>,

    /// Card `created` must be equal to or after this UTCDate.
    ///
    /// See [`created_before`](Self::created_before) for the format and
    /// validation contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<UTCDate>,

    /// Card `updated` must be strictly before this UTCDate.
    ///
    /// See [`created_before`](Self::created_before) for the format and
    /// validation contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_before: Option<UTCDate>,

    /// Card `updated` must be equal to or after this UTCDate.
    ///
    /// See [`created_before`](Self::created_before) for the format and
    /// validation contract.
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
/// The `property` field holds the sort key string. RFC 9610 §3.3.2 declares
/// a small required + recommended set; const identifiers are exposed in
/// [`prop`] for spelling-safe call sites:
///
/// - Required: `"created"`, `"updated"` ([`prop::CREATED`], [`prop::UPDATED`]).
/// - Recommended: `"name/given"`, `"name/surname"`, `"name/surname2"`
///   ([`prop::NAME_GIVEN`], [`prop::NAME_SURNAME`], [`prop::NAME_SURNAME2`]).
///
/// The `property` type stays `String` to preserve forward-compat with
/// server-specific sort keys and with future RFC additions (the workspace
/// filter-algebra exclusion blocks a typed-enum-with-`Other(String)`
/// shape; see [`ContactCardFilterCondition`]).
///
/// # Default impl divergence
///
/// [`ContactCardComparator::default()`] produces
/// `is_ascending: false` because [`bool::default()`] is `false`. The
/// **wire-default** for an absent `isAscending` key is **`true`** —
/// `#[serde(default = "default_ascending")]` (RFC 8620 §5.5). A caller
/// who writes
///
/// ```rust
/// # use jmap_contacts_types::ContactCardComparator;
/// let mut c = ContactCardComparator::default();
/// c.property = "created".into();
/// ```
///
/// and submits the comparator gets **descending** sort silently — the
/// Rust API default does not match the wire-format default semantic.
/// This is the same class of footgun as [`ContactCard::default()`]
/// producing a wire-invalid `{}` (JMAP-glx8.16), but more dangerous
/// because the result *looks* valid and just returns the wrong record
/// order. Always set `is_ascending` explicitly when constructing a
/// comparator from [`Default::default()`]. JMAP-glx8.22.
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
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactCardComparator {
    /// Sort key string.
    pub property: String,
    /// Sort direction; `true` = ascending (wire-default per RFC 8620
    /// §5.5), `false` = descending.
    ///
    /// **`Default::default()` produces `false`** — see the
    /// [`ContactCardComparator`] type-level "Default impl divergence"
    /// note. JMAP-glx8.22.
    #[serde(default = "default_ascending")]
    pub is_ascending: bool,
    /// Optional collation identifier (RFC 4790).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

fn default_ascending() -> bool {
    true
}

/// Constant identifiers for [`ContactCardComparator::property`] sort keys
/// declared by RFC 9610 §3.3.2.
///
/// Using a const at the call site catches typos at compile time:
///
/// ```rust
/// # use jmap_contacts_types::card::{ContactCardComparator, prop};
/// let mut cmp = ContactCardComparator::default();
/// cmp.property = prop::CREATED.into();
/// ```
///
/// Servers MAY support additional sort keys; the `property` type
/// remains `String` so vendor extensions and future RFC additions
/// round-trip unchanged. JMAP-glx8.14.
pub mod prop {
    /// Required sort key (RFC 9610 §3.3.2): card `created` timestamp.
    pub const CREATED: &str = "created";
    /// Required sort key (RFC 9610 §3.3.2): card `updated` timestamp.
    pub const UPDATED: &str = "updated";
    /// Recommended sort key: NameComponent with `kind = "given"`.
    pub const NAME_GIVEN: &str = "name/given";
    /// Recommended sort key: NameComponent with `kind = "surname"`.
    pub const NAME_SURNAME: &str = "name/surname";
    /// Recommended sort key: NameComponent with `kind = "surname2"`.
    pub const NAME_SURNAME2: &str = "name/surname2";
}
