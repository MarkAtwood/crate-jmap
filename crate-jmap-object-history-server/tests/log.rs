//! [`VersionLog`], [`fold_restore`] and the millisecond clock
//! (bd JMAP-tna1.3.2).
//!
//! Oracles: draft-ietf-jmap-object-history §3.1 (replaced values "MUST be
//! distinct"; a server "can collapse them"; "prune old versions in any
//! order"; `expires`), §5.1 (history continues after a restore), the -04
//! editor's copy §3.1 (the newest retained version is the final version)
//! and §5.1 (Foo/changes compares sinceState with now), decision 24
//! (oldest-first pruning), and hand-computed calendar dates.

use std::time::Duration;

use jmap_object_history_server::{
    ChangeEvent, ChangeKind, HistoryRetention, HistorySelector, Selection, VersionLog,
    fold_restore, millis_from_utc_date, utc_date_from_millis,
};
use jmap_types::{Id, UTCDate};

const T: &str = "ContactCard";
const MIN: i64 = 60_000;

fn ids() -> (Id, Id) {
    (Id::from("a1"), Id::from("c1"))
}

/// (snapshot, version, replaced) of each selected entry, or `None`.
fn entries(
    log: &VersionLog<&'static str>,
    live: Option<&'static str>,
    sel: &HistorySelector,
    cut: Option<i64>,
) -> Option<Vec<(&'static str, u64, Option<String>)>> {
    let (a, c) = ids();
    match log.select(T, &a, &c, live, sel, cut).unwrap() {
        Selection::Entries(e) => Some(
            e.into_iter()
                .map(|(v, i)| (v, i.version, i.replaced.map(|r| r.as_ref().to_owned())))
                .collect(),
        ),
        Selection::NotFound => None,
        _ => unreachable!(),
    }
}

fn all() -> HistorySelector {
    HistorySelector::new()
        .include_replaced(true)
        .include_destroyed(true)
}

#[test]
fn records_versions_with_expires() {
    let (a, c) = ids();
    let mut log =
        VersionLog::new(HistoryRetention::new().max_duration(Some(Duration::from_secs(2_592_000))));
    log.record_replace(T, &a, &c, "Robert", 0);
    log.record_replace(T, &a, &c, "Bob", MIN);
    let (_, c2) = (a.clone(), c.clone());
    let sel = all();
    let got = entries(&log, Some("Bob+e2"), &sel, None).unwrap();
    assert_eq!(
        got,
        [
            ("Robert", 1, Some("1970-01-01T00:00:00Z".to_owned())),
            ("Bob", 2, Some("1970-01-01T00:01:00Z".to_owned())),
            ("Bob+e2", 3, None),
        ]
    );
    // §3.2.1: expires = replaced + 2592000 s.
    let Selection::Entries(e) = log.select(T, &a, &c2, Some("x"), &sel, None).unwrap() else {
        panic!()
    };
    assert_eq!(e[0].1.expires, Some(UTCDate::from("1970-01-31T00:00:00Z")));
    assert_eq!(e[2].1.expires, None);
}

/// §3.1: two versions replaced in the same millisecond collapse into one,
/// so `replaced` stays distinct; the newer snapshot is kept.
#[test]
fn equal_millisecond_versions_collapse() {
    let (a, c) = ids();
    let mut log = VersionLog::default();
    log.record_replace(T, &a, &c, "one", 5);
    log.record_replace(T, &a, &c, "two", 5);
    let got = entries(&log, Some("three"), &all(), None).unwrap();
    assert_eq!(
        got,
        [
            ("two", 2, Some("1970-01-01T00:00:00.005Z".to_owned())),
            ("three", 3, None),
        ]
    );
}

/// §5.1: a restore returns the final version, and the history continues:
/// the destroyed versions become previous versions of the restored object.
/// A live or unknown id cannot be restored.
#[test]
fn restore_continues_the_history() {
    let (a, c) = ids();
    let mut log = VersionLog::default();
    assert_eq!(log.restore(T, &a, &c), None);
    log.record_replace(T, &a, &c, "v1", MIN);
    assert_eq!(log.restore(T, &a, &c), None, "live");
    log.record_destroy(T, &a, &c, "v2", 2 * MIN);
    assert!(log.is_destroyed(T, &a, &c) && log.retains(T, &a, &c));
    assert_eq!(entries(&log, None, &HistorySelector::new(), None), None);
    assert_eq!(log.final_version(T, &a, &c), Some(&"v2"));
    assert_eq!(log.destroyed_objects(T, &a)[0].destroyed_at_ms, 2 * MIN);
    log.record_replace(T, &a, &c, "ignored", 3 * MIN);

    assert_eq!(log.restore(T, &a, &c), Some("v2"));
    assert!(!log.is_destroyed(T, &a, &c));
    assert!(log.destroyed_objects(T, &a).is_empty());
    let got = entries(&log, Some("v2 restored"), &all(), None).unwrap();
    let versions: Vec<u64> = got.iter().map(|e| e.1).collect();
    assert_eq!(versions, [1, 2, 3]);
    assert_eq!(got[2], ("v2 restored", 3, None));
}

