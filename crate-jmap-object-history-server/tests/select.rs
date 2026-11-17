//! [`select_versions`] against draft-ietf-jmap-object-history §3
//! (bd JMAP-tna1.3.2).
//!
//! Oracles, hand-derived from the §3 prose (quoted per test): an object
//! updated three times (versions 1-3 replaced at 10:00, 11:00, 12:00; live
//! version 4) and the same history destroyed at 12:00 (version 3 final).

use jmap_object_history_server::{HistorySelector, Selection, Version, select_versions};
use jmap_types::UTCDate;

fn t(hour: u32) -> UTCDate {
    UTCDate::from(format!("2026-03-01T{hour:02}:00:00Z"))
}

fn live_object() -> Vec<Version<&'static str>> {
    vec![
        Version::replaced(1, "v1", t(10), None),
        Version::replaced(2, "v2", t(11), None),
        Version::replaced(3, "v3", t(12), None),
        Version::live(4, "v4"),
    ]
}

fn destroyed_object() -> Vec<Version<&'static str>> {
    live_object()[..3].to_vec()
}

/// The snapshots selected, or `None` for notFound.
fn pick(
    retained: &[Version<&'static str>],
    destroyed: bool,
    s: &HistorySelector,
) -> Option<Vec<&'static str>> {
    match select_versions(retained, destroyed, s).unwrap() {
        Selection::Entries(e) => Some(e.into_iter().map(|(v, _)| v).collect()),
        Selection::NotFound => None,
        _ => unreachable!(),
    }
}

fn sel(replaced: bool, destroyed: bool) -> HistorySelector {
    HistorySelector::new()
        .include_replaced(replaced)
        .include_destroyed(destroyed)
}

/// §3 includeReplaced: "the server returns previous versions ... in
/// addition to the current version"; includeDestroyed: "If
/// includeReplaced is false, only the final version before destruction is
/// returned.  If includeReplaced is true, all retained historical
/// versions of destroyed objects are returned." A destroyed object needs
/// includeDestroyed.
#[test]
fn the_four_flag_combinations() {
    let cases = [
        // (includeReplaced, includeDestroyed, live result, destroyed result)
        (false, false, Some(vec!["v4"]), None),
        (true, false, Some(vec!["v1", "v2", "v3", "v4"]), None),
        (false, true, Some(vec!["v4"]), Some(vec!["v3"])),
        (
            true,
            true,
            Some(vec!["v1", "v2", "v3", "v4"]),
            Some(vec!["v1", "v2", "v3"]),
        ),
    ];
    for (r, d, live, gone) in cases {
        assert_eq!(
            pick(&live_object(), false, &sel(r, d)),
            live,
            "live {r} {d}"
        );
        assert_eq!(
            pick(&destroyed_object(), true, &sel(r, d)),
            gone,
            "destroyed {r} {d}"
        );
    }
}

/// §3: "If includeReplaced and includeDestroyed are both false, the
/// server ignores historyAfter, historyBefore, historySkip, and
/// historyLimit."
#[test]
fn without_flags_the_other_arguments_are_ignored() {
    let s = HistorySelector::new()
        .history_before(Some(t(11)))
        .history_skip(20)
        .history_limit(Some(0));
    assert_eq!(pick(&live_object(), false, &s), Some(vec!["v4"]));
}

/// historyAfter: "only versions with an objectHistory.replaced value
/// strictly after this date are returned, plus the current (live)
/// version".
#[test]
fn history_after_is_strict_and_keeps_the_live_version() {
    let s = sel(true, false).history_after(Some(t(11)));
    assert_eq!(pick(&live_object(), false, &s), Some(vec!["v3", "v4"]));
}

