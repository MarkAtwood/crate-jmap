//! The draft-ietf-jmap-calendars-34 §5.9 per-event account limits that
//! `CalendarEvent/set` checks before calling the backend.
//!
//! See [`CalendarsLimits`] for the caller-facing description of what is
//! checked.

use std::collections::BTreeSet;

use jmap_types::UTCDate;
use serde_json::{Map, Value, json};

use crate::CalendarsLimits;

/// Largest magnitude, in seconds, of any UTC offset a time zone uses.
///
/// Real offsets, historical local mean time included, stay within about
/// 16 hours, so a LocalDateTime in an unknown named zone that is more than
/// a day outside a bound is outside it in every zone.
const MAX_UTC_OFFSET_SECONDS: i64 = 86_400;

/// Top-level CalendarEvent properties a limit check reads. An update whose
/// patch touches none of them cannot newly break a limit.
const LIMITED_PROPERTIES: &[&str] = &[
    "calendarIds",
    "participants",
    "start",
    "timeZone",
    "recurrenceId",
    "recurrenceIdTimeZone",
    "recurrenceRule",
    "recurrenceRules",
    "excludedRecurrenceRules",
    "recurrenceOverrides",
    "created",
    "updated",
    "utcStart",
    "utcEnd",
    "alerts",
];

/// Properties holding recurrence rules: JSCalendar-bis `recurrenceRule` is
/// one rule, RFC 8984 `recurrenceRules` and `excludedRecurrenceRules` are
/// arrays of rules.
const RULE_PROPERTIES: &[&str] = &[
    "recurrenceRule",
    "recurrenceRules",
    "excludedRecurrenceRules",
];

/// Top-level UTCDateTime properties.
const UTC_PROPERTIES: &[&str] = &["created", "updated", "utcStart", "utcEnd"];

/// The top-level properties an update patch changes: the first segment of
/// each patch path.
pub(crate) fn patched_properties(patch: &Map<String, Value>) -> BTreeSet<&str> {
    patch
        .keys()
        .map(|k| k.split_once('/').map_or(k.as_str(), |(head, _)| head))
        .collect()
}

/// Whether a patch touching `touched` can change what a limit check sees.
pub(crate) fn touches_limited_property(touched: &BTreeSet<&str>) -> bool {
    LIMITED_PROPERTIES.iter().any(|p| touched.contains(p))
}

/// Returns the `invalidProperties` SetError for `event` if it breaks one of
/// the per-event caps in `limits`, or `None` if it does not.
///
/// `event` is the CalendarEvent as it would be stored. With
/// `touched: Some(..)` (an update), only those top-level properties are
/// checked, except that a change to `timeZone` or `recurrenceIdTimeZone`
/// rechecks every date. A value this check cannot parse is left to the
/// backend's validation.
pub(crate) fn limits_set_error(
    event: &Map<String, Value>,
    limits: &CalendarsLimits,
    touched: Option<&BTreeSet<&str>>,
) -> Option<Value> {
    let checks = |property: &str| touched.is_none_or(|t| t.contains(property));
    let mut properties = BTreeSet::new();
    let mut broken = BTreeSet::new();

    if let Some(max) = limits.max_calendars_per_event
        && checks("calendarIds")
        && map_len(event.get("calendarIds")) > max
    {
        properties.insert("calendarIds");
        broken.insert("maxCalendarsPerEvent");
    }

    if let Some(max) = limits.max_participants_per_event {
        if checks("participants") && map_len(event.get("participants")) > max {
            properties.insert("participants");
            broken.insert("maxParticipantsPerEvent");
        }
        let override_over = overrides(event)
            .any(|(_, patch)| map_len(patch.and_then(|p| p.get("participants"))) > max);
        if checks("recurrenceOverrides") && override_over {
            properties.insert("recurrenceOverrides");
            broken.insert("maxParticipantsPerEvent");
        }
    }

    let (min, max) = (
        limits.min_date_time_epoch_seconds,
        limits.max_date_time_epoch_seconds,
    );
    if min.is_some() || max.is_some() {
        let zone_changed =
            touched.is_none_or(|t| t.contains("timeZone") || t.contains("recurrenceIdTimeZone"));
        for (property, date) in dates(event) {
            if (zone_changed || checks(property)) && date.outside(min, max) {
                properties.insert(property);
                broken.insert(if date.before(min) {
                    "minDateTime"
                } else {
                    "maxDateTime"
                });
            }
        }
    }

    if properties.is_empty() {
        return None;
    }
    let broken: Vec<&str> = broken.into_iter().collect();
    Some(json!({
        "type": "invalidProperties",
        "properties": properties.into_iter().collect::<Vec<_>>(),
        "description": format!(
            "exceeds the account's {} (draft-ietf-jmap-calendars-34 §5.9)",
            broken.join(", ")
        ),
    }))
}

