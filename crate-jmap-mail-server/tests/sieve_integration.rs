//! Sieve integration tests for jmap-mail-server (RFC 9661).
//!
//! All tests in this file are compiled and run only when `--features sieve` is passed.
//! Test vectors come from RFC 9661 §2.3–§2.6 examples and
//! RFC 5228 §8 (valid Sieve syntax).
#![cfg(feature = "sieve")]
#![allow(async_fn_in_trait)]

mod common;

use common::{
    FAULTY_BACKEND_CANARY, FaultyBackend, INVALID_SIEVE_SCRIPT, MemoryBackend, VALID_SIEVE_SCRIPT,
};
use jmap_mail_server::{
    handle_sieve_get, handle_sieve_query, handle_sieve_set, handle_sieve_validate,
};
use jmap_server::HandlerOutput;
use jmap_types::Id;

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

/// Register a fresh account in the backend.
///
/// Each test calls this on its own `MemoryBackend::new()` — no shared state.
async fn setup_account(backend: &MemoryBackend) -> Id {
    let account_id = Id::from("sieve-test-account");
    backend.register_account(&account_id);
    account_id
}

/// Store a blob and return its Id.
fn store_valid_blob(backend: &MemoryBackend) -> Id {
    let blob_id = Id::from("valid-script-blob");
    backend.store_blob(&blob_id, VALID_SIEVE_SCRIPT.to_vec());
    blob_id
}

fn store_invalid_blob(backend: &MemoryBackend) -> Id {
    let blob_id = Id::from("invalid-script-blob");
    backend.store_blob(&blob_id, INVALID_SIEVE_SCRIPT.to_vec());
    blob_id
}

// ---------------------------------------------------------------------------
// SieveScript/get tests
// ---------------------------------------------------------------------------

/// Test 1: SieveScript/get on empty account.
///
/// Oracle: RFC 9661 §2.3 — when no scripts exist, list is
/// empty and notFound is [].
#[tokio::test]
async fn sieve_get_empty_account() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref()
    });

    let HandlerOutput {
        primary: resp,
        extra_invocations: extra,
        ..
    } = handle_sieve_get(&backend, &(), args)
        .await
        .expect("sieve_get_empty_account: must succeed");

    assert_eq!(
        resp["list"],
        serde_json::json!([]),
        "list must be empty on fresh account; resp: {resp}"
    );
    assert_eq!(
        resp["notFound"],
        serde_json::json!([]),
        "notFound must be [] (not null) on fresh account; resp: {resp}"
    );
    assert!(extra.is_empty(), "get must produce no extra invocations");
}

/// Test 9: SieveScript/get by id — happy path.
///
/// Oracle: RFC 9661 §2.3 — get by ids returns matching entry.
#[tokio::test]
async fn sieve_get_by_id() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create a script to get an assigned id.
    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "get-by-id-script", "blobId": "valid-script-blob" }
        }
    });
    let HandlerOutput {
        primary: create_resp,
        ..
    } = handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("setup create must succeed");

    let assigned_id = create_resp["created"]["A"]["id"]
        .as_str()
        .expect("created[A][id] must be a string")
        .to_owned();

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [assigned_id]
    });

    let HandlerOutput {
        primary: resp,
        extra_invocations: extra,
        ..
    } = handle_sieve_get(&backend, &(), get_args)
        .await
        .expect("sieve_get_by_id: must succeed");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(
        list.len(),
        1,
        "list must have exactly one entry; resp: {resp}"
    );
    assert_eq!(
        list[0]["id"].as_str(),
        Some(assigned_id.as_str()),
        "list[0].id must match requested id; resp: {resp}"
    );
    assert_eq!(
        resp["notFound"],
        serde_json::json!([]),
        "notFound must be [] when id is found; resp: {resp}"
    );
    assert!(extra.is_empty(), "get must produce no extra invocations");
}

/// Test 10: SieveScript/get with a non-existent id.
///
/// Oracle: RFC 9661 §2.3 / RFC 8620 §5.1 — unknown ids appear
/// in notFound; list is empty.
#[tokio::test]
async fn sieve_get_not_found() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": ["nonexistent-id"]
    });

    let HandlerOutput {
        primary: resp,
        extra_invocations: extra,
        ..
    } = handle_sieve_get(&backend, &(), args)
        .await
        .expect("sieve_get_not_found: must succeed");

    assert_eq!(
        resp["list"],
        serde_json::json!([]),
        "list must be empty for unknown id; resp: {resp}"
    );
    let not_found = resp["notFound"]
        .as_array()
        .expect("notFound must be an array");
    assert!(
        not_found
            .iter()
            .any(|v| v.as_str() == Some("nonexistent-id")),
        "notFound must contain nonexistent-id; resp: {resp}"
    );
    assert!(extra.is_empty(), "get must produce no extra invocations");
}

/// Test 14: SieveScript/get with unknown account.
///
/// Oracle: RFC 8620 §3.6.2 — unknown accountId → accountNotFound.
#[tokio::test]
async fn sieve_get_unknown_account() {
    let backend = MemoryBackend::new();

    let args = serde_json::json!({
        "accountId": "no-such-account"
    });

    let err = handle_sieve_get(&backend, &(), args)
        .await
        .expect_err("unknown accountId must return Err");

    assert_eq!(
        err.error_type.to_string(),
        "accountNotFound",
        "unknown accountId must produce accountNotFound; got: {:?}",
        err.error_type
    );
}

