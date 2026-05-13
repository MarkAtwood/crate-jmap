//! ChatSessionExt trait for [`jmap_base_client::Session`].
//!
//! Adds JMAP Chat extension methods to the base `Session` type.
//!
//! Specs:
//!   - draft-atwood-jmap-chat-00 §3      (ChatCapability fields)
//!   - draft-atwood-jmap-chat-push-00    (ChatPushCapability fields)
//!   - draft-atwood-jmap-chat-wss-00     (supports_chat_websocket)

use serde::Deserialize;

// ---------------------------------------------------------------------------
// ChatCapability
// ---------------------------------------------------------------------------

/// Account-level capability object for `"urn:ietf:params:jmap:chat"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:chat"]`.
///
/// Spec: draft-atwood-jmap-chat-00 §3
#[non_exhaustive]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ChatCapability {
    /// Maximum UTF-8 byte length of a Message body.
    pub max_body_bytes: u64,
    /// Maximum single attachment blob size in bytes.
    pub max_attachment_bytes: u64,
    /// Maximum number of attachments per message.
    pub max_attachments_per_message: u64,
    /// Whether the server supports the optional thread model.
    pub supports_threads: bool,
    /// The set of Message `bodyType` values this server understands
    /// (draft-atwood-jmap-chat-00 §3).
    ///
    /// Spec requirements for compliant servers:
    ///
    /// - MUST include `"text/plain"`.
    /// - SHOULD include `"text/markdown"` (RFC 7763 CommonMark).
    /// - SHOULD include `"application/jmap-chat-rich"`.
    /// - SHOULD include `"application/mls-ciphertext"` for E2EE
    ///   deployments.
    /// - MAY include `"application/mimi-content"`.
    ///
    /// An empty `Vec` is non-compliant per spec (`"text/plain"` is
    /// mandatory) but the client tolerates it via `Default` — the
    /// consumer is responsible for enforcing the MUST and acting
    /// accordingly (e.g. refusing to send rich messages to a server
    /// that does not advertise the matching `bodyType`).
    #[serde(default)]
    pub supported_body_types: Vec<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// Per draft-atwood-jmap-chat-00 §3 (revised 2026-05-11, spec commit
    /// `80d5e11`), the five aggregate-count caps `maxGroupMembers`,
    /// `maxSpaceMembers`, `maxRolesPerSpace`, `maxChannelsPerSpace`, and
    /// `maxCategoriesPerSpace` are no longer advertised on this
    /// capability — they are implementation-defined and enforced via
    /// standard `overQuota` SetError (RFC 8620 §5.3) at `Chat/set` and
    /// `Space/set` time. Servers that still emit them will round-trip
    /// the values harmlessly through `extra`.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// ChatPushCapability
// ---------------------------------------------------------------------------

/// Account-level capability object for `"urn:ietf:params:jmap:chat:push"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:chat:push"]`.
///
/// Spec: draft-atwood-jmap-chat-push-00
#[non_exhaustive]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatPushCapability {
    /// Maximum byte length of a `bodySnippet` in `ChatMessagePush`.
    /// Truncation occurs on a UTF-8 boundary.
    pub max_snippet_bytes: u64,
    /// Supported Web Push urgency values.
    /// MUST include at least `"normal"` and `"high"`.
    pub supported_urgency_values: Vec<String>,
    /// Maximum number of `ChatMessageEntry` objects per push payload.
    /// `None` means the server does not impose a bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_messages_per_push: Option<u64>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// ChatSessionExt
// ---------------------------------------------------------------------------

/// Extension methods for [`jmap_base_client::Session`] that surface
/// JMAP Chat capability information.
///
/// Import this trait to use Chat-specific session helpers:
/// ```ignore
/// use jmap_chat_client::ChatSessionExt;
/// ```
pub trait ChatSessionExt {
    /// Returns the primary account ID for the JMAP Chat capability, if present.
    ///
    /// Reads `primaryAccounts["urn:ietf:params:jmap:chat"]`.
    ///
    /// Returns `None` when the server does not declare a primary chat account.
    fn chat_account_id(&self) -> Option<&str>;