/// Number of entries in a JSON object; anything else counts as empty.
fn map_len(value: Option<&Value>) -> u64 {
    value
        .and_then(Value::as_object)
        .map_or(0, |m| u64::try_from(m.len()).unwrap_or(u64::MAX))
}

/// The `recurrenceOverrides` entries of `event` as (recurrence id, patch),
/// with `None` for a patch that is not an object.
fn overrides(
    event: &Map<String, Value>,
) -> impl Iterator<Item = (&str, Option<&Map<String, Value>>)> {
    event
        .get("recurrenceOverrides")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(id, patch)| (id.as_str(), patch.as_object()))
}

/// A date found in an event, with how to read it against UTC bounds.
struct EventDate<'a> {
    text: &'a str,
    kind: DateKind,
}

#[derive(Clone, Copy)]
enum DateKind {
    /// A UTCDateTime.
    Utc,
    /// A LocalDateTime with no time zone: compared as though it were UTC
    /// (draft-ietf-jmap-calendars-34 §1.5.1).
    Floating,
    /// A LocalDateTime in a named time zone, whose offset is unknown here.
    Zoned,
}

impl EventDate<'_> {
    /// Seconds since the Unix epoch, reading a LocalDateTime as UTC, or
    /// `None` if the text is not a date-time.
    fn epoch_seconds(&self) -> Option<i64> {
        let utc = match self.kind {
            DateKind::Utc => self.text.to_owned(),
            DateKind::Floating | DateKind::Zoned => format!("{}Z", self.text),
        };
        UTCDate::from(utc).to_epoch_seconds().ok()
    }

    /// Slack allowed for an unknown zone offset.
    fn margin(&self) -> i64 {
        match self.kind {
            DateKind::Utc | DateKind::Floating => 0,
            DateKind::Zoned => MAX_UTC_OFFSET_SECONDS,
        }
    }

    fn before(&self, min: Option<i64>) -> bool {
        match (self.epoch_seconds(), min) {
            (Some(at), Some(min)) => at < min.saturating_sub(self.margin()),
            _ => false,
        }
    }

    fn after(&self, max: Option<i64>) -> bool {
        match (self.epoch_seconds(), max) {
            (Some(at), Some(max)) => at > max.saturating_add(self.margin()),
            _ => false,
        }
    }

    fn outside(&self, min: Option<i64>, max: Option<i64>) -> bool {
        self.before(min) || self.after(max)
    }
}

