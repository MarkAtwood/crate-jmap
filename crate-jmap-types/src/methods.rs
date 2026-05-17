//! RFC 8620 §5 generic method-response shapes shared by all JMAP method types.
//!
//! These wire types are normative — every JMAP `/get`, `/set`, `/changes`,
//! `/query`, and `/queryChanges` method (across mail, calendars, contacts,
//! chat, etc.) returns one of these shapes. Centralising them here avoids
//! drift between the seven `jmap-*-client` crates that previously each
//! defined their own copies. Server crates may still hand-build wire JSON
//! for their `/set` responses (so they can use the typed
//! [`SetErrorType`](crate::backend) enum at construction time); this crate's
//! [`SetError`] is the deserialization target on the client side.
//!
//! All types use camelCase JSON via `#[serde(rename_all = "camelCase")]` and
//! are marked `#[non_exhaustive]` so future RFC errata or extensions can add
//! fields without a SemVer break.
//!
//! # Spec references
//!
//! | Type | Spec |
//! |---|---|
//! | [`GetResponse`] | RFC 8620 §5.1 |
//! | [`ChangesResponse`] | RFC 8620 §5.2 |
//! | [`SetResponse`], [`SetError`] | RFC 8620 §5.3 |
//! | [`QueryResponse`] | RFC 8620 §5.5 |
//! | [`QueryChangesResponse`], [`AddedItem`] | RFC 8620 §5.6 |

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{Id, State};

// ---------------------------------------------------------------------------
// /get
// ---------------------------------------------------------------------------

/// RFC 8620 §5.1 — `Foo/get` response shape.
///
/// `T` is the type of object being fetched (e.g. `Mailbox`, `CalendarEvent`).
/// `state` is the opaque state token the server returns alongside the result;
/// it advances every time any object of the requested type changes in the
/// account. `not_found` lists ids the client requested that the server could
/// not find — `null` is treated as an empty list per RFC 8620 §5.1.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResponse<T> {
    /// The account the response refers to.
    pub account_id: Id,
    /// Opaque state token for this object type at the time of the response.
    pub state: State,
    /// The fetched objects, one per id that was found.
    pub list: Vec<T>,
    /// Ids that were requested but not found. `null` on the wire is treated
    /// as an empty list per RFC 8620 §5.1.
    pub not_found: Option<Vec<Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// /changes
// ---------------------------------------------------------------------------

/// RFC 8620 §5.2 — `Foo/changes` response shape.
///
/// Reports the ids of objects created, updated, or destroyed since
/// `old_state`. If `has_more_changes` is `true`, the client should call
/// `/changes` again with `since_state = new_state` to retrieve the next
/// page; otherwise `new_state` is the current state.
///
/// # Extension fields
///
/// Some JMAP data-type extensions add an `updatedProperties` field to
/// their `/changes` response shape:
///
/// - RFC 8621 §2.2 (`Mailbox/changes`): set when only `totalEmails` /
///   `unreadEmails` / `totalThreads` / `unreadThreads` changed.
/// - RFC 9425 §5 (`Quota/changes`): set when only the `used` property
///   changed.
///
/// For all other `/changes` methods (RFC 8621 §3.2 `Thread/changes`,
/// §4.3 `Email/changes`, plus every extension `/changes` method not
/// listed above) the server omits the field, and clients deserialize
/// it as `None`. Carrying the field on the base type avoids duplicating
/// the `ChangesResponse` shape into per-extension newtypes.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesResponse {
    /// The account the response refers to.
    pub account_id: Id,
    /// The state token the client passed in.
    pub old_state: State,
    /// The current (or next-page) state token.
    pub new_state: State,
    /// `true` if there are more changes the client must page through.
    pub has_more_changes: bool,
    /// Ids of objects created since `old_state`.
    pub created: Vec<Id>,
    /// Ids of objects updated since `old_state`.
    pub updated: Vec<Id>,
    /// Ids of objects destroyed since `old_state`.
    pub destroyed: Vec<Id>,
    /// Optional list of property names that changed (RFC 8621 §2.2,
    /// RFC 9425 §5). Servers MAY set this for `Mailbox/changes` and
    /// `Quota/changes` responses when the only changes are to a small
    /// known subset of properties; clients can then back-reference
    /// `/updatedProperties` into a follow-up `Mailbox/get` or
    /// `Quota/get` to fetch only those fields. For all other `/changes`
    /// methods the field is absent on the wire and `None` here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_properties: Option<Vec<String>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// /set
