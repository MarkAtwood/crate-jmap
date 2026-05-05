//! RFC 9553 §2.7 anniversary types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── PartialDate ───────────────────────────────────────────────────────────────

/// A partial (possibly incomplete) calendar date (RFC 9553 §1.5.1).
///
/// Fields are `Option` because any component may be absent (e.g. year-only,
/// or month+day without year for recurring anniversaries).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialDate {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub month: Option<u8>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<u8>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_scale: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Timestamp ─────────────────────────────────────────────────────────────────

/// An RFC 3339 UTC timestamp (RFC 9553 §1.5.2).
///
/// Stored as a plain `String` to avoid a date-time library dependency;
/// callers may parse with `time`, `chrono`, etc.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timestamp {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// RFC 3339 date-time string, e.g. `"1970-01-01T00:00:00Z"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utc: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── AnniversaryDate ───────────────────────────────────────────────────────────

/// The date value of an anniversary — either a partial date or a timestamp
/// (RFC 9553 §2.7.1).
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnniversaryDate {
    Partial(PartialDate),
    Timestamp(Timestamp),
}

// ── AnniversaryKind ───────────────────────────────────────────────────────────

/// The kind of anniversary (RFC 9553 §2.7.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AnniversaryKind {
    Birth,
    Death,
    Wedding,
    Other(String),
}

impl_string_enum!(AnniversaryKind, "an anniversary kind",
    "birth"   => Birth,
    "death"   => Death,
    "wedding" => Wedding,
);

// ── Anniversary ───────────────────────────────────────────────────────────────

/// An anniversary associated with a contact (RFC 9553 §2.7.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anniversary {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<AnniversaryKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<AnniversaryDate>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Birth anniversary with partial date (year + month + day).
    #[test]
    fn anniversary_birth_partial_date() {
        let json = r#"{
            "@type": "Anniversary",
            "kind": "birth",
            "date": { "@type": "PartialDate", "year": 1853, "month": 3, "day": 30 }
        }"#;

        let ann: Anniversary = serde_json::from_str(json).expect("deserialize Anniversary");
        assert_eq!(ann.kind, Some(AnniversaryKind::Birth));

        if let Some(AnniversaryDate::Partial(pd)) = &ann.date {
            assert_eq!(pd.year, Some(1853));
            assert_eq!(pd.month, Some(3));
            assert_eq!(pd.day, Some(30));
        } else {
            panic!("expected PartialDate variant");
        }

        let re = serde_json::to_string(&ann).unwrap();
        let ann2: Anniversary = serde_json::from_str(&re).unwrap();
        assert_eq!(ann2.kind, Some(AnniversaryKind::Birth));
    }

    /// AnniversaryKind unknown value maps to Other.
    #[test]
    fn anniversary_kind_other() {
        let k: AnniversaryKind = serde_json::from_str(r#""x-custom""#).unwrap();
        assert_eq!(k, AnniversaryKind::Other("x-custom".to_owned()));
    }
}
