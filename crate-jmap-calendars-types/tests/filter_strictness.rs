//! Filter conditions reject keys they do not define.
//!
//! Oracle: RFC 8620 §5.5 — "unsupportedFilter: The "filter" is
//! syntactically valid, but the server cannot process it." A key the
//! condition type does not define must fail the decode instead of becoming
//! an all-`None` condition that matches every record; servers map the
//! failure to `unsupportedFilter`. An operator object is not a
//! FilterCondition, so a bare condition type must reject it too. Inputs are
//! hand-written JSON; control keys are the property names the cited spec
//! defines.

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

fn assert_strict<T: DeserializeOwned>(known: Value, typo: Value) {
    if let Err(e) = serde_json::from_value::<T>(known.clone()) {
        panic!("{known} must decode: {e}");
    }
    for bad in [
        typo,
        json!({ "bogus": 1 }),
        json!({ "operator": "AND", "conditions": [] }),
    ] {
        assert!(
            serde_json::from_value::<T>(bad.clone()).is_err(),
            "{bad} must be rejected"
        );
    }
}

/// Calendar properties `name` and `isSubscribed`
/// (draft-ietf-jmap-calendars-33 §4).
#[test]
fn calendar_filter_condition_is_strict() {
    assert_strict::<jmap_calendars_types::CalendarFilterCondition>(
        json!({ "name": "Work", "isSubscribed": true }),
        json!({ "is_subscribed": true }),
    );
}

/// draft-ietf-jmap-calendars-33 §5.11.1 FilterCondition: `inCalendar`,
/// `title`.
#[test]
fn calendar_event_filter_condition_is_strict() {
    assert_strict::<jmap_calendars_types::CalendarEventFilterCondition>(
        json!({ "inCalendar": "c1", "title": "standup" }),
        json!({ "in_calendar": "c1" }),
    );
}

/// draft-ietf-jmap-calendars-33 §7.4.1 CalendarEventNotification/query
/// FilterCondition: `type`, `calendarEventIds`.
#[test]
fn notification_filter_condition_is_strict() {
    assert_strict::<jmap_calendars_types::NotificationFilterCondition>(
        json!({ "type": "created", "calendarEventIds": ["e1"] }),
        json!({ "calendar_event_ids": ["e1"] }),
    );
}
