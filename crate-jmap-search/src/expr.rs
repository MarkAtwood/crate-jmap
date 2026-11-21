//! The filter language AST (docs/specs/jmap-search.md §The Filter
//! Language): [`Expression`], [`FieldTest`], [`Bound`], and the parse error
//! [`WhereError`].
//!
//! [`FieldTest`], [`Bound`], [`WhereError`] and [`is_json_pointer`] are
//! shared with the built-in language of draft-atwood-jmap-graph-search-00,
//! whose expressions are these plus structural operators; `jmap-graph-types`
//! builds its own expression type from them.

use std::collections::BTreeMap;
use std::fmt;

use jmap_types::JmapError;
use serde_json::{Map, Number, Value};

/// Why a `where` value was rejected (docs/specs/jmap-search.md §Errors).
///
/// Each variant names its error type; `From<WhereError> for JmapError`
/// builds that error.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhereError {
    /// "`where` is not well-formed (other than by using an unknown
    /// operator)": an `invalidArguments` error. The string describes the
    /// defect.
    Malformed(String),
    /// "`where` uses a `$`-prefixed name that is not an operator defined in
    /// this document": an `unsupportedFilter` error. The string is that
    /// name.
    UnknownOperator(String),
}

impl WhereError {
    /// A [`WhereError::Malformed`] describing the defect `why`.
    pub fn malformed(why: impl Into<String>) -> Self {
        Self::Malformed(why.into())
    }
}

impl fmt::Display for WhereError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "malformed where expression: {why}"),
            Self::UnknownOperator(name) => write!(f, "unknown where operator {name}"),
        }
    }
}

impl std::error::Error for WhereError {}

impl From<WhereError> for JmapError {
    fn from(e: WhereError) -> Self {
        match e {
            WhereError::Malformed(_) => JmapError::invalid_arguments(e.to_string()),
            WhereError::UnknownOperator(_) => JmapError::unsupported_filter(),
        }
    }
}

/// A filter expression (docs/specs/jmap-search.md §Expressions).
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
    /// `/keywords/$flagged`. "A pointer that does not resolve to a value
    /// leaves the field **missing**."
    pub fields: BTreeMap<String, FieldTest>,
    /// `$and`: "True if every expression in the array is true. An empty
    /// array is true."
    pub and: Option<Vec<Expression>>,
    /// `$or`: "True if at least one expression in the array is true. An
    /// empty array is false."
    pub or: Option<Vec<Expression>>,
    /// `$not`: "True if the expression is false."
    pub not: Option<Box<Expression>>,
    /// `$text`: "True if the candidate matches the given text." Matching is
    /// server-defined.
    pub text: Option<String>,
}

/// The operators of one field test (§Field Tests, §Comparison Operators).
///
/// A plain value is "shorthand for `{"$eq": value}`" and parses to a test
/// with only [`eq`](Self::eq) set; both forms compare strings by code point,
/// so they are the same test. Every set operator must be true;
/// [`FieldTest::default`] has none and is always true.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FieldTest {
    /// `$eq`: "The field is present and equal to the operand."
    /// `Some(Value::Null)` is an explicit `null`, which "is true only for an
    /// explicit `null`".
    pub eq: Option<Value>,
    /// `$ne`: "Not `$eq`. True when the field is missing."
    pub ne: Option<Value>,
    /// `$gt`.
    pub gt: Option<Bound>,
    /// `$gte`.
    pub gte: Option<Bound>,
    /// `$lt`.
    pub lt: Option<Bound>,
    /// `$lte`.
    pub lte: Option<Bound>,
    /// `$in`: "The field is `$eq` to at least one element of the operand."
    pub in_: Option<Vec<Value>>,
    /// `$nin`: "Not `$in`. True when the field is missing."
    pub nin: Option<Vec<Value>>,
    /// `$exists`: "If `true`, the field is present (its value may be
    /// `null`); if `false`, it is missing."
    pub exists: Option<bool>,
    /// `$prefix`: "The field is a string that begins with the operand under
    /// the test's collation."
    pub prefix: Option<String>,
    /// `$contains`: "The field is a string that contains the operand as a
    /// substring under the test's collation."
    pub contains: Option<String>,
    /// `$collation`: "Not a test. Names the collation algorithm used by this
    /// operator object's string operators." `None` means code-point
    /// comparison (§Collation).
    pub collation: Option<String>,
}

