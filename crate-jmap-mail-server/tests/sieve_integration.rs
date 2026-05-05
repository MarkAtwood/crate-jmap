//! Sieve integration tests for jmap-mail-server (RFC 9661).
//!
//! All tests in this file are compiled and run only when `--features sieve` is passed.
//! Test vectors come from RFC 9661 §2.3–§2.6 examples and
//! RFC 5228 §8 (valid Sieve syntax).
#![cfg(feature = "sieve")]
#![allow(async_fn_in_trait)]

mod common;

use common::{MemoryBackend, INVALID_SIEVE_SCRIPT, VALID_SIEVE_SCRIPT};
use jmap_mail_server::{
    handle_sieve_get, handle_sieve_query, handle_sieve_set, handle_sieve_validate,
};
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

    let (resp, extra) = handle_sieve_get(&backend, args)
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
    let (create_resp, _) = handle_sieve_set(&backend, create_args)
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

    let (resp, extra) = handle_sieve_get(&backend, get_args)
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

    let (resp, extra) = handle_sieve_get(&backend, args)
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

    let err = handle_sieve_get(&backend, args)
        .await
        .expect_err("unknown accountId must return Err");

    assert_eq!(
        err.error_type.as_str(),
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

    let (resp, extra) = handle_sieve_set(&backend, args)
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

    let (resp, extra) = handle_sieve_set(&backend, args)
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
    let (resp1, _) = handle_sieve_set(&backend, create_s1)
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
    let (resp2, _) = handle_sieve_set(&backend, create_s2)
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
    let (create_resp, _) = handle_sieve_set(&backend, create_args)
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
    let (resp, _) = handle_sieve_set(&backend, destroy_args)
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
    handle_sieve_set(&backend, first)
        .await
        .expect("first create must succeed");

    // Attempt to create second script with same name.
    let second = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "B": { "name": "dup-name", "blobId": "valid-script-blob" }
        }
    });
    let (resp, _) = handle_sieve_set(&backend, second)
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
    let (create_resp, _) = handle_sieve_set(&backend, create_args)
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
    let (resp, _) = handle_sieve_set(&backend, deactivate_args)
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
    let (create_resp, _) = handle_sieve_set(&backend, create_args)
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
    handle_sieve_set(&backend, deactivate_args)
        .await
        .expect("deactivate must succeed");

    // Re-activate using the bare id.
    let reactivate_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "onSuccessActivateScript": s1_id
    });
    let (resp, _) = handle_sieve_set(&backend, reactivate_args)
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

    let err = handle_sieve_set(&backend, args)
        .await
        .expect_err("unknown accountId must return Err");

    assert_eq!(
        err.error_type.as_str(),
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
    handle_sieve_set(&backend, create_args)
        .await
        .expect("create must succeed");

    // Query for isActive: true.
    let query_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": { "isActive": true }
    });
    let (resp, extra) = handle_sieve_query(&backend, query_args)
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

    let (resp, extra) = handle_sieve_validate(&backend, args)
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

    let (resp, extra) = handle_sieve_validate(&backend, args)
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
    let (create_resp, _) = handle_sieve_set(&backend, create_args)
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
    let (destroy_resp, _) = handle_sieve_set(&backend, destroy_args)
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
    let (create_resp, _) = handle_sieve_set(&backend, create_args)
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
    let (update_resp, _) = handle_sieve_set(&backend, update_args)
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
    let (create_resp, _) = handle_sieve_set(
        &backend,
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
    let (resp, _) = handle_sieve_set(
        &backend,
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

    let err = handle_sieve_validate(&backend, args)
        .await
        .expect_err("unknown accountId must return Err");

    assert_eq!(
        err.error_type.as_str(),
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

    let (resp, _) = handle_sieve_set(&backend, args)
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
        error_type, "tooLarge",
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
    let (create_resp, _) = handle_sieve_set(&backend, create_args)
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

    let (resp, _) = handle_sieve_set(&backend, update_args)
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
        error_type, "tooLarge",
        "expected tooLarge SetError for oversized update; resp: {resp}"
    );
}
