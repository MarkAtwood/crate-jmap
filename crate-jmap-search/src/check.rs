//! Checking a `where` against the account's capability
//! (docs/specs/jmap-search.md §Errors, §Nesting Depth, §Opting In a Data
//! Type), and finding the `where` properties of a filter.

use jmap_types::JmapError;
use serde_json::Value;

use crate::{Expression, JMAP_SEARCH_URI, QUERY_TOO_COMPLEX, SearchCapability};

/// The `$collation` values the evaluator implements: `i;ascii-casemap`
/// (RFC 4790 §9.2), `i;octet` (RFC 4790 §9.3) and `i;unicode-casemap`
/// (RFC 5051), the three docs/specs/jmap-search.md §Collation says servers
/// SHOULD support. Each of them "provides equality, substring, and ordering
/// operations" (RFC 4790 §9.2.1, §9.3.1; RFC 5051 §2), so no operator is
/// refused for its collation.
///
/// ponytail: a fixed list rather than the consumer's `collationAlgorithms`;
/// a consumer that advertises fewer accepts these anyway. Upgrade path: a
/// check that takes the advertised list.
pub const COLLATIONS: &[&str] = &["i;ascii-casemap", "i;octet", "i;unicode-casemap"];

/// `true` if `collation` is one of [`COLLATIONS`].
pub fn is_supported_collation(collation: &str) -> bool {
    COLLATIONS.contains(&collation)
}

/// Parse `where_`, a `where` of a `data_type`/query `FilterCondition`, and
/// check it against the account's capability `cap`.
///
/// # Errors
///
/// - `unsupportedFilter` if `data_type` is not in `cap.dataTypes`; if
///   `where` "uses a `$`-prefixed name that is not an operator defined in
///   this document"; if "the expression uses `$text` and the type's
///   `supportsText` is `false`"; or if "an operator object's `$collation`
///   is not in `collationAlgorithms`" (here: not one of [`COLLATIONS`]).
/// - `invalidArguments` "if `where` is not well-formed".
/// - `queryTooComplex` ([`QUERY_TOO_COMPLEX`]) "if the expression's nesting
///   depth exceeds `maxDepth`".
pub fn check_where(
    where_: &Value,
    data_type: &str,
    cap: &SearchCapability,
) -> Result<Expression, JmapError> {
    let Some(info) = cap.data_type(data_type) else {
        return Err(unsupported(format!(
            "{data_type} is not in the {JMAP_SEARCH_URI} dataTypes"
        )));
    };
    let expr = Expression::parse(where_)?;
    let (mut text, mut bad_collation) = (false, false);
    expr.visit(&mut |e| {
        text |= e.text.is_some();
        bad_collation |= e.fields.values().any(|t| {
            t.collation
                .as_deref()
                .is_some_and(|c| !is_supported_collation(c))
        });
    });
    if text && !info.supports_text {
        return Err(unsupported(format!(
            "$text is not supported for {data_type}"
        )));
    }
    if bad_collation {
        return Err(unsupported("unsupported $collation".to_owned()));
    }
    let depth = expr.depth();
    if depth > cap.max_depth {
        let mut err = JmapError::custom(QUERY_TOO_COMPLEX);
        err.description = Some(format!(
            "nesting depth {depth} exceeds maxDepth {}",
            cap.max_depth
        ));
        return Err(err);
    }
    Ok(expr)
}

/// Check every `where` in `filter`, the undecoded `filter` argument of a
/// `data_type`/query or `/queryChanges` call, and return whether there was
/// one. A filter without `where` passes whatever `using` and `cap` are.
///
/// `using` is the request's capability list and `cap` the account's
/// capability, `None` when the account does not advertise it.
///
/// # Errors
///
/// `unsupportedFilter` when a `where` is present but `using` lacks
/// [`JMAP_SEARCH_URI`] or `cap` is `None` (§Opting In a Data Type: "A data
/// type accepts `where` on an account exactly when the server lists it in
/// that account's `dataTypes`, and only in requests whose `using` includes
/// `urn:ietf:params:jmap:search`"); otherwise the first error of
/// [`check_where`].
pub fn check_where_filter(
    filter: Option<&Value>,
    data_type: &str,
    using: &[String],
    cap: Option<&SearchCapability>,
) -> Result<bool, JmapError> {
    let wheres = filter.map(where_values).unwrap_or_default();
    if wheres.is_empty() {
        return Ok(false);
    }
    if !using.iter().any(|u| u == JMAP_SEARCH_URI) {
        return Err(unsupported(format!(
            "where requires {JMAP_SEARCH_URI} in using"
        )));
    }
    let Some(cap) = cap else {
        return Err(unsupported(format!(
            "the account does not advertise {JMAP_SEARCH_URI}"
        )));
    };
    for w in wheres {
        check_where(w, data_type, cap)?;
    }
    Ok(true)
}

/// Every non-`null` `where` in `filter`, a `FilterCondition` or a
/// `FilterOperator` tree (RFC 8620 §5.5), in document order.
///
/// An object with an `operator` member and a `conditions` array is a
/// `FilterOperator`; any other object is a `FilterCondition`.
pub fn where_values(filter: &Value) -> Vec<&Value> {
    fn walk<'a>(f: &'a Value, out: &mut Vec<&'a Value>) {
        if let (Some(_), Some(Value::Array(cs))) = (f.get("operator"), f.get("conditions")) {
            cs.iter().for_each(|c| walk(c, out));
        } else if let Some(w) = f.get("where").filter(|w| !w.is_null()) {
            out.push(w);
        }
    }
    let mut out = Vec::new();
    walk(filter, &mut out);
    out
}

/// `true` if `filter` (as [`where_values`]) sets `where` anywhere: a client
/// then adds [`JMAP_SEARCH_URI`] to `using`.
pub fn filter_uses_where(filter: &Value) -> bool {
    !where_values(filter).is_empty()
}

fn unsupported(description: String) -> JmapError {
    let mut err = JmapError::unsupported_filter();
    err.description = Some(description);
    err
}
