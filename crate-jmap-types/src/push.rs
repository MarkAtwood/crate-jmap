//! RFC 8620 §7 push wire types: [`StateChange`], [`PushSubscription`],
//! [`PushKeys`], and [`PushVerification`].
//!
//! These are shared by servers (which build and send them) and clients
//! (which receive them over SSE, WebSocket, or a push endpoint).
//!
//! # The `@type` discriminant
//!
//! [`StateChange`] and [`PushVerification`] carry an `@type` property that
//! "MUST be the string" naming the object (RFC 8620 §7.1, §7.2.2). Their
//! (De)Serialize impls are hand-written rather than derived: serialize always
//! emits the `@type` string; deserialize consumes it, rejects any other value,
//! and never leaves it in `extra`. An absent `@type` is accepted, because a
//! receiver that dispatched on `@type` (or on an SSE event name) has already
//! established what the object is.

use std::collections::HashMap;
use std::fmt;

use serde::de::DeserializeOwned;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::id::{Id, State, UTCDate};

/// `@type` value of a [`StateChange`] (RFC 8620 §7.1).
const STATE_CHANGE_TYPE: &str = "StateChange";

/// `@type` value of a [`PushVerification`] (RFC 8620 §7.2.2).
const PUSH_VERIFICATION_TYPE: &str = "PushVerification";

/// Placeholder printed by the `Debug` impls in place of secret values.
const REDACTED: &str = "[REDACTED]";

/// A state change push notification (RFC 8620 §7.1).
///
/// Sent by the server over SSE, WebSocket, or to a [`PushSubscription`] URL.
/// The wire form always carries `"@type": "StateChange"`; see the
/// [module docs](self) for how the discriminant is handled.
///
/// # `extra` equality is feature-flag-dependent (bd:JMAP-6r7c.43)
///
/// The derived `PartialEq` / `Eq` impl compares `extra` as a
/// `serde_json::Map`, which is order-insensitive under the workspace default
/// (BTreeMap-backed) and order-sensitive if any crate in the dependency graph
/// enables `serde_json/preserve_order`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChange {
    /// For each account that changed: maps data-type name to the new [`State`] token.
    ///
    /// Outer key: account [`Id`].  Inner key: JMAP data-type name (e.g. `"Email"`).
    /// Inner value: new opaque state string; pass to `Email/changes` etc. as `sinceState`.
    pub changed: HashMap<Id, HashMap<String, State>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md). Never contains `@type`.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Serialize for StateChange {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("@type", STATE_CHANGE_TYPE)?;
        map.serialize_entry("changed", &self.changed)?;
        for (k, v) in &self.extra {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for StateChange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut extra = serde_json::Map::deserialize(d)?;
        take_type(&mut extra, STATE_CHANGE_TYPE)?;
        let changed = take_field(&mut extra, "changed")?;
        Ok(StateChange { changed, extra })
    }
}

/// A push subscription (RFC 8620 §7.2).
///
/// Every property other than `deviceClientId` is optional here because the
/// same type carries a create request (no `id`, `verificationCode` or
/// `expires`), a `PushSubscription/get` result (`url` and `keys` are never
/// returned, RFC 8620 §7.2.1), and the server's stored record. `None` fields
/// are omitted on serialize; a JSON `null` deserializes to `None`.
///
/// Extension properties, such as `chatPush` from
/// draft-atwood-jmap-chat-push-00, are carried in [`extra`](Self::extra).
///
/// The `Debug` impl redacts `url`, `keys.auth` and `verificationCode`.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscription {
    /// The `id` property: server-set and immutable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// The `deviceClientId` property: identifies the client and device.
    pub device_client_id: String,
    /// The `url` property: absolute URL the server POSTs pushes to. The RFC
    /// requires it to begin with `"https://"`; this type does not check that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The `keys` property: RFC 8291 encryption keys, or `None` for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<PushKeys>,
    /// The `verificationCode` property: `None` until the client echoes the
    /// code from the [`PushVerification`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_code: Option<String>,
    /// The `expires` property: the server sends nothing after this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<UTCDate>,
    /// The `types` property: data types to push; `None` means all types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<String>>,
    /// Catch-all for extension properties (e.g. `chatPush`) and vendor /
    /// site / private fields not covered by the typed fields above, per
    /// workspace extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl fmt::Debug for PushSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushSubscription")
            .field("id", &self.id)
            .field("device_client_id", &self.device_client_id)
            .field("url", &self.url.as_ref().map(|_| REDACTED))
            .field("keys", &self.keys)
            .field(
                "verification_code",
                &self.verification_code.as_ref().map(|_| REDACTED),
            )
            .field("expires", &self.expires)
            .field("types", &self.types)
            .field("extra", &self.extra)
            .finish()
    }
}

