//! Integration tests for the `Space/get` non-member field-projection
//! trim (bd:JMAP-v9py.4).
//!
//! Spec source: draft-atwood-jmap-chat-00 §Space/get (lines 1104-1111
//! after bd:JMAP-v9py.19's spec edit landed). Three rules:
//!
//! 1. Member caller → full Space object, subject to the standard
//!    JMAP `/get` `properties` filter if present.
//! 2. Non-member caller AND `isPubliclyPreviewable: true` → restricted
//!    view containing only `id`, `name`, `description`, `iconBlobId`,
//!    `memberCount`, `createdAt`, `isPublic`, `isPubliclyPreviewable`.
//!    All other fields MUST be omitted even when the caller asks for
//!    them via `properties`. The Space id MUST be in `list`, NOT in
//!    `notFound`.
//! 3. Non-member caller AND `isPubliclyPreviewable: false` → id in
//!    `notFound`, Space NOT in `list`. This is split to bd:JMAP-v9py.20
//!    (Layer A here only covers rule 2). The current Layer-A code
//!    leaves rule 3 as a known gap with a TODO marker; the test
//!    here pins the gap explicitly so a future regression is loud.

mod common;

use common::{IdentityBackend, MemoryBackend};
use jmap_chat_server::handle_space_get;
use jmap_types::Id;
use serde_json::json;

const ACCOUNT_ID: &str = "a1";
const SPACE_ID: &str = "s1";

/// Seed a fully-populated Space. `isPubliclyPreviewable` is
/// controlled by `previewable`; everything else is identical.
/// Members include an `admin` plus an additional `member-user`
/// (so a member caller can be exercised without being the
/// admin).
fn seed_space(backend: &IdentityBackend, previewable: bool) {
    let space_val = json!({
        "id": SPACE_ID,
        "name": "Project Atlas",
        "description": "The Atlas server",
        "iconBlobId": "blob-icon-001",
        "createdAt": "2026-01-01T00:00:00Z",
        "memberCount": 2,
        "categories": [
            {
                "id": "cat-a",
                "name": "Voice",
                "position": 0,
                "channelIds": ["ch-1"]
            }
        ],
        "uncategorizedChannelIds": ["ch-2"],
        "isPublic": true,
        "isPubliclyPreviewable": previewable,
        "roles": [
            {
                "id": "r-admin",
                "name": "Admin",
                "permissions": ["manage_space"],
                "position": 100
            }
        ],
        "members": [
            { "id": "admin",       "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "member-user", "roleIds": [],          "joinedAt": "2026-01-02T00:00:00Z" }
        ]
    });
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    backend
        .inner()
        .insert_object_for_test("Space", ACCOUNT_ID, SPACE_ID, space_val);
}

// ---------------------------------------------------------------------------
// Rule 1 — member caller gets full Space
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-v9py.4 acceptance criterion 1.
/// A member of the Space sees every field. Regression guard.
#[tokio::test]
async fn member_caller_sees_full_space() {
    let backend = IdentityBackend::new();
    seed_space(&backend, /* previewable = */ true);
    let caller = Id::from("member-user");

    let (resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("handle_space_get");

    assert!(
        resp["notFound"]
            .as_array()
            .map(Vec::is_empty)
            .unwrap_or(false),
        "notFound must be empty for member caller: {resp:?}"
    );
    let s = &resp["list"][0];
    // All 8 allowed fields are present.
    for f in [
        "id",
        "name",
        "description",
        "iconBlobId",
        "memberCount",
        "createdAt",
        "isPublic",
        "isPubliclyPreviewable",
    ] {
        assert!(!s[f].is_null(), "field {f} must be present: {s:?}");
    }
    // Non-restricted-view fields ARE present for a member.
    assert!(!s["roles"].is_null(), "roles must be present for member");
    assert!(
        !s["members"].is_null(),
        "members must be present for member"
    );
    assert!(
        !s["categories"].is_null(),
        "categories must be present for member"
    );
    assert!(
        !s["uncategorizedChannelIds"].is_null(),
        "uncategorizedChannelIds must be present for member"
    );
}

/// Oracle: bd:JMAP-v9py.4 acceptance criterion 1, properties branch.
/// A member caller with `properties: ["id", "name"]` gets only those
/// two fields — the standard RFC 8620 `/get` semantics.
#[tokio::test]
async fn member_caller_honors_properties_filter() {
    let backend = IdentityBackend::new();
    seed_space(&backend, /* previewable = */ true);
    let caller = Id::from("member-user");

    let (resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "ids": [SPACE_ID],
            "properties": ["id", "name"]
        }),
    )
    .await
    .expect("handle_space_get");

    let s = resp["list"][0].as_object().expect("space obj");
    // Only the requested fields should be present.
    let keys: Vec<&str> = s.keys().map(String::as_str).collect();
    assert_eq!(
        keys.len(),
        2,
        "exactly 2 fields expected for properties=[id,name]: {keys:?}"
    );
    assert!(keys.contains(&"id"), "id must be present: {keys:?}");
    assert!(keys.contains(&"name"), "name must be present: {keys:?}");
    assert!(
        !keys.contains(&"description"),
        "description must NOT be present: {keys:?}"
    );
    assert!(
        !keys.contains(&"roles"),
        "roles must NOT be present: {keys:?}"
    );
}