/// Every date the limit check compares, with the top-level property that
/// holds it.
fn dates(event: &Map<String, Value>) -> Vec<(&'static str, EventDate<'_>)> {
    /// Records `value` under `property` if it is a string.
    fn push<'a>(
        found: &mut Vec<(&'static str, EventDate<'a>)>,
        property: &'static str,
        value: Option<&'a Value>,
        kind: DateKind,
    ) {
        if let Some(text) = value.and_then(Value::as_str) {
            found.push((property, EventDate { text, kind }));
        }
    }
    let zone_of = |time_zone: Option<&Value>| match time_zone {
        Some(Value::String(_)) => DateKind::Zoned,
        _ => DateKind::Floating,
    };
    let event_zone = zone_of(event.get("timeZone"));
    let mut found = Vec::new();

    push(&mut found, "start", event.get("start"), event_zone);
    let recurrence_id_zone = match event.get("recurrenceIdTimeZone") {
        Some(Value::String(_)) => DateKind::Zoned,
        _ => event_zone,
    };
    push(
        &mut found,
        "recurrenceId",
        event.get("recurrenceId"),
        recurrence_id_zone,
    );
    for &property in RULE_PROPERTIES {
        let rules: Vec<&Value> = match event.get(property) {
            Some(Value::Array(rules)) => rules.iter().collect(),
            Some(rule @ Value::Object(_)) => vec![rule],
            _ => Vec::new(),
        };
        for rule in rules {
            push(&mut found, property, rule.get("until"), event_zone);
        }
    }
    for &property in UTC_PROPERTIES {
        push(&mut found, property, event.get(property), DateKind::Utc);
    }
    for (recurrence_id, patch) in overrides(event) {
        found.push((
            "recurrenceOverrides",
            EventDate {
                text: recurrence_id,
                kind: event_zone,
            },
        ));
        if let Some(patch) = patch {
            let start_zone = match patch.get("timeZone") {
                Some(zone) => zone_of(Some(zone)),
                None => event_zone,
            };
            push(
                &mut found,
                "recurrenceOverrides",
                patch.get("start"),
                start_zone,
            );
        }
    }
    for alert in object_values(event.get("alerts")) {
        let when = alert.get("trigger").and_then(|t| t.get("when"));
        push(&mut found, "alerts", when, DateKind::Utc);
        push(
            &mut found,
            "alerts",
            alert.get("acknowledged"),
            DateKind::Utc,
        );
    }
    for participant in object_values(event.get("participants")) {
        push(
            &mut found,
            "participants",
            participant.get("scheduleUpdated"),
            DateKind::Utc,
        );
    }
    found
}

