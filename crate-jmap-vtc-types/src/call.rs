//! draft-atwood-jmap-vtc-00 §VTCMediaState, §VTCCallPolicy and §VTCCall.

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::MediaKind;

/// Placeholder printed by `Debug` impls in place of secret values.
pub(crate) const REDACTED: &str = "[REDACTED]";

/// The `callType` of a [`VtcCall`] (draft-atwood-jmap-vtc-00 §VTCCall).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CallType {
    /// `"ring"`: ring-and-answer.
    Ring,
    /// `"room"`: drop-in room.
    Room,
    /// `"scheduled"`: scheduled call.
    Scheduled,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(CallType, "a VTC call type string",
    "ring" => Ring,
    "room" => Room,
    "scheduled" => Scheduled,
);

/// The `state` of a [`VtcCall`] (draft-atwood-jmap-vtc-00 §State Machine).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CallState {
    /// `"creating"`: a ring call before its rings are dispatched.
    Creating,
    /// `"ringing"`: a ring call waiting for an answer.
    Ringing,
    /// `"active"`.
    Active,
    /// `"pending"`: a scheduled call nobody has joined yet.
    Pending,
    /// `"ended"`: terminal.
    Ended,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(CallState, "a VTC call state string",
    "creating" => Creating,
    "ringing" => Ringing,
    "active" => Active,
    "pending" => Pending,
    "ended" => Ended,
);

/// The `endReason` of a [`VtcCall`]: the call-level outcome
/// (draft-atwood-jmap-vtc-00 §VTCCall).
///
/// Distinct from [`RingEndReason`](crate::RingEndReason), the per-device
/// ring disposition of a `VTCCallEndEvent`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CallEndReason {
    /// `"completed"`: an active call ended normally.
    Completed,
    /// `"missed"`: a ring call timed out with no answer.
    Missed,
    /// `"declined"`: all ring-call targets declined.
    Declined,
    /// `"cancelled"`: the initiator withdrew before anyone answered or
    /// joined.
    Cancelled,
    /// `"failed"`: a server or media-layer error.
    Failed,
    /// `"timeout"`: a deployment inactivity timeout on an empty active
    /// call.
    Timeout,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(CallEndReason, "a VTC call end reason string",
    "completed" => Completed,
    "missed" => Missed,
    "declined" => Declined,
    "cancelled" => Cancelled,
    "failed" => Failed,
    "timeout" => Timeout,
);

/// A participant's self-reported media state
/// (draft-atwood-jmap-vtc-00 §VTCMediaState).
///
/// "The server relays this information without verification."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcMediaState {
    /// `true` if the microphone is active.
    pub audio: bool,
    /// `true` if the camera is active.
    pub video: bool,
    /// `true` if the participant is sharing their screen.
    pub screen: bool,
    /// `true` if the participant has raised their hand.
    pub raised_hand: bool,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Moderator-controlled media policies of a call
/// (draft-atwood-jmap-vtc-00 §VTCCallPolicy).
///
/// A missing field takes its draft default, which is also what
/// [`Default`] returns: `muteOnEntry` and `videoOffOnEntry` `false`, the
/// three `participantsCan*` fields `true`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VtcCallPolicy {
    /// Participants join with `mediaState.audio` `false`.
    pub mute_on_entry: bool,
    /// Participants join with `mediaState.video` `false`.
    pub video_off_on_entry: bool,
    /// Non-moderators may set their own `mediaState.audio` to `true`.
    pub participants_can_unmute: bool,
    /// Non-moderators may set their own `mediaState.screen` to `true`.
    pub participants_can_share_screen: bool,
    /// Non-moderators may set their own `mediaState.video` to `true`.
    pub participants_can_start_video: bool,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for VtcCallPolicy {
    fn default() -> Self {
        Self {
            mute_on_entry: false,
            video_off_on_entry: false,
            participants_can_unmute: true,
            participants_can_share_screen: true,
            participants_can_start_video: true,
            extra: serde_json::Map::new(),
        }
    }
}

