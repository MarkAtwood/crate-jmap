//! Typed JMAP Calendars method wrappers — response types, SessionClient,
//! constants, and helpers.
//!
//! Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.5 /query,
//! §5.2 /changes, §5.3 /set). Method implementations live in sub-modules and
//! operate on `SessionClient`.

pub mod calendar;
pub mod event;
pub mod event_copy;
pub mod event_notification;
pub mod event_parse;
pub mod participant_identity;
pub mod principal_availability;

use std::collections::HashMap;

use serde::Deserialize;

use jmap_types::Id;

// ---------------------------------------------------------------------------
// Response types (RFC 8620 §5)
// ---------------------------------------------------------------------------
//
// Re-exported from `jmap-types::methods` so all `jmap-*-client` crates share
// one canonical set of /get, /set, /changes, /query, /queryChanges shapes.
// The wire format is identical to the previous local definitions.

pub use jmap_types::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetError,
    SetResponse,
};

// ---------------------------------------------------------------------------
// CalendarEvent/parse response
// ---------------------------------------------------------------------------

/// Response type for `CalendarEvent/parse`
/// (draft-ietf-jmap-calendars-26 §5.13).
///
/// `parsed` maps each blob id to the list of `CalendarEvent` objects parsed
/// from that blob (a blob may contain multiple VEVENT components).
/// `not_found` lists blob ids that were not found in the account.
/// `not_parsable` lists blob ids that could not be parsed as iCalendar data.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventParseResponse {
    /// The account this response refers to.
    pub account_id: Id,
    /// Parsed `CalendarEvent` objects keyed by source blob id (a blob may
    /// contain multiple VEVENT components).
    pub parsed: Option<HashMap<Id, Vec<jmap_calendars_types::CalendarEvent>>>,
    /// Blob ids that could not be found in the account's blob store.
    pub not_found: Option<Vec<Id>>,
    /// Blob ids whose contents were not parseable as iCalendar data.
    pub not_parsable: Option<Vec<Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Principal/getAvailability response
// ---------------------------------------------------------------------------

/// Response type for `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-26 §2.2).
///
/// `list` contains the busy periods for the queried principal within the
/// requested time range.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalGetAvailabilityResponse {
    /// The account this response refers to.
    pub account_id: Id,
    /// Busy periods for the queried principal within the requested time range.
    pub list: Vec<jmap_calendars_types::BusyPeriod>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Calendar/set extra parameters
// ---------------------------------------------------------------------------

/// Extra method-level arguments for `Calendar/set`
/// (draft-ietf-jmap-calendars-26 §4.4).
///
/// All fields are optional. Pass `None` (or `Default::default()`) when not
/// needed. The `if_in_state` optimistic-concurrency guard remains a
/// separate inline argument on
/// [`SessionClient::calendar_set`](SessionClient::calendar_set), matching
/// the canonical `email_submission_set` shape in `jmap-mail-client`.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSetParams {
    /// If `true`, destroying a calendar also destroys all its events
    /// (draft-ietf-jmap-calendars-26 §4.4). Server default: false (the
    /// server MUST reject a destroy on a calendar with events,
    /// returning the `calendarHasEvent` SetError).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_destroy_remove_events: Option<bool>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// CalendarEvent/get extra parameters
// ---------------------------------------------------------------------------

