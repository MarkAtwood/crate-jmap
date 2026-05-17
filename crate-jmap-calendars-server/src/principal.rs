//! Principal/* method handlers (draft-ietf-jmap-calendars-26 §2).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding method shape defined by
//! draft-ietf-jmap-calendars-26 §2 (which extends the RFC 8620 §5
//! patterns). The returned `Value` is the corresponding method-response
//! object per the same section refs. `Principal/getAvailability` (§2.2)
//! is a Calendars-specific method with its own request/response shape
//! (`accountId`, `id`, `utcStart`, `utcEnd`, optional `showDetails`,
//! optional `eventProperties` → `accountId`, `list`).
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `serverFail`, plus the
//! Calendars-specific `notFound` / `forbidden` shapes — per draft §2.2).

use jmap_types::{Id, Invocation, JmapError, UTCDate};
use serde_json::{json, Value};

use crate::backend::{AvailabilityError, CalendarsBackend};
use crate::helpers::extract_account_id;
use jmap_server::{bool_arg, server_fail_from_backend};

// ---------------------------------------------------------------------------
// Principal/getAvailability
// ---------------------------------------------------------------------------

/// Handle a `Principal/getAvailability` method call
/// (draft-ietf-jmap-calendars-26 §2.2).
///
/// `args` is the draft §2.2 `Principal/getAvailability` request shape
/// (`accountId`, `id` of the principal, `utcStart`, `utcEnd`, optional
/// `showDetails`, optional `eventProperties`); the returned `Value` is
/// the §2.2 response shape (`accountId`, `list` of
/// [`BusyPeriod`](jmap_calendars_types::BusyPeriod) objects).
///
/// Returns an array of `BusyPeriod` objects for the time range
/// `[utcStart, utcEnd)` for the identified principal.  If `showDetails` is
/// `false` (the default), the `event` and `accountId` fields within each
/// `BusyPeriod` are omitted even when the backend populates them.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_get_availability<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args_map) = extract_account_id(args)?;

    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let principal_id_str = args_map
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JmapError::invalid_arguments("id is required"))?;
    let principal_id = Id::from(principal_id_str);

    // utcStart / utcEnd are UTCDate values per the calendars draft §X.
    // Client-supplied values MUST be valid RFC 8620 §1.4 UTCDate (20-char
    // YYYY-MM-DDTHH:MM:SSZ). Validate via UTCDate::new_validated; malformed
    // input produces invalidArguments rather than silently flowing into
    // downstream string compares with undefined ordering.
    let utc_start = args_map
        .get("utcStart")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JmapError::invalid_arguments("utcStart is required"))?;
    let utc_start = UTCDate::new_validated(utc_start)
        .map_err(|_| JmapError::invalid_arguments("utcStart: invalid UTCDate"))?;

    let utc_end = args_map
        .get("utcEnd")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JmapError::invalid_arguments("utcEnd is required"))?;
    let utc_end = UTCDate::new_validated(utc_end)
        .map_err(|_| JmapError::invalid_arguments("utcEnd: invalid UTCDate"))?;

    // §2.2: utcStart is inclusive and utcEnd is exclusive, defining the
    // half-open interval `[utcStart, utcEnd)`. An empty or reversed
    // interval is semantically meaningless: the relevance predicate
    // ('event finishes after utcStart AND starts before utcEnd') gives
    // an empty set, and a backend that translates the window into a
    // SQL `BETWEEN` or a recurrence-expansion loop could silently
    // return wrong results or, worse, loop unboundedly on reversed
    // bounds. Reject before the backend sees the call.
    //
    // UTCDate is RFC 8620 §1.4 `YYYY-MM-DDTHH:MM:SSZ` (20 chars,
    // fixed-width zero-padded, `Z` suffix) — lexical string ordering
    // coincides with chronological ordering, so comparing `as_ref()`
    // is correct without adding a PartialOrd derive to UTCDate
    // (workspace foundation crate). bd:JMAP-ic0j.5.
    if utc_end.as_ref() <= utc_start.as_ref() {
        return Err(JmapError::invalid_arguments(
            "utcEnd must be strictly after utcStart",
        ));
    }

    let show_details = bool_arg(&args_map, "showDetails", false);

    let event_properties: Option<Vec<String>> = args_map
        .get("eventProperties")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                .collect()
        });

    match backend
        .get_availability(
            caller,
            &account_id,
            &principal_id,
            &utc_start,
            &utc_end,
            show_details,
            event_properties.as_deref(),
        )
        .await
    {
        Ok(busy_list) => {
            let list_json = serde_json::to_value(&busy_list).unwrap_or(Value::Array(vec![]));
            Ok((
                json!({
                    "accountId": account_id.as_ref(),
                    "list": list_json,
                }),
                vec![],
            ))
        }
        Err(AvailabilityError::NotFound) => Err(JmapError::not_found()),
        Err(AvailabilityError::Forbidden) => Err(JmapError::forbidden()),
        Err(AvailabilityError::TooLarge) => Err(JmapError::too_large()),
        Err(AvailabilityError::RateLimit) => Err(JmapError::rate_limit()),
        // bd:JMAP-ic0j.53 — match the rest of this crate (and every other
        // handler that wraps `Self::Error`): use `server_fail_from_backend`
        // so the wire `description` is the constant `SERVER_FAIL_INTERNAL_DESC`
        // rather than the backend error's `Display` output. This preserves the
        // workspace-wide rule that backend-error text MUST NOT reach the wire.
        Err(AvailabilityError::Other(e)) => Err(server_fail_from_backend(&e)),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: §2.2 — default backend returns an empty `list`.
    #[tokio::test]
    async fn get_availability_returns_empty_list_by_default() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "id": "principal1",
            "utcStart": "2024-06-15T09:00:00Z",
            "utcEnd": "2024-06-15T10:00:00Z"
        });
        let (resp, extra) = handle_principal_get_availability(&backend, &(), args)
            .await
            .expect("must succeed");
        assert!(extra.is_empty());
        assert_eq!(resp["accountId"], "acc1");
        assert!(
            resp["list"].as_array().unwrap().is_empty(),
            "list must be empty by default: {resp}"
        );
    }

    /// Oracle: §2.2 — missing `id` argument must return `invalidArguments`.
    #[tokio::test]
    async fn get_availability_missing_id_returns_error() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "utcStart": "2024-06-15T09:00:00Z",
            "utcEnd": "2024-06-15T10:00:00Z"
        });
        let err = handle_principal_get_availability(&backend, &(), args)
            .await
            .expect_err("missing id must return error");
        assert_eq!(
            err.error_type.as_str(),
            "invalidArguments",
            "wrong error type: {err:?}"
        );
    }

    /// Oracle: §2.2 — unknown accountId must return `accountNotFound`.
    #[tokio::test]
    async fn get_availability_unknown_account_returns_error() {
        let backend = MockBackend::new();
        let args = json!({
            "accountId": "no-such-account",
            "id": "principal1",
            "utcStart": "2024-06-15T09:00:00Z",
            "utcEnd": "2024-06-15T10:00:00Z"
        });
        let err = handle_principal_get_availability(&backend, &(), args)
            .await
            .expect_err("must return error for unknown account");
        assert_eq!(
            err.error_type.as_str(),
            "accountNotFound",
            "wrong error type: {err:?}"
        );
    }

    /// Regression for bd:JMAP-ic0j.5: a reversed window (utcEnd
    /// strictly earlier than utcStart) must be rejected with
    /// `invalidArguments` before the backend sees the call.
    ///
    /// Oracle: draft-ietf-jmap-calendars-26 §2.2 defines the window
    /// `[utcStart, utcEnd)` (inclusive/exclusive); the relevance
    /// predicate at §2.2 ('event finishes after utcStart AND starts
    /// before utcEnd') is only satisfiable when utcStart < utcEnd.
    /// A backend that naively translates the window into a database
    /// range query or a recurrence-expansion loop could return wrong
    /// results or loop unboundedly on a reversed window.
    #[tokio::test]
    async fn get_availability_reversed_window_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "id": "principal1",
            "utcStart": "2024-06-15T10:00:00Z",
            "utcEnd": "2024-06-15T09:00:00Z"
        });
        let err = handle_principal_get_availability(&backend, &(), args)
            .await
            .expect_err("reversed window must return error");
        assert_eq!(
            err.error_type.as_str(),
            "invalidArguments",
            "wrong error type: {err:?}"
        );
    }

    /// Regression for bd:JMAP-ic0j.5: a zero-width window
    /// (utcEnd == utcStart) is also rejected with `invalidArguments`
    /// because the half-open interval `[T, T)` is empty by definition
    /// and a query against it is pathological.
    #[tokio::test]
    async fn get_availability_zero_width_window_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "id": "principal1",
            "utcStart": "2024-06-15T09:00:00Z",
            "utcEnd": "2024-06-15T09:00:00Z"
        });
        let err = handle_principal_get_availability(&backend, &(), args)
            .await
            .expect_err("zero-width window must return error");
        assert_eq!(
            err.error_type.as_str(),
            "invalidArguments",
            "wrong error type: {err:?}"
        );
    }

    /// Regression for bd:JMAP-ic0j.53: `AvailabilityError::Other(e)` must
    /// surface to the wire as the constant
    /// [`jmap_server::SERVER_FAIL_INTERNAL_DESC`] description, NOT the
    /// backend error's `Display` output.
    ///
    /// Oracle: `jmap_server::handlers::server_fail_from_backend` is
    /// documented as deliberately discarding the backend error text to
    /// prevent leakage of internal state (account ids, database error
    /// text, principal ids, etc.) into JMAP wire responses. Every other
    /// backend-error site in this crate already uses that helper; the
    /// `AvailabilityError::Other` arm used to call
    /// `JmapError::server_fail(e.to_string())` directly, which bypassed
    /// the redaction. This test asserts the bypass is gone.
    ///
    /// Independent oracle: the canary string
    /// `"SECRET-ACCOUNT-ID=12345-leakage-canary"` is supplied by THIS
    /// test as the backend's `Display` output. If the wire `description`
    /// ever contains the canary, the redaction broke. The assertion
    /// against the constant `SERVER_FAIL_INTERNAL_DESC` is symmetric with
    /// the canonical redaction test in
    /// `crate-jmap-server/src/handlers.rs::server_fail_from_backend_drops_display_text`.
    #[tokio::test]
    async fn get_availability_other_error_is_redacted_on_the_wire() {
        use jmap_server::{
            BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
            JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError as JsSetError,
            SetErrorType as JsSetErrorType, SetObject, SERVER_FAIL_INTERNAL_DESC,
        };
        use jmap_types::State;

        const CANARY: &str = "SECRET-ACCOUNT-ID=12345-leakage-canary";

        #[derive(Debug)]
        struct FaultyError(&'static str);
        impl std::fmt::Display for FaultyError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
        impl std::error::Error for FaultyError {}

        struct FaultyBackend;

        impl JmapBackend for FaultyBackend {
            type Error = FaultyError;
            type CallerCtx = ();

            async fn account_exists(
                &self,
                _caller: &(),
                _account_id: &Id,
            ) -> Result<bool, Self::Error> {
                Ok(true)
            }

            async fn get_objects<O: GetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _ids: Option<&[Id]>,
                _properties: Option<&[String]>,
            ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
                Ok((vec![], vec![]))
            }

            async fn get_state<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
            ) -> Result<State, Self::Error> {
                Ok(State::from("0"))
            }

            async fn get_changes<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _since_state: &State,
                _max_changes: Option<u64>,
            ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
                Ok(ChangesResult::new(
                    vec![],
                    vec![],
                    vec![],
                    false,
                    State::from("0"),
                ))
            }

            async fn query_objects<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _limit: Option<u64>,
                _position: i64,
            ) -> Result<QueryResult, Self::Error> {
                Ok(QueryResult::new(
                    vec![],
                    0,
                    Some(0),
                    State::from("0"),
                    false,
                ))
            }

            async fn query_changes<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                since_query_state: &State,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _max_changes: Option<u64>,
                _up_to_id: Option<&Id>,
                _collapse_threads: bool,
            ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
                Ok(QueryChangesResult::new(
                    since_query_state.clone(),
                    State::from("0"),
                    Some(0),
                    vec![],
                    vec![],
                ))
            }
        }

        impl CalendarsBackend for FaultyBackend {
            async fn create_object<O: SetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _create_id: &str,
                obj: O,
            ) -> Result<(Id, O), BackendSetError<Self::Error>> {
                Ok((Id::from("mock-id"), obj))
            }

            async fn update_object<O: SetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _id: &Id,
                _patch: O::Patch,
            ) -> Result<Option<O>, BackendSetError<Self::Error>> {
                Err(BackendSetError::SetError(JsSetError::new(
                    JsSetErrorType::NotFound,
                )))
            }

            async fn destroy_object<O: SetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _id: &Id,
            ) -> Result<(), BackendSetError<Self::Error>> {
                Ok(())
            }

            fn supports_type<O: JmapObject>(&self) -> bool {
                true
            }

            async fn calendar_has_events(
                &self,
                _caller: &(),
                _account_id: &Id,
                _calendar_id: &Id,
            ) -> Result<bool, Self::Error> {
                Ok(false)
            }

            async fn get_availability(
                &self,
                _caller: &(),
                _account_id: &Id,
                _principal_id: &Id,
                _utc_start: &UTCDate,
                _utc_end: &UTCDate,
                _show_details: bool,
                _event_properties: Option<&[String]>,
            ) -> Result<Vec<jmap_calendars_types::BusyPeriod>, AvailabilityError<Self::Error>>
            {
                Err(AvailabilityError::Other(FaultyError(CANARY)))
            }
        }

        let backend = FaultyBackend;
        let args = json!({
            "accountId": "acc1",
            "id": "principal1",
            "utcStart": "2024-06-15T09:00:00Z",
            "utcEnd": "2024-06-15T10:00:00Z"
        });
        let err = handle_principal_get_availability(&backend, &(), args)
            .await
            .expect_err("backend Other error must surface as JmapError");

        assert_eq!(
            err.error_type.as_str(),
            "serverFail",
            "wrong error type: {err:?}"
        );
        // The canary MUST NOT appear in the wire description — that is
        // the leakage this test exists to catch.
        let desc = err.description.as_deref().unwrap_or("");
        assert!(
            !desc.contains(CANARY),
            "wire description leaked backend error text containing canary: {desc:?}"
        );
        // The description MUST be exactly the constant the redaction
        // helper produces. This pins the contract: if the helper ever
        // changes, the symmetric test in crate-jmap-server will catch it
        // there too.
        assert_eq!(
            desc, SERVER_FAIL_INTERNAL_DESC,
            "wire description must be the constant SERVER_FAIL_INTERNAL_DESC, \
             not the backend error text: {desc:?}"
        );
    }
}
