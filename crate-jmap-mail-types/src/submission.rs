use std::collections::HashMap;
use std::fmt;

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

// Shared deserialize helper: deserialize a string for an enum with an Other(String) catch-all.
macro_rules! impl_string_enum_serde {
    ($ty:ident, $expecting:literal, $( $s:literal => $variant:ident ),+ $(,)?) => {
        impl Serialize for $ty {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(match self {
                    $( $ty::$variant => $s, )+
                    $ty::Other(v) => v.as_str(),
                })
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct Visitor;
                impl serde::de::Visitor<'_> for Visitor {
                    type Value = $ty;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(f, $expecting)
                    }
                    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<$ty, E> {
                        Ok(match v {
                            $( $s => $ty::$variant, )+
                            _ => $ty::Other(v.to_owned()),
                        })
                    }
                }
                d.deserialize_str(Visitor)
            }
        }
    };
}

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
}

impl Address {
    /// Construct an [`Address`] with no SMTP parameters.
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            parameters: None,
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
}

impl Envelope {
    /// Construct an [`Envelope`] from a return address and recipient list.
    pub fn new(mail_from: Address, rcpt_to: Vec<Address>) -> Self {
        Self { mail_from, rcpt_to }
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

impl_string_enum_serde!(Delivered, "a delivery status string",
    "queued" => Queued,
    "yes"    => Yes,
    "no"     => No,
    "unknown" => Unknown,
);

impl fmt::Display for Delivered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Delivered::Queued => "queued",
            Delivered::Yes => "yes",
            Delivered::No => "no",
            Delivered::Unknown => "unknown",
            Delivered::Other(v) => v.as_str(),
        })
    }
}

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

impl_string_enum_serde!(Displayed, "a display status string",
    "unknown" => Unknown,
    "yes"     => Yes,
);

impl fmt::Display for Displayed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Displayed::Unknown => "unknown",
            Displayed::Yes => "yes",
            Displayed::Other(v) => v.as_str(),
        })
    }
}

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

impl_string_enum_serde!(UndoStatus, "an undo status string",
    "pending"  => Pending,
    "final"    => Final,
    "canceled" => Canceled,
);

impl fmt::Display for UndoStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UndoStatus::Pending => "pending",
            UndoStatus::Final => "final",
            UndoStatus::Canceled => "canceled",
            UndoStatus::Other(v) => v.as_str(),
        })
    }
}

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
}

impl DeliveryStatus {
    /// Construct a [`DeliveryStatus`] from its three required fields.
    pub fn new(smtp_reply: impl Into<String>, delivered: Delivered, displayed: Displayed) -> Self {
        Self {
            smtp_reply: smtp_reply.into(),
            delivered,
            displayed,
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
