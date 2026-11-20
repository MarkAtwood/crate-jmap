//! The evaluator (feature `eval`): is an [`Expression`] true for one
//! candidate? (docs/specs/jmap-search.md §The Filter Language.)
//!
//! Expressions are expected to have passed
//! [`check_where`](crate::check_where); a `$collation` outside
//! [`COLLATIONS`](crate::COLLATIONS) compares by code point.

use std::borrow::Cow;
use std::cmp::Ordering;

use icu_casemap::CaseMapper;
use icu_normalizer::DecomposingNormalizerBorrowed;
use serde_json::{Number, Value};

use crate::{Bound, Expression, FieldTest};

/// One object being tested against a filter, seen as its `/get`
/// representation (docs/specs/jmap-search.md §Addressable Properties).
///
/// The server builds the view from what the caller may see: "each is tested
/// against the representation that user would receive" (§Visibility).
pub trait Candidate {
    /// The value at the RFC 6901 `pointer` in the candidate's
    /// representation, `None` when the pointer does not resolve ("leaves the
    /// field **missing**").
    fn field(&self, pointer: &str) -> Option<&Value>;

    /// `$text`: "True if the candidate matches the given text. What
    /// constitutes a match [...] is server-defined" (§Full-Text Matching).
    fn matches_text(&self, text: &str) -> bool;
}

/// A bare JSON representation: fields resolve with
/// [`Value::pointer`], and `$text` is true if some string anywhere in the
/// value (member names excluded) contains the text under
/// `i;unicode-casemap`.
impl Candidate for Value {
    fn field(&self, pointer: &str) -> Option<&Value> {
        self.pointer(pointer)
    }

    fn matches_text(&self, text: &str) -> bool {
        fn any_string(v: &Value, test: &dyn Fn(&str) -> bool) -> bool {
            match v {
                Value::String(s) => test(s),
                Value::Array(a) => a.iter().any(|v| any_string(v, test)),
                Value::Object(m) => m.values().any(|v| any_string(v, test)),
                _ => false,
            }
        }
        let needle = collate(text, Some("i;unicode-casemap"));
        any_string(self, &|s| {
            collate(s, Some("i;unicode-casemap")).contains(&*needle)
        })
    }
}

impl Expression {
    /// `true` if the expression is true for `candidate`: every field test,
    /// the logical operators and `$text`.
    pub fn matches<C: Candidate + ?Sized>(&self, candidate: &C) -> bool {
        self.fields
            .iter()
            .all(|(pointer, t)| t.matches(candidate.field(pointer)))
            && self
                .and
                .as_ref()
                .is_none_or(|v| v.iter().all(|e| e.matches(candidate)))
            && self
                .or
                .as_ref()
                .is_none_or(|v| v.iter().any(|e| e.matches(candidate)))
            && self.not.as_deref().is_none_or(|e| !e.matches(candidate))
            && self.text.as_ref().is_none_or(|t| candidate.matches_text(t))
    }
}

impl FieldTest {
    /// `true` if the test is true for `field`, the value its pointer
    /// resolved to, `None` when the field is missing (§Comparison
    /// Operators, §Arrays). "The test is true if every operator in it is
    /// true."
    pub fn matches(&self, field: Option<&Value>) -> bool {
        let c = self.collation.as_deref();
        let eq = |v: &Value| each(field, v.is_array(), |f| json_eq(f, v, c));
        let in_ = |vs: &[Value]| vs.iter().any(eq);
        let ord = |b: &Bound, want: fn(Ordering) -> bool| {
            each(field, false, |f| bound_cmp(f, b, c).is_some_and(want))
        };
        let string =
            |test: &dyn Fn(&str) -> bool| each(field, false, |f| f.as_str().is_some_and(test));
        self.eq.as_ref().is_none_or(eq)
            && self.ne.as_ref().is_none_or(|v| !eq(v))
            && self.gt.as_ref().is_none_or(|b| ord(b, Ordering::is_gt))
            && self.gte.as_ref().is_none_or(|b| ord(b, Ordering::is_ge))
            && self.lt.as_ref().is_none_or(|b| ord(b, Ordering::is_lt))
            && self.lte.as_ref().is_none_or(|b| ord(b, Ordering::is_le))
            && self.in_.as_deref().is_none_or(in_)
            && self.nin.as_deref().is_none_or(|vs| !in_(vs))
            && self.exists.is_none_or(|x| x == field.is_some())
            && self
                .prefix
                .as_ref()
                .is_none_or(|p| string(&|s| collate(s, c).starts_with(&*collate(p, c))))
            && self
                .contains
                .as_ref()
                .is_none_or(|p| string(&|s| collate(s, c).contains(&*collate(p, c))))
    }
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
        (Value::String(x), Value::String(y)) => collate(x, c) == collate(y, c),
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
        (Value::String(x), Bound::String(y)) => Some(collate(x, c).cmp(&collate(y, c))),
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

/// `s` prepared for octet comparison under the collation `c`, so that
/// equality, ordering, prefix and substring tests on prepared strings are
/// those of the collation.
///
/// Unchanged for code-point comparison (`None`, §Collation) and `i;octet`,
/// whose octet order over UTF-8 is code-point order. `i;ascii-casemap`:
/// "the lower-case letters (octet values 97-122) in each input string are
/// changed to upper case" (RFC 4790 §9.2.1). `i;unicode-casemap`: the RFC
/// 5051 §2 "titlecased canonicalized UTF-8" form, each code point's simple
/// titlecase mapping, then its compatibility decomposition, code point by
/// code point. "Following the above preparation process on each string, the
/// equality, ordering, and substring operations are as for i;octet." Any
/// other name compares by code point.
pub fn collate<'s>(s: &'s str, c: Option<&str>) -> Cow<'s, str> {
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
