//! Integration tests for `*/queryChanges` handlers and the `accountIds` filter
//! (RFC 8620 §5.6, RFC 9670 §2.4.1).
//!
//! Tests 1–2 verify that an unparsable `sinceQueryState` produces a
//! `cannotCalculateChanges` error for both Principal and ShareNotification.
//!
//! Test 3 verifies that the dispatcher correctly passes the `accountIds` filter
//! to the backend and that only matching Principals are returned.

mod common;

use std::sync::Arc;

use jmap_server::{Dispatcher, JmapRequest, State};
use jmap_sharing_server::{
    register_sharing_handlers, BackendChangesError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetObject,
    SharingBackend,
};
use jmap_sharing_types::{Principal, PrincipalFilterCondition, ShareNotification};
use jmap_types::Id;
use serde_json::json;

use common::MemoryBackend;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn single_call(method: &str, args: serde_json::Value, call_id: &str) -> JmapRequest {
    JmapRequest::new(
        vec!["urn:ietf:params:jmap:principals".into()],
        vec![(method.into(), args, call_id.into())],
        None,
    )
}

/// Seed a Principal into `backend` for `account_id` and return the server id.
async fn seed_principal(backend: &MemoryBackend, account_id: &str, p: serde_json::Value) -> Id {
    let mut with_id = p;
    with_id["id"] = json!("placeholder");
    let principal: Principal =
        serde_json::from_value(with_id).expect("test fixture must deserialize");
    let (server_id, _) = backend
        .create_object::<Principal>(&Id::from(account_id), "seed", principal)
        .await
        .expect("seed must succeed");
    server_id
}

/// Seed a ShareNotification into `backend` for `account_id` and return the server id.
async fn seed_notification(backend: &MemoryBackend, account_id: &str, v: serde_json::Value) -> Id {
    let notif: ShareNotification =
        serde_json::from_value(v).expect("test fixture must deserialize");
    let (server_id, _) = backend
        .create_object::<ShareNotification>(&Id::from(account_id), "seed", notif)
        .await
        .expect("seed must succeed");
    server_id
}

// ---------------------------------------------------------------------------
// FilteringBackend
//
// Wraps MemoryBackend and applies `accountIds` filtering in query_objects for
// Principal.  All other methods delegate to the inner backend.
//
// This backend exists solely to test that the dispatcher correctly passes the
// parsed filter through to the backend.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct FilteringBackend(Arc<MemoryBackend>);

impl FilteringBackend {
    fn new(inner: Arc<MemoryBackend>) -> Self {
        Self(inner)
    }
}

#[allow(async_fn_in_trait)]
impl JmapBackend for FilteringBackend {
    type Error = common::MemoryError;

