// Email/query filter types (RFC 8621 §4.4).
//
// Provides [`EmailFilterCondition`] — the mail-specific condition object for
// Email/query — and the [`EmailFilter`] type alias for convenience.
//
// The generic [`Filter`], [`FilterOperator`], and [`Operator`] types used here
// are defined in `jmap-types::query` (RFC 8620 §5.5).

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

pub use jmap_types::query::{Filter, FilterOperator, Operator};

/// Concrete filter type for Email/query (RFC 8621 §4.4).
pub type EmailFilter = Filter<EmailFilterCondition>;

/// Concrete filter type for EmailSubmission/query (RFC 8621 §7.3).
pub type EmailSubmissionFilter = Filter<crate::submission::EmailSubmissionFilterCondition>;

/// Filter condition for Email objects (RFC 8621 §4.4.1).
///
/// All fields are optional.  If zero properties are specified, the condition
/// evaluates to `true` for every Email (RFC 8621 §4.4.1).  When multiple
/// properties are specified, ALL must apply (equivalent to splitting into
/// one-property conditions under AND).
///
/// Do not add `#[serde(deny_unknown_fields)]` — it breaks `#[serde(untagged)]`
/// deserialization when `EmailFilterCondition` is used inside `Filter<T>`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailFilterCondition {
    /// A Mailbox id; the Email must be in this Mailbox.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_mailbox: Option<Id>,

    /// A list of Mailbox ids; the Email must be in at least one Mailbox not in
    /// this list (used to exclude trash/spam from results).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_mailbox_other_than: Option<Vec<Id>>,

    /// The `receivedAt` of the Email must be before this date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<UTCDate>,

    /// The `receivedAt` of the Email must be on or after this date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<UTCDate>,

    /// The `size` of the Email must be >= this value (bytes).
    ///
    /// RFC 8620 §1.3 defines `UnsignedInt` as limited to the range
    /// [0, 2^53-1].  Values above that threshold may not round-trip correctly
    /// through JSON parsers that use IEEE 754 doubles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size: Option<u64>,

    /// The `size` of the Email must be < this value (bytes).
    ///
    /// Same 2^53-1 constraint as `min_size` (RFC 8620 §1.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,

    /// All Emails in the same Thread must have this keyword.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_in_thread_have_keyword: Option<String>,

    /// At least one Email in the same Thread must have this keyword.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub some_in_thread_have_keyword: Option<String>,

    /// No Email in the same Thread may have this keyword.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub none_in_thread_have_keyword: Option<String>,

    /// This Email must have this keyword.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_keyword: Option<String>,

    /// This Email must not have this keyword.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_keyword: Option<String>,

    /// The `hasAttachment` property of the Email must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_attachment: Option<bool>,

    /// Matches text across From, To, Cc, Bcc, Subject, and body parts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Matches text in the From header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,

    /// Matches text in the To header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,

    /// Matches text in the Cc header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc: Option<String>,

    /// Matches text in the Bcc header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bcc: Option<String>,

    /// Matches text in the Subject header field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    /// Matches text in a body part of the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    /// Arbitrary header field match.  RFC 8621 §4.4.1 requires exactly 1 or 2
    /// elements: the first is the header field name; the second (optional) is
    /// the value to match.  Supplying 0 or 3+ elements is a protocol error and
    /// is rejected at deserialization time.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_header"
    )]
    pub header: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// EmailComparator (RFC 8621 §4.4.2)
// ---------------------------------------------------------------------------

/// Sort comparator for Email/query (RFC 8621 §4.4.2).
///
/// The `property` field names the attribute to sort by; see RFC 8621 §4.4.2
/// for the full list ("receivedAt", "size", "from", "to", "subject",
/// "sentAt", "hasKeyword", "allInThreadHaveKeyword", "someInThreadHaveKeyword").
///
/// When `property` is one of the keyword-based sort properties, `keyword`
/// MUST also be set (RFC 8621 §4.4.2).  `is_ascending` defaults to `true`
/// per RFC 8620 §5.5.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailComparator {
    /// The property name to sort by.
    pub property: String,

    /// If `true`, sort ascending; if `false`, sort descending.
    /// Defaults to `true` per RFC 8620 §5.5.
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub is_ascending: bool,

    /// Collation algorithm (e.g. `"i;ascii-casemap"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,

    /// Required when `property` is `"hasKeyword"`, `"allInThreadHaveKeyword"`,
    /// or `"someInThreadHaveKeyword"` (RFC 8621 §4.4.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,
}