// ---------------------------------------------------------------------------

/// A per-item failure in a `/set` response (RFC 8620 §5.3).
///
/// Appears as the value type in the `notCreated`, `notUpdated`, and
/// `notDestroyed` maps of [`SetResponse`]. The `error_type` field uses
/// `String` rather than a typed enum so extension errors (e.g.
/// `"calendarHasEvent"`, `"noSupportedScheduleMethods"`) round-trip
/// cleanly without requiring a version-bump on every new spec extension.
///
/// All fields beyond `error_type` are optional and present only when the
/// corresponding error type calls for them per RFC 8620 §5.3 / RFC 8621
/// §5.5, §5.7, §7.5:
///
/// | Field | Set when error_type is | Spec |
/// |---|---|---|
/// | `description` | any (optional human-readable detail) | RFC 8620 §5.3 |
/// | `properties` | `invalidProperties` | RFC 8620 §5.3 |
/// | `existing_id` | `alreadyExists` | RFC 8620 §5.4, RFC 8621 §5.7 |
/// | `not_found` | `blobNotFound` | RFC 8621 §5.5 |
/// | `max_recipients` | `tooManyRecipients` | RFC 8621 §7.5 |
/// | `invalid_recipients` | `invalidRecipients` | RFC 8621 §7.5 |
/// | `max_size` | `tooLarge` | RFC 8621 §7.5 |
///
/// # Extension fields
///
/// JMAP extensions (e.g. JMAP Chat's `serverRetryAfter` for slow-mode
/// rate limiting) MAY add additional SetError fields beyond the RFC 8620
/// base set. The `extra` field captures any such field via
/// `#[serde(flatten)]` so it round-trips losslessly. Extension crates
/// (e.g. `jmap-chat-client`) provide typed accessor helpers that read
/// from `extra` — the base type stays free of extension-specific fields.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetError {
    /// The machine-readable error type (e.g. `"forbidden"`, `"notFound"`,
    /// `"alreadyExists"`, or an extension-defined string).
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable description of the error. Optional per RFC 8620 §5.3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Property names that caused the error (for `invalidProperties`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<String>>,
    /// The existing object id (for `alreadyExists`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<Id>,
    /// Missing blob ids (for `blobNotFound`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_found: Option<Vec<Id>>,
    /// Maximum recipients allowed (for `tooManyRecipients`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_recipients: Option<u64>,
    /// Invalid recipient addresses (for `invalidRecipients`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_recipients: Option<Vec<String>>,
    /// Maximum message size in octets (for `tooLarge`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,
    /// Catch-all for extension SetError fields not in the RFC 8620 base
    /// set. Captured via `#[serde(flatten)]` so they round-trip losslessly.
    /// Extension crates provide typed accessors (e.g.
    /// `jmap-chat-client`'s helper for reading `serverRetryAfter`).
    ///
    /// Uses `serde_json::Map` (which, under the workspace's default
    /// `serde_json` features — `preserve_order` is NOT enabled — is
    /// backed by `BTreeMap` and therefore deterministically serializes
    /// in lexicographic key order, NOT in insertion order) rather than
    /// `HashMap` to match the workspace extras-preservation policy (see
    /// workspace `AGENTS.md`) and to give callers deterministic serialized
    /// output.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SetError {
    /// Construct a `SetError` with the given type string and all optional
    /// fields `None` / empty. Use this when deserializing tests or when
    /// constructing a wire-shaped error from a typed source. Server crates
    /// that want a typed enum for construction should use
    /// `jmap_server::backend::SetError` (declared in the `jmap-server`
    /// crate, not linkable from here since `jmap-types` does not depend on
    /// `jmap-server`) — this type is deliberately String-typed for
    /// client-side parsing flexibility.
    pub fn new(error_type: impl Into<String>) -> Self {
        Self {
            error_type: error_type.into(),
            description: None,
            properties: None,
            existing_id: None,
            not_found: None,
            max_recipients: None,
            invalid_recipients: None,
            max_size: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.description {
            Some(desc) => write!(f, "{}: {}", self.error_type, desc),
            None => write!(f, "{}", self.error_type),
        }
    }
}

/// RFC 8620 §5.3 — `Foo/set` response shape.
///
/// Wire shape per RFC 8620 §5.3 (rfc8620.txt §5.3 around line 2033):
///
/// ```text
/// created       Id[Foo]      | null
/// updated       Id[Foo|null] | null   ← inner null is REQUIRED
/// destroyed     Id[]         | null
/// notCreated    Id[SetError] | null
/// notUpdated    Id[SetError] | null
/// notDestroyed  Id[SetError] | null
/// ```
///
/// The inner `null` in `updated` is the server's signal that the patch was
/// applied verbatim with no server-set property deltas to report; a typed
/// `SetResponse<Foo>` MUST accept this rather than failing because `null`
/// cannot become `Foo`.
///
/// `created` and `not_created` keys are caller-supplied creation ids
/// (`String`); `updated`, `not_updated`, `not_destroyed` keys are
/// server-assigned record ids ([`Id`]) — typed differently so callers can
/// use `updated`/`destroyed` keys interchangeably with ids from any
/// `/get` response.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(bound(
    deserialize = "T: serde::de::DeserializeOwned",
    serialize = "T: Serialize"
))]
pub struct SetResponse<T = serde_json::Value> {
    /// The account the response refers to.
    pub account_id: Id,
    /// State token before this `/set` was applied. Optional because some
    /// servers omit it on no-op responses (per RFC 8620 §5.3 the field is
    /// nullable).
    pub old_state: Option<State>,
    /// State token after this `/set`.
    pub new_state: State,
    /// Successfully created objects, keyed by caller-supplied creation id.
    pub created: Option<HashMap<String, T>>,
    /// Successfully updated objects, keyed by record id. The value is
    /// `Some(T)` when the server reports server-set property deltas, or
    /// `None` when the patch was applied verbatim with nothing to echo.
    pub updated: Option<HashMap<Id, Option<T>>>,
    /// Ids of successfully destroyed objects.
    pub destroyed: Option<Vec<Id>>,
    /// Failed creates, keyed by caller-supplied creation id.
    pub not_created: Option<HashMap<String, SetError>>,
    /// Failed updates, keyed by record id.
    pub not_updated: Option<HashMap<Id, SetError>>,
    /// Failed destroys, keyed by record id.
    pub not_destroyed: Option<HashMap<Id, SetError>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// /query
// ---------------------------------------------------------------------------

/// RFC 8620 §5.5 — `Foo/query` response shape.
///
/// Returns the ids of objects matching a filter, in sort order. The
/// `query_state` token can be passed to `Foo/queryChanges` to retrieve only
/// the delta against this snapshot.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    /// The account the response refers to.
    pub account_id: Id,
    /// Opaque state token for this query result; pass to `/queryChanges`.
    pub query_state: State,
    /// `true` if `/queryChanges` will give incremental updates against this
    /// `query_state`; `false` if the client must re-run `/query` to refresh.
    pub can_calculate_changes: bool,
    /// Zero-based offset within the full result set of the first id in
    /// `ids`. Per RFC 8620 §5.5, may differ from the requested `position`
    /// when the requested offset exceeds the result count.
    pub position: u64,
    /// The matching ids in sort order.
    pub ids: Vec<Id>,
    /// Total number of matching objects, or `None` when the request did not
    /// set `calculateTotal: true`.
    pub total: Option<u64>,
    /// Server's max page size; `None` when not advertised.
    pub limit: Option<u64>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// /queryChanges
// ---------------------------------------------------------------------------

/// A single item added to a query result set (RFC 8620 §5.6).
///
/// The `index` is the position the new item occupies in the post-change
/// result set, accounting for items also added in this batch.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedItem {
    /// The id of the new item in the result set.
    pub id: Id,
    /// Zero-based position of the new item in the post-change result set.
    pub index: u64,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// RFC 8620 §5.6 — `Foo/queryChanges` response shape.
///
/// Reports the ids removed from and added to a query result set since
/// `old_query_state`. Combined with the previous result, the client can
/// reconstruct the new result without re-fetching all ids.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryChangesResponse {
    /// The account the response refers to.
    pub account_id: Id,
    /// The state token the client passed in.
    pub old_query_state: State,
    /// The current state token.
    pub new_query_state: State,
    /// Total number of matching objects (only when
    /// `calculateTotal: true` was set in the request).
    pub total: Option<u64>,
    /// Ids removed from the result set since `old_query_state`.
    pub removed: Vec<Id>,
    /// Items added to the result set, with their new positions.
    pub added: Vec<AddedItem>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Independent oracles: hand-written JSON shapes from the RFC 8620
    // examples and prose descriptions, NOT derived from the types being
    // tested. Wire round-trips ensure the serde rename rules and field
    // shapes match the spec.

