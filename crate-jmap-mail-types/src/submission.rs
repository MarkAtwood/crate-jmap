//! [`EmailSubmission`] and related types for RFC 8621 §7.
//!
//! Covers the SMTP envelope ([`Envelope`], [`Address`]), per-recipient delivery
//! status ([`DeliveryStatus`], [`Delivered`], [`Displayed`]), undo tracking
//! ([`UndoStatus`]), and the [`EmailSubmission`] object itself.
//!
//! Also defines [`EmailSubmissionFilterCondition`] for EmailSubmission/query
//! (RFC 8621 §7.3); the `EmailSubmissionFilter` type alias lives in
//! [`crate::query`].

use std::collections::HashMap;

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Serialize};

/// SMTP envelope address with optional MAIL FROM / RCPT TO parameters (RFC 8621 §7).
///
/// Used in both `mailFrom` and the elements of `rcptTo` within an [`Envelope`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    /// The email address (Mailbox as per RFC 5321 Reverse-path / Forward-path).
    pub email: String,
    /// Optional SMTP parameters (mail-parameter or rcpt-parameter per RFC 5321).
    ///
    /// Each key is a parameter name; the value is the parameter value string, or
    /// `None` if the parameter takes no value.  xtext / unitext encodings are
    /// stripped; JSON string encoding applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<HashMap<String, Option<String>>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Address {
    /// Construct an [`Address`] with no SMTP parameters.
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            parameters: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// SMTP envelope for an [`EmailSubmission`] (RFC 8621 §7).
///
/// Carries the return address and recipient list used in the SMTP dialogue.
/// If omitted on creation the server derives it from the Email headers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    /// Return address for the SMTP MAIL FROM command.
    pub mail_from: Address,
    /// Recipient addresses for SMTP RCPT TO commands.
    pub rcpt_to: Vec<Address>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Envelope {
    /// Construct an [`Envelope`] from a return address and recipient list.
    pub fn new(mail_from: Address, rcpt_to: Vec<Address>) -> Self {
        Self {
            mail_from,
            rcpt_to,
            extra: serde_json::Map::new(),
        }
    }
}

/// Delivery status of a message to a recipient (RFC 8621 §7, `delivered` field).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Delivered {
    /// The message is in a local mail queue and the status is not yet known.
    Queued,
    /// The message was successfully delivered to the mail store of the recipient.
    Yes,
    /// Delivery failed; the `smtp_reply` field contains the failure reason.
    No,
    /// The final delivery status is unknown.
    Unknown,
    /// An unrecognised value was received from the server.
    ///
    /// The inner string retains the original value so this variant round-trips correctly.
    Other(String),
}

impl_string_enum!(Delivered, "a delivery status string",
    "queued"  => Queued,
    "yes"     => Yes,
    "no"      => No,
    "unknown" => Unknown,
);

/// Display status of a message to a recipient (RFC 8621 §7, `displayed` field).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Displayed {
    /// The display status is unknown.
    Unknown,
    /// The message has been displayed to the recipient at least once.
    Yes,
    /// An unrecognised value was received from the server.
    ///
    /// The inner string retains the original value so this variant round-trips correctly.
    Other(String),
}

impl_string_enum!(Displayed, "a display status string",
    "unknown" => Unknown,
    "yes"     => Yes,
);

/// Whether an [`EmailSubmission`] may still be canceled (RFC 8621 §7).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UndoStatus {
    /// The message has not yet been relayed; cancellation may be possible.
    Pending,
    /// The message has been relayed to at least one recipient and cannot be recalled.
    Final,
    /// The submission was canceled and will not be delivered to any recipient.
    Canceled,
    /// An unrecognised value was received from the server.
    ///
    /// The inner string retains the original value so this variant round-trips correctly.
    Other(String),
}

impl_string_enum!(UndoStatus, "an undo status string",
    "pending"  => Pending,
    "final"    => Final,
    "canceled" => Canceled,
);

/// Per-recipient delivery status for an [`EmailSubmission`] (RFC 8621 §7).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryStatus {
    /// The SMTP reply string returned when the server last attempted relay,
    /// or from a later DSN (RFC 3464).  Multi-line responses are concatenated
    /// into a single string.
    pub smtp_reply: String,
    /// Whether the message reached the recipient's mail store.
    pub delivered: Delivered,
    /// Whether the message has been displayed to the recipient.
    pub displayed: Displayed,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl DeliveryStatus {
    /// Construct a [`DeliveryStatus`] from its three required fields.
    pub fn new(smtp_reply: impl Into<String>, delivered: Delivered, displayed: Displayed) -> Self {
        Self {
            smtp_reply: smtp_reply.into(),
            delivered,
            displayed,
            extra: serde_json::Map::new(),
        }
    }
}

