//! JMAP Object History data types (draft-ietf-jmap-object-history-03, with
//! the -04 editor's copy changes listed in docs/design/object-history.md
//! §2a).
//!
//! Object history (`urn:ietf:params:jmap:object-history`) defines no
//! methods of its own. It adds history arguments to standard `Foo/get`
//! ([`HistoryGetArguments`], answered with [`HistoryEntry`] list entries
//! carrying [`ObjectHistoryInfo`]), a search over destroyed objects to
//! `Foo/query` and `Foo/queryChanges` ([`DestroyedQueryArguments`]), and a
//! `restore` argument to `Foo/set` (answered in [`RestoreOutcome`]), for
//! every data type whose "JMAP Data Types" registration says "Supports
//! History: Yes" ([`is_history_type`], [`HistoryObject`]).
//!
//! This crate is types-only: no method handlers, no async, no network I/O.
//! It sits between `jmap-types` and `jmap-server` / the data-type crates /
//! `jmap-object-history-server` / `jmap-object-history-client`.
//!
//! # Example
//!
//! ```rust
//! use jmap_object_history_types::{HistoryEntry, ObjectHistoryInfo};
//! use serde_json::{Map, Value, json};
//!
//! // A replaced version, as in the §3.2.1 example.
//! let entry: HistoryEntry<Map<String, Value>> = serde_json::from_value(json!({
//!     "id": "c1",
//!     "name": {"full": "Joe Bloggs"},
//!     "objectHistory": {
//!         "version": 2,
//!         "replaced": "2025-03-01T09:00:00Z",
//!         "hasOlder": true,
//!         "expires": "2025-03-31T09:00:00Z"
//!     }
//! })).unwrap();
//! assert_eq!(entry.object_history.version, 2);
//! assert!(entry.object_history.has_older);
//! assert_eq!(entry.object["id"], "c1");
//! ```

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use jmap_types::data_types::data_type;
use jmap_types::{GetObject, Id, JmapObject, PartialObject, SetError, UTCDate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

mod serde_util;

use serde_util::{empty_as_null, null_as_empty};

/// The object-history capability URI (§7.1).
pub const JMAP_OBJECT_HISTORY_URI: &str = "urn:ietf:params:jmap:object-history";

/// A data type the history extensions apply to (§7.2 "Supports History").
///
/// Implemented exactly for the types [`is_history_type`] accepts: the
/// "Supports History: Yes" rows of the `jmap-types` data-type registry
/// (§7.2 Table 1 and Mark's drafts, docs/design/DECISIONS.md decision 14)
/// and the types on [`PENDING_HISTORY_REGISTRATION`]. Each data-type crate
/// implements it for its own types, and tests that its impls match the
/// registry. Never implement it for a "No" type.
pub trait HistoryObject: GetObject {}

/// Types given history ahead of their registration (decisions 5 and 23).
///
/// Empty: Task and TaskList, the types this list held, are now registry
/// rows with "Supports History: Yes" from crate-jmap's local Tasks profile
/// (docs/specs/jmap-tasks-local.md, decision 52; bd JMAP-tna1.3.26). Kept
/// so that callers naming it still compile.
pub const PENDING_HISTORY_REGISTRATION: &[&str] = &[];

/// Whether the history extensions apply to `type_name`: a registry row
/// with "Supports History: Yes", or a type on
/// [`PENDING_HISTORY_REGISTRATION`].
///
/// For consumers building `supportedTypes`, and for each data-type
/// crate's [`HistoryObject`] cross-check test. An unknown name is `false`.
pub fn is_history_type(type_name: &str) -> bool {
    data_type(type_name).is_some_and(|e| e.supports_history)
        || PENDING_HISTORY_REGISTRATION.contains(&type_name)
}

/// The Session `capabilities` value for the URI: it "MUST be an empty
/// object" (§2.1). Unknown members are kept in [`extra`](Self::extra).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectHistoryCapability {
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl ObjectHistoryCapability {
    /// The empty capability object.
    pub fn new() -> Self {
        Self::default()
    }
}

/// The `accountCapabilities` value for the URI (§2.1), which "MUST
/// contain" both properties; both are always serialized.
///
/// A server builds the value it advertises with [`Self::new`]; a client
/// deserializes it from the Session object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectHistoryAccountCapability {
    /// `maxHistoryDuration`: "UnsignedInt|null", in seconds; the longest
    /// time the server retains a replaced or destroyed version, or `None`
    /// for no fixed limit.
    #[serde(default)]
    pub max_history_duration: Option<u64>,
    /// `supportedTypes`: "String[]", the data types with history in this
    /// account. Clients "MUST ignore type names they do not recognise".
    pub supported_types: Vec<String>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl ObjectHistoryAccountCapability {
    /// The capability with `maxHistoryDuration` (in seconds, or `None`
    /// for no fixed limit) and `supportedTypes`, the two properties §2.1
    /// says it "MUST contain", and no unknown members.
    ///
    /// List in `supported_types` only types that [`is_history_type`]
    /// accepts and the backend keeps history for.
    ///
    /// ```
    /// use jmap_object_history_types::ObjectHistoryAccountCapability;
    ///
    /// let cap = ObjectHistoryAccountCapability::new(
    ///     Some(2_592_000),
    ///     vec!["ContactCard".to_owned(), "Email".to_owned()],
    /// );
    /// assert!(cap.supports_type("Email"));
    /// assert_eq!(
    ///     serde_json::to_value(&cap)?,
    ///     serde_json::json!({
    ///         "maxHistoryDuration": 2592000,
    ///         "supportedTypes": ["ContactCard", "Email"]
    ///     })
    /// );
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    pub fn new(max_history_duration: Option<u64>, supported_types: Vec<String>) -> Self {
        Self {
            max_history_duration,
            supported_types,
            extra: Map::new(),
        }
    }

    /// Whether `supportedTypes` lists `O`.
    pub fn supports<O: JmapObject>(&self) -> bool {
        self.supports_type(O::TYPE_NAME)
    }

    /// Whether `supportedTypes` lists `type_name`.
    pub fn supports_type(&self, type_name: &str) -> bool {
        self.supported_types.iter().any(|t| t == type_name)
    }
}

/// The `objectHistory` property of a history `Foo/get` entry (§3.1).
///
/// `replaced` and `expires` are always serialized: `null` marks the live
/// version.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectHistoryInfo {
    /// `version`: the version number; the live version has the highest.
    pub version: u64,
    /// `replaced`: when this version was superseded by an update or a
    /// destroy, or `None` for the live version.
    pub replaced: Option<UTCDate>,
    /// `hasOlder`: whether the server retains a version older than this
    /// one, returned or not.
    pub has_older: bool,
    /// `expires`: when the server expects to discard this version, or
    /// `None` (always for the live version).
    pub expires: Option<UTCDate>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl ObjectHistoryInfo {
    /// The info of the live version `version`: `replaced` and `expires`
    /// null.
    pub fn live(version: u64, has_older: bool) -> Self {
        Self {
            version,
            replaced: None,
            has_older,
            expires: None,
            extra: Map::new(),
        }
    }

    /// The info of a version replaced at `replaced`, which the server
    /// expects to discard at `expires`.
    pub fn replaced(
        version: u64,
        replaced: UTCDate,
        has_older: bool,
        expires: Option<UTCDate>,
    ) -> Self {
        Self {
            version,
            replaced: Some(replaced),
            has_older,
            expires,
            extra: Map::new(),
        }
    }
}

/// One history `Foo/get` list entry: an ordinary Foo plus `objectHistory`
/// (§3).
///
/// There is no `extra` of its own: `T`'s flattened catch-all already keeps
/// unknown keys, and two would compete for them.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry<T> {
    /// The object as this version stored it.
    #[serde(flatten)]
    pub object: T,
    /// `objectHistory`.
    #[serde(rename = "objectHistory")]
    pub object_history: ObjectHistoryInfo,
}

