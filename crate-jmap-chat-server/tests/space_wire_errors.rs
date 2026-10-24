//! Space method wire-level paths not exercised elsewhere: malformed
//! structural-key entries, a non-object patch, `ifInState`, `destroy`
//! element types, `Space/query` arguments, `Space/changes`,
//! `Space/queryChanges`, and an expired invite (bd:JMAP-w8fu.16).
//!
//! Oracles: RFC 8620 §3.6.2, §5.2, §5.3, §5.5, §5.6 and
//! draft-atwood-jmap-chat-00 (29e9a41) §Space/set, §Space/join; each test
//! quotes the sentence it relies on.

mod common;

use common::{ACCOUNT_ID, IdentityBackend, SPACE_ID, seed_space, seed_with_admin};
use jmap_chat_server::{
    handle_space_changes, handle_space_get, handle_space_join, handle_space_query,
    handle_space_query_changes, handle_space_set,
};
use jmap_server::{HandlerOutput, JmapError};
use jmap_types::Id;
use serde_json::{Value, json};

fn caller() -> Id {
    Id::from(ACCOUNT_ID)
}

/// A Space administered by the caller, so permission gates never decide
/// the outcome.
fn admin_backend() -> IdentityBackend {
    let backend = IdentityBackend::new();
    seed_with_admin(&backend, ACCOUNT_ID);
    backend
}

async fn space_set(backend: &IdentityBackend, args: Value) -> Result<Value, JmapError> {
    handle_space_set(
        backend,
        &caller(),
        &std::collections::BTreeMap::new(),
        args,
        &[],
    )
    .await
    .map(|HandlerOutput { primary, .. }| primary)
}

async fn update(backend: &IdentityBackend, patch: Value) -> Value {
    space_set(
        backend,
        json!({ "accountId": ACCOUNT_ID, "update": { SPACE_ID: patch } }),
    )
    .await
    .expect("Space/set")
}

async fn space_state(backend: &IdentityBackend) -> String {
    let HandlerOutput { primary, .. } = handle_space_get(
        backend,
        &caller(),
        json!({ "accountId": ACCOUNT_ID, "ids": [] }),
        &[],
    )
    .await
    .expect("Space/get");
    primary["state"].as_str().expect("state").to_owned()
}

/// Asserts `resp` refuses the Space with `invalidProperties` naming `key`.
fn assert_invalid_key(resp: &Value, key: &str) {
    let err = &resp["notUpdated"][SPACE_ID];
    assert_eq!(err["type"], "invalidProperties", "{resp:?}");
    assert_eq!(err["properties"], json!([key]), "{resp:?}");
    assert!(resp["updated"].get(SPACE_ID).is_none(), "{resp:?}");
}

/// L1355: "`removeMembers` (String[]): ChatContact.ids to remove." A
/// non-string element does not match the declared type.
#[tokio::test]
async fn remove_members_non_string_entry_is_invalid_properties() {
    let backend = admin_backend();
    let resp = update(&backend, json!({ "removeMembers": [42] })).await;
    assert_invalid_key(&resp, "removeMembers");
}

/// L1349-1350: "`updateRoles` (Object[]): Each entry: `id` (String) and
/// any of `name`, `color`, `permissions`, `position`."
#[tokio::test]
async fn update_roles_non_object_entry_is_invalid_properties() {
    let backend = admin_backend();
    let resp = update(&backend, json!({ "updateRoles": ["r-admin"] })).await;
    assert_invalid_key(&resp, "updateRoles");
}

/// L1349-1350: an `updateRoles` entry's `id` is a String.
#[tokio::test]
async fn update_roles_non_string_id_is_invalid_properties() {
    let backend = admin_backend();
    let resp = update(
        &backend,
        json!({ "updateRoles": [{ "id": 7, "name": "X" }] }),
    )
    .await;
    assert_invalid_key(&resp, "updateRoles");
}

/// L1352-1353: "`addMembers` (Object[]): Each entry: `id` (ChatContact.id,
/// String) and optional `roleIds` (String[])."
#[tokio::test]
async fn add_members_non_object_entry_is_invalid_properties() {
    let backend = admin_backend();
    let resp = update(&backend, json!({ "addMembers": ["bob"] })).await;
    assert_invalid_key(&resp, "addMembers");
}