// ---------------------------------------------------------------------------
// SieveScript/set tests
// ---------------------------------------------------------------------------

/// Test 2: SieveScript/set create basic — creates a script, not active by default.
///
/// Oracle: RFC 9661 §2.4 — created script has isActive: false
/// unless onSuccessActivateScript is set.
#[tokio::test]
async fn sieve_set_create_basic() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "my-script", "blobId": "valid-script-blob" }
        }
    });

    let HandlerOutput {
        primary: resp,
        extra_invocations: extra,
        ..
    } = handle_sieve_set(&backend, &(), args)
        .await
        .expect("sieve_set_create_basic: must succeed");

    assert_eq!(
        resp["created"]["A"]["name"].as_str(),
        Some("my-script"),
        "created[A].name must match; resp: {resp}"
    );
    assert_eq!(
        resp["created"]["A"]["isActive"],
        serde_json::json!(false),
        "created[A].isActive must be false on plain create; resp: {resp}"
    );
    assert!(
        resp["notCreated"].is_null(),
        "notCreated must be null when create succeeds; resp: {resp}"
    );
    assert!(extra.is_empty(), "set must produce no extra invocations");
}

/// Test 3: SieveScript/set create + onSuccessActivateScript.
///
/// Oracle: RFC 9661 §2.4 — after create with activation,
/// the script's isActive becomes true (visible via updated map or re-fetch).
#[tokio::test]
async fn sieve_set_create_and_activate() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "active-script", "blobId": "valid-script-blob" }
        },
        "onSuccessActivateScript": "#A"
    });

    let HandlerOutput {
        primary: resp,
        extra_invocations: extra,
        ..
    } = handle_sieve_set(&backend, &(), args)
        .await
        .expect("sieve_set_create_and_activate: must succeed");

    // The script was created (create_object stores isActive=false, then activation
    // patches isActive:true into the created entry per spec §2.4 — activated-on-create
    // scripts appear in `created` with isActive:true, NOT in `updated`).
    assert!(
        !resp["created"]["A"].is_null(),
        "created[A] must be present; resp: {resp}"
    );
    assert!(
        resp["notCreated"].is_null(),
        "notCreated must be null; resp: {resp}"
    );

    // Per spec §2.4: the isActive:true for an activated-on-create script appears in
    // the `created` entry, not in `updated`.
    assert_eq!(
        resp["created"]["A"]["isActive"],
        serde_json::json!(true),
        "created[A].isActive must be true after on-create activation; resp: {resp}"
    );

    // The script must NOT appear in `updated` — it is new to the client.
    let assigned_id = resp["created"]["A"]["id"]
        .as_str()
        .expect("created[A][id] must be a string")
        .to_owned();
    assert!(
        resp["updated"][assigned_id.as_str()].is_null(),
        "updated must not contain the newly-created+activated script; resp: {resp}"
    );
    assert!(extra.is_empty(), "set must produce no extra invocations");
}

/// Test 4: At most one active script — activating a new script deactivates the old one.
///
/// Oracle: RFC 9661 §2.4 — the server MUST deactivate any
/// currently active script when onSuccessActivateScript targets a different script.
#[tokio::test]
async fn sieve_set_at_most_one_active() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create and activate S1.
    let create_s1 = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "S1": { "name": "script-one", "blobId": "valid-script-blob" }
        },
        "onSuccessActivateScript": "#S1"
    });
    let HandlerOutput { primary: resp1, .. } = handle_sieve_set(&backend, &(), create_s1)
        .await
        .expect("create S1 must succeed");
    let s1_id = resp1["created"]["S1"]["id"]
        .as_str()
        .expect("S1 id required")
        .to_owned();

    // Verify S1 is active after the first set — per spec §2.4, isActive:true
    // for an activated-on-create script appears in `created`, not `updated`.
    assert_eq!(
        resp1["created"]["S1"]["isActive"],
        serde_json::json!(true),
        "S1 must be active (isActive:true in created entry) after first create+activate; resp: {resp1}"
    );

    // Create S2 and activate it (should deactivate S1).
    let create_s2 = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "S2": { "name": "script-two", "blobId": "valid-script-blob" }
        },
        "onSuccessActivateScript": "#S2"
    });
    let HandlerOutput { primary: resp2, .. } = handle_sieve_set(&backend, &(), create_s2)
        .await
        .expect("create S2 must succeed");

    // S1 must appear in updated with isActive: false.
    assert_eq!(
        resp2["updated"][s1_id.as_str()]["isActive"],
        serde_json::json!(false),
        "S1 must be deactivated when S2 is activated; resp: {resp2}"
    );

    // S2 was created and activated in the same set call — per spec §2.4,
    // isActive:true appears in `created["S2"]`, not in `updated`.
    assert_eq!(
        resp2["created"]["S2"]["isActive"],
        serde_json::json!(true),
        "S2 must be active (isActive:true in created entry) after activation; resp: {resp2}"
    );
    assert!(
        resp2["notCreated"].is_null(),
        "notCreated must be null; resp: {resp2}"
    );
}

