use std::collections::HashMap;

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// A parsed email address (RFC 8621 §4.1.2.3).
///
/// Represents one address entry from an RFC 5322 address-list.
/// The `email` field contains the "addr-spec"; `name` contains the
/// decoded display-name, or `null` if absent.
///
/// In RFC 5322 terminology this is a "mailbox" (an addr-spec with optional
/// display-name), distinct from the JMAP [`Mailbox`](crate::Mailbox) folder type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct EmailAddress {
    /// The decoded display-name of the mailbox, or `null` if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The addr-spec of the mailbox (e.g. `"user@example.com"`).
    pub email: String,
}

/// A named group of email addresses (RFC 8621 §4.1.2.4).
///
/// Preserves RFC 5322 group structure. Consecutive mailboxes not part of
/// a named group are collected under an `EmailAddressGroup` with `name: null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct EmailAddressGroup {
    /// The decoded display-name of the group, or `null` for ungrouped mailboxes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The mailboxes that belong to this group.
    pub addresses: Vec<EmailAddress>,
}

/// A single RFC 5322 header field (RFC 8621 §4.1.3).
///
/// The `name` retains original capitalisation; `value` is the raw field value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct EmailHeader {
    /// The header field name (e.g. `"Content-Type"`), case-preserved.
    pub name: String,
    /// The header field value in Raw form.
    pub value: String,
}

/// The decoded text content of one body part (RFC 8621 §4.1.4).
///
/// Returned inside the `bodyValues` map of an Email object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct EmailBodyValue {
    /// The decoded text content of the part.
    pub value: String,
    /// `true` if charset decoding or content-transfer-encoding decoding
    /// encountered errors (RFC 8621 §4.1.4).
    #[serde(default)]
    pub is_encoding_problem: bool,
    /// `true` if `value` was truncated due to a `maxBodyValueBytes` limit
    /// (RFC 8621 §4.1.4).
    #[serde(default)]
    pub is_truncated: bool,
}

/// One MIME body part within an Email (RFC 8621 §4.1.4).
///
/// The `sub_parts` field is recursive: multipart bodies nest further
/// `EmailBodyPart` values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
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
}