    #[test]
    fn get_response_round_trips() {
        let raw = json!({
            "accountId": "A1",
            "state": "s42",
            "list": [{"id": "x", "name": "First"}],
            "notFound": ["missing1"]
        });
        let resp = GetResponse::<serde_json::Value>::deserialize(&raw).unwrap();
        assert_eq!(resp.account_id.as_ref(), "A1");
        assert_eq!(resp.state, "s42");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0]["name"], "First");
        let nf = resp.not_found.as_ref().unwrap();
        assert_eq!(nf.len(), 1);
        assert_eq!(nf[0].as_ref(), "missing1");
        // Round-trip back to JSON and confirm the camelCase keys.
        let back = serde_json::to_value(&resp).unwrap();
        assert_eq!(back["accountId"], "A1");
        assert_eq!(back["notFound"][0], "missing1");
    }

    #[test]
    fn get_response_null_not_found() {
        // §5.1 allows notFound to be null when the request did not specify
        // ids (null is treated as the empty list).
        let raw = json!({
            "accountId": "A1",
            "state": "s1",
            "list": [],
            "notFound": null
        });
        let resp: GetResponse<serde_json::Value> = serde_json::from_value(raw).unwrap();
        assert!(resp.not_found.is_none());
    }

    #[test]
    fn changes_response_round_trips() {
        let raw = json!({
            "accountId": "A1",
            "oldState": "s0",
            "newState": "s1",
            "hasMoreChanges": false,
            "created": ["a"],
            "updated": ["b"],
            "destroyed": ["c"]
        });
        let resp: ChangesResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.old_state, "s0");
        assert_eq!(resp.new_state, "s1");
        assert!(!resp.has_more_changes);
        assert_eq!(resp.created[0].as_ref(), "a");
        assert_eq!(resp.updated[0].as_ref(), "b");
        assert_eq!(resp.destroyed[0].as_ref(), "c");
        // RFC 8620 §5.2 base `/changes` does not define `updatedProperties`;
        // the field must default to `None` when absent from the wire.
        assert!(resp.updated_properties.is_none());
    }

    /// Oracle: RFC 8621 §2.2 example response (lines 1015-1031 of rfc8621.txt
    /// in this repo) — `Mailbox/changes` carries `updatedProperties` listing
    /// `totalEmails`, `unreadEmails`, `totalThreads`, `unreadThreads`.
    #[test]
    fn changes_response_deserializes_mailbox_updated_properties() {
        let raw = json!({
            "accountId": "A1",
            "oldState": "78541",
            "newState": "78542",
            "hasMoreChanges": false,
            "updatedProperties": [
                "totalEmails", "unreadEmails",
                "totalThreads", "unreadThreads"
            ],
            "created": [],
            "updated": ["B"],
            "destroyed": []
        });
        let resp: ChangesResponse = serde_json::from_value(raw).unwrap();
        let props = resp
            .updated_properties
            .expect("updatedProperties must be present");
        assert_eq!(
            props,
            vec![
                "totalEmails".to_string(),
                "unreadEmails".to_string(),
                "totalThreads".to_string(),
                "unreadThreads".to_string()
            ]
        );
    }

    /// Oracle: RFC 9425 §5 example response — `Quota/changes` carries
    /// `updatedProperties: ["used"]` when only quota usage changed.
    #[test]
    fn changes_response_deserializes_quota_updated_properties() {
        let raw = json!({
            "accountId": "A1",
            "oldState": "78541",
            "newState": "78542",
            "hasMoreChanges": false,
            "updatedProperties": ["used"],
            "created": [],
            "updated": ["2a06df0d-9865-4e74-a92f-74dcc814270e"],
            "destroyed": []
        });
        let resp: ChangesResponse = serde_json::from_value(raw).unwrap();
        let props = resp
            .updated_properties
            .expect("updatedProperties must be present");
        assert_eq!(props, vec!["used".to_string()]);
    }

    /// `updatedProperties: null` on the wire (RFC 8621 §2.2: "If the server
    /// is unable to tell if only counts have changed, it MUST just be null")
    /// must also deserialize as `None` — distinct from omitted but
    /// semantically equivalent on the typed side.
    #[test]
    fn changes_response_accepts_explicit_null_updated_properties() {
        let raw = json!({
            "accountId": "A1",
            "oldState": "s0",
            "newState": "s1",
            "hasMoreChanges": false,
            "updatedProperties": null,
            "created": [],
            "updated": ["B"],
            "destroyed": []
        });
        let resp: ChangesResponse = serde_json::from_value(raw).unwrap();
        assert!(resp.updated_properties.is_none());
    }

    /// Serializing a `ChangesResponse` without `updated_properties` must
    /// NOT emit a `"updatedProperties": null` key — the
    /// `skip_serializing_if = "Option::is_none"` attribute keeps the wire
    /// shape minimal and matches the RFC 8620 §5.2 base envelope for
    /// methods that don't define the extension field.
    #[test]
    fn changes_response_omits_updated_properties_when_none() {
        let resp = ChangesResponse {
            account_id: Id::from("A1"),
            old_state: "s0".into(),
            new_state: "s1".into(),
            has_more_changes: false,
            created: vec![],
            updated: vec![],
            destroyed: vec![],
            updated_properties: None,
            extra: serde_json::Map::new(),
        };
        let serialized = serde_json::to_value(&resp).expect("must serialize");
        assert!(
            serialized.get("updatedProperties").is_none(),
            "updatedProperties must be omitted when None"
        );
    }

    #[test]
    fn set_response_updated_accepts_null_value() {
        // §5.3 wire type: updated is Id[Foo|null]|null. The inner null
        // signals "patch applied verbatim, no server-set fields to echo".
        let raw = json!({
            "accountId": "A1",
            "oldState": "s1",
            "newState": "s2",
            "updated": { "ev1": null, "ev2": null }
        });
        let resp: SetResponse<serde_json::Value> = serde_json::from_value(raw).unwrap();
        let upd = resp.updated.unwrap();
        assert!(upd.get(&Id::from("ev1")).unwrap().is_none());
        assert!(upd.get(&Id::from("ev2")).unwrap().is_none());
    }

    #[test]
    fn set_response_updated_accepts_object_value() {
        let raw = json!({
            "accountId": "A1",
            "oldState": "s1",
            "newState": "s2",
            "updated": { "ev1": { "id": "ev1", "title": "Meeting" } }
        });
        let resp: SetResponse<serde_json::Value> = serde_json::from_value(raw).unwrap();
        let upd = resp.updated.unwrap();
        let ev1 = upd.get(&Id::from("ev1")).unwrap().as_ref().unwrap();
        assert_eq!(ev1["title"], "Meeting");
    }

    #[test]
    fn set_response_not_updated_keys_are_ids() {
        // §5.3: notUpdated is Id[SetError]|null. Keys are server-assigned
        // ids, not creation ids — typing them as Id (not String) lets
        // callers use the keys interchangeably with /get response ids.
        let raw = json!({
            "accountId": "A1",
            "oldState": "s1",
            "newState": "s1",
            "notUpdated": {
                "ev1": { "type": "stateMismatch" }
            }
        });
        let resp: SetResponse<serde_json::Value> = serde_json::from_value(raw).unwrap();
        let nu = resp.not_updated.unwrap();
        assert_eq!(
            nu.get(&Id::from("ev1")).unwrap().error_type,
            "stateMismatch"
        );
    }

    #[test]
    fn set_error_full_8_fields_round_trip() {
        // §5.3 + RFC 8621 §5.5/§5.7/§7.5: SetError carries up to 8 fields
        // depending on the error type. The client side must preserve all
        // of them on deserialize, otherwise callers cannot recover the
        // existingId / blobNotFound / recipient-list payload that the
        // server relied on to make the error actionable.
        let raw = json!({
            "type": "alreadyExists",
            "description": "conflict",
            "properties": ["name"],
            "existingId": "obj-7",
            "notFound": ["blob-1", "blob-2"],
            "maxRecipients": 50,
            "invalidRecipients": ["bad@", "no@no"],
            "maxSize": 10485760
        });
        let err = SetError::deserialize(&raw).unwrap();
        assert_eq!(err.error_type, "alreadyExists");
        assert_eq!(err.description.as_deref(), Some("conflict"));
        assert_eq!(err.properties.as_ref().unwrap()[0], "name");
        assert_eq!(err.existing_id.as_ref().unwrap().as_ref(), "obj-7");
        assert_eq!(err.not_found.as_ref().unwrap().len(), 2);
        assert_eq!(err.max_recipients, Some(50));
        assert_eq!(err.invalid_recipients.as_ref().unwrap().len(), 2);
        assert_eq!(err.max_size, Some(10_485_760));
        // Round-trip preserves wire field names.
        let back = serde_json::to_value(&err).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn set_error_minimal_omits_optional_fields_on_serialize() {
        // §5.3: only `type` is required. Optional fields MUST be omitted
        // (not serialized as `null`) so the wire matches the server's
        // construction shape exactly.
        let err = SetError::new("forbidden");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["type"], "forbidden");
        assert!(json.get("description").is_none());
        assert!(json.get("properties").is_none());
        assert!(json.get("existingId").is_none());
        assert!(json.get("notFound").is_none());
        assert!(json.get("maxRecipients").is_none());
        assert!(json.get("invalidRecipients").is_none());
        assert!(json.get("maxSize").is_none());
        // Empty extra map must not appear at all in the wire output.
        let obj = json.as_object().unwrap();
        assert_eq!(
            obj.len(),
            1,
            "minimal SetError must serialize to exactly {{type}}: {json}"
        );
    }

    #[test]
    fn set_error_extension_fields_round_trip_via_extra() {
        // JMAP Chat's serverRetryAfter is a per-extension SetError field
        // that must round-trip losslessly through extra without the base
        // type knowing about it. Pin both directions.
        let raw = json!({
            "type": "rateLimited",
            "description": "slow-mode active",
            "serverRetryAfter": "2026-01-01T00:00:00Z"
        });
        let err = SetError::deserialize(&raw).unwrap();
        assert_eq!(err.error_type, "rateLimited");
        assert_eq!(err.description.as_deref(), Some("slow-mode active"));
        assert_eq!(
            err.extra.get("serverRetryAfter").and_then(|v| v.as_str()),
            Some("2026-01-01T00:00:00Z"),
            "extension field must land in extra map: {err:?}"
        );
        let back = serde_json::to_value(&err).unwrap();
        assert_eq!(back, raw, "round-trip must preserve extension field");
    }

    #[test]
    fn set_error_extension_type_round_trips() {
        // Extension errors (e.g. calendars draft §10.7.2) MUST round-trip
        // through the String error_type without a new variant being added.
        let err = SetError::new("noSupportedScheduleMethods");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["type"], "noSupportedScheduleMethods");
        let back: SetError = serde_json::from_value(json).unwrap();
        assert_eq!(back.error_type, "noSupportedScheduleMethods");
    }

    #[test]
    fn set_error_display_with_description() {
        let err = SetError {
            error_type: "forbidden".to_owned(),
            description: Some("not your calendar".to_owned()),
            ..SetError::new("forbidden")
        };
        assert_eq!(err.to_string(), "forbidden: not your calendar");
    }

    #[test]
    fn set_error_display_without_description() {
        let err = SetError::new("forbidden");
        assert_eq!(err.to_string(), "forbidden");
    }

    #[test]
    fn query_response_round_trips() {
        let raw = json!({
            "accountId": "A1",
            "queryState": "qs1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["a", "b", "c"],
            "total": 3,
            "limit": 100
        });
        let resp: QueryResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.query_state, "qs1");
        assert!(resp.can_calculate_changes);
        assert_eq!(resp.ids.len(), 3);
        assert_eq!(resp.total, Some(3));
        assert_eq!(resp.limit, Some(100));
    }

    #[test]
    fn query_response_omits_optional_total_and_limit() {
        let raw = json!({
            "accountId": "A1",
            "queryState": "qs1",
            "canCalculateChanges": false,
            "position": 0,
            "ids": [],
            "total": null,
            "limit": null
        });
        let resp: QueryResponse = serde_json::from_value(raw).unwrap();
        assert!(resp.total.is_none());
        assert!(resp.limit.is_none());
    }

    #[test]
    fn query_changes_response_round_trips() {
        let raw = json!({
            "accountId": "A1",
            "oldQueryState": "qs0",
            "newQueryState": "qs1",
            "total": 5,
            "removed": ["x"],
            "added": [
                {"id": "y", "index": 2}
            ]
        });
        let resp: QueryChangesResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.old_query_state, "qs0");
        assert_eq!(resp.new_query_state, "qs1");
        assert_eq!(resp.total, Some(5));
        assert_eq!(resp.removed[0].as_ref(), "x");
        assert_eq!(resp.added.len(), 1);
        assert_eq!(resp.added[0].id.as_ref(), "y");
        assert_eq!(resp.added[0].index, 2);
    }

    #[test]
    fn added_item_round_trips() {
        let raw = json!({"id": "foo", "index": 7});
        let item = AddedItem::deserialize(&raw).unwrap();
        assert_eq!(item.id.as_ref(), "foo");
        assert_eq!(item.index, 7);
        assert_eq!(serde_json::to_value(&item).unwrap(), raw);
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.1) ───────────────────
    //
    // One round-trip preservation test per migrated type. Each test
    // asserts that an unknown vendor / site / private-extension field
    // survives deserialize/serialize unchanged. Per workspace
    // AGENTS.md "Extras-preservation policy for vendor/site fields".

    /// `GetResponse.extra` captures vendor fields and preserves them on
    /// re-serialize.
    #[test]
    fn get_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "A1",
            "state": "s1",
            "list": [],
            "notFound": null,
            "acmeCorpAuditTrail": {"sequence": 42}
        });
        let resp = GetResponse::<serde_json::Value>::deserialize(&raw).unwrap();
        assert_eq!(
            resp.extra
                .get("acmeCorpAuditTrail")
                .and_then(|v| v["sequence"].as_u64()),
            Some(42),
            "vendor field must land in extra: {:?}",
            resp.extra
        );
        let back = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            back["acmeCorpAuditTrail"]["sequence"], 42,
            "vendor field must survive serialize: {back}"
        );
    }

    /// `ChangesResponse.extra` captures vendor fields and preserves them.
    #[test]
    fn changes_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "A1",
            "oldState": "s0",
            "newState": "s1",
            "hasMoreChanges": false,
            "created": [],
            "updated": [],
            "destroyed": [],
            "acmeCorpReplayToken": "rt-99"
        });
        let resp: ChangesResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(
            resp.extra
                .get("acmeCorpReplayToken")
                .and_then(|v| v.as_str()),
            Some("rt-99")
        );
        let back = serde_json::to_value(&resp).unwrap();
        assert_eq!(back["acmeCorpReplayToken"], "rt-99");
    }

    /// Pins the `#[serde(flatten)] extra` interaction with the typed
    /// `updated_properties: Option<Vec<String>>` field on `ChangesResponse`:
    ///
    /// - Wire `"updatedProperties": null` MUST be consumed by the typed
    ///   field (deserialized as `None`) and MUST NOT leak into `extra`.
    /// - Vendor fields at the top level — including ones whose value is
    ///   a nested object — MUST land in `extra` and survive round-trip.
    ///
    /// Without this regression test, a future serde or `#[serde(flatten)]`
    /// behavior change could silently move the null-valued typed key into
    /// `extra` (or fail to consume it from `extra` on the serialize side),
    /// breaking the wire-format contract for any caller relying on either
    /// the absence of `updatedProperties` from `extra` after parse or on
    /// vendor extras being preserved alongside an explicit null. Filed
    /// under bd:JMAP-6xs8.8.
    #[test]
    fn changes_response_null_updated_properties_and_extras_coexist() {
        let raw = json!({
            "accountId": "A1",
            "oldState": "s0",
            "newState": "s1",
            "hasMoreChanges": false,
            "created": [],
            "updated": ["B"],
            "destroyed": [],
            "updatedProperties": null,
            "acmeCorpReplayToken": "rt-99",
            "acmeCorpMetadata": { "requestId": "r1", "trace": "x" }
        });

        let resp: ChangesResponse =
            serde_json::from_value(raw.clone()).expect("must deserialize");

        // The typed field consumed the explicit null.
        assert!(
            resp.updated_properties.is_none(),
            "explicit null updatedProperties must deserialize as None"
        );

        // The typed key MUST NOT have leaked into `extra` — flatten is
        // supposed to visit only the keys the named fields did not
        // consume, regardless of whether the consumed value was null.
        assert!(
            !resp.extra.contains_key("updatedProperties"),
            "updatedProperties must not appear in extra after the typed \
             field consumed it (was: {:?})",
            resp.extra
        );

        // Top-level vendor fields land in `extra`, including one whose
        // value is itself a nested object.
        assert_eq!(
            resp.extra
                .get("acmeCorpReplayToken")
                .and_then(|v| v.as_str()),
            Some("rt-99")
        );
        let nested = resp
            .extra
            .get("acmeCorpMetadata")
            .and_then(|v| v.as_object())
            .expect("acmeCorpMetadata must be a nested object in extra");
        assert_eq!(nested.get("requestId").and_then(|v| v.as_str()), Some("r1"));
        assert_eq!(nested.get("trace").and_then(|v| v.as_str()), Some("x"));

        // Round-trip: serialize and reparse, all three properties
        // (None updatedProperties omitted, two vendor extras present)
        // must survive.
        let back = serde_json::to_value(&resp).expect("must serialize");

        // None typed field is omitted via skip_serializing_if, so the
        // serialized form should NOT contain updatedProperties at all.
        assert!(
            back.get("updatedProperties").is_none(),
            "None updated_properties must not serialize an explicit null \
             (skip_serializing_if = Option::is_none): {back}"
        );
        assert_eq!(back["acmeCorpReplayToken"], "rt-99");
        assert_eq!(back["acmeCorpMetadata"]["requestId"], "r1");
        assert_eq!(back["acmeCorpMetadata"]["trace"], "x");

        // Reparse the serialized form and confirm equivalence on the
        // typed surface + extras.
        let resp2: ChangesResponse =
            serde_json::from_value(back).expect("reparse must succeed");
        assert!(resp2.updated_properties.is_none());
        assert_eq!(resp2.extra, resp.extra);
    }

    /// `SetResponse.extra` captures vendor fields and preserves them.
    #[test]
    fn set_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "A1",
            "oldState": "s1",
            "newState": "s2",
            "acmeCorpTransactionId": "txn-abc"
        });
        let resp: SetResponse<serde_json::Value> = serde_json::from_value(raw).unwrap();
        assert_eq!(
            resp.extra
                .get("acmeCorpTransactionId")
                .and_then(|v| v.as_str()),
            Some("txn-abc")
        );
        let back = serde_json::to_value(&resp).unwrap();
        assert_eq!(back["acmeCorpTransactionId"], "txn-abc");
    }

    /// `QueryResponse.extra` captures vendor fields and preserves them.
    #[test]
    fn query_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "A1",
            "queryState": "qs1",
            "canCalculateChanges": false,
            "position": 0,
            "ids": [],
            "total": null,
            "limit": null,
            "acmeCorpSearchTimingMs": 17
        });
        let resp: QueryResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(
            resp.extra
                .get("acmeCorpSearchTimingMs")
                .and_then(|v| v.as_u64()),
            Some(17)
        );
        let back = serde_json::to_value(&resp).unwrap();
        assert_eq!(back["acmeCorpSearchTimingMs"], 17);
    }

    /// `QueryChangesResponse.extra` captures vendor fields and preserves them.
    #[test]
    fn query_changes_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "A1",
            "oldQueryState": "qs0",
            "newQueryState": "qs1",
            "total": null,
            "removed": [],
            "added": [],
            "acmeCorpDeltaToken": "dt-2"
        });
        let resp: QueryChangesResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(
            resp.extra
                .get("acmeCorpDeltaToken")
                .and_then(|v| v.as_str()),
            Some("dt-2")
        );
        let back = serde_json::to_value(&resp).unwrap();
        assert_eq!(back["acmeCorpDeltaToken"], "dt-2");
    }

    /// `AddedItem.extra` captures vendor fields and preserves them.
    #[test]
    fn added_item_preserves_vendor_extras() {
        let raw = json!({
            "id": "x",
            "index": 0,
            "acmeCorpHighlight": true
        });
        let item = AddedItem::deserialize(&raw).unwrap();
        assert_eq!(
            item.extra
                .get("acmeCorpHighlight")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        let back = serde_json::to_value(&item).unwrap();
        assert_eq!(back["acmeCorpHighlight"], true);
    }

    /// Empty extras must NOT serialize as a key on the wire — the
    /// `skip_serializing_if = "serde_json::Map::is_empty"` attribute keeps
    /// the wire shape byte-identical to the pre-migration form when no
    /// vendor fields are present.
    #[test]
    fn empty_extras_omitted_from_wire() {
        let resp = AddedItem {
            id: Id::from("z"),
            index: 1,
            extra: serde_json::Map::new(),
        };
        let serialized = serde_json::to_value(&resp).expect("must serialize");
        let obj = serialized.as_object().expect("must be object");
        assert_eq!(
            obj.len(),
            2,
            "empty extras must not add any wire keys; got {serialized}"
        );
        assert!(obj.contains_key("id"));
        assert!(obj.contains_key("index"));
    }
}