/// Test 5: Destroying an active script is rejected with sieveIsActive.
///
/// Oracle: RFC 9661 §2.4 — the server MUST NOT destroy an
/// active script; returns sieveIsActive SetError.
#[tokio::test]
async fn sieve_set_destroy_active_rejected() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create and activate.
    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "active-to-destroy", "blobId": "valid-script-blob" }
        },
        "onSuccessActivateScript": "#A"
    });
    let HandlerOutput {
        primary: create_resp,
        ..
    } = handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("create must succeed");
    let script_id = create_resp["created"]["A"]["id"]
        .as_str()
        .expect("id required")
        .to_owned();

    // Attempt to destroy the active script.
    let destroy_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "destroy": [script_id]
    });
    let HandlerOutput { primary: resp, .. } = handle_sieve_set(&backend, &(), destroy_args)
        .await
        .expect("set must return Ok even when destroy is rejected");

    assert_eq!(
        resp["notDestroyed"][script_id.as_str()]["type"].as_str(),
        Some("sieveIsActive"),
        "notDestroyed[id].type must be sieveIsActive; resp: {resp}"
    );
    assert!(
        resp["destroyed"].is_null(),
        "destroyed must be null when destroy is rejected; resp: {resp}"
    );
}

/// Test 6: Duplicate name is rejected with alreadyExists.
///
/// Oracle: RFC 9661 §2.4 — creating a script whose name
/// already exists returns alreadyExists with existingId.
#[tokio::test]
async fn sieve_set_duplicate_name() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create first script with name "dup-name".
    let first = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "dup-name", "blobId": "valid-script-blob" }
        }
    });
    handle_sieve_set(&backend, &(), first)
        .await
        .expect("first create must succeed");

    // Attempt to create second script with same name.
    let second = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "B": { "name": "dup-name", "blobId": "valid-script-blob" }
        }
    });
    let HandlerOutput { primary: resp, .. } = handle_sieve_set(&backend, &(), second)
        .await
        .expect("set must return Ok even when create fails");

    assert_eq!(
        resp["notCreated"]["B"]["type"].as_str(),
        Some("alreadyExists"),
        "notCreated[B].type must be alreadyExists; resp: {resp}"
    );
    // existingId must be a non-empty string (the id of the first script).
    let existing_id = resp["notCreated"]["B"]["existingId"]
        .as_str()
        .expect("existingId must be a string");
    assert!(
        !existing_id.is_empty(),
        "existingId must be non-empty; resp: {resp}"
    );
}

/// Test 7: onSuccessDeactivateScript with no create/update/destroy.
///
/// Oracle: RFC 9661 §2.4 — onSuccessDeactivateScript: true
/// deactivates the currently active script.
#[tokio::test]
async fn sieve_set_deactivate_only() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create and activate.
    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "to-deactivate", "blobId": "valid-script-blob" }
        },
        "onSuccessActivateScript": "#A"
    });
    let HandlerOutput {
        primary: create_resp,
        ..
    } = handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("create must succeed");
    let script_id = create_resp["created"]["A"]["id"]
        .as_str()
        .expect("id required")
        .to_owned();

    // Deactivate only.
    let deactivate_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "onSuccessDeactivateScript": true
    });
    let HandlerOutput { primary: resp, .. } = handle_sieve_set(&backend, &(), deactivate_args)
        .await
        .expect("deactivate must succeed");

    assert_eq!(
        resp["updated"][script_id.as_str()]["isActive"],
        serde_json::json!(false),
        "updated[id].isActive must be false after deactivate; resp: {resp}"
    );
}

/// Test 8: Re-activate a previously deactivated script.
///
/// Oracle: RFC 9661 §2.4 — onSuccessActivateScript with a
/// bare id (not a #creation-id reference) activates an existing script.
#[tokio::test]
async fn sieve_set_reactivate() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create and activate.
    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "reactivate-me", "blobId": "valid-script-blob" }
        },
        "onSuccessActivateScript": "#A"
    });
    let HandlerOutput {
        primary: create_resp,
        ..
    } = handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("create must succeed");
    let s1_id = create_resp["created"]["A"]["id"]
        .as_str()
        .expect("id required")
        .to_owned();

    // Deactivate.
    let deactivate_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "onSuccessDeactivateScript": true
    });
    handle_sieve_set(&backend, &(), deactivate_args)
        .await
        .expect("deactivate must succeed");

    // Re-activate using the bare id.
    let reactivate_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "onSuccessActivateScript": s1_id
    });
    let HandlerOutput { primary: resp, .. } = handle_sieve_set(&backend, &(), reactivate_args)
        .await
        .expect("reactivate must succeed");

    assert_eq!(
        resp["updated"][s1_id.as_str()]["isActive"],
        serde_json::json!(true),
        "updated[id].isActive must be true after reactivate; resp: {resp}"
    );
}

/// Test 15: SieveScript/set with unknown account returns accountNotFound.
///
/// Oracle: RFC 8620 §3.6.2.
#[tokio::test]
async fn sieve_set_unknown_account() {
    let backend = MemoryBackend::new();

    let args = serde_json::json!({
        "accountId": "no-such-account"
    });

    let err = handle_sieve_set(&backend, &(), args)
        .await
        .expect_err("unknown accountId must return Err");

    assert_eq!(
        err.error_type.to_string(),
        "accountNotFound",
        "unknown accountId must produce accountNotFound; got: {:?}",
        err.error_type
    );
}

// ---------------------------------------------------------------------------
// SieveScript/query tests
// ---------------------------------------------------------------------------

