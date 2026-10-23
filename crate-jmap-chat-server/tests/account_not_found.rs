//! RFC 8620 §3.6.2 `accountNotFound` across the chat-server custom
//! handlers that do not delegate to the generic jmap-server handlers
//! (bd:JMAP-dtpe). The /set handlers are covered by
//! `set_account_not_found.rs`.
//!
//! RFC 8620 §3.6.2: "`accountNotFound`: The accountId does not
//! correspond to a valid account." Each test sends an otherwise
//! well-formed request naming an accountId the backend never registered
//! and asserts the method error type against the hand-written wire
//! string `"accountNotFound"`, then checks the call left no account
//! behind.

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_chat_server::message::{
    handle_message_get, handle_message_query, handle_message_query_changes,
};
use jmap_chat_server::{
    handle_ban_get, handle_chat_typing, handle_contact_changes, handle_contact_get,
    handle_contact_query, handle_emoji_changes, handle_emoji_get, handle_emoji_query,
    handle_invite_changes, handle_invite_get, handle_position_changes, handle_position_get,
    handle_presence_changes, handle_presence_get, handle_space_get, handle_space_join,
    handle_space_query,
};
use jmap_server::JmapBackend;
use jmap_types::Id;
use serde_json::json;

const UNKNOWN_ACCOUNT: &str = "no-such-account";

macro_rules! account_not_found_test {
    ($name:ident, $handler:ident, { $($arg:tt)* }) => {
        #[tokio::test]
        async fn $name() {
            let backend = MemoryBackend::new();
            let args = json!({ "accountId": UNKNOWN_ACCOUNT, $($arg)* });
            let err = $handler(&backend, &(), args)
                .await
                .expect_err(concat!(
                    "unknown accountId must be rejected (",
                    stringify!($handler),
                    ")"
                ));
            assert_eq!(err.error_type.to_string(), "accountNotFound");
            assert!(
                !backend
                    .account_exists(&(), &Id::from(UNKNOWN_ACCOUNT))
                    .await
                    .expect("account_exists"),
                "rejected call must not create the account"
            );
        }
    };
}

account_not_found_test!(ban_get_unknown_account, handle_ban_get, { "ids": null });
account_not_found_test!(contact_get_unknown_account, handle_contact_get, { "ids": null });
account_not_found_test!(contact_changes_unknown_account, handle_contact_changes, { "sinceState": "0" });
account_not_found_test!(contact_query_unknown_account, handle_contact_query, {});
account_not_found_test!(emoji_get_unknown_account, handle_emoji_get, { "ids": null });
account_not_found_test!(emoji_changes_unknown_account, handle_emoji_changes, { "sinceState": "0" });
account_not_found_test!(emoji_query_unknown_account, handle_emoji_query, {});
account_not_found_test!(invite_get_unknown_account, handle_invite_get, { "ids": null });
account_not_found_test!(invite_changes_unknown_account, handle_invite_changes, { "sinceState": "0" });
account_not_found_test!(message_get_unknown_account, handle_message_get, { "ids": null });
account_not_found_test!(message_query_unknown_account, handle_message_query, {});
account_not_found_test!(
    message_query_changes_unknown_account,
    handle_message_query_changes,
    { "filter": { "chatId": "c1" }, "sinceQueryState": "0" }
);
account_not_found_test!(position_get_unknown_account, handle_position_get, { "ids": null });
account_not_found_test!(position_changes_unknown_account, handle_position_changes, { "sinceState": "0" });
account_not_found_test!(presence_get_unknown_account, handle_presence_get, { "ids": null });
account_not_found_test!(presence_changes_unknown_account, handle_presence_changes, { "sinceState": "0" });
account_not_found_test!(space_get_unknown_account, handle_space_get, { "ids": null });
account_not_found_test!(space_query_unknown_account, handle_space_query, {});
account_not_found_test!(chat_typing_unknown_account, handle_chat_typing, { "chatId": "c1", "typing": true });
account_not_found_test!(space_join_unknown_account, handle_space_join, { "spaceId": "s1" });
