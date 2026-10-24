//! Space `calendarId` binding rules and capability co-advertisement.
//!
//! Oracle: draft-atwood-jmap-chat-calendars-00.md at jmap-chat-spec
//! be46208 ("calendars Lnnn") and RFC 8620:
//!
//! - calendars L73: "A deployment that supports JMAP Chat but not JMAP
//!   Calendars MUST NOT advertise the capability defined here."
//! - calendars L87: "This capability is meaningful only when both
//!   `urn:ietf:params:jmap:chat` and `urn:ietf:params:jmap:calendars` are
//!   also advertised on the same account."
//! - calendars L98: "Deployments that prohibit Space-Calendar binding ...
//!   MUST set this to `false`."
//! - calendars L111: "`calendarId` (String, optional): A JMAP Calendars
//!   `Calendar.id` ... bound to this Space."
//! - calendars L114: "The bound Calendar MUST belong to the same account
//!   that owns the Space. Cross-account Calendar references are not
//!   supported in this revision of the specification".
//! - calendars L118: "Servers that advertise `mayBindCalendar: true` MUST
//!   accept patches to `Space.calendarId` from members holding the
//!   `"manage_space"` permission. Setting `calendarId` to a non-null value
//!   binds the Space to the identified Calendar; setting it to `null`
//!   unbinds the Space."
//! - calendars L120: "Servers MAY automatically create a new Calendar when
//!   a Space is created and bind it ... When this is done, the server MUST
//!   set `calendarId` to the new Calendar's id at Space creation time."
//! - calendars L122: "When a Space is destroyed, the bound Calendar SHOULD
//!   NOT be automatically destroyed unless explicitly requested by the
//!   destroying member. The Calendar persists independently and may be
//!   unbound from the Space without being deleted."
//! - calendars L272: "A Space's `calendarId` MUST reference a Calendar on
//!   the same server."
//! - RFC 8620 §5.3 `invalidProperties`: "The record given is invalid in
//!   some way."; §5.1 `notFound`: "the list of ids passed to the method
//!   for records that do not exist".
//!
//! The `manage_space` gate of L118 is `apply_space_metadata_patch`'s, and
//! is tested in `jmap-chat-server`
//! (`registered_property_patch_without_manage_space_is_forbidden`).
//!
//! Records are hand-written JSON from the drafts' field lists.

use jmap_calendars_server::handle_calendar_get;
use jmap_calendars_server::memory::MemoryBackend as CalendarsMemory;
use jmap_chat_calendars_server::{
    check_calendar_id, create_space_calendar, may_advertise, space_extension_properties,
};
use jmap_chat_server::memory::MemoryBackend as ChatMemory;
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_chat_types::{ChatCalendarsCapability, Space};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "A1";
const OTHER_ACCOUNT: &str = "A2";
const CHAT: &str = "urn:ietf:params:jmap:chat";
const CALENDARS: &str = "urn:ietf:params:jmap:calendars";
const CHAT_CALENDARS: &str = "urn:ietf:params:jmap:chat:calendars";

/// A Calendar with the draft-ietf-jmap-calendars field list.
fn calendar_json(id: &str) -> Value {
    json!({
        "id": id, "name": "Team", "description": null, "color": null,
        "sortOrder": 0, "isSubscribed": true, "isVisible": true,
        "isDefault": false, "includeInAvailability": "all",
        "defaultAlertsWithTime": null, "defaultAlertsWithoutTime": null,
        "timeZone": null, "shareWith": null,
        "myRights": { "mayReadFreeBusy": true, "mayReadItems": true,
                      "mayWriteAll": true, "mayWriteOwn": true,
                      "mayUpdatePrivate": true, "mayRSVP": true,
                      "mayShare": true, "mayDelete": true }
    })
}

/// Calendar `C1` on `A1`, Calendar `C2` on `A2`.
fn calendars() -> CalendarsMemory {
    let calendars = CalendarsMemory::new()
        .with_account(&Id::from(ACCOUNT))
        .with_account(&Id::from(OTHER_ACCOUNT));
    calendars.seed_object(
        &Id::from(ACCOUNT),
        "Calendar",
        &Id::from("C1"),
        calendar_json("C1"),
    );
    calendars.seed_object(
        &Id::from(OTHER_ACCOUNT),
        "Calendar",
        &Id::from("C2"),
        calendar_json("C2"),
    );
    calendars
}

