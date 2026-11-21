//! The `urn:ietf:params:jmap:search` capability (docs/specs/jmap-search.md
//! §Capability) and the `queryTooComplex` error name (§Errors).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Capability URI for JMAP Search (docs/specs/jmap-search.md §Capability).
///
/// The session-level value "is an empty JSON object `{}`" and is not
/// modelled; the account-level value is [`SearchCapability`]. A request
/// that sets `where` carries this URI in `using` (§Opting In a Data Type).
pub const JMAP_SEARCH_URI: &str = "urn:ietf:params:jmap:search";

/// The method-level error type for a query that "is valid, but the server
/// will not evaluate it because it exceeds a server limit, such as
/// `maxDepth` or a limit on evaluation time or work"
/// (docs/specs/jmap-search.md §Errors, the same error as
/// draft-atwood-jmap-graph-search-00 §Errors).
/// [`JmapError`](jmap_types::JmapError) carries any type string, so this
/// only names it.
pub const QUERY_TOO_COMPLEX: &str = "queryTooComplex";

/// Account-level capability object for `"urn:ietf:params:jmap:search"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:search"]`
/// (docs/specs/jmap-search.md §Account-Level Capability Object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCapability {
    /// "A map from the name of a data type, as it appears in the method
    /// names of that type (for example `Email` or `Message`), to the search
    /// support for that type on this account. Only the types listed here
    /// accept `where`."
    pub data_types: BTreeMap<String, DataTypeSearchInfo>,
    /// "The largest nesting depth the server accepts in an expression."
    /// Compare with [`Expression::depth`](crate::Expression::depth).
    pub max_depth: u64,
    /// Catch-all for vendor / site / private extension fields ("Clients
    /// ignore fields they do not recognize"), preserved across a
    /// deserialize/serialize round trip per the workspace
    /// extras-preservation policy.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl SearchCapability {
    /// A capability with no data types and the given `maxDepth`; add
    /// types with [`Self::with_data_type`].
    pub fn new(max_depth: u64) -> Self {
        Self {
            max_depth,
            ..Self::default()
        }
    }

    /// This capability with `data_type` listed in `dataTypes` as `info`.
    #[must_use]
    pub fn with_data_type(mut self, data_type: &str, info: DataTypeSearchInfo) -> Self {
        self.data_types.insert(data_type.to_owned(), info);
        self
    }

    /// The `dataTypes` entry of `data_type`, `None` when the type does not
    /// accept `where` on the account.
    pub fn data_type(&self, data_type: &str) -> Option<&DataTypeSearchInfo> {
        self.data_types.get(data_type)
    }
}

/// The search support of one data type: a value of
/// [`SearchCapability::data_types`] (docs/specs/jmap-search.md
/// §Account-Level Capability Object, [`DataTypeSearchInfo`]).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataTypeSearchInfo {
    /// "`true` if the server supports the `$text` operator for this type."
    pub supports_text: bool,
    /// Catch-all for unknown fields, as [`SearchCapability::extra`].
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl DataTypeSearchInfo {
    /// The info with the given `supportsText`.
    pub fn new(supports_text: bool) -> Self {
        Self {
            supports_text,
            extra: Map::new(),
        }
    }
}
