//! [`HistoryStore`] (bd JMAP-tna1.3.9 groundwork): recording only listed
//! types, destroyed listings with windows, destroyed query states and
//! their changes, and restore.
//!
//! Oracles: draft-ietf-jmap-object-history §4 (strict window, most recent
//! first), docs/design/DECISIONS.md decision 25 (a state of another window
//! cannot be calculated), §5.1 (a restored object leaves the destroyed
//! set), §2.1 (`supportedTypes` names history types only).

use std::collections::HashMap;
use std::time::Duration;

use jmap_object_history_server::{HistoryRetention, HistoryStore};
use jmap_object_history_types::HistoryGetArguments;
use jmap_types::{Id, UTCDate};
use serde_json::{Value, json};

fn store(retention: HistoryRetention) -> HistoryStore {
    let mut s = HistoryStore::new(
        serde_json::from_value(
            json!({"maxHistoryDuration": null, "supportedTypes": ["ContactCard"]}),
        )
        .unwrap(),
        retention,
    );
    s.set_clock(0);
    s
}

fn ids(v: &[&str]) -> Vec<Id> {
    v.iter().map(|s| Id::from(*s)).collect()
}

#[test]
#[should_panic(expected = "does not support history")]
fn refuses_types_without_history() {
    let _ = HistoryStore::new(
        serde_json::from_value(json!({"maxHistoryDuration": null, "supportedTypes": ["Thread"]}))
            .unwrap(),
        HistoryRetention::new(),
    );
}

#[test]
fn destroyed_listing_state_and_changes() {
    let a = Id::from("a");
    let mut s = store(HistoryRetention::new());
    // Not a listed type: nothing is kept.
    s.record_destroy("AddressBook", &a, &Id::from("x"), json!({"id": "x"}));
    assert!(!s.retains("AddressBook", &a, &Id::from("x")));

    let empty = s.destroyed_query_state("ContactCard", &a, None, None);
    for (n, id) in ["c1", "c2", "c3"].iter().enumerate() {
        s.set_clock(i64::try_from(n).unwrap() * 60_000 + 60_000);
        s.record_destroy("ContactCard", &a, &Id::from(*id), json!({"id": id}));
    }
    let got: Vec<Id> = s
        .destroyed("ContactCard", &a, None, None)
        .unwrap()
        .into_iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(got, ids(&["c3", "c2", "c1"]));
    let after = UTCDate::from("1970-01-01T00:01:00Z");
    let got: Vec<Id> = s
        .destroyed("ContactCard", &a, Some(&after), None)
        .unwrap()
        .into_iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(got, ids(&["c3", "c2"]), "strictly after");

    let since = s.destroyed_query_state("ContactCard", &a, None, None);
    assert_ne!(since, empty);
    assert_eq!(
        s.destroyed_changes_since("ContactCard", &a, &empty, None, None),
        Some(ids(&["c1", "c2", "c3"]))
    );
    assert!(s.restore("ContactCard", &a, &Id::from("c2")));
    assert!(!s.restore("ContactCard", &a, &Id::from("c2")));
    assert_eq!(
        s.destroyed_changes_since("ContactCard", &a, &since, None, None),
        Some(ids(&["c2"]))
    );
    // Another window, or a live state, cannot be calculated.
    assert_eq!(
        s.destroyed_changes_since("ContactCard", &a, &since, Some(&after), None),
        None
    );
    assert_eq!(
        s.destroyed_changes_since("ContactCard", &a, "42", None, None),
        None
    );
}

#[test]
fn select_reads_live_objects_and_prune_reports_discards() {
    let a = Id::from("a");
    let mut s = store(HistoryRetention::new().max_duration(Some(Duration::from_secs(60))));
    s.record_replace(
        "ContactCard",
        &a,
        &Id::from("c1"),
        json!({"id": "c1", "n": 1}),
    );
    s.record_destroy("ContactCard", &a, &Id::from("c2"), json!({"id": "c2"}));
    let live: HashMap<Id, Value> = [(Id::from("c1"), json!({"id": "c1", "n": 2}))].into();
    let mut args = HistoryGetArguments::new();
    args.include_replaced = true;
    args.include_destroyed = true;
    let (rows, missing) = s.select("ContactCard", &a, None, &live, &args).unwrap();
    let snaps: Vec<&Value> = rows.iter().map(|(v, _)| v).collect();
    assert_eq!(
        snaps,
        [
            &json!({"id": "c1", "n": 1}),
            &json!({"id": "c1", "n": 2}),
            &json!({"id": "c2"})
        ]
    );
    assert!(missing.is_empty());

    let since = s.destroyed_query_state("ContactCard", &a, None, None);
    s.set_clock(60_000);
    s.prune();
    assert!(
        s.destroyed("ContactCard", &a, None, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        s.destroyed_changes_since("ContactCard", &a, &since, None, None),
        Some(ids(&["c2"])),
        "a discarded object leaves the destroyed query"
    );
}

#[test]
fn purge_where_drops_matching_histories() {
    let a = Id::from("a");
    let mut s = store(HistoryRetention::new());
    s.record_destroy(
        "ContactCard",
        &a,
        &Id::from("c1"),
        json!({"id": "c1", "book": "x"}),
    );
    s.record_replace(
        "ContactCard",
        &a,
        &Id::from("c2"),
        json!({"id": "c2", "book": "x"}),
    );
    s.record_destroy(
        "ContactCard",
        &a,
        &Id::from("c3"),
        json!({"id": "c3", "book": "y"}),
    );
    let since = s.destroyed_query_state("ContactCard", &a, None, None);
    s.purge_where("ContactCard", &a, |v| v["book"] == "x");
    assert!(!s.retains("ContactCard", &a, &Id::from("c1")));
    assert!(!s.retains("ContactCard", &a, &Id::from("c2")));
    assert!(s.retains("ContactCard", &a, &Id::from("c3")));
    assert_eq!(
        s.destroyed_changes_since("ContactCard", &a, &since, None, None),
        Some(ids(&["c1"]))
    );
}