/// Extra parameters accepted by `CalendarEvent/get`
/// (draft-ietf-jmap-calendars-26 §5.4).
///
/// All fields are optional. Pass `None` for any field to omit it from the
/// request (the server uses its default).
///
/// Mirrors the canonical [`EmailGetParams`] shape from `jmap-mail-client`:
/// derives `serde::Serialize` + `#[serde(rename_all = "camelCase")]` so
/// the wire shape is enforced by the type system, and carries an `extra`
/// flatten map for the workspace extras-preservation policy
/// (workspace AGENTS.md → "Extras-preservation policy" → in-scope:
/// "Method-argument structs in *-client crates"). Forward-compat for
/// future spec-defined fields is the `extra` map, not `#[non_exhaustive]`.
///
/// [`EmailGetParams`]: https://docs.rs/jmap-mail-client/latest/jmap_mail_client/methods/struct.EmailGetParams.html
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventGetParams {
    /// If `true`, expand recurring events into individual instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expand_recurrences: Option<bool>,
    /// If `true`, participants are filtered to those relevant to the
    /// authenticated user (reducedParticipants draft §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduced_participants: Option<bool>,
    /// If `true`, the server SHOULD also include `Calendar/get`-style
    /// responses for the `Calendar` objects referenced by returned events
    /// (draft §5.4 `fetchCalendars` argument). The fetched calendars are
    /// emitted as additional `methodResponses` entries, not inlined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_calendars: Option<bool>,
    /// Catch-all for vendor / site / private extension fields not
    /// covered by the typed fields above. Preserves unknown fields
    /// across deserialize/serialize round-trip per workspace
    /// extras-preservation policy.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Extra parameters accepted by `CalendarEvent/parse`
/// (draft-ietf-jmap-calendars-26 §5.13).
///
/// Mirrors the canonical [`EmailParseParams`] shape from
/// `jmap-mail-client`: optional spec-defined fields plus a `flatten`-ed
/// `extra` map for the workspace extras-preservation policy
/// (workspace AGENTS.md → "Extras-preservation policy" → in-scope:
/// "Method-argument structs in *-client crates").
///
/// All fields are optional. Pass `None` for any field to omit it from
/// the request (the server uses its default).
///
/// [`EmailParseParams`]: https://docs.rs/jmap-mail-client/latest/jmap_mail_client/methods/struct.EmailParseParams.html
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventParseParams {
    /// Override the set of `CalendarEvent` properties returned per parsed
    /// blob. When `None`, the server returns the default property set
    /// documented in the draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<String>>,
    /// Catch-all for vendor / site / private extension fields not
    /// covered by the typed fields above. Preserves unknown fields
    /// across deserialize/serialize round-trip per workspace
    /// extras-preservation policy.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Extra parameters accepted by `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-26 §2.2).
///
/// All fields are optional. Pass `None` for any field to omit it from
/// the request (the server uses its default).
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalGetAvailabilityParams {
    /// If `true`, the response includes per-`BusyPeriod` event detail
    /// (subject to per-event `mayReadItems` permission). When `None` or
    /// `Some(false)`, only opaque busy markers are returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_details: Option<bool>,
    /// Subset of `CalendarEvent` properties to return on each
    /// `BusyPeriod`'s `event` field (only meaningful when
    /// `show_details: Some(true)`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_properties: Option<Vec<String>>,
    /// Catch-all for vendor / site / private extension fields not
    /// covered by the typed fields above. Preserves unknown fields
    /// across deserialize/serialize round-trip per workspace
    /// extras-preservation policy.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The call-id embedded in every single-method JMAP request produced by
/// [`build_request`]. Pass directly to `jmap_base_client::extract_response`.
pub(crate) const CALL_ID: &str = "r1";

/// Capability URIs for JMAP Calendars method calls (draft-ietf-jmap-calendars-26).
pub(crate) const USING_CALENDARS: &[&str] = &[
    "urn:ietf:params:jmap:core",
    jmap_calendars_types::JMAP_CALENDARS_URI,
];

/// Capability URIs for `CalendarEvent/parse`
/// (draft-ietf-jmap-calendars-26 §5.13).
pub(crate) const USING_PARSE: &[&str] = &[
    "urn:ietf:params:jmap:core",
    jmap_calendars_types::JMAP_CALENDARS_URI,
    jmap_calendars_types::JMAP_CALENDARS_PARSE_URI,
];

/// Capability URIs for `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-26 §2.2).
pub(crate) const USING_AVAILABILITY: &[&str] = &[
    "urn:ietf:params:jmap:core",
    jmap_calendars_types::JMAP_PRINCIPALS_AVAILABILITY_URI,
];

// ---------------------------------------------------------------------------
// build_request helper
// ---------------------------------------------------------------------------

/// Build a single-method JMAP request.
///
/// The embedded call-id is [`CALL_ID`]; pass it directly to
/// `jmap_base_client::extract_response`.
pub(crate) fn build_request(
    method: &str,
    args: serde_json::Value,
    using: &[&str],
) -> jmap_types::JmapRequest {
    let using_vec: Vec<String> = using.iter().map(|&s| s.to_owned()).collect();
    let invocation: jmap_types::Invocation = (method.to_owned(), args, CALL_ID.to_owned());
    jmap_types::JmapRequest::new(using_vec, vec![invocation], None)
}

// ---------------------------------------------------------------------------
// Id validation (RFC 8620 §1.2)
// ---------------------------------------------------------------------------
//
// Earlier revisions exposed `validate_id_field` / `validate_ids_field`
// helpers that wrapped `jmap_types::Id::new_validated` to guard the
// `&str` / `&[&str]` parameters (decision recorded on bd:JMAP-231o.6).
// The 0.2.0 typed-Id refactor (bd:JMAP-6by7) replaced those parameter
// shapes with `&Id` / `&[Id]` directly; once values reach the method
// body they are already validated by virtue of being typed `Id`s, so
// the helpers became dead code and were removed in bd:JMAP-6by7.1.
//
// Callers that still need ad-hoc validation (e.g. when building Ids
// from user input at a higher layer) should call
// `jmap_types::Id::new_validated` directly.

// ---------------------------------------------------------------------------
// SessionClient — session-bound client
// ---------------------------------------------------------------------------

/// A `JmapClient` bound to a JMAP session.
///
/// Obtain via [`JmapCalendarsExt::with_calendars_session`](crate::JmapCalendarsExt::with_calendars_session).
/// All JMAP Calendars methods are available on this type without needing to pass
/// `&Session` on every call.
///
/// `Clone` is derived because `JmapClient` is itself cheap-to-clone (it
/// already implements `Clone` and `with_calendars_session` clones one
/// internally), enabling parallel-task fan-out with one bound session.
///
/// `Debug` is implemented manually to redact the inner `JmapClient` (which
/// holds an HTTP client and is intentionally not `Debug` in
/// `jmap-base-client`); only the `Session` is shown. This lets callers
/// embed a `SessionClient` in a `#[derive(Debug)]` struct without manual
/// impls of their own.
///
/// # Thread safety
///
/// `SessionClient` is `Send + Sync`. Both
/// [`jmap_base_client::JmapClient`] (backed by `reqwest::Client`) and
/// [`jmap_base_client::Session`] (plain serde-derived data) are
/// `Send + Sync` per jmap-base-client's contract, so this type can be
/// shared across async tasks via `Arc<SessionClient>` or cloned for
/// per-task ownership.
///
/// A `Send + Sync` regression in a future jmap-base-client release
/// would be a major-version-breaking change for this crate. A
/// compile-time assertion in `methods/mod.rs` guards against the
/// regression landing silently — see
/// `_assert_session_client_send_sync`.
#[non_exhaustive]
#[derive(Clone)]
pub struct SessionClient {
    pub(crate) client: jmap_base_client::JmapClient,
    pub(crate) session: jmap_base_client::Session,
}

impl std::fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClient")
            // The inner JmapClient is not Debug — show a placeholder so
            // callers know it is present without leaking HTTP-client
            // internals.
            .field("client", &"<JmapClient>")
            .field("session", &self.session)
            .finish()
    }
}

impl SessionClient {
    /// Borrow the underlying [`JmapClient`](jmap_base_client::JmapClient).
    ///
    /// Useful for ad-hoc operations outside the typed JMAP method surface —
    /// for example, calling `JmapClient::upload` / `JmapClient::download_blob`,
    /// or constructing a `JmapClient::event_source` subscription using the
    /// bound session's `event_source_url`.
    pub fn client(&self) -> &jmap_base_client::JmapClient {
        &self.client
    }

    /// Borrow the captured [`Session`](jmap_base_client::Session).
    ///
    /// `SessionClient` captures the `Session` at construction time. After
    /// re-fetching the session via `JmapClient::fetch_session`, callers
    /// should construct a new `SessionClient`. This accessor lets a caller
    /// compare the captured session's `state` field against a freshly
    /// fetched session to detect staleness, or inspect
    /// `accountCapabilities` / `primary_accounts` for capability-specific
    /// metadata not exposed via the typed JMAP method surface.
    pub fn session(&self) -> &jmap_base_client::Session {
        &self.session
    }

    /// Return the primary account id for `urn:ietf:params:jmap:calendars`,
    /// or `Err(InvalidSession)` if the session has no primary account for
    /// that capability.
    pub fn calendars_account_id(&self) -> Result<&str, jmap_base_client::ClientError> {
        self.session
            .primary_account_id(jmap_calendars_types::JMAP_CALENDARS_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:calendars".into(),
                )
            })
    }

    /// Extract `(api_url, calendars_account_id)` from the bound session.
    ///
    /// Returns `Err(InvalidSession)` if there is no primary account for
    /// `urn:ietf:params:jmap:calendars`.
    pub(crate) fn session_parts(&self) -> Result<(&str, &str), jmap_base_client::ClientError> {
        let api_url = self.session.api_url.as_str();
        let account_id = self
            .session
            .primary_account_id(jmap_calendars_types::JMAP_CALENDARS_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:calendars".into(),
                )
            })?;
        Ok((api_url, account_id))
    }

    /// Forward a JMAP request to the underlying HTTP client.
    pub(crate) async fn call_internal(
        &self,
        api_url: &str,
        req: &jmap_types::JmapRequest,
    ) -> Result<jmap_types::JmapResponse, jmap_base_client::ClientError> {
        self.client.call(api_url, req).await
    }
}

