//! AddressBook/* method handlers (RFC 9610 §2).
//!
//! Provides handlers for:
//! - `AddressBook/get`
//! - `AddressBook/changes`
//! - `AddressBook/set` (with `onDestroyRemoveContents` and `onSuccessSetIsDefault`)
//!
//! **No `AddressBook/query` or `AddressBook/queryChanges`** — the spec does
//! not define these methods for AddressBook.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3), with the
//! type-specific arguments defined by RFC 9610 §2. The returned `Value`
//! is the corresponding method-response object per the same section refs.
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `cannotCalculateChanges` — per RFC 8620 §3.6 and §5). Per-target
//! failures inside `/set` surface in the `notCreated` / `notUpdated` /
//! `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_contacts_types::{AddressBook, ContactCard, ContactCardFilterCondition};
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ContactsBackend, SetError, SetErrorType};
use crate::helpers::{
    enforce_max_objects_in_set, extract_account_id, finalize_set_response, set_error_value,
    SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

// ---------------------------------------------------------------------------
// AddressBook/get
// ---------------------------------------------------------------------------

/// Handle an `AddressBook/get` method call (RFC 9610 §2.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_address_book_get<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<AddressBook, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// AddressBook/changes
// ---------------------------------------------------------------------------

/// Handle an `AddressBook/changes` method call (RFC 9610 §2.2).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_address_book_changes<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<AddressBook, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// AddressBook/set
// ---------------------------------------------------------------------------

/// Handle an `AddressBook/set` method call (RFC 9610 §2.3).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps), augmented
/// with the RFC 9610 §2.3 `onDestroyRemoveContents` and
/// `onSuccessSetIsDefault` arguments; the returned `Value` is the §5.3
/// `/set` response shape (`accountId`, `oldState`, `newState`, plus the
/// per-operation `created` / `notCreated` / `updated` / `notUpdated` /
/// `destroyed` / `notDestroyed` maps).
///
/// Contacts-specific extensions:
///
/// - **`onDestroyRemoveContents`** (bool, default `false`): if `false` and
///   the address book still contains ContactCards, the destroy is rejected with
///   `SetError { type: "addressBookHasContents" }`. If `true`, the backend
///   must remove the contents itself.
///
/// - **`onSuccessSetIsDefault`** (`Id|null`, single string id or null): if
///   non-null, after all other set operations succeed the named address book
///   is patched with `{"isDefault": true}`.  Creation-id references (`#c1`)
///   are resolved to the server-assigned id.  Any error from this patch is
///   silently ignored per §2.3.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_address_book_set<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: accountId not recognised → accountNotFound (method-level
    // error). Without this, a /set against an unknown accountId would silently
    // "succeed" with a fake oldState/newState envelope.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.6). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    // Parse contacts-specific arguments before consuming args.
    let on_destroy_remove_contents = args
        .get("onDestroyRemoveContents")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // RFC 9610 §2.3: onSuccessSetIsDefault is Id|null (a single string id or
    // null). §2.3 says "If the id is not found or if the change is not
    // permitted by the server for policy reasons, it MUST be ignored" — that
    // covers id-validity / policy refusal, NOT argument-shape errors. A
    // non-string non-null value is an argument-shape error and is rejected
    // with `invalidArguments` per the same convention the /set destroy
    // non-string rejection uses (line ~322). Mirrors the canonical
    // jmap-mail-server onSuccessActivateScript / onSuccessDeactivateScript
    // pattern in sieve.rs:370-385 (bd:JMAP-qz9v.8).
    let on_success_set_is_default: Option<String> = match args.remove("onSuccessSetIsDefault") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "onSuccessSetIsDefault: expected a string id or null, got {v}"
            )));
        }
    };

    let old_state = backend
        .get_state::<AddressBook>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    // bd:JMAP-qz9v.57 — capture the set of currently-default AddressBook ids
    // before any mutations so we can detect demotions triggered by the
    // single-default invariant (bd:JMAP-qz9v.5 / bd:JMAP-qz9v.11). After the
    // create + update loops we re-fetch and any id whose isDefault flipped
    // from true → false is surfaced in the wire `updated` map per RFC 8620
    // §5.3 ('any properties of any other object that have been changed').
    // A best-effort error from this initial fetch is treated as "no
    // previously-default ids known" — the worst case is a demotion that
    // happened to a book we never saw, which is still picked up by
    // AddressBook/changes.
    let previously_default_ids: std::collections::HashSet<Id> = backend
        .get_objects::<AddressBook>(caller, &account_id, None, None)
        .await
        .map(|(books, _)| {
            books
                .into_iter()
                .filter(|b| b.is_default)
                .map(|b| b.id)
                .collect()
        })
        .unwrap_or_default();

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj_val) in create_map {
            // RFC 8620 §5.3: "The id property MUST NOT be set in the create
            // object" — id is server-assigned. Any present "id" key (even
            // null) is rejected with invalidProperties:["id"].
            if obj_val.get("id").is_some() {
                not_created.insert(
                    create_id,
                    json!({"type": "invalidProperties", "properties": ["id"]}),
                );
                continue;
            }
            let obj_with_id = match obj_val {
                Value::Object(mut m) => {
                    m.entry("id")
                        .or_insert_with(|| Value::String("placeholder".to_owned()));
                    // AddressBook requires myRights — inject a default if absent.
                    m.entry("myRights").or_insert_with(|| {
                        json!({
                            "mayRead": true,
                            "mayWrite": true,
                            "mayShare": false,
                            "mayDelete": false
                        })
                    });
                    Value::Object(m)
                }
                other => other,
            };

            let ab: AddressBook = match serde_json::from_value(obj_with_id) {
                Ok(a) => a,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<AddressBook>(caller, &account_id, &create_id, ab)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id,
                        serde_json::to_value(&created_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(create_id, server_fail_value_from_backend(&e));
                }
                // BackendSetError is #[non_exhaustive]; surface any future
                // variant's Debug repr instead of discarding it (bd:JMAP-qz9v.53).
                Err(other) => {
                    not_created.insert(
                        create_id,
                        json!({
                            "type": "serverFail",
                            "description":
                                format!("unhandled backend error variant: {other:?}"),
                        }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .update_object::<AddressBook>(caller, &account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str, Value::Null);
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(id_str, server_fail_value_from_backend(&e));
                }
                // BackendSetError is #[non_exhaustive]; surface any future
                // variant's Debug repr instead of discarding it (bd:JMAP-qz9v.53).
                Err(other) => {
                    not_updated.insert(
                        id_str,
                        json!({
                            "type": "serverFail",
                            "description":
                                format!("unhandled backend error variant: {other:?}"),
                        }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        if let Some(bad) = destroy_arr.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy: every element must be a string Id; got {bad}"
            )));
        }
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s.to_owned(),
                None => continue, // unreachable: validated above
            };
            let id = Id::from(id_str.as_str());

            // RFC 9610 §2.3 onDestroyRemoveContents:
            // - `true`: cascade — for every ContactCard belonging to this
            //   book, either destroy the card (if this is its only book)
            //   or patch its addressBookIds to remove this book entry. If
            //   any cascade step fails, reject the book destroy.
            // - `false` (default): if the book has contents, reject with
            //   addressBookHasContents; otherwise proceed.
            if on_destroy_remove_contents {
                if let Err(cascade_err) =
                    cascade_address_book_contents(backend, caller, &account_id, &id).await
                {
                    not_destroyed.insert(id_str, cascade_err);
                    continue;
                }
            } else {
                match backend
                    .address_book_has_contents(caller, &account_id, &id)
                    .await
                {
                    Ok(true) => {
                        not_destroyed.insert(
                            id_str,
                            set_error_value(&SetError::new(SetErrorType::Custom(
                                "addressBookHasContents".to_owned(),
                            ))),
                        );
                        continue;
                    }
                    Ok(false) => {
                        // Proceed to destroy_object below.
                    }
                    // Backend signalled it could not determine whether the
                    // AddressBook has contents (e.g. storage degraded).
                    // Per the trait contract, surface as serverFail rather
                    // than fail-open by proceeding with destroy.
                    Err(e) => {
                        not_destroyed.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            match backend
                .destroy_object::<AddressBook>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(id_str, server_fail_value_from_backend(&e));
                }
                // BackendSetError is #[non_exhaustive]; surface any future
                // variant's Debug repr instead of discarding it (bd:JMAP-qz9v.53).
                Err(other) => {
                    not_destroyed.insert(
                        id_str,
                        json!({
                            "type": "serverFail",
                            "description":
                                format!("unhandled backend error variant: {other:?}"),
                        }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // bd:JMAP-qz9v.57 — report books demoted by the single-default invariant.
    //
    // After the regular create + update + destroy loops, any AddressBook that
    // was isDefault:true at the start of /set (captured in
    // `previously_default_ids`) but is now isDefault:false was implicitly
    // demoted by the backend's enforcement of RFC 9610 §2's
    // 'MUST NOT be true for more than one AddressBook within an account'
    // invariant. RFC 8620 §5.3 requires every modified object to surface in
    // the response, so we re-fetch the previously-default ids and insert
    // any flipped-to-false entries into the wire `updated` map.
    //
    // Skip ids that already appear in `updated` (because they were the
    // explicit target of a successful update) or in `created` (because
    // they were just created); those entries are authoritative.
    //
    // A best-effort error from the re-fetch leaves the wire response as-is.
    // The /changes machinery will still surface the demotion.
    //
    // The onSuccessSetIsDefault block below has its own (legacy) demotion
    // reporting; this pass covers the regular paths only. Running this
    // BEFORE onSuccessSetIsDefault ensures no double-attribution: any
    // promotion onSuccessSetIsDefault triggers will be picked up by its
    // own existing block.
    // -----------------------------------------------------------------------
    if !previously_default_ids.is_empty() {
        if let Ok((current_books, _)) = backend
            .get_objects::<AddressBook>(caller, &account_id, None, None)
            .await
        {
            for book in current_books {
                let book_id_str = book.id.to_string();
                if !previously_default_ids.contains(&book.id) {
                    continue; // not previously default → not a demotion candidate
                }
                if book.is_default {
                    continue; // still default → not demoted
                }
                if updated.contains_key(&book_id_str) || created.contains_key(&book_id_str) {
                    continue; // already authoritatively recorded
                }
                if let Ok(v) = serde_json::to_value(&book) {
                    updated.insert(book_id_str, v);
                    mutated = true;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // onSuccessSetIsDefault — post-set isDefault patch
    // RFC 9610 §2.3: onSuccessSetIsDefault is Id|null (a single string id,
    // not a map).  After the main set operations, if it is a non-null string,
    // patch the named address book with {"isDefault": true}.  Errors are
    // silently ignored per §2.3.
    //
    // RFC 9610 §2.3: this block MUST be skipped if any main set operation
    // (create, update, or destroy) produced an error.
    // -----------------------------------------------------------------------
    let main_ops_all_succeeded =
        not_created.is_empty() && not_updated.is_empty() && not_destroyed.is_empty();
    if main_ops_all_succeeded {
        if let Some(id_str) = on_success_set_is_default {
            // Resolve creation reference: if id_str starts with '#', look up in created map.
            let resolved: Option<Id> = if let Some(create_id) = id_str.strip_prefix('#') {
                created
                    .get(create_id)
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .map(Id::from)
            } else {
                Some(Id::from(id_str.as_str()))
            };
            if let Some(target_id) = resolved {
                // Build a one-key PatchObject {"isDefault": true} via
                // the typed constructor. RFC 8620 §5.3.
                let mut patch_map = serde_json::Map::new();
                patch_map.insert("isDefault".to_owned(), Value::Bool(true));
                let patch = PatchObject::from_map(patch_map);
                // §2.3: errors here are silently ignored.
                match backend
                    .update_object::<AddressBook>(caller, &account_id, &target_id, patch)
                    .await
                {
                    Ok(Some(obj)) => {
                        mutated = true;
                        updated.insert(
                            target_id.to_string(),
                            serde_json::to_value(&obj).unwrap_or(Value::Null),
                        );
                        // RFC 8620 §5.3: all changed objects must appear in
                        // updated.  When isDefault transfers, the backend clears
                        // it on all other books; re-fetch to pick them up.
                        if let Ok((all_books, _)) = backend
                            .get_objects::<AddressBook>(caller, &account_id, None, None)
                            .await
                        {
                            for book in all_books {
                                let book_id = book.id.clone();
                                if book_id == target_id {
                                    continue; // already recorded above
                                }
                                // The backend enforces single-default: any book
                                // returned here whose is_default is now false was
                                // implicitly demoted.
                                if !book.is_default {
                                    if let Ok(v) = serde_json::to_value(&book) {
                                        updated.insert(book_id.to_string(), v);
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        mutated = true;
                        updated.insert(target_id.to_string(), Value::Null);
                    }
                    Err(_) => {} // silently ignored per §2.3
                }
            }
        }
    } // end if main_ops_all_succeeded

    // Fetch newState AFTER all mutations including onSuccessSetIsDefault
    // (RFC 8620 §5.3: newState must reflect every mutation in this call).
    finalize_set_response::<B, AddressBook>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        SetAccumulators {
            created,
            updated,
            destroyed: destroyed_list,
            not_created,
            not_updated,
            not_destroyed,
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// AddressBook/set destroy cascade (RFC 9610 §2.3 onDestroyRemoveContents)
// ---------------------------------------------------------------------------

/// Cascade-clean the ContactCards belonging to an AddressBook before
/// destroying the book itself (bd:JMAP-qz9v.1).
///
/// For every ContactCard whose `addressBookIds` set contains `book_id`:
/// - If `book_id` is the card's only entry, destroy the card.
/// - Otherwise, patch the card to remove the entry from
///   `addressBookIds` (RFC 7396 merge patch via the workspace's standard
///   `update_object` path).
///
/// Returns `Ok(())` if every affected card was successfully handled. On
/// any error, returns a `serverFail` SetError-shaped JSON value the
/// caller surfaces in the AddressBook/set `notDestroyed` map; the book
/// itself is then NOT destroyed.
///
/// Note: this is sequenced per-card and not transactional. A real
/// database-backed backend should override the workflow to perform the
/// cascade and the book destroy atomically.
async fn cascade_address_book_contents<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    book_id: &Id,
) -> Result<(), Value> {
    // Query for affected card ids using the inAddressBook filter
    // (RFC 9610 §3.3.1). ContactCardFilterCondition is #[non_exhaustive],
    // so build via default + field assignment.
    let mut filter = ContactCardFilterCondition::default();
    filter.in_address_book = Some(book_id.clone());
    let query_result = backend
        .query_objects::<ContactCard>(caller, account_id, Some(&filter), None, None, 0)
        .await
        .map_err(|e| {
            json!({
                "type": "serverFail",
                "description": format!("cascade query failed: {e}")
            })
        })?;

    if query_result.ids.is_empty() {
        return Ok(());
    }

    // Fetch full card records to inspect each card's addressBookIds set.
    let (cards, _not_found) = backend
        .get_objects::<ContactCard>(caller, account_id, Some(&query_result.ids), None)
        .await
        .map_err(|e| {
            json!({
                "type": "serverFail",
                "description": format!("cascade fetch failed: {e}")
            })
        })?;

    for card in cards {
        let Some(card_id) = card.id.as_ref() else {
            return Err(json!({
                "type": "serverFail",
                "description": "cascade encountered stored card with missing id"
            }));
        };

        let book_count = card.address_book_ids.as_ref().map(|m| m.len()).unwrap_or(0);

        if book_count <= 1 {
            // Card belongs only to this book (or has no books): destroy.
            backend
                .destroy_object::<ContactCard>(caller, account_id, card_id)
                .await
                .map_err(|e| {
                    json!({
                        "type": "serverFail",
                        "description":
                            format!("cascade destroy of card {card_id} failed: {e:?}")
                    })
                })?;
        } else {
            // Card is shared with other books: patch addressBookIds to
            // null out only this entry. RFC 7396 merge patch semantics
            // (the workspace /set update model): `{"addressBookIds":
            // {"<book>": null}}` removes the key from the nested map
            // while leaving sibling entries intact.
            let mut patch_map = serde_json::Map::new();
            patch_map.insert(
                "addressBookIds".to_owned(),
                json!({ book_id.as_ref(): Value::Null }),
            );
            backend
                .update_object::<ContactCard>(
                    caller,
                    account_id,
                    card_id,
                    PatchObject::from_map(patch_map),
                )
                .await
                .map_err(|e| {
                    json!({
                        "type": "serverFail",
                        "description":
                            format!("cascade update of card {card_id} failed: {e:?}")
                    })
                })?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: RFC 9610 §2.3 — destroy with non-empty address book and
    /// onDestroyRemoveContents=false (default) returns addressBookHasContents.
    ///
    /// The mock backend's `address_book_has_contents` returns true for
    /// `"ab-nonempty"`.
    #[tokio::test]
    async fn set_destroy_non_empty_returns_address_book_has_contents() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["ab-nonempty"]
        });
        let (resp, _) = handle_address_book_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["ab-nonempty"]["type"], "addressBookHasContents",
            "non-empty book with onDestroyRemoveContents=false must yield addressBookHasContents: {resp}"
        );
        assert!(
            resp["destroyed"].is_null(),
            "destroyed must be null when blocked: {resp}"
        );
    }

    /// Oracle: bd:JMAP-qz9v.27 — when `address_book_has_contents` returns
    /// `Err`, the destroy MUST be reported as `serverFail` rather than
    /// fail-open (proceeding) or fail-closed-as-`addressBookHasContents`
    /// (mis-attributing the error).
    ///
    /// Independent check: the mock backend is forced into the Err branch
    /// via `set_fail_has_contents(true)`; the handler is expected to map
    /// that to `serverFail` per the ContactsBackend trait contract.
    #[tokio::test]
    async fn set_destroy_when_has_contents_errs_returns_server_fail() {
        let backend = MockBackend::new_with_account("acc1");
        backend.set_fail_has_contents(true);
        let args = json!({
            "accountId": "acc1",
            "destroy": ["ab-anything"]
        });
        let (resp, _) = handle_address_book_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present when has_contents errs: {resp}"
        );
        assert_eq!(
            not_destroyed["ab-anything"]["type"], "serverFail",
            "Err from address_book_has_contents must surface as serverFail, not addressBookHasContents or silent destroy: {resp}"
        );
        assert!(
            resp["destroyed"].is_null(),
            "destroyed must be null when has_contents errs (fail-open is a bug): {resp}"
        );
    }

    /// Oracle: RFC 9610 §2.3 — destroy with `onDestroyRemoveContents=true`
    /// MUST NOT short-circuit with `addressBookHasContents`.
    ///
    /// **Scope:** this test exercises the gate logic only. The mock
    /// backend reports `address_book_has_contents` true for the
    /// `ab-nonempty` id and `destroy_object` returns `notFound` for
    /// every id — so the post-cascade book destroy ends up in
    /// `notDestroyed["ab-nonempty"]["type"]: "notFound"`. The
    /// assertion verifies the gate routes around the
    /// `addressBookHasContents` branch.
    ///
    /// The cascade itself (destroy exclusive cards, patch shared cards)
    /// requires a real backend and is verified in the integration tests
    /// `address_book_set_destroy_cascade_*` in
    /// `tests/contacts_tests.rs` under `MemoryBackend`. See
    /// bd:JMAP-qz9v.1 / bd:JMAP-qz9v.9 for the history.
    #[tokio::test]
    async fn set_destroy_with_on_destroy_remove_contents_true_skips_contents_check() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveContents": true,
            "destroy": ["ab-nonempty"]
        });
        let (resp, _) = handle_address_book_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // The mock backend returns notFound for destroys; verify we got past
        // the contents check (notDestroyed will have notFound, not addressBookHasContents).
        let not_destroyed = &resp["notDestroyed"];
        if not_destroyed.is_object() {
            assert_ne!(
                not_destroyed["ab-nonempty"]["type"], "addressBookHasContents",
                "contents check must be bypassed: {resp}"
            );
        }
    }

    /// Oracle: AddressBook/get with unknown account → accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let err = handle_address_book_get(&backend, &(), args)
            .await
            .expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: AddressBook/changes with known account returns valid response.
    #[tokio::test]
    async fn changes_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceState": "0" });
        let (resp, _) = handle_address_book_changes(&backend, &(), args)
            .await
            .expect("must not error for known account");
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: RFC 9610 §2.3 — onSuccessSetIsDefault with a bare string id
    /// triggers a best-effort isDefault patch.  The MockBackend's update_object
    /// returns NotFound (a SetError), which is silently swallowed per §2.3, so
    /// no top-level error is returned and notUpdated remains null.
    #[tokio::test]
    async fn set_on_success_set_is_default_bare_id() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": [],
            "onSuccessSetIsDefault": "book1"
        });
        let (resp, _) = handle_address_book_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // The update error is silently swallowed — no top-level error and
        // notUpdated must be null (the error is not surfaced per §2.3).
        assert!(
            resp.get("type").is_none(),
            "must not be a top-level error: {resp}"
        );
        assert!(
            resp["notUpdated"].is_null(),
            "§2.3 errors must be silently ignored, notUpdated must be null: {resp}"
        );
    }

    /// Oracle: RFC 9610 §2.3 — onSuccessSetIsDefault: null is a no-op.
    /// No error must be returned and the response must be structurally valid.
    #[tokio::test]
    async fn set_on_success_set_is_default_null_ignored() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "onSuccessSetIsDefault": null
        });
        let (resp, _) = handle_address_book_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        assert!(
            resp.get("type").is_none(),
            "null onSuccessSetIsDefault must not cause an error: {resp}"
        );
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: RFC 9610 §2.3 distinguishes argument-shape errors from
    /// id-validity / policy-refusal errors. §2.3's "If the id is not found
    /// or if the change is not permitted by the server for policy reasons,
    /// it MUST be ignored" covers a *valid Id string* that the server
    /// chooses not to act on — NOT a wire payload where
    /// `onSuccessSetIsDefault` has the wrong JSON type. A non-string
    /// non-null value is an argument-shape error and is rejected with
    /// `invalidArguments`, matching the convention the /set destroy
    /// non-string rejection uses (addressbook.rs:322-326) and the
    /// canonical jmap-mail-server pattern in sieve.rs:370-385
    /// (bd:JMAP-qz9v.8).
    #[tokio::test]
    async fn set_on_success_set_is_default_bad_type_rejected_with_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "onSuccessSetIsDefault": {"wrong": "type"}
        });
        let err = handle_address_book_set(&backend, &(), args)
            .await
            .expect_err("malformed onSuccessSetIsDefault must be rejected, not silently dropped");
        assert_eq!(
            err.error_type.as_str(),
            "invalidArguments",
            "non-string non-null onSuccessSetIsDefault must surface as invalidArguments (argument-shape error, not §2.3 silent-ignore): {err:?}"
        );
    }

    /// Oracle: companion to the object-type test above — number, boolean,
    /// and array values for onSuccessSetIsDefault are also argument-shape
    /// errors and must produce `invalidArguments`. Exercises the same
    /// rejection arm with each of the remaining non-string-non-null JSON
    /// types the bead enumerated (bd:JMAP-qz9v.8).
    #[tokio::test]
    async fn set_on_success_set_is_default_other_bad_types_rejected() {
        let backend = MockBackend::new_with_account("acc1");
        for bad_value in [json!(42), json!(true), json!(["book1"])] {
            let args = json!({
                "accountId": "acc1",
                "onSuccessSetIsDefault": bad_value,
            });
            let err = handle_address_book_set(&backend, &(), args)
                .await
                .expect_err("non-string-non-null onSuccessSetIsDefault must be rejected");
            assert_eq!(
                err.error_type.as_str(),
                "invalidArguments",
                "onSuccessSetIsDefault: {bad_value} must surface as invalidArguments: {err:?}",
            );
        }
    }

    /// Oracle: RFC 8620 §5.3 — onSuccessSetIsDefault must report the DEMOTED
    /// book (previously isDefault=true) in `updated` in addition to the newly
    /// promoted book.
    ///
    /// Pre-conditions: account has two address books; book1 is the default.
    /// Action: onSuccessSetIsDefault="book2".
    /// Expected: updated contains BOTH book1 (isDefault:false) AND book2 (isDefault:true).
    #[tokio::test]
    async fn set_on_success_reports_demoted_book_in_updated() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.seed_addressbook("acc1", "book1", true);
        backend.seed_addressbook("acc1", "book2", false);

        let args = json!({
            "accountId": "acc1",
            "onSuccessSetIsDefault": "book2"
        });
        let (resp, _) = handle_address_book_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        let updated = resp["updated"]
            .as_object()
            .expect("updated must be an object: {resp}");

        // The newly-promoted book must appear.
        assert!(
            updated.contains_key("book2"),
            "book2 (newly default) must appear in updated: {resp}"
        );
        assert_eq!(
            updated["book2"]["isDefault"],
            serde_json::json!(true),
            "book2 must have isDefault:true: {resp}"
        );

        // The demoted book must also appear (RFC 8620 §5.3).
        assert!(
            updated.contains_key("book1"),
            "book1 (demoted, was isDefault=true) must appear in updated: {resp}"
        );
        assert_eq!(
            updated["book1"]["isDefault"],
            serde_json::json!(false),
            "book1 must have isDefault:false after demotion: {resp}"
        );
    }

    /// Oracle: RFC 9610 §2.3 — onSuccessSetIsDefault MUST be skipped when
    /// any main set operation (create, update, or destroy) produced an error.
    /// Here a create with a malformed entry causes notCreated, which means
    /// onSuccessSetIsDefault must NOT be applied.
    #[tokio::test]
    async fn set_on_success_skipped_when_create_fails() {
        let backend = MockBackend::new_with_account("acc1");
        // An empty object {} as the create value has no required "name" field,
        // so deserialization will fail → notCreated entry → main op failed.
        let args = json!({
            "accountId": "acc1",
            "create": { "c1": {} },   // missing required name → invalidProperties
            "onSuccessSetIsDefault": "book1"
        });
        let (resp, _) = handle_address_book_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // The create must have failed.
        assert!(
            resp["notCreated"].is_object(),
            "c1 must be in notCreated: {resp}"
        );
        // onSuccessSetIsDefault must NOT have run (updated must be null/absent).
        // If it did run, "book1" would appear in updated.
        let updated = &resp["updated"];
        let is_empty =
            updated.is_null() || updated.as_object().map(|o| o.is_empty()).unwrap_or(true);
        assert!(
            is_empty,
            "updated must be empty — onSuccessSetIsDefault must not run when create failed: {resp}"
        );
    }
}
