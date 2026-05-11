//! RFC 8620 §5.5 generic filter types for JMAP `/query` methods.
//!
//! Provides [`Filter`], [`FilterOperator`], and [`Operator`].
//! Object-specific filter conditions (e.g. `EmailFilterCondition`) are
//! defined in their respective type crates.
//!
//! # Filter algebra is excluded from extras preservation
//!
//! The filter algebra defined in this module is **intentionally not extensible**
//! via the workspace "extras preservation" policy. See [`Filter`],
//! [`FilterOperator`], and [`Operator`] for details. The same exclusion applies
//! to every per-object `FilterCondition` / `Comparator` / `ComparatorProperty`
//! type in the downstream `jmap-*-types` crates (see workspace `AGENTS.md`,
//! bd JMAP-lbdy "Decision: filter algebra excluded").

use serde::{Deserialize, Serialize};

/// Logical operator for combining filter conditions (RFC 8620 §5.5).
///
/// # Excluded from extras preservation
///
/// This enum is **out of scope** for the workspace extras-preservation policy:
/// it carries no `Unknown(String)` catch-all variant, and backends must
/// dispatch on its known variants (`AND`, `OR`, `NOT`) to evaluate a filter
/// tree. An `Unknown` operator would be meaningless — a server that cannot
/// interpret the operator cannot evaluate the filter, and silently round-
/// tripping it back to the client would yield wrong query results.
///
/// More broadly, filter algebra (this enum and the per-object
/// `FilterCondition` / `Comparator` types) is excluded because unrecognised
/// filter clauses are a query-correctness hazard: silently dropping or
/// round-tripping a clause the server does not understand can return the
/// wrong set of records to the client without any error signal.
///
/// ## What to do instead
///
/// **IETF-track path.** Vendors who need both capability-level declaration
/// and filterability for custom fields should use
/// `draft-ietf-jmap-metadata` (capability URI
/// `urn:ietf:params:jmap:metadata`), which defines a `Metadata` / `Annotation`
/// companion object keyed by `(relatedType, relatedId)` with schema discovery
/// via the capability's `metadataTypes` / `maxDepth` properties and a
/// `Metadata/query` filter. Implemented in `jmap-metadata-types`,
/// `jmap-metadata-server`, and `jmap-metadata-client` (bd JMAP-06zp).
///
/// **Pre-IETF escape.** Vendors who cannot wait for the metadata draft can
/// either escape the filter tree to `serde_json::Value` or fork the
/// per-crate `FilterCondition` type. See
/// `crate-jmap-calendars-types/PLAN.md` for the hybrid sloppy-value
/// pattern.
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Operator {
    /// Logical AND: all sub-filters must match (RFC 8620 §5.5).
    And,
    /// Logical OR: at least one sub-filter must match (RFC 8620 §5.5).
    Or,
    /// Logical NOT: none of the sub-filters may match (RFC 8620 §5.5).
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
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field, and the per-object
/// condition type `T` is also expected to be non-extensible. Filter clauses
/// the server does not understand are a query-correctness hazard — silently
/// preserving an unrecognised clause and round-tripping it back to the
/// client can return the wrong set of records with no error signal.
///
/// ## What to do instead
///
/// **IETF-track path.** Vendors who need both capability-level declaration
/// and filterability for custom fields should use
/// `draft-ietf-jmap-metadata` (capability URI
/// `urn:ietf:params:jmap:metadata`), which defines a filterable
/// `Metadata` / `Annotation` companion object. Implemented in `jmap-metadata-types`,
/// `jmap-metadata-server`, and `jmap-metadata-client` (bd JMAP-06zp).
///
/// **Pre-IETF escape.** Vendors who cannot wait for the metadata draft can
/// either escape the filter tree to `serde_json::Value` or fork the
/// per-crate `FilterCondition` type. See
/// `crate-jmap-calendars-types/PLAN.md` for the hybrid sloppy-value
/// pattern.
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Filter<T> {
    /// A logical combination of sub-filters.
    Operator(FilterOperator<T>),
    /// A type-specific condition object.
    Condition(T),
}

/// Logical combination of filters (RFC 8620 §5.5).
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field, and its
/// [`Operator`] field is a closed control enum that backends must dispatch
/// on. See [`Operator`] and [`Filter`] for the rationale and for the two
/// recommended paths (`draft-ietf-jmap-metadata`, bd JMAP-06zp; or the
/// pre-IETF sloppy-value escape).
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterOperator<T> {
    /// Logical operator: AND, OR, or NOT.
    pub operator: Operator,
    /// Sub-conditions to evaluate.
    pub conditions: Vec<Filter<T>>,
}

impl<T> FilterOperator<T> {
    /// Create a new [`FilterOperator`] with the given logical operator and conditions.
    pub fn new(operator: Operator, conditions: Vec<Filter<T>>) -> Self {
        Self {
            operator,
            conditions,
        }
    }
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
