//! Quota wire form against RFC 9425.
//!
//! Oracle: the RFC 9425 §5.1 example response object (its `description`
//! joined onto one line) and the §3 / §4.4 value lists; all expected JSON
//! is hand-copied from the RFC text.

use jmap_quotas_types::{
    Quota, QuotaComparator, QuotaComparatorProperty, QuotaFilter, QuotaFilterCondition,
    ResourceType, Scope,
};
use serde_json::{Value, json};

fn example() -> Value {
    json!({
        "id": "2a06df0d-9865-4e74-a92f-74dcc814270e",
        "resourceType": "count",
        "used": 1056,
        "warnLimit": 1600,
        "softLimit": 1800,
        "hardLimit": 2000,
        "scope": "account",
        "name": "bob@example.com",
        "description": "Personal account usage. When the soft limit is reached, the user is not allowed to send mails or create contacts and calendar events anymore.",
        "types" : [ "Mail", "Calendar", "Contact" ]
    })
}

#[test]
fn section_5_1_example_round_trips() {
    let q: Quota = serde_json::from_value(example()).unwrap();
    assert_eq!(q.id.as_ref(), "2a06df0d-9865-4e74-a92f-74dcc814270e");
    assert_eq!(q.resource_type, ResourceType::Count);
    assert_eq!(q.used, 1056);
    assert_eq!(
        (q.warn_limit, q.soft_limit, q.hard_limit),
        (Some(1600), Some(1800), 2000)
    );
    assert_eq!(q.scope, Scope::Account);
    assert_eq!(q.name, "bob@example.com");
    assert_eq!(q.types, ["Mail", "Calendar", "Contact"]);
    assert!(q.extra.is_empty());
    assert_eq!(serde_json::to_value(&q).unwrap(), example());
}

/// §4.1: warnLimit, softLimit and description are "optional" and
/// `|null`: absent or null decode to None, which is omitted.
#[test]
fn optional_properties_absent_or_null() {
    let mut v = example();
    let o = v.as_object_mut().unwrap();
    o.remove("warnLimit");
    o.insert("softLimit".into(), Value::Null);
    o.insert("description".into(), Value::Null);
    let q: Quota = serde_json::from_value(v).unwrap();
    assert_eq!((q.warn_limit, q.soft_limit), (None, None));
    assert_eq!(q.description, None);
    let out = serde_json::to_value(&q).unwrap();
    for k in ["warnLimit", "softLimit", "description"] {
        assert!(out.get(k).is_none(), "{k} in {out}");
    }
}

/// Required properties (§4.1 lists them without "optional") must be
/// present.
#[test]
fn required_properties_are_required() {
    for k in [
        "id",
        "resourceType",
        "used",
        "hardLimit",
        "scope",
        "name",
        "types",
    ] {
        let mut v = example();
        v.as_object_mut().unwrap().remove(k);
        assert!(serde_json::from_value::<Quota>(v).is_err(), "{k}");
    }
}

/// Unknown properties survive a round trip in `extra`.
#[test]
fn unknown_properties_round_trip() {
    let mut v = example();
    v["vendor:x"] = json!({ "a": 1 });
    let q: Quota = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(q.extra["vendor:x"], json!({ "a": 1 }));
    assert_eq!(serde_json::to_value(&q).unwrap(), v);
}

/// §3.1 Scope and §3.2 ResourceType value lists; unknown values round-trip.
#[test]
fn scope_and_resource_type_values() {
    for (s, want) in [
        ("account", Scope::Account),
        ("domain", Scope::Domain),
        ("global", Scope::Global),
        ("planet", Scope::Other("planet".into())),
    ] {
        assert_eq!(serde_json::from_value::<Scope>(json!(s)).unwrap(), want);
        assert_eq!(serde_json::to_value(&want).unwrap(), json!(s));
    }
    for (s, want) in [
        ("count", ResourceType::Count),
        ("octets", ResourceType::Octets),
        ("seconds", ResourceType::Other("seconds".into())),
    ] {
        assert_eq!(
            serde_json::from_value::<ResourceType>(json!(s)).unwrap(),
            want
        );
        assert_eq!(serde_json::to_value(&want).unwrap(), json!(s));
    }
}

/// §4.4 FilterCondition: name, scope, resourceType, type. Unknown
/// conditions are rejected.
#[test]
fn filter_condition_wire_form() {
    let wire =
        json!({ "name": "bob", "scope": "domain", "resourceType": "octets", "type": "Mail" });
    let c: QuotaFilterCondition = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(c.name.as_deref(), Some("bob"));
    assert_eq!(c.scope, Some(Scope::Domain));
    assert_eq!(c.resource_type, Some(ResourceType::Octets));
    assert_eq!(c.type_name.as_deref(), Some("Mail"));
    assert_eq!(serde_json::to_value(&c).unwrap(), wire);

    assert_eq!(
        serde_json::to_value(QuotaFilterCondition::default()).unwrap(),
        json!({})
    );
    assert!(serde_json::from_value::<QuotaFilterCondition>(json!({ "used": 3 })).is_err());

    let tree: QuotaFilter = serde_json::from_value(json!({
        "operator": "OR", "conditions": [{ "scope": "account" }, { "type": "Calendar" }]
    }))
    .unwrap();
    assert!(matches!(tree, jmap_quotas_types::Filter::Operator(_)));
}

/// §4.4 sort properties name and used; RFC 8620 §5.5 isAscending default.
#[test]
fn comparator_wire_form() {
    let up = QuotaComparator::new(QuotaComparatorProperty::Used);
    assert_eq!(
        serde_json::to_value(&up).unwrap(),
        json!({ "property": "used" })
    );
    let mut down = QuotaComparator::new(QuotaComparatorProperty::Name);
    down.is_ascending = false;
    let wire = json!({ "property": "name", "isAscending": false });
    assert_eq!(serde_json::to_value(&down).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<QuotaComparator>(wire).unwrap(),
        down
    );
    assert_eq!(
        serde_json::from_value::<QuotaComparator>(json!({ "property": "hardLimit" }))
            .unwrap()
            .property,
        QuotaComparatorProperty::Other("hardLimit".into())
    );
}

/// draft-atwood-jmap-updates-00 §4.1, §5.2: `locale` and `localizations`
/// round-trip, and Quota lists `name` and `description` as localizable.
#[test]
fn localizations_round_trip() {
    use jmap_types::JmapObject;
    let mut v = example();
    v["locale"] = json!("en");
    v["localizations"] = json!({ "de": { "name": "Bobs Konto", "description": null } });
    let q: Quota = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(q.locale.as_ref().map(|t| t.as_str()), Some("en"));
    assert_eq!(serde_json::to_value(&q).unwrap(), v);
    let names: Vec<(&str, bool)> = Quota::LOCALIZABLE
        .iter()
        .map(|p| (p.name, p.nullable))
        .collect();
    assert_eq!(names, [("name", false), ("description", true)]);
}
