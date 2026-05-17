//! RFC 8621 §4 Email object and its component types.
//!
//! Provides [`Email`], [`EmailAddress`], [`EmailAddressGroup`], [`EmailHeader`],
//! [`EmailBodyPart`], and [`EmailBodyValue`].  These are the types used in
//! `Email/get` responses and `Email/set` requests.
//!
//! See [`Email`] for notes on full vs partial responses.

use std::collections::HashMap;

use jmap_types::{Date, Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::keyword::Keyword;

/// A parsed email address (RFC 8621 §4.1.2.3).
///
/// Represents one address entry from an RFC 5322 address-list.
/// The `email` field contains the "addr-spec"; `name` contains the
/// decoded display-name, or `null` if absent.
///
/// In RFC 5322 terminology this is a "mailbox" (an addr-spec with optional
/// display-name), distinct from the JMAP [`Mailbox`](crate::Mailbox) folder type.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAddress {
    /// The decoded display-name of the mailbox, or `null` if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The addr-spec of the mailbox (e.g. `"user@example.com"`).
    pub email: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl EmailAddress {
    /// Construct an [`EmailAddress`] with no display name.
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            name: None,
            email: email.into(),
            extra: serde_json::Map::new(),
        }
    }
}

/// A named group of email addresses (RFC 8621 §4.1.2.4).
///
/// Preserves RFC 5322 group structure. Consecutive mailboxes not part of
/// a named group are collected under an `EmailAddressGroup` with `name: null`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAddressGroup {
    /// The decoded display-name of the group, or `null` for ungrouped mailboxes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The mailboxes that belong to this group.
    pub addresses: Vec<EmailAddress>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl EmailAddressGroup {
    /// Construct an [`EmailAddressGroup`] with no group name.
    pub fn new(addresses: Vec<EmailAddress>) -> Self {
        Self {
            name: None,
            addresses,
            extra: serde_json::Map::new(),
        }
    }
}

/// A single RFC 5322 header field (RFC 8621 §4.1.3).
///
/// The `name` retains original capitalisation; `value` is the raw field value.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailHeader {
    /// The header field name (e.g. `"Content-Type"`), case-preserved.
    pub name: String,
    /// The header field value in Raw form.
    pub value: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl EmailHeader {
    /// Construct an [`EmailHeader`] from its name and value.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            extra: serde_json::Map::new(),
        }
    }
}

/// The decoded text content of one body part (RFC 8621 §4.1.4).
///
/// Returned inside the `bodyValues` map of an Email object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailBodyValue {
    /// The decoded text content of the part.
    pub value: String,
    /// `true` if charset decoding or content-transfer-encoding decoding
    /// encountered errors (RFC 8621 §4.1.4).
    ///
    /// Always present in serialized output (no `skip_serializing_if`); RFC 8621 §4.1.4
    /// requires both flags in the `bodyValues` map.  `#[serde(default)]` handles
    /// deserialization when absent (treated as `false`).
    #[serde(default)]
    pub is_encoding_problem: bool,
    /// `true` if `value` was truncated due to a `maxBodyValueBytes` limit
    /// (RFC 8621 §4.1.4).
    ///
    /// Always present in serialized output; same rationale as `is_encoding_problem`.
    #[serde(default)]
    pub is_truncated: bool,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl EmailBodyValue {
    /// Construct an [`EmailBodyValue`] with the given text content.
    ///
    /// `is_encoding_problem` and `is_truncated` default to `false`.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            is_encoding_problem: false,
            is_truncated: false,
            extra: serde_json::Map::new(),
        }
    }
}

/// One MIME body part within an Email (RFC 8621 §4.1.4).
///
/// The `sub_parts` field is recursive: multipart bodies nest further
/// `EmailBodyPart` values.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailBodyPart {
    /// Uniquely identifies this part within the Email (null for multipart/*).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    /// Blob id of the decoded part content (null for multipart/*).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<Id>,
    /// Size in octets of the decoded content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// All header fields of the part in Raw form, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<EmailHeader>,
    /// Decoded filename from Content-Disposition or Content-Type parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// MIME content type (e.g. `"text/plain"`).
    // `type` is a Rust keyword; the trailing underscore is the conventional escape.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// Charset parameter of the Content-Type header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charset: Option<String>,
    /// Value of the Content-Disposition header field (parameters stripped).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disposition: Option<String>,
    /// Content-Id value with CFWS and angle brackets removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    /// Language tags from the Content-Language header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Vec<String>>,
    /// URI from the Content-Location header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Child parts when `type_` is `"multipart/*"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_parts: Option<Vec<EmailBodyPart>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An Email object (RFC 8621 §4.1).