/// Test 11: SieveScript/query with isActive filter.
///
/// Oracle: RFC 9661 §4.2 — filter.isActive filters to only
/// active (or inactive) scripts.
#[tokio::test]
async fn sieve_query_filter_active() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create two scripts, activate only the first.
    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "S1": { "name": "active-script", "blobId": "valid-script-blob" },
            "S2": { "name": "inactive-script", "blobId": "valid-script-blob" }
        },
        "onSuccessActivateScript": "#S1"
    });
    handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("create must succeed");

    // Query for isActive: true.
    let query_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": { "isActive": true }
    });
    let HandlerOutput {
        primary: resp,
        extra_invocations: extra,
        ..
    } = handle_sieve_query(&backend, &(), query_args)
        .await
        .expect("sieve_query_filter_active: must succeed");

    let ids = resp["ids"].as_array().expect("ids must be an array");
    assert_eq!(
        ids.len(),
        1,
        "exactly one active script expected; resp: {resp}"
    );
    assert!(extra.is_empty(), "query must produce no extra invocations");
}

/// SieveScript/query evaluates AND/OR/NOT filter trees.
///
/// Oracle: RFC 8620 §5.5 — `filter` is "FilterOperator|FilterCondition|null";
/// "AND: All of the conditions must match", "OR: At least one of the
/// conditions must match", "NOT: None of the conditions must match".
/// Expected ids are worked out by hand from the three scripts below
/// (`alpha` active; `beta`, `gamma` inactive), in the handler's
/// name-ascending order.
#[tokio::test]
async fn sieve_query_filter_operator_trees() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "alpha", "blobId": "valid-script-blob" },
            "B": { "name": "beta", "blobId": "valid-script-blob" },
            "G": { "name": "gamma", "blobId": "valid-script-blob" }
        },
        "onSuccessActivateScript": "#A"
    });
    let created = handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("create must succeed")
        .primary;
    let id = |k: &str| {
        created["created"][k]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("created.{k}.id missing: {created}"))
            .to_owned()
    };
    let (a, b, g) = (id("A"), id("B"), id("G"));

    let cases = [
        (
            serde_json::json!({ "operator": "OR", "conditions": [
                { "name": "alpha" }, { "name": "gamma" }
            ]}),
            vec![a.clone(), g.clone()],
        ),
        (
            serde_json::json!({ "operator": "NOT", "conditions": [
                { "isActive": true }
            ]}),
            vec![b.clone(), g.clone()],
        ),
        (
            serde_json::json!({ "operator": "AND", "conditions": [
                { "name": "a" },
                { "operator": "NOT", "conditions": [{ "name": "alpha" }] }
            ]}),
            vec![b.clone(), g.clone()],
        ),
        (
            serde_json::json!({ "operator": "AND", "conditions": [
                { "isActive": true }, { "name": "beta" }
            ]}),
            vec![],
        ),
    ];
    for (filter, expected) in cases {
        let query_args = serde_json::json!({
            "accountId": account_id.as_ref(),
            "filter": filter
        });
        let resp = handle_sieve_query(&backend, &(), query_args)
            .await
            .unwrap_or_else(|e| panic!("filter {filter} must be accepted: {e:?}"))
            .primary;
        let ids: Vec<&str> = resp["ids"]
            .as_array()
            .expect("ids must be an array")
            .iter()
            .map(|v| v.as_str().expect("id must be a string"))
            .collect();
        assert_eq!(ids, expected, "filter {filter}; resp: {resp}");
    }
}

// ---------------------------------------------------------------------------
// SieveScript/validate tests
// ---------------------------------------------------------------------------

/// Test 12: SieveScript/validate with a valid script.
///
/// Oracle: RFC 9661 §2.6 — error field MUST be null when
/// the script is valid. VALID_SIEVE_SCRIPT is `b"keep;"` from RFC 5228 §8.
#[tokio::test]
async fn sieve_validate_valid() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobId": "valid-script-blob"
    });

    let HandlerOutput {
        primary: resp,
        extra_invocations: extra,
        ..
    } = handle_sieve_validate(&backend, &(), args)
        .await
        .expect("sieve_validate_valid: must succeed");

    assert!(
        resp["error"].is_null(),
        "error must be null for a valid script; resp: {resp}"
    );
    assert!(
        extra.is_empty(),
        "validate must produce no extra invocations"
    );
}

/// Test 13: SieveScript/validate with an invalid (empty) script.
///
/// Oracle: RFC 9661 §2.6 — error field MUST be present as
/// an object with type "invalidSieve" when the script fails validation.
/// INVALID_SIEVE_SCRIPT is `b""` — empty bytes fail the MemoryBackend validator.
#[tokio::test]
async fn sieve_validate_invalid() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_invalid_blob(&backend);

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobId": "invalid-script-blob"
    });

    let HandlerOutput {
        primary: resp,
        extra_invocations: extra,
        ..
    } = handle_sieve_validate(&backend, &(), args)
        .await
        .expect("sieve_validate_invalid: must return Ok (not Err)");

    assert_eq!(
        resp["error"]["type"].as_str(),
        Some("invalidSieve"),
        "error.type must be invalidSieve for an invalid script; resp: {resp}"
    );
    assert!(
        extra.is_empty(),
        "validate must produce no extra invocations"
    );
}

