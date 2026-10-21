//! `VTCCall/query`, `VTCParticipant/query`, `VTCRecording/query` and
//! `VTCLivestream/query` filter and comparator types
//! (draft-atwood-jmap-vtc-00 §VTCCall/query, §VTCParticipant/query,
//! §VTCRecording/query, §VTCLivestream/query; RFC 8620 §5.5).
//!
//! # Excluded from extras preservation
//!
//! Like every filter and comparator type in the workspace, these carry no
//! flatten-extras `extra` field and no `Other(String)` sort property: a
//! server that silently drops a filter clause or sort key it does not
//! understand returns the wrong result set. The condition carries
//! `#[serde(deny_unknown_fields)]`, so an unknown clause fails the decode
//! and servers map that to `unsupportedFilter`. See the workspace
//! AGENTS.md "Filter algebra and control enums are explicitly EXCLUDED".
//!
//! # Construction from outside this crate
//!
//! ```rust
//! use jmap_vtc_types::query::{VtcCallFilter, VtcCallFilterCondition};
//! use jmap_vtc_types::CallState;
//!
//! let mut cond = VtcCallFilterCondition::default();
//! cond.state = Some(CallState::Active);
//! let filter = VtcCallFilter::Condition(cond);
//! ```

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::{
    CallRole, CallState, CallType, JoinMethod, LivestreamState, LobbyState, RecordingState,
};

pub use jmap_types::query::{Filter, FilterOperator, Operator};

/// Filter for `VTCCall/query`.
pub type VtcCallFilter = Filter<VtcCallFilterCondition>;

/// Filter condition for `VTCCall/query`
/// (draft-atwood-jmap-vtc-00 §VTCCall/query).
///
/// "All filter properties are combined with logical AND." `chatId` and
/// `spaceId` are "Only valid when `urn:ietf:params:jmap:chat` is present;
/// servers MUST return `unsupportedFilter` otherwise."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VtcCallFilterCondition {
    /// The call's `state` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<CallState>,
    /// The call's `callType` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_type: Option<CallType>,
    /// The call's `initiatorId` must equal this userId.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initiator_id: Option<String>,
    /// The call's `chatId` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<Id>,
    /// The call's `spaceId` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,
    /// This userId "is or was a participant" in the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_participant: Option<String>,
    /// The call was "created at or after this time".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<UTCDate>,
    /// The call was "created before this time".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<UTCDate>,
    /// The call "ended at or after this time".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_after: Option<UTCDate>,
    /// The call "ended before this time".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_before: Option<UTCDate>,
}

/// Sort property for `VTCCall/query`: "Sort properties: `createdAt`,
/// `endedAt`. Default sort: `createdAt` descending."
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VtcCallSortProperty {
    /// Sort by `createdAt`.
    CreatedAt,
    /// Sort by `endedAt`.
    EndedAt,
}

/// RFC 8620 §5.5 `Comparator` for `VTCCall/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VtcCallComparator {
    /// The property to sort by.
    pub property: VtcCallSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl VtcCallComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: VtcCallSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Filter for `VTCParticipant/query`.
pub type VtcParticipantFilter = Filter<VtcParticipantFilterCondition>;

/// Filter condition for `VTCParticipant/query`
/// (draft-atwood-jmap-vtc-00 §VTCParticipant/query).
///
/// "All filter properties are combined with logical AND." Servers "SHOULD
/// require" `callId "on every query unless combined with `userId`".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VtcParticipantFilterCondition {
    /// "Filter to participants in this call."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<Id>,
    /// "Filter to a specific user across calls."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// The participant's `role` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<CallRole>,
    /// `true`: "participants with `joinedAt != null` and `leftAt ==
    /// null`"; `false`: "participants who have left".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    /// The participant's `lobbyState` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lobby_state: Option<LobbyState>,
    /// The participant's `joinMethod` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join_method: Option<JoinMethod>,
}