/// historyBefore: "strictly before this date ... The current (live)
/// version ... is not returned."
#[test]
fn history_before_is_strict_and_drops_the_live_version() {
    let s = sel(true, false).history_before(Some(t(12)));
    assert_eq!(pick(&live_object(), false, &s), Some(vec!["v1", "v2"]));
    let s = sel(true, true).history_before(Some(t(12)));
    assert_eq!(pick(&destroyed_object(), true, &s), Some(vec!["v1", "v2"]));
    let both = sel(true, false)
        .history_after(Some(t(10)))
        .history_before(Some(t(12)));
    assert_eq!(pick(&live_object(), false, &both), Some(vec!["v2"]));
}

/// historySkip: "counting from the most recent ... The current version,
/// if it would otherwise be returned, counts as one entry. ... a client
/// that wants only versions older than the 20 most recent passes
/// historySkip: 20; any id with 20 or fewer matching entries is returned
/// in notFound."
#[test]
fn history_skip_counts_from_the_newest_including_live() {
    let s = |n| sel(true, false).history_skip(n);
    assert_eq!(
        pick(&live_object(), false, &s(1)),
        Some(vec!["v1", "v2", "v3"])
    );
    assert_eq!(pick(&live_object(), false, &s(3)), Some(vec!["v1"]));
    assert_eq!(pick(&live_object(), false, &s(4)), None);
    assert_eq!(pick(&live_object(), false, &s(20)), None);
}

/// historyLimit: "the server MUST return the most recent entries ...
/// remaining after historySkip is applied, not the oldest." notFound is
/// decided before the limit, so `historyLimit: 0` puts the id in neither
/// list.
#[test]
fn history_limit_keeps_the_newest() {
    let s = sel(true, false).history_limit(Some(2));
    assert_eq!(pick(&live_object(), false, &s), Some(vec!["v3", "v4"]));
    let s = sel(true, false).history_skip(1).history_limit(Some(2));
    assert_eq!(pick(&live_object(), false, &s), Some(vec!["v2", "v3"]));
    let s = sel(true, false).history_limit(Some(0));
    assert_eq!(pick(&live_object(), false, &s), Some(vec![]));
}

/// §3.1 hasOlder: true when "the server retains at least one older
/// version of this object than this one, whether or not that version was
/// returned"; the live version has `replaced` and `expires` null.
#[test]
fn has_older_counts_unreturned_versions() {
    let Selection::Entries(e) = select_versions(
        &live_object(),
        false,
        &sel(true, false).history_limit(Some(1)),
    )
    .unwrap() else {
        panic!("entries")
    };
    let info = &e[0].1;
    assert_eq!((info.version, info.has_older), (4, true));
    assert_eq!((info.replaced.clone(), info.expires.clone()), (None, None));

    let Selection::Entries(e) = select_versions(&live_object(), false, &sel(true, false)).unwrap()
    else {
        panic!("entries")
    };
    let older: Vec<bool> = e.iter().map(|(_, i)| i.has_older).collect();
    assert_eq!(older, [false, true, true, true]);
    assert_eq!(e[0].1.replaced, Some(t(10)));

    let Selection::Entries(e) =
        select_versions(&destroyed_object(), true, &sel(false, true)).unwrap()
    else {
        panic!("entries")
    };
    assert_eq!((e[0].1.version, e[0].1.has_older), (3, true));
    assert_eq!(e[0].1.replaced, Some(t(12)));
}

/// §3.1: with no retained history the server "MUST still return the
/// current version ... with a replaced of null".
#[test]
fn an_object_without_history_returns_its_live_version() {
    let only = vec![Version::live(1, "now")];
    assert_eq!(pick(&only, false, &sel(true, true)), Some(vec!["now"]));
    assert_eq!(pick(&[], false, &sel(true, true)), None);
}

/// Fractional seconds compare as instants: 12:00:00.5 is after 12:00:00.
#[test]
fn bounds_compare_fractional_seconds() {
    let s = sel(true, false).history_after(Some(UTCDate::from("2026-03-01T11:59:59.5Z")));
    assert_eq!(pick(&live_object(), false, &s), Some(vec!["v3", "v4"]));
    let bad = sel(true, false).history_after(Some(UTCDate::from("nope")));
    assert!(select_versions(&live_object(), false, &bad).is_err());
}
