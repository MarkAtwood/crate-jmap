//! JMAP MDN types (RFC 9007).
//!
//! Covers all request/response types for `MDN/send` (§3.1) and `MDN/parse` (§3.3),
//! plus the [`Mdn`] object and its sub-types.
//!
//! All items are gated by the `mdn` feature flag (enabled at the module level in
//! `lib.rs` via `#[cfg(feature = "mdn")]`).

use std::collections::HashMap;

use jmap_types::{Id, PatchObject};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Capability URI for JMAP MDN (RFC 9007 §2).
pub const JMAP_MDN_URI: &str = "urn:ietf:params:jmap:mdn";

/// Whether the MDN was triggered manually or automatically (RFC 9007 §2,
/// derived from RFC 8098 disposition-mode action-mode).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionMode {
    /// The user manually caused the MDN to be sent.
    ManualAction,
    /// The MDN was sent automatically without user involvement.
    AutomaticAction,
}

impl std::fmt::Display for ActionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ActionMode::ManualAction => "manual-action",
            ActionMode::AutomaticAction => "automatic-action",
        })
    }
}

/// Whether the MDN itself was sent manually or automatically (RFC 9007 §2,
/// derived from RFC 8098 disposition-mode sending-mode).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SendingMode {
    /// The user explicitly requested the MDN be sent.
    MdnSentManually,
    /// The MDN was generated and sent automatically.
    MdnSentAutomatically,
}

impl std::fmt::Display for SendingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SendingMode::MdnSentManually => "mdn-sent-manually",
            SendingMode::MdnSentAutomatically => "mdn-sent-automatically",
        })
    }
}

/// The disposition type — what happened to the original message
/// (RFC 9007 §2, derived from RFC 8098 disposition-type).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DispositionType {
    /// The message was deleted without being displayed.
    Deleted,
    /// The message was dispatched to a further destination.
    Dispatched,
    /// The message was displayed to the user.
    Displayed,
    /// The message was processed in some manner without being displayed.
    Processed,
}

impl std::fmt::Display for DispositionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DispositionType::Deleted => "deleted",
            DispositionType::Dispatched => "dispatched",
            DispositionType::Displayed => "displayed",
            DispositionType::Processed => "processed",
        })
    }
}

/// RFC 8098 disposition field — describes the action taken on the original message
/// (RFC 9007 §2).
///
/// Construct with [`Disposition::new`] rather than struct-literal syntax (the
/// struct is `#[non_exhaustive]` to allow future fields).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disposition {
    /// Whether the MDN was triggered manually or automatically.
    pub action_mode: ActionMode,
    /// Whether the MDN itself was sent manually or automatically.
    pub sending_mode: SendingMode,
    /// What happened to the original message.
    ///
    /// Wire name is `type` (a Rust keyword — accessed as `type_` in code).
    #[serde(rename = "type")]
    pub type_: DispositionType,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Disposition {
    /// Construct a [`Disposition`] from its three required fields.
    pub fn new(action_mode: ActionMode, sending_mode: SendingMode, type_: DispositionType) -> Self {
        Self {
            action_mode,
            sending_mode,
            type_,
            extra: serde_json::Map::new(),
        }
    }
}