/// Test 17: Default VacationResponse protection — no script protected.
///
/// `MemoryBackend::vacation_response_script_id` returns `Ok(None)` (the
/// default impl), so no script is protected and a normal destroy succeeds.
///
/// This test verifies the **absence** of a spurious `forbidden` error when
/// the backend does not designate any script as VR-backed (RFC 9661 §4).
#[tokio::test]
async fn sieve_set_vr_script_destroy_no_protection_by_default() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create a script.
    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "vr-test", "blobId": "valid-script-blob" }
        }
    });
    let HandlerOutput {
        primary: create_resp,
        ..
    } = handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("create must succeed");
    let script_id = create_resp["created"]["A"]["id"]
        .as_str()
        .expect("created[A][id] must be a string")
        .to_owned();

    // Destroy the script. MemoryBackend returns no VR script id, so this
    // MUST succeed — no forbidden error should be produced.
    let destroy_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "destroy": [script_id.clone()]
    });
    let HandlerOutput {
        primary: destroy_resp,
        ..
    } = handle_sieve_set(&backend, &(), destroy_args)
        .await
        .expect("destroy must succeed");

    let destroyed = destroy_resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array when no VR protection is active");
    assert!(
        destroyed
            .iter()
            .any(|v| v.as_str() == Some(script_id.as_str())),
        "script_id must appear in destroyed; resp: {destroy_resp}"
    );
    assert_eq!(
        destroy_resp["notDestroyed"],
        serde_json::Value::Null,
        "notDestroyed must be null; resp: {destroy_resp}"
    );
}

/// Test 18: Default VacationResponse protection — blobId update allowed.
///
/// When no VR-backed script is designated (`vacation_response_script_id` returns
/// `Ok(None)`), updating the `blobId` of any script MUST succeed
/// (RFC 9661 §4 guard does not fire).
#[tokio::test]
async fn sieve_set_vr_script_blob_update_no_protection_by_default() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create a second valid blob to update to.
    let new_blob_id = jmap_types::Id::from("second-valid-blob");
    backend.store_blob(&new_blob_id, b"discard;".to_vec());

    // Create a script.
    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "vr-blob-test", "blobId": "valid-script-blob" }
        }
    });
    let HandlerOutput {
        primary: create_resp,
        ..
    } = handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("create must succeed");
    let script_id = create_resp["created"]["A"]["id"]
        .as_str()
        .expect("created[A][id] must be a string")
        .to_owned();

    // Update blobId. Must succeed because MemoryBackend has no VR script.
    let update_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            script_id.clone(): { "blobId": "second-valid-blob" }
        }
    });
    let HandlerOutput {
        primary: update_resp,
        ..
    } = handle_sieve_set(&backend, &(), update_args)
        .await
        .expect("update must succeed");

    assert!(
        !update_resp["updated"][script_id.as_str()].is_null()
            || update_resp["updated"][script_id.as_str()] == serde_json::Value::Null,
        "update must not produce a forbidden error; resp: {update_resp}"
    );
    assert_eq!(
        update_resp["notUpdated"],
        serde_json::Value::Null,
        "notUpdated must be null; resp: {update_resp}"
    );
}

/// Test 19: onSuccessActivateScript suppressed when any create/update/destroy fails.
///
/// Oracle: RFC 9661 §2.4 — activation side-effects only run if
/// ALL operations succeed. A partial failure (B fails with alreadyExists) must
/// suppress onSuccessActivateScript even when another create (C) succeeds.
///
/// B fails → `any_failure` is true → Step 7 activation state machine is skipped
/// entirely → C is created but NOT activated.
#[tokio::test]
async fn sieve_set_on_success_suppressed_on_partial_failure() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    // Create first script (will succeed); establishes "existing-script" name in DB.
    let HandlerOutput {
        primary: create_resp,
        ..
    } = handle_sieve_set(
        &backend,
        &(),
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {"A": {"name": "existing-script", "blobId": "valid-script-blob"}}
        }),
    )
    .await
    .expect("first create must succeed");
    assert!(
        !create_resp["created"]["A"].is_null(),
        "A must be created; resp: {create_resp}"
    );

    // Now: create two scripts where B has duplicate name (will fail) and C is valid
    // (will succeed); onSuccessActivateScript points to "#C".
    // Because B fails, the activation state machine must NOT fire.
    let HandlerOutput { primary: resp, .. } = handle_sieve_set(
        &backend,
        &(),
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "B": {"name": "existing-script", "blobId": "valid-script-blob"},
                "C": {"name": "new-script", "blobId": "valid-script-blob"}
            },
            "onSuccessActivateScript": "#C"
        }),
    )
    .await
    .expect("set must return Ok even with partial failure");

    // B must have failed with alreadyExists.
    assert_eq!(
        resp["notCreated"]["B"]["type"].as_str(),
        Some("alreadyExists"),
        "B must fail with alreadyExists; resp: {resp}"
    );

    // C must have been created but must NOT be active (activation was suppressed).
    assert!(
        !resp["created"]["C"].is_null(),
        "C must be created; resp: {resp}"
    );
    assert_eq!(
        resp["created"]["C"]["isActive"],
        serde_json::json!(false),
        "C must NOT be active because B failed (activation suppressed); resp: {resp}"
    );

    // There must be no entries in updated (no activation = no deactivate-old-active
    // and no activate-new-target were applied).
    assert!(
        resp["updated"].is_null(),
        "updated must be null — no activation fired; resp: {resp}"
    );
}

