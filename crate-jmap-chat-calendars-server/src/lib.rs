//! Chat binding for the JMAP Calendars method handlers: the rules of
//! draft-atwood-jmap-chat-calendars-00 that apply when a server offers
//! `urn:ietf:params:jmap:chat:calendars` together with
//! `urn:ietf:params:jmap:chat` and `urn:ietf:params:jmap:calendars`.
//!
//! `jmap-calendars-server` and `jmap-chat-server` do not depend on each
//! other. A deployment that offers both implements
//! [`jmap_calendars_server::CalendarsBackend`] and `ChatBackend` and calls
//! these functions from its trait methods and Session builder:
//!
//! | Where | Call |
//! |---|---|
//! | Session object, before advertising `urn:ietf:params:jmap:chat:calendars` on an account | [`may_advertise`] |
//! | `ChatBackend::space_extension_properties` | [`space_extension_properties`] |
//! | `ChatBackend::create_object::<Space>` and `ChatBackend::apply_space_metadata_patch`, for a `calendarId` value | [`check_calendar_id`] |
//! | `ChatBackend::create_object::<Space>`, before the insert, for auto-bind on create | [`create_space_calendar`] |
//!
//! Space destroy needs no call: this crate never destroys a Calendar
//! (§Binding and unbinding: "the bound Calendar SHOULD NOT be
//! automatically destroyed unless explicitly requested by the destroying
//! member"). The draft gives the destroying member no way to make that
//! request on `Space/set`, so the Calendar always persists; a member who
//! wants it gone destroys it with `Calendar/set`.

#![forbid(unsafe_code)]

use jmap_calendars_server::{BackendSetError, CalendarsBackend, SetError, SetErrorType};
use jmap_calendars_types::{Calendar, JMAP_CALENDARS_URI};
use jmap_chat_types::{ChatCalendarsCapability, Space, JMAP_CHAT_URI, SPACE_CALENDAR_ID_PROPERTY};
use jmap_types::Id;
use serde_json::Value;

/// Whether an account may advertise `urn:ietf:params:jmap:chat:calendars`,
/// given the capability URIs it already advertises.
///
/// draft-atwood-jmap-chat-calendars-00 §Capability: "This capability is
/// meaningful only when both `urn:ietf:params:jmap:chat` and
/// `urn:ietf:params:jmap:calendars` are also advertised on the same
/// account." §Relationship to JMAP Calendars: "A deployment that supports
/// JMAP Chat but not JMAP Calendars MUST NOT advertise the capability
/// defined here." Returns `true` only when both URIs are present.
pub fn may_advertise<'a>(account_capabilities: impl IntoIterator<Item = &'a str>) -> bool {
    let (mut chat, mut calendars) = (false, false);
    for uri in account_capabilities {
        chat |= uri == JMAP_CHAT_URI;
        calendars |= uri == JMAP_CALENDARS_URI;
    }
    chat && calendars
}

/// The Space extension properties to return from
/// `ChatBackend::space_extension_properties` for an account advertising
/// `capability`: `calendarId` when `mayBindCalendar` is `true`, none
/// otherwise.
///
/// With `mayBindCalendar: false` ("Deployments that prohibit
/// Space-Calendar binding ... MUST set this to `false`") no Space is
/// bound: the `Space/set` handler does not store `calendarId` on create
/// and rejects it in an update patch as `invalidProperties`. With `true`,
/// the handler stores it on create and routes a `calendarId` patch through
/// `apply_space_metadata_patch`, whose `manage_space` gate gives
/// §Binding and unbinding: servers "MUST accept patches to
/// `Space.calendarId` from members holding the `"manage_space"`
/// permission".
pub fn space_extension_properties(capability: &ChatCalendarsCapability) -> &'static [&'static str] {
    if capability.may_bind_calendar {
        &[SPACE_CALENDAR_ID_PROPERTY]
    } else {
        &[]
    }
}

