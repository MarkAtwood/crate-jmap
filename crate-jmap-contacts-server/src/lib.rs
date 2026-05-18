//! JMAP Contacts extension method handlers (RFC 9610).
//!
//! # Usage
//!
//! Implement [`ContactsBackend`] for your storage layer, then call
//! [`register_contacts_handlers`] to wire all method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_contacts_server::{ContactsBackend, register_contacts_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: ContactsBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_contacts_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`ContactsBackend`]. This is the same
//! backend used by this crate's own integration tests, intended for
//! downstream contributors to study and for smoke tests / examples
//! that do not want to stand up a real database. **Not production.**
//! API stability is opt-in via this feature and may break across minor
//! versions while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod addressbook;
pub mod backend;
pub mod card;
mod helpers;
/// In-memory reference implementation of [`ContactsBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;

pub use addressbook::{
    handle_address_book_changes, handle_address_book_get, handle_address_book_set,
};
pub use backend::{
    AddedItem, AddressBookProperty, BackendChangesError, BackendSetError, ChangesResult,
    ContactCardProperty, ContactsBackend, GetObject, JmapBackend, JmapObject, QueryChangesResult,
    QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};
pub use card::{
    handle_contact_card_changes, handle_contact_card_copy, handle_contact_card_get,
    handle_contact_card_query, handle_contact_card_query_changes, handle_contact_card_set,
};

/// Capability URI for `urn:ietf:params:jmap:contacts`.
pub use jmap_contacts_types::JMAP_CONTACTS_URI;