/// The values of a JSON object; nothing for any other value.
fn object_values(value: Option<&Value>) -> impl Iterator<Item = &Value> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Map::values)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2000-01-01T00:00:00Z.
    const Y2000: i64 = 946_684_800;
    /// 2100-01-01T00:00:00Z.
    const Y2100: i64 = 4_102_444_800;

    fn object(value: Value) -> Map<String, Value> {
        serde_json::from_value(value).expect("test event is written as a JSON object")
    }

    fn range() -> CalendarsLimits {
        CalendarsLimits::default()
            .with_min_date_time_epoch_seconds(Some(Y2000))
            .with_max_date_time_epoch_seconds(Some(Y2100))
    }

    fn invalid_properties(error: Option<Value>) -> Vec<String> {
        let error = error.expect("event must break a limit");
        assert_eq!(error["type"], "invalidProperties", "{error}");
        serde_json::from_value(error["properties"].clone()).expect("properties is a string array")
    }

    /// Unset limits never reject, however large or far-dated the event.
    #[test]
    fn default_limits_accept_everything() {
        let event = object(json!({
            "calendarIds": { "a": true, "b": true, "c": true },
            "participants": { "p1": {}, "p2": {} },
            "start": "1066-10-14T09:00:00"
        }));
        assert!(limits_set_error(&event, &CalendarsLimits::default(), None).is_none());
    }

    /// A count equal to the limit is allowed; one more is rejected and the
    /// error names the property (-34 §5.9: "more Calendars than the ...
    /// limit").
    #[test]
    fn calendar_and_participant_counts_are_inclusive_limits() {
        let limits = CalendarsLimits::default()
            .with_max_calendars_per_event(Some(2))
            .with_max_participants_per_event(Some(1));
        let at_limit = object(json!({
            "calendarIds": { "a": true, "b": true },
            "participants": { "p1": {} }
        }));
        assert!(limits_set_error(&at_limit, &limits, None).is_none());

        let over = object(json!({
            "calendarIds": { "a": true, "b": true, "c": true },
            "participants": { "p1": {}, "p2": {} }
        }));
        assert_eq!(
            invalid_properties(limits_set_error(&over, &limits, None)),
            ["calendarIds", "participants"]
        );
    }

    /// An override that replaces `participants` with too many entries is
    /// reported under `recurrenceOverrides`.
    #[test]
    fn override_participants_count_against_the_limit() {
        let limits = CalendarsLimits::default().with_max_participants_per_event(Some(1));
        let event = object(json!({
            "participants": { "p1": {} },
            "recurrenceOverrides": {
                "2030-01-08T09:00:00": { "participants": { "p1": {}, "p2": {} } }
            }
        }));
        assert_eq!(
            invalid_properties(limits_set_error(&event, &limits, None)),
            ["recurrenceOverrides"]
        );
    }

    /// A floating LocalDateTime is compared as though it were UTC, so one
    /// second before minDateTime is rejected (-34 §1.5.1).
    #[test]
    fn floating_dates_compare_as_utc() {
        let inside = object(json!({ "start": "2000-01-01T00:00:00" }));
        assert!(limits_set_error(&inside, &range(), None).is_none());
        let before = object(json!({ "start": "1999-12-31T23:59:59" }));
        assert_eq!(
            invalid_properties(limits_set_error(&before, &range(), None)),
            ["start"]
        );
        let after = object(json!({ "start": "2100-01-01T00:00:01" }));
        assert_eq!(
            invalid_properties(limits_set_error(&after, &range(), None)),
            ["start"]
        );
    }

    /// A zoned LocalDateTime near a bound may be inside it after the zone
    /// offset, so the handler leaves it alone; a day or more out is
    /// rejected in any zone.
    #[test]
    fn zoned_dates_are_rejected_only_beyond_any_offset() {
        let near =
            object(json!({ "start": "1999-12-31T20:00:00", "timeZone": "America/New_York" }));
        assert!(limits_set_error(&near, &range(), None).is_none());
        let far = object(json!({ "start": "1999-12-30T00:00:00", "timeZone": "America/New_York" }));
        assert_eq!(
            invalid_properties(limits_set_error(&far, &range(), None)),
            ["start"]
        );
    }

    /// Each date location the check documents is reached and reported
    /// under its top-level property.
    #[test]
    fn every_documented_date_location_is_checked() {
        let past = "1900-01-01T00:00:00";
        let past_utc = "1900-01-01T00:00:00Z";
        let cases = [
            ("recurrenceId", json!({ "recurrenceId": past })),
            (
                "recurrenceRules",
                json!({ "recurrenceRules": [{ "until": past }] }),
            ),
            (
                "recurrenceRule",
                json!({ "recurrenceRule": { "until": past } }),
            ),
            (
                "excludedRecurrenceRules",
                json!({ "excludedRecurrenceRules": [{ "until": past }] }),
            ),
            (
                "recurrenceOverrides",
                json!({ "recurrenceOverrides": { past: {} } }),
            ),
            (
                "recurrenceOverrides",
                json!({ "recurrenceOverrides": { "2030-01-01T00:00:00": { "start": past } } }),
            ),
            ("created", json!({ "created": past_utc })),
            ("updated", json!({ "updated": past_utc })),
            ("utcStart", json!({ "utcStart": past_utc })),
            ("utcEnd", json!({ "utcEnd": past_utc })),
            (
                "alerts",
                json!({ "alerts": { "a": { "trigger": { "when": past_utc } } } }),
            ),
            (
                "alerts",
                json!({ "alerts": { "a": { "acknowledged": past_utc } } }),
            ),
            (
                "participants",
                json!({ "participants": { "p": { "scheduleUpdated": past_utc } } }),
            ),
        ];
        for (property, event) in cases {
            let event = object(event);
            assert_eq!(
                invalid_properties(limits_set_error(&event, &range(), None)),
                [property],
                "{event:?}"
            );
        }
    }

    /// An update checks only the properties its patch changes, unless it
    /// changes the event's time zone, which can move every date.
    #[test]
    fn update_checks_touched_properties_and_all_dates_on_zone_change() {
        let event = object(json!({
            "start": "1900-01-01T00:00:00",
            "title": "x",
            "timeZone": null
        }));
        let title_only = BTreeSet::from(["title"]);
        assert!(limits_set_error(&event, &range(), Some(&title_only)).is_none());
        let zone = BTreeSet::from(["timeZone"]);
        assert_eq!(
            invalid_properties(limits_set_error(&event, &range(), Some(&zone))),
            ["start"]
        );
    }

    /// The first path segment of each patch key is the touched property.
    #[test]
    fn patched_properties_are_first_path_segments() {
        let patch = object(json!({
            "participants/p1/participationStatus": "accepted",
            "start": "2030-01-01T00:00:00"
        }));
        let touched = patched_properties(&patch);
        assert_eq!(touched, BTreeSet::from(["participants", "start"]));
        assert!(touches_limited_property(&touched));
        assert!(!touches_limited_property(&BTreeSet::from(["title"])));
    }
}
