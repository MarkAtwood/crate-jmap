//! JMAP Chat client-side auxiliary types.
//!
//! This module contains types used in client-facing APIs that are not part of
//! the wire-format types defined in `jmap-chat-types`.

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

impl std::fmt::Display for QuotaScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for QuotaScope {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for QuotaScope {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(match raw.as_str() {
            "account" => QuotaScope::Account,
            "domain" => QuotaScope::Domain,
            "global" => QuotaScope::Global,
            _ => QuotaScope::Other(raw),
        })
    }
}

// ---------------------------------------------------------------------------
// ChatMemberRole
// ---------------------------------------------------------------------------

/// Role of a participant in a group Chat.
///
/// The spec defines two well-known values: `"admin"` and `"member"`.
/// `Unknown(String)` preserves any unrecognized value for lossless round-trip.
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
    Unknown(String),
}

impl ChatMemberRole {
    /// The canonical wire string for this role.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Unknown(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for ChatMemberRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for ChatMemberRole {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ChatMemberRole {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(match raw.as_str() {
            "admin" => ChatMemberRole::Admin,
            "member" => ChatMemberRole::Member,
            _ => ChatMemberRole::Unknown(raw),
        })
    }
}

// ---------------------------------------------------------------------------
// BodyType
// ---------------------------------------------------------------------------

/// MIME type for a message body.
///
/// The spec defines three well-known values. `Unknown(String)` preserves any
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
    Unknown(String),
}

impl BodyType {
    /// The canonical MIME type string for this body type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Plain => "text/plain",
            Self::Markdown => "text/markdown",
            Self::Rich => "application/jmap-chat-rich",
            Self::Unknown(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for BodyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for BodyType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for BodyType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(match raw.as_str() {
            "text/plain" => BodyType::Plain,
            "text/markdown" => BodyType::Markdown,
            "application/jmap-chat-rich" => BodyType::Rich,
            _ => BodyType::Unknown(raw),
        })
    }
}
