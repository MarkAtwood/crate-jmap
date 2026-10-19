//! ChatContact and Endpoint objects for remote users.

use crate::presence::Presence;
use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// An out-of-band capability endpoint advertised on a contact or
/// session object (draft-atwood-jmap-chat-00 §4.4).
///
/// Examples per the draft: `urn:jmap:chat:cap:vtc` (video/voice
/// teleconference), `urn:jmap:chat:cap:payment`,
/// `urn:jmap:chat:cap:blob`, etc. The
/// [`endpoint_type`](Self::endpoint_type) field is the discriminant
/// for the [`metadata`](Self::metadata) value shape.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    /// The `type` property (draft-atwood-jmap-chat-00 §4.4).
    ///
    /// URI namespace per the draft. Well-known values include
    /// `urn:jmap:chat:cap:vtc`, `urn:jmap:chat:cap:payment`,
    /// `urn:jmap:chat:cap:blob`, `urn:jmap:chat:cap:calendar-event`,
    /// `urn:jmap:chat:cap:availability`, `urn:jmap:chat:cap:task`,
    /// `urn:jmap:chat:cap:filenode`. Deployments and future drafts
    /// MAY define additional values. Clients MUST silently ignore
    /// Endpoint records whose `type` they do not recognize.
    ///
    /// Wire name is "type" — camelCase expansion would give "endpointType".
    #[serde(rename = "type")]
    pub endpoint_type: String,
    /// The `uri` property (draft-atwood-jmap-chat-00 §4.4).
    ///
    /// Format is type-specific (per `endpoint_type`). Peer-supplied;
    /// MUST be treated as untrusted by consumers.
    pub uri: String,
    /// The `label` property (draft-atwood-jmap-chat-00 §4.4).
    ///
    /// Human-readable label for this endpoint (e.g.
    /// `"Personal Jitsi"`, `"Zcash address"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The `metadata` property (draft-atwood-jmap-chat-00 §4.4).
    ///
    /// Type-specific key-value pairs whose shape is keyed by the
    /// [`endpoint_type`](Self::endpoint_type) discriminant. The
    /// draft (§4.4) gives illustrative per-type examples (`vtc`:
    /// `{"protocol": "webrtc", "roomName": "...", "password": "..."}`;
    /// `payment`: `{"network": "lightning", "currency": "BTC"}`;
    /// `blob`: `{"maxBytes": 10485760}`; etc.) but deliberately
    /// leaves the per-type value shape open and requires that
    /// "Clients MUST ignore unknown keys".
    ///
    /// Consumers that need typed access for a known
    /// `endpoint_type` value MUST cast via
    /// `serde_json::from_value::<MyTypedShape>(metadata.clone())`
    /// at their boundary; this crate does not enforce a schema and
    /// will not in future revisions, because the draft is the
    /// schema authority and explicitly keeps the value shape
    /// extensible per type.
    ///
    /// The value is a JSON Object on the wire per the draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A remote user known to this server (spec: ChatContact object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContact {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.8).
    ///
    /// The server-assigned RFC 8620 §1.2 Id of this ChatContact. It is
    /// local to this server: another server holds a different `id` for
    /// the same user. Every client-facing reference to a user (a
    /// Message `senderId`, a Mention `id`, a ChatMember `id`) carries
    /// this value. The user's authentication-layer identity is in
    /// [`identity`](Self::identity), not here.
    pub id: Id,
    /// The `identity` property (draft-atwood-jmap-chat-00 §4.8).
    ///
    /// The identity string the authentication layer provides for this
    /// user, for example a `user@host`-style string, a W3C DID-Core URI
    /// (`did:web:alice.example`), or another URI. Immutable. Set by the
    /// server when it records the contact automatically, or by the
    /// owner in `ChatContact/set` create.
    ///
    /// Servers MUST treat the value as opaque and compare it octet for
    /// octet; within a server, at most one ChatContact has a given
    /// `identity`.
    pub identity: String,
    /// The `login` property (draft-atwood-jmap-chat-00 §4.8).
    ///
    /// Clients fall back to `login` when `display_name` is absent or
    /// empty, and to [`identity`](Self::identity) when `login` is
    /// unavailable; never to `id`, which is a server-local handle.
    pub login: String,
    /// The `firstSeenAt` property (draft-atwood-jmap-chat-00 §4.8).
    pub first_seen_at: UTCDate,
    /// The `lastSeenAt` property (draft-atwood-jmap-chat-00 §4.8).
    pub last_seen_at: UTCDate,
    /// The `blocked` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(default)]
    pub blocked: bool,
    /// The `displayName` property (draft-atwood-jmap-chat-00 §4.8).
    ///
    /// When absent or empty, clients fall back to `login`, then
    /// [`identity`](Self::identity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The `presence` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
    /// The `lastActiveAt` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<UTCDate>,
    /// The `statusText` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
    /// The `statusEmoji` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_emoji: Option<String>,
    /// The `endpoints` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<Vec<Endpoint>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ChatContact {
    /// Construct a [`ChatContact`] from its required fields.
    ///
    /// All optional fields default to `None`.
    pub fn new(
        id: Id,
        identity: impl Into<String>,
        login: impl Into<String>,
        first_seen_at: UTCDate,
        last_seen_at: UTCDate,
        blocked: bool,
    ) -> Self {
        Self {
            id,
            identity: identity.into(),
            login: login.into(),
            first_seen_at,
            last_seen_at,
            blocked,
            display_name: None,
            presence: None,
            last_active_at: None,
            status_text: None,
            status_emoji: None,
            endpoints: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: minimal JSON — only required fields present; all optional fields absent.
    #[test]
    fn contact_deser_minimal() {
        let json = r#"{
            "id": "u1",
            "identity": "alice@example.com",
            "login": "alice@example.com",
            "firstSeenAt": "2026-01-01T00:00:00Z",
            "lastSeenAt": "2026-04-01T00:00:00Z",
            "blocked": false
        }"#;
        let c: ChatContact = serde_json::from_str(json).expect("deserialize ChatContact");
        assert_eq!(c.id.as_ref(), "u1");
        assert_eq!(c.login, "alice@example.com");
        assert!(!c.blocked);
        assert!(c.display_name.is_none());
        assert!(c.endpoints.is_none());
    }

    // Oracle: the wire field name for endpoint_type must be "type", not "endpointType".
    #[test]
    fn endpoint_type_wire_name() {
        let ep = Endpoint {
            endpoint_type: "xmpp".to_owned(),
            uri: "xmpp:alice@example.com".to_owned(),
            label: None,
            metadata: None,
            extra: serde_json::Map::new(),
        };
        let json = serde_json::to_string(&ep).expect("serialize Endpoint");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse JSON");
        assert!(v.get("type").is_some(), "expected key \"type\" in JSON");
        assert!(
            v.get("endpointType").is_none(),
            "unexpected key \"endpointType\" in JSON"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `Endpoint.extra` captures vendor fields and preserves them.
    #[test]
    fn endpoint_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "type": "xmpp",
            "uri": "xmpp:alice@example.com",
            "acmeCorpReachability": "direct"
        });
        let ep: Endpoint = serde_json::from_value(raw).unwrap();
        assert_eq!(
            ep.extra
                .get("acmeCorpReachability")
                .and_then(|v| v.as_str()),
            Some("direct")
        );
        let back = serde_json::to_value(&ep).unwrap();
        assert_eq!(back["acmeCorpReachability"], "direct");
    }

    /// `ChatContact.extra` captures vendor fields and preserves them.
    #[test]
    fn chat_contact_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "u1",
            "identity": "alice@example.com",
            "login": "alice@example.com",
            "firstSeenAt": "2026-01-01T00:00:00Z",
            "lastSeenAt": "2026-04-01T00:00:00Z",
            "blocked": false,
            "acmeCorpFederationPeer": "node-2"
        });
        let c: ChatContact = serde_json::from_value(raw).unwrap();
        assert_eq!(
            c.extra
                .get("acmeCorpFederationPeer")
                .and_then(|v| v.as_str()),
            Some("node-2")
        );
        let back = serde_json::to_value(&c).unwrap();
        assert_eq!(back["acmeCorpFederationPeer"], "node-2");
    }

    // Oracle: per draft-atwood-jmap-chat-00 at jmap-chat-spec cefc217,
    // ChatContact.identity holds the authentication-layer identity
    // ("a `user@host`-style string, a Decentralized Identifier URI, or
    // another URI") and "Servers MUST treat it as opaque". A `did:web:...`
    // value MUST round-trip the wire string verbatim beside a local Id —
    // the serializer does not URL-encode, parse, or normalize.
    #[test]
    fn contact_did_uri_identity_round_trips_verbatim() {
        let original = ChatContact {
            id: Id::from("c1"),
            identity: "did:web:alice.example".to_owned(),
            login: "alice@alice.example".to_owned(),
            first_seen_at: UTCDate::from("2026-01-01T00:00:00Z"),
            last_seen_at: UTCDate::from("2026-04-01T00:00:00Z"),
            blocked: false,
            display_name: None,
            presence: None,
            last_active_at: None,
            status_text: None,
            status_emoji: None,
            endpoints: None,
            extra: serde_json::Map::new(),
        };

        let serialized = serde_json::to_value(&original).expect("serialize");
        assert_eq!(
            serialized["identity"], "did:web:alice.example",
            "DID URI must serialize as the verbatim wire string"
        );

        let round: ChatContact = serde_json::from_value(serialized).expect("deserialize");
        assert_eq!(
            round.identity, "did:web:alice.example",
            "DID URI must deserialize back to the same opaque string"
        );

        // A second non-DID form is also opaque — round-trip a federated
        // user@host form for parity.
        let federated = ChatContact {
            identity: "alice@matrix.example".to_owned(),
            login: "alice@matrix.example".to_owned(),
            ..original
        };
        let serialized = serde_json::to_value(&federated).expect("serialize");
        assert_eq!(serialized["identity"], "alice@matrix.example");
        let round: ChatContact = serde_json::from_value(serialized).expect("deserialize");
        assert_eq!(round.identity, "alice@matrix.example");
    }

    // Oracle: when blocked=true, the "blocked" key must appear in serialized output.
    #[test]
    fn contact_blocked_present() {
        let c = ChatContact {
            id: Id::from("u2"),
            identity: "bob@example.com".to_owned(),
            login: "bob@example.com".to_owned(),
            first_seen_at: UTCDate::from("2026-01-01T00:00:00Z"),
            last_seen_at: UTCDate::from("2026-04-01T00:00:00Z"),
            blocked: true,
            display_name: None,
            presence: None,
            last_active_at: None,
            status_text: None,
            status_emoji: None,
            endpoints: None,
            extra: serde_json::Map::new(),
        };
        let json = serde_json::to_string(&c).expect("serialize ChatContact");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse JSON");
        assert_eq!(v["blocked"], serde_json::json!(true));
    }
}
