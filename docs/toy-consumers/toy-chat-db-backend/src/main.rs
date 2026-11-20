//! Toy consumer: a database-shaped `ChatBackend`.
//!
//! `DbBackend` wraps the reference `MemoryBackend` the way a database
//! backend with a directory service and a policy layer would: it answers
//! `account_exists` from its own account directory, refuses new Messages
//! in Chats its policy layer marks read-only (`ChatRestrictions`), and
//! delegates everything else with the `Delegate*` traits and
//! `DelegateJmapReads`. The handlers are registered on a `Dispatcher` and a
//! `Message/query` with a JMAP Search `where` filter runs end to end.

use std::collections::BTreeSet;
use std::sync::Arc;

use jmap_chat_server::memory::{MemoryBackend, MemoryError};
use jmap_chat_server::{
    ChatRestrictions, DelegateChatAccessPolicy, DelegateChatEvents, DelegateChatMessageLifecycle,
    DelegateChatModeration, DelegateChatResolvers, DelegateChatSettings, DelegateChatSpaces,
    DelegateChatStore, DelegateSpacePolicy, register_chat_handlers,
};
use jmap_search::{DataTypeSearchInfo, JMAP_SEARCH_URI, SearchCapability};
use jmap_server::{
    DelegateJmapReads, Dispatcher, JmapAccounts, JmapObject, SetError, SetErrorType,
};
use jmap_types::{Id, Invocation, JmapRequest, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "A1";

/// The database-shaped backend: an account directory, a set of read-only
/// Chat names, and the store it wraps.
#[derive(Clone)]
struct DbBackend {
    directory: Arc<BTreeSet<String>>,
    read_only_chats: Arc<BTreeSet<String>>,
    store: MemoryBackend,
}

impl JmapAccounts for DbBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    /// From the directory, not the store.
    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, MemoryError> {
        Ok(self.directory.contains(account_id.as_ref()))
    }

    /// `JmapAccounts` has no delegate, so every method the wrapped backend
    /// overrides has to be forwarded by hand (finding a1 in
    /// docs/audits/2026-11-20-toy-consumers).
    fn metadata_info<O: JmapObject>(
        &self,
        caller: &(),
        account_id: &Id,
    ) -> Option<jmap_metadata_types::DataTypeMetadataInfo> {
        self.store.metadata_info::<O>(caller, account_id)
    }
}

/// The one hook trait implemented by hand: refuse a new Message in a
/// read-only Chat, forward the other two methods.
impl ChatRestrictions for DbBackend {
    async fn may_create_message(
        &self,
        caller: &(),
        account_id: &Id,
        chat: &jmap_chat_types::Chat,
    ) -> Result<Result<(), SetError>, MemoryError> {
        if chat
            .name
            .as_deref()
            .is_some_and(|n| self.read_only_chats.contains(n))
        {
            return Ok(Err(
                SetError::new(SetErrorType::Forbidden).with_description("read-only chat")
            ));
        }
        self.store
            .may_create_message(caller, account_id, chat)
            .await
    }

    async fn may_update_message(
        &self,
        caller: &(),
        account_id: &Id,
        chat: &jmap_chat_types::Chat,
        message: &jmap_chat_types::Message,
        patch_keys: &[&str],
    ) -> Result<Result<(), SetError>, MemoryError> {
        self.store
            .may_update_message(caller, account_id, chat, message, patch_keys)
            .await
    }

    async fn may_update_chat(
        &self,
        caller: &(),
        account_id: &Id,
        chat: &jmap_chat_types::Chat,
        patch_keys: &[&str],
    ) -> Result<Result<(), SetError>, MemoryError> {
        self.store
            .may_update_chat(caller, account_id, chat, patch_keys)
            .await
    }
}

macro_rules! delegate {
    ($($tr:ident => $acc:ident),* $(,)?) => {$(
        impl $tr for DbBackend {
            type Target = MemoryBackend;
            fn $acc(&self) -> &MemoryBackend {
                &self.store
            }
        }
    )*};
}

