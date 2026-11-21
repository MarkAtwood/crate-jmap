//! The Email filter evaluator: does one Email match an [`EmailFilter`]?
//!
//! Shared by the memory backend's `Email/query` and by `EmailPush` delivery
//! (draft-ietf-jmap-emailpush-05 §4, which requires "`FilterOperator` objects
//! with any of the "AND", "OR", and "NOT" operators" and, for a mail account,
//! "all the `FilterCondition` properties it supports for the "Email/query"
//! method"). Every RFC 8621 §4.4.1 condition and the six
//! draft-ietf-jmap-metadata-05 §3.6 conditions are evaluated; none is
//! skipped.
//!
//! Text conditions (`text`, `from`, `to`, `cc`, `bcc`, `subject`, `body`
//! and the `header` value) are case-insensitive substring tests ("SHOULD be
//! matched in a case-insensitive manner", RFC 8621 §4.4.1), comparing the
//! [`unicode_casemap_prepare`] forms of both strings.

use std::collections::{BTreeMap, BTreeSet};

use jmap_mail_types::{
    Email, EmailAddress, EmailFilter, EmailFilterCondition, Filter, Keyword, Operator,
};
use jmap_metadata_server::{
    metadata_path_exists, metadata_text_contains, metadata_text_equals, unicode_casemap_prepare,
};
use jmap_metadata_types::MetadataTextMatch;
use jmap_types::UTCDate;
use serde_json::{Map, Value};

/// One `FilterCondition` property of RFC 8621 §4.4.1 or
/// draft-ietf-jmap-metadata-05 §3.6.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EmailCondition {
    /// `inMailbox`
    InMailbox,
    /// `inMailboxOtherThan`
    InMailboxOtherThan,
    /// `before`
    Before,
    /// `after`
    After,
    /// `minSize`
    MinSize,
    /// `maxSize`
    MaxSize,
    /// `allInThreadHaveKeyword`
    AllInThreadHaveKeyword,
    /// `someInThreadHaveKeyword`
    SomeInThreadHaveKeyword,
    /// `noneInThreadHaveKeyword`
    NoneInThreadHaveKeyword,
    /// `hasKeyword`
    HasKeyword,
    /// `notKeyword`
    NotKeyword,
    /// `hasAttachment`
    HasAttachment,
    /// `text`
    Text,
    /// `from`
    From,
    /// `to`
    To,
    /// `cc`
    Cc,
    /// `bcc`
    Bcc,
    /// `subject`
    Subject,
    /// `body`
    Body,
    /// `header`
    Header,
    /// `metadataExists`
    MetadataExists,
    /// `privateMetadataExists`
    PrivateMetadataExists,
    /// `metadataTextContains`
    MetadataTextContains,
    /// `privateMetadataTextContains`
    PrivateMetadataTextContains,
    /// `metadataTextEquals`
    MetadataTextEquals,
    /// `privateMetadataTextEquals`
    PrivateMetadataTextEquals,
    /// `where` (JMAP Search, crate-jmap docs/specs/jmap-search.md)
    Where,
}

impl EmailCondition {
    /// `true` for the six draft-ietf-jmap-metadata-05 §3.6 conditions.
    #[cfg_attr(not(feature = "emailpush"), allow(dead_code))]
    pub fn is_metadata(self) -> bool {
        matches!(
            self,
            Self::MetadataExists
                | Self::PrivateMetadataExists
                | Self::MetadataTextContains
                | Self::PrivateMetadataTextContains
                | Self::MetadataTextEquals
                | Self::PrivateMetadataTextEquals
        )
    }
}

/// The conditions an account without `urn:ietf:params:jmap:mail` "MUST
/// support at least" (draft-ietf-jmap-emailpush-05 §4), in the draft's
/// order.
#[cfg_attr(not(feature = "emailpush"), allow(dead_code))]
pub const EMAIL_PUSH_REQUIRED_CONDITIONS: [EmailCondition; 10] = [
    EmailCondition::HasKeyword,
    EmailCondition::NotKeyword,
    EmailCondition::From,
    EmailCondition::To,
    EmailCondition::Cc,
    EmailCondition::Bcc,
    EmailCondition::Subject,
    EmailCondition::Header,
    EmailCondition::MinSize,
    EmailCondition::MaxSize,
];