// ---------------------------------------------------------------------------
// Rule 2 — non-member caller, previewable Space, restricted view
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-v9py.4 acceptance criterion 2.
/// A non-member caller of a Space with `isPubliclyPreviewable: true`
/// gets only the 8 allowed fields. All other fields MUST be omitted.
/// The Space id MUST appear in `list`, MUST NOT appear in `notFound`.
#[tokio::test]
async fn non_member_caller_sees_restricted_view_on_previewable_space() {
    let backend = IdentityBackend::new();
    seed_space(&backend, /* previewable = */ true);
    let outsider = Id::from("outsider");

    let (resp, _) = handle_space_get(
        &backend,
        &outsider,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("handle_space_get");

    // Id MUST be in list, MUST NOT be in notFound.
    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1, "list must have 1 entry: {resp:?}");
    let not_found = resp["notFound"].as_array().expect("notFound");
    assert!(
        not_found.is_empty(),
        "notFound must be empty for previewable non-member: {resp:?}"
    );

    let s = list[0].as_object().expect("space obj");
    let keys: Vec<&str> = s.keys().map(String::as_str).collect();

    // The 8 allowed fields must all be present.
    let allowed = [
        "id",
        "name",
        "description",
        "iconBlobId",
        "memberCount",
        "createdAt",
        "isPublic",
        "isPubliclyPreviewable",
    ];
    for f in allowed {
        assert!(
            keys.contains(&f),
            "allowed field {f} must be present: {keys:?}"
        );
    }
    // The 4 non-allowed fields must be ABSENT.
    for f in ["roles", "members", "categories", "uncategorizedChannelIds"] {
        assert!(
            !keys.contains(&f),
            "non-allowed field {f} must be absent: {keys:?}"
        );
    }
    assert_eq!(
        keys.len(),
        allowed.len(),
        "exactly 8 fields expected (the allowed set): {keys:?}"
    );
}

