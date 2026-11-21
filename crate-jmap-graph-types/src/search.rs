//! draft-atwood-jmap-graph-search-00 — JMAP Graph Search, the
//! `urn:ietf:params:jmap:graph:search` capability.
//!
//! - [`JMAP_GRAPH_SEARCH_URI`], [`GraphSearchCapability`] (§Capability).
//! - The §`FilterCondition` Extensions `where`, `language` and `expression`
//!   are fields of [`GraphNodeFilterCondition`] and
//!   [`GraphEdgeFilterCondition`].
//! - [`Expression`], the §The Built-In Filter Language AST, with
//!   [`FieldTest`], [`Bound`], [`EdgeTest`], [`ReachTest`] and
//!   [`Direction`]; [`Expression::parse`] checks well-formedness and
//!   [`Expression::traversal_depth`] computes §Traversal Depth.
//! - [`QUERY_TOO_COMPLEX`] (§Errors).
//!
//! The type-neutral parts — [`FieldTest`], [`Bound`], [`WhereError`] and
//! [`QUERY_TOO_COMPLEX`] — are those of the `jmap-search` crate
//! (urn:ietf:params:jmap:search), re-exported at this crate's root: the
//! built-in language is that crate's language plus the structural
//! operators defined here.
//!
//! # Why `where` is raw JSON in the filter condition
//!
//! The draft gives a malformed expression `invalidArguments` but an unknown
//! `$`-prefixed name `unsupportedFilter`, and whether `$edge` or `$from` is
//! well-formed depends on the kind of candidate. A serde decode can do
//! neither, so the condition keeps `where` as a [`serde_json::Value`] and
//! [`Expression::parse`] takes the [`CandidateKind`] and returns a
//! [`WhereError`] that names the error type.
//!
//! Not checked here, because they need the account's capabilities: the
//! traversal depth against `maxDepth` (`queryTooComplex`), `$text` against
//! `supportsText`, `$collation` against `collationAlgorithms`, `language`
//! against `languages`, and `language` present if and only if `expression`
//! is. Evaluation is the server's job.
//!
//! [`GraphNodeFilterCondition`]: crate::query::GraphNodeFilterCondition
//! [`GraphEdgeFilterCondition`]: crate::query::GraphEdgeFilterCondition

use std::collections::BTreeMap;

use jmap_search::{FieldTest, WhereError, is_json_pointer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[cfg(doc)]
use jmap_search::{Bound, QUERY_TOO_COMPLEX};

/// Capability URI for JMAP Graph Search
/// (draft-atwood-jmap-graph-search-00 §Capability).
///
/// "This capability is meaningful only when `urn:ietf:params:jmap:graph` is
/// also advertised for the account." The session-level value "is an empty
/// JSON object `{}`" and is not modelled; the account-level value is
/// [`GraphSearchCapability`].
pub const JMAP_GRAPH_SEARCH_URI: &str = "urn:ietf:params:jmap:graph:search";

/// The largest RFC 8620 §1.3 `UnsignedInt`, 2^53-1.
const MAX_UNSIGNED_INT: u64 = (1 << 53) - 1;

/// Account-level capability object for
/// `"urn:ietf:params:jmap:graph:search"`.
///
/// Found at
/// `accounts[id].accountCapabilities["urn:ietf:params:jmap:graph:search"]`.
///
/// Spec: draft-atwood-jmap-graph-search-00 §Account-Level Capability
/// Object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSearchCapability {
    /// "The identifiers, from the "JMAP Graph Query Languages" registry, of
    /// the additional query languages the server supports on this account.
    /// May be empty. Clients ignore identifiers they do not recognize."
    pub languages: Vec<String>,
    /// "The largest traversal depth the server accepts in a built-in filter
    /// expression." Compare with [`Expression::traversal_depth`].
    pub max_depth: u64,
    /// "`true` if the server supports the `$text` operator."
    pub supports_text: bool,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// The kind of object an expression is tested against: "a `GraphNode` during
/// GraphNode/query, or a `GraphEdge` during GraphEdge/query, or a node or edge
/// reached while evaluating a traversal operator" (§Conventions and
/// Definitions, Candidate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    /// A `GraphNode`: `$edge` and `$reachable` apply.
    Node,
    /// A `GraphEdge`: `$from` and `$to` apply.
    Edge,
}

