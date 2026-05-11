//! Principal/* method handlers (draft-ietf-jmap-calendars-26 §2).

use jmap_types::{Id, Invocation, JmapError, UTCDate};
use serde_json::{json, Value};

use crate::backend::{AvailabilityError, CalendarsBackend};
use crate::helpers::extract_account_id;

// ---------------------------------------------------------------------------
// Principal/getAvailability
// ---------------------------------------------------------------------------

/// Handle a `Principal/getAvailability` method call
/// (draft-ietf-jmap-calendars-26 §2.2).
///
/// Returns an array of [`BusyPeriod`](jmap_calendars_types::BusyPeriod) objects for the time range
/// `[utcStart, utcEnd)` for the identified principal.  If `showDetails` is
/// `false` (the default), the `event` and `accountId` fields within each
/// `BusyPeriod` are omitted even when the backend populates them.
pub async fn handle_principal_get_availability<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args_map) = extract_account_id(args)?;

    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
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

    let show_details = args_map
        .get("showDetails")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

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
            event_properties.as_deref().map(|v| v as &[String]),
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
        Err(AvailabilityError::Other(e)) => Err(JmapError::server_fail(e.to_string())),
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
}