    /// Returns the parsed [`ChatCapability`] for the given account, if present.
    ///
    /// Reads `accounts[account_id].accountCapabilities["urn:ietf:params:jmap:chat"]`.
    ///
    /// - `Ok(None)` — the account is absent or has no chat capability key.
    /// - `Ok(Some(...))` — the capability is present and parsed successfully.
    /// - `Err(ClientError::Parse(...))` — the key is present but malformed JSON.
    fn chat_capability(
        &self,
        account_id: &str,
    ) -> Result<Option<ChatCapability>, jmap_base_client::ClientError>;

    /// Returns the parsed [`ChatPushCapability`] for the given account, if present.
    ///
    /// Reads `accounts[account_id].accountCapabilities["urn:ietf:params:jmap:chat:push"]`.
    ///
    /// - `Ok(None)` — the account is absent or has no chat push capability key.
    /// - `Ok(Some(...))` — the capability is present and parsed successfully.
    /// - `Err(ClientError::Parse(...))` — the key is present but malformed JSON.
    fn chat_push_capability(
        &self,
        account_id: &str,
    ) -> Result<Option<ChatPushCapability>, jmap_base_client::ClientError>;

    /// Returns `true` if the server advertises JMAP Chat WebSocket ephemeral events.
    ///
    /// Checks for presence of `capabilities["urn:ietf:params:jmap:chat:websocket"]`.
    /// Use [`jmap_base_client::Session::websocket_capability`] to obtain the actual
    /// WebSocket URL for connecting.
    fn supports_chat_websocket(&self) -> bool;

    /// Returns the VAPID public key advertised by the server, if present.
    ///
    /// Reads `capabilities["urn:ietf:params:jmap:webpush-vapid"]["vapidPublicKey"]`.
    ///
    /// Returns `None` when the capability is absent or when `vapidPublicKey` is missing
    /// or not a string value.
    fn vapid_public_key(&self) -> Option<&str>;

    /// Returns `true` if the server supports JMAP RefPlus result references.
    ///
    /// Checks for `capabilities["urn:ietf:params:jmap:refplus"]`.
    fn supports_refplus(&self) -> bool;

    /// Returns `true` if the server supports JMAP Quotas.
    ///
    /// Checks for `capabilities["urn:ietf:params:jmap:quota"]`.
    fn supports_quotas(&self) -> bool;
}

// ---------------------------------------------------------------------------
// impl ChatSessionExt for jmap_base_client::Session
// ---------------------------------------------------------------------------

impl ChatSessionExt for jmap_base_client::Session {
    fn chat_account_id(&self) -> Option<&str> {
        self.primary_account_id("urn:ietf:params:jmap:chat")
    }

    fn chat_capability(
        &self,
        account_id: &str,
    ) -> Result<Option<ChatCapability>, jmap_base_client::ClientError> {
        let Some(account) = self.accounts.get(account_id) else {
            return Ok(None);
        };
        let Some(raw) = account
            .account_capabilities
            .get("urn:ietf:params:jmap:chat")
        else {
            return Ok(None);
        };
        ChatCapability::deserialize(raw)
            .map(Some)
            .map_err(jmap_base_client::ClientError::Parse)
    }

    fn chat_push_capability(
        &self,
        account_id: &str,
    ) -> Result<Option<ChatPushCapability>, jmap_base_client::ClientError> {
        let Some(account) = self.accounts.get(account_id) else {
            return Ok(None);
        };
        let Some(raw) = account
            .account_capabilities
            .get("urn:ietf:params:jmap:chat:push")
        else {
            return Ok(None);
        };
        ChatPushCapability::deserialize(raw)
            .map(Some)
            .map_err(jmap_base_client::ClientError::Parse)
    }

    fn supports_chat_websocket(&self) -> bool {
        self.capabilities
            .contains_key("urn:ietf:params:jmap:chat:websocket")
    }