/// A built-in filter expression (draft-atwood-jmap-graph-search-00 §The
/// Built-In Filter Language).
///
/// "An Expression is a JSON object. The expression is true if every one of
/// its members is true. An empty object is always true." Each field holds
/// one kind of member, so [`Expression::default`] is the empty, always-true
/// expression and [`Expression::to_value`] cannot produce a duplicate
/// member name.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Expression {
    /// Field tests, keyed by their JSON Pointer (RFC 6901), such as
    /// `/annotations/com.example.status`. "A pointer that does not resolve
    /// to a value leaves the field **missing**."
    pub fields: BTreeMap<String, FieldTest>,
    /// `$and`: "True if every expression in the array is true. An empty
    /// array is true."
    pub and: Option<Vec<Expression>>,
    /// `$or`: "True if at least one expression in the array is true. An
    /// empty array is false."
    pub or: Option<Vec<Expression>>,
    /// `$not`: "True if the expression is false."
    pub not: Option<Box<Expression>>,
    /// `$edge` (`GraphNode` candidates only): "True if the candidate has at
    /// least one edge satisfying the [`EdgeTest`]."
    pub edge: Option<Box<EdgeTest>>,
    /// `$reachable` (`GraphNode` candidates only): "True if some node
    /// reachable from the candidate satisfies the test."
    pub reachable: Option<Box<ReachTest>>,
    /// `$from` (`GraphEdge` candidates only): "The node named by the candidate
    /// edge's `fromId` satisfies the expression."
    pub from: Option<Box<Expression>>,
    /// `$to` (`GraphEdge` candidates only): "The node named by the candidate
    /// edge's `toId` satisfies the expression."
    pub to: Option<Box<Expression>>,
    /// `$text`: "True if the candidate matches the given text." Matching is
    /// server-defined.
    pub text: Option<String>,
}

/// The `direction` of an [`EdgeTest`] or [`ReachTest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// `"out"`: "edges whose `fromId` is the candidate".
    Out,
    /// `"in"`: "edges whose `toId` is the candidate".
    In,
    /// `"any"`: both.
    Any,
}

/// The operand of `$edge` (§Structural Operators).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeTest {
    /// Required.
    pub direction: Direction,
    /// `type`: "the edge's `type` equals this value exactly."
    pub edge_type: Option<String>,
    /// `edge`: "an expression that the edge itself satisfies" (a `GraphEdge`
    /// candidate).
    pub edge: Option<Expression>,
    /// `node`: "an expression that the node at the other end of the edge
    /// satisfies" (a `GraphNode` candidate).
    pub node: Option<Expression>,
}

impl EdgeTest {
    /// A test for any edge in `direction`.
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            edge_type: None,
            edge: None,
            node: None,
        }
    }
}

/// The operand of `$reachable` (§Structural Operators).
///
/// "A node is reachable if a path of 1 to `maxDepth` steps leads to it from
/// the candidate."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ReachTest {
    /// Required: "each step follows an edge in this direction."
    pub direction: Direction,
    /// `type`: "only edges whose `type` equals this value are followed."
    pub edge_type: Option<String>,
    /// Required: "the largest number of steps followed; at least 1."
    pub max_depth: u64,
    /// `node`: "an expression that the reached node satisfies."
    pub node: Option<Expression>,
}

impl ReachTest {
    /// A test for any node within `max_depth` steps in `direction`.
    pub fn new(direction: Direction, max_depth: u64) -> Self {
        Self {
            direction,
            edge_type: None,
            max_depth,
            node: None,
        }
    }
}

