//! ChatSessionExt trait for [`jmap_base_client::Session`].
//!
//! Adds JMAP Chat extension methods to the base `Session` type.
//!
//! Specs:
//!   - draft-atwood-jmap-chat-00 §3      (ChatCapability fields)
//!   - draft-atwood-jmap-chat-push-00    (ChatPushCapability fields)
//!   - draft-atwood-jmap-chat-wss-00     (supports_chat_websocket)
//!
//! [`ChatCapability`] and [`ChatPushCapability`] live in `jmap-chat-types`
//! (the canonical home for chat wire-format types); this module re-exports
//! them for ergonomic use from the trait signatures below.

pub use jmap_chat_types::{ChatCapability, ChatPushCapability};

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
///
/// This trait is **sealed**: implementations outside this crate are not
/// permitted. The crate adds an `impl` only for
/// [`jmap_base_client::Session`]. Sealing prevents downstream
/// divergence and keeps adding methods to the trait a non-breaking
/// change.
pub trait ChatSessionExt: sealed::Sealed {
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

mod sealed {
    /// Sealing-trait for [`super::ChatSessionExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::Session {}
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
        // Delegate to the foundation helper rather than duplicating its
        // body. Future changes to the helper (extra logging, error
        // mapping, telemetry) propagate automatically.
        account.account_extension_capability::<ChatCapability>("urn:ietf:params:jmap:chat")
    }

    fn chat_push_capability(
        &self,
        account_id: &str,
    ) -> Result<Option<ChatPushCapability>, jmap_base_client::ClientError> {
        let Some(account) = self.accounts.get(account_id) else {
            return Ok(None);
        };
        // Delegate to the foundation helper — see chat_capability above.
        account.account_extension_capability::<ChatPushCapability>("urn:ietf:params:jmap:chat:push")
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
    /// `ChatCapability.supported_body_types: Vec<BodyType>` preserving
    /// order, with canonical wire strings mapped to typed variants.
    /// The spec (draft-atwood-jmap-chat-00 §3) mandates "text/plain"
    /// and recommends a defined set of additional values; the client
    /// trusts the server's advertised list verbatim.
    #[test]
    fn chat_capability_supported_body_types_round_trips() {
        use jmap_chat_types::BodyType;
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
            vec![BodyType::Plain, BodyType::Markdown, BodyType::Rich],
            "supported_body_types must preserve wire order and map canonical strings to typed variants"
        );
    }

    /// Oracle: unknown body-type wire strings round-trip via the
    /// `impl_string_enum!` `Other(String)` catch-all when nested inside
    /// `Vec<BodyType>`.
    #[test]
    fn chat_capability_supported_body_types_unknown_variant_round_trips() {
        use jmap_chat_types::BodyType;
        let raw = json!({
            "maxBodyBytes": 65536,
            "maxAttachmentBytes": 10485760,
            "maxAttachmentsPerMessage": 10,
            "supportsThreads": true,
            "supportedBodyTypes": [
                "text/plain",
                "application/mls-ciphertext"
            ]
        });
        let cap: ChatCapability =
            serde_json::from_value(raw).expect("ChatCapability must deserialize");
        assert_eq!(
            cap.supported_body_types,
            vec![
                BodyType::Plain,
                BodyType::Other("application/mls-ciphertext".to_owned()),
            ],
            "unknown wire strings must land in BodyType::Other preserving the original string"
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

    /// `ChatCapability.extra` captures unknown fields on deserialize
    /// AND survives serialize round-trip.
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
            serde_json::from_value(raw.clone()).expect("ChatCapability must deserialize");
        assert_eq!(
            obj.extra
                .get("acmeCorpFeatureFlag")
                .and_then(|v| v.as_str()),
            Some("beta")
        );

        // Serialize round-trip: the vendor field must survive to the
        // wire and the typed fields must NOT be duplicated into extra
        // (no `maxBodyBytes` key inside the flattened-extra payload).
        let reserialized = serde_json::to_value(&obj).expect("ChatCapability must serialize");
        assert_eq!(
            reserialized
                .get("acmeCorpFeatureFlag")
                .and_then(|v| v.as_str()),
            Some("beta"),
            "vendor field must survive deserialize -> serialize"
        );
        assert_eq!(
            reserialized.get("maxBodyBytes").and_then(|v| v.as_u64()),
            Some(65536),
            "typed field must round-trip with its typed value"
        );
        // No duplication: extra must NOT have shadowed any typed key.
        assert!(
            obj.extra.get("maxBodyBytes").is_none(),
            "typed field maxBodyBytes must NOT be duplicated into extra"
        );
        assert!(
            obj.extra.get("supportsThreads").is_none(),
            "typed field supportsThreads must NOT be duplicated into extra"
        );
    }

    /// `ChatPushCapability.extra` captures unknown fields on deserialize
    /// AND survives serialize round-trip.
    #[test]
    fn chat_push_capability_preserves_vendor_extras() {
        let raw = json!({
            "maxSnippetBytes": 256,
            "supportedUrgencyValues": ["normal", "high"],
            "maxMessagesPerPush": 10,
            "acmeCorpPushTier": "gold"
        });
        let obj: ChatPushCapability =
            serde_json::from_value(raw.clone()).expect("ChatPushCapability must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpPushTier").and_then(|v| v.as_str()),
            Some("gold")
        );

        // Serialize round-trip + no-duplication into extra.
        let reserialized = serde_json::to_value(&obj).expect("ChatPushCapability must serialize");
        assert_eq!(
            reserialized
                .get("acmeCorpPushTier")
                .and_then(|v| v.as_str()),
            Some("gold"),
            "vendor field must survive deserialize -> serialize"
        );
        assert_eq!(
            reserialized.get("maxSnippetBytes").and_then(|v| v.as_u64()),
            Some(256),
            "typed field must round-trip with its typed value"
        );
        assert!(
            obj.extra.get("maxSnippetBytes").is_none(),
            "typed field maxSnippetBytes must NOT be duplicated into extra"
        );
        assert!(
            obj.extra.get("supportedUrgencyValues").is_none(),
            "typed field supportedUrgencyValues must NOT be duplicated into extra"
        );
    }