/// A voice or video call session (draft-atwood-jmap-vtc-00 §VTCCall).
///
/// `T|null` properties are `Option<T>` and serialize `None` as `null`.
/// The binding properties (`chatId`, `spaceId`, `channelId`,
/// `calendarEventId`) are "present only when the corresponding companion
/// capability is advertised", so each is `Option<Option<Id>>`: `None` when
/// absent, `Some(None)` for `null`.
///
/// The `Debug` impl redacts `joinPassword` and `gatewayPin`.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcCall {
    /// A ULID assigned at creation.
    pub id: Id,
    /// The account that owns this call. `None` when absent: the draft's
    /// own `VTCCall/get` example (L527, "properties abridged") omits it,
    /// and the `/get` response already carries `accountId`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<Id>,
    /// Ring, room or scheduled.
    pub call_type: CallType,
    /// Current state.
    pub state: CallState,
    /// Time the call was created.
    pub created_at: UTCDate,
    /// Time the call ended; `None` while active or pending.
    pub ended_at: Option<UTCDate>,
    /// Why the call ended; `None` until it has.
    pub end_reason: Option<CallEndReason>,
    /// The userId of the participant who created the call.
    pub initiator_id: String,
    /// Human-readable call title.
    pub subject: Option<String>,
    /// The media-layer entry point. "Peer-supplied; MUST be treated as
    /// untrusted. Clients MUST NOT connect to this URI without explicit
    /// user initiation."
    pub join_uri: String,
    /// Password or PIN required at the media layer.
    pub join_password: Option<String>,
    /// Media types active for this call.
    pub media_types: Vec<MediaKind>,
    /// Participants currently in the call.
    pub active_participant_count: u64,
    /// New participants wait in a lobby until a moderator admits them.
    /// Draft default `false`.
    #[serde(default)]
    pub lobby_enabled: bool,
    /// Call-level media policies; missing means the draft defaults.
    #[serde(default)]
    pub policy: VtcCallPolicy,
    /// For a breakout room, the parent call.
    pub parent_call_id: Option<Id>,
    /// For a parent call, its breakout-room calls.
    #[serde(default)]
    pub breakout_room_ids: Vec<Id>,
    /// For a scheduled call, the intended start time.
    pub scheduled_start_at: Option<UTCDate>,
    /// PIN for joining via a protocol gateway.
    pub gateway_pin: Option<String>,
    /// End-to-end encryption is active for the media. Draft default
    /// `false`.
    #[serde(default)]
    pub e2ee_enabled: bool,
    /// Bound Chat (`urn:ietf:params:jmap:chat`).
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub chat_id: Option<Option<Id>>,
    /// Bound Space (`urn:ietf:params:jmap:chat`).
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub space_id: Option<Option<Id>>,
    /// Bound channel Chat within the Space (`urn:ietf:params:jmap:chat`).
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub channel_id: Option<Option<Id>>,
    /// Bound CalendarEvent (`urn:ietf:params:jmap:calendars`).
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub calendar_event_id: Option<Option<Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl std::fmt::Debug for VtcCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VtcCall")
            .field("id", &self.id)
            .field("account_id", &self.account_id)
            .field("call_type", &self.call_type)
            .field("state", &self.state)
            .field("created_at", &self.created_at)
            .field("ended_at", &self.ended_at)
            .field("end_reason", &self.end_reason)
            .field("initiator_id", &self.initiator_id)
            .field("subject", &self.subject)
            .field("join_uri", &self.join_uri)
            .field(
                "join_password",
                &self.join_password.as_ref().map(|_| REDACTED),
            )
            .field("media_types", &self.media_types)
            .field("active_participant_count", &self.active_participant_count)
            .field("lobby_enabled", &self.lobby_enabled)
            .field("policy", &self.policy)
            .field("parent_call_id", &self.parent_call_id)
            .field("breakout_room_ids", &self.breakout_room_ids)
            .field("scheduled_start_at", &self.scheduled_start_at)
            .field("gateway_pin", &self.gateway_pin.as_ref().map(|_| REDACTED))
            .field("e2ee_enabled", &self.e2ee_enabled)
            .field("chat_id", &self.chat_id)
            .field("space_id", &self.space_id)
            .field("channel_id", &self.channel_id)
            .field("calendar_event_id", &self.calendar_event_id)
            .field("extra", &self.extra)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A room call with no companion capabilities, hand-written from the
    /// field list of draft-atwood-jmap-vtc-00 L342-406.
    fn standalone_room() -> serde_json::Value {
        json!({
            "id": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "accountId": "A1",
            "callType": "room",
            "state": "active",
            "createdAt": "2026-06-05T14:30:00Z",
            "endedAt": null,
            "endReason": null,
            "initiatorId": "user:bob@example.com",
            "subject": null,
            "joinUri": "https://meet.example.com/room/xkz",
            "joinPassword": "pin-canary-1234",
            "mediaTypes": ["audio"],
            "activeParticipantCount": 1,
            "lobbyEnabled": false,
            "policy": {
                "muteOnEntry": false,
                "videoOffOnEntry": false,
                "participantsCanUnmute": true,
                "participantsCanShareScreen": true,
                "participantsCanStartVideo": true
            },
            "parentCallId": null,
            "breakoutRoomIds": [],
            "scheduledStartAt": null,
            "gatewayPin": "gw-canary-5678",
            "e2eeEnabled": false
        })
    }

    #[test]
    fn call_type_wire_values() {
        // Oracle: L349 "`\"ring\"`, `\"room\"`, or `\"scheduled\"`".
        let v: Vec<CallType> =
            serde_json::from_value(json!(["ring", "room", "scheduled"])).unwrap();
        assert_eq!(v, [CallType::Ring, CallType::Room, CallType::Scheduled]);
    }

    #[test]
    fn call_state_wire_values() {
        // Oracle: L728 "One of `\"creating\"`, `\"ringing\"`, `\"active\"`,
        // `\"pending\"`, or `\"ended\"`".
        let v: Vec<CallState> =
            serde_json::from_value(json!(["creating", "ringing", "active", "pending", "ended"]))
                .unwrap();
        assert_eq!(
            v,
            [
                CallState::Creating,
                CallState::Ringing,
                CallState::Active,
                CallState::Pending,
                CallState::Ended
            ]
        );
    }

    #[test]
    fn call_end_reason_wire_values() {
        // Oracle: L362-367 lists "completed", "missed", "declined",
        // "cancelled", "failed", "timeout".
        let v: Vec<CallEndReason> = serde_json::from_value(json!([
            "completed",
            "missed",
            "declined",
            "cancelled",
            "failed",
            "timeout"
        ]))
        .unwrap();
        assert_eq!(
            v,
            [
                CallEndReason::Completed,
                CallEndReason::Missed,
                CallEndReason::Declined,
                CallEndReason::Cancelled,
                CallEndReason::Failed,
                CallEndReason::Timeout
            ]
        );
    }

    #[test]
    fn unknown_enum_values_round_trip() {
        let s: CallState = serde_json::from_value(json!("x-hold")).unwrap();
        assert_eq!(s, CallState::Other("x-hold".to_owned()));
        assert_eq!(serde_json::to_value(&s).unwrap(), json!("x-hold"));
        let t: CallType = serde_json::from_value(json!("x-conference")).unwrap();
        assert_eq!(serde_json::to_value(&t).unwrap(), json!("x-conference"));
        let r: CallEndReason = serde_json::from_value(json!("x-lost")).unwrap();
        assert_eq!(serde_json::to_value(&r).unwrap(), json!("x-lost"));
    }

    #[test]
    fn policy_defaults_match_draft() {
        // Oracle: L257 muteOnEntry "Default: `false`"; L260
        // videoOffOnEntry "Default: `false`"; L263, L266, L269 the three
        // participantsCan* fields "Default: `true`".
        let p: VtcCallPolicy = serde_json::from_value(json!({})).unwrap();
        assert!(!p.mute_on_entry);
        assert!(!p.video_off_on_entry);
        assert!(p.participants_can_unmute);
        assert!(p.participants_can_share_screen);
        assert!(p.participants_can_start_video);
        assert_eq!(p, VtcCallPolicy::default());
    }

    #[test]
    fn standalone_call_omits_binding_fields() {
        // Oracle: L410 "The following fields are present only when the
        // corresponding companion capability is advertised".
        let raw = standalone_room();
        let call: VtcCall = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(call.chat_id, None);
        assert_eq!(call.calendar_event_id, None);
        assert_eq!(serde_json::to_value(&call).unwrap(), raw);
    }

    #[test]
    fn null_binding_distinct_from_absent() {
        // Oracle: L413 chatId "`null` if the call has no chat binding".
        let mut raw = standalone_room();
        raw["chatId"] = json!(null);
        raw["spaceId"] = json!("S1");
        let call: VtcCall = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(call.chat_id, Some(None));
        assert_eq!(call.space_id, Some(Some(Id::from("S1"))));
        assert_eq!(call.channel_id, None);
        assert_eq!(serde_json::to_value(&call).unwrap(), raw);
    }

    #[test]
    fn nullable_fields_serialize_as_null() {
        // Oracle: L357 endedAt "(UTCDate|null ...) `null` while the call is
        // active or pending": a present property with a null value.
        let call: VtcCall = serde_json::from_value(standalone_room()).unwrap();
        let out = serde_json::to_value(&call).unwrap();
        for key in [
            "endedAt",
            "endReason",
            "subject",
            "parentCallId",
            "scheduledStartAt",
        ] {
            assert_eq!(out.get(key), Some(&json!(null)), "{key}");
        }
    }

    #[test]
    fn required_fields_rejected_when_missing() {
        // Oracle: L342-406 mark these without "optional" and without a
        // default.
        for field in [
            "id",
            "callType",
            "state",
            "createdAt",
            "initiatorId",
            "joinUri",
            "mediaTypes",
            "activeParticipantCount",
        ] {
            let mut raw = standalone_room();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<VtcCall>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn debug_redacts_join_password_and_gateway_pin() {
        let call: VtcCall = serde_json::from_value(standalone_room()).unwrap();
        let dbg = format!("{call:?}");
        assert!(!dbg.contains("pin-canary-1234"), "{dbg}");
        assert!(!dbg.contains("gw-canary-5678"), "{dbg}");
        assert!(dbg.contains("[REDACTED]"));
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = standalone_room();
        raw["vendor.example:room"] = json!({"floor": 3});
        raw["policy"]["vendor.example:noiseSuppression"] = json!(true);
        let call: VtcCall = serde_json::from_value(raw.clone()).unwrap();
        assert!(call.extra.contains_key("vendor.example:room"));
        assert_eq!(serde_json::to_value(&call).unwrap(), raw);

        let ms = json!({
            "audio": true, "video": false, "screen": false, "raisedHand": true,
            "vendor.example:blur": true
        });
        let parsed: VtcMediaState = serde_json::from_value(ms.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), ms);
    }
}
