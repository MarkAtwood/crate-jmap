//! Wire forms of the object-history types (bd JMAP-tna1.3.1).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §2.1 (both capability
//! properties "MUST" be present; the Session value "MUST be an empty
//! object"), §3 (argument defaults), §3.1 (`objectHistory` members, `null`
//! for the live version), §3.2.1 (the ContactCard/get response, verbatim),
//! §5.1 (`restored`/`notRestored`, `null` when empty), §7.2 Table 1 ("Yes"
//! rows), and docs/design/DECISIONS.md decisions 5 and 23 (Task, TaskList
//! pending).

use jmap_object_history_types::{
    DestroyedQueryArguments, HistoryEntry, HistoryGetArguments, JMAP_OBJECT_HISTORY_URI,
    ObjectHistoryAccountCapability, ObjectHistoryCapability, ObjectHistoryInfo,
    PENDING_HISTORY_REGISTRATION, RestoreOutcome, is_history_type,
};
use jmap_types::{Id, SetErrorType, UTCDate};
use serde_json::{Map, Value, json};

#[test]
fn uri() {
    assert_eq!(
        JMAP_OBJECT_HISTORY_URI,
        "urn:ietf:params:jmap:object-history"
    );
}

/// §2.1: the Session value is `{}`; unknown members round-trip.
#[test]
fn session_capability_is_an_empty_object() {
    assert_eq!(
        serde_json::to_value(ObjectHistoryCapability::new()).unwrap(),
        json!({})
    );
    let cap: ObjectHistoryCapability = serde_json::from_value(json!({"x": 1})).unwrap();
    assert_eq!(serde_json::to_value(&cap).unwrap(), json!({"x": 1}));
}

/// §2.1: both properties are always present, `null` included.
#[test]
fn account_capability_always_has_both_properties() {
    let cap: ObjectHistoryAccountCapability = serde_json::from_value(json!({
        "maxHistoryDuration": null,
        "supportedTypes": ["ContactCard", "Email"],
        "vendor": true
    }))
    .unwrap();
    assert_eq!(cap.max_history_duration, None);
    assert!(cap.supports_type("Email"));
    assert!(!cap.supports_type("Thread"));
    assert_eq!(
        serde_json::to_value(&cap).unwrap(),
        json!({"maxHistoryDuration": null,
               "supportedTypes": ["ContactCard", "Email"], "vendor": true})
    );
    let cap: ObjectHistoryAccountCapability = serde_json::from_value(json!({
        "maxHistoryDuration": 2_592_000, "supportedTypes": []
    }))
    .unwrap();
    assert_eq!(cap.max_history_duration, Some(2_592_000));
}

/// §3.2.1, the response `list` verbatim: entries parse, and serialize back
/// to the same JSON, `replaced: null` and `expires: null` included.
#[test]
fn example_3_2_1_entries_round_trip() {
    let list = json!([
      {
        "id": "contact1",
        "name": {"full": "Robert Smith"},
        "emails": {"e1": {"address": "bob@example.com"}},
        "objectHistory": {"version": 1, "replaced": "2026-02-20T14:15:00Z",
                          "hasOlder": false, "expires": "2026-03-22T14:15:00Z"}
      },
      {
        "id": "contact1",
        "name": {"full": "Bob Smith"},
        "emails": {"e1": {"address": "bob@example.com"}},
        "objectHistory": {"version": 2, "replaced": "2026-03-01T09:00:00Z",
                          "hasOlder": true, "expires": "2026-03-31T09:00:00Z"}
      },
      {
        "id": "contact1",
        "name": {"full": "Bob Smith"},
        "emails": {"e1": {"address": "bob@example.com"},
                   "e2": {"address": "bob@personal.example"}},
        "objectHistory": {"version": 3, "replaced": null,
                          "hasOlder": true, "expires": null}
      }
    ]);
    let entries: Vec<HistoryEntry<Map<String, Value>>> =
        serde_json::from_value(list.clone()).unwrap();
    assert_eq!(entries[2].object_history, ObjectHistoryInfo::live(3, true));
    assert_eq!(
        entries[0].object_history,
        ObjectHistoryInfo::replaced(
            1,
            UTCDate::from("2026-02-20T14:15:00Z"),
            false,
            Some(UTCDate::from("2026-03-22T14:15:00Z")),
        )
    );
    assert!(!entries[0].object.contains_key("objectHistory"));
    assert_eq!(serde_json::to_value(&entries).unwrap(), list);
}

/// Unknown `objectHistory` members round-trip.
#[test]
fn object_history_info_keeps_unknown_members() {
    let v = json!({"version": 1, "replaced": null, "hasOlder": false,
                   "expires": null, "x": [1]});
    let info: ObjectHistoryInfo = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(info.extra["x"], json!([1]));
    assert_eq!(serde_json::to_value(&info).unwrap(), v);
}

