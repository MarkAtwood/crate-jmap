//! JMAP Object Metadata extension method handlers
//! ([draft-ietf-jmap-metadata-01](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/)).
//!
//! # Usage
//!
//! Implement [`MetadataBackend`] for your storage layer, then call
//! [`register_metadata_handlers`] to wire all method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_metadata_server::{MetadataBackend, register_metadata_handlers};
//! # use jmap_server::{Dispatcher, JmapBackend};
//! # fn example<B: MetadataBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_metadata_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`MetadataBackend`]. This is the same
//! backend used by this crate's own integration tests, intended for
//! downstream contributors to study and for smoke tests / examples
//! that do not want to stand up a real database. **Not production.**
//! API stability is opt-in via this feature and may break across minor
//! versions while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
mod helpers;
/// In-memory reference implementation of [`MetadataBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;
pub mod metadata;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, MetadataBackend, MetadataProperty, QueryChangesResult, QueryObject, QueryResult,
    SetError, SetErrorType, SetObject,
};
pub use metadata::{
    handle_metadata_changes, handle_metadata_get, handle_metadata_query,
    handle_metadata_query_changes, handle_metadata_set,
};

/// Capability URI for `urn:ietf:params:jmap:metadata` (draft-ietf-jmap-metadata-01 §1.2.1).
pub use jmap_metadata_types::JMAP_METADATA_URI;

// ---------------------------------------------------------------------------
// register_metadata_handlers — main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all JMAP Metadata method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler
/// closure. You may pass any `Arc<B>` — the function clones it internally
/// into each registered handler closure. Sharing the same `Arc<B>` across
/// this call and other application-level uses of the backend is a memory
/// optimization, not a correctness requirement; separate `Arc<B>` instances
/// pointing at the same underlying backend would also work.
///
/// After this call the dispatcher handles:
/// `Metadata/get`, `Metadata/changes`, `Metadata/set`,
/// `Metadata/query`, `Metadata/queryChanges`.
///
/// The dispatcher's `CallerCtx` is taken from `B::CallerCtx`; every registered
/// closure forwards it as `&ctx` into the wrapped `handle_*` function. Backends
/// that use `type CallerCtx = ()` therefore see `&()` inside every handler.
///
/// # Re-registration semantics
///
/// This function calls [`Dispatcher::register`] once per
/// draft-ietf-jmap-metadata-01 method name. `Dispatcher::register`
/// **silently overwrites** any pre-existing handler under the same
/// method name (the underlying primitive is `HashMap::insert`). Three
/// consequences callers MUST be aware of:
///
/// - **Double-call**: invoking this function twice on the same
///   dispatcher loses the first set's handlers. The second call wins.
/// - **Custom overrides go LAST**: to replace a single handler (e.g.
///   provide a custom `Metadata/get`), call this function FIRST, then
///   `dispatcher.register("Metadata/get", my_override)`. The inverse
///   order silently undoes the custom handler.
/// - **No collision diagnostic**: there is no error or log when a
///   handler is overwritten. The contract is "last register wins" and
///   the caller is responsible for ordering.
///
/// Concurrent calls to this function on the same dispatcher are
/// forbidden at the type-system level by the `&mut Dispatcher`
/// argument; the borrow checker rejects them at compile time.
///
/// [`Dispatcher::register`]: jmap_server::Dispatcher::register
pub fn register_metadata_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: MetadataBackend + 'static,
{
    // Helper: register one method with a closure that takes
    // (Arc<B>, call_id, args, ctx). `$ctx` is the per-request caller context
    // (`B::CallerCtx`) forwarded by the dispatcher; closures pass `&ctx` to the
    // inner `handle_*` fn.
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

    reg!("Metadata/get", backend, |b, _ci, a, ctx| {
        handle_metadata_get(&*b, &ctx, a).await
    });
    reg!("Metadata/changes", backend, |b, _ci, a, ctx| {
        handle_metadata_changes(&*b, &ctx, a).await
    });
    reg!("Metadata/set", backend, |b, _ci, a, ctx| {
        handle_metadata_set(&*b, &ctx, a).await
    });
    reg!("Metadata/query", backend, |b, _ci, a, ctx| {
        handle_metadata_query(&*b, &ctx, a).await
    });
    reg!("Metadata/queryChanges", backend, |b, _ci, a, ctx| {
        handle_metadata_query_changes(&*b, &ctx, a).await
    });
}

