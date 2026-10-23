//! draft-ietf-jmap-metadata-04 properties and filter conditions on
//! Calendar and CalendarEvent (bd JMAP-s4kp.3).
//!
//! Oracle: the -04 text quoted on each test
//! (/home/mark/PROJECT/IETF/specs/draft-ietf-jmap-metadata-04.txt), with
//! hand-written JSON. The namespace and keys follow the -04 §6.6
//! CalendarEvent example ("metadata/acme.example.com/approvalStatus").

use jmap_calendars_types::{
    Calendar, CalendarEvent, CalendarEventFilter, CalendarEventFilterCondition,
};
use jmap_metadata_types::MetadataTextMatch;
use jmap_types::Filter;
use serde_json::json;

/// -04 §2 adds `metadata` and `privateMetadata` (objects keyed by
/// namespace) to each listed type; -04 §4.1 names Calendar among the
/// types that can be listed. The values land in the typed fields, not
/// `extra`. -04 §3.2: "null is equivalent to {}", so `null` decodes as
/// absent; a non-object is rejected.
#[test]
fn calendar_metadata_properties() {
    let mut raw = json!({
        "id": "cal1", "name": "Work", "description": null, "color": null,
        "sortOrder": 0, "isSubscribed": true, "isVisible": true, "isDefault": false,
        "includeInAvailability": "all", "defaultAlertsWithTime": null,
        "defaultAlertsWithoutTime": null, "timeZone": null, "shareWith": null,
        "myRights": {
            "mayReadFreeBusy": true, "mayReadItems": true, "mayWriteAll": true,
            "mayWriteOwn": true, "mayUpdatePrivate": true, "mayRSVP": true,
            "mayShare": true, "mayDelete": true
        },
        "metadata": {"acme.example.com": {"department": "sales"}},
        "privateMetadata": {"acme.example.com": {"pinned": true}}
    });
    let cal: Calendar = serde_json::from_value(raw.clone()).expect("decode");
    assert_eq!(
        cal.metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["department"]),
        Some(&json!("sales"))
    );
    assert_eq!(
        cal.private_metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["pinned"]),
        Some(&json!(true))
    );
    assert!(cal.extra.is_empty());
    let back = serde_json::to_value(&cal).expect("encode");
    assert_eq!(back["metadata"], raw["metadata"]);
    assert_eq!(back["privateMetadata"], raw["privateMetadata"]);

    raw["metadata"] = serde_json::Value::Null;
    let cal: Calendar = serde_json::from_value(raw.clone()).expect("decode null");
    assert_eq!(cal.metadata, None);
    assert!(
        serde_json::to_value(&cal)
            .expect("encode")
            .get("metadata")
            .is_none()
    );
    raw["metadata"] = json!(["x"]);
    assert!(serde_json::from_value::<Calendar>(raw).is_err());
}

/// -04 §2 on CalendarEvent, the type of the -04 §6.6 example. A
/// non-object `privateMetadata` is rejected.
#[test]
fn calendar_event_metadata_properties() {
    let mut raw = json!({
        "id": "CE789", "calendarIds": {"cal1": true}, "title": "Quarterly Review Meeting",
        "metadata": {"acme.example.com": {"approvalStatus": "pending"}},
        "privateMetadata": {"acme.example.com": {"reminder": "bring slides"}}
    });
    let event: CalendarEvent = serde_json::from_value(raw.clone()).expect("decode");
    assert_eq!(
        event
            .metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["approvalStatus"]),
        Some(&json!("pending"))
    );
    assert_eq!(
        event
            .private_metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["reminder"]),
        Some(&json!("bring slides"))
    );
    assert!(event.extra.is_empty());
    let back = serde_json::to_value(&event).expect("encode");
    assert_eq!(back["metadata"], raw["metadata"]);
    assert_eq!(back["privateMetadata"], raw["privateMetadata"]);

    raw["privateMetadata"] = json!(7);
    assert!(serde_json::from_value::<CalendarEvent>(raw).is_err());
}

/// The six wire names of -04 §3.6 ("metadataExists",
/// "privateMetadataExists", "metadataTextContains",
/// "privateMetadataTextContains", "metadataTextEquals",
/// "privateMetadataTextEquals") extend "the FilterCondition object for
/// Foo/query". Each decodes; a misspelt one is still rejected by
/// `deny_unknown_fields`.
#[test]
fn metadata_condition_names_decode_and_typos_reject() {
    let m = json!({"path": "acme.example.com/approvalStatus", "value": "pending"});
    for (name, value) in [
        ("metadataExists", json!("acme.example.com")),
        ("privateMetadataExists", json!("acme.example.com")),
        ("metadataTextContains", m.clone()),
        ("privateMetadataTextContains", m.clone()),
        ("metadataTextEquals", m.clone()),
        ("privateMetadataTextEquals", m.clone()),
    ] {
        let c: CalendarEventFilterCondition =
            serde_json::from_value(json!({ name: value })).expect(name);
        assert!(c.has_metadata_condition(), "{name}");
    }
    assert!(!CalendarEventFilterCondition::default().has_metadata_condition());
    assert!(
        serde_json::from_value::<CalendarEventFilterCondition>(json!({"metadataExist": "ns"}))
            .is_err()
    );
}

/// -04 §3.6 conditions "compose with the data type's existing filter
/// conditions through the usual AND/OR/NOT operators": a hand-written
/// `AND` of a draft-ietf-jmap-calendars-33 §5.11.1 `inCalendar` leaf and a
/// `metadataTextEquals` leaf survives decoding and re-encodes unchanged.
#[test]
fn metadata_condition_survives_operator_tree() {
    let wire = json!({
        "operator": "AND",
        "conditions": [
            { "inCalendar": "cal1" },
            { "metadataTextEquals": { "path": "acme.example.com/approvalStatus", "value": "pending" } }
        ]
    });
    let filter: CalendarEventFilter = serde_json::from_value(wire.clone()).expect("decode");
    let Filter::Operator(op) = &filter else {
        panic!("expected an operator, got {filter:?}");
    };
    let Filter::Condition(leaf) = &op.conditions[1] else {
        panic!("expected a condition");
    };
    assert_eq!(
        leaf.metadata_text_equals,
        Some(MetadataTextMatch::new(
            "acme.example.com/approvalStatus",
            "pending"
        ))
    );
    assert_eq!(serde_json::to_value(&filter).expect("encode"), wire);
}
