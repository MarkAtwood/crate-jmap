//! Ephemeral Scene event subscription state (draft-atwood-jmap-scene-wss-00
//! §SceneStreamEnable).
//!
//! A WebSocket transport keeps one [`SceneEphemeralSubscription`] per
//! connection, starting from [`SceneEphemeralSubscription::default`]
//! (inactive). Each `SceneStreamEnable` frame goes through
//! [`SceneEphemeralSubscription::replace_with`], which validates the frame,
//! normalizes it, and replaces the whole subscription. A
//! `SceneStreamDisable` frame resets it to the default, which "MUST
//! silently succeed if no ephemeral Scene subscription is currently
//! active" (§SceneStreamDisable).
//!
//! # Active avatars are checked at delivery
//!
//! `regionIds` is kept as sent. An entry naming a region in which the user
//! has no active avatar "produces no events but is retained in the
//! subscription record" (§SceneStreamEnable, Semantics), so whether a
//! region produces events is decided per event at delivery time, not here.
//! That lets events "begin (or resume) without a new `SceneStreamEnable`"
//! when the user enters or re-enters a listed region.
//!
//! # Delivery and relay
//!
//! Before sending an event to a connection, the transport asks
//! [`should_deliver`] whether that connection may receive it. A
//! `SceneInteractionEvent` frame a client sends goes through
//! [`validate_interaction`] first; the transport silently discards it on
//! `None` and otherwise relays the returned event, whose `userId` is the
//! sender's, to every connection for which [`should_deliver`] holds
//! (§Client-to-Server Submission).

use jmap_scene_types::{
    ObjectEventKind, SceneAvatar, SceneEvent, SceneInteractionEvent, SceneObject,
    SceneStreamEnable, SCENE_EVENT_TYPES,
};
use jmap_types::{Id, JmapError};

use crate::helpers::is_visible;
use crate::region::JMAP_CHAT_URI;
use crate::SceneBackend;

/// Which regions a subscription covers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionScope {
    /// The frame sent `null` or left `regionIds` out. The subscription is
    /// dynamic: every region in which the user has an active avatar,
    /// adjusting as the user enters and leaves regions (§Subscription
    /// Scope).
    All,
    /// The frame sent an explicit list, in the order sent. The
    /// subscription is static: only these regions, and of them only those
    /// with an active avatar of the user at delivery time. An empty list
    /// covers no region.
    Only(Vec<Id>),
}

impl RegionScope {
    /// Whether `region_id` is in scope. Always `true` for
    /// [`RegionScope::All`]; the active-avatar check is separate.
    pub fn covers(&self, region_id: &Id) -> bool {
        match self {
            RegionScope::All => true,
            RegionScope::Only(ids) => ids.contains(region_id),
        }
    }
}

/// The active filters of an enabled subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneStreamFilter {
    region_ids: RegionScope,
    event_types: Option<Vec<&'static str>>,
}

impl SceneStreamFilter {
    /// The regions covered.
    pub fn region_ids(&self) -> &RegionScope {
        &self.region_ids
    }

    /// Whether `event_type` (an `@type` such as `"SceneAvatarEvent"`) was
    /// requested. Always `true` when the frame's `eventTypes` was `null`.
    pub fn includes(&self, event_type: &str) -> bool {
        self.event_types
            .as_ref()
            .is_none_or(|types| types.contains(&event_type))
    }
}

/// The ephemeral Scene subscription of one WebSocket connection.
///
/// The default value is inactive, which is the state before the first
/// `SceneStreamEnable` and after a `SceneStreamDisable`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SceneEphemeralSubscription {
    active: Option<SceneStreamFilter>,
}