/// Why a filter could not be evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilterError {
    /// A thread condition (`allInThreadHaveKeyword`,
    /// `someInThreadHaveKeyword`, `noneInThreadHaveKeyword`) was evaluated
    /// without the keywords of the Email's Thread.
    MissingThreadKeywords,
    /// The filter holds an operator or filter form this crate does not
    /// know (`Operator` and `Filter` are `#[non_exhaustive]`).
    Unsupported,
    /// A `before` or `after` value is not an RFC 8620 §1.4 `UTCDate`.
    InvalidDate,
}

/// Whether every `before` and `after` in `filter` is a valid RFC 8620 §1.4
/// `UTCDate` ("date-time" with the time-offset "Z"). Callers reject a
/// filter that fails this before evaluating it.
pub(crate) fn filter_dates_valid(filter: &EmailFilter) -> bool {
    match filter {
        Filter::Condition(c) => [&c.before, &c.after]
            .into_iter()
            .flatten()
            .all(|d| UTCDate::new_validated(d.as_ref()).is_ok()),
        Filter::Operator(op) => op.conditions.iter().all(filter_dates_valid),
        _ => true,
    }
}

/// The keywords of every Email in one Email's Thread, that Email included.
pub(crate) type ThreadKeywords<'a> = [&'a BTreeMap<Keyword, bool>];

/// Whether `email` matches `filter` (RFC 8621 §4.4.1, RFC 8620 §5.5).
///
/// `thread_keywords` holds the keywords of every Email in the Email's
/// Thread; it is needed only for the thread conditions.
///
/// # Errors
///
/// [`FilterError::MissingThreadKeywords`] when a thread condition is
/// reached and `thread_keywords` is `None`; [`FilterError::Unsupported`]
/// for an unknown operator or filter form. Neither ever counts as a match.
pub(crate) fn email_matches(
    email: &Email,
    thread_keywords: Option<&ThreadKeywords<'_>>,
    filter: &EmailFilter,
) -> Result<bool, FilterError> {
    match filter {
        Filter::Condition(c) => condition_matches(email, thread_keywords, c),
        Filter::Operator(op) => {
            let mut results = op
                .conditions
                .iter()
                .map(|f| email_matches(email, thread_keywords, f));
            match op.operator {
                // RFC 8620 §5.5: AND "all of the conditions must match",
                // OR "at least one", NOT "none of the conditions must match".
                // Errors propagate rather than short-circuit to a result.
                Operator::And => fold(&mut results, true, |acc, r| acc && r),
                Operator::Or => fold(&mut results, false, |acc, r| acc || r),
                Operator::Not => fold(&mut results, false, |acc, r| acc || r).map(|any| !any),
                _ => Err(FilterError::Unsupported),
            }
        }
        _ => Err(FilterError::Unsupported),
    }
}

/// Folds the results, stopping at the first error, so an error is reported
/// rather than a result that might depend on the failed condition.
fn fold(
    results: &mut impl Iterator<Item = Result<bool, FilterError>>,
    init: bool,
    f: fn(bool, bool) -> bool,
) -> Result<bool, FilterError> {
    results.try_fold(init, |acc, r| r.map(|r| f(acc, r)))
}

/// The conditions `filter` uses, at any depth.
pub(crate) fn conditions_used(filter: &EmailFilter) -> BTreeSet<EmailCondition> {
    let mut out = BTreeSet::new();
    collect(filter, &mut out);
    out
}

fn collect(filter: &EmailFilter, out: &mut BTreeSet<EmailCondition>) {
    match filter {
        Filter::Condition(c) => out.extend(condition_kinds(c)),
        Filter::Operator(op) => op.conditions.iter().for_each(|f| collect(f, out)),
        _ => {}
    }
}