impl<T> HistoryEntry<T> {
    /// `object` with `object_history`.
    pub fn new(object: T, object_history: ObjectHistoryInfo) -> Self {
        Self {
            object,
            object_history,
        }
    }
}

/// The `Foo/get` arguments object history adds (§3), with the spec's
/// defaults. Each is left out of the serialized form when it has its
/// default.
///
/// When both flags are false the four `history*` arguments are ignored
/// (§3), so a server reads the flags first.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryGetArguments {
    /// `includeReplaced` (default false): also return replaced versions.
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_replaced: bool,
    /// `includeDestroyed` (default false): also return destroyed objects.
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_destroyed: bool,
    /// `historyAfter` (default null): only versions replaced strictly
    /// after this time; the live version is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_after: Option<UTCDate>,
    /// `historyBefore` (default null): only versions replaced strictly
    /// before this time; the live version is dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_before: Option<UTCDate>,
    /// `historySkip` (default 0): entries to skip per id, newest first.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub history_skip: u64,
    /// `historyLimit` (default null): the most entries to return per id,
    /// the most recent ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_limit: Option<u64>,
    /// Other arguments, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl HistoryGetArguments {
    /// Every argument at its default.
    pub fn new() -> Self {
        Self::default()
    }
}

/// The `Foo/query` and `Foo/queryChanges` arguments object history adds
/// (§4; queryChanges per the -04 editor's copy), with the spec's
/// defaults. Each is left out of the serialized form when it has its
/// default.
///
/// `destroyedAfter` or `destroyedBefore` supplied while `onlyDestroyed` is
/// false is `invalidArguments` (§4).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DestroyedQueryArguments {
    /// `onlyDestroyed` (default false): search destroyed objects instead
    /// of current ones.
    #[serde(default, skip_serializing_if = "is_false")]
    pub only_destroyed: bool,
    /// `destroyedAfter` (default null): only objects destroyed strictly
    /// after this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destroyed_after: Option<UTCDate>,
    /// `destroyedBefore` (default null): only objects destroyed strictly
    /// before this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destroyed_before: Option<UTCDate>,
    /// Other arguments, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl DestroyedQueryArguments {
    /// Every argument at its default.
    pub fn new() -> Self {
        Self::default()
    }
}

/// The `Foo/set` response properties object history adds (§5.1).
///
/// A projection of the response object: read with
/// [`from_set_extra`](Self::from_set_extra) from the generic `/set`
/// response's `extra`, so it keeps no unknown members of its own. Empty
/// maps are written as `null`; `null` or absent ones read as empty.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    /// `restored`: "Id[Foo|null]|null", each restored id with the
    /// properties whose values differ from the final version with the
    /// patch applied, or `None` when none do.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub restored: BTreeMap<Id, Option<PartialObject>>,
    /// `notRestored`: "Id[SetError]|null".
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub not_restored: BTreeMap<Id, SetError>,
}

impl RestoreOutcome {
    /// An empty outcome.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read `restored` and `notRestored` from the members of a `/set`
    /// response that the generic response type does not name.
    ///
    /// # Errors
    ///
    /// A [`serde_json::Error`] when either member has the wrong shape.
    pub fn from_set_extra(extra: &Map<String, Value>) -> Result<Self, serde_json::Error> {
        let mut m = Map::new();
        for key in ["restored", "notRestored"] {
            if let Some(v) = extra.get(key) {
                m.insert(key.to_owned(), v.clone());
            }
        }
        serde_json::from_value(Value::Object(m))
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_zero(n: &u64) -> bool {
    *n == 0
}
