//! Capability types for the JMAP for Mail extension (RFC 8621).
//!
//! RFC 8621 §1.3 defines three capability URIs.  Each has both a
//! session-level capability (value in the JMAP Session `capabilities` map)
//! and an account-level capability (value in the account's
//! `accountCapabilities` map).  Session-level values are empty objects for
//! all three URIs; only `urn:ietf:params:jmap:mail` and
//! `urn:ietf:params:jmap:submission` carry non-empty account-level
//! capabilities.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Capability URI for core JMAP for Mail support (RFC 8621 §1.3.1).
pub const JMAP_MAIL_URI: &str = "urn:ietf:params:jmap:mail";

/// Capability URI for the JMAP Email Submission extension (RFC 8621 §1.3.2).
pub const JMAP_SUBMISSION_URI: &str = "urn:ietf:params:jmap:submission";

/// Capability URI for the JMAP Vacation Response extension (RFC 8621 §1.3.3).
pub const JMAP_VACATIONRESPONSE_URI: &str = "urn:ietf:params:jmap:vacationresponse";

/// Session-level Mail capability (RFC 8621 §1.3.1).
///
/// The value of `capabilities["urn:ietf:params:jmap:mail"]` in the JMAP
/// Session object.  RFC 8621 §1.3.1 mandates that this is an empty object
/// `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MailCapability {}

/// Account-level Mail capability (RFC 8621 §1.3.1).
///
/// The value of `accountCapabilities["urn:ietf:params:jmap:mail"]` for a
/// given account.  Describes server capabilities and account-level
/// permissions for the Mail extension.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAccountCapability {
    /// Maximum number of Mailboxes that may be assigned to a single Email,
    /// or `null` for no limit (RFC 8621 §1.3.1).  MUST be >= 1 when set.
    pub max_mailboxes_per_email: Option<u64>,

    /// Maximum depth of the Mailbox hierarchy (one more than the maximum
    /// number of ancestors a Mailbox may have), or `null` for no limit
    /// (RFC 8621 §1.3.1).
    pub max_mailbox_depth: Option<u64>,

    /// Maximum length, in UTF-8 octets, allowed for a Mailbox name
    /// (RFC 8621 §1.3.1).  MUST be at least 100.
    pub max_size_mailbox_name: u64,

    /// Maximum total size, in octets, of attachments allowed for a single
    /// Email object (RFC 8621 §1.3.1).  This is the unencoded attachment
    /// total; servers may enforce a separate limit on encoded message size.
    pub max_size_attachments_per_email: u64,

    /// List of values the server supports for the `property` field of the
    /// Comparator object in an `Email/query` sort (RFC 8621 §1.3.1 / §4.4.2).
    /// May include vendor-specific extensions; clients MUST ignore unknown
    /// entries.
    pub email_query_sort_options: Vec<String>,

    /// If `true`, the user may create a Mailbox in this account with a
    /// `null` parentId (RFC 8621 §1.3.1).  Permission to create a child of
    /// an existing Mailbox is governed by `Mailbox.myRights` instead.
    pub may_create_top_level_mailbox: bool,
}

/// Session-level Submission capability (RFC 8621 §1.3.2).
///
/// The value of `capabilities["urn:ietf:params:jmap:submission"]` in the
/// JMAP Session object.  RFC 8621 §1.3.2 mandates that this is an empty
/// object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SubmissionCapability {}

/// Account-level Submission capability (RFC 8621 §1.3.2).
///
/// The value of `accountCapabilities["urn:ietf:params:jmap:submission"]`
/// for a given account.  Describes server capabilities and account-level
/// permissions for the Email Submission extension.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionAccountCapability {
    /// Maximum delay, in seconds, that the server supports for delayed
    /// send (RFC 8621 §1.3.2 / §7).  Zero if the server does not support
    /// delayed send.
    pub max_delayed_send: u64,

    /// SMTP submission extensions supported by the server.  Each key is
    /// an `ehlo-name` and the value is a list of `ehlo-args`
    /// (RFC 8621 §1.3.2).
    pub submission_extensions: HashMap<String, Vec<String>>,
}

