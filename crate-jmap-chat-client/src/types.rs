//! JMAP Chat client-side auxiliary types.
//!
//! This module contains types used in client-facing APIs that are not part of
//! the wire-format types defined in `jmap-chat-types`.

use jmap_types::impl_string_enum;
use serde::Serialize;

// ---------------------------------------------------------------------------
// ContactPresenceFilter
// ---------------------------------------------------------------------------

/// Presence filter for `ChatContact/query` operations.
///
/// Mirrors [`jmap_chat_types::Presence`] but omits `Other`, which has no
/// defined filter semantics and must never be sent to the server.
///
/// Use [`TryFrom<jmap_chat_types::Presence>`] to convert a deserialized
/// presence value into a filter value (fails if `Other`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContactPresenceFilter {
    /// Filter to contacts currently online.
    Online,
    /// Filter to contacts marked away.
    Away,
    /// Filter to contacts marked busy.
    Busy,
    /// Filter to contacts marked invisible.
    Invisible,
    /// Filter to contacts currently offline.
    Offline,
}

impl TryFrom<jmap_chat_types::Presence> for ContactPresenceFilter {
    /// Conversion fails when `p` is [`jmap_chat_types::Presence::Other`].
    /// The failed value is returned in the `Err` so callers can recover
    /// the original wire string (typically for logging or selective
    /// fallback) rather than dropping it to a unit error.
    type Error = jmap_chat_types::Presence;

    fn try_from(p: jmap_chat_types::Presence) -> Result<Self, Self::Error> {
        match p {
            jmap_chat_types::Presence::Online => Ok(ContactPresenceFilter::Online),
            jmap_chat_types::Presence::Away => Ok(ContactPresenceFilter::Away),
            jmap_chat_types::Presence::Busy => Ok(ContactPresenceFilter::Busy),
            jmap_chat_types::Presence::Invisible => Ok(ContactPresenceFilter::Invisible),
            jmap_chat_types::Presence::Offline => Ok(ContactPresenceFilter::Offline),
            other => Err(other),
        }
    }
}

// ---------------------------------------------------------------------------
// QuotaScope
// ---------------------------------------------------------------------------

/// RFC 9425 §3.1 Scope — the set of accounts the quota limit applies to.
///
/// Wire strings: `"account"`, `"domain"`, `"global"`.
/// `Other(String)` preserves any unrecognized value for lossless round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaScope {
    /// Quota applies to this account only.
    Account,
    /// Quota applies to all accounts sharing this domain.
    Domain,
    /// Quota applies to all accounts on the server.
    Global,
    /// Catch-all for any unrecognized wire value from a future spec version.
    /// The original wire value is preserved for lossless round-trip.
    ///
    /// # Forging caveat
    ///
    /// `Other(String)` is `pub`, so callers can construct
    /// `QuotaScope::Other("account".into())`. The custom serde impl
    /// emits the wrapped string verbatim on serialize and normalises
    /// canonical wire strings to their typed variant on deserialize.
    /// Consequences:
    /// * `QuotaScope::Other("account".into()) != QuotaScope::Account`
    ///   on PartialEq, but both serialize to `"account"`.
    /// * `Other("account")` -> `"account"` -> `Account` is a lossy
    ///   round-trip (the variant changes shape).
    ///
    /// Reserve `Other(s)` for genuinely unrecognised wire strings.
    /// Comparing wire-string equality across two values requires
    /// matching on `as_str()`, not on `PartialEq`.
    Other(String),
}

impl QuotaScope {
    /// The canonical wire string for this quota scope.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Account => "account",
            Self::Domain => "domain",
            Self::Global => "global",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl_string_enum!(QuotaScope, "a QuotaScope wire string",
    "account" => Account,
    "domain"  => Domain,
    "global"  => Global,
);

// ---------------------------------------------------------------------------
// QuotaResourceType
// ---------------------------------------------------------------------------

/// RFC 9425 §3.2 ResourceType — the unit of measure for a quota.
///
/// Wire strings: `"count"`, `"octets"`.
/// `Other(String)` preserves any unrecognized value for lossless round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaResourceType {
    /// Quota measured in number of data-type objects.
    Count,
    /// Quota measured in size (octets / bytes).
    Octets,
    /// Catch-all for any unrecognized wire value from a future spec version.
    /// The original wire value is preserved for lossless round-trip.
    ///
    /// **Forging caveat**: see [`QuotaScope::Other`] for the full
    /// discussion. Constructing
    /// `QuotaResourceType::Other("count".into())` produces a value
    /// that is unequal to `QuotaResourceType::Count` on `PartialEq`
    /// but serialises to the same wire string and round-trips back
    /// to `QuotaResourceType::Count`. Reserve `Other(s)` for
    /// genuinely unrecognised wire strings; compare wire equality
    /// via `as_str()`, not `PartialEq`.
    Other(String),
}

