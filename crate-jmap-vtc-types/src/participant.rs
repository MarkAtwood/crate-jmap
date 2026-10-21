//! draft-atwood-jmap-vtc-00 §VTCParticipant.

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::VtcMediaState;

/// The VTC `role` of a participant (draft-atwood-jmap-vtc-00
/// §VTCParticipant). Independent of any Chat or Space role.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CallRole {
    /// `"moderator"`: may perform moderation actions.
    Moderator,
    /// `"participant"`.
    Participant,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(CallRole, "a VTC participant role string",
    "moderator" => Moderator,
    "participant" => Participant,
);

/// A ring target's `callResponse` (draft-atwood-jmap-vtc-00
/// §VTCParticipant).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CallResponse {
    /// `"pending"`: not yet responded.
    Pending,
    /// `"accepted"`.
    Accepted,
    /// `"declined"`.
    Declined,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(CallResponse, "a VTC call response string",
    "pending" => Pending,
    "accepted" => Accepted,
    "declined" => Declined,
);

/// A participant's `lobbyState` (draft-atwood-jmap-vtc-00
/// §VTCParticipant).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LobbyState {
    /// `"waiting"`.
    Waiting,
    /// `"admitted"`.
    Admitted,
    /// `"rejected"`.
    Rejected,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(LobbyState, "a VTC lobby state string",
    "waiting" => Waiting,
    "admitted" => Admitted,
    "rejected" => Rejected,
);

/// How a participant connected: its `joinMethod`
/// (draft-atwood-jmap-vtc-00 §VTCParticipant).
///
/// The draft registers `"webrtc"`, `"sip"`, `"pstn"` and `"h323"`;
/// reverse-domain values land in `Other`, which "Clients MUST ignore" for
/// dispatch but which round-trips verbatim.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JoinMethod {
    /// `"webrtc"`.
    Webrtc,
    /// `"sip"`.
    Sip,
    /// `"pstn"`.
    Pstn,
    /// `"h323"`.
    H323,
    /// A reverse-domain or other unregistered connection method.
    Other(String),
}

impl_string_enum!(JoinMethod, "a VTC join method string",
    "webrtc" => Webrtc,
    "sip" => Sip,
    "pstn" => Pstn,
    "h323" => H323,
);