/// An MDN object as defined in RFC 9007 §2.
///
/// Represents either a to-be-sent MDN (in [`MdnSendRequest`]) or a parsed MDN
/// (in [`MdnParseResponse`]).
///
/// The struct is `#[non_exhaustive]` to allow future fields without a breaking
/// version bump. Construct with [`Mdn::new`] rather than struct-literal syntax;
/// set optional fields directly after construction.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mdn {
    /// The JMAP email ID of the message this MDN is for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub for_email_id: Option<Id>,

    /// Subject of the MDN message (defaults to auto-generated if absent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    /// Human-readable explanation in the text/plain part of the MDN.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_body: Option<String>,

    /// If `true`, the original message is attached to the MDN.
    ///
    /// Defaults to `false` when deserialized from a request that omits the field.
    #[serde(default)]
    pub include_original_message: bool,

    /// Identifying information for the MUA that generated the MDN.
    #[serde(rename = "reportingUA", skip_serializing_if = "Option::is_none")]
    pub reporting_ua: Option<String>,

    /// Disposition describing the action taken on the original message.
    pub disposition: Disposition,

    /// Gateway or MTA name through which the MDN passed (server-set on parse).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mdn_gateway: Option<String>,

    /// Original recipient address as per RFC 8098 Original-Recipient header (server-set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_recipient: Option<String>,

    /// Final recipient address (the address to which the message was delivered).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_recipient: Option<String>,

    /// Message-ID of the original message (server-set on parse).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_message_id: Option<String>,

    /// Error descriptions from the MDN (server-set on parse).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Vec<String>>,

    /// Extension fields from the MDN.
    ///
    /// Wire name is `extensionFields` per RFC 9007 §2 (normative).
    /// The §3.1 example incorrectly uses `extension`; the §2 definition is authoritative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_fields: Option<HashMap<String, String>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// Distinct from RFC 9007 `extensionFields` above: that field carries
    /// RFC 9007-defined MDN extension headers; `extra` captures JMAP-level
    /// vendor / site fields on the `Mdn` object itself.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Mdn {
    /// Construct an [`Mdn`] with the required `disposition` field.
    ///
    /// All optional fields are initialised to `None` / `false`. Set them
    /// directly after construction as needed, e.g.:
    ///
    /// ```rust
    /// # use jmap_mail_types::mdn::{Mdn, Disposition, ActionMode, SendingMode, DispositionType};
    /// let mut mdn = Mdn::new(Disposition::new(
    ///     ActionMode::ManualAction,
    ///     SendingMode::MdnSentManually,
    ///     DispositionType::Displayed,
    /// ));
    /// mdn.subject = Some("Read: Hello".to_owned());
    /// ```
    pub fn new(disposition: Disposition) -> Self {
        Self {
            for_email_id: None,
            subject: None,
            text_body: None,
            include_original_message: false,
            reporting_ua: None,
            disposition,
            mdn_gateway: None,
            original_recipient: None,
            final_recipient: None,
            original_message_id: None,
            error: None,
            extension_fields: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Request object for `MDN/send` (RFC 9007 §3.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MdnSendRequest {
    /// The account to act on.
    pub account_id: Id,
    /// The identity to use as the MDN sender.
    pub identity_id: Id,
    /// Map of client-assigned creation IDs to [`Mdn`] objects to send.
    pub send: HashMap<String, Mdn>,
    /// Patches to apply to Email objects on successful send.
    ///
    /// Keys are Email IDs (or `#creationId` references resolved by the
    /// dispatcher); values are PatchObjects (RFC 8620 §5.3). Both
    /// [`Id`] and [`PatchObject`] are `#[serde(transparent)]`, so the
    /// wire format is byte-identical to a `HashMap<String, Object>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_update_email: Option<HashMap<Id, PatchObject>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response object for `MDN/send` (RFC 9007 §3.1).
///
/// The `notSent` map values are JMAP SetError objects (RFC 8620 §5.3) serialized
/// as JSON objects; they are typed as [`Value`] here to avoid an upward dependency
/// on the server crate.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MdnSendResponse {
    /// The account the operation was performed on.
    pub account_id: Id,
    /// Map of client creation IDs to the MDN objects that were successfully sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent: Option<HashMap<String, Mdn>>,
    /// Map of client creation IDs to SetError objects for MDNs that could not be sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_sent: Option<HashMap<String, Value>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Request object for `MDN/parse` (RFC 9007 §3.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MdnParseRequest {
    /// The account to act on.
    pub account_id: Id,
    /// Blob IDs to parse as MDN messages.
    pub blob_ids: Vec<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response object for `MDN/parse` (RFC 9007 §3.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MdnParseResponse {
    /// The account the operation was performed on.
    pub account_id: Id,
    /// Map of blob IDs that were successfully parsed to their [`Mdn`] representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed: Option<HashMap<Id, Mdn>>,
    /// Blob IDs that could not be parsed as MDN messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_parsable: Option<Vec<Id>>,
    /// Blob IDs that were not found in the account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_found: Option<Vec<Id>>,
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

    /// Round-trip test for [`Disposition`] against the wire format specified
    /// in RFC 9007 §2.
    ///
    /// Wire values are taken directly from the spec text — this is the independent
    /// oracle.
    #[test]
    fn disposition_roundtrip() {
        // Spec §2: actionMode values are "manual-action" and "automatic-action"
        // Spec §2: sendingMode values are "mdn-sent-manually" and "mdn-sent-automatically"
        // Spec §2: type values are "deleted", "dispatched", "displayed", "processed"
        let json = r#"{"actionMode":"manual-action","sendingMode":"mdn-sent-manually","type":"displayed"}"#;
        let d: Disposition = serde_json::from_str(json).unwrap();
        assert_eq!(d.action_mode, ActionMode::ManualAction);
        assert_eq!(d.sending_mode, SendingMode::MdnSentManually);
        assert_eq!(d.type_, DispositionType::Displayed);

        let serialized = serde_json::to_string(&d).unwrap();
        assert_eq!(serialized, json);
    }

    /// Round-trip test for [`Mdn`] verifying camelCase wire names and
    /// `include_original_message` default behaviour.
    ///
    /// Field names taken from RFC 9007 §2 (normative definition).
    #[test]
    fn mdn_camel_case_wire_names() {
        // Minimal MDN — only required field is `disposition`.
        // Spec §2: includeOriginalMessage defaults to false.
        let json = r#"{
            "forEmailId": "e1",
            "subject": "Read: Hello",
            "textBody": "This is a receipt.",
            "reportingUA": "Acme Mail 1.0; example.com",
            "disposition": {
                "actionMode": "manual-action",
                "sendingMode": "mdn-sent-manually",
                "type": "displayed"
            }
        }"#;
        let mdn: Mdn = serde_json::from_str(json).unwrap();
        assert_eq!(mdn.for_email_id.as_ref().map(|id| id.as_ref()), Some("e1"));
        assert_eq!(mdn.subject.as_deref(), Some("Read: Hello"));
        assert!(!mdn.include_original_message, "should default to false");
        assert_eq!(mdn.disposition.type_, DispositionType::Displayed);
    }

    /// Verify `extensionFields` wire name (not `extension`).
    ///
    /// RFC 9007 §2 uses `extensionFields` (normative).
    /// The §3.1 example incorrectly uses `extension` — we follow §2.
    #[test]
    fn extension_fields_wire_name() {
        let json = r#"{"disposition":{"actionMode":"manual-action","sendingMode":"mdn-sent-manually","type":"processed"},"extensionFields":{"X-Custom":"value"}}"#;
        let mdn: Mdn = serde_json::from_str(json).unwrap();
        let fields = mdn.extension_fields.as_ref().unwrap();
        assert_eq!(fields.get("X-Custom").map(|s| s.as_str()), Some("value"));

        let serialized = serde_json::to_string(&mdn).unwrap();
        assert!(
            serialized.contains("extensionFields"),
            "must use extensionFields not extension"
        );
    }

    /// Verify all four DispositionType variants serialize to lowercase per spec §2.
    #[test]
    fn disposition_type_lowercase() {
        // Spec §2: all type values are lowercase
        let cases = [
            (DispositionType::Deleted, "\"deleted\""),
            (DispositionType::Dispatched, "\"dispatched\""),
            (DispositionType::Displayed, "\"displayed\""),
            (DispositionType::Processed, "\"processed\""),
        ];
        for (variant, expected) in &cases {
            let got = serde_json::to_string(variant).unwrap();
            assert_eq!(&got, expected, "DispositionType wire value mismatch");
        }
    }

    /// Verify ActionMode and SendingMode wire values per spec §2.
    #[test]
    fn action_and_sending_mode_wire_values() {
        // Spec §2: actionMode values
        assert_eq!(
            serde_json::to_string(&ActionMode::ManualAction).unwrap(),
            "\"manual-action\""
        );
        assert_eq!(
            serde_json::to_string(&ActionMode::AutomaticAction).unwrap(),
            "\"automatic-action\""
        );
        // Spec §2: sendingMode values
        assert_eq!(
            serde_json::to_string(&SendingMode::MdnSentManually).unwrap(),
            "\"mdn-sent-manually\""
        );
        assert_eq!(
            serde_json::to_string(&SendingMode::MdnSentAutomatically).unwrap(),
            "\"mdn-sent-automatically\""
        );
    }

    /// Verify MdnSendRequest round-trips with camelCase field names.
    ///
    /// Field names taken from RFC 9007 §3.1.
    #[test]
    fn mdn_send_request_roundtrip() {
        // Wire JSON uses camelCase per spec §3.1
        let json = r#"{
            "accountId": "acc1",
            "identityId": "idt1",
            "send": {
                "k1": {
                    "forEmailId": "e1",
                    "disposition": {
                        "actionMode": "manual-action",
                        "sendingMode": "mdn-sent-manually",
                        "type": "displayed"
                    }
                }
            }
        }"#;
        let req: MdnSendRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.account_id.as_ref(), "acc1");
        assert_eq!(req.identity_id.as_ref(), "idt1");
        assert!(req.send.contains_key("k1"));
        assert!(req.on_success_update_email.is_none());
    }

    /// Verify MdnSendRequest round-trips with `onSuccessUpdateEmail`
    /// populated — the typed shape `HashMap<Id, PatchObject>` MUST be
    /// wire-byte-identical to a `HashMap<String, Object>` because both
    /// `Id` and `PatchObject` are `#[serde(transparent)]`.
    ///
    /// Oracle: hand-written JSON literal taken from RFC 9007 §3.1
    /// (`onSuccessUpdateEmail` field shape) and RFC 8620 §5.3
    /// (PatchObject path-key syntax with the `keywords/$mdnsent`
    /// example used in the MDN/send acceptance flow).
    #[test]
    fn mdn_send_request_on_success_update_email_roundtrip() {
        // Raw-string fence is `r##"..."##` because the JSON body itself
        // contains a `#` (in the `#k1` creation-id reference key) — a
        // single-`#` raw-string fence would terminate prematurely.
        let json = r##"{
            "accountId": "acc1",
            "identityId": "idt1",
            "send": {
                "k1": {
                    "forEmailId": "e1",
                    "disposition": {
                        "actionMode": "manual-action",
                        "sendingMode": "mdn-sent-manually",
                        "type": "displayed"
                    }
                }
            },
            "onSuccessUpdateEmail": {
                "#k1": { "keywords/$mdnsent": true }
            }
        }"##;
        let req: MdnSendRequest = serde_json::from_str(json).unwrap();

        // Verify the typed shape: Id key, PatchObject value.
        let patches = req
            .on_success_update_email
            .as_ref()
            .expect("onSuccessUpdateEmail must deserialize as Some");
        let key = Id::from("#k1");
        let patch = patches
            .get(&key)
            .expect("patch for #k1 must be present after round-trip");
        assert_eq!(
            patch.as_map().get("keywords/$mdnsent"),
            Some(&serde_json::json!(true)),
            "patch leaf must round-trip the boolean true"
        );
        assert_eq!(patch.as_map().len(), 1, "exactly one leaf in the patch");

        // Re-serialize and compare just the `onSuccessUpdateEmail` subtree
        // structurally. Comparing the whole document would also pick up the
        // pre-existing `Mdn::include_original_message` serialise-default
        // behaviour, which is unrelated to this migration.
        //
        // Equality of these two subtrees proves that both `Id`
        // (the map key type) and `PatchObject` (the map value type) are
        // wire-byte-identical to plain `String` and `Object` — i.e. that
        // `#[serde(transparent)]` is doing what we claim it does.
        let re_serialized = serde_json::to_value(&req).unwrap();
        let original: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            re_serialized.get("onSuccessUpdateEmail"),
            original.get("onSuccessUpdateEmail"),
            "onSuccessUpdateEmail must round-trip wire-byte-identical \
             through HashMap<Id, PatchObject>"
        );
    }

    /// Verify MdnParseResponse round-trips with camelCase field names.
    ///
    /// Field names taken from RFC 9007 §3.3.
    #[test]
    fn mdn_parse_response_roundtrip() {
        // Wire JSON uses camelCase per spec §3.3
        let json = r#"{
            "accountId": "acc1",
            "notParsable": ["blob2"],
            "notFound": ["blob3"]
        }"#;
        let resp: MdnParseResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.account_id.as_ref(), "acc1");
        assert!(resp.parsed.is_none());
        assert_eq!(resp.not_parsable.as_deref(), Some(&[Id::from("blob2")][..]));
        assert_eq!(resp.not_found.as_deref(), Some(&[Id::from("blob3")][..]));
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.2) ───────────────────
    //
    // One round-trip preservation test per migrated type. Each asserts
    // that an unknown vendor / site / private-extension field survives
    // deserialize/serialize unchanged. Per workspace AGENTS.md
    // "Extras-preservation policy for vendor/site fields".

    /// `Disposition.extra` captures vendor fields and preserves them.
    #[test]
    fn disposition_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "actionMode": "manual-action",
            "sendingMode": "mdn-sent-manually",
            "type": "displayed",
            "acmeCorpDispositionFlag": "auto-ack"
        });
        let d: Disposition = serde_json::from_value(raw).unwrap();
        assert_eq!(
            d.extra
                .get("acmeCorpDispositionFlag")
                .and_then(|v| v.as_str()),
            Some("auto-ack")
        );
        let back = serde_json::to_value(&d).unwrap();
        assert_eq!(back["acmeCorpDispositionFlag"], "auto-ack");
    }

    /// `Mdn.extra` captures vendor fields and preserves them. Distinct from
    /// the typed RFC 9007 `extensionFields` member which carries
    /// MDN-extension headers only.
    #[test]
    fn mdn_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "forEmailId": "e1",
            "disposition": {
                "actionMode": "manual-action",
                "sendingMode": "mdn-sent-manually",
                "type": "displayed"
            },
            "acmeCorpClientTrace": "ua-42"
        });
        let mdn: Mdn = serde_json::from_value(raw).unwrap();
        assert_eq!(
            mdn.extra
                .get("acmeCorpClientTrace")
                .and_then(|v| v.as_str()),
            Some("ua-42")
        );
        let back = serde_json::to_value(&mdn).unwrap();
        assert_eq!(back["acmeCorpClientTrace"], "ua-42");
    }

    /// `MdnSendRequest.extra` captures vendor fields and preserves them.
    #[test]
    fn mdn_send_request_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "a1",
            "identityId": "i1",
            "send": {},
            "acmeCorpRequestTag": "batch-7"
        });
        let req: MdnSendRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(
            req.extra.get("acmeCorpRequestTag").and_then(|v| v.as_str()),
            Some("batch-7")
        );
        let back = serde_json::to_value(&req).unwrap();
        assert_eq!(back["acmeCorpRequestTag"], "batch-7");
    }

    /// `MdnSendResponse.extra` captures vendor fields and preserves them.
    #[test]
    fn mdn_send_response_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "a1",
            "acmeCorpServerTrace": "node-3"
        });
        let resp: MdnSendResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(
            resp.extra
                .get("acmeCorpServerTrace")
                .and_then(|v| v.as_str()),
            Some("node-3")
        );
        let back = serde_json::to_value(&resp).unwrap();
        assert_eq!(back["acmeCorpServerTrace"], "node-3");
    }

    /// `MdnParseRequest.extra` captures vendor fields and preserves them.
    #[test]
    fn mdn_parse_request_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "a1",
            "blobIds": ["b1"],
            "acmeCorpParseHint": "lenient"
        });
        let req: MdnParseRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(
            req.extra.get("acmeCorpParseHint").and_then(|v| v.as_str()),
            Some("lenient")
        );
        let back = serde_json::to_value(&req).unwrap();
        assert_eq!(back["acmeCorpParseHint"], "lenient");
    }

    /// `MdnParseResponse.extra` captures vendor fields and preserves them.
    #[test]
    fn mdn_parse_response_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "a1",
            "acmeCorpStatus": "complete"
        });
        let resp: MdnParseResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(
            resp.extra.get("acmeCorpStatus").and_then(|v| v.as_str()),
            Some("complete")
        );
        let back = serde_json::to_value(&resp).unwrap();
        assert_eq!(back["acmeCorpStatus"], "complete");
    }
}