/// The `keys` object of a [`PushSubscription`] (RFC 8620 §7.2).
///
/// "The object MUST have the following properties: p256dh ... auth", so both
/// are required on deserialize. The `Debug` impl redacts `auth`.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushKeys {
    /// The `p256dh` property: the client's P-256 ECDH public key (RFC 8291),
    /// URL-safe base64.
    pub p256dh: String,
    /// The `auth` property: the RFC 8291 authentication secret, URL-safe
    /// base64.
    pub auth: String,
    /// Catch-all for vendor / site / private extension fields, per
    /// workspace extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl fmt::Debug for PushKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushKeys")
            .field("p256dh", &self.p256dh)
            .field("auth", &REDACTED)
            .field("extra", &self.extra)
            .finish()
    }
}

/// The object pushed to a new subscription's URL (RFC 8620 §7.2.2).
///
/// The wire form always carries `"@type": "PushVerification"`; see the
/// [module docs](self) for how the discriminant is handled. The `Debug` impl
/// redacts `verificationCode`.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub struct PushVerification {
    /// The `pushSubscriptionId` property: the id of the new subscription.
    pub push_subscription_id: Id,
    /// The `verificationCode` property: the code the client must set on the
    /// subscription before the server sends it anything else.
    pub verification_code: String,
    /// Catch-all for vendor / site / private extension fields, per
    /// workspace extras-preservation policy (see workspace AGENTS.md).
    /// Never contains `@type`.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl fmt::Debug for PushVerification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushVerification")
            .field("push_subscription_id", &self.push_subscription_id)
            .field("verification_code", &REDACTED)
            .field("extra", &self.extra)
            .finish()
    }
}

impl Serialize for PushVerification {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("@type", PUSH_VERIFICATION_TYPE)?;
        map.serialize_entry("pushSubscriptionId", &self.push_subscription_id)?;
        map.serialize_entry("verificationCode", &self.verification_code)?;
        for (k, v) in &self.extra {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for PushVerification {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut extra = serde_json::Map::deserialize(d)?;
        take_type(&mut extra, PUSH_VERIFICATION_TYPE)?;
        let push_subscription_id = take_field(&mut extra, "pushSubscriptionId")?;
        let verification_code = take_field(&mut extra, "verificationCode")?;
        Ok(PushVerification {
            push_subscription_id,
            verification_code,
            extra,
        })
    }
}

/// Removes `@type` from `map`, failing if it is present and not `expected`.
fn take_type<E: serde::de::Error>(
    map: &mut serde_json::Map<String, serde_json::Value>,
    expected: &str,
) -> Result<(), E> {
    match map.remove("@type") {
        None => Ok(()),
        Some(serde_json::Value::String(s)) if s == expected => Ok(()),
        Some(other) => Err(E::custom(format!(
            "@type must be \"{expected}\", found {other}"
        ))),
    }
}

/// Removes the required property `key` from `map` and deserializes it.
fn take_field<T: DeserializeOwned, E: serde::de::Error>(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &'static str,
) -> Result<T, E> {
    let value = map.remove(key).ok_or_else(|| E::missing_field(key))?;
    serde_json::from_value(value).map_err(E::custom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `StateChange.extra` captures unknown fields on deserialize and
    /// flattens them on serialize (round-trip).
    #[test]
    fn state_change_preserves_vendor_extras() {
        let raw = json!({
            "changed": {
                "acc1": { "Email": "s42" }
            },
            "acmeCorpSequence": 17
        });
        let obj: StateChange = serde_json::from_value(raw).expect("StateChange must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpSequence").and_then(|v| v.as_u64()),
            Some(17)
        );

        // Round-trip: serializing back must reproduce the vendor field.
        let v = serde_json::to_value(&obj).expect("StateChange must serialize");
        assert_eq!(v["acmeCorpSequence"], json!(17));
    }

    /// bd:JMAP-6r7c.43 — regression-guard for the workspace's
    /// serde_json/preserve_order posture. Two StateChange values with the
    /// same `extra` keys inserted in different orders MUST compare equal
    /// under the workspace's default configuration (BTreeMap-backed
    /// serde_json::Map). If a future Cargo.lock or workspace-level feature
    /// change accidentally enables `preserve_order`, this assertion fails
    /// loudly and surfaces the SemVer-policy break before downstream
    /// consumers hit silently-different equality semantics.
    ///
    /// Both values are deserialized from JSON (the wire path) so the test
    /// exercises the same construction code path consumers use.
    #[test]
    fn extra_equality_is_order_insensitive_under_workspace_flags() {
        // Two equivalent JSON payloads with different key-insertion orders
        // for the vendor extras. Under BTreeMap-backed Map the keys are
        // re-sorted lexicographically on deserialize, so the resulting
        // structures compare equal.
        let raw_a = json!({
            "changed": {"acc1": {"Email": "s1"}},
            "vendorA": 1,
            "vendorB": 2
        });
        let raw_b = json!({
            "changed": {"acc1": {"Email": "s1"}},
            "vendorB": 2,
            "vendorA": 1
        });

        let a: StateChange = serde_json::from_value(raw_a).expect("a must deserialize");
        let b: StateChange = serde_json::from_value(raw_b).expect("b must deserialize");

        assert_eq!(
            a, b,
            "extra-map equality is order-insensitive under the workspace's \
             default serde_json::Map (BTreeMap-backed); if this fails, \
             check whether preserve_order has been enabled in the dep graph"
        );
    }
}
