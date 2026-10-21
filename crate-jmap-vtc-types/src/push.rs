//! draft-atwood-jmap-vtc-00 §Push: VTCCallPush.

use jmap_types::Id;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{CallType, MediaKind};

/// Wire value of the `@type` of a [`VtcCallPush`].
const VTC_CALL_PUSH_TYPE: &str = "VTCCallPush";

/// The ring push payload (draft-atwood-jmap-vtc-00 §Push: VTCCallPush),
/// in its full or content-free form.
///
/// The content-free form "carries only `@type`, `accountId`, `callId`,
/// and `callType`", so every other property may be absent. The `T|null`
/// properties (`subject`, `chatId`, `spaceId`, `channelId`) are
/// `Option<Option<T>>`: `None` when absent, `Some(None)` for `null`. The
/// others are `None` when absent. A push without
/// [`initiator_id`](Self::initiator_id) is the cue for the client to fetch
/// the call ("A client that receives a `VTCCallPush` without `initiatorId`
/// SHOULD fetch the call details after waking").
///
/// The wire form always carries `"@type": "VTCCallPush"`: serialize
/// emits it, and deserialize requires it and rejects any other value.
/// It is not stored in [`extra`](Self::extra).
///
/// # Example
///
/// ```
/// use jmap_vtc_types::VtcCallPush;
///
/// let push: VtcCallPush = serde_json::from_str(r#"{
///     "@type": "VTCCallPush",
///     "accountId": "alice-account",
///     "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
///     "callType": "ring"
/// }"#)?;
/// assert!(push.initiator_id.is_none());
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", rename_all = "camelCase")]
pub struct VtcCallPush {
    /// "The call's hosting account ... This is usually not the target's
    /// personal account" (draft-atwood-jmap-vtc-00 §VTCCallPush).
    pub account_id: Id,
    /// The ringing call.
    pub call_id: Id,
    /// "Always `\"ring\"` for ring notifications."
    pub call_type: CallType,
    /// The userId of the caller; absent in the content-free form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiator_id: Option<String>,
    /// The caller's display name at push time. "Snapshot; not
    /// authoritative identity."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiator_display_name: Option<String>,
    /// The call subject.
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub subject: Option<Option<String>>,
    /// The media types for this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_types: Option<Vec<MediaKind>>,
    /// The media-layer entry point. Untrusted (§VTCCall `joinUri`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_uri: Option<String>,
    /// The associated Chat.
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub chat_id: Option<Option<Id>>,
    /// The associated Space.
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub space_id: Option<Option<Id>>,
    /// The associated channel within the Space.
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub channel_id: Option<Option<Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Serialize for VtcCallPush {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let fields = VtcCallPush::serialize(self, serde_json::value::Serializer)
            .map_err(serde::ser::Error::custom)?;
        crate::wire::with_tag(fields, VTC_CALL_PUSH_TYPE)
            .map_err(serde::ser::Error::custom)?
            .serialize(s)
    }
}

impl<'de> Deserialize<'de> for VtcCallPush {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut map = serde_json::Map::deserialize(d)?;
        let tag = crate::wire::take_tag::<D::Error>(&mut map)?;
        if tag != VTC_CALL_PUSH_TYPE {
            return Err(D::Error::custom(format!(
                "@type must be \"{VTC_CALL_PUSH_TYPE}\", found \"{tag}\""
            )));
        }
        VtcCallPush::deserialize(serde_json::Value::Object(map)).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn type_tag_required_and_checked() {
        // Oracle: L1240-1241 "`@type` (String): MUST be `\"VTCCallPush\"`."
        let base = json!({
            "accountId": "alice-account",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "callType": "ring"
        });
        assert!(serde_json::from_value::<VtcCallPush>(base.clone()).is_err());
        let mut wrong = base.clone();
        wrong["@type"] = json!("ChatMessagePush");
        assert!(serde_json::from_value::<VtcCallPush>(wrong).is_err());
        let mut right = base;
        right["@type"] = json!("VTCCallPush");
        let push: VtcCallPush = serde_json::from_value(right).unwrap();
        assert!(!push.extra.contains_key("@type"));
    }

    #[test]
    fn content_free_core_fields_required() {
        // Oracle: L1298 "A content-free `VTCCallPush` carries only `@type`,
        // `accountId`, `callId`, and `callType`."
        for field in ["accountId", "callId", "callType"] {
            let mut raw = json!({
                "@type": "VTCCallPush",
                "accountId": "alice-account",
                "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
                "callType": "ring"
            });
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<VtcCallPush>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn vendor_extras_preserved() {
        let raw = json!({
            "@type": "VTCCallPush",
            "accountId": "alice-account",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "callType": "ring",
            "vendor.example:ringtone": "classic"
        });
        let push: VtcCallPush = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            push.extra.get("vendor.example:ringtone"),
            Some(&json!("classic"))
        );
        assert_eq!(serde_json::to_value(&push).unwrap(), raw);
    }
}