/// Test 16: SieveScript/validate with unknown account returns accountNotFound.
///
/// Oracle: RFC 8620 §3.6.2.
#[tokio::test]
async fn sieve_validate_unknown_account() {
    let backend = MemoryBackend::new();

    let args = serde_json::json!({
        "accountId": "no-such-account",
        "blobId": "any-blob"
    });

    let err = handle_sieve_validate(&backend, &(), args)
        .await
        .expect_err("unknown accountId must return Err");

    assert_eq!(
        err.error_type.to_string(),
        "accountNotFound",
        "unknown accountId must produce accountNotFound; got: {:?}",
        err.error_type
    );
}

/// Test 17: SieveScript/set create rejects script exceeding maxSizeScript with tooLarge.
///
/// Oracle: RFC 9661 §2.4 — "If the SieveScript cannot be created
/// or updated because its size exceeds the maxSizeScript limit, the server MUST
/// reject the request with a tooLarge SetError."
///
/// The valid script is 5 bytes ("keep;"). We set the limit to 4 bytes, so the
/// create must fail with tooLarge.
#[tokio::test]
async fn sieve_set_create_too_large() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;

    // VALID_SIEVE_SCRIPT is b"keep;" — 5 bytes. Limit is 4 bytes → too large.
    let blob_id = store_valid_blob(&backend);
    assert_eq!(
        VALID_SIEVE_SCRIPT.len(),
        5,
        "test oracle: VALID_SIEVE_SCRIPT must be 5 bytes"
    );
    backend.set_max_sieve_script_bytes(4);

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "C1": { "name": "too-big", "blobId": blob_id.as_ref() }
        }
    });

    let HandlerOutput { primary: resp, .. } = handle_sieve_set(&backend, &(), args)
        .await
        .expect("handle_sieve_set must not return a method-level error");

    assert!(
        resp["created"].is_null(),
        "created must be null when script is too large; resp: {resp}"
    );
    let error_type = resp["notCreated"]["C1"]["type"]
        .as_str()
        .unwrap_or("(missing)");
    assert_eq!(
        error_type.to_string(),
        "tooLarge",
        "expected tooLarge SetError for oversized create; resp: {resp}"
    );
}

/// Test 18: SieveScript/set update rejects blobId patch exceeding maxSizeScript with tooLarge.
///
/// Oracle: RFC 9661 §2.4 — same tooLarge rule applies to updates.
#[tokio::test]
async fn sieve_set_update_too_large() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;

    // First, create a script with no size limit so it succeeds.
    let blob_id = store_valid_blob(&backend);
    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "C1": { "name": "update-size-test", "blobId": blob_id.as_ref() }
        }
    });
    let HandlerOutput {
        primary: create_resp,
        ..
    } = handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("initial create must succeed");
    let assigned_id = create_resp["created"]["C1"]["id"]
        .as_str()
        .expect("created[C1][id] must be a string")
        .to_owned();

    // Now enforce a size limit that the existing blob exceeds.
    assert_eq!(
        VALID_SIEVE_SCRIPT.len(),
        5,
        "test oracle: VALID_SIEVE_SCRIPT must be 5 bytes"
    );
    backend.set_max_sieve_script_bytes(4);

    // Store a new (also oversized) blob and try to update blobId.
    let new_blob_id = Id::from("new-oversized-blob");
    backend.store_blob(&new_blob_id, b"keep;".to_vec()); // still 5 bytes

    let update_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            assigned_id.clone(): { "blobId": new_blob_id.as_ref() }
        }
    });

    let HandlerOutput { primary: resp, .. } = handle_sieve_set(&backend, &(), update_args)
        .await
        .expect("handle_sieve_set must not return a method-level error");

    assert!(
        resp["updated"].is_null(),
        "updated must be null when new blob is too large; resp: {resp}"
    );
    let error_type = resp["notUpdated"][&assigned_id]["type"]
        .as_str()
        .unwrap_or("(missing)");
    assert_eq!(
        error_type.to_string(),
        "tooLarge",
        "expected tooLarge SetError for oversized update; resp: {resp}"
    );
}

/// SieveScript/set rejects the (N+1)th create with overQuota once the
/// per-account script-count cap is reached.
///
/// Oracle: RFC 8620 §5.3 — overQuota is the spec-defined SetError type for
/// creates that would exceed a backend-enforced quota. The cap value itself
/// is workspace-policy backend-configurable (workspace AGENTS.md "Backend
/// caps and limits"), exposed via the
/// [`SieveBackend::max_sieve_scripts_per_account`] trait method.
///
/// Regression: bd:JMAP-q2wa.14. The cap used to be a private constant
/// `MAX_SIEVE_SCRIPTS = 100` enforced on every backend with no override
/// path — this test demonstrates that a backend can now configure a lower
/// cap and the handler honours it.
#[tokio::test]
async fn sieve_set_create_overquota_respects_backend_cap() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);
    // Cap at 1 — the second create in the same request must overQuota.
    backend.set_max_sieve_scripts_per_account(1);

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "a": { "name": "first", "blobId": "valid-script-blob" },
            "b": { "name": "second", "blobId": "valid-script-blob" }
        }
    });

    let HandlerOutput { primary: resp, .. } = handle_sieve_set(&backend, &(), args)
        .await
        .expect("handle_sieve_set must not return a method-level error");

    // Exactly one of {a, b} must be created (BTreeMap iteration order is
    // alphabetical, so 'a' wins, but we don't depend on that).
    let created = resp.get("created").and_then(|v| v.as_object());
    let not_created = resp.get("notCreated").and_then(|v| v.as_object());

    let created_count = created.map(|m| m.len()).unwrap_or(0);
    let not_created_count = not_created.map(|m| m.len()).unwrap_or(0);

    assert_eq!(
        created_count, 1,
        "exactly one create must succeed under cap=1; resp: {resp}"
    );
    assert_eq!(
        not_created_count, 1,
        "exactly one create must be rejected under cap=1; resp: {resp}"
    );

    // The rejected entry must be an overQuota SetError.
    let rejected_entry = not_created
        .and_then(|m| m.values().next())
        .expect("notCreated must have one entry");
    assert_eq!(
        rejected_entry["type"].as_str(),
        Some("overQuota"),
        "rejected create must be overQuota; got: {rejected_entry}"
    );
}

