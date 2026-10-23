//! Forced backend failures inside the AddressBook/set
//! `onDestroyRemoveContents` cascade (RFC 9610 §2.3).
//!
//! Every cascade step (`query_objects`, `get_objects`, `update_object`,
//! `destroy_object` on ContactCard) maps a backend error to a redacted
//! `serverFail` in `notDestroyed` (bd:JMAP-wlip.2), and the book is then
//! not destroyed. The expected SetError is hand-written; the stored state
//! after the call is read back through the unwrapped `MemoryBackend`.
//!
//! Bead: JMAP-w1lm (yt1w.23 follow-up).

mod common;

use common::{MemoryBackend, MemoryError};
use jmap_contacts_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, ContactsBackend,
    GetObject, JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, SetObject, handle_address_book_get, handle_address_book_set,
    handle_contact_card_get,
};
use jmap_server::HandlerOutput;
use jmap_types::{Id, State};
use serde_json::{Value, json};

/// Backend-internal text carried by every forced failure; it must never
/// reach the wire.
const DETAIL: &str = "pg: relation contact_cards_7 is locked";

/// The cascade step that fails.
#[derive(Clone, Copy, PartialEq)]
enum Step {
    Query,
    Get,
    Update,
    Destroy,
}

/// Forwards to `MemoryBackend`, except that `fail` returns `Err` when
/// called for ContactCard.
struct FailingBackend {
    inner: MemoryBackend,
    fail: Step,
}

impl FailingBackend {
    fn fails<O: JmapObject>(&self, step: Step) -> bool {
        self.fail == step && O::TYPE_NAME == "ContactCard"
    }
}