    fn vapid_public_key(&self) -> Option<&str> {
        self.capabilities
            .get("urn:ietf:params:jmap:webpush-vapid")?
            .get("vapidPublicKey")?
            .as_str()
    }

    fn supports_refplus(&self) -> bool {
        self.capabilities
            .contains_key("urn:ietf:params:jmap:refplus")
    }

    fn supports_quotas(&self) -> bool {
        self.capabilities.contains_key("urn:ietf:params:jmap:quota")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_base_client::Session;
    use serde_json::json;

    /// Build a minimal Session value from JSON without hitting the network.
    /// Caller can inject arbitrary capabilities / accounts.
    fn make_session(
        capabilities: serde_json::Value,
        accounts: serde_json::Value,
        primary_accounts: serde_json::Value,
    ) -> Session {
        let raw = json!({
            "capabilities": capabilities,
            "accounts": accounts,
            "primaryAccounts": primary_accounts,
            "username": "test@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1"
        });
        serde_json::from_value(raw).expect("make_session: malformed test JSON")
    }

    // -----------------------------------------------------------------------
    // chat_account_id_present
    // -----------------------------------------------------------------------

    /// Oracle: primaryAccounts["urn:ietf:params:jmap:chat"] = "acct1" →
    /// chat_account_id() returns Some("acct1").
    /// Value derived from the JMAP Chat draft §3 (not from code under test).
    #[test]
    fn chat_account_id_present() {
        let session = make_session(
            json!({}),
            json!({}),
            json!({"urn:ietf:params:jmap:chat": "acct1"}),
        );
        assert_eq!(session.chat_account_id(), Some("acct1"));
    }

    // -----------------------------------------------------------------------
    // chat_account_id_absent
    // -----------------------------------------------------------------------

    /// Oracle: empty primaryAccounts → chat_account_id() returns None.
    /// Per RFC 8620 §2, primaryAccounts is a map; an absent key means no
    /// primary account for that capability.
    #[test]
    fn chat_account_id_absent() {
        let session = make_session(json!({}), json!({}), json!({}));
        assert!(
            session.chat_account_id().is_none(),
            "expected None for missing primaryAccounts entry"
        );
    }

    // -----------------------------------------------------------------------
    // chat_capability_parses
    // -----------------------------------------------------------------------

    /// Oracle: valid ChatCapability JSON at accounts[id].accountCapabilities
    /// → Ok(Some(cap)) with correct field values.
    /// Field names and types from draft-atwood-jmap-chat-00 §3.
    #[test]
    fn chat_capability_parses() {
        let session = make_session(
            json!({}),
            json!({
                "acct1": {
                    "name": "test@example.com",
                    "isPersonal": true,
                    "isReadOnly": false,
                    "accountCapabilities": {
                        "urn:ietf:params:jmap:chat": {
                            "maxBodyBytes": 65536,
                            "maxAttachmentBytes": 10485760,
                            "maxAttachmentsPerMessage": 10,
                            "supportsThreads": true
                        }
                    }
                }
            }),
            json!({"urn:ietf:params:jmap:chat": "acct1"}),
        );

        let cap = session
            .chat_capability("acct1")
            .expect("chat_capability must not return Err")
            .expect("acct1 must have chat capability");

        // Oracle: field values match what was put in the JSON above
        assert_eq!(cap.max_body_bytes, 65536);
        assert_eq!(cap.max_attachment_bytes, 10485760);
        assert_eq!(cap.max_attachments_per_message, 10);
        assert!(cap.supports_threads);
    }

    // -----------------------------------------------------------------------
    // supports_chat_websocket_true
    // -----------------------------------------------------------------------

    /// Oracle: capabilities contains "urn:ietf:params:jmap:chat:websocket" →
    /// supports_chat_websocket() returns true.
    /// Per draft-atwood-jmap-chat-wss-00, presence of this key signals support.
    #[test]
    fn supports_chat_websocket_true() {
        let session = make_session(
            json!({"urn:ietf:params:jmap:chat:websocket": {}}),
            json!({}),
            json!({}),
        );
        assert!(
            session.supports_chat_websocket(),
            "expected true when capability key is present"
        );
    }

    // -----------------------------------------------------------------------
    // supports_chat_websocket_false
    // -----------------------------------------------------------------------

    /// Oracle: capabilities does not contain "urn:ietf:params:jmap:chat:websocket" →
    /// supports_chat_websocket() returns false.
    #[test]
    fn supports_chat_websocket_false() {
        let session = make_session(json!({}), json!({}), json!({}));
        assert!(
            !session.supports_chat_websocket(),
            "expected false when capability key is absent"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // Each test deserialises wire JSON containing a synthetic `acmeCorp*`
    // vendor field and asserts it survives in `extra`. The vendor field
    // names cannot collide with any field defined in
    // draft-atwood-jmap-chat-00 §3 or draft-atwood-jmap-chat-push-00, so
    // the tests are independent of the code under test (workspace
    // test-integrity rule).

    /// Oracle: `supportedBodyTypes` on the wire deserializes into
    /// `ChatCapability.supported_body_types: Vec<String>` preserving
    /// order. The spec (draft-atwood-jmap-chat-00 §3) mandates
    /// "text/plain" and recommends a defined set of additional values;
    /// the client trusts the server's advertised list verbatim.
    #[test]
    fn chat_capability_supported_body_types_round_trips() {
        let raw = json!({
            "maxBodyBytes": 65536,
            "maxAttachmentBytes": 10485760,
            "maxAttachmentsPerMessage": 10,
            "supportsThreads": true,
            "supportedBodyTypes": [
                "text/plain",
                "text/markdown",
                "application/jmap-chat-rich"
            ]
        });
        let cap: ChatCapability =
            serde_json::from_value(raw).expect("ChatCapability must deserialize");
        assert_eq!(
            cap.supported_body_types,
            vec![
                "text/plain".to_owned(),
                "text/markdown".to_owned(),
                "application/jmap-chat-rich".to_owned(),
            ],
            "supported_body_types must preserve wire order"
        );
    }

    /// Oracle: a server that omits `supportedBodyTypes` deserializes
    /// to an empty `Vec` via `#[serde(default)]`. This is technically
    /// non-compliant per spec (`"text/plain"` is mandatory) but the
    /// client tolerates it — enforcement is the consumer's job.
    #[test]
    fn chat_capability_supported_body_types_absent_defaults_empty() {
        let raw = json!({
            "maxBodyBytes": 65536,
            "maxAttachmentBytes": 10485760,
            "maxAttachmentsPerMessage": 10,
            "supportsThreads": true
        });
        let cap: ChatCapability =
            serde_json::from_value(raw).expect("ChatCapability must deserialize");
        assert!(
            cap.supported_body_types.is_empty(),
            "missing supportedBodyTypes must default to an empty Vec"
        );
    }

    /// `ChatCapability.extra` captures unknown fields on deserialize.
    #[test]
    fn chat_capability_preserves_vendor_extras() {
        let raw = json!({
            "maxBodyBytes": 65536,
            "maxAttachmentBytes": 10485760,
            "maxAttachmentsPerMessage": 10,
            "supportsThreads": true,
            "acmeCorpFeatureFlag": "beta"
        });
        let obj: ChatCapability =
            serde_json::from_value(raw).expect("ChatCapability must deserialize");
        assert_eq!(
            obj.extra
                .get("acmeCorpFeatureFlag")
                .and_then(|v| v.as_str()),
            Some("beta")
        );
    }

    /// `ChatPushCapability.extra` captures unknown fields on deserialize.
    #[test]
    fn chat_push_capability_preserves_vendor_extras() {
        let raw = json!({
            "maxSnippetBytes": 256,
            "supportedUrgencyValues": ["normal", "high"],
            "maxMessagesPerPush": 10,
            "acmeCorpPushTier": "gold"
        });
        let obj: ChatPushCapability =
            serde_json::from_value(raw).expect("ChatPushCapability must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpPushTier").and_then(|v| v.as_str()),
            Some("gold")
        );
    }
}
