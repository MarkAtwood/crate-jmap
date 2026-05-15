//! JMAP Calendars extension method handlers (draft-ietf-jmap-calendars-26).
//!
//! # Usage
//!
//! Implement [`CalendarsBackend`] for your storage layer, then call
//! [`register_calendars_handlers`] to wire all method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_calendars_server::{CalendarsBackend, register_calendars_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: CalendarsBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_calendars_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`CalendarsBackend`]. This is the same
//! backend used by this crate's own integration tests, intended for
//! downstream contributors to study and for smoke tests / examples
//! that do not want to stand up a real database. **Not production.**
//! API stability is opt-in via this feature and may break across minor
//! versions while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
pub mod calendar;
pub mod event;
pub mod event_notification;
mod helpers;
/// In-memory reference implementation of [`CalendarsBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;
pub mod participant_identity;
pub mod principal;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, CalendarsBackend, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
};
pub use calendar::{handle_calendar_changes, handle_calendar_get, handle_calendar_set};
pub use event::{
    handle_calendar_event_changes, handle_calendar_event_copy, handle_calendar_event_get,
    handle_calendar_event_parse, handle_calendar_event_query, handle_calendar_event_query_changes,
    handle_calendar_event_set,
};
pub use event_notification::{
    handle_calendar_event_notification_changes, handle_calendar_event_notification_get,
    handle_calendar_event_notification_query, handle_calendar_event_notification_query_changes,
    handle_calendar_event_notification_set,
};
pub use participant_identity::{
    handle_participant_identity_changes, handle_participant_identity_get,
    handle_participant_identity_set,
};
pub use principal::handle_principal_get_availability;

/// Capability URI for `urn:ietf:params:jmap:calendars`.
pub use jmap_calendars_types::JMAP_CALENDARS_URI;

// ---------------------------------------------------------------------------
// register_calendars_handlers — the main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all JMAP Calendars method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler.
///
/// After this call, the dispatcher handles all 20 Calendars methods:
/// - `Calendar/get`, `Calendar/changes`, `Calendar/set`
/// - `CalendarEvent/get`, `CalendarEvent/changes`, `CalendarEvent/set`,
///   `CalendarEvent/copy`, `CalendarEvent/query`, `CalendarEvent/queryChanges`,
///   `CalendarEvent/parse`
/// - `CalendarEventNotification/get`, `CalendarEventNotification/changes`,
///   `CalendarEventNotification/set`, `CalendarEventNotification/query`,
///   `CalendarEventNotification/queryChanges`
/// - `ParticipantIdentity/get`, `ParticipantIdentity/changes`,
///   `ParticipantIdentity/set`
/// - `Principal/getAvailability`
///
/// The dispatcher's `CallerCtx` is taken from `B::CallerCtx`; every registered
/// closure forwards it as `&ctx` into the wrapped `handle_*` function. Backends
/// that use `type CallerCtx = ()` therefore see `&()` inside every handler.
pub fn register_calendars_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: CalendarsBackend + 'static,
{
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

    // Calendar
    reg!("Calendar/get", backend, |b, _ci, a, ctx| {
        handle_calendar_get(&*b, &ctx, a).await
    });
    reg!("Calendar/changes", backend, |b, _ci, a, ctx| {
        handle_calendar_changes(&*b, &ctx, a).await
    });
    reg!("Calendar/set", backend, |b, _ci, a, ctx| {
        handle_calendar_set(&*b, &ctx, a).await
    });

    // CalendarEvent
    reg!("CalendarEvent/get", backend, |b, _ci, a, ctx| {
        handle_calendar_event_get(&*b, &ctx, a).await
    });
    reg!("CalendarEvent/changes", backend, |b, _ci, a, ctx| {
        handle_calendar_event_changes(&*b, &ctx, a).await
    });
    reg!("CalendarEvent/set", backend, |b, _ci, a, ctx| {
        handle_calendar_event_set(&*b, &ctx, a).await
    });
    reg!("CalendarEvent/copy", backend, |b, ci, a, ctx| {
        handle_calendar_event_copy(&*b, &ctx, a, &ci).await
    });
    reg!("CalendarEvent/query", backend, |b, _ci, a, ctx| {
        handle_calendar_event_query(&*b, &ctx, a).await
    });
    reg!("CalendarEvent/queryChanges", backend, |b, _ci, a, ctx| {
        handle_calendar_event_query_changes(&*b, &ctx, a).await
    });
    reg!("CalendarEvent/parse", backend, |b, _ci, a, ctx| {
        handle_calendar_event_parse(&*b, &ctx, a).await
    });

    // CalendarEventNotification
    reg!(
        "CalendarEventNotification/get",
        backend,
        |b, _ci, a, ctx| handle_calendar_event_notification_get(&*b, &ctx, a).await
    );
    reg!(
        "CalendarEventNotification/changes",
        backend,
        |b, _ci, a, ctx| handle_calendar_event_notification_changes(&*b, &ctx, a).await
    );
    reg!(
        "CalendarEventNotification/set",
        backend,
        |b, _ci, a, ctx| handle_calendar_event_notification_set(&*b, &ctx, a).await
    );
    reg!(
        "CalendarEventNotification/query",
        backend,
        |b, _ci, a, ctx| handle_calendar_event_notification_query(&*b, &ctx, a).await
    );
    reg!(
        "CalendarEventNotification/queryChanges",
        backend,
        |b, _ci, a, ctx| handle_calendar_event_notification_query_changes(&*b, &ctx, a).await
    );

    // ParticipantIdentity
    reg!("ParticipantIdentity/get", backend, |b, _ci, a, ctx| {
        handle_participant_identity_get(&*b, &ctx, a).await
    });
    reg!("ParticipantIdentity/changes", backend, |b, _ci, a, ctx| {
        handle_participant_identity_changes(&*b, &ctx, a).await
    });
    reg!("ParticipantIdentity/set", backend, |b, _ci, a, ctx| {
        handle_participant_identity_set(&*b, &ctx, a).await
    });

    // Principal
    reg!("Principal/getAvailability", backend, |b, _ci, a, ctx| {
        handle_principal_get_availability(&*b, &ctx, a).await
    });
}

pub use jmap_server::ClosureHandler;

// ---------------------------------------------------------------------------
// test_support — in-memory mock backend used by inline tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[deny(clippy::await_holding_lock)]
pub(crate) mod test_support {
    //! In-memory mock backend for unit tests.
    //!
    //! `std::sync::Mutex` is used here for simplicity. Every lock is dropped
    //! before any `.await` (the `await_holding_lock` lint enforces this at
    //! the module level). If a future change needs to hold the lock across
    //! an `.await`, switch to `tokio::sync::Mutex` instead of disabling the
    //! lint.

    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    use jmap_server::{
        BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject,
        QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    };
    use jmap_types::{Id, State};

    use crate::backend::{
        CalendarEventGetArgs, CalendarEventQueryArgs, CalendarEventSetArgs, CalendarsBackend,
        QueryCalendarEventsError,
    };

