//! draft-atwood-jmap-updates-00 on Calendar (§5.5: `name` and
//! `description` are localizable) through the calendars handlers and the
//! in-memory backend.
//!
//! Oracle: the draft's normative text at jmap-chat-spec 2b85363 (§3,
//! §4.1, §4.2.1, §5.5), with hand-written requests and expected responses.

mod common;

use common::MemoryBackend;
use jmap_calendars_server::{handle_calendar_event_get, handle_calendar_get, handle_calendar_set};
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const ACC: &str = "acc1";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:calendars".to_owned(),
        "urn:ietf:params:jmap:i18n".to_owned(),
    ]
}

fn using_without_i18n() -> Vec<String> {
    using()[..2].to_vec()
}

fn backend() -> MemoryBackend {
    MemoryBackend::new().with_account(&Id::from(ACC))
}

fn error_type(err: JmapError) -> Value {
    serde_json::to_value(&err).unwrap()["type"].clone()
}

async fn set(b: &MemoryBackend, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACC);
    handle_calendar_set(b, &(), args, using)
        .await
        .unwrap()
        .primary
}

async fn get(b: &MemoryBackend, args: Value, using: &[String]) -> Result<Value, JmapError> {
    let mut args = args;
    args["accountId"] = json!(ACC);
    handle_calendar_get(b, &(), args, using)
        .await
        .map(|o| o.primary)
}

async fn work_calendar(b: &MemoryBackend) -> String {
    let resp = set(
        b,
        json!({ "create": { "c": {
            "name": "Work", "description": "Team schedule", "locale": "en",
            "localizations": {
                "de": { "name": "Arbeit", "description": null },
                "fr": { "name": "Travail" }
            }
        } } }),
        &using(),
    )
    .await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("create failed: {resp}"))
        .to_owned()
}

/// §4.2.1 on Calendar/get: the German view, with a null localized
/// description; state unchanged.
#[tokio::test]
async fn get_with_language() {
    let b = backend();
    let id = work_calendar(&b).await;
    let plain = get(&b, json!({ "ids": [id] }), &using()).await.unwrap();
    let de = get(
        &b,
        json!({ "ids": [id], "properties": ["name", "description", "locale"], "language": ["de-AT"] }),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(
        de["list"][0],
        json!({ "id": id, "name": "Arbeit", "description": null, "locale": "de" })
    );
    assert_eq!(de["state"], plain["state"]);
}

/// §4.1: renaming without the capability prunes the stale names and keeps
/// the German null description; a client with it gets `localizations`
/// back in `updated`.
#[tokio::test]
async fn rename_prunes() {
    let b = backend();
    let id = work_calendar(&b).await;
    let resp = set(
        &b,
        json!({ "update": { id.clone(): { "name": "Job" } } }),
        &using(),
    )
    .await;
    assert_eq!(
        resp["updated"][&id]["localizations"],
        json!({ "de": { "description": null } }),
        "{resp}"
    );
}

/// §3: without the capability, `language` is invalidArguments and the
/// properties are neither returned nor accepted.
#[tokio::test]
async fn without_capability() {
    let b = backend();
    let id = work_calendar(&b).await;
    let resp = get(&b, json!({ "ids": [id] }), &using_without_i18n())
        .await
        .unwrap();
    assert!(resp["list"][0].get("localizations").is_none());
    let err = get(&b, json!({ "language": ["de"] }), &using_without_i18n())
        .await
        .unwrap_err();
    assert_eq!(error_type(err), "invalidArguments");
    let resp = set(
        &b,
        json!({ "update": { id.clone(): { "locale": "de" } } }),
        &using_without_i18n(),
    )
    .await;
    assert_eq!(resp["notUpdated"][&id]["properties"], json!(["locale"]));
}

/// §5.5: CalendarEvent keeps JSCalendar's own localizations, and "the
/// language argument does not apply to CalendarEvent/get".
#[tokio::test]
async fn calendar_event_rejects_language() {
    let b = backend();
    let err = handle_calendar_event_get(
        &b,
        &(),
        json!({ "accountId": ACC, "language": ["de"] }),
        &using(),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(err), "invalidArguments");
}
