//! draft-atwood-jmap-vtc-00 §Capability and §VTCGateway — capability URI,
//! account-level capability object, and the gateway types it embeds.

use jmap_types::impl_string_enum;
use serde::{Deserialize, Serialize};

/// Capability URI for JMAP VTC (draft-atwood-jmap-vtc-00 §Capability).
///
/// "Servers supporting this specification MUST advertise the
/// `urn:ietf:params:jmap:vtc` capability in the JMAP Session object." The
/// session-level value is an empty object `{}` and is not modelled; the
/// account-level value is [`VtcCapability`].
pub const JMAP_VTC_URI: &str = "urn:ietf:params:jmap:vtc";

/// Capability URI for ephemeral VTC event push over WebSocket
/// (draft-atwood-jmap-vtc-wss-00 §Capability).
///
/// The session-level value is an empty object `{}` and is not modelled.
/// Advertising it means the server accepts
/// [`VtcStreamEnable`](crate::VtcStreamEnable) and
/// [`VtcStreamDisable`](crate::VtcStreamDisable).
pub const JMAP_VTC_WEBSOCKET_URI: &str = "urn:ietf:params:jmap:vtc:websocket";

/// A media type of a call or gateway: `"audio"`, `"video"` or `"screen"`
/// (draft-atwood-jmap-vtc-00 §Account-Level Capability Object,
/// `supportedMediaTypes`).
///
/// `Other` preserves any unregistered value for round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// `"audio"`.
    Audio,
    /// `"video"`.
    Video,
    /// `"screen"`: screen sharing.
    Screen,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(MediaKind, "a VTC media type string",
    "audio" => Audio,
    "video" => Video,
    "screen" => Screen,
);

/// The `protocol` of a [`VtcGateway`] and of a
/// [`VtcGatewaySignal`](crate::VtcGatewaySignal)
/// (draft-atwood-jmap-vtc-00 §VTCGateway).
///
/// The draft registers `"pstn"`, `"sip"` and `"h323"`; other protocols use
/// reverse-domain notation and land in `Other`, which "Clients MUST
/// ignore" for dispatch but which round-trips verbatim.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GatewayProtocol {
    /// `"pstn"`: the public switched telephone network.
    Pstn,
    /// `"sip"`: SIP.
    Sip,
    /// `"h323"`: H.323.
    H323,
    /// A reverse-domain or other unregistered protocol identifier.
    Other(String),
}

impl_string_enum!(GatewayProtocol, "a VTC gateway protocol string",
    "pstn" => Pstn,
    "sip" => Sip,
    "h323" => H323,
);

/// Account-level capability object for `"urn:ietf:params:jmap:vtc"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:vtc"]`.
///
/// Spec: draft-atwood-jmap-vtc-00 §Account-Level Capability Object.
///
/// `supportedMediaTypes` "MUST include at least `\"audio\"`"; this is
/// documented, not enforced on deserialize.
///
/// # Example
///
/// ```
/// use jmap_vtc_types::VtcCapability;
///
/// let cap: VtcCapability = serde_json::from_str(r#"{
///     "mayCreateCall": true,
///     "supportsRingCalls": true,
///     "supportsRoomCalls": true,
///     "supportsScheduledCalls": false,
///     "supportsRecording": false,
///     "supportsLivestream": false,
///     "supportsLobby": false,
///     "supportsBreakoutRooms": false,
///     "supportedMediaTypes": ["audio", "video"],
///     "maxParticipantsPerCall": 50,
///     "maxConcurrentCalls": null
/// }"#)?;
/// assert_eq!(cap.max_participants_per_call, Some(50));
/// assert_eq!(cap.max_breakout_rooms, None);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcCapability {
    /// `true` if the authenticated user may create new VTCCall objects.
    pub may_create_call: bool,
    /// `true` if the server supports ring calls (`callType: "ring"`).
    pub supports_ring_calls: bool,
    /// `true` if the server supports room calls (`callType: "room"`).
    pub supports_room_calls: bool,
    /// `true` if the server supports scheduled calls
    /// (`callType: "scheduled"`).
    pub supports_scheduled_calls: bool,
    /// `true` if the server supports VTCRecording.
    pub supports_recording: bool,
    /// `true` if the server supports VTCLivestream.
    pub supports_livestream: bool,
    /// `true` if the server supports lobby (waiting-room) mode.
    pub supports_lobby: bool,
    /// `true` if the server supports breakout rooms.
    pub supports_breakout_rooms: bool,
    /// Media types the deployment supports.
    pub supported_media_types: Vec<MediaKind>,
    /// Maximum participants per call; `None` (`null`) means no
    /// server-imposed limit.
    pub max_participants_per_call: Option<u64>,
    /// Maximum concurrent active calls per account; `None` (`null`) means
    /// no limit.
    pub max_concurrent_calls: Option<u64>,
    /// Maximum breakout rooms per call. `None` when absent, which the
    /// draft requires "when `supportsBreakoutRooms` is `false`";
    /// `Some(None)` for `null`, meaning no limit; `Some(Some(n))` for a
    /// limit.
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_breakout_rooms: Option<Option<u64>>,
    /// Protocol gateways available for this deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateways: Option<Vec<VtcGateway>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A protocol gateway through which external telephony or