/// The conditions set in one `FilterCondition`.
fn condition_kinds(c: &EmailFilterCondition) -> impl Iterator<Item = EmailCondition> {
    use EmailCondition as C;
    [
        (c.in_mailbox.is_some(), C::InMailbox),
        (c.in_mailbox_other_than.is_some(), C::InMailboxOtherThan),
        (c.before.is_some(), C::Before),
        (c.after.is_some(), C::After),
        (c.min_size.is_some(), C::MinSize),
        (c.max_size.is_some(), C::MaxSize),
        (
            c.all_in_thread_have_keyword.is_some(),
            C::AllInThreadHaveKeyword,
        ),
        (
            c.some_in_thread_have_keyword.is_some(),
            C::SomeInThreadHaveKeyword,
        ),
        (
            c.none_in_thread_have_keyword.is_some(),
            C::NoneInThreadHaveKeyword,
        ),
        (c.has_keyword.is_some(), C::HasKeyword),
        (c.not_keyword.is_some(), C::NotKeyword),
        (c.has_attachment.is_some(), C::HasAttachment),
        (c.text.is_some(), C::Text),
        (c.from.is_some(), C::From),
        (c.to.is_some(), C::To),
        (c.cc.is_some(), C::Cc),
        (c.bcc.is_some(), C::Bcc),
        (c.subject.is_some(), C::Subject),
        (c.body.is_some(), C::Body),
        (c.header.is_some(), C::Header),
        (c.metadata_exists.is_some(), C::MetadataExists),
        (
            c.private_metadata_exists.is_some(),
            C::PrivateMetadataExists,
        ),
        (c.metadata_text_contains.is_some(), C::MetadataTextContains),
        (
            c.private_metadata_text_contains.is_some(),
            C::PrivateMetadataTextContains,
        ),
        (c.metadata_text_equals.is_some(), C::MetadataTextEquals),
        (
            c.private_metadata_text_equals.is_some(),
            C::PrivateMetadataTextEquals,
        ),
        (c.where_.is_some(), C::Where),
    ]
    .into_iter()
    .filter_map(|(set, kind)| set.then_some(kind))
}

/// One `FilterCondition`: every property set must match (RFC 8621 §4.4.1:
/// "the Email must match all of them").
fn condition_matches(
    email: &Email,
    thread_keywords: Option<&ThreadKeywords<'_>>,
    c: &EmailFilterCondition,
) -> Result<bool, FilterError> {
    let thread = |kw: &Option<Keyword>, test: fn(&ThreadKeywords<'_>, &Keyword) -> bool| match kw {
        None => Ok(true),
        Some(k) => thread_keywords
            .map(|t| test(t, k))
            .ok_or(FilterError::MissingThreadKeywords),
    };
    let all = thread(&c.all_in_thread_have_keyword, |t, k| {
        t.iter().all(|m| has(m, k))
    })?;
    let some = thread(&c.some_in_thread_have_keyword, |t, k| {
        t.iter().any(|m| has(m, k))
    })?;
    let none = thread(&c.none_in_thread_have_keyword, |t, k| {
        !t.iter().any(|m| has(m, k))
    })?;
    if !(all && some && none) {
        return Ok(false);
    }
    // "receivedAt ... must be before this date-time" / "the same or after"
    // (RFC 8621 §4.4.1), compared as instants with fractional seconds.
    let received_vs = |d: &UTCDate| {
        email
            .received_at
            .cmp_instant(d)
            .map_err(|_| FilterError::InvalidDate)
    };
    if let Some(d) = &c.before
        && received_vs(d)?.is_ge()
    {
        return Ok(false);
    }
    if let Some(d) = &c.after
        && received_vs(d)?.is_lt()
    {
        return Ok(false);
    }
    if let Some(w) = &c.where_
        && !where_matches(email, w)?
    {
        return Ok(false);
    }
    let empty = Map::new();
    let metadata = email.metadata.as_ref().unwrap_or(&empty);
    let private = email.private_metadata.as_ref().unwrap_or(&empty);
    Ok(c
        .in_mailbox
        .as_ref()
        .is_none_or(|m| email.mailbox_ids.contains_key(m))
        && c.in_mailbox_other_than
            .as_ref()
            .is_none_or(|ex| email.mailbox_ids.keys().any(|m| !ex.contains(m)))
        // "size ... must be equal to or greater than" / "less than".
        && c.min_size.is_none_or(|n| email.size >= n)
        && c.max_size.is_none_or(|n| email.size < n)
        && c.has_keyword.as_ref().is_none_or(|k| has(&email.keywords, k))
        && c.not_keyword.as_ref().is_none_or(|k| !has(&email.keywords, k))
        && c.has_attachment.is_none_or(|b| email.has_attachment == b)
        && c.text.as_deref().is_none_or(|t| text_matches(email, t))
        && c.from.as_deref().is_none_or(|t| addresses_match(email, "From", &email.from, t))
        && c.to.as_deref().is_none_or(|t| addresses_match(email, "To", &email.to, t))
        && c.cc.as_deref().is_none_or(|t| addresses_match(email, "Cc", &email.cc, t))
        && c.bcc.as_deref().is_none_or(|t| addresses_match(email, "Bcc", &email.bcc, t))
        && c.subject.as_deref().is_none_or(|t| subject_matches(email, t))
        && c.body.as_deref().is_none_or(|t| body_matches(email, t))
        && c.header.as_deref().is_none_or(|h| header_matches(email, h))
        && c.metadata_exists.as_deref().is_none_or(|p| metadata_path_exists(metadata, p))
        && c.private_metadata_exists.as_deref().is_none_or(|p| metadata_path_exists(private, p))
        && text_match(metadata, c.metadata_text_contains.as_ref(), metadata_text_contains)
        && text_match(private, c.private_metadata_text_contains.as_ref(), metadata_text_contains)
        && text_match(metadata, c.metadata_text_equals.as_ref(), metadata_text_equals)
        && text_match(private, c.private_metadata_text_equals.as_ref(), metadata_text_equals))
}