impl JmapBackend for FailingBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, caller: &(), account_id: &Id) -> Result<bool, MemoryError> {
        self.inner.account_exists(caller, account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        if self.fails::<O>(Step::Get) {
            return Err(MemoryError::new(DETAIL));
        }
        self.inner
            .get_objects(caller, account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
    ) -> Result<State, MemoryError> {
        self.inner.get_state::<O>(caller, account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        self.inner
            .get_changes::<O>(caller, account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        let QueryParams {
            filter,
            sort,
            limit,
            position,
            anchor,
            anchor_offset,
            ..
        } = params;
        if self.fails::<O>(Step::Query) {
            return Err(BackendQueryError::Other(MemoryError::new(DETAIL)));
        }
        self.inner
            .query_objects::<O>(
                caller,
                account_id,
                QueryParams::new()
                    .filter(filter)
                    .sort(sort)
                    .limit(limit)
                    .position(position)
                    .anchor(anchor)
                    .anchor_offset(anchor_offset),
            )
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        self.inner
            .query_changes::<O>(caller, account_id, params)
            .await
    }
}

impl ContactsBackend for FailingBackend {
    async fn create_object<O, F>(
        &self,
        caller: &(),
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<MemoryError>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        self.inner
            .create_object(caller, account_id, create_id, build)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<MemoryError>> {
        if self.fails::<O>(Step::Update) {
            return Err(BackendSetError::Other(MemoryError::new(DETAIL)));
        }
        self.inner
            .update_object::<O>(caller, account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        if self.fails::<O>(Step::Destroy) {
            return Err(BackendSetError::Other(MemoryError::new(DETAIL)));
        }
        self.inner.destroy_object::<O>(caller, account_id, id).await
    }

    fn supports_type<O: JmapObject>(&self, caller: &(), account_id: &Id) -> bool {
        self.inner.supports_type::<O>(caller, account_id)
    }

    async fn copy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        from_account_id: &Id,
        to_account_id: &Id,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<MemoryError>> {
        self.inner
            .copy_object(caller, from_account_id, to_account_id, obj)
            .await
    }

    async fn address_book_has_contents(
        &self,
        caller: &(),
        account_id: &Id,
        address_book_id: &Id,
    ) -> Result<bool, MemoryError> {
        self.inner
            .address_book_has_contents(caller, account_id, address_book_id)
            .await
    }
}

/// Seeds books `ab1` and `ab2` and one card `card1` in `books`, then
/// destroys `ab1` with `onDestroyRemoveContents: true` while `fail` is
/// forced. Returns the AddressBook/set response and the backend.
async fn destroy_ab1(fail: Step, books: Value) -> (Value, FailingBackend) {
    let inner = MemoryBackend::new().with_account("acc1");
    for id in ["ab1", "ab2"] {
        inner.seed_object(
            "acc1",
            "AddressBook",
            id,
            json!({
                "id": id,
                "name": id,
                "description": null,
                "sortOrder": 0,
                "isDefault": false,
                "isSubscribed": true,
                "shareWith": null,
                "myRights": {
                    "mayRead": true,
                    "mayWrite": true,
                    "mayShare": true,
                    "mayDelete": true
                }
            }),
        );
    }
    inner.seed_object(
        "acc1",
        "ContactCard",
        "card1",
        json!({
            "id": "card1",
            "@type": "Card",
            "version": "1.0",
            "uid": "card1-uid",
            "addressBookIds": books,
            "name": { "@type": "Name", "full": "Alice" }
        }),
    );
    let backend = FailingBackend { inner, fail };
    let args = json!({
        "accountId": "acc1",
        "onDestroyRemoveContents": true,
        "destroy": ["ab1"]
    });
    let HandlerOutput { primary: resp, .. } = handle_address_book_set(&backend, &(), args)
        .await
        .expect("a cascade failure is a per-record SetError, not a method error");
    (resp, backend)
}

/// Asserts the redacted `serverFail` for `ab1` and that `ab1` survives.
async fn assert_cascade_failed(resp: &Value, backend: &FailingBackend) {
    assert_eq!(
        resp["notDestroyed"]["ab1"],
        json!({ "type": "serverFail", "description": "internal error" }),
        "{resp}"
    );
    assert!(resp["destroyed"].is_null(), "{resp}");
    assert!(
        !resp.to_string().contains(DETAIL),
        "backend error text leaked to the wire: {resp}"
    );

    let HandlerOutput { primary: get, .. } = handle_address_book_get(
        &backend.inner,
        &(),
        json!({ "accountId": "acc1", "ids": ["ab1"] }),
    )
    .await
    .expect("AddressBook/get");
    assert_eq!(get["notFound"], json!([]), "ab1 must survive: {get}");
}

/// Returns `card1`'s stored `addressBookIds`.
async fn card1_books(backend: &FailingBackend) -> Value {
    let HandlerOutput { primary: get, .. } = handle_contact_card_get(
        &backend.inner,
        &(),
        json!({ "accountId": "acc1", "ids": ["card1"], "properties": ["addressBookIds"] }),
    )
    .await
    .expect("ContactCard/get");
    get["list"][0]["addressBookIds"].clone()
}

#[tokio::test]
async fn cascade_query_failure_is_server_fail() {
    let (resp, backend) = destroy_ab1(Step::Query, json!({ "ab1": true })).await;
    assert_cascade_failed(&resp, &backend).await;
    assert_eq!(card1_books(&backend).await, json!({ "ab1": true }));
}

#[tokio::test]
async fn cascade_get_failure_is_server_fail() {
    let (resp, backend) = destroy_ab1(Step::Get, json!({ "ab1": true })).await;
    assert_cascade_failed(&resp, &backend).await;
    assert_eq!(card1_books(&backend).await, json!({ "ab1": true }));
}

/// A card only in `ab1` takes the destroy path; the failed destroy
/// leaves it stored.
#[tokio::test]
async fn cascade_destroy_failure_is_server_fail() {
    let (resp, backend) = destroy_ab1(Step::Destroy, json!({ "ab1": true })).await;
    assert_cascade_failed(&resp, &backend).await;
    assert_eq!(card1_books(&backend).await, json!({ "ab1": true }));
}

/// A card in `ab1` and `ab2` takes the patch path; the failed update
/// leaves both entries in `addressBookIds`.
#[tokio::test]
async fn cascade_update_failure_is_server_fail() {
    let (resp, backend) = destroy_ab1(Step::Update, json!({ "ab1": true, "ab2": true })).await;
    assert_cascade_failed(&resp, &backend).await;
    assert_eq!(
        card1_books(&backend).await,
        json!({ "ab1": true, "ab2": true })
    );
}