fn bool_true() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

// ---------------------------------------------------------------------------
// Deserializer helpers
// ---------------------------------------------------------------------------

/// Deserialize `header` and enforce the 1–2 element constraint from RFC 8621 §4.4.1.
fn deserialize_header<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<Vec<String>> = Option::deserialize(deserializer)?;
    if let Some(ref h) = v {
        if h.is_empty() || h.len() > 2 {
            return Err(serde::de::Error::custom(format!(
                "header must have 1 or 2 elements (RFC 8621 §4.4.1), got {}",
                h.len()
            )));
        }
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: RFC 8621 §4.4 example — filter by Mailbox id.
    /// Wire format from RFC 8621 §4.4 (line 3240 area of the RFC text).
    #[test]
    fn filter_condition_in_mailbox() {
        let json = r#"{"inMailbox":"fb666a55"}"#;
        let f: EmailFilter = serde_json::from_str(json).expect("must parse");
        match &f {
            Filter::Condition(c) => {
                assert_eq!(
                    c.in_mailbox.as_ref().map(|id| id.as_ref()),
                    Some("fb666a55")
                );
            }
            other => panic!("expected Condition, got {other:?}"),
        }
        let back = serde_json::to_string(&f).expect("serialize");
        assert_eq!(back, json);
    }

    /// Oracle: RFC 8620 §5.5 example — OR of two keyword conditions applied to Email.
    /// Adapted from the Todo/query example in RFC 8620 §5.5.
    #[test]
    fn filter_operator_or_two_keywords() {
        let json =
            r#"{"operator":"OR","conditions":[{"hasKeyword":"music"},{"hasKeyword":"video"}]}"#;
        let f: EmailFilter = serde_json::from_str(json).expect("must parse");
        match &f {
            Filter::Operator(op) => {
                assert_eq!(op.operator, Operator::Or);
                assert_eq!(op.conditions.len(), 2);
                match &op.conditions[0] {
                    Filter::Condition(c) => assert_eq!(c.has_keyword.as_deref(), Some("music")),
                    other => panic!("expected Condition, got {other:?}"),
                }
                match &op.conditions[1] {
                    Filter::Condition(c) => assert_eq!(c.has_keyword.as_deref(), Some("video")),
                    other => panic!("expected Condition, got {other:?}"),
                }
            }
            other => panic!("expected Operator, got {other:?}"),
        }
        let back = serde_json::to_string(&f).expect("serialize");
        assert_eq!(back, json);
    }

    /// Oracle: nested AND(OR(...)) structure roundtrips.
    #[test]
    fn nested_and_or_roundtrip() {
        let filter = EmailFilter::Operator(FilterOperator {
            operator: Operator::And,
            conditions: vec![
                EmailFilter::Condition(EmailFilterCondition {
                    in_mailbox: Some(Id::from("inbox-id")),
                    ..Default::default()
                }),
                EmailFilter::Operator(FilterOperator {
                    operator: Operator::Or,
                    conditions: vec![
                        EmailFilter::Condition(EmailFilterCondition {
                            has_keyword: Some("$flagged".to_owned()),
                            ..Default::default()
                        }),
                        EmailFilter::Condition(EmailFilterCondition {
                            has_keyword: Some("$answered".to_owned()),
                            ..Default::default()
                        }),
                    ],
                }),
            ],
        });
        let json = serde_json::to_string(&filter).expect("serialize");
        let back: EmailFilter = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(filter, back);
    }

    /// Oracle: empty EmailFilterCondition serializes to `{}` and omits all fields.
    #[test]
    fn empty_condition_serializes_to_empty_object() {
        let c = EmailFilterCondition::default();
        let json = serde_json::to_string(&c).expect("serialize");
        assert_eq!(json, "{}");
    }

    /// Oracle: all scalar fields roundtrip correctly.
    #[test]
    fn all_scalar_fields_roundtrip() {
        let c = EmailFilterCondition {
            in_mailbox: Some(Id::from("mb1")),
            min_size: Some(1024),
            max_size: Some(65536),
            all_in_thread_have_keyword: Some("$seen".to_owned()),
            some_in_thread_have_keyword: Some("$flagged".to_owned()),
            none_in_thread_have_keyword: Some("$draft".to_owned()),
            has_keyword: Some("$answered".to_owned()),
            not_keyword: Some("$junk".to_owned()),
            has_attachment: Some(true),
            text: Some("hello".to_owned()),
            from: Some("alice@example.com".to_owned()),
            to: Some("bob@example.com".to_owned()),
            cc: Some("carol@example.com".to_owned()),
            bcc: Some("dave@example.com".to_owned()),
            subject: Some("Meeting".to_owned()),
            body: Some("agenda".to_owned()),
            ..Default::default()
        };
        let json = serde_json::to_string(&c).expect("serialize");
        let back: EmailFilterCondition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(c, back);
    }

    /// Oracle: `header` with one element (field name only) is accepted.
    #[test]
    fn header_one_element_accepted() {
        let json = r#"{"header":["X-Spam-Status"]}"#;
        let c: EmailFilterCondition = serde_json::from_str(json).expect("must parse");
        let h = c.header.as_ref().expect("header must be present");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0], "X-Spam-Status");
    }

    /// Oracle: `header` with two elements (field name + value) is accepted.
    #[test]
    fn header_two_elements_accepted() {
        let json = r#"{"header":["X-Spam-Status","Yes"]}"#;
        let c: EmailFilterCondition = serde_json::from_str(json).expect("must parse");
        let h = c.header.as_ref().expect("header must be present");
        assert_eq!(h.len(), 2);
        assert_eq!(h[0], "X-Spam-Status");
        assert_eq!(h[1], "Yes");
    }

    /// Oracle: RFC 8621 §4.4.1 — `header` with zero elements is a protocol error.
    #[test]
    fn header_zero_elements_rejected() {
        let json = r#"{"header":[]}"#;
        let err = serde_json::from_str::<EmailFilterCondition>(json)
            .expect_err("0-element header must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("header must have 1 or 2 elements"),
            "unexpected error: {msg}"
        );
    }

    /// Oracle: RFC 8621 §4.4.1 — `header` with three elements is a protocol error.
    #[test]
    fn header_three_elements_rejected() {
        let json = r#"{"header":["X-Foo","bar","extra"]}"#;
        let err = serde_json::from_str::<EmailFilterCondition>(json)
            .expect_err("3-element header must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("header must have 1 or 2 elements"),
            "unexpected error: {msg}"
        );
    }

    /// Oracle: `before` and `after` fields use UTCDate wire format (RFC 3339 UTC).
    #[test]
    fn date_fields_roundtrip() {
        let json = r#"{"before":"2024-01-15T12:00:00Z","after":"2024-01-01T00:00:00Z"}"#;
        let c: EmailFilterCondition = serde_json::from_str(json).expect("must parse");
        let back = serde_json::to_string(&c).expect("serialize");
        assert_eq!(back, json);
    }

    /// Oracle: `inMailboxOtherThan` is a list of Ids.
    #[test]
    fn in_mailbox_other_than_roundtrip() {
        let json = r#"{"inMailboxOtherThan":["trash-id","spam-id"]}"#;
        let c: EmailFilterCondition = serde_json::from_str(json).expect("must parse");
        let ids: Vec<&str> = c
            .in_mailbox_other_than
            .as_ref()
            .unwrap()
            .iter()
            .map(|id| id.as_ref())
            .collect();
        assert_eq!(ids, ["trash-id", "spam-id"]);
        let back = serde_json::to_string(&c).expect("serialize");
        assert_eq!(back, json);
    }

    // ---------------------------------------------------------------------------
    // EmailComparator tests
    // ---------------------------------------------------------------------------

    /// Oracle: RFC 8621 §4.4.2 example — three-comparator sort array.
    /// Wire format taken verbatim from the RFC 8621 §4.4.2 example.
    #[test]
    fn comparator_example_from_rfc() {
        // Three comparators from RFC 8621 §4.4.2.
        // First: keyword sort (isAscending=false, keyword present).
        let json0 =
            r#"{"property":"someInThreadHaveKeyword","keyword":"$flagged","isAscending":false}"#;
        let c0: EmailComparator = serde_json::from_str(json0).expect("must parse");
        assert_eq!(c0.property, "someInThreadHaveKeyword");
        assert_eq!(c0.keyword.as_deref(), Some("$flagged"));
        assert!(!c0.is_ascending);

        // Second: subject sort with collation, isAscending defaults to true (omitted).
        let json1 = r#"{"property":"subject","collation":"i;ascii-casemap"}"#;
        let c1: EmailComparator = serde_json::from_str(json1).expect("must parse");
        assert_eq!(c1.property, "subject");
        assert_eq!(c1.collation.as_deref(), Some("i;ascii-casemap"));
        assert!(c1.is_ascending);
        // isAscending is true (default) and must be omitted on serialization.
        let back1 = serde_json::to_string(&c1).expect("serialize");
        assert_eq!(back1, json1);

        // Third: receivedAt descending.
        let json2 = r#"{"property":"receivedAt","isAscending":false}"#;
        let c2: EmailComparator = serde_json::from_str(json2).expect("must parse");
        assert_eq!(c2.property, "receivedAt");
        assert!(!c2.is_ascending);
        let back2 = serde_json::to_string(&c2).expect("serialize");
        assert_eq!(back2, json2);
    }

    /// Oracle: isAscending defaults to true when absent; omitted from serialization
    /// when true (RFC 8620 §5.5 default).
    #[test]
    fn comparator_is_ascending_default_and_skip() {
        let json = r#"{"property":"receivedAt"}"#;
        let c: EmailComparator = serde_json::from_str(json).expect("must parse");
        assert!(c.is_ascending, "default must be true");
        let back = serde_json::to_string(&c).expect("serialize");
        assert_eq!(back, json, "isAscending:true must be omitted");
    }

    // ---------------------------------------------------------------------------
    // Filter ambiguity tests (JMAP-amk.2)
    // ---------------------------------------------------------------------------

    /// Oracle: RFC 8621 §4.4 — a JSON object with no recognized fields deserializes
    /// as Filter::Condition with all fields None.  RFC 8621 §4.4.1 states "if zero
    /// properties are specified, the condition MUST always evaluate to true."
    /// The untagged enum tries FilterOperator first (fails: no "operator" key),
    /// then falls through to Condition(T) with all-None fields.
    #[test]
    fn filter_unknown_fields_become_empty_condition() {
        let json = r#"{"unknownField":"value"}"#;
        let f: EmailFilter = serde_json::from_str(json).expect("must parse as empty Condition");
        match &f {
            Filter::Condition(c) => {
                assert_eq!(*c, EmailFilterCondition::default(), "unknown fields yield all-None");
            }
            other => panic!("expected empty Condition, got {other:?}"),
        }
    }

    /// Oracle: an empty JSON object `{}` produces an empty condition (all None),
    /// which matches every Email per RFC 8621 §4.4.1.
    #[test]
    fn filter_empty_object_becomes_empty_condition() {
        let json = "{}";
        let f: EmailFilter = serde_json::from_str(json).expect("must parse");
        assert!(
            matches!(&f, Filter::Condition(c) if c == &EmailFilterCondition::default()),
            "empty object must yield all-None Condition"
        );
    }
}