/// Decision 24: retention discards oldest first, so a destroyed object's
/// final version goes last, and the object goes with it.
#[test]
fn prune_discards_oldest_first() {
    let (a, c) = ids();
    let mut log =
        VersionLog::new(HistoryRetention::new().max_duration(Some(Duration::from_secs(600))));
    log.record_replace(T, &a, &c, "v1", 0);
    log.record_replace(T, &a, &c, "v2", 5 * MIN);
    log.record_destroy(T, &a, &c, "v3", 8 * MIN);
    log.prune(10 * MIN);
    let got = entries(&log, None, &all(), None).unwrap();
    assert_eq!(got.iter().map(|e| e.0).collect::<Vec<_>>(), ["v2", "v3"]);
    log.prune(15 * MIN);
    assert_eq!(log.final_version(T, &a, &c), Some(&"v3"));
    log.prune(18 * MIN);
    assert!(!log.is_destroyed(T, &a, &c));
    assert!(!log.retains(T, &a, &c));
    assert_eq!(entries(&log, None, &all(), None), None);
    assert!(log.destroyed_objects(T, &a).is_empty());

    let mut log = VersionLog::new(HistoryRetention::new().max_versions_per_object(Some(1)));
    log.record_replace(T, &a, &c, "v1", MIN);
    log.record_replace(T, &a, &c, "v2", 2 * MIN);
    log.prune(3 * MIN);
    let got = entries(&log, Some("v3"), &all(), None).unwrap();
    assert_eq!(got.iter().map(|e| e.0).collect::<Vec<_>>(), ["v2", "v3"]);
}

/// -04 §3.1: when the version current at destruction is discarded and an
/// older one is retained, "the older one is the final version ... and its
/// replaced value ... is used as the time the object was destroyed".
#[test]
fn an_older_version_stands_in_for_a_discarded_final_one() {
    let (a, c) = ids();
    let mut log = VersionLog::default();
    log.record_replace(T, &a, &c, "v1", MIN);
    log.record_destroy(T, &a, &c, "v2", 2 * MIN);
    assert!(log.discard_version(T, &a, &c, 2));
    assert!(!log.discard_version(T, &a, &c, 2));
    let gone = &log.destroyed_objects(T, &a)[0];
    assert_eq!((gone.snapshot, gone.destroyed_at_ms), ("v1", MIN));
    assert_eq!(gone.destroyed_at, UTCDate::from("1970-01-01T00:01:00Z"));
    let only_final = HistorySelector::new().include_destroyed(true);
    assert_eq!(
        entries(&log, None, &only_final, None),
        Some(vec![("v1", 1, Some("1970-01-01T00:01:00Z".to_owned()))])
    );
    assert_eq!(log.restore(T, &a, &c), Some("v1"));

    log.record_destroy(T, &a, &c, "v3", 3 * MIN);
    log.purge(T, &a, &c);
    assert!(!log.retains(T, &a, &c));
}

/// §8.2 conservative cut: versions replaced before the container's rights
/// changed are hidden, and so is an object destroyed before it.
#[test]
fn rights_cut_hides_older_versions() {
    let (a, c) = ids();
    let mut log = VersionLog::default();
    log.record_replace(T, &a, &c, "v1", MIN);
    log.record_replace(T, &a, &c, "v2", 3 * MIN);
    let got = entries(&log, Some("v3"), &all(), Some(2 * MIN)).unwrap();
    assert_eq!(got.iter().map(|e| e.0).collect::<Vec<_>>(), ["v2", "v3"]);
    log.record_destroy(T, &a, &c, "v3", 4 * MIN);
    assert_eq!(entries(&log, None, &all(), Some(5 * MIN)), None);
}

/// -04 §5.1: "Foo/changes ... reports a restored object by comparing
/// sinceState with the new state, exactly as if the object had never been
/// destroyed between them".
#[test]
fn fold_restore_compares_since_state_with_now() {
    use ChangeEvent::{Created, Destroyed, Restored, Updated};
    let cases: [(&[ChangeEvent], Option<ChangeKind>); 9] = [
        // Existed at sinceState, destroyed and restored since: updated.
        (&[Destroyed, Restored], Some(ChangeKind::Updated)),
        (
            &[Updated, Destroyed, Restored, Updated],
            Some(ChangeKind::Updated),
        ),
        // Destroyed before sinceState, restored since: created.
        (&[Restored], Some(ChangeKind::Created)),
        (&[Restored, Updated], Some(ChangeKind::Created)),
        // Restored before sinceState, destroyed again since: destroyed.
        (&[Destroyed], Some(ChangeKind::Destroyed)),
        (
            &[Destroyed, Restored, Destroyed],
            Some(ChangeKind::Destroyed),
        ),
        // Destroyed at sinceState, restored and destroyed again: not reported.
        (&[Restored, Destroyed], None),
        (&[Created, Destroyed], None),
        (&[], None),
    ];
    for (events, want) in cases {
        assert_eq!(fold_restore(events), want, "{events:?}");
    }
}

/// RFC 8620 §1.4 dates, hand-computed: 2000-02-29T12:00:00Z is
/// 951825600 s; a zero fraction is omitted and trailing zeros trimmed.
#[test]
fn millisecond_clock_as_utc_dates() {
    assert_eq!(utc_date_from_millis(0).as_ref(), "1970-01-01T00:00:00Z");
    assert_eq!(
        utc_date_from_millis(951_825_600_500).as_ref(),
        "2000-02-29T12:00:00.5Z"
    );
    assert_eq!(
        utc_date_from_millis(-1).as_ref(),
        "1969-12-31T23:59:59.999Z"
    );
    for ms in [0, 951_825_600_500, -1, 1_772_323_200_042] {
        let d = utc_date_from_millis(ms);
        assert!(UTCDate::new_validated(d.as_ref()).is_ok(), "{d:?}");
        assert_eq!(millis_from_utc_date(&d).unwrap(), ms);
    }
    assert_eq!(
        millis_from_utc_date(&UTCDate::from("1970-01-01T00:00:00.0005Z")).unwrap(),
        0
    );
}
