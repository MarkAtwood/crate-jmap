use std::collections::HashMap;

use jmap_types::{Date, Id, UTCDate};
use serde::{Deserialize, Serialize};

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
}

/// An Email object (RFC 8621 §4.1).
///
/// Combines metadata (§4.1.1), parsed header convenience properties (§4.1.3),
/// and body fields (§4.1.4).  Fields that are not requested in an `Email/get`
/// call will be absent from the response; all optional fields are represented
/// as `Option` so a partial response can still deserialize.
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
    /// Same `HashMap<Id, bool>` shape as `mailbox_ids` — JMAP wire format requirement.
    /// Values are always `true` in full-object responses (RFC 8621 §4.1.1).
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
}
