//! RFC 8620 §5.3 `maxObjectsInSet` enforcement across the 9 chat-server
//! /set handlers (bd:JMAP-ayoz.41.3).
//!
//! Every test asserts the wire-format `JmapError::limit("maxObjectsInSet")`
//! shape against a hand-built independent oracle — the helper under test
//! is never the oracle. Oversize batches use 501 entries against the
//! default `JmapBackend::max_objects_in_set` cap of 500.

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_chat_server::{
    handle_ban_set, handle_chat_set, handle_contact_set, handle_emoji_set, handle_invite_set,
    handle_message_set, handle_position_set, handle_presence_set, handle_space_set,
};
use jmap_types::Id;
use serde_json::{json, Value};

const DEFAULT_CAP: usize = 500;

fn create_map_of_size(n: usize) -> Value {
    let mut map = serde_json::Map::with_capacity(n);
    for i in 0..n {
        map.insert(format!("c{i}"), json!({}));
    }
    Value::Object(map)
}

fn destroy_array_of_size(n: usize) -> Value {
    Value::Array((0..n).map(|i| json!(format!("id-{i}"))).collect())
}

/// Independent oracle for `JmapError::limit("maxObjectsInSet")`.
fn expected_limit_error_shape() -> (&'static str, &'static str) {
    ("limit", "maxObjectsInSet")
}

fn one_account_backend() -> (MemoryBackend, Id) {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    backend.register_account(&account_id);
    (backend, account_id)
}

macro_rules! over_limit_test {
    ($name:ident, $handler:ident, $shape:expr) => {
        #[tokio::test]
        async fn $name() {
            let (backend, account_id) = one_account_backend();
            let mut args = serde_json::Map::new();
            args.insert("accountId".into(), json!(account_id.as_ref()));
            for (k, v) in $shape {
                args.insert(k.into(), v);
            }
            let err = $handler(&backend, &(), Value::Object(args))
                .await
                .expect_err(concat!(
                    "501-entry batch must trip maxObjectsInSet cap (",
                    stringify!($handler),
                    ")"
                ));
            let (etype, edesc) = expected_limit_error_shape();
            assert_eq!(err.error_type.as_str(), etype);
            assert_eq!(err.description.as_deref(), Some(edesc));
        }
    };
}

over_limit_test!(
    chat_set_over_limit,
    handle_chat_set,
    [("create", create_map_of_size(DEFAULT_CAP + 1))]
);

over_limit_test!(
    message_set_over_limit,
    handle_message_set,
    [("create", create_map_of_size(DEFAULT_CAP + 1))]
);

over_limit_test!(
    presence_set_over_limit,
    handle_presence_set,
    [("create", create_map_of_size(DEFAULT_CAP + 1))]
);

over_limit_test!(
    position_set_over_limit,
    handle_position_set,
    [("create", create_map_of_size(DEFAULT_CAP + 1))]
);

over_limit_test!(
    contact_set_over_limit,
    handle_contact_set,
    [("create", create_map_of_size(DEFAULT_CAP + 1))]
);

over_limit_test!(
    space_set_over_limit,
    handle_space_set,
    [("create", create_map_of_size(DEFAULT_CAP + 1))]
);

over_limit_test!(
    emoji_set_over_limit,
    handle_emoji_set,
    [("create", create_map_of_size(DEFAULT_CAP + 1))]
);

// Invite/set uses destroy (not create — invites are typically destroyed
// after consumption rather than re-created in bulk). Cap counts apply
// uniformly across create/update/destroy.
over_limit_test!(
    invite_set_over_limit,
    handle_invite_set,
    [("destroy", destroy_array_of_size(DEFAULT_CAP + 1))]
);

over_limit_test!(
    ban_set_over_limit,
    handle_ban_set,
    [("destroy", destroy_array_of_size(DEFAULT_CAP + 1))]
);
