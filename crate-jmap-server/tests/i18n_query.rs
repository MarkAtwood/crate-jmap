//! `language` on `Foo/query` and `Foo/queryChanges`
//! (draft-atwood-jmap-updates-00 §3, §4.2.2, §4.2.3).
//!
//! Oracle: the draft's normative text at jmap-chat-spec 2b85363 and the
//! queryState encoding of decision 28 (docs/design/DECISIONS.md), written
//! out by hand.

use jmap_server::{
    BackendChangesError, BackendQueryError, ChangesParams, ChangesResult, GetObject, Id,
    JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, State, handle_query, handle_query_changes, localized_query_state,
    unwrap_query_state,
};
use jmap_types::MethodErrorType;
use jmap_types::i18n::LanguageRange;
use serde_json::{Value, json};

fn ranges(rs: &[&str]) -> Vec<LanguageRange> {
    rs.iter().map(|r| r.parse().unwrap()).collect()
}

// --- queryState --------------------------------------------------------

/// Without `language` the backend's queryState is returned as is, so
/// clients that do not use the capability see no change.
#[test]
fn no_language_leaves_state_alone() {
    let s = State::from("42");
    assert_eq!(localized_query_state(&s, None), s);
    assert_eq!(unwrap_query_state(&s, None).unwrap(), s);
}

/// §4.2.3: "The queryState of a /query result reflects the language it
/// was computed with"; decision 28 encodes it as `<state>|lang=<ranges>`
/// in canonical case, so case variants of the same ranges match.
#[test]
fn language_round_trips() {
    let s = localized_query_state(&State::from("42"), Some(&ranges(&["FR-ch", "de"])));
    assert_eq!(s.as_ref(), "42|lang=fr-CH,de");
    let inner = unwrap_query_state(&s, Some(&ranges(&["fr-ch", "DE"]))).unwrap();
    assert_eq!(inner.as_ref(), "42");
}

/// §4.2.3: "When the language of a /queryChanges differs from that of the
/// /query that produced sinceQueryState, the server returns a
/// cannotCalculateChanges error".
#[test]
fn language_mismatch_cannot_calculate_changes() {
    let fr = localized_query_state(&State::from("42"), Some(&ranges(&["fr", "de"])));
    let plain = State::from("42");
    let cases: [(&State, Option<Vec<LanguageRange>>); 5] = [
        (&fr, Some(ranges(&["de", "fr"]))), // same ranges, other order
        (&fr, Some(ranges(&["fr"]))),
        (&fr, None),
        (&plain, Some(ranges(&["fr"]))),
        (&plain, Some(vec![])), // an empty list is a language, not null
    ];
    for (since, lang) in cases {
        let err = unwrap_query_state(since, lang.as_deref()).unwrap_err();
        assert_eq!(
            err.error_type,
            MethodErrorType::CannotCalculateChanges,
            "{since:?} {lang:?}"
        );
    }
}

// --- generic handlers --------------------------------------------------

#[derive(Debug)]
struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock")
    }
}

impl std::error::Error for MockError {}

#[derive(serde::Serialize, serde::Deserialize)]
struct Thing {}

impl JmapObject for Thing {
    const TYPE_NAME: &'static str = "Thing";
    type Property = ();
}

impl GetObject for Thing {}

impl QueryObject for Thing {
    type Filter = Value;
    type Comparator = Value;
}

/// One empty account; /query and /queryChanges answer with nothing.
struct Backend;

impl JmapBackend for Backend {
    type Error = MockError;
    type CallerCtx = ();

    async fn account_exists(&self, _: &(), _: &Id) -> Result<bool, MockError> {
        Ok(true)
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: Option<&[Id]>,
        _: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MockError> {
        unreachable!("only /query is exercised")
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
    ) -> Result<State, MockError> {
        Ok(State::from("s0"))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MockError>> {
        unreachable!("only /query is exercised")
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MockError>> {
        Ok(QueryResult::new(
            vec![],
            0,
            Some(0),
            State::from("q0"),
            true,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        p: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MockError>> {
        Ok(QueryChangesResult::new(
            p.since_query_state.clone(),
            State::from("q0"),
            None,
            vec![],
            vec![],
        ))
    }
}

/// The generic handlers see no `using`, so §3 applies: a non-null
/// `language` is invalidArguments; `null` is the default.
#[tokio::test]
async fn generic_handlers_reject_language() {
    let err = handle_query::<Thing, _>(
        &Backend,
        &(),
        json!({ "accountId": "a1", "language": ["fr"] }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type, MethodErrorType::InvalidArguments);
    let err = handle_query_changes::<Thing, _>(
        &Backend,
        &(),
        json!({ "accountId": "a1", "sinceQueryState": "q0", "language": ["fr"] }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type, MethodErrorType::InvalidArguments);

    handle_query::<Thing, _>(
        &Backend,
        &(),
        json!({ "accountId": "a1", "language": null }),
    )
    .await
    .unwrap();
    handle_query_changes::<Thing, _>(
        &Backend,
        &(),
        json!({ "accountId": "a1", "sinceQueryState": "q0", "language": null }),
    )
    .await
    .unwrap();
}

/// The backend sees `language` through the params builders.
#[test]
fn params_carry_language() {
    let fr = ranges(&["fr"]);
    let q = QueryParams::<Thing>::new().language(Some(&fr));
    assert_eq!(q.language.map(<[LanguageRange]>::len), Some(1));
    assert!(QueryParams::<Thing>::new().language.is_none());
    let since = State::from("q0");
    let qc = QueryChangesParams::<Thing>::new(&since).language(Some(&fr));
    assert_eq!(qc.language.map(|l| l[0].as_str()), Some("fr"));
}