impl Expression {
    /// Parse and check a `where` value for a candidate of `kind`
    /// (§`FilterCondition` Extensions, well-formedness).
    ///
    /// Unknown members of an [`EdgeTest`] or [`ReachTest`] are malformed even when
    /// `$`-prefixed: they "contain only the members defined for them".
    ///
    /// # Errors
    ///
    /// [`WhereError::UnknownOperator`] for a `$`-prefixed expression member
    /// or operator-object name the draft does not define, otherwise
    /// [`WhereError::Malformed`] when the value is not well-formed. The
    /// first defect found is reported.
    pub fn parse(value: &Value, kind: CandidateKind) -> Result<Self, WhereError> {
        let obj = value
            .as_object()
            .ok_or_else(|| malformed("an expression must be a JSON object"))?;
        let mut expr = Self::default();
        for (name, v) in obj {
            match name.as_str() {
                "$and" => expr.and = Some(parse_list(v, kind, name)?),
                "$or" => expr.or = Some(parse_list(v, kind, name)?),
                "$not" => expr.not = Some(Box::new(Self::parse(v, kind)?)),
                "$edge" => {
                    require_kind(kind, CandidateKind::Node, name)?;
                    expr.edge = Some(Box::new(parse_edge_test(v)?));
                }
                "$reachable" => {
                    require_kind(kind, CandidateKind::Node, name)?;
                    expr.reachable = Some(Box::new(parse_reach_test(v)?));
                }
                "$from" => {
                    require_kind(kind, CandidateKind::Edge, name)?;
                    expr.from = Some(Box::new(Self::parse(v, CandidateKind::Node)?));
                }
                "$to" => {
                    require_kind(kind, CandidateKind::Edge, name)?;
                    expr.to = Some(Box::new(Self::parse(v, CandidateKind::Node)?));
                }
                "$text" => expr.text = Some(string(v, name)?),
                n if n.starts_with('$') => return Err(WhereError::UnknownOperator(n.to_owned())),
                n if is_json_pointer(n) => {
                    expr.fields.insert(n.to_owned(), FieldTest::parse(v)?);
                }
                n => {
                    return Err(malformed(&format!(
                        "member {n:?} is neither a JSON Pointer nor an operator"
                    )));
                }
            }
        }
        Ok(expr)
    }

    /// The JSON form of the expression, for a filter condition's `where`.
    ///
    /// Field tests are always written as operator objects, never as the
    /// plain-value shorthand, so an `$eq` operand that is itself an object
    /// with `$`-prefixed names keeps its meaning. A field test with no
    /// operators is always true but has no wire form (§Field Tests: `{}`
    /// is shorthand for `{"$eq": {}}`), so it is left out, which keeps the
    /// expression's meaning because every member must be true.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        for (pointer, test) in &self.fields {
            if *test != FieldTest::default() {
                m.insert(pointer.clone(), test.to_value());
            }
        }
        let list = |v: &[Expression]| Value::Array(v.iter().map(Self::to_value).collect());
        if let Some(v) = &self.and {
            m.insert("$and".to_owned(), list(v));
        }
        if let Some(v) = &self.or {
            m.insert("$or".to_owned(), list(v));
        }
        if let Some(e) = &self.not {
            m.insert("$not".to_owned(), e.to_value());
        }
        if let Some(t) = &self.edge {
            m.insert("$edge".to_owned(), t.to_value());
        }
        if let Some(t) = &self.reachable {
            m.insert("$reachable".to_owned(), t.to_value());
        }
        if let Some(e) = &self.from {
            m.insert("$from".to_owned(), e.to_value());
        }
        if let Some(e) = &self.to {
            m.insert("$to".to_owned(), e.to_value());
        }
        if let Some(t) = &self.text {
            m.insert("$text".to_owned(), Value::String(t.clone()));
        }
        Value::Object(m)
    }

    /// The traversal depth (§Traversal Depth): "the largest, over every
    /// chain of nested structural operators, of the sum of the steps along
    /// the chain, where `$edge`, `$from`, and `$to` count as one step and
    /// `$reachable` counts as its `maxDepth`." An expression without
    /// structural operators has depth 0. A depth above the advertised
    /// [`GraphSearchCapability::max_depth`] is [`QUERY_TOO_COMPLEX`].
    pub fn traversal_depth(&self) -> u64 {
        let opt = |e: Option<&Expression>| e.map_or(0, Self::traversal_depth);
        let max_of = |v: Option<&Vec<Expression>>| {
            v.into_iter()
                .flatten()
                .map(Self::traversal_depth)
                .max()
                .unwrap_or(0)
        };
        let edge = self
            .edge
            .as_deref()
            .map_or(0, |t| 1 + opt(t.edge.as_ref()).max(opt(t.node.as_ref())));
        let reach = self
            .reachable
            .as_deref()
            .map_or(0, |t| t.max_depth.saturating_add(opt(t.node.as_ref())));
        let from = self.from.as_deref().map_or(0, |e| 1 + e.traversal_depth());
        let to = self.to.as_deref().map_or(0, |e| 1 + e.traversal_depth());
        [
            max_of(self.and.as_ref()),
            max_of(self.or.as_ref()),
            opt(self.not.as_deref()),
            edge,
            reach,
            from,
            to,
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }
}