/// Generic closure-to-[`JmapHandler`] adapter from [`jmap_server`].
///
/// Re-exported so the [`register_metadata_handlers`] macro body can name
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
    //! Provides a minimal [`MetadataBackend`] implementation backed by
    //! `HashMap`s. Not suitable for production use; see
    //! [`crate::memory::MemoryBackend`] for the public reference impl.

    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use jmap_metadata_types::Metadata;
    use jmap_server::{
        BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject,
        QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    };
    use jmap_types::{Id, State};

    use crate::backend::MetadataBackend;

    /// Opaque storage-layer error returned by [`MockBackend`] operations.
    ///
    /// Mirrors the canonical [`jmap-mail-server`]'s `MemoryError`
    /// shape — `#[non_exhaustive]`, named-field, constructor +
    /// description accessor — per workspace AGENTS.md canonical-template
    /// propagation rule. Outside-crate construction goes through
    /// [`MockError::new`]; outside-crate reads go through
    /// [`MockError::description`]. Future revisions can add structured
    /// context (error kind, source reference, account id, etc.) without
    /// a breaking change.
    #[non_exhaustive]
    #[derive(Debug)]
    pub struct MockError {
        description: String,
    }

    impl MockError {
        /// Construct a [`MockError`] from a human-readable description.
        pub fn new(description: impl Into<String>) -> Self {
            Self {
                description: description.into(),
            }
        }

        /// Human-readable description of the underlying failure.
        pub fn description(&self) -> &str {
            &self.description
        }
    }

    impl std::fmt::Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "mock error: {}", self.description)
        }
    }

    impl std::error::Error for MockError {}

    /// In-memory state for one account. Public-in-test so the
    /// `metadata::tests` module can pre-populate the change log.
    #[derive(Default, Clone)]
    pub(crate) struct AccountState {
        pub(crate) metadatas: HashMap<Id, Metadata>,
        pub(crate) state: u64,
        /// Recorded created Ids since the last state snapshot, in
        /// insertion order, for `get_changes`-style tests.
        pub(crate) created: Vec<Id>,
        /// Recorded updated Ids since the last state snapshot.
        pub(crate) updated: Vec<Id>,
        /// Recorded destroyed Ids since the last state snapshot.
        pub(crate) destroyed: Vec<Id>,
        /// Monotonic counter for synthesising mock-id values.
        pub(crate) next_id: u64,
        /// When set to `Some`, `create_object` returns this SetError
        /// unconditionally — used by tests that exercise the §3.1
        /// uniqueness / forbidden / overQuota paths.
        pub(crate) forced_create_error: Option<SetError>,
    }

    /// In-memory mock backend for testing.
    #[derive(Clone)]
    pub struct MockBackend {
        /// Known accounts and their state. The outer `Arc<Mutex<…>>` allows
        /// the mock to be shared across threads (required by
        /// `JmapBackend: Sync`).
        state: Arc<Mutex<HashMap<String, AccountState>>>,
        /// Counter incremented on every `get_objects` call. Used by
        /// `Metadata/changes` round-trip-count regression tests
        /// (bd:JMAP-ayoz.9) to pin the handler's union-fetch behaviour.
        get_objects_calls: Arc<AtomicU64>,
    }

    impl MockBackend {
        /// Create a backend with no accounts registered.
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(HashMap::new())),
                get_objects_calls: Arc::new(AtomicU64::new(0)),
            }
        }

        /// Create a backend with the given account already registered.
        pub fn new_with_account(account_id: &str) -> Self {
            let b = Self::new();
            b.state
                .lock()
                .unwrap()
                .insert(account_id.to_owned(), AccountState::default());
            b
        }

        /// Number of `get_objects` calls observed since this backend
        /// instance was constructed. Tests assert against this value to
        /// pin handler round-trip counts (bd:JMAP-ayoz.9).
        pub fn get_objects_call_count(&self) -> u64 {
            self.get_objects_calls.load(Ordering::Relaxed)
        }

        /// Pre-populate a Metadata object in the given account.
        pub fn add_metadata(&self, account_id: &str, id: &str, json: serde_json::Value) {
            let meta: Metadata =
                serde_json::from_value(json).expect("test fixture must deserialize");
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.metadatas.insert(Id::from(id), meta);
        }

        /// Force the **next** `create_object` call to fail with the given
        /// [`SetError`]. One-shot: auto-resets after a single create call
        /// consumes the slot (bd:JMAP-826m.15). Tests that need a second
        /// forced error must call this method again.
        ///
        /// The one-shot shape matches the existing call-site pattern
        /// (every existing usage in this crate is `force_create_error(...)`
        /// followed by exactly one `create_object` call) and removes a
        /// future-bug magnet where a sticky error silently affected
        /// subsequent unrelated creates on the same backend instance.
        pub fn force_create_error(&self, account_id: &str, err: SetError) {
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.forced_create_error = Some(err);
        }

        /// Lock the backend's internal account-state map for the duration of
        /// a test. Used by `Metadata/changes` filter tests to seed the
        /// change log (created/updated/destroyed vectors) and bump the
        /// `state` counter without going through `create_object` /
        /// `update_object` / `destroy_object`.
        pub fn state_for_test(&self) -> std::sync::MutexGuard<'_, HashMap<String, AccountState>> {
            self.state.lock().unwrap()
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
            self.get_objects_calls.fetch_add(1, Ordering::Relaxed);
            // The mock stores Metadata objects only. Other O types return an
            // empty list — fine for these tests.
            if O::TYPE_NAME != Metadata::TYPE_NAME {
                return Ok((vec![], vec![]));
            }
            let guard = self.state.lock().unwrap();
            let Some(acct) = guard.get(account_id.as_ref()) else {
                return Ok((vec![], vec![]));
            };

            let (found_meta, not_found): (Vec<Metadata>, Vec<Id>) = match ids {
                None => (acct.metadatas.values().cloned().collect(), vec![]),
                Some(req_ids) => {
                    let mut found = Vec::new();
                    let mut missing = Vec::new();
                    for id in req_ids {
                        if let Some(m) = acct.metadatas.get(id) {
                            found.push(m.clone());
                        } else {
                            missing.push(id.clone());
                        }
                    }
                    (found, missing)
                }
            };
            // Down-cast Metadata -> O via serde round-trip. This is the
            // canonical bridge for the JmapBackend::get_objects generic
            // parameter (the same bridge every reference MockBackend uses
            // to satisfy the generic O on a backend that stores a single
            // concrete type).
            let mut converted: Vec<O> = Vec::with_capacity(found_meta.len());
            for m in found_meta {
                let v = serde_json::to_value(&m).map_err(|e| MockError::new(e.to_string()))?;
                let o: O = serde_json::from_value(v).map_err(|e| MockError::new(e.to_string()))?;
                converted.push(o);
            }
            Ok((converted, not_found))
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
        ) -> Result<State, Self::Error> {
            let guard = self.state.lock().unwrap();
            let s = guard.get(account_id.as_ref()).map(|a| a.state).unwrap_or(0);
            Ok(State::from(s.to_string()))
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            since_state: &State,
            _max_changes: Option<u64>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            let guard = self.state.lock().unwrap();
            let Some(acct) = guard.get(account_id.as_ref()) else {
                return Ok(ChangesResult::new(
                    vec![],
                    vec![],
                    vec![],
                    false,
                    State::from("0"),
                ));
            };
            // For test simplicity: if since_state matches current, no changes;
            // otherwise return all recorded.
            let cur = acct.state.to_string();
            if since_state.as_ref() == cur {
                return Ok(ChangesResult::new(
                    vec![],
                    vec![],
                    vec![],
                    false,
                    State::from(cur),
                ));
            }
            Ok(ChangesResult::new(
                acct.created.clone(),
                acct.updated.clone(),
                acct.destroyed.clone(),
                false,
                State::from(cur),
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

    impl MetadataBackend for MockBackend {
        async fn create_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            _create_id: &str,
            obj: O,
        ) -> Result<(Id, O), BackendSetError<Self::Error>> {
            let mut guard = self.state.lock().unwrap();

            // Defense-in-depth account guard (bd:JMAP-826m.19).
            // Pre-fix this method silently auto-registered an unknown
            // account via `.entry(...).or_default()`, diverging from
            // the public MemoryBackend which hard-errors. Two backends
            // with different unknown-account behavior are a future-bug
            // magnet for contributors writing dual-target tests.
            if !guard.contains_key(account_id.as_ref()) {
                return Err(BackendSetError::Other(MockError::new(format!(
                    "unknown account: {}",
                    account_id.as_ref()
                ))));
            }
            let acct = guard
                .get_mut(account_id.as_ref())
                .expect("checked contains_key above");

            // Consume the one-shot forced error if present
            // (bd:JMAP-826m.15). `.take()` resets the slot so a second
            // create_object call on the same backend instance is not
            // silently affected by a prior test's forced error.
            if let Some(err) = acct.forced_create_error.take() {
                return Err(BackendSetError::SetError(err));
            }

            acct.next_id += 1;
            let new_id = Id::from(format!("md{}", acct.next_id));

            // Inject the assigned id into obj via serde round-trip, then
            // store it as a Metadata for later retrieval.
            let v = serde_json::to_value(&obj)
                .map_err(|e| BackendSetError::Other(MockError::new(format!("serialize: {e}"))))?;
            let serde_json::Value::Object(mut v) = v else {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::InvalidProperties,
                )));
            };
            v.insert(
                "id".to_owned(),
                serde_json::Value::String(new_id.as_ref().to_owned()),
            );
            let v = serde_json::Value::Object(v);

            let stored: Metadata = serde_json::from_value(v.clone()).map_err(|e| {
                BackendSetError::Other(MockError::new(format!("deserialize stored: {e}")))
            })?;
            acct.metadatas.insert(new_id.clone(), stored);
            acct.state += 1;
            acct.created.push(new_id.clone());

            let echoed: O = serde_json::from_value(v).map_err(|e| {
                BackendSetError::Other(MockError::new(format!("deserialize echo: {e}")))
            })?;
            Ok((new_id, echoed))
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
            _patch: O::Patch,
        ) -> Result<Option<O>, BackendSetError<Self::Error>> {
            let mut guard = self.state.lock().unwrap();
            let Some(acct) = guard.get_mut(account_id.as_ref()) else {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )));
            };
            if !acct.metadatas.contains_key(id) {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )));
            }
            acct.state += 1;
            acct.updated.push(id.clone());
            // The mock does not apply the patch — it just signals success.
            // Tests for the patch semantics belong with the real
            // MemoryBackend in JMAP-06zp.3.4.
            Ok(None)
        }

        async fn destroy_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
        ) -> Result<(), BackendSetError<Self::Error>> {
            let mut guard = self.state.lock().unwrap();
            let Some(acct) = guard.get_mut(account_id.as_ref()) else {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )));
            };
            if acct.metadatas.remove(id).is_none() {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )));
            }
            acct.state += 1;
            acct.destroyed.push(id.clone());
            Ok(())
        }

        fn supports_type<O: JmapObject>(&self) -> bool {
            O::TYPE_NAME == Metadata::TYPE_NAME
        }
    }
}