    async fn account_exists(&self, account_id: &Id) -> Result<bool, Self::Error> {
        self.0.account_exists(account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        self.0.get_objects(account_id, ids, properties).await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.0.get_state::<O>(account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.0
            .get_changes::<O>(account_id, since_state, max_changes)
            .await
    }

    /// For Principal: apply `accountIds` filtering in-memory.
    /// For other types: delegate unchanged.
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        // Only intercept Principal queries — all others delegate as-is.
        if O::TYPE_NAME != "Principal" {
            return self
                .0
                .query_objects::<O>(account_id, filter, sort, limit, position)
                .await;
        }

        // Fetch the raw result from the inner backend (which ignores filters).
        let base = self
            .0
            .query_objects::<O>(account_id, None, sort, limit, position)
            .await?;

        // If no filter, return the unmodified result.
        let Some(filter_val) = filter else {
            return Ok(base);
        };

        // Downcast O::Filter to PrincipalFilterCondition via serde round-trip.
        let filter_json =
            serde_json::to_value(filter_val).expect("filter must be serializable in test context");
        let pfc: PrincipalFilterCondition =
            serde_json::from_value(filter_json).expect("filter must deserialize as PFC");

        // If no accountIds constraint, return unfiltered.
        let Some(required_account_ids) = &pfc.account_ids else {
            return Ok(base);
        };

        // Fetch all Principal objects to check the accounts field.
        // We know O::TYPE_NAME == "Principal" here, so we use Principal directly.
        let (all_principals, _) = self
            .0
            .get_objects::<Principal>(account_id, None, None)
            .await?;

        // For each id in base.ids, include only those whose accounts map
        // contains at least one of the required_account_ids.
        let filtered_ids: Vec<Id> = base
            .ids
            .iter()
            .filter(|id| {
                all_principals
                    .iter()
                    .find(|p| p.id.as_ref() == id.as_ref())
                    .is_some_and(|p| {
                        p.accounts.as_ref().is_some_and(|accounts| {
                            required_account_ids
                                .iter()
                                .any(|req_id| accounts.contains_key(&Id::from(req_id.as_str())))
                        })
                    })
            })
            .cloned()
            .collect();

        let total = filtered_ids.len() as u64;
        Ok(QueryResult::new(
            filtered_ids,
            0,
            Some(total),
            base.query_state,
            true,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_query_state: &State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.0
            .query_changes::<O>(
                account_id,
                since_query_state,
                filter,
                sort,
                max_changes,
                up_to_id,
                collapse_threads,
            )
            .await
    }
}

#[allow(async_fn_in_trait)]
impl SharingBackend for FilteringBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        self.0.create_object(account_id, create_id, obj).await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        self.0.update_object(account_id, id, patch).await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        self.0.destroy_object::<O>(account_id, id).await
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        self.0.supports_type::<O>()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §5.6 — when `sinceQueryState` cannot be resolved, the
/// server MUST return `cannotCalculateChanges`.
///
/// MemoryBackend's `get_changes` parses `sinceQueryState` as a u64 counter;
/// a non-numeric value fails the parse and returns
/// `BackendChangesError::TooManyChanges { limit: 0 }`, which the dispatcher
/// maps to `cannotCalculateChanges`.
#[tokio::test]
async fn principal_query_changes_cannot_calculate() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Principal/queryChanges",
        json!({
            "accountId": "acc1",
            "sinceQueryState": "bogus-non-numeric-state"
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    assert_eq!(
        args["type"].as_str(),
        Some("cannotCalculateChanges"),
        "unparsable sinceQueryState must produce cannotCalculateChanges; got: {args}"
    );
}

/// Oracle: RFC 8620 §5.6 — same `cannotCalculateChanges` contract for
/// `ShareNotification/queryChanges`.
///
/// Identical trigger: non-numeric `sinceQueryState` fails the u64 parse in
/// MemoryBackend and produces `BackendChangesError::TooManyChanges { limit: 0 }`.
#[tokio::test]
async fn notification_query_changes_cannot_calculate() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    seed_notification(
        &backend,
        "acc1",
        json!({
            "id": "n1",
            "created": "2024-06-01T10:00:00Z",
            "changedBy": {
                "name": "Bob",
                "email": "bob@example.com",
                "principalId": null
            },
            "objectType": "Mailbox",
            "objectAccountId": "acc1",
            "objectId": "obj1",
            "oldRights": null,
            "newRights": null,
            "name": "Shared Inbox"
        }),
    )
    .await;

    let req = single_call(
        "ShareNotification/queryChanges",
        json!({
            "accountId": "acc1",
            "sinceQueryState": "not-a-number"
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    assert_eq!(
        args["type"].as_str(),
        Some("cannotCalculateChanges"),
        "unparsable sinceQueryState must produce cannotCalculateChanges; got: {args}"
    );
}

/// Oracle: RFC 9670 §2.4.1 — `accountIds` filter returns only Principals
/// whose `accounts` map contains at least one of the specified account ids.
///
/// Seed two Principals:
/// - Alice: `accounts: {"acc2": {...}}`  — matches `accountIds: ["acc2"]`
/// - Bob:   `accounts: null`             — no accounts, does NOT match
///
/// Dispatch `Principal/query` with `filter: {"accountIds": ["acc2"]}`.
/// Assert only one result is returned and its id matches Alice's.
#[tokio::test]
async fn principal_query_filter_by_account_ids() {
    let inner = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));

    let alice_id = seed_principal(
        &inner,
        "acc1",
        json!({
            "type": "individual",
            "name": "Alice",
            "email": "alice@example.com",
            "description": null,
            "timeZone": null,
            "capabilities": {},
            "accounts": {
                "acc2": {
                    "name": "Alice's Mail",
                    "isPersonal": true,
                    "isReadOnly": false,
                    "accountCapabilities": {}
                }
            }
        }),
    )
    .await;

    // Bob has no accounts — should NOT match the accountIds filter.
    seed_principal(
        &inner,
        "acc1",
        json!({
            "type": "individual",
            "name": "Bob",
            "email": "bob@example.com",
            "description": null,
            "timeZone": null,
            "capabilities": {},
            "accounts": null
        }),
    )
    .await;

    // Wrap with FilteringBackend so the accountIds filter is actually applied.
    let backend = Arc::new(FilteringBackend::new(Arc::clone(&inner)));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Principal/query",
        json!({
            "accountId": "acc1",
            "filter": { "accountIds": ["acc2"] }
        }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    assert!(
        args.get("type").is_none(),
        "query must not be an error: {args}"
    );

    let ids = args["ids"].as_array().expect("ids must be an array");
    assert_eq!(
        ids.len(),
        1,
        "only Alice should match accountIds=[acc2]; got ids: {ids:?}"
    );
    assert_eq!(
        ids[0].as_str(),
        Some(alice_id.as_ref()),
        "matching id must be Alice's; got: {ids:?}"
    );
}
