//! A wrapper that implements every chat hook trait through its `Delegate*`
//! companion behaves exactly like the backend it wraps: the `testkit`
//! conformance suite passes on it (bd JMAP-it7g.6.2;
//! docs/design/chat-backend-split.md §3.2, §6 step 2).

use jmap_chat_server::memory::{MemoryBackend, MemoryError};
use jmap_chat_server::testkit::ConformanceBackend;
use jmap_chat_server::{
    DelegateChatAccessPolicy, DelegateChatEvents, DelegateChatMessageLifecycle,
    DelegateChatModeration, DelegateChatResolvers, DelegateChatRestrictions, DelegateChatSettings,
    DelegateChatSpaces, DelegateChatStore, DelegateSpacePolicy, OutboundEvent,
};
use jmap_server::{
    BackendChangesError, BackendQueryError, ChangesParams, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult,
};
use jmap_types::{Id, State};
use serde_json::Value;

/// Delegates every hook trait to `inner`; forwards `JmapBackend` by hand
/// (phase 2 of the design splits that out).
#[derive(Clone, Default)]
struct Wrapper {
    inner: MemoryBackend,
}

impl JmapBackend for Wrapper {
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
        params: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        self.inner
            .get_changes::<O>(caller, account_id, params)
            .await
    }
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        self.inner.query_objects(caller, account_id, params).await
    }
    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        self.inner.query_changes(caller, account_id, params).await
    }
    fn metadata_info<O: JmapObject>(
        &self,
        caller: &(),
        account_id: &Id,
    ) -> Option<jmap_metadata_types::DataTypeMetadataInfo> {
        self.inner.metadata_info::<O>(caller, account_id)
    }
}

macro_rules! delegate {
    ($($tr:ident => $acc:ident),* $(,)?) => {$(
        impl $tr for Wrapper {
            type Target = MemoryBackend;
            fn $acc(&self) -> &MemoryBackend {
                &self.inner
            }
        }
    )*};
}

delegate!(
    DelegateChatStore => chat_store,
    DelegateChatSpaces => chat_spaces,
    DelegateSpacePolicy => space_policy,
    DelegateChatAccessPolicy => chat_access_policy,
    DelegateChatRestrictions => chat_restrictions,
    DelegateChatModeration => chat_moderation,
    DelegateChatMessageLifecycle => chat_message_lifecycle,
    DelegateChatEvents => chat_events,
    DelegateChatSettings => chat_settings,
    DelegateChatResolvers => chat_resolvers,
);

impl ConformanceBackend for Wrapper {
    fn caller(&self, account_id: &Id) {
        self.inner.caller(account_id)
    }
    async fn add_account(&self, account_id: &Id, owner_user_id: &str) {
        self.inner.add_account(account_id, owner_user_id).await;
    }
    async fn seed(&self, account_id: &Id, type_name: &'static str, object: Value) {
        self.inner.seed(account_id, type_name, object).await;
    }
    fn take_outbound_events(&self) -> Option<Vec<OutboundEvent>> {
        self.inner.take_outbound_events()
    }
}

jmap_chat_server::chat_backend_conformance!(Wrapper::default);
