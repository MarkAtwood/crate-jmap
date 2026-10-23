//! `SceneObject/interact` (draft-atwood-jmap-scene-00
//! §SceneObject/interact).
//!
//! Interactions are ephemeral: the handler changes no record or state
//! string. Each entry of `interactions` is checked on its own, in this
//! order, and the first failure is its SetError:
//!
//! 1. a missing or wrongly typed `regionId`, `objectId`, `action` or
//!    `data` is `invalidProperties` naming it;
//! 2. a region the caller has no access to
//!    ([`SceneBackend::may_access_region`]), an object that does not exist
//!    or is in another region, and an object outside the caller's
//!    visibility set ([`SceneBackend::is_object_visible`]) are `notFound`,
//!    so the error never confirms that a hidden object exists;
//! 3. no active SceneAvatar of the caller in the region, or an object
//!    with `interactable: false`, is `forbidden`;
//! 4. [`SceneBackend::accept_interaction`] returning `false` is
//!    `rateLimit`; `true` accepts the interaction.

use jmap_scene_types::{SceneInteraction, SceneInteractionEvent, SceneObject};
use jmap_types::{Id, JmapError};
use serde_json::{Map, Value, json};

use crate::SceneBackend;
use crate::ephemeral::has_active_avatar;
use crate::helpers::{
    extract_account_id, fetch_one, forbidden, invalid, is_visible, not_found, set_error_value,
};
use jmap_server::{HandlerOutput, SetError, SetErrorType, server_fail_value_from_backend};

/// Handle `SceneObject/interact`.
///
/// The response is `accountId`, `accepted` (the accepted client ids, or
/// `null` if none) and `notAccepted` (client id to SetError, or `null` if
/// none). An accepted interaction goes to
/// [`SceneBackend::accept_interaction`] as a [`SceneInteractionEvent`]
/// whose `userId` is [`SceneBackend::caller_user_id`].
///
/// ponytail: the number of `interactions` is capped at
/// `maxObjectsInSet` (RFC 8620 §2), since the method "follows the shape
/// of `/set`"; more is `requestTooLarge`.
///
/// # Errors
///
/// `invalidArguments` when `interactions` is missing or not an object,
/// `accountNotFound` for an unknown account, `requestTooLarge` over the
/// cap, and `serverFail` when the backend fails reading the account.
pub async fn handle_scene_object_interact<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    let Some(Value::Object(interactions)) = args.remove("interactions") else {
        return Err(JmapError::invalid_arguments(
            "interactions must be an Id[SceneInteraction] map",
        ));
    };
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| jmap_server::server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    if interactions.len() as u64 > backend.max_objects_in_set(caller, &account_id) {
        let mut err = JmapError::request_too_large();
        err.description = Some("maxObjectsInSet".into());
        return Err(err);
    }

    let mut accepted = Vec::new();
    let mut not_accepted = Map::new();
    for (client_id, raw) in interactions {
        match interact(backend, caller, &account_id, raw).await {
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

/// Check one interaction and hand it to
/// [`SceneBackend::accept_interaction`]; `Err` is its SetError JSON.
async fn interact<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    raw: Value,
) -> Result<(), Value> {
    let interaction = parse(raw)?;
    let fail = |e: B::Error| server_fail_value_from_backend(&e);
    let object = fetch_one::<SceneObject, B>(backend, caller, account_id, &interaction.object_id)
        .await?
        .filter(|o| o.region_id == interaction.region_id);
    let Some(object) = object else {
        return Err(not_found());
    };
    // Region access, then the visibility set.
    if !is_visible(backend, caller, account_id, &object)
        .await
        .map_err(fail)?
    {
        return Err(not_found());
    }
    let me = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(fail)?;
    if !has_active_avatar(backend, caller, account_id, &me, &object.region_id)
        .await
        .map_err(fail)?
    {
        return Err(forbidden("The caller has no active avatar in the region."));
    }
    if !object.interactable {
        return Err(forbidden("The object is not interactable."));
    }
    // The interaction's extension properties stay out of the event: a
    // flattened `userId` among them would contradict the server-set one.
    let mut event = SceneInteractionEvent::new(
        account_id.clone(),
        interaction.region_id,
        interaction.object_id,
        me,
        interaction.action,
    );
    event.data = interaction.data;
    if !backend
        .accept_interaction(caller, account_id, &event)
        .await
        .map_err(fail)?
    {
        return Err(set_error_value(
            &SetError::new(SetErrorType::RateLimit)
                .with_description("The interaction rate was exceeded."),
        ));
    }
    Ok(())
}

/// Decode one SceneInteraction: "`invalidProperties` | A property is
/// missing or has the wrong type; `properties` names it."
fn parse(raw: Value) -> Result<SceneInteraction, Value> {
    let Value::Object(m) = &raw else {
        return Err(invalid(&[], "A SceneInteraction must be an object."));
    };
    let bad: Vec<&str> = ["regionId", "objectId", "action"]
        .into_iter()
        .filter(|k| !m.get(*k).is_some_and(Value::is_string))
        .chain(
            m.get("data")
                .is_some_and(|d| !d.is_null() && !d.is_object())
                .then_some("data"),
        )
        .collect();
    if !bad.is_empty() {
        return Err(invalid(
            &bad,
            "A property is missing or has the wrong type.",
        ));
    }
    serde_json::from_value(raw).map_err(|_| invalid(&[], "Malformed SceneInteraction."))
}
