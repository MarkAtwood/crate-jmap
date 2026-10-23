//! RFC 8620 §3.6.2 `accountNotFound` across the 9 chat-server /set
//! handlers (bd:JMAP-fo38).
//!
//! RFC 8620 §3.6.2: "`accountNotFound`: The accountId does not
//! correspond to a valid account." Each test calls a /set handler with
//! an accountId the backend never registered and asserts the method
//! error type against the hand-written wire string `"accountNotFound"`,
//! then checks the rejected call left no account behind (no write ran).

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_chat_server::message::handle_message_set;
use jmap_chat_server::{
    handle_ban_set, handle_chat_set, handle_contact_set, handle_emoji_set, handle_invite_set,
    handle_position_set, handle_presence_set, handle_space_set,
};
use jmap_server::JmapBackend;
use jmap_types::Id;
use serde_json::json;

const UNKNOWN_ACCOUNT: &str = "no-such-account";

/// The `ids` form calls a `/set` handler that takes `created_ids`, with
/// an empty map. The `using` form calls one that takes the request's
/// `using` list, with an empty list.
macro_rules! account_not_found_test {
    (ids using $name:ident, $handler:ident) => {
        account_not_found_test!(@ $name, $handler, [&std::collections::BTreeMap::new(),], [, &[]]);
    };
    (ids $name:ident, $handler:ident) => {
        account_not_found_test!(@ $name, $handler, [&std::collections::BTreeMap::new(),], []);
    };
    (using $name:ident, $handler:ident) => {
        account_not_found_test!(@ $name, $handler, [], [, &[]]);
    };
    ($name:ident, $handler:ident) => {
        account_not_found_test!(@ $name, $handler, [], []);
    };
    (@ $name:ident, $handler:ident, [$($ids:tt)*], [$($using:tt)*]) => {
        #[tokio::test]
        async fn $name() {
            let backend = MemoryBackend::new();
            let args = json!({
                "accountId": UNKNOWN_ACCOUNT,
                "create": { "c0": {} }
            });
            let err = $handler(&backend, &(), $($ids)* args $($using)*)
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
                "rejected /set must not create the account"
            );
        }
    };
}

account_not_found_test!(ids using chat_set_unknown_account, handle_chat_set);
account_not_found_test!(ids using message_set_unknown_account, handle_message_set);
account_not_found_test!(presence_set_unknown_account, handle_presence_set);
account_not_found_test!(ids position_set_unknown_account, handle_position_set);
account_not_found_test!(contact_set_unknown_account, handle_contact_set);
account_not_found_test!(using space_set_unknown_account, handle_space_set);
account_not_found_test!(ids emoji_set_unknown_account, handle_emoji_set);
account_not_found_test!(ids invite_set_unknown_account, handle_invite_set);
account_not_found_test!(ids ban_set_unknown_account, handle_ban_set);