/// JMAP Search `where` (crate-jmap docs/specs/jmap-search.md) on `email`.
///
/// The candidate is the Email's serialized form, its `Email/get`
/// representation, with `bodyValues` empty (§Addressable Properties:
/// "`bodyValues` of an Email is empty because `fetchTextBodyValues`
/// defaults to `false`"). `$text` is the RFC 8621 §4.4.1 `text` condition
/// (§Full-Text Matching: it "SHOULD match as that property does").
///
/// # Errors
///
/// [`FilterError::Unsupported`] when `where_` does not parse (the handler
/// checks it first) or the Email does not serialize.
fn where_matches(email: &Email, where_: &Value) -> Result<bool, FilterError> {
    struct EmailCandidate<'a> {
        json: Value,
        email: &'a Email,
    }
    impl jmap_search::Candidate for EmailCandidate<'_> {
        fn field(&self, pointer: &str) -> Option<&Value> {
            self.json.pointer(pointer)
        }
        fn matches_text(&self, text: &str) -> bool {
            text_matches(self.email, text)
        }
    }
    let expr = jmap_search::Expression::parse(where_).map_err(|_| FilterError::Unsupported)?;
    let mut json = serde_json::to_value(email).map_err(|_| FilterError::Unsupported)?;
    if let Some(bv) = json.get_mut("bodyValues") {
        *bv = Value::Object(Map::new());
    }
    Ok(expr.matches(&EmailCandidate { json, email }))
}

/// RFC 8621 §4.1.1: "a keyword is a case-insensitive string".
fn has(keywords: &BTreeMap<Keyword, bool>, k: &Keyword) -> bool {
    keywords
        .iter()
        .any(|(kw, set)| *set && kw.eq_ignore_ascii_case(k))
}

fn text_match(
    map: &Map<String, Value>,
    m: Option<&MetadataTextMatch>,
    test: fn(&Map<String, Value>, &MetadataTextMatch) -> bool,
) -> bool {
    m.is_none_or(|m| test(map, m))
}

/// Case-insensitive substring test.
fn contains(haystack: &str, needle: &str) -> bool {
    unicode_casemap_prepare(haystack).contains(&unicode_casemap_prepare(needle))
}