/// Test 19: SieveScript/validate rejects blob exceeding maxSizeScript with tooLarge.
///
/// Oracle: RFC 9661 §2.6 — the validate method MUST observe `maxSizeScript`,
/// matching the consistency requirement that the handler not delegate blobs
/// exceeding the advertised limit to the backend parser.
///
/// Witness discriminator: VALID_SIEVE_SCRIPT (5 bytes) is a *valid* script, so
/// if the size-check were skipped the response would be `error: null`. Observing
/// `error.type == "tooLarge"` proves the size guard short-circuited before
/// `validate_sieve_script` was invoked.
#[tokio::test]
async fn sieve_validate_too_large() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;

    let blob_id = store_valid_blob(&backend);
    assert_eq!(
        VALID_SIEVE_SCRIPT.len(),
        5,
        "test oracle: VALID_SIEVE_SCRIPT must be 5 bytes"
    );
    backend.set_max_sieve_script_bytes(4);

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobId": blob_id.as_ref(),
    });

    let HandlerOutput {
        primary: resp,
        extra_invocations: extra,
        ..
    } = handle_sieve_validate(&backend, &(), args)
        .await
        .expect("handle_sieve_validate must not return a method-level error");

    assert_eq!(
        resp["error"]["type"].as_str(),
        Some("tooLarge"),
        "expected tooLarge error for oversized validate; resp: {resp}"
    );
    assert!(
        extra.is_empty(),
        "validate must produce no extra invocations"
    );
}

/// SieveScript/set update with `blobId: null`.
///
/// Oracle: RFC 8620 §5.3 — "invalidProperties": "The record given is
/// invalid in some way. For example: It contains properties that are
/// invalid according to the type specification of this record type."
/// RFC 9661 §2.1 types `*blobId*:  Id` with no default.
///
/// The update must be rejected, and SieveScript/get must still return the
/// stored blobId.
#[tokio::test]
async fn sieve_set_update_null_blob_id_is_invalid_properties() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    let HandlerOutput {
        primary: create_resp,
        ..
    } = handle_sieve_set(
        &backend,
        &(),
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": { "A": { "name": "my-script", "blobId": "valid-script-blob" } }
        }),
    )
    .await
    .expect("SieveScript/set create");
    let id = create_resp["created"]["A"]["id"]
        .as_str()
        .expect("A must have id")
        .to_owned();

    let HandlerOutput {
        primary: upd_resp, ..
    } = handle_sieve_set(
        &backend,
        &(),
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "update": { &id: { "blobId": null } }
        }),
    )
    .await
    .expect("SieveScript/set update must not return a protocol error");
    assert_eq!(
        upd_resp["notUpdated"][&id]["type"], "invalidProperties",
        "{upd_resp}"
    );

    let HandlerOutput {
        primary: get_resp, ..
    } = handle_sieve_get(
        &backend,
        &(),
        serde_json::json!({ "accountId": account_id.as_ref(), "ids": [&id] }),
    )
    .await
    .expect("SieveScript/get must succeed after rejected update");
    assert_eq!(
        get_resp["list"][0]["blobId"], "valid-script-blob",
        "{get_resp}"
    );
}

// ---------------------------------------------------------------------------
// Single blob fetch (bd JMAP-j7pa.5)
// ---------------------------------------------------------------------------

/// Create, update and validate each read the script blob exactly once,
/// even with `maxSizeScript` configured (size check plus syntax check).
///
/// Oracle: the counting `FaultyBackend` wrapper counts real
/// `get_sieve_blob` calls; the expected count of one per operation is the
/// JMAP-j7pa.5 requirement, not derived from the handler.
#[tokio::test]
async fn sieve_set_and_validate_fetch_blob_once() {
    let backend = FaultyBackend::new();
    let account_id = Id::from("sieve-test-account");
    backend.inner.register_account(&account_id);
    backend.inner.set_max_sieve_script_bytes(1024);
    backend
        .inner
        .store_blob(&Id::from("blob-a"), VALID_SIEVE_SCRIPT.to_vec());
    backend
        .inner
        .store_blob(&Id::from("blob-b"), b"discard;".to_vec());

    let HandlerOutput { primary: resp, .. } = handle_sieve_set(
        &backend,
        &(),
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": { "A": { "name": "one", "blobId": "blob-a" } }
        }),
    )
    .await
    .expect("create must not be a method-level error");
    let id = resp["created"]["A"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("create must succeed; resp: {resp}"))
        .to_owned();
    assert_eq!(backend.sieve_blob_fetches(), 1, "create fetches once");

    let HandlerOutput { primary: resp, .. } = handle_sieve_set(
        &backend,
        &(),
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "update": { id.clone(): { "blobId": "blob-b" } }
        }),
    )
    .await
    .expect("update must not be a method-level error");
    assert!(
        resp["updated"].get(&id).is_some(),
        "update must succeed; resp: {resp}"
    );
    assert_eq!(backend.sieve_blob_fetches(), 2, "update fetches once");

    let HandlerOutput { primary: resp, .. } = handle_sieve_validate(
        &backend,
        &(),
        serde_json::json!({ "accountId": account_id.as_ref(), "blobId": "blob-b" }),
    )
    .await
    .expect("validate must not be a method-level error");
    assert_eq!(resp["error"], serde_json::Value::Null, "resp: {resp}");
    assert_eq!(backend.sieve_blob_fetches(), 3, "validate fetches once");
}