impl QuotaResourceType {
    /// The canonical wire string for this resource type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Count => "count",
            Self::Octets => "octets",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl_string_enum!(QuotaResourceType, "a QuotaResourceType wire string",
    "count"  => Count,
    "octets" => Octets,
);

// ---------------------------------------------------------------------------
// ChatMemberRole
// ---------------------------------------------------------------------------

/// Role of a participant in a group Chat.
///
/// The spec defines two well-known values: `"admin"` and `"member"`.
/// `Other(String)` preserves any unrecognized value for lossless round-trip.
///
/// Wire strings: `"admin"`, `"member"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ChatMemberRole {
    /// Group or channel administrator with management permissions.
    Admin,
    /// Regular member.
    Member,
    /// Catch-all for any unrecognized wire value from a future spec version.
    ///
    /// **Forging caveat**: see [`QuotaScope::Other`] for the full
    /// discussion. Constructing `ChatMemberRole::Other("admin".into())`
    /// produces a value that is unequal to `ChatMemberRole::Admin` on
    /// `PartialEq` but serialises to the same wire string `"admin"`
    /// and round-trips back to `ChatMemberRole::Admin`. Reserve
    /// `Other(s)` for genuinely unrecognised wire strings; compare
    /// wire equality via `as_str()`, not `PartialEq`.
    Other(String),
}

impl ChatMemberRole {
    /// The canonical wire string for this role.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl_string_enum!(ChatMemberRole, "a ChatMemberRole wire string",
    "admin"  => Admin,
    "member" => Member,
);

// ---------------------------------------------------------------------------
// Wire-enum round-trip preservation tests
// ---------------------------------------------------------------------------
//
// Per workspace AGENTS.md "Extras-preservation policy" for in-scope result
// enums: each enum that carries an `Other(String)` catch-all MUST have a
// test asserting an unknown wire string deserialises into `Other(s)` and
// round-trips back to the same wire string.
//
// jmap_types::impl_string_enum!'s own test module exercises the macro
// logic; these tests exercise the per-enum (wire-string, variant)
// mapping registered by each invocation in this file. Independent
// oracles: each test uses a hand-chosen wire string that is provably
// outside the registered set for that enum.

#[cfg(test)]
mod tests {
    use super::*;

    /// QuotaScope: unknown wire string round-trips via Other(s).
    /// Oracle: `"siteCustom-tier-A"` is not in RFC 9425 §3.1
    /// `{account, domain, global}`.
    #[test]
    fn quota_scope_unknown_round_trips_via_other() {
        let raw = r#""siteCustom-tier-A""#;
        let parsed: QuotaScope = serde_json::from_str(raw).expect("must deserialize");
        assert_eq!(parsed, QuotaScope::Other("siteCustom-tier-A".to_owned()));
        assert_eq!(serde_json::to_string(&parsed).unwrap(), raw);
    }

    /// QuotaResourceType: unknown wire string round-trips via Other(s).
    /// Oracle: `"vendorUnit-decibels"` is not in RFC 9425 §3.2
    /// `{count, octets}`.
    #[test]
    fn quota_resource_type_unknown_round_trips_via_other() {
        let raw = r#""vendorUnit-decibels""#;
        let parsed: QuotaResourceType = serde_json::from_str(raw).expect("must deserialize");
        assert_eq!(
            parsed,
            QuotaResourceType::Other("vendorUnit-decibels".to_owned())
        );
        assert_eq!(serde_json::to_string(&parsed).unwrap(), raw);
    }

    /// ChatMemberRole: unknown wire string round-trips via Other(s).
    /// Oracle: `"moderator"` is not in draft-atwood-jmap-chat-00 §Chat
    /// roles `{admin, member}` — it is the canonical vendor-extension
    /// example the chat smoke tests use.
    #[test]
    fn chat_member_role_unknown_round_trips_via_other() {
        let raw = r#""moderator""#;
        let parsed: ChatMemberRole = serde_json::from_str(raw).expect("must deserialize");
        assert_eq!(parsed, ChatMemberRole::Other("moderator".to_owned()));
        assert_eq!(serde_json::to_string(&parsed).unwrap(), raw);
    }

    /// QuotaScope canonical variants round-trip correctly to/from their
    /// registered wire strings.
    #[test]
    fn quota_scope_canonical_variants_round_trip() {
        let cases: &[(&str, QuotaScope)] = &[
            (r#""account""#, QuotaScope::Account),
            (r#""domain""#, QuotaScope::Domain),
            (r#""global""#, QuotaScope::Global),
        ];
        for (raw, expected) in cases {
            let parsed: QuotaScope = serde_json::from_str(raw).expect("must deserialize");
            assert_eq!(
                &parsed, expected,
                "wire {raw} must deserialise to {expected:?}"
            );
            assert_eq!(
                serde_json::to_string(&parsed).unwrap(),
                *raw,
                "wire {raw} must round-trip"
            );
        }
    }
}