/// Compile-time assertion that [`SessionClient`] is `Send + Sync`.
///
/// The `# Thread safety` section of [`SessionClient`]'s rustdoc promises
/// auto-trait inheritance from
/// [`jmap_base_client::JmapClient`] and
/// [`jmap_base_client::Session`]. If a future jmap-base-client release
/// adds a `!Sync` interior-mutability field to either, this assertion
/// fails at compile time — flagging the regression at the dependency
/// upgrade rather than at the downstream caller's "cannot send between
/// threads safely" error.
#[allow(dead_code)]
fn _assert_session_client_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SessionClient>();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: build_request produces the correct method name and CALL_ID.
    /// Expected values are literals from the spec, not derived from the function.
    #[test]
    fn build_request_method_name_and_call_id() {
        let req = build_request(
            "Calendar/get",
            json!({"accountId": "acc1", "ids": null}),
            USING_CALENDARS,
        );
        let v = serde_json::to_value(&req).expect("serialize JmapRequest");

        let calls = v["methodCalls"]
            .as_array()
            .expect("methodCalls must be array");
        assert_eq!(calls.len(), 1, "must have exactly 1 method call");
        assert_eq!(calls[0][0], json!("Calendar/get"), "method name must match");
        assert_eq!(calls[0][2], json!("r1"), "call_id must be CALL_ID constant");
    }

    /// Oracle: USING_CALENDARS contains exactly the two required capability URIs.
    /// Expected values are from draft-ietf-jmap-calendars-26 §1.
    #[test]
    fn using_calendars_contains_correct_uris() {
        let req = build_request("Calendar/get", json!({}), USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using must be array");
        assert_eq!(using.len(), 2);
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:core")),
            "must include jmap:core"
        );
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:calendars")),
            "must include jmap:calendars"
        );
    }

    /// Oracle: SetResponse<T>.updated must accept null values per RFC 8620
    /// §5.3 wire type "Id[Foo|null]|null" (rfc8620.txt line 2043).
    ///
    /// The server returns null for a successfully updated object when the
    /// patch was applied verbatim with no server-set property deltas to
    /// report. A typed SetResponse<CalendarEvent> must deserialize this
    /// shape rather than failing because `null` cannot become CalendarEvent.
    ///
    /// Independent oracle: hand-written JSON fixture mirroring the spec
    /// wire shape directly — not generated by any code in this crate.
    #[test]
    fn set_response_updated_accepts_null_values() {
        let raw = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "updated": {
                "ev1": null,
                "ev2": null
            }
        });
        let resp: SetResponse<jmap_calendars_types::CalendarEvent> = serde_json::from_value(raw)
            .expect("SetResponse must accept Id[Foo|null] per RFC 8620 §5.3");
        let updated = resp.updated.expect("updated must be Some");
        assert_eq!(updated.len(), 2, "two ids in updated map");
        assert!(
            updated
                .get(&Id::from("ev1"))
                .expect("ev1 key present")
                .is_none(),
            "ev1 value must be None (null)"
        );
        assert!(
            updated
                .get(&Id::from("ev2"))
                .expect("ev2 key present")
                .is_none(),
            "ev2 value must be None (null)"
        );
    }

    /// Oracle: SetResponse<T>.updated also accepts non-null Foo values per
    /// RFC 8620 §5.3 — the union "Id[Foo|null]" must round-trip both arms.
    /// Server returns a Foo object when server-set or computed properties
    /// changed beyond what the client patched (rfc8620.txt lines 2048-2051).
    #[test]
    fn set_response_updated_accepts_object_values() {
        let raw = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "updated": {
                "ev1": { "id": "ev1", "title": "Meeting", "calendarIds": {"cal-1": true} }
            }
        });
        let resp: SetResponse<serde_json::Value> =
            serde_json::from_value(raw).expect("SetResponse must accept Id[Foo] per RFC 8620 §5.3");
        let updated = resp.updated.expect("updated must be Some");
        let ev1 = updated
            .get(&Id::from("ev1"))
            .expect("ev1 key present")
            .as_ref()
            .expect("ev1 value must be Some when server reports deltas");
        assert_eq!(ev1["title"], json!("Meeting"));
    }

    /// Oracle: RFC 8620 §5.3 — `notUpdated` and `notDestroyed` are
    /// `Id[SetError]` maps (server-assigned ids). The map keys MUST
    /// deserialize as `Id`, not `String`, so callers can use them
    /// interchangeably with ids from `/get` responses or the typed
    /// `destroyed` array.
    ///
    /// Independent oracle: hand-written JSON shaped per the spec wire
    /// definition; not derived from any code in this crate.
    #[test]
    fn set_response_not_updated_and_not_destroyed_keys_are_ids() {
        let raw = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "notUpdated": {
                "ev-1": { "type": "stateMismatch" }
            },
            "notDestroyed": {
                "ev-2": { "type": "notFound" }
            }
        });
        let resp: SetResponse<serde_json::Value> = serde_json::from_value(raw)
            .expect("SetResponse must accept Id[SetError] per RFC 8620 §5.3");
        let not_updated = resp.not_updated.expect("notUpdated must be Some");
        assert_eq!(
            not_updated
                .get(&Id::from("ev-1"))
                .expect("ev-1 key present")
                .error_type,
            "stateMismatch",
            "notUpdated key must deserialize as Id, not String"
        );
        let not_destroyed = resp.not_destroyed.expect("notDestroyed must be Some");
        assert_eq!(
            not_destroyed
                .get(&Id::from("ev-2"))
                .expect("ev-2 key present")
                .error_type,
            "notFound",
            "notDestroyed key must deserialize as Id, not String"
        );
    }

    /// Oracle: GetResponse<T> deserializes from RFC 8620 §5.1 shape.
    #[test]
    fn get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s42",
            "list": [],
            "notFound": ["missing1"]
        });
        let resp: GetResponse<serde_json::Value> =
            serde_json::from_value(json).expect("GetResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.state, "s42");
        assert!(resp.list.is_empty());
        assert_eq!(
            resp.not_found.as_deref(),
            Some(["missing1".into()].as_slice())
        );
    }

    /// Oracle: CalendarEventParseResponse deserializes from a spec-derived JSON
    /// fixture (draft-ietf-jmap-calendars-26 §5.13).
    #[test]
    fn calendar_event_parse_response_deserializes() {
        let raw = json!({
            "accountId": "acc1",
            "parsed": { "blob1": [{"id": "ev1", "title": "Meeting"}] },
            "notFound": ["missing1"],
            "notParsable": []
        });
        let resp: CalendarEventParseResponse =
            serde_json::from_value(raw).expect("CalendarEventParseResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        let parsed = resp.parsed.expect("parsed must be Some");
        let blob1_key = jmap_types::Id::from("blob1");
        assert!(parsed.contains_key(&blob1_key), "blob1 must be in parsed");
        assert_eq!(parsed[&blob1_key].len(), 1, "one event under blob1");
        assert_eq!(
            resp.not_found.as_deref(),
            Some(["missing1".into()].as_slice())
        );
        let not_parsable = resp.not_parsable.expect("notParsable must be Some");
        assert!(not_parsable.is_empty(), "notParsable must be empty");
    }

    /// Oracle: PrincipalGetAvailabilityResponse deserializes from a
    /// spec-derived JSON fixture (draft-ietf-jmap-calendars-26 §2.2).
    #[test]
    fn principal_get_availability_response_deserializes() {
        let raw = json!({
            "accountId": "acc1",
            "list": [
                {
                    "utcStart": "2024-06-15T09:00:00Z",
                    "utcEnd": "2024-06-15T10:00:00Z",
                    "busyStatus": "busy",
                    "accountId": "acc1"
                }
            ]
        });
        let resp: PrincipalGetAvailabilityResponse =
            serde_json::from_value(raw).expect("PrincipalGetAvailabilityResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.list.len(), 1, "one busy period");
        let period = &resp.list[0];
        assert_eq!(period.utc_start, "2024-06-15T09:00:00Z");
        assert_eq!(period.utc_end, "2024-06-15T10:00:00Z");
        assert_eq!(
            period.busy_status.as_deref(),
            Some("busy"),
            "busyStatus must be 'busy'"
        );
        assert_eq!(
            period.account_id.as_ref().map(|id| id.as_ref()),
            Some("acc1"),
            "accountId in BusyPeriod must be 'acc1'"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // For Deserialize-only method-response structs, the test deserialises
    // JSON containing a vendor field and asserts the field is captured in
    // `extra`. The test uses synthetic `acmeCorp*` keys that are guaranteed
    // not to appear in any draft-defined field — so the tests are
    // independent of the crate under test.

    /// `CalendarEventParseResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn calendar_event_parse_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "acc1",
            "parsed": null,
            "notFound": null,
            "notParsable": null,
            "acmeCorpRequestId": "req-42"
        });
        let resp: CalendarEventParseResponse =
            serde_json::from_value(raw).expect("CalendarEventParseResponse must deserialize");
        assert_eq!(
            resp.extra.get("acmeCorpRequestId").and_then(|v| v.as_str()),
            Some("req-42")
        );
    }

    /// `PrincipalGetAvailabilityResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn principal_get_availability_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "acc1",
            "list": [],
            "acmeCorpQuotaRemaining": 99
        });
        let resp: PrincipalGetAvailabilityResponse =
            serde_json::from_value(raw).expect("PrincipalGetAvailabilityResponse must deserialize");
        assert_eq!(
            resp.extra
                .get("acmeCorpQuotaRemaining")
                .and_then(|v| v.as_i64()),
            Some(99)
        );
    }

    /// Oracle: CalendarEventGetParams serializes correctly to JSON.
    /// All Some fields appear; None fields are absent via
    /// `skip_serializing_if`. Independent of the production builder —
    /// directly exercises the struct's Serialize derive against the
    /// expected wire shape from draft-ietf-jmap-calendars-26 §5.4.
    #[test]
    fn calendar_event_get_params_serializes() {
        let params = CalendarEventGetParams {
            expand_recurrences: Some(true),
            reduced_participants: Some(false),
            fetch_calendars: None,
            extra: serde_json::Map::new(),
        };
        let out = serde_json::to_value(&params).expect("Serialize is infallible for plain data");
        let obj = out
            .as_object()
            .expect("Params must serialize to a JSON object");
        // Some fields appear with camelCase wire names
        assert_eq!(obj.get("expandRecurrences"), Some(&json!(true)));
        assert_eq!(obj.get("reducedParticipants"), Some(&json!(false)));
        // None field is omitted via skip_serializing_if
        assert!(
            obj.get("fetchCalendars").is_none(),
            "fetchCalendars must be absent when None"
        );
        // No snake_case leakage
        assert!(
            obj.get("expand_recurrences").is_none(),
            "snake_case must not appear on the wire"
        );
        // Extra map is empty so it doesn't add anything to the output
        assert!(obj.len() == 2, "expected exactly 2 set fields, got: {out}");
    }

    /// Oracle: CalendarEventGetParams.extra carries vendor / site
    /// extension fields through the wire request via the flatten map.
    /// Workspace AGENTS.md mandates this for method-argument structs.
    #[test]
    fn calendar_event_get_params_extras_flatten() {
        let mut extra = serde_json::Map::new();
        extra.insert("acmeCorpDebug".to_owned(), json!(true));
        let params = CalendarEventGetParams {
            expand_recurrences: Some(true),
            reduced_participants: None,
            fetch_calendars: None,
            extra,
        };
        let out = serde_json::to_value(&params).expect("Serialize is infallible for plain data");
        assert_eq!(out["expandRecurrences"], json!(true));
        assert_eq!(out["acmeCorpDebug"], json!(true));
    }

    /// Oracle: CalendarSetParams with on_destroy_remove_events serializes
    /// the field at the expected camelCase wire name.
    /// Expected field name "onDestroyRemoveEvents" from
    /// draft-ietf-jmap-calendars-26 §4.4.
    #[test]
    fn calendar_set_params_on_destroy_remove_events_serializes() {
        let params = CalendarSetParams {
            on_destroy_remove_events: Some(true),
            extra: serde_json::Map::new(),
        };
        let out = serde_json::to_value(&params).expect("serialize CalendarSetParams");
        assert_eq!(out["onDestroyRemoveEvents"], json!(true));
    }

    /// Oracle: CalendarSetParams default (all-None) serializes to an empty
    /// object — every typed field is `skip_serializing_if = "Option::is_none"`
    /// and `extra` is `skip_serializing_if = "Map::is_empty"`.
    #[test]
    fn calendar_set_params_default_serializes_empty() {
        let params = CalendarSetParams::default();
        let out = serde_json::to_value(&params).expect("serialize CalendarSetParams::default");
        let obj = out.as_object().expect("must be Object");
        assert!(
            obj.is_empty(),
            "all-None default must serialize to empty object, got: {out}"
        );
    }

    /// `CalendarSetParams.extra` flattens into serialized JSON.
    #[test]
    fn calendar_set_params_propagates_vendor_extras() {
        let mut params = CalendarSetParams::default();
        params
            .extra
            .insert("acmeCorpCascade".into(), json!("strict"));
        let v = serde_json::to_value(&params).expect("serialize CalendarSetParams");
        assert_eq!(v["acmeCorpCascade"], json!("strict"));
    }
}