/// §3 defaults: omitted arguments read as their defaults, and defaults are
/// not written.
#[test]
fn get_arguments_defaults() {
    let args: HistoryGetArguments = serde_json::from_value(json!({})).unwrap();
    assert_eq!(args, HistoryGetArguments::new());
    assert_eq!(serde_json::to_value(&args).unwrap(), json!({}));
    let v = json!({"includeReplaced": true, "includeDestroyed": true,
                   "historyAfter": "2026-01-01T00:00:00Z",
                   "historyBefore": "2026-02-01T00:00:00Z",
                   "historySkip": 20, "historyLimit": 5, "ids": null});
    let args: HistoryGetArguments = serde_json::from_value(v.clone()).unwrap();
    assert!(args.include_replaced && args.include_destroyed);
    assert_eq!(args.history_skip, 20);
    assert_eq!(args.history_limit, Some(5));
    assert_eq!(args.extra["ids"], Value::Null);
    assert_eq!(serde_json::to_value(&args).unwrap(), v);
}

/// §4 defaults, and the §4.1 example's arguments.
#[test]
fn destroyed_query_arguments() {
    let args: DestroyedQueryArguments = serde_json::from_value(json!({})).unwrap();
    assert_eq!(serde_json::to_value(&args).unwrap(), json!({}));
    let v = json!({"onlyDestroyed": true,
                   "destroyedAfter": "2026-03-01T00:00:00Z", "limit": 50});
    let args: DestroyedQueryArguments = serde_json::from_value(v.clone()).unwrap();
    assert!(args.only_destroyed);
    assert_eq!(
        args.destroyed_after,
        Some(UTCDate::from("2026-03-01T00:00:00Z"))
    );
    assert_eq!(serde_json::to_value(&args).unwrap(), v);
}

/// §5.1: `restored` and `notRestored` read from a `/set` response's other
/// members; `null` reads as empty, empty writes as `null`, and ids keep
/// BTreeMap order.
#[test]
fn restore_outcome_from_set_extra() {
    let extra = json!({
        "restored": {"email42": null, "email7": {"threadId": "T9"}},
        "notRestored": null,
        "other": 1
    });
    let outcome = RestoreOutcome::from_set_extra(extra.as_object().unwrap()).unwrap();
    assert_eq!(
        outcome.restored.keys().collect::<Vec<_>>(),
        [&Id::from("email42"), &Id::from("email7")]
    );
    assert_eq!(outcome.restored[&Id::from("email42")], None);
    let changed = outcome.restored[&Id::from("email7")].as_ref().unwrap();
    assert_eq!(changed.extra["threadId"], "T9");
    assert!(outcome.not_restored.is_empty());

    let extra = json!({"notRestored": {"x": {"type": "notFound"}}});
    let outcome = RestoreOutcome::from_set_extra(extra.as_object().unwrap()).unwrap();
    assert_eq!(
        outcome.not_restored[&Id::from("x")].error_type,
        SetErrorType::NotFound
    );
    assert_eq!(
        serde_json::to_value(&outcome).unwrap(),
        json!({"restored": null, "notRestored": {"x": {"type": "notFound"}}})
    );
    assert_eq!(
        RestoreOutcome::from_set_extra(&Map::new()).unwrap(),
        RestoreOutcome::new()
    );
    let bad = json!({"restored": [1]});
    assert!(RestoreOutcome::from_set_extra(bad.as_object().unwrap()).is_err());
}

/// §7.2 Table 1 "Yes" rows plus the pending Task/TaskList; "No" rows and
/// unknown names are not history types.
#[test]
fn history_types_follow_the_registry() {
    for t in [
        "Mailbox",
        "Email",
        "Identity",
        "VacationResponse",
        "SieveScript",
        "AddressBook",
        "ContactCard",
        "Calendar",
        "CalendarEvent",
        "ParticipantIdentity",
        "FileNode",
        "Task",
        "TaskList",
    ] {
        assert!(is_history_type(t), "{t}");
    }
    for t in [
        "Core",
        "PushSubscription",
        "Thread",
        "EmailDelivery",
        "SearchSnippet",
        "EmailSubmission",
        "MDN",
        "Quota",
        "Principal",
        "ShareNotification",
        "CalendarEventNotification",
        "TaskNotification",
        "NoSuchType",
    ] {
        assert!(!is_history_type(t), "{t}");
    }
    assert_eq!(PENDING_HISTORY_REGISTRATION, ["Task", "TaskList"]);
}
