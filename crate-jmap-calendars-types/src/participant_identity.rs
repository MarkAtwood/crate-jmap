//! ParticipantIdentity object.
//!
//! Normative reference: draft-ietf-jmap-calendars-26 §3.

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// A JMAP ParticipantIdentity object (draft-ietf-jmap-calendars-26 §3).
///
/// Represents a URI (typically a `mailto:` address) that identifies the
/// authenticated user for iTIP scheduling purposes within a calendar account.
/// Each account may have multiple participant identities, and at most one may
/// be marked as the default.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantIdentity {
    /// Server-assigned immutable identifier.
    ///
    /// `Option<Id>` with `skip_serializing_if` so /set create payloads
    /// constructed by a typed caller do NOT emit a client-supplied `id`
    /// on the wire. Per RFC 8620 §5.3, the `id` property MUST NOT be set
    /// in the create object — the server assigns it. Matches the
    /// in-crate sibling `CalendarEvent.id`
    /// (crate-jmap-calendars-types/src/event.rs:43-44).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// Display name to use when adding this identity as a participant
    /// (default `""`).
    pub name: String,

    /// The URI that represents this participant for iTIP scheduling
    /// (e.g. `"mailto:user@example.com"`).
    pub calendar_address: String,

    /// Server-set; `true` for at most one ParticipantIdentity per account.
    pub is_default: bool,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