impl EdgeTest {
    fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("direction".to_owned(), direction_value(self.direction));
        if let Some(t) = &self.edge_type {
            m.insert("type".to_owned(), Value::String(t.clone()));
        }
        if let Some(e) = &self.edge {
            m.insert("edge".to_owned(), e.to_value());
        }
        if let Some(e) = &self.node {
            m.insert("node".to_owned(), e.to_value());
        }
        Value::Object(m)
    }
}

impl ReachTest {
    fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("direction".to_owned(), direction_value(self.direction));
        if let Some(t) = &self.edge_type {
            m.insert("type".to_owned(), Value::String(t.clone()));
        }
        m.insert("maxDepth".to_owned(), Value::from(self.max_depth));
        if let Some(e) = &self.node {
            m.insert("node".to_owned(), e.to_value());
        }
        Value::Object(m)
    }
}

fn malformed(why: &str) -> WhereError {
    WhereError::malformed(why)
}

fn require_kind(kind: CandidateKind, wanted: CandidateKind, name: &str) -> Result<(), WhereError> {
    if kind == wanted {
        Ok(())
    } else {
        Err(malformed(&format!(
            "{name} does not apply to a {kind:?} candidate"
        )))
    }
}

fn string(v: &Value, name: &str) -> Result<String, WhereError> {
    v.as_str()
        .map(str::to_owned)
        .ok_or_else(|| malformed(&format!("{name} must be a String")))
}

fn array<'a>(v: &'a Value, name: &str) -> Result<&'a Vec<Value>, WhereError> {
    v.as_array()
        .ok_or_else(|| malformed(&format!("{name} must be an Array")))
}

fn parse_list(v: &Value, kind: CandidateKind, name: &str) -> Result<Vec<Expression>, WhereError> {
    array(v, name)?
        .iter()
        .map(|e| Expression::parse(e, kind))
        .collect()
}

fn parse_direction(v: &Value) -> Result<Direction, WhereError> {
    match v.as_str() {
        Some("out") => Ok(Direction::Out),
        Some("in") => Ok(Direction::In),
        Some("any") => Ok(Direction::Any),
        _ => Err(malformed(r#"direction must be "out", "in", or "any""#)),
    }
}

fn direction_value(d: Direction) -> Value {
    Value::from(match d {
        Direction::Out => "out",
        Direction::In => "in",
        Direction::Any => "any",
    })
}

fn test_object<'a>(v: &'a Value, name: &str) -> Result<&'a Map<String, Value>, WhereError> {
    v.as_object()
        .ok_or_else(|| malformed(&format!("{name} must be a JSON object")))
}

fn required_direction(obj: &Map<String, Value>, name: &str) -> Result<Direction, WhereError> {
    obj.get("direction")
        .ok_or_else(|| malformed(&format!("{name} requires direction")))
        .and_then(parse_direction)
}

fn parse_edge_test(v: &Value) -> Result<EdgeTest, WhereError> {
    let obj = test_object(v, "$edge")?;
    let mut t = EdgeTest::new(required_direction(obj, "$edge")?);
    for (name, v) in obj {
        match name.as_str() {
            "direction" => {}
            "type" => t.edge_type = Some(string(v, "$edge type")?),
            "edge" => t.edge = Some(Expression::parse(v, CandidateKind::Edge)?),
            "node" => t.node = Some(Expression::parse(v, CandidateKind::Node)?),
            n => return Err(malformed(&format!("$edge has no member {n:?}"))),
        }
    }
    Ok(t)
}

fn parse_reach_test(v: &Value) -> Result<ReachTest, WhereError> {
    let obj = test_object(v, "$reachable")?;
    let max_depth = obj
        .get("maxDepth")
        .ok_or_else(|| malformed("$reachable requires maxDepth"))?
        .as_u64()
        .filter(|d| (1..=MAX_UNSIGNED_INT).contains(d))
        .ok_or_else(|| malformed("$reachable maxDepth must be an UnsignedInt of at least 1"))?;
    let mut t = ReachTest::new(required_direction(obj, "$reachable")?, max_depth);
    for (name, v) in obj {
        match name.as_str() {
            "direction" | "maxDepth" => {}
            "type" => t.edge_type = Some(string(v, "$reachable type")?),
            "node" => t.node = Some(Expression::parse(v, CandidateKind::Node)?),
            n => return Err(malformed(&format!("$reachable has no member {n:?}"))),
        }
    }
    Ok(t)
}
