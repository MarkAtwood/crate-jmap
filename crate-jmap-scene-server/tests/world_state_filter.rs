//! The `worldStateEquals` filter of `SceneRegion/query`
//! (draft-atwood-jmap-scene-00 at jmap-chat-spec 93f2258,
//! §SceneRegion/query and §Filter Validation). Each test quotes the
//! sentence it checks; the expected id lists are worked out by hand from
//! the seeded regions.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{handle_scene_region_query, handle_scene_region_query_changes};
use jmap_types::JmapError;
use serde_json::{Value, json};

/// | id | worldState |
/// |---|---|
/// | chess1 | `{"gameSchema": "urn:game:chess:standard", "phase": "waiting-for-players", "seats": 2}` |
/// | chess2 | `{"gameSchema": "urn:game:chess:standard", "phase": "in-progress", "seats": 2.0}` |
/// | go1 | `{"gameSchema": "urn:game:go", "ranked": true, "nested": {"phase": "waiting-for-players"}}` |
/// | plain | `null` |
/// | none | absent |
fn backend_with_games() -> MemoryBackend {
    let backend = backend();
    let rows = [
        (
            "chess1",
            json!({"gameSchema": "urn:game:chess:standard", "phase": "waiting-for-players", "seats": 2}),
        ),
        (
            "chess2",
            json!({"gameSchema": "urn:game:chess:standard", "phase": "in-progress", "seats": 2.0}),
        ),
        (
            "go1",
            json!({"gameSchema": "urn:game:go", "ranked": true, "nested": {"phase": "waiting-for-players"}}),
        ),
        ("plain", Value::Null),
    ];
    for (i, (id, ws)) in rows.into_iter().enumerate() {
        let mut r = stored_region(id, id, &format!("2026-06-0{}T10:00:00Z", i + 1));
        r["worldState"] = ws;
        seed_as(&backend, "SceneRegion", r);
    }
    seed_as(
        &backend,
        "SceneRegion",
        stored_region("none", "none", "2026-06-09T10:00:00Z"),
    );
    backend
}

async fn query(backend: &MemoryBackend, filter: Value) -> Result<Value, JmapError> {
    let args = json!({
        "accountId": ACCOUNT,
        "filter": filter,
        "sort": [{ "property": "name", "isAscending": true }]
    });
    handle_scene_region_query(backend, &jmap_types::Id::from(ALICE), args)
        .await
        .map(|o| o.primary)
}

async fn ids(backend: &MemoryBackend, filter: Value) -> Vec<String> {
    let resp = query(backend, filter).await.expect("query");
    resp["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().expect("id").to_owned())
        .collect()
}

/// The draft's lobby-browser example: "regions whose `worldState` names a
/// game schema" with `{"gameSchema": "urn:game:chess:standard", "phase":
/// "waiting-for-players"}`; only chess1 has both.
#[tokio::test]
async fn spec_example_lobby_browser() {
    let backend = backend_with_games();
    let got = ids(
        &backend,
        json!({"worldStateEquals": {
            "gameSchema": "urn:game:chess:standard",
            "phase": "waiting-for-players"
        }}),
    )
    .await;
    assert_eq!(got, ["chess1"]);
}

/// "A region matches when its `worldState` has every one of these keys
/// with an equal value": both chess regions name the schema.
#[tokio::test]
async fn every_key_must_match() {
    let backend = backend_with_games();
    let got = ids(
        &backend,
        json!({"worldStateEquals": {"gameSchema": "urn:game:chess:standard"}}),
    )
    .await;
    assert_eq!(got, ["chess1", "chess2"]);
}

/// "for Numbers the same numeric value": `2` matches both `2` and `2.0`.
/// "the same JSON type": the String `"2"` matches neither, and `true`
/// matches only the Boolean.
#[tokio::test]
async fn numbers_by_value_and_types_must_agree() {
    let backend = backend_with_games();
    assert_eq!(
        ids(&backend, json!({"worldStateEquals": {"seats": 2}})).await,
        ["chess1", "chess2"]
    );
    assert_eq!(
        ids(&backend, json!({"worldStateEquals": {"seats": 2.0}})).await,
        ["chess1", "chess2"]
    );
    assert!(
        ids(&backend, json!({"worldStateEquals": {"seats": "2"}}))
            .await
            .is_empty()
    );
    assert_eq!(
        ids(&backend, json!({"worldStateEquals": {"ranked": true}})).await,
        ["go1"]
    );
    assert!(
        ids(&backend, json!({"worldStateEquals": {"ranked": "true"}}))
            .await
            .is_empty()
    );
}

/// "Nested values are not compared": the top-level key `phase` is absent
/// from go1 even though its nested object has one.
#[tokio::test]
async fn nested_values_are_not_compared() {
    let backend = backend_with_games();
    assert_eq!(
        ids(
            &backend,
            json!({"worldStateEquals": {"phase": "waiting-for-players"}})
        )
        .await,
        ["chess1"]
    );
}

/// "An empty object matches every region", including those whose
/// `worldState` is `null` or absent.
#[tokio::test]
async fn empty_object_matches_every_region() {
    let backend = backend_with_games();
    assert_eq!(
        ids(&backend, json!({"worldStateEquals": {}})).await,
        ["chess1", "chess2", "go1", "none", "plain"]
    );
}

/// The filter combines with the others "with logical AND" and inside a
/// FilterOperator.
#[tokio::test]
async fn combines_with_operators() {
    let backend = backend_with_games();
    let got = ids(
        &backend,
        json!({"operator": "OR", "conditions": [
            {"worldStateEquals": {"phase": "in-progress"}},
            {"worldStateEquals": {"gameSchema": "urn:game:go"}}
        ]}),
    )
    .await;
    assert_eq!(got, ["chess2", "go1"]);
}

/// "If `worldStateEquals` is not an object, or one of its values is not a
/// String, Number, or Boolean, the server MUST return `invalidArguments`",
/// also inside a FilterOperator and on `/queryChanges`.
#[tokio::test]
async fn non_scalar_values_are_invalid_arguments() {
    let backend = backend_with_games();
    for filter in [
        json!({"worldStateEquals": "urn:game:go"}),
        json!({"worldStateEquals": null}),
        json!({"worldStateEquals": ["gameSchema"]}),
        json!({"worldStateEquals": {"phase": null}}),
        json!({"worldStateEquals": {"nested": {"phase": "x"}}}),
        json!({"worldStateEquals": {"tags": ["a"]}}),
        json!({"operator": "AND", "conditions": [{"worldStateEquals": {"x": [1]}}]}),
    ] {
        let err = query(&backend, filter.clone()).await.unwrap_err();
        assert_eq!(err.error_type.as_str(), "invalidArguments", "{filter}");
    }
    let err = handle_scene_region_query_changes(
        &backend,
        &jmap_types::Id::from(ALICE),
        json!({
            "accountId": ACCOUNT,
            "sinceQueryState": "0",
            "filter": {"worldStateEquals": {"x": {}}}
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}
