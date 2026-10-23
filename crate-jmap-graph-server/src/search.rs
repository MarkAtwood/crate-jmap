//! draft-atwood-jmap-graph-search-00 on the server: checking a search
//! filter against the account's `urn:ietf:params:jmap:graph:search`
//! capability ([`check_search_filter`]) and evaluating the built-in `where`
//! language over one Graph ([`GraphView`]).
//!
//! The GraphNode and GraphEdge `/query` handlers call
//! [`check_search_filter`] for an account whose
//! [`GraphBackend::graph_search_capability`](crate::GraphBackend::graph_search_capability)
//! is `Some`; the backend's `query_objects` then evaluates `where` with a
//! [`GraphView`] of the Graph's contents the caller may see, as the
//! reference `MemoryBackend` does. No advertised language
//! (`language`/`expression`) is implemented here.
//!
//! # Collations
//!
//! `$collation` may name one of [`COLLATIONS`]; anything else is
//! `unsupportedFilter`. Each of them "provides equality, substring, and
//! ordering operations" (RFC 4790 §9.2.1, §9.3.1; RFC 5051 §2), so no operator
//! is refused for its collation. A consumer advertises at least these in
//! the core capability's `collationAlgorithms` (RFC 8620 §2).

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use icu_casemap::CaseMapper;
use icu_normalizer::DecomposingNormalizerBorrowed;
use jmap_graph_types::{
    Bound, CandidateKind, Direction, EdgeTest, Expression, FieldTest, GraphSearchCapability,
    QUERY_TOO_COMPLEX, ReachTest,
};
use jmap_graph_types::{GraphEdge, GraphNode};
use jmap_types::{Id, JmapError};
use serde_json::{Number, Value};

/// The `$collation` values the evaluator implements: `i;ascii-casemap`
/// (RFC 4790 §9.2), `i;octet` (RFC 4790 §9.3) and `i;unicode-casemap`
/// (RFC 5051).
///
/// ponytail: a fixed list rather than the consumer's `collationAlgorithms`;
/// a consumer that advertises fewer accepts these anyway. Upgrade path: a
/// `GraphBackend` hook returning the advertised list.
pub const COLLATIONS: &[&str] = &["i;ascii-casemap", "i;octet", "i;unicode-casemap"];

/// The FilterCondition properties draft-atwood-jmap-graph-search-00 adds
/// (§FilterCondition Extensions).
const SEARCH_PROPERTIES: [&str; 3] = ["where", "language", "expression"];

/// `true` if `filter` sets `where`, `language` or `expression` to anything
/// but `null`.
pub(crate) fn uses_search(filter: &Value) -> bool {
    SEARCH_PROPERTIES
        .iter()
        .any(|k| filter.get(k).is_some_and(|v| !v.is_null()))
}

/// Check the search properties of a GraphNode/query (`kind`
/// [`CandidateKind::Node`]) or GraphEdge/query ([`CandidateKind::Edge`])
/// `filter` against the account's capability `cap`
/// (draft-atwood-jmap-graph-search-00 §FilterCondition Extensions,
/// §Traversal Depth). A filter without them passes.
///
/// Whether `expression` "is not a valid query in its language" is left to
/// the backend that implements the language.
///
/// # Errors
///
/// - `invalidArguments` "if `where` is not well-formed (other than by using
///   an unknown operator), if exactly one of `language` and `expression` is
///   present".
/// - `unsupportedFilter` if "`where` uses a `$`-prefixed name that is not an
///   operator defined in this document; `language` is not in the account's
///   `languages` list; the expression uses `$text` and `supportsText` is
///   `false`; or an operator object's `$collation`" is not one of
///   [`COLLATIONS`].
/// - `queryTooComplex` for "an expression whose traversal depth exceeds the
///   advertised `maxDepth`".
pub fn check_search_filter(
    filter: &Value,
    kind: CandidateKind,
    cap: &GraphSearchCapability,
) -> Result<(), JmapError> {
    let present = |k: &str| filter.get(k).filter(|v| !v.is_null());
    let language = present("language");
    if language.is_some() != present("expression").is_some() {
        return Err(JmapError::invalid_arguments(
            "language and expression must be given together",
        ));
    }
    if language.is_some_and(|l| !cap.languages.iter().any(|x| l.as_str() == Some(x))) {
        return Err(JmapError::unsupported_filter());
    }
    let Some(w) = present("where") else {
        return Ok(());
    };
    let expr = Expression::parse(w, kind)?;
    let (mut text, mut bad_collation) = (false, false);
    visit(&expr, &mut |e| {
        text |= e.text.is_some();
        bad_collation |= e.fields.values().any(|t| {
            t.collation
                .as_deref()
                .is_some_and(|c| !COLLATIONS.contains(&c))
        });
    });
    if (text && !cap.supports_text) || bad_collation {
        return Err(JmapError::unsupported_filter());
    }
    let depth = expr.traversal_depth();
    if depth > cap.max_depth {
        let mut err = JmapError::custom(QUERY_TOO_COMPLEX);
        err.description = Some(format!(
            "traversal depth {depth} exceeds maxDepth {}",
            cap.max_depth
        ));
        return Err(err);
    }
    Ok(())
}