/// One participant in a call (draft-atwood-jmap-vtc-00 §VTCParticipant).
///
/// `id` is opaque. The draft says the server "SHOULD use the userId as the
/// id within a given call", so ids such as `"user:alice@example.com"`
/// occur; [`Id`] does not validate on deserialize and carries them.
///
/// `T|null` properties are `Option<T>` and serialize `None` as `null`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcParticipant {
    /// Opaque server-assigned identifier.
    pub id: Id,
    /// The call this participant belongs to.
    pub call_id: Id,
    /// The authenticated user identity; `None` for PSTN callers and
    /// anonymous guests.
    pub user_id: Option<String>,
    /// Display name, set at join time.
    pub display_name: String,
    /// Moderator or participant.
    pub role: CallRole,
    /// Time the participant joined; `None` if invited but not joined.
    pub joined_at: Option<UTCDate>,
    /// Time the participant left; `None` while in the call.
    pub left_at: Option<UTCDate>,
    /// A ring target's response; `None` for participants who were not
    /// ring targets.
    pub call_response: Option<CallResponse>,
    /// How this participant connected.
    pub join_method: JoinMethod,
    /// Protocol-specific gateway state, relayed without interpretation
    /// except for the reserved `"pendingSignal"` key.
    pub gateway_data: Option<serde_json::Map<String, serde_json::Value>>,
    /// Lobby state; `None` when no lobby is active or the participant
    /// bypassed it.
    pub lobby_state: Option<LobbyState>,
    /// Current self-reported media state.
    pub media_state: VtcMediaState,
    /// Cumulative talk time in milliseconds; `0` when the server cannot
    /// track it.
    pub speaker_time_ms: u64,
    /// The userId of the moderator who removed this participant.
    pub kicked_by: Option<String>,
    /// The participant's E2EE public key fingerprint, media-layer format.
    pub e2ee_fingerprint: Option<String>,
    /// A moderator asked this participant to unmute. Draft default
    /// `false`.
    #[serde(default)]
    pub unmute_requested: bool,
    /// The child breakout-room call a moderator has assigned this
    /// participant to; `None` (serialized as `null`, the draft default)
    /// when unassigned.
    #[serde(default)]
    pub breakout_call_id: Option<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A PSTN participant, hand-written from draft-atwood-jmap-vtc-00
    /// L281-330, using the PSTN `gatewayData` example of L310.
    fn pstn() -> serde_json::Value {
        json!({
            "id": "gw-pstn-14155559876",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "userId": null,
            "displayName": "Alice Smith",
            "role": "participant",
            "joinedAt": "2026-06-05T14:40:00Z",
            "leftAt": null,
            "callResponse": null,
            "joinMethod": "pstn",
            "gatewayData": {"callerIdNumber": "+14155559876", "callerIdName": "Alice Smith"},
            "lobbyState": "waiting",
            "mediaState": {"audio": true, "video": false, "screen": false, "raisedHand": false},
            "speakerTimeMs": 0,
            "kickedBy": null,
            "e2eeFingerprint": null,
            "unmuteRequested": false,
            "breakoutCallId": null
        })
    }

    #[test]
    fn pstn_participant_round_trips() {
        let raw = pstn();
        let p: VtcParticipant = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(p.user_id, None);
        assert_eq!(p.join_method, JoinMethod::Pstn);
        assert_eq!(p.lobby_state, Some(LobbyState::Waiting));
        assert_eq!(
            p.gateway_data.as_ref().unwrap()["callerIdNumber"],
            json!("+14155559876")
        );
        assert_eq!(serde_json::to_value(&p).unwrap(), raw);
    }

    #[test]
    fn enum_wire_values() {
        // Oracle: L294 role "`\"moderator\"` or `\"participant\"`"; L303
        // callResponse "`\"pending\"` ..., `\"accepted\"`, `\"declined\"`";
        // L315 lobbyState "`\"waiting\"`, `\"admitted\"`, `\"rejected\"`";
        // L306 joinMethod "registers `\"webrtc\"`, `\"sip\"`, `\"pstn\"`, and
        // `\"h323\"`".
        let r: Vec<CallRole> = serde_json::from_value(json!(["moderator", "participant"])).unwrap();
        assert_eq!(r, [CallRole::Moderator, CallRole::Participant]);
        let c: Vec<CallResponse> =
            serde_json::from_value(json!(["pending", "accepted", "declined"])).unwrap();
        assert_eq!(
            c,
            [
                CallResponse::Pending,
                CallResponse::Accepted,
                CallResponse::Declined
            ]
        );
        let l: Vec<LobbyState> =
            serde_json::from_value(json!(["waiting", "admitted", "rejected"])).unwrap();
        assert_eq!(
            l,
            [
                LobbyState::Waiting,
                LobbyState::Admitted,
                LobbyState::Rejected
            ]
        );
        let j: Vec<JoinMethod> =
            serde_json::from_value(json!(["webrtc", "sip", "pstn", "h323"])).unwrap();
        assert_eq!(
            j,
            [
                JoinMethod::Webrtc,
                JoinMethod::Sip,
                JoinMethod::Pstn,
                JoinMethod::H323
            ]
        );
    }

    #[test]
    fn reverse_domain_join_method_round_trips() {
        // Oracle: L306 "Vendor-specific connection methods SHOULD use
        // reverse-domain notation (e.g., `\"com.apple.facetime\"`). Clients
        // MUST ignore unrecognized values."
        let j: JoinMethod = serde_json::from_value(json!("com.apple.facetime")).unwrap();
        assert_eq!(j, JoinMethod::Other("com.apple.facetime".to_owned()));
        assert_eq!(
            serde_json::to_value(&j).unwrap(),
            json!("com.apple.facetime")
        );
        let r: CallRole = serde_json::from_value(json!("x-presenter")).unwrap();
        assert_eq!(serde_json::to_value(&r).unwrap(), json!("x-presenter"));
    }

    #[test]
    fn unmute_requested_defaults_false() {
        // Oracle: L330 unmuteRequested "Default: `false`."
        let mut raw = pstn();
        raw.as_object_mut().unwrap().remove("unmuteRequested");
        let p: VtcParticipant = serde_json::from_value(raw).unwrap();
        assert!(!p.unmute_requested);
    }

    #[test]
    fn breakout_call_id_defaults_null_and_round_trips() {
        // Oracle: draft-atwood-jmap-vtc-00 (jmap-chat-spec 6bcd9d8) L380-381
        // "`breakoutCallId` (String|null): The id of a child breakout-room
        // VTCCall to which a moderator has assigned this participant ...
        // Default: `null`."; L920 shows `"breakoutCallId": null` in the
        // complete record.
        let mut raw = pstn();
        raw.as_object_mut().unwrap().remove("breakoutCallId");
        let p: VtcParticipant = serde_json::from_value(raw).unwrap();
        assert_eq!(p.breakout_call_id, None);
        assert_eq!(
            serde_json::to_value(&p).unwrap()["breakoutCallId"],
            json!(null)
        );

        let mut raw = pstn();
        raw["breakoutCallId"] = json!("01J4XZBRK0TZXX3VVGJMXY8KM0");
        let p: VtcParticipant = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            p.breakout_call_id,
            Some(Id::from("01J4XZBRK0TZXX3VVGJMXY8KM0"))
        );
        assert!(p.extra.is_empty(), "typed, not an extra: {:?}", p.extra);
        assert_eq!(serde_json::to_value(&p).unwrap(), raw);
    }

    #[test]
    fn required_fields_rejected_when_missing() {
        // Oracle: L281-327 mark these without "optional" and without a
        // default.
        for field in [
            "id",
            "callId",
            "displayName",
            "role",
            "joinMethod",
            "mediaState",
            "speakerTimeMs",
        ] {
            let mut raw = pstn();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<VtcParticipant>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = pstn();
        raw["vendor.example:seat"] = json!("A4");
        let p: VtcParticipant = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(p.extra.get("vendor.example:seat"), Some(&json!("A4")));
        assert_eq!(serde_json::to_value(&p).unwrap(), raw);
    }
}