/// Sort property for `VTCParticipant/query`: "Sort properties: `joinedAt`,
/// `displayName`. Default sort: `joinedAt` ascending."
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VtcParticipantSortProperty {
    /// Sort by `joinedAt`.
    JoinedAt,
    /// Sort by `displayName`.
    DisplayName,
}

/// RFC 8620 §5.5 `Comparator` for `VTCParticipant/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VtcParticipantComparator {
    /// The property to sort by.
    pub property: VtcParticipantSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl VtcParticipantComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: VtcParticipantSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

/// Filter for `VTCRecording/query`.
pub type VtcRecordingFilter = Filter<VtcRecordingFilterCondition>;

/// Filter condition for `VTCRecording/query`
/// (draft-atwood-jmap-vtc-00 §VTCRecording/query).
///
/// "All filter properties are combined with logical AND."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VtcRecordingFilterCondition {
    /// "Filter to recordings for this call."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<Id>,
    /// The recording's `state` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<RecordingState>,
    /// "Filter to recordings started by this userId."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<String>,
    /// "Recordings started at or after this time."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_after: Option<UTCDate>,
    /// "Recordings started before this time."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_before: Option<UTCDate>,
}

/// Filter for `VTCLivestream/query`.
pub type VtcLivestreamFilter = Filter<VtcLivestreamFilterCondition>;

/// Filter condition for `VTCLivestream/query`
/// (draft-atwood-jmap-vtc-00 §VTCLivestream/query).
///
/// "All filter properties are combined with logical AND."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VtcLivestreamFilterCondition {
    /// "Filter to livestreams for this call."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<Id>,
    /// The livestream's `state` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<LivestreamState>,
    /// "Filter by platform identifier."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

/// Sort property for `VTCRecording/query` and `VTCLivestream/query`: "Sort
/// properties: `startedAt`. Default sort: `startedAt` descending."
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VtcMediaSortProperty {
    /// Sort by `startedAt`.
    StartedAt,
}