delegate!(
    DelegateJmapReads => jmap_reads,
    DelegateChatStore => chat_store,
    DelegateChatSpaces => chat_spaces,
    DelegateSpacePolicy => space_policy,
    DelegateChatAccessPolicy => chat_access_policy,
    DelegateChatModeration => chat_moderation,
    DelegateChatMessageLifecycle => chat_message_lifecycle,
    DelegateChatEvents => chat_events,
    DelegateChatSettings => chat_settings,
    DelegateChatResolvers => chat_resolvers,
);

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".into(),
        "urn:ietf:params:jmap:chat".into(),
        JMAP_SEARCH_URI.into(),
    ]
}

async fn call(d: &Dispatcher<()>, calls: Vec<(&str, Value)>) -> Vec<Invocation> {
    let calls = calls
        .into_iter()
        .enumerate()
        .map(|(i, (name, args))| Invocation::new(name, args, i.to_string()))
        .collect();
    d.dispatch(
        JmapRequest::new(using(), calls, None),
        (),
        State::from("s0"),
    )
    .await
    .method_responses
}

#[tokio::main]
async fn main() {
    let search = SearchCapability::new(4).with_data_type("Message", DataTypeSearchInfo::new(true));
    let store = MemoryBackend::new()
        .with_search_capability(search)
        .with_account(&Id::from(ACCOUNT));
    let backend = DbBackend {
        directory: Arc::new(BTreeSet::from([ACCOUNT.to_owned()])),
        read_only_chats: Arc::new(BTreeSet::from(["announcements".to_owned()])),
        store,
    };
    let mut d = Dispatcher::new();
    register_chat_handlers(&mut d, Arc::new(backend));

    // Two group Chats, one of them read-only by policy.
    let r = call(
        &d,
        vec![(
            "Chat/set",
            json!({ "accountId": ACCOUNT, "create": {
                "g": { "kind": "group", "name": "general" },
                "ro": { "kind": "group", "name": "announcements" }
            }}),
        )],
    )
    .await;
    let created = &r[0].arguments["created"];
    let general = created["g"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Chat/set: {}", r[0].arguments))
        .to_owned();
    let ro = created["ro"]["id"].as_str().expect("ro chat").to_owned();

    let r = call(
        &d,
        vec![(
            "Message/set",
            json!({ "accountId": ACCOUNT, "create": {
                "m1": { "chatId": general, "body": "Release notes are out",
                        "bodyType": "text/markdown", "sentAt": "2026-11-20T10:00:00Z" },
                "m2": { "chatId": general, "body": "Lunch?",
                        "bodyType": "text/plain", "sentAt": "2026-11-20T10:01:00Z" },
                "m3": { "chatId": ro, "body": "Blocked",
                        "bodyType": "text/plain", "sentAt": "2026-11-20T10:02:00Z" }
            }}),
        )],
    )
    .await;
    let set = &r[0].arguments;
    let m1 = set["created"]["m1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Message/set: {set}"))
        .to_owned();
    assert!(set["created"]["m2"]["id"].is_string(), "{set}");
    assert_eq!(set["notCreated"]["m3"]["type"], "forbidden", "{set}");

    // Message/query with a `where` filter.
    let r = call(
        &d,
        vec![(
            "Message/query",
            json!({ "accountId": ACCOUNT, "filter": {
                "chatId": general,
                "where": { "$and": [
                    { "/bodyType": "text/markdown" },
                    { "$text": "release" }
                ]}
            }}),
        )],
    )
    .await;
    assert_eq!(r[0].name, "Message/query", "{:?}", r[0]);
    assert_eq!(r[0].arguments["ids"], json!([m1]), "{}", r[0].arguments);

    // An unknown account is refused by the directory.
    let r = call(
        &d,
        vec![(
            "Message/query",
            json!({ "accountId": "nobody", "filter": null }),
        )],
    )
    .await;
    assert_eq!(r[0].arguments["type"], "accountNotFound", "{:?}", r[0]);

    println!("toy-chat-db-backend: ok");
}
