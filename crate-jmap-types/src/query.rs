// Generic filter types for JMAP /query methods (RFC 8620 §5.5).
//
// These types are object-independent.  Object-specific filter conditions
// (e.g. `EmailFilterCondition`) are defined in their respective type crates.

use serde::{Deserialize, Serialize};

/// Logical operator for combining filter conditions (RFC 8620 §5.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Operator {
    And,
    Or,
    Not,
}

/// A filter node: either a logical operator combining sub-filters, or a
/// type-specific condition object (RFC 8620 §5.5).
///
/// Serializes as an untagged union.  The presence of the `"operator"` key
/// distinguishes `Filter::Operator` from `Filter::Condition`.
///
/// **Variant ordering is critical**: `Operator` is listed before `Condition`
/// because serde untagged tries variants in declaration order.
/// `FilterOperator<T>` requires an `"operator"` field and fails fast without
/// it, allowing the deserializer to fall through to `Condition(T)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Filter<T> {
    /// A logical combination of sub-filters.
    Operator(FilterOperator<T>),
    /// A type-specific condition object.
    Condition(T),
}

/// Logical combination of filters (RFC 8620 §5.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterOperator<T> {
    /// Logical operator: AND, OR, or NOT.
    pub operator: Operator,
    /// Sub-conditions to evaluate.
    pub conditions: Vec<Filter<T>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // FilterCondition stub used only within this test module.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Cond {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub has_keyword: Option<String>,
    }

    /// Oracle: exercises the Filter<T> generic with a local stub type.
    /// Adapted from the RFC 8620 §5.5 Todo/query example; `has_keyword` uses
    /// Rust snake_case because `Cond` is a test stub, not a real JMAP type.
    #[test]
    fn filter_operator_or_roundtrip() {
        let json =
            r#"{"operator":"OR","conditions":[{"has_keyword":"music"},{"has_keyword":"video"}]}"#;
        let f: Filter<Cond> = serde_json::from_str(json).expect("must parse");
        match &f {
            Filter::Operator(op) => {
                assert_eq!(op.operator, Operator::Or);
                assert_eq!(op.conditions.len(), 2);
            }
            other => panic!("expected Operator, got {other:?}"),
        }
        let back = serde_json::to_string(&f).expect("must serialize");
        let f2: Filter<Cond> = serde_json::from_str(&back).expect("roundtrip");
        assert_eq!(f, f2);
    }

    /// Oracle: a bare condition object (no "operator" key) deserializes as
    /// Filter::Condition.
    #[test]
    fn filter_condition_deserialization() {
        let json = r#"{"has_keyword":"$seen"}"#;
        let f: Filter<Cond> = serde_json::from_str(json).expect("must parse");
        match &f {
            Filter::Condition(c) => assert_eq!(c.has_keyword.as_deref(), Some("$seen")),
            other => panic!("expected Condition, got {other:?}"),
        }
    }

    /// Oracle: Operator enum serializes as SCREAMING_SNAKE_CASE per RFC 8620 §5.5.
    #[test]
    fn operator_serialization() {
        assert_eq!(serde_json::to_string(&Operator::And).unwrap(), r#""AND""#);
        assert_eq!(serde_json::to_string(&Operator::Or).unwrap(), r#""OR""#);
        assert_eq!(serde_json::to_string(&Operator::Not).unwrap(), r#""NOT""#);
    }

    /// Oracle: nested AND(OR(...)) structure roundtrips correctly.
    #[test]
    fn nested_filter_roundtrip() {
        let filter = Filter::Operator(FilterOperator {
            operator: Operator::And,
            conditions: vec![
                Filter::Operator(FilterOperator {
                    operator: Operator::Or,
                    conditions: vec![
                        Filter::Condition(Cond {
                            has_keyword: Some("a".to_owned()),
                        }),
                        Filter::Condition(Cond {
                            has_keyword: Some("b".to_owned()),
                        }),
                    ],
                }),
                Filter::Condition(Cond {
                    has_keyword: Some("c".to_owned()),
                }),
            ],
        });
        let json = serde_json::to_string(&filter).expect("serialize");
        let back: Filter<Cond> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(filter, back);
    }
}
