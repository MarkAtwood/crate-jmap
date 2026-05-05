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
    pub id: Id,

    /// Display name to use when adding this identity as a participant
    /// (default `""`).
    pub name: String,

    /// The URI that represents this participant for iTIP scheduling
    /// (e.g. `"mailto:user@example.com"`).
    pub calendar_address: String,

    /// Server-set; `true` for at most one ParticipantIdentity per account.
    pub is_default: bool,
}