/// Represents the submission of an Email for delivery (RFC 8621 §7).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailSubmission {
    /// Server-assigned immutable identifier for this submission.
    pub id: Id,
    /// Id of the Identity used to send this submission.
    pub identity_id: Id,
    /// Id of the Email being submitted.
    pub email_id: Id,
    /// Thread id of the submitted Email (server-set).
    pub thread_id: Id,
    /// SMTP envelope; server-derived from Email headers when absent on creation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope: Option<Envelope>,
    /// UTC timestamp when the submission was / will be released for delivery.
    pub send_at: UTCDate,
    /// Whether the submission may still be canceled.
    pub undo_status: UndoStatus,
    /// Per-recipient delivery status, keyed by recipient email address.
    ///
    /// `None` when the server does not support delivery-status tracking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_status: Option<HashMap<String, DeliveryStatus>>,
    /// Blob ids of DSN messages (RFC 3464) received for this submission.
    ///
    /// Always present in serialized output (empty array when no DSN has been received);
    /// RFC 8621 §7 requires these fields in responses.  Do not add `skip_serializing_if`.
    pub dsn_blob_ids: Vec<Id>,
    /// Blob ids of MDN messages (RFC 8098) received for this submission.
    ///
    /// Always present in serialized output; same rationale as `dsn_blob_ids`.
    pub mdn_blob_ids: Vec<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl EmailSubmission {
    /// Construct an [`EmailSubmission`] from its required fields.
    ///
    /// `envelope` and `delivery_status` default to `None`.
    /// `dsn_blob_ids` and `mdn_blob_ids` default to empty.
    pub fn new(
        id: Id,
        identity_id: Id,
        email_id: Id,
        thread_id: Id,
        send_at: UTCDate,
        undo_status: UndoStatus,
    ) -> Self {
        Self {
            id,
            identity_id,
            email_id,
            thread_id,
            envelope: None,
            send_at,
            undo_status,
            delivery_status: None,
            dsn_blob_ids: Vec::new(),
            mdn_blob_ids: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// EmailSubmission/query filter (RFC 8621 §7.3)
// ---------------------------------------------------------------------------

/// Filter condition for EmailSubmission/query (RFC 8621 §7.3).
///
/// All fields are optional.  If zero properties are specified, the condition
/// evaluates to `true` for every submission.
///
/// RFC 8621 §7.3 uses the standard `/query` mechanism (RFC 8620 §5.5), so
/// `EmailSubmissionFilterCondition` can be used inside a
/// `Filter<EmailSubmissionFilterCondition>` to combine conditions with
/// logical operators.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailSubmissionFilterCondition {
    /// The submission's `identityId` must be in this list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_ids: Option<Vec<Id>>,

    /// The submission's `emailId` must be in this list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_ids: Option<Vec<Id>>,

    /// The submission's `threadId` must be in this list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ids: Option<Vec<Id>>,

    /// The submission's `undoStatus` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo_status: Option<UndoStatus>,

    /// The `sendAt` of the submission must be before this date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<UTCDate>,

    /// The `sendAt` of the submission must be on or after this date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<UTCDate>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Extras-preservation policy tests (JMAP-lbdy.2) ───────────────────

    /// `Address.extra` captures vendor fields and preserves them.
    #[test]
    fn address_preserves_vendor_extras() {
        let raw = json!({
            "email": "alice@example.com",
            "acmeCorpRouting": "us-east"
        });
        let addr: Address = serde_json::from_value(raw).unwrap();
        assert_eq!(
            addr.extra.get("acmeCorpRouting").and_then(|v| v.as_str()),
            Some("us-east")
        );
        let back = serde_json::to_value(&addr).unwrap();
        assert_eq!(back["acmeCorpRouting"], "us-east");
    }

    /// `Envelope.extra` captures vendor fields and preserves them.
    #[test]
    fn envelope_preserves_vendor_extras() {
        let raw = json!({
            "mailFrom": {"email": "a@b"},
            "rcptTo": [{"email": "c@d"}],
            "acmeCorpSubmissionPath": "smarthost-3"
        });
        let env: Envelope = serde_json::from_value(raw).unwrap();
        assert_eq!(
            env.extra
                .get("acmeCorpSubmissionPath")
                .and_then(|v| v.as_str()),
            Some("smarthost-3")
        );
        let back = serde_json::to_value(&env).unwrap();
        assert_eq!(back["acmeCorpSubmissionPath"], "smarthost-3");
    }

    /// `DeliveryStatus.extra` captures vendor fields and preserves them.
    #[test]
    fn delivery_status_preserves_vendor_extras() {
        let raw = json!({
            "smtpReply": "250 OK",
            "delivered": "yes",
            "displayed": "unknown",
            "acmeCorpDeliveryTimeMs": 120
        });
        let st: DeliveryStatus = serde_json::from_value(raw).unwrap();
        assert_eq!(
            st.extra
                .get("acmeCorpDeliveryTimeMs")
                .and_then(|v| v.as_u64()),
            Some(120)
        );
        let back = serde_json::to_value(&st).unwrap();
        assert_eq!(back["acmeCorpDeliveryTimeMs"], 120);
    }

    /// `EmailSubmission.extra` captures vendor fields and preserves them.
    #[test]
    fn email_submission_preserves_vendor_extras() {
        let raw = json!({
            "id": "es1",
            "identityId": "i1",
            "emailId": "e1",
            "threadId": "t1",
            "sendAt": "2024-06-01T00:00:00Z",
            "undoStatus": "pending",
            "dsnBlobIds": [],
            "mdnBlobIds": [],
            "acmeCorpSubmissionTag": "campaign-42"
        });
        let sub: EmailSubmission = serde_json::from_value(raw).unwrap();
        assert_eq!(
            sub.extra
                .get("acmeCorpSubmissionTag")
                .and_then(|v| v.as_str()),
            Some("campaign-42")
        );
        let back = serde_json::to_value(&sub).unwrap();
        assert_eq!(back["acmeCorpSubmissionTag"], "campaign-42");
    }
}