/// Call `f` on `e` and on every expression nested in it.
fn visit(e: &Expression, f: &mut impl FnMut(&Expression)) {
    f(e);
    let lists = e.and.iter().chain(&e.or).flatten();
    let edge = e.edge.iter().flat_map(|t| t.edge.iter().chain(&t.node));
    let reach = e.reachable.iter().flat_map(|t| &t.node);
    let boxed = e.not.iter().chain(&e.from).chain(&e.to).map(|b| &**b);
    for sub in lists.chain(edge).chain(reach).chain(boxed) {
        visit(sub, f);
    }
}

/// The nodes and edges of one Graph that a search may consider, and the
/// `where` evaluator over them (draft-atwood-jmap-graph-search-00 §The
/// Built-In Filter Language).
///
/// "The nodes and edges a search considers are exactly those the user
/// could fetch with `/get`" (§Visibility): build the view from those only,
/// for a projected Graph the contents visible under
/// draft-atwood-jmap-graph-projection-00, so "a node or edge the user
/// cannot see has no effect on any search result". An edge whose `fromId`
/// or `toId` is not in the view is never followed by `$edge` or
/// `$reachable`.
///
/// Field tests resolve their JSON Pointer against each object's serialized
/// form, which is its `/get` representation. Expressions are expected to
/// have passed [`check_search_filter`]; a `$collation` outside
/// [`COLLATIONS`] compares by code point.
///
/// `$text` is server-defined (§Full-Text Matching); here it is true if a
/// string-valued annotation contains the text under `i;unicode-casemap`.
///
/// ponytail: no memoization or work budget. Each candidate re-walks the
/// Graph, so nested traversal costs grow as the product of the node degrees
/// along the chain, bounded only by the advertised `maxDepth`. Upgrade
/// path: a step budget that answers `queryTooComplex` (§Resource
/// Consumption, a SHOULD).
#[derive(Debug)]
pub struct GraphView<'a> {
    nodes: HashMap<&'a Id, Value>,
    edges: HashMap<&'a Id, (&'a GraphEdge, Value)>,
    outgoing: HashMap<&'a Id, Vec<&'a GraphEdge>>,
    incoming: HashMap<&'a Id, Vec<&'a GraphEdge>>,
}

impl<'a> GraphView<'a> {
    /// A view of `nodes` and `edges`, the visible contents of one Graph.
    ///
    /// # Errors
    ///
    /// `serverFail` if a node or edge fails to serialize.
    pub fn new(nodes: &'a [GraphNode], edges: &'a [GraphEdge]) -> Result<Self, JmapError> {
        let nodes: HashMap<&Id, Value> = nodes
            .iter()
            .map(|n| Ok((&n.id, jmap_server::serialize_value(n)?)))
            .collect::<Result<_, JmapError>>()?;
        let mut outgoing: HashMap<&Id, Vec<&GraphEdge>> = HashMap::new();
        let mut incoming: HashMap<&Id, Vec<&GraphEdge>> = HashMap::new();
        for e in edges {
            if nodes.contains_key(&e.from_id) && nodes.contains_key(&e.to_id) {
                outgoing.entry(&e.from_id).or_default().push(e);
                incoming.entry(&e.to_id).or_default().push(e);
            }
        }
        let edges = edges
            .iter()
            .map(|e| Ok((&e.id, (e, jmap_server::serialize_value(e)?))))
            .collect::<Result<_, JmapError>>()?;
        Ok(Self {
            nodes,
            edges,
            outgoing,
            incoming,
        })
    }

    /// `true` if `expr` is true for the node `id` of the view; `false` for
    /// an id not in it.
    pub fn node_matches(&self, expr: &Expression, id: &Id) -> bool {
        let Some(json) = self.nodes.get(id) else {
            return false;
        };
        self.common(expr, json, |e| self.node_matches(e, id))
            && expr.edge.as_deref().is_none_or(|t| self.edge_test(t, id))
            && expr.reachable.as_deref().is_none_or(|t| self.reach(t, id))
    }

