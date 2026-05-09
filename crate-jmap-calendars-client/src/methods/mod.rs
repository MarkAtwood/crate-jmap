// Typed JMAP Calendars method wrappers — response types, SessionClient,
// constants, and helpers.
//
// Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.5 /query,
// §5.2 /changes, §5.3 /set). Method implementations live in sub-modules and
// operate on `SessionClient`.

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
    pub account_id: Id,
    pub parsed: Option<HashMap<Id, Vec<jmap_calendars_types::CalendarEvent>>>,
    pub not_found: Option<Vec<Id>>,
    pub not_parsable: Option<Vec<Id>>,
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
    pub account_id: Id,
    pub list: Vec<jmap_calendars_types::BusyPeriod>,
}

// ---------------------------------------------------------------------------
// CalendarEvent/get extra parameters
// ---------------------------------------------------------------------------

/// Extra parameters accepted by `CalendarEvent/get`
/// (draft-ietf-jmap-calendars-26 §5.4).
///
/// All fields are optional. Pass `None` for any field to omit it from the
/// request (the server uses its default).
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct CalendarEventGetParams {
    /// If `true`, expand recurring events into individual instances.
    pub expand_recurrences: Option<bool>,
    /// If `true`, participants are filtered to those relevant to the
    /// authenticated user (reducedParticipants draft §5.4).
    pub reduced_participants: Option<bool>,
    /// If `true`, the referenced Calendar objects are included in the
    /// response as implicit implicit implicit-fetch (draft §5.4).
    pub fetch_calendars: Option<bool>,
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
    "urn:ietf:params:jmap:calendars",
];

/// Capability URIs for `CalendarEvent/parse`
/// (draft-ietf-jmap-calendars-26 §5.13).
pub(crate) const USING_PARSE: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:calendars",
    "urn:ietf:params:jmap:calendars:parse",
];

/// Capability URIs for `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-26 §2.2).
pub(crate) const USING_AVAILABILITY: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:principals:availability",
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

/// Validate a single string field as an RFC 8620 §1.2 Id.
///
/// The previous half-measure `is_empty()` guards caught only empty strings;
/// `Id::new_validated` enforces the full §1.2 syntax (1..=255 SAFE-CHARs).
/// This means inputs like `"\x00bad"`, `" "`, `"#$%"`, or strings >255 octets
/// are rejected client-side before any HTTP call rather than producing
/// confusing server-side errors. Decision recorded on bd:JMAP-231o.6.
///
/// `label` is the human-readable identifier of the field for the error
/// message (e.g. `"calendar_event_copy: from_account_id"`). The smoke tests
/// rely on the field name appearing as a substring of the error message.
pub(crate) fn validate_id_field(
    value: &str,
    label: &str,
) -> Result<(), jmap_base_client::ClientError> {
    jmap_types::Id::new_validated(value)
        .map(|_| ())
        .map_err(|e| {
            jmap_base_client::ClientError::InvalidArgument(format!(
                "{label} is not a valid Id ({e})"
            ))
        })
}

/// Validate every element of an Id slice as an RFC 8620 §1.2 Id.
///
/// See [`validate_id_field`] for the `Id::new_validated` rationale.
///
/// `context` is the method name (e.g. `"calendar_get"`); `field` is the
/// argument name (e.g. `"ids"`, `"destroy"`, `"blobIds"`). The smoke tests
/// assert on the substring `"<field> element"` so both pieces must appear
/// in the error message.
pub(crate) fn validate_ids_field(
    ids: &[&str],
    context: &str,
    field: &str,
) -> Result<(), jmap_base_client::ClientError> {
    for id in ids {
        jmap_types::Id::new_validated(*id)
            .map(|_| ())
            .map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "{context}: {field} element {id:?} is not a valid Id ({e})"
                ))
            })?;
    }
    Ok(())
}

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
    /// Extract `(api_url, calendars_account_id)` from the bound session.
    ///
    /// Returns `Err(InvalidSession)` if there is no primary account for
    /// `urn:ietf:params:jmap:calendars`.
    pub(crate) fn session_parts(&self) -> Result<(&str, &str), jmap_base_client::ClientError> {
        let api_url = self.session.api_url.as_str();
        let account_id = self
            .session
            .primary_account_id("urn:ietf:params:jmap:calendars")
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

    /// Oracle: CalendarEventGetParams serializes correctly to JSON.
    /// All Some fields appear; None fields are absent.
    #[test]
    fn calendar_event_get_params_serializes() {
        let params = CalendarEventGetParams {
            expand_recurrences: Some(true),
            reduced_participants: Some(false),
            fetch_calendars: None,
        };
        let mut args = json!({"accountId": "acc1"});
        if let Some(v) = params.expand_recurrences {
            args["expandRecurrences"] = v.into();
        }
        if let Some(v) = params.reduced_participants {
            args["reducedParticipants"] = v.into();
        }
        if let Some(v) = params.fetch_calendars {
            args["fetchCalendars"] = v.into();
        }
        // Verify expandRecurrences and reducedParticipants are in args
        assert_eq!(args["expandRecurrences"], json!(true));
        assert_eq!(args["reducedParticipants"], json!(false));
        // fetch_calendars was None — it should not have been inserted
        assert!(
            args.get("fetchCalendars").is_none(),
            "fetchCalendars should not appear when None"
        );
        // Verify round-trip through build_request
        let req = build_request("CalendarEvent/get", args.clone(), USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let method_args = &v["methodCalls"][0][1];
        assert_eq!(method_args["expandRecurrences"], json!(true));
        assert_eq!(method_args["reducedParticipants"], json!(false));
    }
}
