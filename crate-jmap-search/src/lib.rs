//! JMAP Search (`urn:ietf:params:jmap:search`): one filter language for
//! the `/query` method of every JMAP data type, specified by the local
//! draft `docs/specs/jmap-search.md` (draft-atwood-jmap-search-00) in the
//! crate-jmap repository.
//!
//! A server lists the data types it supports in the account's
//! [`SearchCapability`], and the FilterCondition of each listed type gains
//! a `where` property: a JSON expression of field tests (JSON Pointers into
//! the object's `/get` representation), comparison operators, logical
//! operators, `$collation` and `$text`. The language is the type-neutral
//! part of the built-in language of draft-atwood-jmap-graph-search-00;
//! Graph search (`jmap-graph-types`, `jmap-graph-server`) builds its
//! structural operators on the pieces here.
//!
//! - Capability: [`JMAP_SEARCH_URI`], [`SearchCapability`],
//!   [`DataTypeSearchInfo`], and the [`QUERY_TOO_COMPLEX`] error name.
//! - AST: [`Expression`] ([`Expression::parse`], [`Expression::to_value`],
//!   [`Expression::depth`]), [`FieldTest`], [`Bound`], [`WhereError`]
//!   (→ `invalidArguments` / `unsupportedFilter`), [`is_json_pointer`].
//! - Checks: [`check_where`] (one `where` against the capability),
//!   [`check_where_filter`] (every `where` of a filter, with the `using`
//!   gate), [`where_values`], [`filter_uses_where`] (for clients choosing
//!   `using`), [`COLLATIONS`].
//! - Evaluator (feature `eval`): the [`Candidate`] view of one object,
//!   `Expression::matches`, `FieldTest::matches` and [`collate`].
//!
//! # Example
//!
//! ```
//! use jmap_search::{DataTypeSearchInfo, SearchCapability, check_where};
//! use serde_json::json;
//!
//! let cap = SearchCapability::new(4).with_data_type("Email", DataTypeSearchInfo::new(true));
//! let expr = check_where(&json!({ "/size": { "$gt": 1000 } }), "Email", &cap).unwrap();
//! assert_eq!(expr.depth(), 0);
//! // A type not listed in dataTypes does not accept `where`.
//! let err = check_where(&json!({}), "Mailbox", &cap).unwrap_err();
//! assert_eq!(err.error_type.to_string(), "unsupportedFilter");
//! ```
//!
//! No async and no I/O; the evaluator holds no index and scans what the
//! caller hands it.

#![forbid(unsafe_code)]

mod capability;
mod check;
#[cfg(feature = "eval")]
mod eval;
mod expr;

pub use capability::{DataTypeSearchInfo, JMAP_SEARCH_URI, QUERY_TOO_COMPLEX, SearchCapability};
pub use check::{
    COLLATIONS, check_where, check_where_filter, filter_uses_where, is_supported_collation,
    where_values,
};
#[cfg(feature = "eval")]
pub use eval::{Candidate, collate};
pub use expr::{Bound, Expression, FieldTest, WhereError, is_json_pointer};
