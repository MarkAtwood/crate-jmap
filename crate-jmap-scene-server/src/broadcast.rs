//! `SceneRegion/broadcast` (draft-atwood-jmap-scene-00 at jmap-chat-spec
//! 93f2258, §`SceneRegion/broadcast`).
//!
//! Region events are ephemeral: the handler changes no record or state
//! string. The method mirrors `SceneObject/interact`. Each entry of
//! `events` is checked on its own, in this order, and the first failure is
//! its `SetError`:
//!
//! 1. a missing or wrongly typed `regionId`, `event` or `data` is
//!    `invalidProperties` naming it;
//! 2. a region that does not exist or that the caller has no access to
//!    ([`SceneBackend::may_access_region`]) is `notFound`;
//! 3. a caller who is not the region owner or an administrator
//!    ([`SceneBackend::may_update_region`]) is `forbidden`;
//! 4. [`SceneBackend::accept_region_event`] returning `false` is
//!    `rateLimit`; `true` accepts the event.

use jmap_scene_types::{SceneRegion, SceneRegionBroadcast, SceneRegionEvent};
use jmap_types::{Id, JmapError};
use serde_json::{Map, Value, json};

use crate::SceneBackend;
use crate::helpers::{extract_account_id, fetch_one, forbidden, invalid, not_found};
use jmap_server::{
    HandlerOutput, SetError, SetErrorType, server_fail_from_backend, server_fail_value_from_backend,
};

/// Handle `SceneRegion/broadcast`.
///
/// Returns `unknownMethod` when the account's
/// [`SceneCapability::supports_region_broadcast`](jmap_scene_types::SceneCapability::supports_region_broadcast)
/// is `false`: "When `false`, a call to that method returns an
/// `unknownMethod` error". Otherwise the response is `accountId`,
/// `accepted` (the accepted client ids, or `null` if none) and
/// `notAccepted` (client id to `SetError`, or `null` if none). An accepted
/// event goes to [`SceneBackend::accept_region_event`] as a
/// [`SceneRegionEvent`] whose `accountId` is the hosting account; extension
/// properties of the submitted object are not copied into it.
///
/// ponytail: the number of `events` is capped at `maxObjectsInSet`
/// (RFC 8620 §2), as for `SceneObject/interact`; more is
/// `requestTooLarge`.
///
/// # Errors
///
/// `invalidArguments` when `events` is missing or not an object,
/// `accountNotFound` for an unknown account, `unknownMethod` without the
/// capability, `requestTooLarge` over the cap, and `serverFail` when the
/// backend fails reading the account or its capability.
pub async fn handle_scene_region_broadcast<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    let cap = backend
        .scene_capability(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if !cap.supports_region_broadcast {
        return Err(JmapError::unknown_method());
    }
    let Some(Value::Object(events)) = args.remove("events") else {
        return Err(JmapError::invalid_arguments(
            "events must be an Id[SceneRegionBroadcast] map",
        ));
    };
    if events.len() as u64 > backend.max_objects_in_set(caller, &account_id) {
        let mut err = JmapError::request_too_large();
        err.description = Some("maxObjectsInSet".into());
        return Err(err);
    }

    let mut accepted = Vec::new();
    let mut not_accepted = Map::new();
    for (client_id, raw) in events {
        match broadcast(backend, caller, &account_id, raw).await {
            Ok(()) => accepted.push(Value::String(client_id)),
            Err(e) => {
                not_accepted.insert(client_id, e);
            }
        }
    }
    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "accepted": if accepted.is_empty() { Value::Null } else { Value::Array(accepted) },
        "notAccepted": if not_accepted.is_empty() { Value::Null } else { Value::Object(not_accepted) },
    })))
}

/// Check one region event and hand it to
/// [`SceneBackend::accept_region_event`]; `Err` is its `SetError` JSON.
async fn broadcast<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    raw: Value,
) -> Result<(), Value> {
    let submitted = parse(raw)?;
    let fail = |e: B::Error| server_fail_value_from_backend(&e);
    // "The region does not exist or the caller has no access to it":
    // `notFound`.
    let region: SceneRegion = fetch_one(backend, caller, account_id, &submitted.region_id)
        .await?
        .ok_or_else(not_found)?;
    if !backend
        .may_access_region(caller, account_id, &region.id)
        .await
        .map_err(fail)?
    {
        return Err(not_found());
    }
    // "The caller is not the region owner and does not have
    // administrative privileges": `forbidden`.
    if !backend
        .may_update_region(caller, account_id, &region)
        .await
        .map_err(fail)?
    {
        return Err(forbidden(
            "only the region owner or an admin may broadcast to this region",
        ));
    }
    let mut event = SceneRegionEvent::new(account_id.clone(), region.id, submitted.event);
    event.data = submitted.data;
    if !backend
        .accept_region_event(caller, account_id, &event)
        .await
        .map_err(fail)?
    {
        return Err(serde_json::Value::from(
            &SetError::new(SetErrorType::RateLimit)
                .with_description("The region event rate was exceeded."),
        ));
    }
    Ok(())
}

/// Decode one `SceneRegionBroadcast`: "`invalidProperties` | A property is
/// missing or has the wrong type ...; `properties` names it." An empty
/// `event` is refused too, since it names no event.
fn parse(raw: Value) -> Result<SceneRegionBroadcast, Value> {
    let Value::Object(m) = &raw else {
        return Err(invalid(&[], "A SceneRegionBroadcast must be an object."));
    };
    let bad: Vec<&str> = [
        ("regionId", m.get("regionId").is_some_and(Value::is_string)),
        (
            "event",
            m.get("event")
                .and_then(Value::as_str)
                .is_some_and(|e| !e.is_empty()),
        ),
        (
            "data",
            m.get("data").is_none_or(|d| d.is_null() || d.is_object()),
        ),
    ]
    .into_iter()
    .filter_map(|(k, ok)| (!ok).then_some(k))
    .collect();
    if !bad.is_empty() {
        return Err(invalid(
            &bad,
            "A property is missing or has the wrong type.",
        ));
    }
    serde_json::from_value(raw).map_err(|_| invalid(&[], "Malformed SceneRegionBroadcast."))
}