/// L1352-1353: an `addMembers` entry's `id` is a String.
#[tokio::test]
async fn add_members_non_string_id_is_invalid_properties() {
    let backend = admin_backend();
    let resp = update(&backend, json!({ "addMembers": [{ "id": ["bob"] }] })).await;
    assert_invalid_key(&resp, "addMembers");
}

/// L1352-1353: an `addMembers` entry has only `id` and `roleIds`; a
/// `nick` there is not part of the entry shape.
#[tokio::test]
async fn add_members_unknown_entry_property_is_invalid_properties() {
    let backend = admin_backend();
    let resp = update(
        &backend,
        json!({ "addMembers": [{ "id": "bob", "nick": "Bobby" }] }),
    )
    .await;
    assert_invalid_key(&resp, "addMembers");
}

/// L1352-1353: `roleIds` is optional, so `null` means no roles and the
/// member is added.
#[tokio::test]
async fn add_members_null_role_ids_adds_member() {
    let backend = admin_backend();
    let resp = update(
        &backend,
        json!({ "addMembers": [{ "id": "bob", "roleIds": null }] }),
    )
    .await;
    assert!(resp["notUpdated"].get(SPACE_ID).is_none(), "{resp:?}");
    let HandlerOutput { primary, .. } = handle_space_get(
        &backend,
        &caller(),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    let bob = primary["list"][0]["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == "bob")
        .cloned()
        .expect("bob added");
    assert_eq!(bob["roleIds"], json!([]), "{bob:?}");
}

/// RFC 8620 §5.3 "invalidPatch": "The PatchObject given to update the
/// record was not a valid patch." A PatchObject is a JSON object.
#[tokio::test]
async fn non_object_patch_is_invalid_patch() {
    let backend = admin_backend();
    let resp = update(&backend, json!(["name", "X"])).await;
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "invalidPatch",
        "{resp:?}"
    );
}

/// RFC 8620 §5.3 `ifInState`: "If supplied, the string must match the
/// current state; otherwise, the method will be aborted and a
/// "stateMismatch" error returned."
#[tokio::test]
async fn if_in_state_mismatch_aborts_with_state_mismatch() {
    let backend = admin_backend();
    let before = space_state(&backend).await;
    let err = space_set(
        &backend,
        json!({
            "accountId": ACCOUNT_ID,
            "ifInState": format!("{before}-stale"),
            "update": { SPACE_ID: { "name": "Renamed" } }
        }),
    )
    .await
    .expect_err("stale ifInState");
    assert_eq!(err.error_type.to_string(), "stateMismatch");
    assert_eq!(space_state(&backend).await, before, "nothing applied");
}

/// RFC 8620 §5.3: `destroy` is `Id[]`; §3.6.2 "invalidArguments": "One of
/// the arguments is of the wrong type".
#[tokio::test]
async fn destroy_non_string_element_is_invalid_arguments() {
    let backend = admin_backend();
    let err = space_set(&backend, json!({ "accountId": ACCOUNT_ID, "destroy": [1] }))
        .await
        .expect_err("non-string destroy id");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}

async fn space_query(backend: &IdentityBackend, extra: Value) -> Result<Value, JmapError> {
    let mut args = json!({ "accountId": ACCOUNT_ID });
    args.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    handle_space_query(backend, &caller(), args, &[])
        .await
        .map(|HandlerOutput { primary, .. }| primary)
}

/// RFC 8620 §5.5 `limit`: "If a negative value is given, the call MUST be
/// rejected with an "invalidArguments" error."
#[tokio::test]
async fn query_negative_limit_is_invalid_arguments() {
    let backend = admin_backend();
    let err = space_query(&backend, json!({ "limit": -1 }))
        .await
        .expect_err("negative limit");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}

/// RFC 8620 §5.5 `position: "Int"`; §3.6.2 "invalidArguments": "One of
/// the arguments is of the wrong type".
#[tokio::test]
async fn query_string_position_is_invalid_arguments() {
    let backend = admin_backend();
    let err = space_query(&backend, json!({ "position": "0" }))
        .await
        .expect_err("string position");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}

/// RFC 8620 §5.5 `total`: "The total number of Foos in the results (given
/// the "filter"). This argument MUST be omitted if the "calculateTotal"
/// request argument is not true." One Space is seeded.
#[tokio::test]
async fn query_total_present_only_when_requested() {
    let backend = admin_backend();
    let with = space_query(&backend, json!({ "calculateTotal": true }))
        .await
        .expect("Space/query");
    assert_eq!(with["total"], 1, "{with:?}");
    let without = space_query(&backend, json!({})).await.expect("Space/query");
    assert!(without.get("total").is_none(), "{without:?}");
}

/// Creates a Space named `name`; returns its id.
async fn create_space(backend: &IdentityBackend, name: &str) -> String {
    let resp = space_set(
        backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "c1": { "name": name } } }),
    )
    .await
    .expect("Space/set create");
    resp["created"]["c1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created: {resp:?}"))
        .to_owned()
}