/// video-conferencing systems interoperate with calls
/// (draft-atwood-jmap-vtc-00 §VTCGateway).
///
/// Advertised in [`VtcCapability::gateways`]; not a JMAP object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcGateway {
    /// The gateway protocol.
    pub protocol: GatewayProtocol,
    /// Human-readable label, e.g. `"US PSTN"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// `true` if a moderator may dial out through this gateway.
    pub supports_dial_out: bool,
    /// Media types this gateway supports; a subset of
    /// [`VtcCapability::supported_media_types`].
    pub supported_media_types: Vec<MediaKind>,
    /// Addresses external callers can use to reach a call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dial_in_numbers: Option<Vec<VtcDialInEntry>>,
    /// Protocol-specific gateway configuration. "Clients MUST ignore
    /// unknown keys."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An address through which an external caller reaches a call via a
/// gateway (draft-atwood-jmap-vtc-00 §VTCDialInEntry).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcDialInEntry {
    /// The dial-in address: an E.164 number for PSTN, a SIP URI for SIP,
    /// an H.323 alias or E.164 number for H.323.
    pub address: String,
    /// Human-readable region label, e.g. `"US West"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// `true` for a toll-free number; `false` for non-PSTN protocols.
    pub toll_free: bool,
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

    /// Oracle: draft-atwood-jmap-vtc-wss-00 L97-99 "Servers supporting
    /// this specification MUST add a property named
    /// `urn:ietf:params:jmap:vtc:websocket` to the JMAP Session object
    /// capabilities."
    #[test]
    fn websocket_uri_constant_matches_draft() {
        assert_eq!(JMAP_VTC_WEBSOCKET_URI, "urn:ietf:params:jmap:vtc:websocket");
    }

    /// Hand-written from the field list of draft-atwood-jmap-vtc-00
    /// L148-187. The gateway uses the SIP `metadata` example of L217 and
    /// the SIP dial-in address example of L227.
    fn sample() -> serde_json::Value {
        json!({
            "mayCreateCall": true,
            "supportsRingCalls": true,
            "supportsRoomCalls": true,
            "supportsScheduledCalls": true,
            "supportsRecording": false,
            "supportsLivestream": false,
            "supportsLobby": true,
            "supportsBreakoutRooms": true,
            "supportedMediaTypes": ["audio", "video", "screen"],
            "maxParticipantsPerCall": 100,
            "maxConcurrentCalls": null,
            "maxBreakoutRooms": 10,
            "gateways": [{
                "protocol": "sip",
                "displayName": "Corporate SIP Trunk",
                "supportsDialOut": true,
                "supportedMediaTypes": ["audio", "video"],
                "dialInNumbers": [{"address": "sip:meet@example.com", "tollFree": false}],
                "metadata": {"registrar": "sip.example.com", "transport": "tls"}
            }]
        })
    }

    #[test]
    fn capability_uri_matches_draft() {
        // Oracle: L140 "MUST advertise the `urn:ietf:params:jmap:vtc`
        // capability".
        assert_eq!(JMAP_VTC_URI, "urn:ietf:params:jmap:vtc");
    }

    #[test]
    fn account_capability_round_trips() {
        let raw = sample();
        let cap: VtcCapability = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            cap.supported_media_types,
            [MediaKind::Audio, MediaKind::Video, MediaKind::Screen]
        );
        assert_eq!(cap.max_participants_per_call, Some(100));
        assert_eq!(cap.max_concurrent_calls, None);
        assert_eq!(cap.max_breakout_rooms, Some(Some(10)));
        let gw = &cap.gateways.as_ref().unwrap()[0];
        assert_eq!(gw.protocol, GatewayProtocol::Sip);
        assert_eq!(
            gw.dial_in_numbers.as_ref().unwrap()[0].address,
            "sip:meet@example.com"
        );
        assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
    }

    #[test]
    fn max_breakout_rooms_absent_null_and_value_are_distinct() {
        // Oracle: L183-184 "`null` means no limit. Absent when
        // `supportsBreakoutRooms` is `false`."
        let mut raw = sample();
        raw.as_object_mut().unwrap().remove("maxBreakoutRooms");
        let cap: VtcCapability = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(cap.max_breakout_rooms, None);
        assert_eq!(serde_json::to_value(&cap).unwrap(), raw);

        raw["maxBreakoutRooms"] = json!(null);
        let cap: VtcCapability = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(cap.max_breakout_rooms, Some(None));
        assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
    }

    #[test]
    fn null_limits_serialize_as_null() {
        // Oracle: L177-181 types the limits "UnsignedInt|null": the
        // property is present with a `null` value.
        let cap: VtcCapability = serde_json::from_value(sample()).unwrap();
        let out = serde_json::to_value(&cap).unwrap();
        assert_eq!(out["maxConcurrentCalls"], json!(null));
    }

    #[test]
    fn media_and_protocol_wire_values() {
        // Oracle: L175 "Values: `\"audio\"`, `\"video\"`, `\"screen\"`";
        // L200 "registers `\"pstn\"`, `\"sip\"`, and `\"h323\"`".
        let media: Vec<MediaKind> =
            serde_json::from_value(json!(["audio", "video", "screen"])).unwrap();
        assert_eq!(
            media,
            [MediaKind::Audio, MediaKind::Video, MediaKind::Screen]
        );
        let protos: Vec<GatewayProtocol> =
            serde_json::from_value(json!(["pstn", "sip", "h323"])).unwrap();
        assert_eq!(
            protos,
            [
                GatewayProtocol::Pstn,
                GatewayProtocol::Sip,
                GatewayProtocol::H323
            ]
        );
    }

    #[test]
    fn reverse_domain_protocol_round_trips() {
        // Oracle: L200 "Vendor-specific or proprietary protocols SHOULD use
        // reverse-domain notation (e.g., `\"com.apple.facetime\"` ...).
        // Clients MUST ignore unrecognized values."
        let p: GatewayProtocol = serde_json::from_value(json!("com.apple.facetime")).unwrap();
        assert_eq!(p, GatewayProtocol::Other("com.apple.facetime".to_owned()));
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            json!("com.apple.facetime")
        );
        let m: MediaKind = serde_json::from_value(json!("x-hologram")).unwrap();
        assert_eq!(serde_json::to_value(&m).unwrap(), json!("x-hologram"));
    }

    #[test]
    fn required_fields_rejected_when_missing() {
        // Oracle: L150-175 list these fields without an "optional" marker.
        for field in [
            "mayCreateCall",
            "supportsRingCalls",
            "supportsRoomCalls",
            "supportsScheduledCalls",
            "supportsRecording",
            "supportsLivestream",
            "supportsLobby",
            "supportsBreakoutRooms",
            "supportedMediaTypes",
        ] {
            let mut raw = sample();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<VtcCapability>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = sample();
        raw["vendor.example:region"] = json!("eu");
        raw["gateways"][0]["vendor.example:trunk"] = json!(7);
        raw["gateways"][0]["dialInNumbers"][0]["vendor.example:note"] = json!("x");
        let cap: VtcCapability = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(cap.extra.get("vendor.example:region"), Some(&json!("eu")));
        assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
    }
}
