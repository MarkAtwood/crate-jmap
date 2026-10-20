//! Space properties owned by a Space extension, registered through
//! `ChatBackend::space_extension_properties` (bd:JMAP-5mz6.1,
//! bd:JMAP-y3f2.16).
//!
//! Oracles (jmap-chat-spec commit 91672c9):
//!
//! - draft-atwood-jmap-chat-calendars-00 L111-113: "`calendarId`
//!   (String, optional): A JMAP Calendars `Calendar.id` ... bound to
//!   this Space. ... When absent, this Space has no bound Calendar."
//!   L118: "Servers that advertise `mayBindCalendar: true` MUST accept
//!   patches to `Space.calendarId` from members holding the
//!   `"manage_space"` permission. Setting `calendarId` to a non-null
//!   value binds the Space to the identified Calendar; setting it to
//!   `null` unbinds the Space."
//! - draft-atwood-jmap-chat-tasks-00 L114 and L125: the same rules for
//!   `taskListId`. L127: "the server MUST set `taskListId` to the new
//!   TaskList's id at Space creation time."
//! - draft-atwood-jmap-chat-calendars-00 L300: "A Space's `calendarId`
//!   is set by members holding the `"manage_space"` permission." RFC 8620
//!   §5.3 `forbidden`: "the action would violate an ACL or other
//!   permissions policy".
//! - RFC 8620 §5.3 `invalidProperties`: "The record given is invalid in
//!   some way. For example: It contains properties that are invalid
//!   according to the type specification of this record type." Without
//!   the extension, `calendarId` is not a Space property
//!   (draft-atwood-jmap-chat-00 §Space).

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

/// A backend with the account registered and `keys` registered as
/// extension properties.
fn backend_with(keys: &'static [&'static str]) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.set_space_extension_properties_for_test(keys);
    backend
}

async fn space_set(backend: &MemoryBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_space_set(backend, &(), args)
        .await
        .expect("Space/set");
    primary
}

/// Create a Space from `props` (merged with a name) and return its id.
async fn create_space(backend: &MemoryBackend, props: Value) -> String {
    let mut obj = json!({ "name": "Team" });
    obj.as_object_mut()
        .expect("object")
        .extend(props.as_object().expect("object").clone());
    let resp = space_set(
        backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "s0": obj } }),
    )
    .await;
    resp["created"]["s0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Space not created: {resp}"))
        .to_owned()
}

async fn get_space(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_space_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("Space/get");
    primary["list"][0].clone()
}

#[tokio::test]
async fn registered_properties_round_trip_on_create() {
    let backend = backend_with(&["calendarId", "taskListId"]);
    let id = create_space(
        &backend,
        json!({ "calendarId": "cal-1", "taskListId": "tl-1" }),
    )
    .await;

    let space = get_space(&backend, &id).await;
    assert_eq!(space["calendarId"], "cal-1", "{space}");
    assert_eq!(space["taskListId"], "tl-1", "{space}");
}

#[tokio::test]
async fn registered_property_binds_and_unbinds_on_update() {
    let backend = backend_with(&["calendarId"]);
    let id = create_space(&backend, json!({})).await;

    // calendars-00 L118: a non-null value binds the Space.
    let resp = space_set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { &id: { "calendarId": "cal-9" } } }),
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "bind rejected: {resp}");
    assert_eq!(get_space(&backend, &id).await["calendarId"], "cal-9");

    // calendars-00 L118: `null` unbinds; L113: absent means no bound
    // Calendar.
    let resp = space_set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { &id: { "calendarId": null } } }),
    )
    .await;
    assert!(
        resp["updated"].get(&id).is_some(),
        "unbind rejected: {resp}"
    );
    let space = get_space(&backend, &id).await;
    assert!(space.get("calendarId").is_none(), "{space}");
}

#[tokio::test]
async fn unregistered_property_is_invalid_properties() {
    let backend = backend_with(&[]);
    let id = create_space(&backend, json!({})).await;

    let resp = space_set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { &id: { "calendarId": "cal-1" } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&id],
        json!({ "type": "invalidProperties", "properties": ["calendarId"] }),
        "{resp}"
    );
}

#[tokio::test]
async fn only_registered_keys_are_accepted() {
    let backend = backend_with(&["calendarId"]);
    let id = create_space(&backend, json!({})).await;

    let resp = space_set(
        &backend,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { &id: { "calendarId": "cal-1", "taskListId": "tl-1" } }
        }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&id],
        json!({ "type": "invalidProperties", "properties": ["taskListId"] }),
        "{resp}"
    );
    // RFC 8620 §5.3: a rejected update leaves the record unchanged.
    let space = get_space(&backend, &id).await;
    assert!(space.get("calendarId").is_none(), "{space}");
}

/// `ChatBackend::space_extension_properties`: "A listed key that is
/// already a core Space property or a structural patch key keeps its
/// core meaning." draft-atwood-jmap-chat-00 L820: Space `closedAt` is
/// "server-set" and "Absent while the Space is open", so a listed
/// `closedAt` on create is not stored, while a listed extension key is.
#[tokio::test]
async fn listed_core_property_is_not_stored_on_create() {
    let backend = backend_with(&["closedAt", "addRoles", "calendarId"]);
    let id = create_space(
        &backend,
        json!({
            "closedAt": "2026-01-01T00:00:00Z",
            "addRoles": [],
            "calendarId": "cal-1",
        }),
    )
    .await;

    let space = get_space(&backend, &id).await;
    assert!(space.get("closedAt").is_none(), "{space}");
    assert!(space.get("addRoles").is_none(), "{space}");
    assert_eq!(space["calendarId"], "cal-1", "{space}");
}

/// The update half of the rule above: draft-atwood-jmap-chat-00 L820
/// makes Space `closedAt` "server-set", so a client patch carrying it is
/// `invalidProperties` (RFC 8620 §5.3: "It contains properties that are
/// invalid according to the type specification of this record type")
/// even when the backend lists it, and the Space stays open.
#[tokio::test]
async fn listed_core_property_is_invalid_on_update() {
    let backend = backend_with(&["closedAt", "calendarId"]);
    let id = create_space(&backend, json!({})).await;

    let resp = space_set(
        &backend,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { &id: { "closedAt": "2026-01-01T00:00:00Z", "calendarId": "cal-1" } }
        }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&id],
        json!({ "type": "invalidProperties", "properties": ["closedAt"] }),
        "{resp}"
    );
    let space = get_space(&backend, &id).await;
    assert!(space.get("closedAt").is_none(), "{space}");
    assert!(space.get("calendarId").is_none(), "{space}");
}

/// calendars-00 L300 with RFC 8620 §5.3 `forbidden`: a member whose roles
/// grant only `"view"` cannot bind the Space's Calendar, and the stored
/// Space is unchanged.
#[tokio::test]
async fn registered_property_patch_without_manage_space_is_forbidden() {
    let backend = backend_with(&["calendarId"]);
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        "s-view",
        json!({
            "id": "s-view",
            "name": "Team",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 1,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [{ "id": "r-view", "name": "Viewer", "permissions": ["view"], "position": 1 }],
            "members": [{ "id": ACCOUNT_ID, "roleIds": ["r-view"], "joinedAt": "2026-01-01T00:00:00Z" }]
        }),
    );

    let resp = space_set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { "s-view": { "calendarId": "cal-1" } } }),
    )
    .await;
    assert_eq!(resp["notUpdated"]["s-view"]["type"], "forbidden", "{resp}");
    assert!(
        get_space(&backend, "s-view")
            .await
            .get("calendarId")
            .is_none(),
        "the Space must not be bound"
    );
}
