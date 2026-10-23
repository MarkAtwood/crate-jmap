//! Comparator `isAscending` wire form matches `EmailComparator`.
//!
//! Oracle: RFC 8620 §5.5 — "isAscending: `Boolean` (optional; default:
//! true)". An ascending comparator omits the default; a descending one
//! carries `"isAscending": false`; an absent key decodes as ascending.
//! Expected JSON is hand-written.

use jmap_calendars_types::{
    CalendarEventComparator, CalendarEventComparatorProperty, CalendarEventNotificationComparator,
    CalendarEventNotificationComparatorProperty,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn assert_default_omitted<C>(ascending: C, descending: C, property: &str)
where
    C: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let bare = json!({ "property": property });
    assert_eq!(serde_json::to_value(&ascending).unwrap(), bare);
    assert_eq!(serde_json::from_value::<C>(bare).unwrap(), ascending);
    let desc: Value = json!({ "property": property, "isAscending": false });
    assert_eq!(serde_json::to_value(&descending).unwrap(), desc);
    assert_eq!(serde_json::from_value::<C>(desc).unwrap(), descending);
}

#[test]
fn calendar_event_comparator_omits_default_is_ascending() {
    let up = CalendarEventComparator::new(CalendarEventComparatorProperty::Created);
    let mut down = up.clone();
    down.is_ascending = false;
    assert_default_omitted(up, down, "created");
}

#[test]
fn calendar_event_notification_comparator_omits_default_is_ascending() {
    let up = CalendarEventNotificationComparator::new(
        CalendarEventNotificationComparatorProperty::Created,
    );
    let mut down = up.clone();
    down.is_ascending = false;
    assert_default_omitted(up, down, "created");
}