/// Check a `calendarId` value from a `Space/set` create or update of a
/// Space on `account_id`.
///
/// - `null` unbinds the Space (§Binding and unbinding: "setting it to
///   `null` unbinds the Space") and passes. The Calendar is not touched:
///   it "may be unbound from the Space without being deleted".
/// - Any other value must be an `Id` naming a Calendar that `calendars`
///   returns for `account_id`: "The bound Calendar MUST belong to the same
///   account that owns the Space" (§calendarId), and "A Space's
///   `calendarId` MUST reference a Calendar on the same server"
///   (§Federation). Otherwise the result is `invalidProperties` naming
///   `calendarId`, as `ChatBackend::space_extension_properties` asks.
///
/// `caller` is the requesting user's Calendars context, so a Calendar the
/// user cannot see is refused like a missing one. Call this after the
/// `manage_space` gate of `apply_space_metadata_patch`; on create, the
/// Space's creator is its first member.
///
/// Returns `Ok(Err(set_error))` when the value must be refused; return
/// that SetError without storing the Space.
///
/// # Errors
///
/// Returns the Calendars backend's error if reading the Calendar fails.
pub async fn check_calendar_id<K: CalendarsBackend>(
    calendars: &K,
    caller: &K::CallerCtx,
    account_id: &Id,
    value: &Value,
) -> Result<Result<(), SetError>, K::Error> {
    let calendar_id = match value {
        Value::Null => return Ok(Ok(())),
        Value::String(s) => match Id::new_validated(s.as_str()) {
            Ok(id) => id,
            Err(_) => return Ok(Err(invalid("calendarId must be a valid Id"))),
        },
        _ => return Ok(Err(invalid("calendarId must be a String or null"))),
    };
    let ids = std::slice::from_ref(&calendar_id);
    let (found, _) = calendars
        .get_objects::<Calendar>(caller, account_id, Some(ids), None)
        .await?;
    Ok(
        if found.iter().any(|c| c.id.as_ref() == Some(&calendar_id)) {
            Ok(())
        } else {
            Err(invalid(
                "calendarId must name a Calendar on the account that owns the Space",
            ))
        },
    )
}

/// Auto-bind on create: create a Calendar for the new `space` on
/// `account_id` and set the Space's `calendarId` to it, for a server
/// advertising `mayBindCalendar: true` that binds every new Space.
///
/// §Binding and unbinding: "Servers MAY automatically create a new
/// Calendar when a Space is created and bind it ... When this is done, the
/// server MUST set `calendarId` to the new Calendar's id at Space creation
/// time." Call it from `ChatBackend::create_object::<Space>` before the
/// insert, so the stored Space and the `Space/set` `created` entry carry
/// the binding.
///
/// A `space` that already names a Calendar (the client supplied a
/// non-null `calendarId`, checked with [`check_calendar_id`]) is left as
/// it is and nothing is created.
///
/// The Calendar is named after the Space, subscribed, not the default,
/// with `includeInAvailability: "all"` and every right for the account
/// owner. The [`CalendarsBackend::create_object`] builder sets the `id`
/// the backend assigns; that contract records the create for
/// `Calendar/changes`. `caller` is a server context.
///
/// Returns the bound Calendar's id.
///
/// # Errors
///
/// Returns the backend's error from `create_object`.
// Same error type as `CalendarsBackend::create_object`, which it propagates.
pub async fn create_space_calendar<K: CalendarsBackend>(
    calendars: &K,
    caller: &K::CallerCtx,
    account_id: &Id,
    space: &mut Space,
) -> Result<Id, BackendSetError<K::Error>> {
    if let Some(Value::String(existing)) = space.extra.get(SPACE_CALENDAR_ID_PROPERTY) {
        return Ok(Id::from(existing.as_str()));
    }
    let mut calendar: Calendar = serde_json::from_value(serde_json::json!({
        "name": space.name,
        "isSubscribed": true,
        "isDefault": false,
        "includeInAvailability": "all",
        "myRights": {
            "mayReadFreeBusy": true,
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayWriteOwn": true,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "mayShare": true,
            "mayDelete": true,
        },
    }))
    .expect("a Calendar with every required field deserializes");
    let (calendar_id, _) = calendars
        .create_object(caller, account_id, "spaceCalendar", move |id: &Id| {
            calendar.id = Some(id.clone());
            calendar
        })
        .await?;
    space.extra.insert(
        SPACE_CALENDAR_ID_PROPERTY.to_owned(),
        Value::String(calendar_id.as_ref().to_owned()),
    );
    Ok(calendar_id)
}

/// An `invalidProperties` SetError naming `calendarId`.
fn invalid(description: &str) -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties([SPACE_CALENDAR_ID_PROPERTY])
        .with_description(description)
}
