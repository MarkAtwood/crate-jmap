//! JMAP Chat client-side auxiliary types.
//!
//! This module contains types used in client-facing APIs that are not part of
//! the wire-format types defined in `jmap-chat-types`.

use jmap_types::impl_string_enum;

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

/// Re-exported from `jmap-chat-types`, which owns the `ChatMember.role`
/// wire vocabulary shared by client and server.
pub use jmap_chat_types::ChatMemberRole;

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