// ---------------------------------------------------------------------------
// register_contacts_handlers — the main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all JMAP Contacts method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler.
/// You may pass any `Arc<B>` — the function clones it internally into each
/// registered handler closure. Sharing the same `Arc<B>` across this call
/// and other application-level uses of the backend is a memory
/// optimization, not a correctness requirement; separate `Arc<B>` instances
/// pointing at the same underlying backend would also work.
///
/// After this call, the dispatcher handles:
/// `AddressBook/get`, `AddressBook/changes`, `AddressBook/set`,
/// `ContactCard/get`, `ContactCard/changes`, `ContactCard/set`,
/// `ContactCard/copy`, `ContactCard/query`, `ContactCard/queryChanges`.
///
/// # Re-registration semantics
///
/// This function calls [`Dispatcher::register`] once per
/// draft-ietf-jmap-contacts-15 method name. `Dispatcher::register`
/// **silently overwrites** any pre-existing handler under the same
/// method name (the underlying primitive is `HashMap::insert`). Three
/// consequences callers MUST be aware of:
///
/// - **Double-call**: invoking this function twice on the same
///   dispatcher loses the first set's handlers. The second call wins.
/// - **Custom overrides go LAST**: to replace a single handler (e.g.
///   provide a custom `ContactCard/get`), call this function FIRST,
///   then `dispatcher.register("ContactCard/get", my_override)`. The
///   inverse order silently undoes the custom handler.
/// - **No collision diagnostic**: there is no error or log when a
///   handler is overwritten. The contract is "last register wins" and
///   the caller is responsible for ordering.
///
/// [`Dispatcher::register`]: jmap_server::Dispatcher::register
///
/// **No `AddressBook/query` or `AddressBook/queryChanges`** — the spec
/// (RFC 9610) does not define these methods.
pub fn register_contacts_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: ContactsBackend + 'static,
{
    // Helper: register one method with a closure that takes
    // (Arc<B>, call_id, args, ctx).
    //
    // `$ci` is the call_id string (echoed back to the client). Most handlers
    // ignore it and use `_ci` as the identifier. Only handlers that generate
    // onSuccess* side-effect invocations need `ci`.
    //
    // `$ctx` is the per-request caller context (`B::CallerCtx`) forwarded
    // by the dispatcher. Closures pass `&ctx` to the inner `handle_*` fn.
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident, $ctx:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                backend_arc,
                move |$b: Arc<B>, $ci: String, $a: serde_json::Value, $ctx: B::CallerCtx| {
                    Box::pin(async move { $body }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    // AddressBook
    reg!("AddressBook/get", backend, |b, _ci, a, ctx| {
        handle_address_book_get(&*b, &ctx, a).await
    });
    reg!("AddressBook/changes", backend, |b, _ci, a, ctx| {
        handle_address_book_changes(&*b, &ctx, a).await
    });
    reg!("AddressBook/set", backend, |b, _ci, a, ctx| {
        handle_address_book_set(&*b, &ctx, a).await
    });

    // ContactCard
    reg!("ContactCard/get", backend, |b, _ci, a, ctx| {
        handle_contact_card_get(&*b, &ctx, a).await
    });
    reg!("ContactCard/changes", backend, |b, _ci, a, ctx| {
        handle_contact_card_changes(&*b, &ctx, a).await
    });
    reg!("ContactCard/set", backend, |b, _ci, a, ctx| {
        handle_contact_card_set(&*b, &ctx, a).await
    });
    reg!("ContactCard/copy", backend, |b, ci, a, ctx| {
        handle_contact_card_copy(&*b, &ctx, a, &ci).await
    });
    reg!("ContactCard/query", backend, |b, _ci, a, ctx| {
        handle_contact_card_query(&*b, &ctx, a).await
    });
    reg!("ContactCard/queryChanges", backend, |b, _ci, a, ctx| {
        handle_contact_card_query_changes(&*b, &ctx, a).await
    });
}

/// Generic closure-to-[`JmapHandler`] adapter from [`jmap_server`].
///
/// Re-exported so the [`register_contacts_handlers`] macro body can name
/// `ClosureHandler` without a fully-qualified path. **Stability**: this
/// re-export pins the major-version contract of [`jmap_server::ClosureHandler`]
/// into this crate's public surface — a breaking change to that type
/// upstream is a breaking change here. Consumers needing a closure handler
/// adapter SHOULD prefer importing from [`jmap_server`] directly; the
/// re-export is retained primarily for the in-crate macro and for
/// backward-compatible spelling of the existing handler-registration
/// pattern.
pub use jmap_server::ClosureHandler;

// ---------------------------------------------------------------------------
// test_support — in-memory mock backend used by inline tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[deny(clippy::await_holding_lock)]
pub(crate) mod test_support {
    //! In-memory mock backend for unit tests.
    //!
    //! Provides a minimal `ContactsBackend` implementation. The mock
    //! `address_book_has_contents` returns `true` for the special id
    //! `"ab-nonempty"` and `false` for everything else, enabling tests of
    //! the `onDestroyRemoveContents` logic without a real storage layer.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use jmap_contacts_types::{AddressBook, ContactCard};
    use jmap_server::{
        BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject,
        QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    };
    use jmap_types::{Id, State};

    use crate::backend::ContactsBackend;

    /// Minimal error type for the mock backend.
    #[derive(Debug)]
    pub struct MockError(pub String);

    impl std::fmt::Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "mock error: {}", self.0)
        }
    }

    impl std::error::Error for MockError {}

    /// In-memory state for one account.
    #[derive(Default, Clone)]
    struct AccountState {
        contact_cards: HashMap<Id, ContactCard>,
        addressbooks: HashMap<Id, AddressBook>,
    }

    /// In-memory mock backend for testing.
    #[derive(Clone)]
    pub struct MockBackend {
        state: Arc<Mutex<HashMap<String, AccountState>>>,
        /// Ids in this set report as having contents (for onDestroyRemoveContents tests).
        nonempty_books: Arc<Mutex<std::collections::HashSet<String>>>,
        /// Whether copy_contact_card was called (for copy test verification).
        pub copy_called: Arc<Mutex<bool>>,
        /// When set, `address_book_has_contents` returns `Err` instead of
        /// `Ok(_)`, used to exercise the trait's storage-degraded path
        /// (bd:JMAP-qz9v.27).
        fail_has_contents: Arc<Mutex<bool>>,
    }

    impl MockBackend {
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(HashMap::new())),
                nonempty_books: Arc::new(Mutex::new({
                    let mut s = std::collections::HashSet::new();
                    s.insert("ab-nonempty".to_owned());
                    s
                })),
                copy_called: Arc::new(Mutex::new(false)),
                fail_has_contents: Arc::new(Mutex::new(false)),
            }
        }

        /// Make subsequent `address_book_has_contents` calls return
        /// `Err(MockError)` to exercise the storage-degraded path.
        pub fn set_fail_has_contents(&self, fail: bool) {
            *self.fail_has_contents.lock().unwrap() = fail;
        }

        pub fn new_with_account(account_id: &str) -> Self {
            let b = Self::new();
            b.state
                .lock()
                .unwrap()
                .insert(account_id.to_owned(), AccountState::default());
            b
        }

        /// Register an additional account (used in copy tests).
        pub fn add_account(&mut self, account_id: &str) {
            self.state
                .lock()
                .unwrap()
                .insert(account_id.to_owned(), AccountState::default());
        }

        /// Pre-populate a ContactCard in the given account.
        pub fn add_contact_card(&mut self, account_id: &str, card_id: &str) {
            let card: ContactCard = serde_json::from_value(serde_json::json!({
                "id": card_id,
                "addressBookIds": { "ab1": true }
            }))
            .expect("test fixture must deserialize");
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.contact_cards.insert(Id::from(card_id), card);
        }

        /// Pre-populate an AddressBook in the given account.
        pub fn seed_addressbook(&mut self, account_id: &str, book_id: &str, is_default: bool) {
            let book: AddressBook = serde_json::from_value(serde_json::json!({
                "id": book_id,
                "name": book_id,
                "sortOrder": 0,
                "isDefault": is_default,
                "isSubscribed": false,
                "shareWith": null,
                "myRights": {
                    "mayRead": true,
                    "mayWrite": true,
                    "mayShare": false,
                    "mayDelete": false
                }
            }))
            .expect("test fixture must deserialize");
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.addressbooks.insert(Id::from(book_id), book);
        }
    }

    impl JmapBackend for MockBackend {
        type Error = MockError;
        type CallerCtx = ();

        async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
            Ok(self.state.lock().unwrap().contains_key(account_id.as_ref()))
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            ids: Option<&[Id]>,
            _properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            // Try AddressBook store first (all or by id), then ContactCard store.
            let guard = self.state.lock().unwrap();
            if let Some(acct) = guard.get(account_id.as_ref()) {
                // Try to deserialize from addressbooks map.
                let ab_values: Vec<serde_json::Value> = if let Some(requested_ids) = ids {
                    requested_ids
                        .iter()
                        .filter_map(|id| acct.addressbooks.get(id))
                        .filter_map(|b| serde_json::to_value(b).ok())
                        .collect()
                } else {
                    acct.addressbooks
                        .values()
                        .filter_map(|b| serde_json::to_value(b).ok())
                        .collect()
                };
                // If the store is non-empty and the type is AddressBook, return it.
                if !ab_values.is_empty() || (ids.is_none() && !acct.addressbooks.is_empty()) {
                    let found: Vec<O> = ab_values
                        .into_iter()
                        .filter_map(|v| serde_json::from_value::<O>(v).ok())
                        .collect();
                    return Ok((found, vec![]));
                }

                // Fall through to ContactCard store.
                if let Some(requested_ids) = ids {
                    let mut found: Vec<O> = Vec::new();
                    let mut not_found: Vec<Id> = Vec::new();
                    for id in requested_ids {
                        if let Some(card) = acct.contact_cards.get(id) {
                            let v = serde_json::to_value(card)
                                .ok()
                                .and_then(|v| serde_json::from_value::<O>(v).ok());
                            match v {
                                Some(obj) => found.push(obj),
                                None => not_found.push(id.clone()),
                            }
                        } else {
                            not_found.push(id.clone());
                        }
                    }
                    return Ok((found, not_found));
                }
            }
            Ok((vec![], vec![]))
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<State, Self::Error> {
            Ok(State::from("0"))
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _since_state: &State,
            _max_changes: Option<u64>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            Ok(ChangesResult::new(
                vec![],
                vec![],
                vec![],
                false,
                State::from("0"),
            ))
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _limit: Option<u64>,
            _position: i64,
        ) -> Result<QueryResult, Self::Error> {
            Ok(QueryResult::new(
                vec![],
                0,
                Some(0),
                State::from("0"),
                false,
            ))
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            since_query_state: &State,
            _filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _max_changes: Option<u64>,
            _up_to_id: Option<&Id>,
            _collapse_threads: bool,
        ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
            Ok(QueryChangesResult::new(
                since_query_state.clone(),
                State::from("0"),
                Some(0),
                vec![],
                vec![],
            ))
        }
    }

    impl ContactsBackend for MockBackend {
        async fn create_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _create_id: &str,
            obj: O,
        ) -> Result<(Id, O), BackendSetError<Self::Error>> {
            Ok((Id::from("mock-id-1"), obj))
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
            patch: O::Patch,
        ) -> Result<Option<O>, BackendSetError<Self::Error>> {
            // Attempt to apply as an AddressBook patch.
            let patch_val = match serde_json::to_value(&patch) {
                Ok(v) => v,
                Err(_) => {
                    return Err(BackendSetError::SetError(SetError::new(
                        SetErrorType::NotFound,
                    )))
                }
            };

            let mut guard = self.state.lock().unwrap();
            if let Some(acct) = guard.get_mut(account_id.as_ref()) {
                if let Some(existing_book) = acct.addressbooks.get(id).cloned() {
                    // Apply the patch: merge JSON fields into the existing book.
                    if let Ok(mut book_val) = serde_json::to_value(&existing_book) {
                        if let (Some(obj), Some(patch_obj)) =
                            (book_val.as_object_mut(), patch_val.as_object())
                        {
                            for (k, v) in patch_obj {
                                obj.insert(k.clone(), v.clone());
                            }
                        }
                        if let Ok(updated_book) = serde_json::from_value::<AddressBook>(book_val) {
                            // Single-default invariant: if this book is now
                            // default, clear isDefault on all other books.
                            if updated_book.is_default {
                                let target_id = id.clone();
                                for (other_id, other_book) in acct.addressbooks.iter_mut() {
                                    if *other_id != target_id {
                                        other_book.is_default = false;
                                    }
                                }
                            }
                            // Store the updated book.
                            acct.addressbooks.insert(id.clone(), updated_book.clone());
                            // Try to cast back to O (succeeds when O = AddressBook).
                            if let Ok(obj) = serde_json::to_value(&updated_book)
                                .and_then(|v| serde_json::from_value::<O>(v))
                            {
                                return Ok(Some(obj));
                            }
                        }
                    }
                }
            }

            Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )))
        }

        async fn destroy_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _id: &Id,
        ) -> Result<(), BackendSetError<Self::Error>> {
            Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )))
        }

        fn supports_type<O: JmapObject>(&self) -> bool {
            true
        }

        async fn copy_contact_card(
            &self,
            _caller: &(),
            _from_account_id: &Id,
            _to_account_id: &Id,
            card: ContactCard,
        ) -> Result<(Id, ContactCard), BackendSetError<Self::Error>> {
            *self.copy_called.lock().unwrap() = true;
            Ok((Id::from("copied-id-1"), card))
        }

        async fn address_book_has_contents(
            &self,
            _caller: &(),
            _account_id: &Id,
            address_book_id: &Id,
        ) -> Result<bool, Self::Error> {
            if *self.fail_has_contents.lock().unwrap() {
                return Err(MockError("simulated storage failure".to_owned()));
            }
            Ok(self
                .nonempty_books
                .lock()
                .unwrap()
                .contains(address_book_id.as_ref()))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jmap_server::{Dispatcher, JmapRequest, State};
    use serde_json::json;

    use super::*;
    use crate::test_support::MockBackend;

    fn single_call(method: &str, args: serde_json::Value, call_id: &str) -> JmapRequest {
        JmapRequest::new(
            vec![JMAP_CONTACTS_URI.into()],
            vec![(method.into(), args, call_id.into())],
            None,
        )
    }

    /// Oracle: register_contacts_handlers registers all 9 spec methods.
    ///
    /// Verification: each method name returns a non-unknownMethod response.
    #[tokio::test]
    async fn registers_all_9_methods() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::clone(&backend));

        let methods: &[(&str, serde_json::Value)] = &[
            ("AddressBook/get", json!({"accountId": "acc1", "ids": null})),
            (
                "AddressBook/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            (
                "AddressBook/set",
                json!({"accountId": "acc1", "destroy": []}),
            ),
            ("ContactCard/get", json!({"accountId": "acc1", "ids": null})),
            (
                "ContactCard/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            (
                "ContactCard/set",
                json!({"accountId": "acc1", "destroy": []}),
            ),
            (
                "ContactCard/copy",
                json!({
                    "accountId": "acc1",
                    "fromAccountId": "acc1",
                    "create": {}
                }),
            ),
            (
                "ContactCard/query",
                json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "ContactCard/queryChanges",
                json!({"accountId": "acc1", "sinceQueryState": "0"}),
            ),
        ];

        for (method, args) in methods {
            let req = single_call(method, args.clone(), "c0");
            let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
            assert_eq!(
                resp.method_responses.len(),
                1,
                "{method}: expected 1 response"
            );
            let (_, resp_args, _) = &resp.method_responses[0];
            assert_ne!(
                resp_args["type"], "unknownMethod",
                "{method}: must not be unknownMethod — is it registered?"
            );
        }
    }

    /// Oracle: AddressBook/query and AddressBook/queryChanges are NOT registered.
    ///
    /// Source: RFC 9610 §2 — the spec does not define these methods.
    #[tokio::test]
    async fn address_book_query_and_query_changes_are_not_registered() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::clone(&backend));

        for method in &["AddressBook/query", "AddressBook/queryChanges"] {
            let req = single_call(method, json!({"accountId": "acc1", "filter": null}), "c0");
            let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
            let (_, resp_args, _) = &resp.method_responses[0];
            assert_eq!(
                resp_args["type"], "unknownMethod",
                "{method}: must be unknownMethod (not defined by spec)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // AddressBook/set onSuccessSetIsDefault dispatcher-based tests (JMAP-jf2h.6)
    // -----------------------------------------------------------------------

    /// Oracle: RFC 9610 §2.3 — onSuccessSetIsDefault with a bare string id
    /// applied via the dispatcher sets the target book as default and reports
    /// both the promoted and demoted books in `updated`.
    ///
    /// Pre-conditions: book1 is default; book2 is not.
    /// Action (via dispatcher): AddressBook/set with onSuccessSetIsDefault: "book2".
    /// Expected: updated contains book2 (isDefault:true) and book1 (isDefault:false).
    #[tokio::test]
    async fn on_success_set_is_default_string_id_applied_via_dispatcher() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.seed_addressbook("acc1", "book1", true);
        backend.seed_addressbook("acc1", "book2", false);

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::new(backend));

        let req = single_call(
            "AddressBook/set",
            json!({
                "accountId": "acc1",
                "onSuccessSetIsDefault": "book2"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];

        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );

        let updated = args["updated"]
            .as_object()
            .expect("updated must be an object when isDefault transfers");

        assert!(
            updated.contains_key("book2"),
            "book2 (newly default) must appear in updated: {args}"
        );
        assert_eq!(
            updated["book2"]["isDefault"],
            json!(true),
            "book2 must have isDefault:true: {args}"
        );

        assert!(
            updated.contains_key("book1"),
            "book1 (demoted) must appear in updated: {args}"
        );
        assert_eq!(
            updated["book1"]["isDefault"],
            json!(false),
            "book1 must have isDefault:false after demotion: {args}"
        );
    }

    /// Oracle: RFC 9610 §2.3 — onSuccessSetIsDefault MUST be skipped when
    /// any main set operation produced an error.  A failing create (missing
    /// required `name` field) produces notCreated, which must prevent
    /// onSuccessSetIsDefault from running.
    #[tokio::test]
    async fn on_success_set_is_default_skipped_when_create_fails_via_dispatcher() {
        let backend = MockBackend::new_with_account("acc1");
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::new(backend));

        let req = single_call(
            "AddressBook/set",
            json!({
                "accountId": "acc1",
                "create": { "c1": {} },   // missing required name → invalidProperties
                "onSuccessSetIsDefault": "book1"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];

        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        // The create must have failed.
        assert!(
            args["notCreated"].is_object(),
            "c1 must be in notCreated: {args}"
        );
        // onSuccessSetIsDefault must NOT have run.
        let updated = &args["updated"];
        let is_empty =
            updated.is_null() || updated.as_object().map(|o| o.is_empty()).unwrap_or(true);
        assert!(
            is_empty,
            "updated must be empty — onSuccessSetIsDefault must not run after create failure: {args}"
        );
    }

    /// Oracle: RFC 9610 §2.3 — onSuccessSetIsDefault: null is a no-op;
    /// the dispatcher must return a valid response with no top-level error.
    #[tokio::test]
    async fn on_success_set_is_default_null_no_op_via_dispatcher() {
        let backend = MockBackend::new_with_account("acc1");
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::new(backend));

        let req = single_call(
            "AddressBook/set",
            json!({
                "accountId": "acc1",
                "onSuccessSetIsDefault": null
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];

        assert!(
            args.get("type").is_none(),
            "null onSuccessSetIsDefault must not cause an error: {args}"
        );
        assert_eq!(args["accountId"], "acc1");
    }

    /// Oracle: AddressBook/set destroy with non-empty book returns
    /// addressBookHasContents via the dispatcher path.
    #[tokio::test]
    async fn dispatcher_address_book_set_destroy_non_empty_returns_error() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "AddressBook/set",
            json!({"accountId": "acc1", "destroy": ["ab-nonempty"]}),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notDestroyed"]["ab-nonempty"]["type"], "addressBookHasContents",
            "non-empty book must yield addressBookHasContents: {args}"
        );
    }
}
