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
// BodyType
// ---------------------------------------------------------------------------

/// MIME type for a message body.
///
/// The spec defines three well-known values. `Other(String)` preserves any
/// unrecognized MIME type for lossless round-trip.
///
/// Wire strings: `"text/plain"`, `"text/markdown"`, `"application/jmap-chat-rich"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum BodyType {
    /// `"text/plain"` — unformatted UTF-8 text.
    Plain,
    /// `"text/markdown"` — CommonMark-formatted text.
    Markdown,
    /// `"application/jmap-chat-rich"` — structured rich text (spans array).
    Rich,
    /// Any unrecognized MIME type string, preserved as-is.
    ///
    /// **Forging caveat**: see [`QuotaScope::Other`] for the full
    /// discussion. Constructing `BodyType::Other("text/plain".into())`
    /// produces a value that is unequal to `BodyType::Plain` on
    /// `PartialEq` but serialises to the same wire string and
    /// round-trips back to `BodyType::Plain`. Reserve `Other(s)` for
    /// genuinely unrecognised MIME types; compare wire equality via
    /// `as_str()`, not `PartialEq`.
    Other(String),
}

impl BodyType {
    /// The canonical MIME type string for this body type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Plain => "text/plain",
            Self::Markdown => "text/markdown",
            Self::Rich => "application/jmap-chat-rich",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl_string_enum!(BodyType, "a BodyType MIME-string",
    "text/plain"                 => Plain,
    "text/markdown"              => Markdown,
    "application/jmap-chat-rich" => Rich,
);