/// A storage failure reading one create's script blob fails only that
/// create, as a redacted `serverFail`; the other create in the same call
/// still succeeds.
///
/// Oracle: RFC 8620 §5.3 — per-object failures go in `notCreated` with a
/// SetError; `serverFail` is the RFC 8620 §3.6.1 type for an unexpected
/// server error. Two creates and a fire-once fault make the result
/// independent of create order.
#[tokio::test]
async fn sieve_set_create_blob_fetch_failure_is_per_item() {
    let backend = FaultyBackend::new();
    let account_id = Id::from("sieve-test-account");
    backend.inner.register_account(&account_id);
    backend
        .inner
        .store_blob(&Id::from("blob-a"), VALID_SIEVE_SCRIPT.to_vec());
    backend.inject("SieveScript", "get_sieve_blob");

    let HandlerOutput { primary: resp, .. } = handle_sieve_set(
        &backend,
        &(),
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "A": { "name": "one", "blobId": "blob-a" },
                "B": { "name": "two", "blobId": "blob-a" }
            }
        }),
    )
    .await
    .expect("a blob fetch failure must not be a method-level error");

    let created = resp["created"].as_object().expect("created map");
    let not_created = resp["notCreated"].as_object().expect("notCreated map");
    assert_eq!(created.len(), 1, "one create must succeed; resp: {resp}");
    assert_eq!(not_created.len(), 1, "one create must fail; resp: {resp}");
    let err = not_created.values().next().expect("one entry");
    assert_eq!(err["type"], "serverFail", "resp: {resp}");
    assert!(
        !resp.to_string().contains(FAULTY_BACKEND_CANARY),
        "backend error text must not reach the wire; resp: {resp}"
    );
}

/// SieveScript/query rejects a sort property it does not support.
///
/// Oracle: RFC 8620 §5.5 — "unsupportedSort: The "sort" is syntactically
/// valid, but it includes a property the server does not support sorting
/// on"; RFC 9661 §2.5 lists only `name` and `isActive` for sorting.
#[tokio::test]
async fn sieve_query_unknown_sort_property_returns_unsupported_sort() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;

    let query_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sort": [{ "property": "blobId" }]
    });
    let err = handle_sieve_query(&backend, &(), query_args)
        .await
        .expect_err("sort by blobId must be rejected");
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}

/// SieveScript/query honours `sort` on `name` and `isActive`, including
/// `isAscending: false` and multi-key ordering.
///
/// Oracle: RFC 9661 §2.5 — "The following SieveScript properties MUST be
/// supported for sorting: name, isActive"; RFC 8620 §5.5 — "If true, sort
/// in ascending order. If false, reverse the comparator's results to sort
/// in descending order", and later comparators break ties. Expected ids
/// are worked out by hand from `alpha` (active), `beta`, `gamma`.
#[tokio::test]
async fn sieve_query_sort_by_name_and_is_active() {
    let backend = MemoryBackend::new();
    let account_id = setup_account(&backend).await;
    store_valid_blob(&backend);

    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "A": { "name": "alpha", "blobId": "valid-script-blob" },
            "B": { "name": "beta", "blobId": "valid-script-blob" },
            "G": { "name": "gamma", "blobId": "valid-script-blob" }
        },
        "onSuccessActivateScript": "#A"
    });
    let created = handle_sieve_set(&backend, &(), create_args)
        .await
        .expect("create must succeed")
        .primary;
    let id = |k: &str| {
        created["created"][k]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("created.{k}.id missing: {created}"))
            .to_owned()
    };
    let (a, b, g) = (id("A"), id("B"), id("G"));

    let cases = [
        (
            serde_json::json!([{ "property": "name", "isAscending": false }]),
            vec![g.clone(), b.clone(), a.clone()],
        ),
        (
            serde_json::json!([{ "property": "isActive" }, { "property": "name" }]),
            vec![b.clone(), g.clone(), a.clone()],
        ),
        (
            serde_json::json!([
                { "property": "isActive", "isAscending": false },
                { "property": "name", "isAscending": false }
            ]),
            vec![a.clone(), g.clone(), b.clone()],
        ),
    ];
    for (sort, expected) in cases {
        let query_args = serde_json::json!({
            "accountId": account_id.as_ref(),
            "sort": sort
        });
        let resp = handle_sieve_query(&backend, &(), query_args)
            .await
            .unwrap_or_else(|e| panic!("sort {sort} must be accepted: {e:?}"))
            .primary;
        let ids: Vec<&str> = resp["ids"]
            .as_array()
            .expect("ids must be an array")
            .iter()
            .map(|v| v.as_str().expect("id must be a string"))
            .collect();
        assert_eq!(ids, expected, "sort {sort}; resp: {resp}");
    }
}