    // ── Strictness regression tests (bd:JMAP-26di.4) ───────────────────
    //
    // Oracle: draft-atwood-jmap-chat-00 §3 lines 171-184 mark
    // maxBodyBytes / maxAttachmentBytes / maxAttachmentsPerMessage /
    // supportsThreads as required (not optional). A server returning `{}`
    // is non-compliant; we surface that as a deserialize error rather
    // than silently zeroing every cap. Same expectation applies to
    // draft-atwood-jmap-chat-push-00 lines 90-94 for maxSnippetBytes /
    // supportedUrgencyValues.

    /// `ChatCapability` from `{}` must FAIL to deserialize. Prior to
    /// bd:JMAP-26di.4 the struct-level `#[serde(default)]` made this
    /// succeed with every field silently zeroed, breaking callers that
    /// trust `max_body_bytes` as a soft validation gate.
    #[test]
    fn chat_capability_empty_object_rejected() {
        let raw = json!({});
        let result: Result<ChatCapability, _> = serde_json::from_value(raw);
        assert!(
            result.is_err(),
            "ChatCapability {{}} must fail deserialize (missing required fields); got Ok"
        );
    }

    /// `ChatCapability` missing only `maxBodyBytes` must FAIL — partial
    /// silent zeroing is the same hazard as `{}`.
    #[test]
    fn chat_capability_missing_max_body_bytes_rejected() {
        let raw = json!({
            "maxAttachmentBytes": 10485760,
            "maxAttachmentsPerMessage": 10,
            "supportsThreads": true
        });
        let result: Result<ChatCapability, _> = serde_json::from_value(raw);
        assert!(
            result.is_err(),
            "ChatCapability without maxBodyBytes must fail deserialize; got Ok"
        );
    }

    /// `ChatPushCapability` from `{}` must FAIL to deserialize.
    /// `maxSnippetBytes` and `supportedUrgencyValues` are spec-required.
    #[test]
    fn chat_push_capability_empty_object_rejected() {
        let raw = json!({});
        let result: Result<ChatPushCapability, _> = serde_json::from_value(raw);
        assert!(
            result.is_err(),
            "ChatPushCapability {{}} must fail deserialize (missing required fields); got Ok"
        );
    }

    /// `ChatPushCapability` missing `supportedUrgencyValues` must FAIL.
    /// `maxMessagesPerPush` IS optional (spec line 96) and may be omitted,
    /// but the other two MUST be present.
    #[test]
    fn chat_push_capability_missing_supported_urgency_values_rejected() {
        let raw = json!({
            "maxSnippetBytes": 256
        });
        let result: Result<ChatPushCapability, _> = serde_json::from_value(raw);
        assert!(
            result.is_err(),
            "ChatPushCapability without supportedUrgencyValues must fail deserialize; got Ok"
        );
    }

    /// `ChatPushCapability` with both required fields but no
    /// `maxMessagesPerPush` must SUCCEED — the optional field stays
    /// optional after the strictness fix.
    #[test]
    fn chat_push_capability_optional_max_messages_per_push_absent_succeeds() {
        let raw = json!({
            "maxSnippetBytes": 256,
            "supportedUrgencyValues": ["normal", "high"]
        });
        let cap: ChatPushCapability = serde_json::from_value(raw)
            .expect("ChatPushCapability without maxMessagesPerPush must deserialize");
        assert_eq!(cap.max_snippet_bytes, 256);
        assert_eq!(
            cap.supported_urgency_values,
            vec![
                jmap_chat_types::UrgencyLevel::Normal,
                jmap_chat_types::UrgencyLevel::High,
            ]
        );
        assert!(
            cap.max_messages_per_push.is_none(),
            "maxMessagesPerPush optional must default to None when absent"
        );
    }
}