/// Oracle: bd:JMAP-v9py.4 acceptance criterion 2, properties branch.
/// A non-member caller of a previewable Space who asks for
/// `properties: ["id", "roles"]` gets `id` (in the allowed set) and
/// NOT `roles` (outside the allowed set) — the projection is a hard
/// cap, not a min.
#[tokio::test]
async fn non_member_properties_filter_intersected_with_allowed_cap() {
    let backend = IdentityBackend::new();
    seed_space(&backend, /* previewable = */ true);
    let outsider = Id::from("outsider");

    let (resp, _) = handle_space_get(
        &backend,
        &outsider,
        json!({
            "accountId": ACCOUNT_ID,
            "ids": [SPACE_ID],
            "properties": ["id", "roles"]
        }),
    )
    .await
    .expect("handle_space_get");

    let s = resp["list"][0].as_object().expect("space obj");
    let keys: Vec<&str> = s.keys().map(String::as_str).collect();

    assert!(
        keys.contains(&"id"),
        "id must be present (allowed & requested): {keys:?}"
    );
    assert!(
        !keys.contains(&"roles"),
        "roles must be absent (requested but not allowed): {keys:?}"
    );
    // Nothing else slipped through — only `id` matches both filters.
    assert_eq!(
        keys.len(),
        1,
        "exactly 1 field expected (id is the only allowed ∩ requested): {keys:?}"
    );
}

/// Oracle: bd:JMAP-v9py.4 acceptance criterion 2, all-allowed-properties
/// branch. A non-member caller of a previewable Space who explicitly
/// asks for all 8 allowed fields gets all 8. Sanity check that the
/// intersection passes through the full cap when the request matches.
#[tokio::test]
async fn non_member_properties_filter_full_allowed_set() {
    let backend = IdentityBackend::new();
    seed_space(&backend, /* previewable = */ true);
    let outsider = Id::from("outsider");

    let (resp, _) = handle_space_get(
        &backend,
        &outsider,
        json!({
            "accountId": ACCOUNT_ID,
            "ids": [SPACE_ID],
            "properties": [
                "id", "name", "description", "iconBlobId",
                "memberCount", "createdAt", "isPublic", "isPubliclyPreviewable"
            ]
        }),
    )
    .await
    .expect("handle_space_get");

    let s = resp["list"][0].as_object().expect("space obj");
    let keys: Vec<&str> = s.keys().map(String::as_str).collect();
    assert_eq!(keys.len(), 8, "all 8 allowed fields expected: {keys:?}");
}