///
/// Combines metadata (§4.1.1), parsed header convenience properties (§4.1.3),
/// and body fields (§4.1.4).
///
/// # Full vs partial responses
///
/// This type is designed for **full `Email/get` responses** where all metadata
/// properties are present.  The metadata fields `blob_id`, `thread_id`,
/// `mailbox_ids`, `size`, and `received_at` are required (non-`Option`);
/// deserialization fails if any of them is absent from the JSON.
///
/// RFC 8621 §4.5 allows clients to request only a subset of properties.  If
/// a partial response omits any required metadata field, `serde_json::from_str`
/// will return a "missing field" error.  For partial-property responses,
/// deserialize into `serde_json::Value` first or define a narrower type with
/// all fields `Option`.
///
/// Header convenience properties (§4.1.3) and body fields (§4.1.4) are all
/// `Option`; they deserialize as `None` when not included in the response.
///
/// # Serialization caveat for server implementors
///
/// Several collection fields (`keywords`, `body_values`, `text_body`,
/// `html_body`, `attachments`, `headers`) use
/// `#[serde(skip_serializing_if = "…::is_empty")]`.  This is correct for
/// partial responses — a property not in the client's `properties` list MUST be
/// absent from the response.  However, RFC 8621 §4.1.1 defines `keywords` with
/// `default: {}`, meaning a server MUST include `"keywords":{}` in the response
/// when the property was requested and the email has no keywords.
///
/// **Do not rely on `serde_json::to_value(email)` to produce RFC-compliant JSON
/// for full-object responses.**  Server code in `jmap-mail-server` must
/// explicitly populate any collection fields that are in the requested
/// `properties` set before serialization, or use a custom serializer that
/// includes them.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email {
    // --- Metadata (§4.1.1) ---
    /// The JMAP object id of this Email.
    pub id: Id,
    /// Blob id of the raw RFC 5322 message octets.
    pub blob_id: Id,
    /// Id of the Thread this Email belongs to.
    pub thread_id: Id,
    /// Set of Mailbox ids this Email belongs to.
    ///
    /// Represented as `HashMap<Id, bool>` because the JMAP wire format uses a JSON object
    /// with boolean values (RFC 8621 §4.1.1).  Values are always `true` in full-object
    /// responses; the map shape is also used in PatchObject updates (RFC 8620 §5.3) where
    /// a `null` value removes an entry.
    pub mailbox_ids: HashMap<Id, bool>,
    /// Keywords applied to this Email.
    ///
    /// Same JSON object shape as `mailbox_ids` (string keys, boolean values) — JMAP wire
    /// format requirement.  Keys are [`Keyword`] values (not JMAP `Id`s); system keywords
    /// start with `$` which is not valid inside a JMAP `Id` (RFC 8620 §1.2).
    /// Values are always `true` in full-object responses (RFC 8621 §4.1.1).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub keywords: HashMap<Keyword, bool>,
    /// Size in octets of the raw RFC 5322 message.
    pub size: u64,
    /// Date the Email was received by the message store.
    pub received_at: UTCDate,

    // --- Parsed header convenience properties (§4.1.3) ---
    /// Value of the Message-ID header field as a list of message ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<Vec<String>>,
    /// Value of the In-Reply-To header field as a list of message ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<Vec<String>>,
    /// Value of the References header field as a list of message ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<String>>,
    /// Parsed addresses from the Sender header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<Vec<EmailAddress>>,
    /// Parsed addresses from the From header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<Vec<EmailAddress>>,
    /// Parsed addresses from the To header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Vec<EmailAddress>>,
    /// Parsed addresses from the Cc header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc: Option<Vec<EmailAddress>>,
    /// Parsed addresses from the Bcc header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bcc: Option<Vec<EmailAddress>>,
    /// Parsed addresses from the Reply-To header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Vec<EmailAddress>>,
    /// Decoded text value of the Subject header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Parsed value of the Date header field (RFC 8621 §4.1.3).
    ///
    /// Type `Date` (any RFC 3339 timezone offset) per the RFC.  Email Date
    /// headers commonly carry non-UTC offsets such as `"+10:00"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<Date>,

    // --- Raw headers (§4.1.3) ---
    /// All header fields of the message in Raw form, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<EmailHeader>,

    // --- Body fields (§4.1.4) ---
    /// Map from partId to decoded text content for text body parts.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub body_values: HashMap<String, EmailBodyValue>,
    /// Text body parts to display, preferring text/plain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_body: Vec<EmailBodyPart>,
    /// HTML body parts to display, preferring text/html.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub html_body: Vec<EmailBodyPart>,
    /// All attachment parts (depth-first, excluding subParts).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<EmailBodyPart>,
    /// Full MIME body structure of the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_structure: Option<EmailBodyPart>,
    /// True if there is at least one downloadable attachment.
    #[serde(default)]
    pub has_attachment: bool,
    /// Short plaintext preview of the message body (≤256 characters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Email {
    /// Construct an [`Email`] from its six required metadata fields.
    ///
    /// All parsed-header and body fields default to `None` / empty.
    pub fn new(
        id: Id,
        blob_id: Id,
        thread_id: Id,
        mailbox_ids: HashMap<Id, bool>,
        size: u64,
        received_at: UTCDate,
    ) -> Self {
        Self {
            id,
            blob_id,
            thread_id,
            mailbox_ids,
            keywords: HashMap::new(),
            size,
            received_at,
            message_id: None,
            in_reply_to: None,
            references: None,
            sender: None,
            from: None,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            subject: None,
            sent_at: None,
            headers: Vec::new(),
            body_values: HashMap::new(),
            text_body: Vec::new(),
            html_body: Vec::new(),
            attachments: Vec::new(),
            body_structure: None,
            has_attachment: false,
            preview: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Extras-preservation policy tests (JMAP-lbdy.2) ───────────────────
    //
    // One round-trip preservation test per migrated type. Each test
    // asserts that an unknown vendor / site / private-extension field
    // survives deserialize/serialize unchanged. Per workspace
    // AGENTS.md "Extras-preservation policy for vendor/site fields".

    /// `EmailAddress.extra` captures vendor fields and preserves them.
    #[test]
    fn email_address_preserves_vendor_extras() {
        let raw = json!({
            "name": "Alice",
            "email": "alice@example.com",
            "acmeCorpVerified": true
        });
        let addr: EmailAddress = serde_json::from_value(raw).unwrap();
        assert_eq!(
            addr.extra.get("acmeCorpVerified").and_then(|v| v.as_bool()),
            Some(true)
        );
        let back = serde_json::to_value(&addr).unwrap();
        assert_eq!(back["acmeCorpVerified"], true);
    }

    /// `EmailAddressGroup.extra` captures vendor fields and preserves them.
    #[test]
    fn email_address_group_preserves_vendor_extras() {
        let raw = json!({
            "name": "Engineering",
            "addresses": [],
            "acmeCorpDistributionId": "dl-eng"
        });
        let grp: EmailAddressGroup = serde_json::from_value(raw).unwrap();
        assert_eq!(
            grp.extra
                .get("acmeCorpDistributionId")
                .and_then(|v| v.as_str()),
            Some("dl-eng")
        );
        let back = serde_json::to_value(&grp).unwrap();
        assert_eq!(back["acmeCorpDistributionId"], "dl-eng");
    }

    /// `EmailHeader.extra` captures vendor fields and preserves them.
    #[test]
    fn email_header_preserves_vendor_extras() {
        let raw = json!({
            "name": "X-Custom",
            "value": "v",
            "acmeCorpOrigin": "edge-1"
        });
        let hdr: EmailHeader = serde_json::from_value(raw).unwrap();
        assert_eq!(
            hdr.extra.get("acmeCorpOrigin").and_then(|v| v.as_str()),
            Some("edge-1")
        );
        let back = serde_json::to_value(&hdr).unwrap();
        assert_eq!(back["acmeCorpOrigin"], "edge-1");
    }

    /// `EmailBodyValue.extra` captures vendor fields and preserves them.
    #[test]
    fn email_body_value_preserves_vendor_extras() {
        let raw = json!({
            "value": "hello",
            "isEncodingProblem": false,
            "isTruncated": false,
            "acmeCorpScanResult": "clean"
        });
        let bv: EmailBodyValue = serde_json::from_value(raw).unwrap();
        assert_eq!(
            bv.extra.get("acmeCorpScanResult").and_then(|v| v.as_str()),
            Some("clean")
        );
        let back = serde_json::to_value(&bv).unwrap();
        assert_eq!(back["acmeCorpScanResult"], "clean");
    }

    /// `EmailBodyPart.extra` captures vendor fields and preserves them.
    #[test]
    fn email_body_part_preserves_vendor_extras() {
        let raw = json!({
            "partId": "1",
            "blobId": "b1",
            "size": 42,
            "type": "text/plain",
            "acmeCorpChecksum": "sha256:deadbeef"
        });
        let part: EmailBodyPart = serde_json::from_value(raw).unwrap();
        assert_eq!(
            part.extra.get("acmeCorpChecksum").and_then(|v| v.as_str()),
            Some("sha256:deadbeef")
        );
        let back = serde_json::to_value(&part).unwrap();
        assert_eq!(back["acmeCorpChecksum"], "sha256:deadbeef");
    }

    /// `Email.extra` captures vendor fields and preserves them across
    /// deserialize/serialize round-trip.
    #[test]
    fn email_preserves_vendor_extras() {
        let raw = json!({
            "id": "e1",
            "blobId": "b1",
            "threadId": "t1",
            "mailboxIds": {"m1": true},
            "size": 1024,
            "receivedAt": "2024-06-01T00:00:00Z",
            "acmeCorpClassification": {"label": "internal", "score": 0.9}
        });
        let email: Email = serde_json::from_value(raw).unwrap();
        assert_eq!(
            email
                .extra
                .get("acmeCorpClassification")
                .and_then(|v| v["label"].as_str()),
            Some("internal")
        );
        let back = serde_json::to_value(&email).unwrap();
        assert_eq!(back["acmeCorpClassification"]["score"], 0.9);
    }

    /// Empty extras must NOT serialize as a key on the wire — wire shape
    /// is byte-identical to the pre-migration form when no vendor fields
    /// are present.
    #[test]
    fn email_address_empty_extras_omitted_from_wire() {
        let addr = EmailAddress::new("a@b");
        let serialized = serde_json::to_value(&addr).unwrap();
        let obj = serialized.as_object().expect("must be object");
        // Only "email" — name is None and is skip_serializing_if; extra is empty.
        assert_eq!(
            obj.len(),
            1,
            "empty extras must not add wire keys; got {serialized}"
        );
        assert!(obj.contains_key("email"));
    }

    /// Regression test locking in the wire-key-collision contract.
    ///
    /// Workspace AGENTS.md ("Extras-preservation policy" → "Caller
    /// contract — wire-key collisions") documents that a caller MUST
    /// NOT insert a key into `extra` whose name matches a typed field
    /// on the same struct. This test asserts the observable behavior
    /// that justifies the doc warning:
    ///
    /// 1. Serialize succeeds and emits both the typed field and the
    ///    `extra` entry (no dedup by serde-flatten).
    /// 2. Deserialize on the same bytes rejects with `duplicate field`.
    ///
    /// If a future serde or serde_json release changes either behavior
    /// (e.g. silently drops the typed field, silently drops the flatten
    /// entry, accepts duplicate keys), this test will fail loudly so
    /// the workspace policy can be re-evaluated rather than silently
    /// drift.
    ///
    /// Independent oracle: the expected JSON shape (two `"email"`
    /// keys) is constructed by string concatenation, not by the code
    /// under test.
    #[test]
    fn extra_collision_with_typed_field_round_trip_fails() {
        let mut addr = EmailAddress::new("real@example.com");
        addr.extra.insert(
            "email".into(),
            serde_json::Value::from("override@example.com"),
        );

        // (1) Serialize succeeds and emits both keys.
        let serialised = serde_json::to_string(&addr).expect("serialize must succeed");
        // Independent oracle: the bytes must contain two "email" keys.
        let occurrences = serialised.matches("\"email\":").count();
        assert_eq!(
            occurrences, 2,
            "wire output must contain two duplicate keys; got {serialised}"
        );

        // (2) Deserialize on the same bytes must reject with duplicate
        // field. RFC 8259 §4 calls duplicate keys "unpredictable" and
        // the workspace's strict-deserialize stance rejects them.
        let err = serde_json::from_str::<EmailAddress>(&serialised)
            .expect_err("deserialize must reject duplicate-key wire form");
        assert!(
            err.to_string().contains("duplicate field"),
            "error must mention duplicate field; got: {err}"
        );
    }

    /// Stress test for the workspace extras-preservation contract.
    ///
    /// The per-type `*_preserves_vendor_extras` tests above each insert one
    /// scalar/shallow vendor field. This test exercises three round-trip
    /// paths that the per-type tests do not:
    ///
    /// 1. **Multiple vendor fields on the same object** — if serde-flatten
    ///    + Map had a single-key happy-path bias, a two-key bug would slip
    ///    past the per-type tests.
    /// 2. **Nested-object + nested-array extras** — exercises
    ///    `serde_json::Value`'s recursive round-trip beyond one level of
    ///    depth.
    /// 3. **Byte-level string round-trip** — every per-type test uses
    ///    `from_value` / `to_value`, which skips the tokenizer. This test
    ///    goes through `to_string` → `from_str` → `to_string` and asserts
    ///    every vendor field key is preserved on both serialisations.
    ///
    /// `Email` is the canonical extension-types template (workspace
    /// AGENTS.md "Canonical Templates"), so one comprehensive test here
    /// covers every cookie-cut sibling that follows the same pattern.
    ///
    /// Independent oracle: hand-written JSON; assertions compare keys
    /// and shape, not byte equality (object key order is not guaranteed
    /// to be preserved across HashMap iteration, but every key MUST be
    /// retained).
    #[test]
    fn email_extras_multi_field_nested_and_string_roundtrip() {
        let raw_str = r#"{
            "id": "e1",
            "blobId": "b1",
            "threadId": "t1",
            "mailboxIds": {"m1": true},
            "size": 1024,
            "receivedAt": "2024-06-01T00:00:00Z",
            "acmeCorpFoo": "bar",
            "siteHint": "high-priority",
            "acmeCorpNested": {
                "version": 2,
                "signed": [
                    {"by": "alice", "at": "2024-06-01T00:00:00Z"},
                    {"by": "bob", "at": "2024-06-01T00:01:00Z"}
                ],
                "tags": ["x", "y", "z"]
            }
        }"#;

        // Gap 3: byte-level from_str (not from_value) — exercises the
        // streaming tokenizer + serde-flatten's interaction with it.
        let email: Email = serde_json::from_str(raw_str).expect("from_str must accept the wire form");

        // Gap 1: every vendor key must survive deserialize.
        assert!(email.extra.contains_key("acmeCorpFoo"), "scalar vendor field lost");
        assert!(email.extra.contains_key("siteHint"), "second scalar vendor field lost");
        assert!(email.extra.contains_key("acmeCorpNested"), "nested vendor field lost");
        assert_eq!(email.extra.len(), 3, "vendor key count must be exactly three; got {:?}", email.extra.keys().collect::<Vec<_>>());

        // Gap 2: nested object structure must be preserved verbatim.
        let nested = email
            .extra
            .get("acmeCorpNested")
            .expect("acmeCorpNested key present");
        assert_eq!(nested["version"], 2);
        assert_eq!(nested["signed"][0]["by"], "alice");
        assert_eq!(nested["signed"][1]["by"], "bob");
        assert_eq!(nested["tags"][2], "z");

        // Gap 3 (cont.): byte-level to_string → from_str → re-parse must
        // preserve every vendor key. We do not assert byte equality on
        // the serialised string (HashMap iteration order is not stable),
        // but every vendor key MUST be present on the second parse and
        // the nested structure MUST round-trip intact.
        let serialised = serde_json::to_string(&email).expect("to_string must succeed");
        let reparsed: Email = serde_json::from_str(&serialised).expect("from_str must re-accept own output");
        assert_eq!(reparsed.extra.len(), 3);
        assert_eq!(reparsed.extra.get("acmeCorpFoo").and_then(|v| v.as_str()), Some("bar"));
        assert_eq!(reparsed.extra.get("siteHint").and_then(|v| v.as_str()), Some("high-priority"));
        let nested2 = reparsed.extra.get("acmeCorpNested").expect("present");
        assert_eq!(nested2["signed"][0]["by"], "alice");
        assert_eq!(nested2["tags"][2], "z");
    }
}
