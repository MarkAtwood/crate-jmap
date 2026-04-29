use std::collections::HashMap;
use std::fmt;

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// SMTP envelope address with optional MAIL FROM / RCPT TO parameters (RFC 8621 §7).
///
/// Used in both `mailFrom` and the elements of `rcptTo` within an [`Envelope`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
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
}

/// SMTP envelope for an [`EmailSubmission`] (RFC 8621 §7).
///
/// Carries the return address and recipient list used in the SMTP dialogue.
/// If omitted on creation the server derives it from the Email headers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Envelope {
    /// Return address for the SMTP MAIL FROM command.
    pub mail_from: Address,
    /// Recipient addresses for SMTP RCPT TO commands.
    pub rcpt_to: Vec<Address>,
}

/// Delivery status of a message to a recipient (RFC 8621 §7, `delivered` field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
    #[serde(other)]
    Other,
}

impl fmt::Display for Delivered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Delivered::Queued => "queued",
            Delivered::Yes => "yes",
            Delivered::No => "no",
            Delivered::Unknown => "unknown",
            Delivered::Other => "other",
        })
    }
}

/// Display status of a message to a recipient (RFC 8621 §7, `displayed` field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Displayed {
    /// The display status is unknown.
    Unknown,
    /// The message has been displayed to the recipient at least once.
    Yes,
    /// An unrecognised value was received from the server.
    #[serde(other)]
    Other,
}

impl fmt::Display for Displayed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Displayed::Unknown => "unknown",
            Displayed::Yes => "yes",
            Displayed::Other => "other",
        })
    }
}

/// Whether an [`EmailSubmission`] may still be canceled (RFC 8621 §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum UndoStatus {
    /// The message has not yet been relayed; cancellation may be possible.
    Pending,
    /// The message has been relayed to at least one recipient and cannot be recalled.
    Final,
    /// The submission was canceled and will not be delivered to any recipient.
    Canceled,
    /// An unrecognised value was received from the server.
    #[serde(other)]
    Other,
}

impl fmt::Display for UndoStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UndoStatus::Pending => "pending",
            UndoStatus::Final => "final",
            UndoStatus::Canceled => "canceled",
            UndoStatus::Other => "other",
        })
    }
}

/// Per-recipient delivery status for an [`EmailSubmission`] (RFC 8621 §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DeliveryStatus {
    /// The SMTP reply string returned when the server last attempted relay,
    /// or from a later DSN (RFC 3464).  Multi-line responses are concatenated
    /// into a single string.
    pub smtp_reply: String,
    /// Whether the message reached the recipient's mail store.
    pub delivered: Delivered,
    /// Whether the message has been displayed to the recipient.
    pub displayed: Displayed,
}

/// Represents the submission of an Email for delivery (RFC 8621 §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
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
    pub dsn_blob_ids: Vec<Id>,
    /// Blob ids of MDN messages (RFC 8098) received for this submission.
    pub mdn_blob_ids: Vec<Id>,
}