/// An Email object (RFC 8621 §4.1).
///
/// Combines metadata (§4.1.1), parsed header convenience properties (§4.1.3),
/// and body fields (§4.1.4).  Fields that are not requested in an `Email/get`
/// call will be absent from the response; all optional fields are represented
/// as `Option` so a partial response can still deserialize.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Email {
    // --- Metadata (§4.1.1) ---
    /// The JMAP object id of this Email.
    pub id: Id,
    /// Blob id of the raw RFC 5322 message octets.
    pub blob_id: Id,
    /// Id of the Thread this Email belongs to.
    pub thread_id: Id,
    /// Set of Mailbox ids this Email belongs to (value is always true).
    pub mailbox_ids: HashMap<Id, bool>,
    /// Keywords applied to this Email (value is always true).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub keywords: HashMap<String, bool>,
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
    /// The RFC specifies this as type `Date` (any RFC 3339 timezone offset),
    /// not `UTCDate`. Email Date headers commonly use non-UTC offsets such as
    /// `"+10:00"`. This field uses `UTCDate` as a placeholder because `jmap-types`
    /// does not yet provide a timezone-aware `Date` type; the underlying `String`
    /// storage accepts any RFC 3339 value without validation.
    ///
    /// Tracked: replace with a proper `Date` type when available in jmap-types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<UTCDate>,

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
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: RFC 8621 §4.1.2.3 — EmailAddress with name and email.
    #[test]
    fn email_address_with_name_round_trips() {
        let addr = EmailAddress {
            name: Some("John Doe".to_owned()),
            email: "john@example.com".to_owned(),
        };
        let json = serde_json::to_string(&addr).expect("serialize");
        let back: EmailAddress = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(addr, back);
        assert!(json.contains("\"name\":\"John Doe\""));
        assert!(json.contains("\"email\":\"john@example.com\""));
    }

    // Oracle: RFC 8621 §4.1.2.3 — name is omitted from JSON when null.
    #[test]
    fn email_address_null_name_omitted() {
        let addr = EmailAddress {
            name: None,
            email: "anon@example.com".to_owned(),
        };
        let json = serde_json::to_string(&addr).expect("serialize");
        assert!(!json.contains("name"), "null name must not appear in JSON");
        assert!(json.contains("\"email\":\"anon@example.com\""));
    }

    // Oracle: RFC 8621 §4.1.2.3 — deserialize with explicit null name.
    #[test]
    fn email_address_deserializes_explicit_null_name() {
        let json = r#"{"name":null,"email":"x@example.com"}"#;
        let addr: EmailAddress = serde_json::from_str(json).expect("deserialize");
        assert_eq!(addr.name, None);
        assert_eq!(addr.email, "x@example.com");
    }

    // Oracle: RFC 8621 §4.1.2.4 — EmailAddressGroup with named group.
    #[test]
    fn email_address_group_round_trips() {
        let group = EmailAddressGroup {
            name: Some("Friends".to_owned()),
            addresses: vec![
                EmailAddress {
                    name: None,
                    email: "a@example.com".to_owned(),
                },
                EmailAddress {
                    name: Some("Bob".to_owned()),
                    email: "b@example.com".to_owned(),
                },
            ],
        };
        let json = serde_json::to_string(&group).expect("serialize");
        let back: EmailAddressGroup = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(group, back);
    }

    // Oracle: RFC 8621 §4.1.2.4 — null group name is omitted.
    #[test]
    fn email_address_group_null_name_omitted() {
        let group = EmailAddressGroup {
            name: None,
            addresses: vec![],
        };
        let json = serde_json::to_string(&group).expect("serialize");
        assert!(!json.contains("name"), "null name must not appear in JSON");
    }

    // Oracle: RFC 8621 §4.1.3 — EmailHeader serialises name and value.
    #[test]
    fn email_header_round_trips() {
        let hdr = EmailHeader {
            name: "Content-Type".to_owned(),
            value: " text/plain; charset=utf-8".to_owned(),
        };
        let json = serde_json::to_string(&hdr).expect("serialize");
        let back: EmailHeader = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(hdr, back);
        assert!(json.contains("\"name\":\"Content-Type\""));
        assert!(json.contains("\"value\":\" text/plain; charset=utf-8\""));
    }

    // Oracle: RFC 8621 §4.1.4 example — isEncodingProblem: false, isTruncated: true.
    #[test]
    fn email_body_value_round_trips() {
        let bv = EmailBodyValue {
            value: "<html><body><p>Hello ...".to_owned(),
            is_encoding_problem: false,
            is_truncated: true,
        };
        let json = serde_json::to_string(&bv).expect("serialize");
        let back: EmailBodyValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(bv, back);
        assert!(json.contains("\"isEncodingProblem\":false"));
        assert!(json.contains("\"isTruncated\":true"));
    }

    // Oracle: RFC 8621 §4.1.4 — default values are false when absent from JSON.
    #[test]
    fn email_body_value_defaults_when_fields_absent() {
        let json = r#"{"value":"hello"}"#;
        let bv: EmailBodyValue = serde_json::from_str(json).expect("deserialize");
        assert!(!bv.is_encoding_problem);
        assert!(!bv.is_truncated);
    }

    // Oracle: RFC 8621 §4.1.4 example from §4.5.2 appendix — partId "1" entry.
    #[test]
    fn email_body_value_from_rfc_example() {
        let json =
            r#"{"value":"<html><body><p>Hello ...","isEncodingProblem":false,"isTruncated":true}"#;
        let bv: EmailBodyValue = serde_json::from_str(json).expect("deserialize");
        assert_eq!(bv.value, "<html><body><p>Hello ...");
        assert!(!bv.is_encoding_problem);
        assert!(bv.is_truncated);
    }
}