/// The raw values of the header fields named `name` (case-insensitive).
fn header_values<'a>(email: &'a Email, name: &'a str) -> impl Iterator<Item = &'a str> {
    email
        .headers
        .iter()
        .filter(move |h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// `from`, `to`, `cc`, `bcc`: "Looks for the text in the <name> header
/// field of the message" (RFC 8621 §4.4.1). Searches the raw header value
/// when the Email carries its headers, and each parsed address's name and
/// email.
fn addresses_match(
    email: &Email,
    header: &str,
    parsed: &Option<Vec<EmailAddress>>,
    text: &str,
) -> bool {
    header_values(email, header).any(|v| contains(v, text))
        || parsed.iter().flatten().any(|a| {
            contains(&a.email, text) || a.name.as_deref().is_some_and(|n| contains(n, text))
        })
}

/// `subject`: "Looks for the text in the Subject header field".
fn subject_matches(email: &Email, text: &str) -> bool {
    email.subject.as_deref().is_some_and(|s| contains(s, text))
        || header_values(email, "Subject").any(|v| contains(v, text))
}

/// `body`: "Looks for the text in one of the body parts of the message".
/// Searches the decoded `bodyValues` the Email carries.
fn body_matches(email: &Email, text: &str) -> bool {
    email.body_values.values().any(|v| contains(&v.value, text))
}

/// `text`: "Looks for the text in the From, To, Cc, Bcc, or Subject header
/// fields of the message, or in any text body part".
fn text_matches(email: &Email, text: &str) -> bool {
    addresses_match(email, "From", &email.from, text)
        || addresses_match(email, "To", &email.to, text)
        || addresses_match(email, "Cc", &email.cc, text)
        || addresses_match(email, "Bcc", &email.bcc, text)
        || subject_matches(email, text)
        || body_matches(email, text)
}

/// `header`: `[name]` matches when the Email has a header field of that
/// name; `[name, text]` when one such field's value contains the text
/// (RFC 8621 §4.4.1).
fn header_matches(email: &Email, header: &[String]) -> bool {
    match header {
        [name] => header_values(email, name).next().is_some(),
        [name, text] => header_values(email, name).any(|v| contains(v, text)),
        // EmailFilterCondition's deserializer allows only one or two
        // elements; any other shape constrains nothing it can name.
        _ => false,
    }
}

/// Parse an RFC 3339 / RFC 8620 §1.4 `Date` timestamp string to seconds
/// since the Unix epoch (UTC), returning `None` on any parse failure.
///
/// Handles both `Z` suffix and `+HH:MM` / `-HH:MM` offsets so that
/// timestamps with non-UTC offsets compare correctly by absolute UTC
/// time. Also accepts optional fractional seconds before the offset.
///
/// Limitations:
/// - Does not validate calendar date/time fields beyond the month range.
/// - Does not handle leap seconds.
/// - Year must be in the range 1970–9999.
///
/// Call sites whose input is a typed `UTCDate` (strict UTC) should use
/// [`jmap_types::UTCDate::to_epoch_seconds`]; this helper is for values a
/// client may send with an offset, such as filter `before` / `after`.
// Only the memory backend calls it; with `emailpush` alone this module is
// built without a caller.
#[cfg_attr(not(feature = "memory"), allow(dead_code))]
pub(crate) fn try_rfc3339_to_epoch_secs(s: &str) -> Option<i64> {
    // Expected format: YYYY-MM-DDTHH:MM:SS[.fff](Z|+HH:MM|-HH:MM)
    // Length with Z offset: 20 chars; with millis+Z: 24 chars; with ±HH:MM: 25 chars.
    let s = s.trim();
    // The byte slicing below needs ASCII: a multi-byte character across a
    // field boundary would panic.
    if s.len() < 20 || !s.is_ascii() {
        return None;
    }

    let year: i64 = s[0..4].parse().ok()?;
    if s.as_bytes()[4] != b'-' {
        return None;
    }
    let month: i64 = s[5..7].parse().ok()?;
    if s.as_bytes()[7] != b'-' {
        return None;
    }
    let day: i64 = s[8..10].parse().ok()?;
    if !matches!(s.as_bytes()[10], b'T' | b't') {
        return None;
    }
    let hour: i64 = s[11..13].parse().ok()?;
    if s.as_bytes()[13] != b':' {
        return None;
    }
    let minute: i64 = s[14..16].parse().ok()?;
    if s.as_bytes()[16] != b':' {
        return None;
    }
    let second: i64 = s[17..19].parse().ok()?;

    // Skip optional fractional seconds (.NNN or .NNNNNN etc.) before the offset.
    let frac_skip = if s.as_bytes().get(19) == Some(&b'.') {
        let frac_end = s[20..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| 20 + i)
            .unwrap_or(s.len());
        frac_end - 19
    } else {
        0
    };
    let offset_start = 19 + frac_skip;

    let offset_str = &s[offset_start..];
    let offset_secs: i64 = if offset_str.eq_ignore_ascii_case("z") {
        0
    } else if offset_str.len() == 6
        && (offset_str.starts_with('+') || offset_str.starts_with('-'))
        && offset_str.as_bytes()[3] == b':'
    {
        let sign: i64 = if offset_str.starts_with('-') { -1 } else { 1 };
        let oh: i64 = offset_str[1..3].parse().ok()?;
        let om: i64 = offset_str[4..6].parse().ok()?;
        sign * (oh * 3600 + om * 60)
    } else {
        return None;
    };

    // Days-since-epoch calculation using the proleptic Gregorian calendar.
    // Number of days from 1970-01-01 to year-01-01 (ignoring this year's months/days).
    let y = year - 1;
    let leap_days = y / 4 - y / 100 + y / 400;
    // 477 = number of leap days from year 1 to year 1969 inclusive (1969/4 - 1969/100 + 1969/400).
    let days_to_year_start = y * 365 + leap_days - (1969 * 365 + 477);

    // Days within the year up to the start of the month.
    let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut days_in_year: i64 = 0;
    for (m, days) in MONTH_DAYS
        .iter()
        .enumerate()
        .take(usize::try_from(month - 1).ok()?)
    {
        let extra = if m == 1 && is_leap { 1 } else { 0 };
        days_in_year += days + extra;
    }

    let total_days = days_to_year_start + days_in_year + (day - 1);
    let utc_secs = total_days * 86400 + hour * 3600 + minute * 60 + second - offset_secs;
    Some(utc_secs)
}

#[cfg(test)]
mod tests {
    //! Oracles: RFC 8621 §4.4.1 condition definitions, RFC 8620 §5.5
    //! operators and the draft-ietf-jmap-emailpush-05 §5 filter, applied by
    //! hand to the hand-written Email below.
    use super::*;
    use serde_json::json;

    const INBOX: &str = "674cc240-95db-49ce-a8a2-054f4f733095";

    /// A 1000-octet Email in the inbox, flagged, from John Smith to Jane,
    /// with a List-Id header and one text body value.
    fn email() -> Email {
        serde_json::from_value(json!({
            "id": "M1", "blobId": "B1", "threadId": "T1",
            "mailboxIds": {INBOX: true},
            "keywords": {"$flagged": true},
            "size": 1000,
            "receivedAt": "2026-01-02T03:04:05Z",
            "from": [{"name": "John Smith", "email": "john@example.com"}],
            "to": [{"name": "Jane", "email": "jane@example.net"}],
            "cc": [{"email": "carol@example.org"}],
            "bcc": [{"email": "dave@example.org"}],
            "subject": "Surprise birthday party for Jane tonight",
            "headers": [
                {"name": "From", "value": " John Smith <john@example.com>"},
                {"name": "List-Id", "value": " <party.example.com>"}
            ],
            "bodyValues": {"1": {"value": "Bring CAKE.", "isEncodingProblem": false,
                                 "isTruncated": false}},
            "hasAttachment": false
        }))
        .unwrap()
    }

    fn f(v: serde_json::Value) -> EmailFilter {
        serde_json::from_value(v).unwrap()
    }

    fn matches(v: serde_json::Value) -> bool {
        email_matches(&email(), None, &f(v)).unwrap()
    }

    /// emailpush-05 §4's ten required conditions, one match and one
    /// non-match each.
    #[test]
    fn required_conditions() {
        let cases = [
            (
                json!({"hasKeyword": "$flagged"}),
                json!({"hasKeyword": "$seen"}),
            ),
            (
                json!({"notKeyword": "$seen"}),
                json!({"notKeyword": "$flagged"}),
            ),
            (json!({"from": "SMITH"}), json!({"from": "jane"})),
            (json!({"to": "jane@example"}), json!({"to": "john"})),
            (json!({"cc": "carol"}), json!({"cc": "dave"})),
            (json!({"bcc": "dave"}), json!({"bcc": "carol"})),
            (
                json!({"subject": "birthday PARTY"}),
                json!({"subject": "meeting"}),
            ),
            (
                json!({"header": ["list-id"]}),
                json!({"header": ["X-Spam"]}),
            ),
            (json!({"minSize": 1000}), json!({"minSize": 1001})),
            (json!({"maxSize": 1001}), json!({"maxSize": 1000})),
        ];
        for (yes, no) in cases {
            assert!(matches(yes.clone()), "{yes}");
            assert!(!matches(no.clone()), "{no}");
        }
        assert!(matches(json!({"header": ["List-Id", "PARTY.example"]})));
        assert!(!matches(json!({"header": ["List-Id", "other"]})));
    }

    #[test]
    fn other_conditions() {
        assert!(matches(json!({"inMailbox": INBOX})));
        assert!(!matches(json!({"inMailbox": "trash"})));
        assert!(matches(json!({"inMailboxOtherThan": ["trash"]})));
        assert!(!matches(json!({"inMailboxOtherThan": [INBOX]})));
        // "before": receivedAt strictly before; "after": same or after.
        assert!(matches(json!({"before": "2026-01-02T03:04:06Z"})));
        assert!(!matches(json!({"before": "2026-01-02T03:04:05Z"})));
        assert!(matches(json!({"after": "2026-01-02T03:04:05Z"})));
        assert!(!matches(json!({"after": "2026-01-02T03:04:06Z"})));
        assert!(matches(json!({"hasAttachment": false})));
        assert!(!matches(json!({"hasAttachment": true})));
        assert!(matches(json!({"body": "cake"})));
        assert!(!matches(json!({"body": "jane"})));
        // text: header fields or any text body part.
        assert!(matches(json!({"text": "cake"})));
        assert!(matches(json!({"text": "carol"})));
        assert!(!matches(json!({"text": "zebra"})));
        // Every property of one condition must match.
        assert!(!matches(json!({"from": "john", "subject": "meeting"})));
    }

    #[test]
    fn operators() {
        let yes = json!({"from": "john"});
        let no = json!({"from": "zebra"});
        let op = |o: &str, c: Vec<&serde_json::Value>| json!({"operator": o, "conditions": c});
        assert!(matches(op("AND", vec![&yes, &yes])));
        assert!(!matches(op("AND", vec![&yes, &no])));
        assert!(matches(op("OR", vec![&no, &yes])));
        assert!(!matches(op("OR", vec![&no, &no])));
        // RFC 8620 §5.5 NOT: "none of the conditions must match".
        assert!(matches(op("NOT", vec![&no, &no])));
        assert!(!matches(op("NOT", vec![&no, &yes])));
        assert!(!matches(op("NOT", vec![&yes])));
    }

    /// emailpush-05 §5: in the inbox, or has $notify.
    #[test]
    fn section_5_filter() {
        let filter = f(json!({
            "operator": "OR",
            "conditions": [{"inMailbox": INBOX}, {"hasKeyword": "$notify"}]
        }));
        let in_inbox = email();
        assert!(email_matches(&in_inbox, None, &filter).unwrap());
        let mut elsewhere = email();
        elsewhere.mailbox_ids = BTreeMap::from([(jmap_types::Id::from("archive"), true)]);
        assert!(!email_matches(&elsewhere, None, &filter).unwrap());
        elsewhere.keywords.insert(Keyword::from("$notify"), true);
        assert!(email_matches(&elsewhere, None, &filter).unwrap());
    }

    #[test]
    fn thread_conditions() {
        let flagged = BTreeMap::from([(Keyword::from("$flagged"), true)]);
        let plain = BTreeMap::new();
        let thread: Vec<&BTreeMap<Keyword, bool>> = vec![&flagged, &plain];
        let run = |v| email_matches(&email(), Some(&thread), &f(v)).unwrap();
        assert!(!run(json!({"allInThreadHaveKeyword": "$flagged"})));
        assert!(run(json!({"someInThreadHaveKeyword": "$flagged"})));
        assert!(!run(json!({"noneInThreadHaveKeyword": "$flagged"})));
        assert!(run(json!({"noneInThreadHaveKeyword": "$seen"})));
        let one: Vec<&BTreeMap<Keyword, bool>> = vec![&flagged];
        assert!(
            email_matches(
                &email(),
                Some(&one),
                &f(json!({"allInThreadHaveKeyword": "$flagged"}))
            )
            .unwrap()
        );
    }

    /// A thread condition without the Thread's keywords is an error, never
    /// a match, at any depth.
    #[test]
    fn thread_condition_without_keywords_is_an_error() {
        for v in [
            json!({"someInThreadHaveKeyword": "$flagged"}),
            json!({"operator": "NOT", "conditions": [{"allInThreadHaveKeyword": "$x"}]}),
            json!({"operator": "OR", "conditions": [
                {"from": "john"}, {"noneInThreadHaveKeyword": "$x"}]}),
        ] {
            assert_eq!(
                email_matches(&email(), None, &f(v.clone())),
                Err(FilterError::MissingThreadKeywords),
                "{v}"
            );
        }
    }

    #[test]
    fn metadata_conditions() {
        let mut e = email();
        e.metadata = Some(
            json!({"tags.example.com": {"t": "Urgent"}})
                .as_object()
                .unwrap()
                .clone(),
        );
        e.private_metadata = Some(json!({"p.example.com": "x"}).as_object().unwrap().clone());
        let run = |v| email_matches(&e, None, &f(v)).unwrap();
        assert!(run(json!({"metadataExists": "tags.example.com"})));
        assert!(!run(json!({"metadataExists": "p.example.com"})));
        assert!(run(json!({"privateMetadataExists": "p.example.com"})));
        assert!(run(
            json!({"metadataTextContains": {"path": "tags.example.com/t", "value": "urg"}})
        ));
        assert!(!run(
            json!({"metadataTextEquals": {"path": "tags.example.com/t", "value": "urgent"}})
        ));
        assert!(run(
            json!({"privateMetadataTextEquals": {"path": "p.example.com", "value": "x"}})
        ));
    }

    #[test]
    fn conditions_used_walks_operators() {
        use EmailCondition as C;
        let filter = f(json!({"operator": "NOT", "conditions": [
            {"from": "a", "minSize": 1},
            {"operator": "OR", "conditions": [{"header": ["X"]}]}
        ]}));
        assert_eq!(
            conditions_used(&filter),
            BTreeSet::from([C::From, C::MinSize, C::Header])
        );
        assert!(
            EMAIL_PUSH_REQUIRED_CONDITIONS
                .iter()
                .all(|c| !c.is_metadata())
        );
    }

    /// RFC 3339 §5.6: "date-month = 2DIGIT ; 01-12". Month `00` is not a
    /// date, so it must parse as `None` rather than wrapping `month - 1`
    /// to `usize::MAX` (bd:JMAP-uqhn.1). Valid-path oracle: January has
    /// 31 days, so 1970-02-01T00:00:00Z is 31 * 86400 = 2678400 s.
    #[test]
    fn rfc3339_month_zero_is_rejected() {
        assert_eq!(
            try_rfc3339_to_epoch_secs("1970-02-01T00:00:00Z"),
            Some(2_678_400)
        );
        assert_eq!(try_rfc3339_to_epoch_secs("1970-00-01T00:00:00Z"), None);
    }
}