    #[derive(Debug)]
    pub struct MockError(pub String);

    impl std::fmt::Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "mock error: {}", self.0)
        }
    }

    impl std::error::Error for MockError {}

    /// Per-account state for the mock.
    #[derive(Default, Clone)]
    struct AccountState {
        /// Set of calendar IDs that have at least one event.
        calendars_with_events: HashSet<Id>,
        /// Simple object store: type_name → id → serialized object.
        objects: HashMap<String, HashMap<Id, serde_json::Value>>,
        /// Current default Calendar id (for §4.3 onSuccessSetIsDefault tests).
        default_calendar: Option<Id>,
        /// Current default ParticipantIdentity id (for §3.3
        /// onSuccessSetIsDefault tests).
        default_participant_identity: Option<Id>,
    }

    #[derive(Clone)]
    pub struct MockBackend {
        state: Arc<Mutex<HashMap<String, AccountState>>>,
        /// Last `CalendarEventGetArgs` seen by `get_calendar_events`. Tests
        /// inspect this to verify §5.7 args were threaded from the handler.
        last_get_args: Arc<Mutex<Option<CalendarEventGetArgs>>>,
    }

    impl MockBackend {
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(HashMap::new())),
                last_get_args: Arc::new(Mutex::new(None)),
            }
        }

        /// Return and clear the last `CalendarEventGetArgs` recorded by
        /// `get_calendar_events`. `None` if the method has not been called
        /// since the last `take`.
        #[allow(dead_code)]
        pub fn take_last_get_args(&self) -> Option<CalendarEventGetArgs> {
            self.last_get_args.lock().unwrap().take()
        }

        pub fn new_with_account(account_id: &str) -> Self {
            let b = Self::new();
            b.state
                .lock()
                .unwrap()
                .insert(account_id.to_owned(), AccountState::default());
            b
        }

        /// Create an account where `calendar_id` is registered as having events.
        pub fn new_with_account_and_events(account_id: &str, calendar_id: &str) -> Self {
            let b = Self::new();
            let mut state = AccountState::default();
            state.calendars_with_events.insert(Id::from(calendar_id));
            // Register the calendar as a destroyable object so destroy succeeds when
            // onDestroyRemoveEvents=true.
            state
                .objects
                .entry("Calendar".to_owned())
                .or_default()
                .insert(
                    Id::from(calendar_id),
                    serde_json::json!({"id": calendar_id}),
                );
            b.state.lock().unwrap().insert(account_id.to_owned(), state);
            b
        }

        #[allow(dead_code)]
        pub fn add_notification(&mut self, account_id: &str, notif_id: &str) {
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.objects
                .entry("CalendarEventNotification".to_owned())
                .or_default()
                .insert(Id::from(notif_id), serde_json::json!({ "id": notif_id }));
        }

        /// Seed a serialized object into the mock store for `get_objects` retrieval.
        ///
        /// The `type_name` must match `O::TYPE_NAME` for the object type being
        /// tested (e.g. `"CalendarEvent"`).
        #[allow(dead_code)]
        pub fn add_object(
            &mut self,
            account_id: &str,
            type_name: &str,
            id: &str,
            value: serde_json::Value,
        ) {
            self.seed_object(account_id, type_name, id, value);
        }

        /// Seed a serialized object into the store via interior mutability.
        ///
        /// Like [`add_object`](Self::add_object) but takes `&self`, so callers
        /// already holding an `Arc<MockBackend>` (e.g. dispatcher-driven tests)
        /// can seed without unwrapping the `Arc`.
        #[allow(dead_code)]
        pub fn seed_object(
            &self,
            account_id: &str,
            type_name: &str,
            id: &str,
            value: serde_json::Value,
        ) {
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.objects
                .entry(type_name.to_owned())
                .or_default()
                .insert(Id::from(id), value);
        }

        /// Set the mock's recorded "default Calendar" id for an account.
        ///
        /// Used by tests exercising §4.3 `onSuccessSetIsDefault` swap
        /// semantics — the previously-default calendar must appear in
        /// `updated` with `isDefault: false`.
        #[allow(dead_code)]
        pub fn set_default_calendar_for_test(&self, account_id: &str, default_id: Option<&str>) {
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.default_calendar = default_id.map(Id::from);
        }

        /// Read the mock's recorded "default Calendar" id for an account.
        #[allow(dead_code)]
        pub fn get_default_calendar_for_test(&self, account_id: &str) -> Option<Id> {
            let guard = self.state.lock().unwrap();
            guard
                .get(account_id)
                .and_then(|acct| acct.default_calendar.clone())
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
            let type_name = O::TYPE_NAME;
            let guard = self.state.lock().unwrap();
            let Some(acct) = guard.get(account_id.as_ref()) else {
                return Ok((vec![], vec![]));
            };
            let Some(store) = acct.objects.get(type_name) else {
                // No objects of this type → all ids are not-found.
                let not_found = match ids {
                    Some(id_slice) => id_slice.to_vec(),
                    None => vec![],
                };
                return Ok((vec![], not_found));
            };
            match ids {
                None => {
                    // Return all objects of this type.
                    let mut found = Vec::new();
                    for v in store.values() {
                        if let Ok(obj) = O::deserialize(v) {
                            found.push(obj);
                        }
                    }
                    Ok((found, vec![]))
                }
                Some(id_slice) => {
                    let mut found = Vec::new();
                    let mut not_found = Vec::new();
                    for id in id_slice {
                        match store.get(id) {
                            Some(v) => match O::deserialize(v) {
                                Ok(obj) => found.push(obj),
                                Err(_) => not_found.push(id.clone()),
                            },
                            None => not_found.push(id.clone()),
                        }
                    }
                    Ok((found, not_found))
                }
            }
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
            account_id: &Id,
            filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _limit: Option<u64>,
            _position: i64,
        ) -> Result<QueryResult, Self::Error> {
            // Minimal filter implementation for the cleanup test path:
            // for CalendarEvent with an `inCalendar` filter, return ids of
            // stored events whose calendarIds map contains that calendar id.
            // Other queries return empty (this mock isn't a full query engine).
            let type_name = O::TYPE_NAME;
            if type_name != "CalendarEvent" {
                return Ok(QueryResult::new(
                    vec![],
                    0,
                    Some(0),
                    State::from("0"),
                    false,
                ));
            }
            let in_calendar: Option<String> = filter
                .and_then(|f| serde_json::to_value(f).ok())
                .and_then(|v| {
                    v.get("inCalendar")
                        .and_then(|c| c.as_str())
                        .map(String::from)
                });
            let guard = self.state.lock().unwrap();
            let Some(acct) = guard.get(account_id.as_ref()) else {
                return Ok(QueryResult::new(
                    vec![],
                    0,
                    Some(0),
                    State::from("0"),
                    false,
                ));
            };
            let Some(store) = acct.objects.get("CalendarEvent") else {
                return Ok(QueryResult::new(
                    vec![],
                    0,
                    Some(0),
                    State::from("0"),
                    false,
                ));
            };
            let mut ids: Vec<Id> = Vec::new();
            for (id, value) in store.iter() {
                let matches = match &in_calendar {
                    None => true,
                    Some(target) => value
                        .get("calendarIds")
                        .and_then(|v| v.as_object())
                        .map(|m| m.contains_key(target))
                        .unwrap_or(false),
                };
                if matches {
                    ids.push(id.clone());
                }
            }
            let total = ids.len() as u64;
            Ok(QueryResult::new(
                ids,
                0,
                Some(total),
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

    impl CalendarsBackend for MockBackend {
        async fn create_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            _create_id: &str,
            obj: O,
        ) -> Result<(Id, O), BackendSetError<Self::Error>> {
            // Persist the created object so subsequent operations
            // (set_default_*, get, update, destroy) can find it. The mock
            // assigns a unique id per call within a (account, type) namespace
            // so creation-reference tests have a stable, predictable target.
            let type_name = O::TYPE_NAME;
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.as_ref().to_owned()).or_default();
            let store = acct.objects.entry(type_name.to_owned()).or_default();
            let id = Id::from(format!("mock-{}-{}", type_name, store.len() + 1));
            // Stamp the assigned id into the serialized form so:
            // 1. The handler's response shows the real id (not "placeholder").
            // 2. Subsequent get_objects calls return the object with id set.
            // 3. The onSuccessSetIsDefault creation-ref path can resolve
            //    "#createId" → assigned id via the response's "id" field.
            let mut as_json = serde_json::to_value(&obj).map_err(|e| {
                BackendSetError::Other(MockError(format!("create serialize failed: {e}")))
            })?;
            if let Some(map) = as_json.as_object_mut() {
                map.insert(
                    "id".to_owned(),
                    serde_json::Value::String(id.as_ref().to_owned()),
                );
            }
            store.insert(id.clone(), as_json.clone());
            // Re-deserialize so the returned O carries the assigned id.
            // If deserialization fails (shouldn't, since we just serialized
            // the same shape), fall back to returning the original obj —
            // the response will still show the assigned id via the JSON
            // serialization in `created`.
            let typed: O = serde_json::from_value(as_json).unwrap_or(obj);
            Ok((id, typed))
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
            patch: O::Patch,
        ) -> Result<Option<O>, BackendSetError<Self::Error>> {
            // Only CalendarEvent updates are implemented for the cleanup test
            // path. Other types still return Forbidden (matching prior behaviour
            // — no inline test relies on a successful update via MockBackend).
            let type_name = O::TYPE_NAME;
            if type_name != "CalendarEvent" {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::Forbidden,
                )));
            }
            // Apply a JMAP PatchObject (RFC 8620 §5.3): top-level keys may
            // contain "/" separators denoting nested paths, and a value of
            // null at a leaf removes that leaf.
            let patch_val: serde_json::Value = serde_json::to_value(&patch).map_err(|e| {
                BackendSetError::Other(MockError(format!("patch serialize failed: {e}")))
            })?;
            let mut guard = self.state.lock().unwrap();
            let Some(acct) = guard.get_mut(account_id.as_ref()) else {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )));
            };
            let Some(store) = acct.objects.get_mut("CalendarEvent") else {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )));
            };
            let Some(stored) = store.get_mut(id) else {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )));
            };
            let Some(patch_map) = patch_val.as_object() else {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::InvalidPatch,
                )));
            };
            for (path, value) in patch_map {
                apply_patch_path(stored, path, value);
            }
            // Echo None: this mock does not surface server-set property deltas.
            Ok(None)
        }

        async fn destroy_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
        ) -> Result<(), BackendSetError<Self::Error>> {
            let type_name = O::TYPE_NAME;
            let mut guard = self.state.lock().unwrap();
            if let Some(acct) = guard.get_mut(account_id.as_ref()) {
                // Also clear from calendars_with_events if it's a Calendar.
                acct.calendars_with_events.remove(id);
                if let Some(store) = acct.objects.get_mut(type_name) {
                    if store.remove(id).is_some() {
                        return Ok(());
                    }
                }
            }
            Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )))
        }

        fn supports_type<O: JmapObject>(&self) -> bool {
            true
        }

        async fn calendar_has_events(
            &self,
            _caller: &(),
            account_id: &Id,
            calendar_id: &Id,
        ) -> bool {
            let guard = self.state.lock().unwrap();
            guard
                .get(account_id.as_ref())
                .map(|acct| acct.calendars_with_events.contains(calendar_id))
                .unwrap_or(false)
        }

        // Scheduling-aware overrides: the mock has no iTIP delivery support,
        // so any request to send scheduling messages produces the
        // noSupportedScheduleMethods SetError per draft-ietf-jmap-calendars-26
        // §5.9, §10.7.2. When sendSchedulingMessages is false the mock falls
        // through to the generic create/update/destroy_object path used by
        // every other test in this file.
        async fn create_calendar_event(
            &self,
            caller: &(),
            account_id: &Id,
            create_id: &str,
            event: jmap_calendars_types::CalendarEvent,
            args: &CalendarEventSetArgs,
        ) -> Result<(Id, jmap_calendars_types::CalendarEvent), BackendSetError<Self::Error>>
        {
            if args.send_scheduling_messages {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::custom("noSupportedScheduleMethods"),
                )));
            }
            self.create_object::<jmap_calendars_types::CalendarEvent>(
                caller, account_id, create_id, event,
            )
            .await
        }

        async fn update_calendar_event(
            &self,
            caller: &(),
            account_id: &Id,
            id: &Id,
            patch: jmap_types::PatchObject,
            args: &CalendarEventSetArgs,
        ) -> Result<Option<jmap_calendars_types::CalendarEvent>, BackendSetError<Self::Error>>
        {
            if args.send_scheduling_messages {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::custom("noSupportedScheduleMethods"),
                )));
            }
            self.update_object::<jmap_calendars_types::CalendarEvent>(caller, account_id, id, patch)
                .await
        }

        async fn destroy_calendar_event(
            &self,
            caller: &(),
            account_id: &Id,
            id: &Id,
            args: &CalendarEventSetArgs,
        ) -> Result<(), BackendSetError<Self::Error>> {
            if args.send_scheduling_messages {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::custom("noSupportedScheduleMethods"),
                )));
            }
            self.destroy_object::<jmap_calendars_types::CalendarEvent>(caller, account_id, id)
                .await
        }

        // §5.7 get_calendar_events: record args for test assertions and
        // delegate to get_objects. Tests inspect take_last_get_args() to
        // verify the handler parsed and forwarded each extra correctly.
        async fn get_calendar_events(
            &self,
            caller: &(),
            account_id: &Id,
            ids: Option<&[Id]>,
            properties: Option<&[String]>,
            args: &CalendarEventGetArgs,
        ) -> Result<(Vec<jmap_calendars_types::CalendarEvent>, Vec<Id>), Self::Error> {
            *self.last_get_args.lock().unwrap() = Some(args.clone());
            self.get_objects::<jmap_calendars_types::CalendarEvent>(
                caller, account_id, ids, properties,
            )
            .await
        }

        // §5.7 timeZone: echo the tz_hint back into utcStart so tests can
        // verify the handler forwarded the timeZone arg (the previous
        // implementation hardcoded None for tz_hint, which was a bug).
        async fn compute_utc_times(
            &self,
            _caller: &(),
            _account_id: &Id,
            _event: &jmap_calendars_types::CalendarEvent,
            tz_hint: Option<&str>,
        ) -> (Option<jmap_types::UTCDate>, Option<jmap_types::UTCDate>) {
            let tz = tz_hint.unwrap_or("Etc/UTC");
            (
                Some(jmap_types::UTCDate::from(format!("tz={tz}"))),
                Some(jmap_types::UTCDate::from(format!("tz={tz}"))),
            )
        }

        // §5.11 expandRecurrences: drive the two new error paths via
        // sentinel filter values so tests can trigger them deterministically:
        // - in_calendar = "trigger-too-large" → ExpandDurationTooLarge
        // - in_calendar = "trigger-cannot-calc" → CannotCalculateOccurrences
        // Any other input falls through to the default (which delegates to
        // query_objects). The args themselves are forwarded for inspection
        // in tests but otherwise ignored by this mock.
        async fn query_calendar_events(
            &self,
            caller: &(),
            account_id: &Id,
            filter: Option<&jmap_calendars_types::CalendarEventFilterCondition>,
            sort: Option<&[jmap_calendars_types::CalendarEventComparator]>,
            limit: Option<u64>,
            position: i64,
            args: &CalendarEventQueryArgs,
        ) -> Result<jmap_server::QueryResult, QueryCalendarEventsError<Self::Error>> {
            if let Some(f) = filter {
                if let Some(in_cal) = f.in_calendar.as_ref() {
                    match in_cal.as_ref() {
                        "trigger-too-large" => {
                            return Err(QueryCalendarEventsError::ExpandDurationTooLarge);
                        }
                        "trigger-cannot-calc" => {
                            return Err(QueryCalendarEventsError::CannotCalculateOccurrences);
                        }
                        _ => {}
                    }
                }
            }
            let _ = args; // happy path delegates to the generic backend
            self.query_objects::<jmap_calendars_types::CalendarEvent>(
                caller, account_id, filter, sort, limit, position,
            )
            .await
            .map_err(QueryCalendarEventsError::Other)
        }

        // §4.3 onSuccessSetIsDefault for Calendar/set: track the default in
        // AccountState. If the requested calendar id does not exist in the
        // store, the spec says the change MUST be silently ignored — model
        // this by returning new_default=None.
        async fn set_default_calendar(
            &self,
            _caller: &(),
            account_id: &Id,
            calendar_id: &Id,
        ) -> Result<crate::backend::SetDefaultResult, Self::Error> {
            let mut guard = self.state.lock().unwrap();
            let Some(acct) = guard.get_mut(account_id.as_ref()) else {
                return Ok(crate::backend::SetDefaultResult::default());
            };
            let exists = acct
                .objects
                .get("Calendar")
                .map(|m| m.contains_key(calendar_id))
                .unwrap_or(false);
            if !exists {
                return Ok(crate::backend::SetDefaultResult::default());
            }
            let previous = acct.default_calendar.replace(calendar_id.clone());
            Ok(crate::backend::SetDefaultResult {
                new_default: Some(calendar_id.clone()),
                previous_default: previous,
            })
        }

        // §3.3 onSuccessSetIsDefault for ParticipantIdentity/set: same
        // contract as set_default_calendar but for ParticipantIdentity.
        async fn set_default_participant_identity(
            &self,
            _caller: &(),
            account_id: &Id,
            identity_id: &Id,
        ) -> Result<crate::backend::SetDefaultResult, Self::Error> {
            let mut guard = self.state.lock().unwrap();
            let Some(acct) = guard.get_mut(account_id.as_ref()) else {
                return Ok(crate::backend::SetDefaultResult::default());
            };
            let exists = acct
                .objects
                .get("ParticipantIdentity")
                .map(|m| m.contains_key(identity_id))
                .unwrap_or(false);
            if !exists {
                return Ok(crate::backend::SetDefaultResult::default());
            }
            let previous = acct
                .default_participant_identity
                .replace(identity_id.clone());
            Ok(crate::backend::SetDefaultResult {
                new_default: Some(identity_id.clone()),
                previous_default: previous,
            })
        }
    }

    /// Apply a single JMAP PatchObject path to a stored JSON object.
    ///
    /// `path` is a "/"-separated sequence of property names per RFC 8620 §5.3.
    /// A `null` value at the leaf removes that leaf (PatchObject semantics);
    /// any other value sets it. Missing intermediate objects are created.
    /// Used only by MockBackend to support the onDestroyRemoveEvents test path.
    fn apply_patch_path(target: &mut serde_json::Value, path: &str, value: &serde_json::Value) {
        let parts: Vec<&str> = path.split('/').collect();
        let mut cursor = target;
        // Walk to the parent of the leaf, creating intermediate objects as needed.
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !cursor.is_object() {
                *cursor = serde_json::Value::Object(serde_json::Map::new());
            }
            let map = cursor.as_object_mut().expect("cursor is object");
            cursor = map
                .entry((*part).to_owned())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        }
        let leaf_key = parts.last().copied().unwrap_or("");
        if !cursor.is_object() {
            *cursor = serde_json::Value::Object(serde_json::Map::new());
        }
        let map = cursor.as_object_mut().expect("cursor is object");
        if value.is_null() {
            map.remove(leaf_key);
        } else {
            map.insert(leaf_key.to_owned(), value.clone());
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
            vec!["urn:ietf:params:jmap:calendars".into()],
            vec![(method.into(), args, call_id.into())],
            None,
        )
    }

    /// Oracle: register_calendars_handlers registers all 20 Calendars methods.
    ///
    /// Verification: each method returns a non-`unknownMethod` response when
    /// dispatched with a valid account.
    #[tokio::test]
    async fn registers_all_20_methods() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let methods: &[(&str, serde_json::Value)] = &[
            ("Calendar/get", json!({"accountId": "acc1", "ids": null})),
            (
                "Calendar/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            ("Calendar/set", json!({"accountId": "acc1", "destroy": []})),
            (
                "CalendarEvent/get",
                json!({"accountId": "acc1", "ids": null}),
            ),
            (
                "CalendarEvent/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            (
                "CalendarEvent/set",
                json!({"accountId": "acc1", "destroy": []}),
            ),
            (
                "CalendarEvent/copy",
                json!({"accountId": "acc1", "fromAccountId": "acc1", "create": {}}),
            ),
            (
                "CalendarEvent/query",
                json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "CalendarEvent/queryChanges",
                json!({"accountId": "acc1", "sinceQueryState": "0"}),
            ),
            (
                "CalendarEventNotification/get",
                json!({"accountId": "acc1", "ids": null}),
            ),
            (
                "CalendarEventNotification/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            (
                "CalendarEventNotification/set",
                json!({"accountId": "acc1", "destroy": []}),
            ),
            (
                "CalendarEventNotification/query",
                json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "CalendarEventNotification/queryChanges",
                json!({"accountId": "acc1", "sinceQueryState": "0"}),
            ),
            (
                "ParticipantIdentity/get",
                json!({"accountId": "acc1", "ids": null}),
            ),
            (
                "ParticipantIdentity/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            (
                "ParticipantIdentity/set",
                json!({"accountId": "acc1", "destroy": []}),
            ),
            (
                "CalendarEvent/parse",
                json!({"accountId": "acc1", "blobIds": []}),
            ),
            (
                "Principal/getAvailability",
                json!({
                    "accountId": "acc1",
                    "id": "p1",
                    "utcStart": "2024-06-15T09:00:00Z",
                    "utcEnd": "2024-06-15T10:00:00Z"
                }),
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

    /// Oracle: CalendarEventNotification/set with create entries → notCreated
    /// contains `forbidden` for every create entry; no top-level error.
    #[tokio::test]
    async fn calendar_event_notification_set_create_returns_forbidden() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEventNotification/set",
            json!({
                "accountId": "acc1",
                "create": {
                    "c1": {
                        "id": "n1", "created": "2024-01-01T00:00:00Z",
                        "changedBy": { "name": "A", "email": null, "principalId": null },
                        "type": "created", "calendarEventId": "ev1", "event": {}
                    }
                }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notCreated"]["c1"]["type"], "forbidden",
            "create must be forbidden: {args}"
        );
    }

    /// Oracle: Calendar/set destroy with calendar that has events and
    /// onDestroyRemoveEvents absent (default false) → notDestroyed with
    /// `calendarHasEvent`.
    #[tokio::test]
    async fn calendar_set_destroy_with_events_returns_calendar_has_events() {
        let backend = Arc::new(MockBackend::new_with_account_and_events("acc1", "cal1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Calendar/set",
            json!({
                "accountId": "acc1",
                "destroy": ["cal1"]
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notDestroyed"]["cal1"]["type"], "calendarHasEvent",
            "must produce calendarHasEvent: {args}"
        );
    }

    /// Oracle: CalendarEventNotification/set with only destroy:[] returns
    /// a valid empty /set response.
    #[tokio::test]
    async fn calendar_event_notification_set_empty_destroy_is_valid() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEventNotification/set",
            json!({"accountId": "acc1", "destroy": []}),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be an error response: {args}"
        );
        assert_eq!(args["accountId"], "acc1");
    }

    /// Oracle: RFC 8620 §5.4 — `fromAccountId` that does not exist returns a
    /// top-level `fromAccountNotFound` error (via dispatcher).
    #[tokio::test]
    async fn copy_missing_from_account_returns_from_account_not_found() {
        let backend = Arc::new(MockBackend::new_with_account("dst"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/copy",
            json!({
                "accountId": "dst",
                "fromAccountId": "no-such-account",
                "create": {}
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert_eq!(
            args["type"], "fromAccountNotFound",
            "must be fromAccountNotFound: {args}"
        );
    }

    /// Oracle: RFC 8620 §5.4 — create entry with no `"id"` field → `notCreated`
    /// contains `invalidProperties` citing `["id"]` (via dispatcher).
    #[tokio::test]
    async fn copy_missing_source_id_returns_invalid_properties() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/copy",
            json!({
                "accountId": "acc1",
                "fromAccountId": "acc1",
                "create": {
                    "c1": { "calendarIds": { "cal1": true } }
                }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notCreated"]["c1"]["type"], "invalidProperties",
            "must be invalidProperties: {args}"
        );
        assert_eq!(
            args["notCreated"]["c1"]["properties"][0], "id",
            "must cite 'id' in properties: {args}"
        );
    }

    /// Oracle: RFC 8620 §5.4 — source id that does not exist in `fromAccountId`
    /// → `notCreated` contains `{"type":"notFound"}` (via dispatcher).
    #[tokio::test]
    async fn copy_source_event_not_found_returns_not_found() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/copy",
            json!({
                "accountId": "acc1",
                "fromAccountId": "acc1",
                "create": {
                    "c1": { "id": "no-such-event" }
                }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notCreated"]["c1"]["type"], "notFound",
            "must be notFound: {args}"
        );
    }

    /// Oracle: §5.13 — default backend puts unrecognised blob ids in
    /// `notParsable`; `parsed` is null (via dispatcher).
    #[tokio::test]
    async fn parse_unknown_blob_returns_not_parsable() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/parse",
            json!({
                "accountId": "acc1",
                "blobIds": ["unknown-blob"]
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notParsable"],
            json!(["unknown-blob"]),
            "blob must appear in notParsable: {args}"
        );
        assert_eq!(
            args["parsed"],
            serde_json::Value::Null,
            "parsed must be null: {args}"
        );
    }

    /// Oracle: §2.2 — default backend returns an empty `list` (via dispatcher).
    #[tokio::test]
    async fn get_availability_returns_empty_list() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Principal/getAvailability",
            json!({
                "accountId": "acc1",
                "id": "principal1",
                "utcStart": "2024-06-15T09:00:00Z",
                "utcEnd": "2024-06-15T10:00:00Z"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(args["accountId"], "acc1");
        assert!(
            args["list"].as_array().unwrap().is_empty(),
            "list must be empty: {args}"
        );
    }

    /// Oracle: §2.2 — missing `id` argument returns `invalidArguments`
    /// (via dispatcher).
    #[tokio::test]
    async fn get_availability_missing_id_returns_error() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Principal/getAvailability",
            json!({
                "accountId": "acc1",
                "utcStart": "2024-06-15T09:00:00Z",
                "utcEnd": "2024-06-15T10:00:00Z"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert_eq!(
            args["type"], "invalidArguments",
            "must be invalidArguments for missing id: {args}"
        );
    }

    /// Oracle: §5.9 — creating an event with both `utcStart` and `start`
    /// produces `notCreated` with `invalidProperties` (via dispatcher).
    #[tokio::test]
    async fn set_with_utc_start_and_start_returns_invalid_properties() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/set",
            json!({
                "accountId": "acc1",
                "create": {
                    "c1": {
                        "calendarIds": { "cal1": true },
                        "start": "2024-06-01T10:00:00",
                        "utcStart": "2024-06-01T08:00:00Z"
                    }
                }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notCreated"]["c1"]["type"], "invalidProperties",
            "must be invalidProperties: {args}"
        );
        let props = args["notCreated"]["c1"]["properties"].as_array().unwrap();
        let prop_strs: Vec<&str> = props.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            prop_strs.contains(&"utcStart") && prop_strs.contains(&"start"),
            "must cite utcStart and start: {args}"
        );
    }

    // -----------------------------------------------------------------------
    // sendSchedulingMessages / noSupportedScheduleMethods (§5.9, §10.7.2)
    //
    // The MockBackend overrides create_calendar_event, update_calendar_event,
    // and destroy_calendar_event to return SetError(noSupportedScheduleMethods)
    // whenever args.send_scheduling_messages is true (i.e. behaves as a
    // backend with no iTIP delivery support). When the flag is false (the
    // default), it falls through to the generic create/update/destroy_object
    // path. These tests pin both ends of the contract.
    // -----------------------------------------------------------------------

    /// Oracle: §5.9 + §10.7.2 — `CalendarEvent/set` create with
    /// `sendSchedulingMessages: true` MUST surface a backend-issued
    /// `noSupportedScheduleMethods` SetError under `notCreated.<createId>`,
    /// not a top-level method error and not `serverFail`.
    #[tokio::test]
    async fn set_create_with_scheduling_no_supported_methods() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/set",
            json!({
                "accountId": "acc1",
                "sendSchedulingMessages": true,
                "create": {
                    "c1": {
                        "calendarIds": { "cal1": true },
                        "title": "Team meeting",
                        "start": "2024-06-15T10:00:00",
                        "duration": "PT1H"
                    }
                }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notCreated"]["c1"]["type"], "noSupportedScheduleMethods",
            "create must be rejected with noSupportedScheduleMethods: {args}"
        );
        // No success entries on this create.
        assert_eq!(
            args["created"],
            serde_json::Value::Null,
            "created must be null when scheduling fails: {args}"
        );
    }

    /// Oracle: §5.9 + §10.7.2 — `CalendarEvent/set` update with
    /// `sendSchedulingMessages: true` MUST surface
    /// `noSupportedScheduleMethods` under `notUpdated.<id>`.
    ///
    /// The mock fails before the patch is applied, so no pre-existing event
    /// is needed to exercise the wrapper path.
    #[tokio::test]
    async fn set_update_with_scheduling_no_supported_methods() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/set",
            json!({
                "accountId": "acc1",
                "sendSchedulingMessages": true,
                "update": {
                    "ev1": { "title": "Renamed meeting" }
                }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notUpdated"]["ev1"]["type"], "noSupportedScheduleMethods",
            "update must be rejected with noSupportedScheduleMethods: {args}"
        );
    }

    /// Oracle: §5.9 + §10.7.2 — `CalendarEvent/set` destroy with
    /// `sendSchedulingMessages: true` MUST surface
    /// `noSupportedScheduleMethods` under `notDestroyed.<id>` (CANCEL
    /// scheduling messages cannot be sent).
    #[tokio::test]
    async fn set_destroy_with_scheduling_no_supported_methods() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/set",
            json!({
                "accountId": "acc1",
                "sendSchedulingMessages": true,
                "destroy": ["ev1"]
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notDestroyed"]["ev1"]["type"], "noSupportedScheduleMethods",
            "destroy must be rejected with noSupportedScheduleMethods: {args}"
        );
    }

    /// Oracle: §5.9 — `sendSchedulingMessages` defaults to `false`. Without
    /// the arg, the create succeeds and the mock never produces a scheduling
    /// error, proving the flag is parsed as `false` when absent.
    #[tokio::test]
    async fn set_create_without_scheduling_succeeds() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/set",
            json!({
                "accountId": "acc1",
                "create": {
                    "c1": {
                        "calendarIds": { "cal1": true },
                        "title": "Team meeting",
                        "start": "2024-06-15T10:00:00",
                        "duration": "PT1H"
                    }
                }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert!(
            args["created"].get("c1").is_some(),
            "create must succeed when sendSchedulingMessages is absent: {args}"
        );
        assert_eq!(
            args["notCreated"],
            serde_json::Value::Null,
            "no notCreated entries when scheduling not requested: {args}"
        );
    }

    /// Oracle: §5.9 — explicit `sendSchedulingMessages: false` is equivalent
    /// to the default and the create succeeds. Pinning this guards against
    /// a future regression where any non-true value (false, null, missing)
    /// could be misparsed as true.
    #[tokio::test]
    async fn set_create_with_scheduling_false_succeeds() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/set",
            json!({
                "accountId": "acc1",
                "sendSchedulingMessages": false,
                "create": {
                    "c1": {
                        "calendarIds": { "cal1": true },
                        "title": "Team meeting",
                        "start": "2024-06-15T10:00:00",
                        "duration": "PT1H"
                    }
                }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert!(
            args["created"].get("c1").is_some(),
            "create must succeed when sendSchedulingMessages is false: {args}"
        );
    }

    // -----------------------------------------------------------------------
    // onSuccessSetIsDefault (§3.3 ParticipantIdentity, §4.3 Calendar)
    //
    // The MockBackend tracks `default_calendar` and
    // `default_participant_identity` per account, returning them in
    // SetDefaultResult.previous_default. The lookup ignores ids that don't
    // exist in the store (silent ignore per spec). These tests cover:
    // (a) creation-reference resolution (#c1 → assigned id),
    // (b) existing-id with previous default swap,
    // (c) unknown id silently ignored, no response change,
    // (d) default change skipped when any CRUD op failed,
    // (e) ParticipantIdentity/set parallel path.
    // -----------------------------------------------------------------------

    /// Oracle: §4.3 — `Calendar/set` with `onSuccessSetIsDefault: "#c1"`
    /// resolves the `#`-prefix as a creation reference and applies
    /// `isDefault: true` to the corresponding `created` entry. The mock
    /// has no previous default, so no `updated` entry is emitted.
    #[tokio::test]
    async fn calendar_set_default_via_creation_reference() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        // Calendar has many required (non-Option) fields per
        // jmap_calendars_types::Calendar — the create object must carry all
        // of them or the handler rejects with invalidProperties before any
        // backend call. Spec example values (§4 type definition).
        let req = single_call(
            "Calendar/set",
            json!({
                "accountId": "acc1",
                "create": {
                    "c1": {
                        "name": "My new calendar",
                        "sortOrder": 0,
                        "isSubscribed": true,
                        "isVisible": true,
                        "isDefault": false,
                        "includeInAvailability": "all",
                        "myRights": {
                            "mayReadFreeBusy": true,
                            "mayReadItems": true,
                            "mayWriteAll": true,
                            "mayWriteOwn": true,
                            "mayUpdatePrivate": true,
                            "mayRSVP": true,
                            "mayShare": true,
                            "mayDelete": true
                        }
                    }
                },
                "onSuccessSetIsDefault": "#c1"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["created"]["c1"]["isDefault"], true,
            "created entry must reflect isDefault:true: {args}"
        );
        assert_eq!(
            args["updated"],
            serde_json::Value::Null,
            "no previous default → no updated entry: {args}"
        );
    }

    /// Oracle: §4.3 — `Calendar/set` with `onSuccessSetIsDefault: <existing id>`
    /// emits an `updated.<id>` entry with `isDefault: true`. When a previous
    /// default exists, that calendar appears in `updated` with
    /// `isDefault: false` (the atomic-swap response contract).
    #[tokio::test]
    async fn calendar_set_default_swaps_previous_default() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        // Seed two calendars, mark cal-A as the current default.
        backend.seed_object("acc1", "Calendar", "cal-A", json!({"id": "cal-A"}));
        backend.seed_object("acc1", "Calendar", "cal-B", json!({"id": "cal-B"}));
        backend.set_default_calendar_for_test("acc1", Some("cal-A"));

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Calendar/set",
            json!({
                "accountId": "acc1",
                "onSuccessSetIsDefault": "cal-B"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["updated"]["cal-B"]["isDefault"], true,
            "new default must appear in updated with isDefault:true: {args}"
        );
        assert_eq!(
            args["updated"]["cal-A"]["isDefault"], false,
            "previous default must appear in updated with isDefault:false: {args}"
        );
    }

    /// Oracle: §4.3 — when `onSuccessSetIsDefault` references an id that
    /// does not exist (or the change is forbidden), the server MUST silently
    /// ignore the request. No response state changes; no top-level error.
    #[tokio::test]
    async fn calendar_set_default_unknown_id_silently_ignored() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Calendar/set",
            json!({
                "accountId": "acc1",
                "onSuccessSetIsDefault": "no-such-calendar"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["updated"],
            serde_json::Value::Null,
            "no updated entries when target id is unknown: {args}"
        );
        assert_eq!(
            args["created"],
            serde_json::Value::Null,
            "no created entries: {args}"
        );
    }

    /// Oracle: §4.3 — "if all creates, updates and destroys (if any) succeed
    /// without error" guard: when ANY CRUD op failed, the default change
    /// MUST NOT be applied even though other entries may have succeeded.
    /// Here a destroy of an unknown id fails, so the default-set is skipped.
    #[tokio::test]
    async fn calendar_set_default_skipped_when_destroy_fails() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        // Seed cal-B so it WOULD be a valid default target.
        backend.seed_object("acc1", "Calendar", "cal-B", json!({"id": "cal-B"}));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Calendar/set",
            json!({
                "accountId": "acc1",
                "destroy": ["does-not-exist"],
                "onSuccessSetIsDefault": "cal-B"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert!(
            args["notDestroyed"].get("does-not-exist").is_some(),
            "destroy of unknown id must produce notDestroyed entry: {args}"
        );
        assert_eq!(
            args["updated"],
            serde_json::Value::Null,
            "default change must be skipped when any op failed: {args}"
        );
        // Verify the mock's stored default is still unchanged.
        assert!(
            backend.get_default_calendar_for_test("acc1").is_none(),
            "backend default must not have been updated"
        );
    }

    /// Oracle: §3.3 — `ParticipantIdentity/set` with `onSuccessSetIsDefault`
    /// against an existing identity emits the `updated.<id>` entry with
    /// `isDefault: true`. Mirror of the Calendar/set path through the
    /// `set_default_participant_identity` backend method.
    #[tokio::test]
    async fn participant_identity_set_default_existing_id() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        // Seed an identity to be made default.
        backend.seed_object("acc1", "ParticipantIdentity", "pi-1", json!({"id": "pi-1"}));

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "ParticipantIdentity/set",
            json!({
                "accountId": "acc1",
                "onSuccessSetIsDefault": "pi-1"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["updated"]["pi-1"]["isDefault"], true,
            "ParticipantIdentity/set default must emit updated entry: {args}"
        );
    }

    // -----------------------------------------------------------------------
    // CalendarEvent/query §5.11 expandRecurrences / timeZone
    //
    // The MockBackend's query_calendar_events override returns the §10.7.3
    // / §10.7.4 errors when the filter's in_calendar matches a sentinel
    // value, otherwise it falls through to the generic query_objects path.
    // -----------------------------------------------------------------------

    /// Oracle: §5.11 — `expandRecurrences: true` without `before` AND `after`
    /// in the filter MUST be rejected with `invalidArguments` BEFORE any
    /// backend call. The default value (false) is exercised by every other
    /// test in this file.
    #[tokio::test]
    async fn calendar_event_query_expand_recurrences_requires_before_and_after() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        // Filter with only `before` and no `after` → rejected.
        let req = single_call(
            "CalendarEvent/query",
            json!({
                "accountId": "acc1",
                "filter": { "before": "2025-01-01T00:00:00" },
                "expandRecurrences": true
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert_eq!(
            args["type"], "invalidArguments",
            "expandRecurrences without before+after must be invalidArguments: {args}"
        );
    }

    /// Oracle: §5.11 — `expandRecurrences: true` with no filter at all is
    /// also rejected with `invalidArguments`.
    #[tokio::test]
    async fn calendar_event_query_expand_recurrences_requires_filter() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/query",
            json!({
                "accountId": "acc1",
                "filter": null,
                "expandRecurrences": true
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert_eq!(
            args["type"], "invalidArguments",
            "expandRecurrences with null filter must be invalidArguments: {args}"
        );
    }

    /// Oracle: §10.7.3 — when the backend signals `ExpandDurationTooLarge`,
    /// the handler MUST return a method-level `expandDurationTooLarge` error
    /// (not `serverFail`).
    #[tokio::test]
    async fn calendar_event_query_expand_duration_too_large() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/query",
            json!({
                "accountId": "acc1",
                "filter": {
                    "inCalendar": "trigger-too-large",
                    "before": "2030-01-01T00:00:00",
                    "after":  "2020-01-01T00:00:00"
                },
                "expandRecurrences": true
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert_eq!(
            args["type"], "expandDurationTooLarge",
            "backend ExpandDurationTooLarge must surface as expandDurationTooLarge: {args}"
        );
    }

    /// Oracle: §10.7.4 — when the backend signals
    /// `CannotCalculateOccurrences`, the handler MUST return a method-level
    /// `cannotCalculateOccurrences` error (not `serverFail`).
    #[tokio::test]
    async fn calendar_event_query_cannot_calculate_occurrences() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/query",
            json!({
                "accountId": "acc1",
                "filter": {
                    "inCalendar": "trigger-cannot-calc",
                    "before": "2030-01-01T00:00:00",
                    "after":  "2020-01-01T00:00:00"
                },
                "expandRecurrences": true
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert_eq!(
            args["type"], "cannotCalculateOccurrences",
            "backend CannotCalculateOccurrences must surface as cannotCalculateOccurrences: {args}"
        );
    }

    /// Oracle: §5.11 — happy path with `expandRecurrences: true` and a valid
    /// filter (both `before` and `after`) reaches the backend and returns
    /// a normal /query response envelope. The mock returns an empty result
    /// since there are no events in the store.
    #[tokio::test]
    async fn calendar_event_query_expand_recurrences_happy_path() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/query",
            json!({
                "accountId": "acc1",
                "filter": {
                    "before": "2025-12-31T23:59:59",
                    "after":  "2025-01-01T00:00:00"
                },
                "expandRecurrences": true,
                "timeZone": "America/New_York"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(args["accountId"], "acc1");
        assert!(
            args["ids"].as_array().unwrap().is_empty(),
            "no events seeded → ids must be empty: {args}"
        );
    }

    // -----------------------------------------------------------------------
    // CalendarEvent/get §5.7 extra args
    //
    // The MockBackend's get_calendar_events records the received
    // CalendarEventGetArgs so tests can verify each extra was parsed and
    // threaded. compute_utc_times echoes back the tz_hint so timeZone
    // forwarding can be observed in the response.
    // -----------------------------------------------------------------------

    /// Oracle: §5.7 — `recurrenceOverridesBefore`, `recurrenceOverridesAfter`,
    /// and `reduceParticipants` MUST be parsed from the request and forwarded
    /// to the backend. Without this, a backend cannot honour the spec's
    /// override-filter / participant-reduction semantics.
    #[tokio::test]
    async fn calendar_event_get_forwards_section_5_7_args_to_backend() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/get",
            json!({
                "accountId": "acc1",
                "ids": null,
                "recurrenceOverridesBefore": "2026-01-01T00:00:00Z",
                "recurrenceOverridesAfter":  "2025-01-01T00:00:00Z",
                "reduceParticipants": true,
                "timeZone": "Europe/London"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        let recorded = backend
            .take_last_get_args()
            .expect("backend must have received get_args");
        assert_eq!(
            recorded.recurrence_overrides_before.as_deref(),
            Some("2026-01-01T00:00:00Z"),
            "recurrenceOverridesBefore must be forwarded"
        );
        assert_eq!(
            recorded.recurrence_overrides_after.as_deref(),
            Some("2025-01-01T00:00:00Z"),
            "recurrenceOverridesAfter must be forwarded"
        );
        assert!(
            recorded.reduce_participants,
            "reduceParticipants:true must be forwarded"
        );
        assert_eq!(
            recorded.time_zone.as_deref(),
            Some("Europe/London"),
            "timeZone must be forwarded"
        );
    }

    /// Oracle: §5.7 — `timeZone` MUST be passed to `compute_utc_times` (the
    /// prior implementation hardcoded `None` for `tz_hint`, ignoring the
    /// client-supplied value). When `properties` requests `utcStart`, the
    /// computed value must reflect the requested time zone.
    #[tokio::test]
    async fn calendar_event_get_time_zone_passed_to_compute_utc_times() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        // Seed a minimal event so the response list is non-empty.
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "ev1",
            json!({
                "id": "ev1",
                "calendarIds": {"cal1": true},
                "title": "Meeting",
                "start": "2025-06-01T10:00:00",
                "duration": "PT1H"
            }),
        );

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/get",
            json!({
                "accountId": "acc1",
                "ids": ["ev1"],
                "properties": ["id", "utcStart", "utcEnd"],
                "timeZone": "America/New_York"
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        let list = args["list"].as_array().expect("list array");
        assert_eq!(list.len(), 1, "one event in response: {args}");
        // The mock's compute_utc_times echoes "tz=<received>" so we can
        // observe what tz_hint was passed.
        assert_eq!(
            list[0]["utcStart"], "tz=America/New_York",
            "timeZone arg must be passed to compute_utc_times: {args}"
        );
        assert_eq!(
            list[0]["utcEnd"], "tz=America/New_York",
            "timeZone arg must be passed to compute_utc_times: {args}"
        );
    }

    /// Oracle: §5.7 — when `timeZone` is absent, the backend receives
    /// `None` and `compute_utc_times` defaults to `Etc/UTC`.
    #[tokio::test]
    async fn calendar_event_get_default_time_zone_is_etc_utc() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "ev1",
            json!({
                "id": "ev1",
                "calendarIds": {"cal1": true},
                "title": "Meeting",
                "start": "2025-06-01T10:00:00",
                "duration": "PT1H"
            }),
        );

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/get",
            json!({
                "accountId": "acc1",
                "ids": ["ev1"],
                "properties": ["id", "utcStart"]
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        // The backend sees None; the mock's compute_utc_times maps that
        // to "Etc/UTC" when echoing.
        assert_eq!(
            args["list"][0]["utcStart"], "tz=Etc/UTC",
            "default timeZone must be Etc/UTC: {args}"
        );
        let recorded = backend
            .take_last_get_args()
            .expect("backend must have received get_args");
        assert!(
            recorded.time_zone.is_none(),
            "absent timeZone arg must remain None at the backend: {recorded:?}"
        );
    }

    /// Oracle: §5.7 — when `properties` does not include `utcStart` or
    /// `utcEnd`, `compute_utc_times` MUST NOT be invoked (or at least its
    /// output MUST NOT appear in the response). Avoids leaking computed
    /// fields into responses that didn't request them.
    #[tokio::test]
    async fn calendar_event_get_no_utc_when_not_requested() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "ev1",
            json!({
                "id": "ev1",
                "calendarIds": {"cal1": true},
                "title": "Meeting",
                "start": "2025-06-01T10:00:00",
                "duration": "PT1H"
            }),
        );

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/get",
            json!({
                "accountId": "acc1",
                "ids": ["ev1"],
                "properties": ["id", "title"]
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args["list"][0].get("utcStart").is_none(),
            "utcStart must NOT appear when not in properties: {args}"
        );
        assert!(
            args["list"][0].get("utcEnd").is_none(),
            "utcEnd must NOT appear when not in properties: {args}"
        );
    }

    /// Oracle: §5.7 — defaults from the spec: `reduceParticipants` defaults
    /// to `false`, the recurrence-override bounds default to `None`. The
    /// handler MUST construct `CalendarEventGetArgs` with these defaults
    /// when the wire request omits each.
    #[tokio::test]
    async fn calendar_event_get_default_args() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_calendars_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "CalendarEvent/get",
            json!({"accountId": "acc1", "ids": null}),
            "c0",
        );
        let _ = dispatcher.dispatch(req, (), State::from("s0")).await;
        let recorded = backend
            .take_last_get_args()
            .expect("backend must have received get_args");
        assert!(
            recorded.recurrence_overrides_before.is_none(),
            "recurrenceOverridesBefore default is None"
        );
        assert!(
            recorded.recurrence_overrides_after.is_none(),
            "recurrenceOverridesAfter default is None"
        );
        assert!(
            !recorded.reduce_participants,
            "reduceParticipants default is false"
        );
        assert!(
            recorded.time_zone.is_none(),
            "timeZone default is None (handler treats as Etc/UTC)"
        );
    }
}