// ---------------------------------------------------------------------------
// Rule 3 — non-member non-previewable case classified as notFound
// (bd:JMAP-v9py.20)
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-v9py.20. A non-member caller of a Space with
/// `isPubliclyPreviewable: false` MUST receive the Space id back in
/// the `notFound` array, NOT in `list`. The non-previewable
/// existence of the Space MUST NOT leak in any field.
#[tokio::test]
async fn non_member_non_previewable_classified_as_not_found() {
    let backend = IdentityBackend::new();
    seed_space(&backend, /* previewable = */ false);
    let outsider = Id::from("outsider");

    let (resp, _) = handle_space_get(
        &backend,
        &outsider,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("handle_space_get");

    assert!(
        resp["list"].as_array().map(Vec::is_empty).unwrap_or(false),
        "list must be empty for non-member of non-previewable Space: {resp:?}"
    );
    let not_found: Vec<&str> = resp["notFound"]
        .as_array()
        .expect("notFound array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        not_found,
        &[SPACE_ID],
        "notFound must contain exactly the requested id: {resp:?}"
    );
}

/// Oracle: bd:JMAP-v9py.20 — indistinguishability. A non-member
/// caller's request for a non-existent Space and for a
/// non-previewable Space MUST produce identical response shapes.
/// This prevents existence-probing.
#[tokio::test]
async fn non_member_cannot_distinguish_non_previewable_from_nonexistent() {
    let backend = IdentityBackend::new();
    seed_space(&backend, /* previewable = */ false);
    let outsider = Id::from("outsider");

    // Request the seeded non-previewable Space.
    let (resp_priv, _) = handle_space_get(
        &backend,
        &outsider,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("handle_space_get private");

    // Request a Space that does not exist.
    let (resp_missing, _) = handle_space_get(
        &backend,
        &outsider,
        json!({ "accountId": ACCOUNT_ID, "ids": ["does-not-exist"] }),
    )
    .await
    .expect("handle_space_get nonexistent");

    // Both responses must have empty `list` and a single-entry
    // `notFound` carrying the requested id. The only difference is
    // the id string itself — by construction.
    let list_priv = resp_priv["list"].as_array().expect("list");
    let list_missing = resp_missing["list"].as_array().expect("list");
    assert!(list_priv.is_empty() && list_missing.is_empty());

    let nf_priv: Vec<&str> = resp_priv["notFound"]
        .as_array()
        .expect("nf")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let nf_missing: Vec<&str> = resp_missing["notFound"]
        .as_array()
        .expect("nf")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(nf_priv, &[SPACE_ID]);
    assert_eq!(nf_missing, &["does-not-exist"]);

    // No other top-level key should differ — `state` MAY differ if
    // the test seeded an account state token (it does), so we just
    // assert the `list` and `notFound` shapes are matched.
}

/// Oracle: bd:JMAP-v9py.20. The notFound classification does NOT
/// fire for member callers of a non-previewable Space — they see
/// the full object. Regression guard for the member-vs-non-member
/// branch.
#[tokio::test]
async fn member_caller_unaffected_by_non_previewable_classification() {
    let backend = IdentityBackend::new();
    seed_space(&backend, /* previewable = */ false);
    let caller = Id::from("member-user");

    let (resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("handle_space_get");

    assert_eq!(
        resp["list"].as_array().map(Vec::len).unwrap_or(0),
        1,
        "member must see the Space in list: {resp:?}"
    );
    let not_found = resp["notFound"].as_array().expect("notFound");
    assert!(
        not_found.is_empty(),
        "member must not see notFound entry: {resp:?}"
    );
    let s = resp["list"][0].as_object().expect("space obj");
    assert!(
        s.contains_key("roles"),
        "member sees full Space even when non-previewable: {s:?}"
    );
}

/// Oracle: bd:JMAP-v9py.20. The notFound classification does NOT
/// fire for non-member callers of a publicly-previewable Space —
/// they get the 8-field restricted view. Regression guard for the
/// previewable-vs-non-previewable branch (bd:JMAP-v9py.4 path
/// remains intact).
#[tokio::test]
async fn non_member_previewable_unaffected_by_classification() {
    let backend = IdentityBackend::new();
    seed_space(&backend, /* previewable = */ true);
    let outsider = Id::from("outsider");

    let (resp, _) = handle_space_get(
        &backend,
        &outsider,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("handle_space_get");

    assert_eq!(
        resp["list"].as_array().map(Vec::len).unwrap_or(0),
        1,
        "non-member sees previewable Space in list: {resp:?}"
    );
    let not_found = resp["notFound"].as_array().expect("notFound");
    assert!(
        not_found.is_empty(),
        "non-member of previewable Space sees no notFound entry: {resp:?}"
    );
    let s = resp["list"][0].as_object().expect("space obj");
    assert!(
        !s.contains_key("roles"),
        "non-member of previewable Space still gets the restricted view: {s:?}"
    );
}

// ---------------------------------------------------------------------------
// Single-user mode regression bound
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-v9py.4 + workspace AGENTS.md "Caller identity
/// (foundation seam)". When `principal_id` returns `None`
/// (single-user mode — the reference `MemoryBackend` with
/// `CallerCtx = ()` does this), every caller is treated as if they
/// were a member. The restricted-view trim must NOT fire. This is
/// the regression bound for the kit's no-identity posture.
#[tokio::test]
async fn single_user_mode_sees_full_space() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Single User Space",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 0,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [{"id":"r","name":"R","permissions":[],"position":1}],
            "members": []
        }),
    );

    let (resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("handle_space_get");

    let s = resp["list"][0].as_object().expect("space obj");
    assert!(
        s.contains_key("roles"),
        "single-user mode must see full Space: {s:?}"
    );
}