    /// `true` if `expr` is true for the edge `id` of the view; `false` for
    /// an id not in it.
    pub fn edge_matches(&self, expr: &Expression, id: &Id) -> bool {
        let Some((edge, json)) = self.edges.get(id) else {
            return false;
        };
        self.common(expr, json, |e| self.edge_matches(e, id))
            && expr
                .from
                .as_deref()
                .is_none_or(|e| self.node_matches(e, &edge.from_id))
            && expr
                .to
                .as_deref()
                .is_none_or(|e| self.node_matches(e, &edge.to_id))
    }

    /// The members every candidate kind has: field tests, the logical
    /// operators (each operand evaluated by `sub`) and `$text`.
    fn common(&self, expr: &Expression, json: &Value, sub: impl Fn(&Expression) -> bool) -> bool {
        expr.fields
            .iter()
            .all(|(pointer, t)| field_test(t, json.pointer(pointer)))
            && expr.and.as_ref().is_none_or(|v| v.iter().all(&sub))
            && expr.or.as_ref().is_none_or(|v| v.iter().any(&sub))
            && expr.not.as_deref().is_none_or(|e| !sub(e))
            && expr.text.as_ref().is_none_or(|t| text_matches(json, t))
    }

    /// The followable edges at node `id` in `direction`, each with the node
    /// at its other end.
    fn steps<'s>(
        &'s self,
        id: &Id,
        direction: Direction,
    ) -> impl Iterator<Item = (&'a GraphEdge, &'a Id)> + 's + use<'a, 's> {
        let (out, inc) = match direction {
            Direction::Out => (true, false),
            Direction::In => (false, true),
            Direction::Any => (true, true),
        };
        let list = |m: &'s HashMap<&'a Id, Vec<&'a GraphEdge>>, take: bool| {
            m.get(id).filter(|_| take).into_iter().flatten().copied()
        };
        let out = list(&self.outgoing, out).map(|e| (e, &e.to_id));
        let inc = list(&self.incoming, inc).map(|e| (e, &e.from_id));
        out.chain(inc)
    }

    /// `$edge`: "True if the candidate has at least one edge satisfying the
    /// EdgeTest."
    fn edge_test(&self, t: &EdgeTest, id: &Id) -> bool {
        self.steps(id, t.direction).any(|(edge, other)| {
            t.edge_type.as_ref().is_none_or(|ty| *ty == edge.edge_type)
                && t.edge
                    .as_ref()
                    .is_none_or(|e| self.edge_matches(e, &edge.id))
                && t.node.as_ref().is_none_or(|e| self.node_matches(e, other))
        })
    }

    /// `$reachable`: "A node is reachable if a path of 1 to `maxDepth`
    /// steps leads to it from the candidate. ... The candidate itself is
    /// reachable only if a path of at least one step returns to it."
    /// Breadth-first, so each node is tested once, at its shortest depth.
    fn reach(&self, t: &ReachTest, id: &Id) -> bool {
        let mut reached: HashSet<&Id> = HashSet::new();
        let mut frontier: Vec<&Id> = vec![id];
        let mut depth = 0;
        while depth < t.max_depth && !frontier.is_empty() {
            depth += 1;
            let mut next = Vec::new();
            for n in frontier {
                for (edge, other) in self.steps(n, t.direction) {
                    if t.edge_type.as_ref().is_some_and(|ty| *ty != edge.edge_type)
                        || !reached.insert(other)
                    {
                        continue;
                    }
                    if t.node.as_ref().is_none_or(|e| self.node_matches(e, other)) {
                        return true;
                    }
                    next.push(other);
                }
            }
            frontier = next;
        }
        false
    }
}

/// One field test against the value its pointer resolved to, `None` when
/// the field is missing (§Comparison Operators, §Arrays). "The test is true
/// if every operator in it is true."
fn field_test(t: &FieldTest, field: Option<&Value>) -> bool {
    let c = t.collation.as_deref();
    let eq = |v: &Value| each(field, v.is_array(), |f| json_eq(f, v, c));
    let in_ = |vs: &[Value]| vs.iter().any(eq);
    let ord = |b: &Bound, want: fn(Ordering) -> bool| {
        each(field, false, |f| bound_cmp(f, b, c).is_some_and(want))
    };
    let string = |test: &dyn Fn(&str) -> bool| each(field, false, |f| f.as_str().is_some_and(test));
    t.eq.as_ref().is_none_or(eq)
        && t.ne.as_ref().is_none_or(|v| !eq(v))
        && t.gt.as_ref().is_none_or(|b| ord(b, Ordering::is_gt))
        && t.gte.as_ref().is_none_or(|b| ord(b, Ordering::is_ge))
        && t.lt.as_ref().is_none_or(|b| ord(b, Ordering::is_lt))
        && t.lte.as_ref().is_none_or(|b| ord(b, Ordering::is_le))
        && t.in_.as_deref().is_none_or(in_)
        && t.nin.as_deref().is_none_or(|vs| !in_(vs))
        && t.exists.is_none_or(|x| x == field.is_some())
        && t.prefix
            .as_ref()
            .is_none_or(|p| string(&|s| prepare(s, c).starts_with(&*prepare(p, c))))
        && t.contains
            .as_ref()
            .is_none_or(|p| string(&|s| prepare(s, c).contains(&*prepare(p, c))))
}

