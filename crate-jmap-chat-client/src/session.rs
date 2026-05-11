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
    /// Maximum number of members in a group Chat.
    pub max_group_members: u64,
    /// Maximum number of members in a Space.
    pub max_space_members: u64,
    /// Maximum number of roles per Space.
    pub max_roles_per_space: u64,
    /// Maximum number of channels per Space.
    pub max_channels_per_space: u64,
    /// Maximum number of categories per Space.
    pub max_categories_per_space: u64,
    /// Whether the server supports the optional thread model.
    pub supports_threads: bool,
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
                            "maxGroupMembers": 100,
                            "maxSpaceMembers": 500,
                            "maxRolesPerSpace": 50,
                            "maxChannelsPerSpace": 200,
                            "maxCategoriesPerSpace": 25,
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
        assert_eq!(cap.max_group_members, 100);
        assert_eq!(cap.max_space_members, 500);
        assert_eq!(cap.max_roles_per_space, 50);
        assert_eq!(cap.max_channels_per_space, 200);
        assert_eq!(cap.max_categories_per_space, 25);
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
}