impl SceneEphemeralSubscription {
    /// Replaces this subscription with the one in `enable`.
    ///
    /// The new frame "REPLACES any previous `SceneStreamEnable`
    /// subscription in its entirety"; nothing is merged from it.
    /// Unrecognized `eventTypes` values next to recognized ones are dropped
    /// (draft-atwood-jmap-scene-wss-00 §SceneStreamEnable, Semantics).
    ///
    /// # Errors
    ///
    /// Returns an `invalidArguments` [`JmapError`] when `eventTypes` is an
    /// empty array or holds only values outside [`SCENE_EVENT_TYPES`]. On
    /// error `self` is unchanged, as the draft requires. The transport
    /// sends the error as an RFC 8887 §4.3.4 `RequestError` frame whose
    /// `type` is the full URN
    /// `"urn:ietf:params:jmap:error:invalidArguments"` and whose
    /// `requestId` is [`SceneStreamEnable::id`] when the client sent one;
    /// the returned [`JmapError`] carries the short method-level name. The
    /// connection stays open.
    pub fn replace_with(&mut self, enable: &SceneStreamEnable) -> Result<(), JmapError> {
        let event_types = match &enable.event_types {
            None => None,
            Some(requested) => {
                let recognized: Vec<&'static str> = SCENE_EVENT_TYPES
                    .iter()
                    .copied()
                    .filter(|t| requested.iter().any(|r| r.as_str() == *t))
                    .collect();
                if recognized.is_empty() {
                    return Err(JmapError::invalid_arguments(
                        "eventTypes contains no recognized values",
                    ));
                }
                Some(recognized)
            }
        };
        self.active = Some(SceneStreamFilter {
            region_ids: enable
                .region_ids
                .clone()
                .map_or(RegionScope::All, RegionScope::Only),
            event_types,
        });
        Ok(())
    }

    /// The active filters, or `None` when no subscription is enabled.
    pub fn filter(&self) -> Option<&SceneStreamFilter> {
        self.active.as_ref()
    }
}

/// Whether `event` may be delivered on a connection whose subscription is
/// `subscription`.
///
/// `caller` is the connection's authenticated identity, the recipient.
/// The transport calls this at delivery time, for each event and
/// connection. The originating user is never told of a suppression
/// (draft-atwood-jmap-scene-wss-00 §Blocked-Sender Suppression).
///
/// Delivery needs an active subscription whose `eventTypes` includes the
/// event's `@type`; an unrecognized `@type` is never delivered. All
/// three events are per-region (§Event Summary): the event's region must
/// be in the subscription's [`RegionScope`], the recipient must have
/// access to it ([`SceneBackend::may_access_region`]) and an active
/// avatar (`leftAt` `null`) in it. Then:
///
/// - `SceneAvatarEvent` needs the recipient to see the region's avatars
///   ([`SceneBackend::may_see_region_avatars`]): a region bound to an
///   E2EE VTCCall suppresses them toward non-participants
///   (draft-atwood-jmap-scene-00 at 6bcd9d8, L2654), through
///   `SceneStreamEnable` and `ChatStreamEnable` alike
///   (draft-atwood-jmap-scene-wss-00 at 6bcd9d8, L1233-1234).
/// - `SceneObjectEvent` and `SceneInteractionEvent` need the named object
///   to pass the `SceneObject/get` visibility contract for the recipient
///   ([`SceneBackend::is_object_visible`]; §Visibility Filtering). A
///   `"destroyed"` event whose object the backend no longer returns has
///   no record to check and needs region access only; any other event
///   for an object the backend does not return is dropped.
/// - When the recipient's account has `urn:ietf:params:jmap:chat`
///   ([`SceneBackend::has_capability`]), the event is dropped if the
///   recipient blocked its originating user
///   ([`SceneBackend::is_user_blocked`]): the `userId` of a
///   `SceneAvatarEvent` or `SceneInteractionEvent`, the `updatedBy` of a
///   `SceneObjectEvent`. A `SceneObjectEvent` with `updatedBy` `null` is
///   never suppressed. Without Chat no blocked-sender filtering applies.
///
/// `hosting_account_id` is the account the region, avatars and object are
/// read from. It must be the event's `accountId`, "the id of the region's
/// hosting account ... Other users reach that account as a shared
/// account" (draft-atwood-jmap-scene-wss-00 at 33702a5, L178-181); an
/// event whose `accountId` differs is not delivered, since "SceneRegion
/// and SceneObject ids are unique only within an account".
/// `recipient_account_id` is the recipient's own account: the Chat
/// capability is checked there ("available for the receiving account",
/// L791-792). `caller` is the recipient, so
/// [`SceneBackend::is_user_blocked`] consults "the recipient's ChatContact
/// records" (L793-795).
///
/// This function does not rate-limit; the §Rate Limiting SHOULD rules are
/// the transport's or deployment's to apply.
///
/// # Errors
///
/// Returns the backend's error if reading the recipient's userId, its
/// avatars, the object, region access, visibility, avatar visibility or
/// block status fails.
pub async fn should_deliver<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    hosting_account_id: &Id,
    recipient_account_id: &Id,
    subscription: &SceneEphemeralSubscription,
    event: &SceneEvent,
) -> Result<bool, B::Error> {
    let Some(filter) = subscription.filter() else {
        return Ok(false);
    };
    if !filter.includes(event.type_name()) {
        return Ok(false);
    }
    let (event_account, region_id, object, origin) = match event {
        SceneEvent::Avatar(e) => (&e.account_id, &e.region_id, None, Some(e.user_id.as_str())),
        SceneEvent::Object(e) => (
            &e.account_id,
            &e.region_id,
            Some((&e.object_id, e.event == ObjectEventKind::Destroyed)),
            e.updated_by.as_deref(),
        ),
        SceneEvent::Interaction(e) => (
            &e.account_id,
            &e.region_id,
            Some((&e.object_id, false)),
            Some(e.user_id.as_str()),
        ),
        _ => return Ok(false),
    };
    if event_account != hosting_account_id
        || !filter.region_ids().covers(region_id)
        || !backend
            .may_access_region(caller, hosting_account_id, region_id)
            .await?
    {
        return Ok(false);
    }
    let me = backend.caller_user_id(caller, hosting_account_id).await?;
    if !has_active_avatar(backend, caller, hosting_account_id, &me, region_id).await? {
        return Ok(false);
    }
    if matches!(event, SceneEvent::Avatar(_))
        && !backend
            .may_see_region_avatars(caller, hosting_account_id, region_id)
            .await?
    {
        return Ok(false);
    }
    if let Some((object_id, destroyed)) = object {
        let visible = match fetch_object(backend, caller, hosting_account_id, object_id).await? {
            Some(o) => {
                o.region_id == *region_id
                    && is_visible(backend, caller, hosting_account_id, &o).await?
            }
            None => destroyed,
        };
        if !visible {
            return Ok(false);
        }
    }
    Ok(match origin {
        Some(user) if backend.has_capability(caller, recipient_account_id, JMAP_CHAT_URI) => {
            !backend.is_user_blocked(caller, user).await?
        }
        _ => true,
    })
}