/// `test` applied to the field: "When the field is an array and the operand
/// ... is not an array, the operator is true if it is true for at least one
/// element of the field. ... Elements are not searched recursively." A
/// missing field fails every test.
fn each(field: Option<&Value>, operand_is_array: bool, test: impl Fn(&Value) -> bool) -> bool {
    match field {
        None => false,
        Some(Value::Array(a)) if !operand_is_array => a.iter().any(test),
        Some(f) => test(f),
    }
}

/// "Two JSON values are equal when they have the same JSON type and: both
/// are `null`; both are the same Boolean; both are numbers with the same
/// numeric value; both are strings that are equal under the test's
/// collation; both are arrays of the same length whose elements are
/// pairwise equal; or both are objects with the same member names whose
/// values are pairwise equal."
fn json_eq(a: &Value, b: &Value, c: Option<&str>) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => num_cmp(x, y) == Some(Ordering::Equal),
        (Value::String(x), Value::String(y)) => prepare(x, c) == prepare(y, c),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| json_eq(x, y, c))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_eq(v, w, c)))
        }
        _ => false,
    }
}

/// `$gt`, `$gte`, `$lt`, `$lte`: "The field and the operand are both
/// numbers and compare numerically as stated, or are both strings and
/// compare as stated under the test's collation. Otherwise false."
fn bound_cmp(field: &Value, b: &Bound, c: Option<&str>) -> Option<Ordering> {
    match (field, b) {
        (Value::Number(x), Bound::Number(y)) => num_cmp(x, y),
        (Value::String(x), Bound::String(y)) => Some(prepare(x, c).cmp(&prepare(y, c))),
        _ => None,
    }
}

/// Numeric order, exact for integers and by `f64` otherwise.
fn num_cmp(a: &Number, b: &Number) -> Option<Ordering> {
    if let (Some(x), Some(y)) = (a.as_i64(), b.as_i64()) {
        return Some(x.cmp(&y));
    }
    if let (Some(x), Some(y)) = (a.as_u64(), b.as_u64()) {
        return Some(x.cmp(&y));
    }
    a.as_f64()?.partial_cmp(&b.as_f64()?)
}

/// `s` prepared for octet comparison under collation `c`: unchanged for
/// code-point comparison (no `$collation`, §Collation) and `i;octet`, whose
/// octet order over UTF-8 is code-point order. `i;ascii-casemap`: "the
/// lower-case letters (octet values 97-122) in each input string are
/// changed to upper case" (RFC 4790 §9.2.1). `i;unicode-casemap`: the RFC
/// 5051 §2 "titlecased canonicalized UTF-8" form, each code point's simple
/// titlecase mapping, then its compatibility decomposition, code point by
/// code point. "Following the above preparation process on each string, the
/// equality, ordering, and substring operations are as for i;octet."
fn prepare<'s>(s: &'s str, c: Option<&str>) -> Cow<'s, str> {
    match c {
        Some("i;ascii-casemap") => Cow::Owned(s.to_ascii_uppercase()),
        Some("i;unicode-casemap") => {
            let case = CaseMapper::new();
            let nfkd = DecomposingNormalizerBorrowed::new_nfkd();
            Cow::Owned(
                s.chars()
                    .flat_map(|ch| nfkd.normalize_iter(std::iter::once(case.simple_titlecase(ch))))
                    .collect(),
            )
        }
        _ => Cow::Borrowed(s),
    }
}

/// `$text` over the candidate `json`: a string-valued annotation contains
/// `text` under `i;unicode-casemap`.
fn text_matches(json: &Value, text: &str) -> bool {
    let needle = prepare(text, Some("i;unicode-casemap"));
    json.get("annotations")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(Value::as_str)
        .any(|s| prepare(s, Some("i;unicode-casemap")).contains(&*needle))
}
