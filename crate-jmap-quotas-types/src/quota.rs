//! RFC 9425 §3 and §4: [`Quota`], its sub-types [`Scope`] and
//! [`ResourceType`], and the `Quota/query` filter and comparator.

use jmap_types::{Id, impl_string_enum};
use serde::{Deserialize, Serialize};

/// The entities a quota applies to (RFC 9425 §3.1 Scope).
///
/// Unknown values keep their wire string in [`Other`](Self::Other) so they
/// round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    /// "The quota information applies to just the client's account."
    Account,
    /// "The quota information applies to all accounts sharing this domain."
    Domain,
    /// "The quota information applies to all accounts belonging to the
    /// server."
    Global,
    /// A value not defined by RFC 9425.
    Other(String),
}

impl_string_enum!(Scope, "a JMAP Quota scope string",
    "account" => Account,
    "domain" => Domain,
    "global" => Global,
);

/// The unit of measure of a quota (RFC 9425 §3.2 ResourceType).
///
/// Unknown values keep their wire string in [`Other`](Self::Other) so they
/// round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// "The quota is measured in a number of data type objects."
    Count,
    /// "The quota is measured in size (in octets)."
    Octets,
    /// A value not defined by RFC 9425.
    Other(String),
}

impl_string_enum!(ResourceType, "a JMAP Quota resource type string",
    "count" => Count,
    "octets" => Octets,
);

/// A JMAP Quota object (RFC 9425 §4.1): a limit on an account's usage and
/// the current usage against it.
///
/// Every property is server-set: RFC 9425 defines no `Quota/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    /// "The unique identifier for this object."
    pub id: Id,
    /// "The resource type of the quota as defined in Section 3.2."
    pub resource_type: ResourceType,
    /// "The current usage of the defined quota, using the "resourceType"
    /// defined as unit of measure."
    pub used: u64,
    /// "The hard limit set by this quota ... Objects in scope may not be
    /// created or updated if this limit is reached."
    pub hard_limit: u64,
    /// "The "Scope" of this quota as defined in Section 3.1."
    pub scope: Scope,
    /// "The name of the quota. Useful for managing quotas and using queries
    /// for searching."
    pub name: String,
    /// "A list of all the type names as defined in the "JMAP Types Names"
    /// registry (e.g., Email, Calendar, etc.) to which this quota applies."
    /// A server filters it to the types whose capability the request uses
    /// (§4.1).
    pub types: Vec<String>,
    /// "The warn limit set by this quota" (optional; `UnsignedInt|null`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn_limit: Option<u64>,
    /// "The soft limit set by this quota" (optional; `UnsignedInt|null`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_limit: Option<u64>,
    /// "Arbitrary, free, human-readable description of this quota"
    /// (optional; `String|null`). Without `urn:ietf:params:jmap:i18n` in
    /// `using`, the server selects it by Accept-Language (RFC 9425 §4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The language of `name` and `description`
    /// (draft-atwood-jmap-updates-00 §4.1); `None` when unknown or not
    /// stored. Returned only to requests that use
    /// `urn:ietf:params:jmap:i18n`; jmap-quotas-server shapes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<jmap_types::i18n::LanguageTag>,
    /// `name` and `description` in other languages
    /// (draft-atwood-jmap-updates-00 §4.1, §5.2). The server
    /// sets them: both properties are server-set. A `null` localized
    /// `description` means there is none in that language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub localizations: Option<jmap_types::i18n::Localizations>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Filter condition for `Quota/query` (RFC 9425 §4.4). "A Quota object
/// matches the FilterCondition if, and only if, all the given conditions
/// match. If zero properties are specified, it is automatically true for
/// all objects."
///
/// Unknown condition names fail to decode (`deny_unknown_fields`), which a
/// server answers with `unsupportedFilter` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaFilterCondition {
    /// "The Quota name property contains the given string."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// "The Quota scope property must match the given value exactly."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    /// "The Quota resourceType property must match the given value exactly."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<ResourceType>,
    /// "The Quota types property contains the given value." Wire name
    /// `type`.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
}

/// Concrete filter type for `Quota/query` (RFC 9425 §4.4).
///
/// Alias for `jmap_types::Filter<QuotaFilterCondition>`, mirroring the
/// canonical `jmap_mail_types::EmailFilter` shape.
pub type QuotaFilter = jmap_types::Filter<QuotaFilterCondition>;

/// The property to sort by in a [`QuotaComparator`] (RFC 9425 §4.4: "The
/// following Quota properties MUST be supported for sorting: name, used").
///
/// Unknown property names are preserved in [`Other`](Self::Other) so they
/// round-trip; a server answers one it cannot sort by with
/// `unsupportedSort` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QuotaComparatorProperty {
    /// Sort by `name`.
    Name,
    /// Sort by `used`.
    Used,
    /// A property not listed above, kept so it round-trips.
    Other(String),
}

impl_string_enum!(QuotaComparatorProperty, "a Quota sort property string",
    "name" => Name,
    "used" => Used,
);

/// Comparator for `Quota/query` (RFC 8620 §5.5, RFC 9425 §4.4).
///
/// Build one with [`QuotaComparator::new`]; the struct is
/// `#[non_exhaustive]`. It carries no flatten-extras field: backends
/// dispatch on `property`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaComparator {
    /// The property to sort by.
    pub property: QuotaComparatorProperty,
    /// If `true`, sort ascending; if `false`, sort descending (default `true`).
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub is_ascending: bool,
    /// A collation identifier (RFC 4790) to use when comparing strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl QuotaComparator {
    /// An ascending comparator on `property` with no collation
    /// (`isAscending` defaults to `true`, RFC 8620 §5.5).
    pub fn new(property: QuotaComparatorProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

fn bool_true() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}