/// RFC 8620 §5.5 `Comparator` for `VTCRecording/query` and
/// `VTCLivestream/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VtcMediaComparator {
    /// The property to sort by.
    pub property: VtcMediaSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl VtcMediaComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: VtcMediaSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn condition_carries_every_spec_filter_property() {
        // Oracle: draft-atwood-jmap-vtc-00 L725-L755 lists `state`,
        // `callType`, `initiatorId`, `chatId`, `spaceId`, `hasParticipant`,
        // `createdAfter`, `createdBefore`, `endedAfter`, `endedBefore`.
        let wire = json!({
            "state": "active",
            "callType": "room",
            "initiatorId": "user:bob@example.com",
            "chatId": "C1",
            "spaceId": "S1",
            "hasParticipant": "user:alice@example.com",
            "createdAfter": "2026-06-01T00:00:00Z",
            "createdBefore": "2026-07-01T00:00:00Z",
            "endedAfter": "2026-06-02T00:00:00Z",
            "endedBefore": "2026-07-02T00:00:00Z"
        });
        let cond: VtcCallFilterCondition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cond.state, Some(CallState::Active));
        assert_eq!(cond.call_type, Some(CallType::Room));
        assert_eq!(serde_json::to_value(&cond).unwrap(), wire);
    }

    #[test]
    fn unknown_filter_property_is_rejected() {
        // Oracle: RFC 8620 §5.5 "unsupportedFilter"; workspace policy maps
        // an unknown clause to a decode failure.
        assert!(serde_json::from_value::<VtcCallFilter>(json!({"bogus": 1})).is_err());
        assert!(serde_json::from_value::<VtcCallFilter>(json!({
            "operator": "AND",
            "conditions": [{"state": "ended"}, {"bogus": 1}]
        }))
        .is_err());
    }

    #[test]
    fn comparator_accepts_only_spec_sort_properties() {
        // Oracle: L759 "Sort properties: `createdAt`, `endedAt`."
        let c: VtcCallComparator = serde_json::from_value(json!({"property": "endedAt"})).unwrap();
        assert_eq!(c.property, VtcCallSortProperty::EndedAt);
        // RFC 8620 §5.5 isAscending "default: true".
        assert!(c.is_ascending);
        assert!(
            serde_json::from_value::<VtcCallComparator>(json!({"property": "subject"})).is_err()
        );
    }

    #[test]
    fn participant_condition_carries_every_spec_filter_property() {
        // Oracle: draft-atwood-jmap-vtc-00 L1074-1094 lists `callId`,
        // `userId`, `role`, `isActive`, `lobbyState`, `joinMethod`.
        let wire = json!({
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "userId": "user:alice@example.com",
            "role": "moderator",
            "isActive": true,
            "lobbyState": "waiting",
            "joinMethod": "pstn"
        });
        let cond: VtcParticipantFilterCondition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cond.role, Some(CallRole::Moderator));
        assert_eq!(cond.is_active, Some(true));
        assert_eq!(cond.join_method, Some(JoinMethod::Pstn));
        assert_eq!(serde_json::to_value(&cond).unwrap(), wire);
        assert!(serde_json::from_value::<VtcParticipantFilter>(json!({"bogus": 1})).is_err());
    }

    #[test]
    fn participant_comparator_accepts_only_spec_sort_properties() {
        // Oracle: L1096 "Sort properties: `joinedAt`, `displayName`."
        for (wire, want) in [
            ("joinedAt", VtcParticipantSortProperty::JoinedAt),
            ("displayName", VtcParticipantSortProperty::DisplayName),
        ] {
            let c: VtcParticipantComparator =
                serde_json::from_value(json!({ "property": wire })).unwrap();
            assert_eq!(c.property, want);
            assert!(c.is_ascending);
        }
        assert!(serde_json::from_value::<VtcParticipantComparator>(
            json!({"property": "speakerTimeMs"})
        )
        .is_err());
    }

    #[test]
    fn recording_condition_carries_every_spec_filter_property() {
        // Oracle: draft-atwood-jmap-vtc-00 L1110-1123 lists `callId`,
        // `state`, `initiatedBy`, `startedAfter`, `startedBefore`.
        let wire = json!({
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "state": "paused",
            "initiatedBy": "user:bob@example.com",
            "startedAfter": "2026-06-01T00:00:00Z",
            "startedBefore": "2026-07-01T00:00:00Z"
        });
        let cond: VtcRecordingFilterCondition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cond.state, Some(RecordingState::Paused));
        assert_eq!(serde_json::to_value(&cond).unwrap(), wire);
        assert!(serde_json::from_value::<VtcRecordingFilter>(json!({"platform": "x"})).is_err());
    }

    #[test]
    fn livestream_condition_carries_every_spec_filter_property() {
        // Oracle: L1215-1222 lists `callId`, `state`, `platform`.
        let wire = json!({
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "state": "live",
            "platform": "youtube"
        });
        let cond: VtcLivestreamFilterCondition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cond.state, Some(LivestreamState::Live));
        assert_eq!(serde_json::to_value(&cond).unwrap(), wire);
        assert!(
            serde_json::from_value::<VtcLivestreamFilter>(json!({"initiatedBy": "x"})).is_err()
        );
    }

    #[test]
    fn media_comparator_accepts_only_started_at() {
        // Oracle: L1127 and L1226 "Sort properties: `startedAt`."
        let c: VtcMediaComparator =
            serde_json::from_value(json!({"property": "startedAt"})).unwrap();
        assert_eq!(c.property, VtcMediaSortProperty::StartedAt);
        assert!(c.is_ascending);
        assert!(
            serde_json::from_value::<VtcMediaComparator>(json!({"property": "stoppedAt"})).is_err()
        );
    }
}