/// A Space per the draft-atwood-jmap-chat-00 §Space field list, with
/// `extra` merged in.
fn space(extra: Value) -> Space {
    let mut v = json!({
        "id": "S1", "name": "Team", "roles": [], "members": [],
        "categories": [], "uncategorizedChannelIds": [],
        "createdAt": "2026-01-01T00:00:00Z", "isPublic": false,
        "isPubliclyPreviewable": false, "memberCount": 0
    });
    v.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    serde_json::from_value(v).expect("Space fixture")
}

fn capability(may_bind_calendar: bool) -> ChatCalendarsCapability {
    serde_json::from_value(json!({
        "mayBindCalendar": may_bind_calendar,
        "supportsIcsParsing": false,
        "supportsAvailabilityLookup": false
    }))
    .expect("capability fixture")
}

/// `Calendar/get` of `ids` on `account`: the ids listed and not found.
async fn calendar_get(
    calendars: &CalendarsMemory,
    account: &str,
    ids: Value,
) -> (Vec<String>, Value) {
    let out = handle_calendar_get(calendars, &(), json!({ "accountId": account, "ids": ids }))
        .await
        .expect("Calendar/get");
    let listed = out.primary["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|c| c["id"].as_str().expect("id").to_owned())
        .collect();
    (listed, out.primary["notFound"].clone())
}

async fn check(calendars: &CalendarsMemory, value: Value) -> Result<(), (Value, Value)> {
    check_calendar_id(calendars, &(), &Id::from(ACCOUNT), &value)
        .await
        .expect("Calendar read")
        .map_err(|e| {
            let wire = serde_json::to_value(&e).expect("SetError serializes");
            (wire["type"].clone(), wire["properties"].clone())
        })
}

/// L87: advertised only with both `chat` and `calendars` on the account.
#[test]
fn advertise_with_chat_and_calendars() {
    assert!(may_advertise([CHAT, CALENDARS]));
    assert!(may_advertise([
        CALENDARS,
        "urn:ietf:params:jmap:core",
        CHAT
    ]));
}

/// L73 ("supports JMAP Chat but not JMAP Calendars MUST NOT advertise")
/// and L87 ("meaningful only when both").
#[test]
fn no_advertise_without_both() {
    assert!(!may_advertise([CHAT]));
    assert!(!may_advertise([CALENDARS]));
    assert!(!may_advertise([CHAT_CALENDARS]));
    assert!(!may_advertise(std::iter::empty()));
}

/// A chat backend whose Space extension properties come from
/// [`space_extension_properties`] for `mayBindCalendar`.
fn chat(may_bind_calendar: bool) -> ChatMemory {
    let chat = ChatMemory::new();
    chat.register_account(&Id::from(ACCOUNT));
    chat.set_space_extension_properties_for_test(space_extension_properties(&capability(
        may_bind_calendar,
    )));
    chat
}

async fn space_set(chat: &ChatMemory, args: Value) -> Value {
    handle_space_set(chat, &(), &std::collections::BTreeMap::new(), args, &[])
        .await
        .expect("Space/set")
        .primary
}

/// Create a Space with `calendarId: "C1"`; return its id and its
/// `calendarId` as `Space/get` returns it.
async fn create_bound_space(chat: &ChatMemory) -> (String, Value) {
    let out = space_set(
        chat,
        json!({ "accountId": ACCOUNT,
                "create": { "s": { "name": "Team", "calendarId": "C1" } } }),
    )
    .await;
    let id = out["created"]["s"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Space not created: {out}"))
        .to_owned();
    let got = handle_space_get(chat, &(), json!({ "accountId": ACCOUNT, "ids": [&id] }))
        .await
        .expect("Space/get")
        .primary;
    let calendar_id = got["list"][0]
        .get("calendarId")
        .cloned()
        .unwrap_or(Value::Null);
    (id, calendar_id)
}

/// L98: with `mayBindCalendar: false` no Space is bound. `calendarId` is
/// then no Space property, so a create or an update patch naming it is
/// `invalidProperties` (RFC 8620 §5.3) (bd:JMAP-zmqp).
#[tokio::test]
async fn binding_refused_without_may_bind_calendar() {
    let chat = chat(false);
    let out = space_set(
        &chat,
        json!({ "accountId": ACCOUNT,
                "create": { "s": { "name": "Team", "calendarId": "C1" } } }),
    )
    .await;
    assert_eq!(
        out["notCreated"]["s"],
        json!({ "type": "invalidProperties", "properties": ["calendarId"] })
    );
    let out = space_set(
        &chat,
        json!({ "accountId": ACCOUNT, "create": { "s": { "name": "Team" } } }),
    )
    .await;
    let id = out["created"]["s"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Space not created: {out}"))
        .to_owned();
    let out = space_set(
        &chat,
        json!({ "accountId": ACCOUNT, "update": { &id: { "calendarId": "C1" } } }),
    )
    .await;
    assert_eq!(
        out["notUpdated"][&id],
        json!({ "type": "invalidProperties", "properties": ["calendarId"] })
    );
}

/// L111, L118: with `mayBindCalendar: true`, `Space/set` binds on create
/// and accepts a patch to `calendarId` from the owner, who holds
/// `manage_space`.
#[tokio::test]
async fn binding_accepted_with_may_bind_calendar() {
    let chat = chat(true);
    let (id, calendar_id) = create_bound_space(&chat).await;
    assert_eq!(calendar_id, json!("C1"));
    let out = space_set(
        &chat,
        json!({ "accountId": ACCOUNT, "update": { &id: { "calendarId": null } } }),
    )
    .await;
    assert!(out["updated"].get(&id).is_some(), "{out}");
}

/// L114: a Calendar on the Space's own account may be bound.
#[tokio::test]
async fn same_account_calendar_is_accepted() {
    assert_eq!(check(&calendars(), json!("C1")).await, Ok(()));
}

/// L114: "The bound Calendar MUST belong to the same account that owns
/// the Space"; `C2` is on `A2`.
#[tokio::test]
async fn other_account_calendar_is_invalid() {
    assert_eq!(
        check(&calendars(), json!("C2")).await,
        Err((json!("invalidProperties"), json!(["calendarId"])))
    );
}

/// L272: "MUST reference a Calendar on the same server"; a Calendar this
/// server does not hold is refused.
#[tokio::test]
async fn unknown_calendar_is_invalid() {
    assert_eq!(
        check(&calendars(), json!("C9")).await,
        Err((json!("invalidProperties"), json!(["calendarId"])))
    );
}

/// L111: `calendarId` is a String (or `null` per L118).
#[tokio::test]
async fn non_string_calendar_id_is_invalid() {
    for value in [json!(1), json!(["C1"]), json!({ "id": "C1" })] {
        assert_eq!(
            check(&calendars(), value).await,
            Err((json!("invalidProperties"), json!(["calendarId"])))
        );
    }
}

/// L118 ("setting it to `null` unbinds the Space") and L122 (the
/// Calendar "may be unbound from the Space without being deleted"):
/// `null` is accepted and `C1` is still there.
#[tokio::test]
async fn unbind_keeps_the_calendar() {
    let calendars = calendars();
    assert_eq!(check(&calendars, Value::Null).await, Ok(()));
    let (listed, not_found) = calendar_get(&calendars, ACCOUNT, json!(["C1"])).await;
    assert_eq!(listed, ["C1"]);
    assert_eq!(not_found, json!([]));
}

/// L120: auto-bind creates a Calendar on the Space's account and sets
/// `calendarId` to its id before the Space is stored.
#[tokio::test]
async fn auto_bind_sets_calendar_id_to_the_new_calendar() {
    let calendars = calendars();
    let mut space = space(json!({}));
    let id = create_space_calendar(&calendars, &(), &Id::from(ACCOUNT), &mut space)
        .await
        .expect("create Calendar");
    assert_eq!(space.extra["calendarId"], json!(id.as_ref()));
    let (listed, _) = calendar_get(&calendars, ACCOUNT, json!([id.as_ref()])).await;
    assert_eq!(listed, [id.as_ref()]);
}

/// L118: a Space created already bound by the client keeps that binding
/// and no further Calendar is created.
#[tokio::test]
async fn auto_bind_keeps_a_client_binding() {
    let calendars = calendars();
    let mut space = space(json!({ "calendarId": "C1" }));
    let id = create_space_calendar(&calendars, &(), &Id::from(ACCOUNT), &mut space)
        .await
        .expect("create Calendar");
    assert_eq!(id.as_ref(), "C1");
    assert_eq!(space.extra["calendarId"], json!("C1"));
    let (listed, _) = calendar_get(&calendars, ACCOUNT, Value::Null).await;
    assert_eq!(listed, ["C1"]);
}