/// Session-level VacationResponse capability (RFC 8621 §1.3.3).
///
/// The value of `capabilities["urn:ietf:params:jmap:vacationresponse"]`
/// in the JMAP Session object.  RFC 8621 §1.3.3 mandates that this is an
/// empty object `{}` at both session level and account level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VacationResponseCapability {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: hand-written JSON from RFC 8621 §1.3.1 field definitions.
    #[test]
    fn mail_account_capability_round_trip() {
        let json = r#"{"maxMailboxesPerEmail":null,"maxMailboxDepth":10,"maxSizeMailboxName":100,"maxSizeAttachmentsPerEmail":50000000,"emailQuerySortOptions":["receivedAt","size"],"mayCreateTopLevelMailbox":true}"#;
        let cap: MailAccountCapability = serde_json::from_str(json).expect("must parse");
        assert!(cap.max_mailboxes_per_email.is_none());
        assert_eq!(cap.max_mailbox_depth, Some(10));
        assert_eq!(cap.max_size_mailbox_name, 100);
        assert_eq!(cap.max_size_attachments_per_email, 50_000_000);
        assert_eq!(
            cap.email_query_sort_options,
            vec!["receivedAt".to_owned(), "size".to_owned()]
        );
        assert!(cap.may_create_top_level_mailbox);
        let back = serde_json::to_string(&cap).expect("serialize");
        assert_eq!(back, json);
    }

    /// Oracle: RFC 8621 §1.3.1 requires the session-level value to be `{}`.
    #[test]
    fn mail_capability_session_level_is_empty_object() {
        let cap = MailCapability::default();
        let json = serde_json::to_string(&cap).expect("serialize");
        assert_eq!(json, "{}");
        let back: MailCapability = serde_json::from_str("{}").expect("must parse");
        assert_eq!(back, cap);
    }

    /// Oracle: hand-written JSON from RFC 8621 §1.3.2 field definitions.
    #[test]
    fn submission_account_capability_round_trip() {
        let json = r#"{"maxDelayedSend":86400,"submissionExtensions":{"FUTURERELEASE":["86400","2024-01-01T00:00:00Z"]}}"#;
        let cap: SubmissionAccountCapability = serde_json::from_str(json).expect("must parse");
        assert_eq!(cap.max_delayed_send, 86_400);
        assert_eq!(
            cap.submission_extensions.get("FUTURERELEASE"),
            Some(&vec!["86400".to_owned(), "2024-01-01T00:00:00Z".to_owned()])
        );
        let back = serde_json::to_string(&cap).expect("serialize");
        assert_eq!(back, json);
    }

    /// Oracle: RFC 8621 §1.3.2 requires the session-level value to be `{}`.
    #[test]
    fn submission_capability_session_level_is_empty_object() {
        let cap = SubmissionCapability::default();
        let json = serde_json::to_string(&cap).expect("serialize");
        assert_eq!(json, "{}");
    }

    /// Oracle: RFC 8621 §1.3.3 requires the value to be `{}` at both
    /// session-level and account-level (the same struct serves both).
    #[test]
    fn vacation_response_capability_is_empty_object() {
        let cap = VacationResponseCapability::default();
        let json = serde_json::to_string(&cap).expect("serialize");
        assert_eq!(json, "{}");
        let back: VacationResponseCapability = serde_json::from_str("{}").expect("must parse");
        assert_eq!(back, cap);
    }

    /// Oracle: capability URI constants match the RFC 8621 §1.3 IANA
    /// registrations verbatim.
    #[test]
    fn capability_uri_constants_match_rfc8621() {
        assert_eq!(JMAP_MAIL_URI, "urn:ietf:params:jmap:mail");
        assert_eq!(JMAP_SUBMISSION_URI, "urn:ietf:params:jmap:submission");
        assert_eq!(
            JMAP_VACATIONRESPONSE_URI,
            "urn:ietf:params:jmap:vacationresponse"
        );
    }
}