/// The `SceneInteractionEvent` to relay for a frame a client sent, or
/// `None` to discard it (draft-atwood-jmap-scene-wss-00 §Client-to-Server
/// Submission).
///
/// `caller` is the sending connection's authenticated identity and
/// `account_id` the account the region and object are read from. "The
/// server sets `userId` on the relayed event to the authenticated user's
/// userId; a `userId` supplied in the frame is ignored", so the returned
/// event is `event` with `userId` replaced by the caller's. The frame is
/// relayable when its `accountId` is `account_id`, the caller has an
/// active avatar (`leftAt` `null`) "in the region identified by
/// `accountId` and `regionId`", and its `objectId` is a SceneObject in that
/// region with `interactable` `true`. On `None` the server "discards the
/// frame and returns no error to the sender"; on `Some` it relays the
/// returned event, filtered per recipient by [`should_deliver`].
///
/// The opaque `data` is not inspected; size limits on it are
/// deployment-defined, as is the §Rate Limiting SHOULD of five events per
/// user per region per second.
///
/// # Errors
///
/// Returns the backend's error if reading the caller's userId, its
/// avatars or the object fails.
pub async fn validate_interaction<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    event: &SceneInteractionEvent,
) -> Result<Option<SceneInteractionEvent>, B::Error> {
    if event.account_id != *account_id {
        return Ok(None);
    }
    let me = backend.caller_user_id(caller, account_id).await?;
    if !has_active_avatar(backend, caller, account_id, &me, &event.region_id).await? {
        return Ok(None);
    }
    let interactable = fetch_object(backend, caller, account_id, &event.object_id)
        .await?
        .is_some_and(|o| o.region_id == event.region_id && o.interactable);
    Ok(interactable.then(|| {
        let mut relayed = event.clone();
        relayed.user_id = me;
        relayed
    }))
}

/// `true` if `user_id` has an avatar in `region_id` with `leftAt` `null`.
///
/// ponytail: reads every SceneAvatar in the account; O(avatars) per
/// check. Upgrade: a backend hook for the user's active avatar once a
/// deployment needs it.
pub(crate) async fn has_active_avatar<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    user_id: &str,
    region_id: &Id,
) -> Result<bool, B::Error> {
    let (avatars, _) = backend
        .get_objects::<SceneAvatar>(caller, account_id, None, None)
        .await?;
    Ok(avatars
        .iter()
        .any(|a| a.region_id == *region_id && a.user_id == user_id && a.left_at.is_none()))
}

async fn fetch_object<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    object_id: &Id,
) -> Result<Option<SceneObject>, B::Error> {
    let (objects, _) = backend
        .get_objects::<SceneObject>(
            caller,
            account_id,
            Some(std::slice::from_ref(object_id)),
            None,
        )
        .await?;
    Ok(objects.into_iter().next())
}
