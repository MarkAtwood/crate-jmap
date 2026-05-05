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
//! # fn example<B: CalendarsBackend + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_calendars_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
pub mod calendar;
pub mod event;
pub mod event_notification;
mod helpers;
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
pub const CAPABILITY_CALENDARS: &str = "urn:ietf:params:jmap:calendars";

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
/// `CallerCtx` IS now forwarded as `_ctx` to each closure.  Handler bodies
/// still receive only `(b, ci, a)`; `_ctx` is available for custom use by
/// backends that register handlers individually via [`ClosureHandlerWithCtx`].
pub fn register_calendars_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: CalendarsBackend + 'static,
    C: Clone + Send + 'static,
{
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<C>> = Arc::new(ClosureHandlerWithCtx {
                backend: backend_arc,
                call_fn: Box::new(
                    move |$b: Arc<B>, $ci: String, $a: serde_json::Value, _ctx: C| {
                        Box::pin(async move { $body }) as HandlerFuture
                    },
                ),
            });
            dispatcher.register($method, h);
        }};
    }

    // Calendar
    reg!("Calendar/get", backend, |b, _ci, a| {
        handle_calendar_get(&*b, a).await
    });
    reg!("Calendar/changes", backend, |b, _ci, a| {
        handle_calendar_changes(&*b, a).await
    });
    reg!("Calendar/set", backend, |b, _ci, a| {
        handle_calendar_set(&*b, a).await
    });

    // CalendarEvent
    reg!("CalendarEvent/get", backend, |b, _ci, a| {
        handle_calendar_event_get(&*b, a).await
    });
    reg!("CalendarEvent/changes", backend, |b, _ci, a| {
        handle_calendar_event_changes(&*b, a).await
    });
    reg!("CalendarEvent/set", backend, |b, _ci, a| {
        handle_calendar_event_set(&*b, a).await
    });
    reg!("CalendarEvent/copy", backend, |b, ci, a| {
        handle_calendar_event_copy(&*b, a, &ci).await
    });
    reg!("CalendarEvent/query", backend, |b, _ci, a| {
        handle_calendar_event_query(&*b, a).await
    });
    reg!("CalendarEvent/queryChanges", backend, |b, _ci, a| {
        handle_calendar_event_query_changes(&*b, a).await
    });
    reg!("CalendarEvent/parse", backend, |b, _ci, a| {
        handle_calendar_event_parse(&*b, a).await
    });

    // CalendarEventNotification
    reg!("CalendarEventNotification/get", backend, |b, _ci, a| {
        handle_calendar_event_notification_get(&*b, a).await
    });
    reg!("CalendarEventNotification/changes", backend, |b, _ci, a| {
        handle_calendar_event_notification_changes(&*b, a).await
    });
    reg!("CalendarEventNotification/set", backend, |b, _ci, a| {
        handle_calendar_event_notification_set(&*b, a).await
    });
    reg!("CalendarEventNotification/query", backend, |b, _ci, a| {
        handle_calendar_event_notification_query(&*b, a).await
    });
    reg!(
        "CalendarEventNotification/queryChanges",
        backend,
        |b, _ci, a| handle_calendar_event_notification_query_changes(&*b, a).await
    );

    // ParticipantIdentity
    reg!("ParticipantIdentity/get", backend, |b, _ci, a| {
        handle_participant_identity_get(&*b, a).await
    });
    reg!("ParticipantIdentity/changes", backend, |b, _ci, a| {
        handle_participant_identity_changes(&*b, a).await
    });
    reg!("ParticipantIdentity/set", backend, |b, _ci, a| {
        handle_participant_identity_set(&*b, a).await
    });

    // Principal
    reg!("Principal/getAvailability", backend, |b, _ci, a| {
        handle_principal_get_availability(&*b, a).await
    });
}

pub use jmap_server::{ClosureHandler, ClosureHandlerWithCtx};

// ---------------------------------------------------------------------------
// test_support — in-memory mock backend used by inline tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_support {
    //! In-memory mock backend for unit tests.

    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    use jmap_server::{
        BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject,
        QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    };
    use jmap_types::{Id, State};

    use crate::backend::CalendarsBackend;

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
    }

    #[derive(Clone)]
    pub struct MockBackend {
        state: Arc<Mutex<HashMap<String, AccountState>>>,
    }

    impl MockBackend {
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(HashMap::new())),
            }
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
            let acct = guard
                .entry(account_id.to_owned())
                .or_insert_with(AccountState::default);
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
            let mut guard = self.state.lock().unwrap();
            let acct = guard
                .entry(account_id.to_owned())
                .or_insert_with(AccountState::default);
            acct.objects
                .entry(type_name.to_owned())
                .or_default()
                .insert(Id::from(id), value);
        }
    }

    impl JmapBackend for MockBackend {
        type Error = MockError;

        async fn account_exists(&self, account_id: &Id) -> Result<bool, Self::Error> {
            Ok(self.state.lock().unwrap().contains_key(account_id.as_ref()))
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
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
                        if let Ok(obj) = serde_json::from_value::<O>(v.clone()) {
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
                            Some(v) => match serde_json::from_value::<O>(v.clone()) {
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
            _account_id: &Id,
        ) -> Result<State, Self::Error> {
            Ok(State::from("0"))
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
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
            _account_id: &Id,
            _create_id: &str,
            obj: O,
        ) -> Result<(Id, O), BackendSetError<Self::Error>> {
            Ok((Id::from("mock-id-1"), obj))
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
            _account_id: &Id,
            _id: &Id,
            _patch: O::Patch,
        ) -> Result<Option<O>, BackendSetError<Self::Error>> {
            Err(BackendSetError::SetError(SetError::new(
                SetErrorType::Forbidden,
            )))
        }

        async fn destroy_object<O: SetObject + Send + Sync>(
            &self,
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

        async fn calendar_has_events(&self, account_id: &Id, calendar_id: &Id) -> bool {
            let guard = self.state.lock().unwrap();
            guard
                .get(account_id.as_ref())
                .map(|acct| acct.calendars_with_events.contains(calendar_id))
                .unwrap_or(false)
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
    /// `calendarHasEvents`.
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
            args["notDestroyed"]["cal1"]["type"], "calendarHasEvents",
            "must produce calendarHasEvents: {args}"
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
}