/// L1314 §Space/changes: "Standard JMAP `/changes`." RFC 8620 §5.2
/// `created`: "An array of ids for records that have been created since
/// the old state."
#[tokio::test]
async fn changes_reports_created_space() {
    let backend = admin_backend();
    let since = space_state(&backend).await;
    let new_id = create_space(&backend, "Zeta").await;

    let HandlerOutput { primary, .. } = handle_space_changes(
        &backend,
        &caller(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
        &[],
    )
    .await
    .expect("Space/changes");
    assert_eq!(primary["oldState"], since, "{primary:?}");
    assert_eq!(primary["created"], json!([new_id]), "{primary:?}");
    assert_eq!(primary["destroyed"], json!([]), "{primary:?}");
}

/// L1396 §Space/queryChanges: "Standard JMAP `/queryChanges`"; L1385
/// §Space/query "Default sort: `name` ascending." RFC 8620 §5.6 `added`:
/// "The id and index in the query results (in the new state) for every Foo
/// that has been added to the results since the old state". "Zeta" sorts
/// after the seeded "Test Space", so its index is 1.
#[tokio::test]
async fn query_changes_reports_added_space_at_default_sort_index() {
    let backend = admin_backend();
    let before = space_query(&backend, json!({})).await.expect("Space/query");
    let new_id = create_space(&backend, "Zeta").await;

    let HandlerOutput { primary, .. } = handle_space_query_changes(
        &backend,
        &caller(),
        json!({ "accountId": ACCOUNT_ID, "sinceQueryState": before["queryState"] }),
        &[],
    )
    .await
    .expect("Space/queryChanges");
    assert_eq!(
        primary["added"],
        json!([{ "id": new_id, "index": 1 }]),
        "{primary:?}"
    );
}

/// RFC 8620 §5.6: `sort` is as for `/query`; §5.5 "unsupportedSort": "The
/// "sort" is syntactically valid, but it includes a property the server
/// does not support sorting on". L1385 names only `name` as a Space sort.
#[tokio::test]
async fn query_changes_unsupported_sort_is_rejected() {
    let backend = admin_backend();
    let err = handle_space_query_changes(
        &backend,
        &caller(),
        json!({
            "accountId": ACCOUNT_ID,
            "sinceQueryState": "0",
            "sort": [{ "property": "memberCount" }]
        }),
        &[],
    )
    .await
    .expect_err("unsupported sort");
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}

/// L1408 §Space/join: "If the invite has expired (`expiresAt` is in the
/// past) ..., the server MUST return an `invalidArguments` method error."
#[tokio::test]
async fn join_with_expired_invite_is_invalid_arguments() {
    let backend = IdentityBackend::new();
    seed_space(&backend, json!([]), json!([]));
    backend.inner().insert_object_for_test(
        "SpaceInvite",
        ACCOUNT_ID,
        "inv1",
        json!({
            "id": "inv1",
            "code": "0123456789abcdef0123456789abcdef",
            "spaceId": SPACE_ID,
            "createdBy": "admin-user",
            "uses": 0,
            "expiresAt": "2020-01-01T00:00:00Z",
            "createdAt": "2019-12-01T00:00:00Z",
        }),
    );
    let before = space_state(&backend).await;

    let err = handle_space_join(
        &backend,
        &caller(),
        json!({ "accountId": ACCOUNT_ID, "inviteCode": "0123456789abcdef0123456789abcdef" }),
    )
    .await
    .expect_err("expired invite");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
    assert_eq!(space_state(&backend).await, before, "no member added");
}