/// The operand of `$gt`, `$gte`, `$lt` and `$lte`: "(Number or String)".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    /// Compared numerically with a number field.
    Number(Number),
    /// Compared under the test's collation with a string field.
    String(String),
}

impl Expression {
    /// Parse and check a `where` value (docs/specs/jmap-search.md
    /// §The where `FilterCondition` Property, well-formedness).
    ///
    /// # Errors
    ///
    /// [`WhereError::UnknownOperator`] for a `$`-prefixed expression member
    /// or operator-object name this language does not define (the
    /// draft-atwood-jmap-graph-search-00 structural operators included),
    /// otherwise [`WhereError::Malformed`] when the value is not
    /// well-formed. The first defect found is reported.
    pub fn parse(value: &Value) -> Result<Self, WhereError> {
        let obj = value
            .as_object()
            .ok_or_else(|| WhereError::malformed("an expression must be a JSON object"))?;
        let mut expr = Self::default();
        for (name, v) in obj {
            match name.as_str() {
                "$and" => expr.and = Some(parse_list(v, name)?),
                "$or" => expr.or = Some(parse_list(v, name)?),
                "$not" => expr.not = Some(Box::new(Self::parse(v)?)),
                "$text" => expr.text = Some(operand_string(v, name)?),
                n if n.starts_with('$') => return Err(WhereError::UnknownOperator(n.to_owned())),
                n if is_json_pointer(n) => {
                    expr.fields.insert(n.to_owned(), FieldTest::parse(v)?);
                }
                n => {
                    return Err(WhereError::malformed(format!(
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
    /// operators is always true but has no wire form (`{}` is shorthand for
    /// `{"$eq": {}}`), so it is left out, which keeps the expression's
    /// meaning because every member must be true.
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
        if let Some(t) = &self.text {
            m.insert("$text".to_owned(), Value::String(t.clone()));
        }
        Value::Object(m)
    }

    /// The nesting depth (docs/specs/jmap-search.md §Nesting Depth): "0 if
    /// it has no logical operator, and otherwise 1 more than the largest
    /// nesting depth of the expressions that are operands of its `$and`,
    /// `$or`, and `$not` members." A depth above the advertised
    /// [`SearchCapability::max_depth`](crate::SearchCapability::max_depth)
    /// is [`QUERY_TOO_COMPLEX`](crate::QUERY_TOO_COMPLEX).
    pub fn depth(&self) -> u64 {
        if self.and.is_none() && self.or.is_none() && self.not.is_none() {
            return 0;
        }
        let operands = self.and.iter().chain(&self.or).flatten();
        let deepest = operands
            .chain(self.not.as_deref())
            .map(Self::depth)
            .max()
            .unwrap_or(0);
        deepest.saturating_add(1)
    }

    /// Call `f` on this expression and on every expression nested in it.
    pub fn visit(&self, f: &mut impl FnMut(&Expression)) {
        f(self);
        let operands = self.and.iter().chain(&self.or).flatten();
        for sub in operands.chain(self.not.as_deref()) {
            sub.visit(f);
        }
    }
}

impl FieldTest {
    /// A test with only `$eq` set: the meaning of a plain value.
    pub fn equal_to(value: Value) -> Self {
        Self {
            eq: Some(value),
            ..Self::default()
        }
    }

    /// Parse a field test's value (§Field Tests): "An **operator object**
    /// is a JSON object at least one of whose member names begins with `$`.
    /// [...] Any other value is shorthand for `{"$eq": value}`."
    ///
    /// # Errors
    ///
    /// [`WhereError::UnknownOperator`] for a `$`-prefixed name that is not
    /// a comparison operator, [`WhereError::Malformed`] for an operator
    /// object that mixes `$`-prefixed and other names or an operand of the
    /// wrong type.
    pub fn parse(v: &Value) -> Result<Self, WhereError> {
        let ops = match v.as_object() {
            Some(obj) if obj.keys().any(|k| k.starts_with('$')) => obj,
            _ => return Ok(Self::equal_to(v.clone())),
        };
        let mut t = Self::default();
        for (name, v) in ops {
            match name.as_str() {
                "$eq" => t.eq = Some(v.clone()),
                "$ne" => t.ne = Some(v.clone()),
                "$gt" => t.gt = Some(parse_bound(v, name)?),
                "$gte" => t.gte = Some(parse_bound(v, name)?),
                "$lt" => t.lt = Some(parse_bound(v, name)?),
                "$lte" => t.lte = Some(parse_bound(v, name)?),
                "$in" => t.in_ = Some(operand_array(v, name)?.clone()),
                "$nin" => t.nin = Some(operand_array(v, name)?.clone()),
                "$exists" => {
                    t.exists = Some(
                        v.as_bool()
                            .ok_or_else(|| WhereError::malformed("$exists must be a Boolean"))?,
                    );
                }
                "$prefix" => t.prefix = Some(operand_string(v, name)?),
                "$contains" => t.contains = Some(operand_string(v, name)?),
                "$collation" => t.collation = Some(operand_string(v, name)?),
                n if n.starts_with('$') => return Err(WhereError::UnknownOperator(n.to_owned())),
                n => {
                    return Err(WhereError::malformed(format!(
                        "operator object mixes $-prefixed names with {n:?}"
                    )));
                }
            }
        }
        Ok(t)
    }

    /// The operator-object form of the test (never the plain-value
    /// shorthand); `{}` for [`FieldTest::default`].
    pub fn to_value(&self) -> Value {
        let bound = |b: &Bound| match b {
            Bound::Number(n) => Value::Number(n.clone()),
            Bound::String(s) => Value::String(s.clone()),
        };
        let entries = [
            ("$eq", self.eq.clone()),
            ("$ne", self.ne.clone()),
            ("$gt", self.gt.as_ref().map(bound)),
            ("$gte", self.gte.as_ref().map(bound)),
            ("$lt", self.lt.as_ref().map(bound)),
            ("$lte", self.lte.as_ref().map(bound)),
            ("$in", self.in_.clone().map(Value::Array)),
            ("$nin", self.nin.clone().map(Value::Array)),
            ("$exists", self.exists.map(Value::Bool)),
            ("$prefix", self.prefix.clone().map(Value::String)),
            ("$contains", self.contains.clone().map(Value::String)),
            ("$collation", self.collation.clone().map(Value::String)),
        ];
        Value::Object(
            entries
                .into_iter()
                .filter_map(|(k, v)| v.map(|v| (k.to_owned(), v)))
                .collect(),
        )
    }
}

/// `true` if `s` is an RFC 6901 JSON Pointer: `"" / *( "/" reference-token
/// )`, where `~` is only allowed as `~0` or `~1`.
pub fn is_json_pointer(s: &str) -> bool {
    (s.is_empty() || s.starts_with('/')) && s.split('~').skip(1).all(|t| t.starts_with(['0', '1']))
}

/// The String operand of the operator `name`.
fn operand_string(v: &Value, name: &str) -> Result<String, WhereError> {
    v.as_str()
        .map(str::to_owned)
        .ok_or_else(|| WhereError::malformed(format!("{name} must be a String")))
}

fn operand_array<'a>(v: &'a Value, name: &str) -> Result<&'a Vec<Value>, WhereError> {
    v.as_array()
        .ok_or_else(|| WhereError::malformed(format!("{name} must be an Array")))
}

fn parse_list(v: &Value, name: &str) -> Result<Vec<Expression>, WhereError> {
    operand_array(v, name)?
        .iter()
        .map(Expression::parse)
        .collect()
}

fn parse_bound(v: &Value, name: &str) -> Result<Bound, WhereError> {
    match v {
        Value::Number(n) => Ok(Bound::Number(n.clone())),
        Value::String(s) => Ok(Bound::String(s.clone())),
        _ => Err(WhereError::malformed(format!(
            "{name} must be a Number or String"
        ))),
    }
}
