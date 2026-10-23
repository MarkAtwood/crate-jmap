//! Shared fixtures. Line numbers cite draft-atwood-jmap-graph-00.md at
//! jmap-chat-spec 6bcd9d8.

#![allow(dead_code)]

use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_types::GraphCapability;
use jmap_types::Id;
use serde_json::{Value, json};

/// The account of the L354-393 example.
pub const ACCOUNT: &str = "A1";
/// The owner of [`ACCOUNT`]: "The owner of the account that holds the
/// Graph implicitly has all rights" (L173).
pub const ALICE: &str = "alice";
pub const BOB: &str = "bob";
pub const CAROL: &str = "carol";

/// A backend with [`ACCOUNT`] owned by [`ALICE`], advertising
/// `maxAnnotationSize` (L107) as `max`.
pub fn backend_with(max: Value) -> MemoryBackend {
    let backend = MemoryBackend::new();
    let cap: GraphCapability =
        serde_json::from_value(json!({ "maxAnnotationSize": max })).expect("capability fixture");
    backend.register_account(&Id::from(ACCOUNT), &Id::from(ALICE), cap);
    backend
}

/// A backend whose capability advertises no limit (`null`, L108).
pub fn backend() -> MemoryBackend {
    backend_with(Value::Null)
}

/// Run handler `$method` on [`ACCOUNT`] as `$caller` with `$args` (without
/// `accountId`) and return the response.
#[allow(unused_macros)]
macro_rules! call {
    ($method:path, $backend:expr, $caller:expr, $args:expr $(,)?) => {{
        let mut args: serde_json::Value = $args;
        args["accountId"] = serde_json::json!($crate::common::ACCOUNT);
        $method($backend, &jmap_types::Id::from($caller), args)
            .await
            .expect("method")
            .primary
    }};
}

/// Like [`call!`], but return the method-level error's `type`.
#[allow(unused_macros)]
macro_rules! call_err {
    ($method:path, $backend:expr, $caller:expr, $args:expr $(,)?) => {{
        let mut args: serde_json::Value = $args;
        args["accountId"] = serde_json::json!($crate::common::ACCOUNT);
        let err = $method($backend, &jmap_types::Id::from($caller), args)
            .await
            .expect_err("method must fail");
        serde_json::to_value(&err).expect("JmapError serializes")["type"].clone()
    }};
}

/// `{ mayRead, mayWrite, mayAdmin }`, the GraphRights of L162-171.
pub fn rights(read: bool, write: bool, admin: bool) -> Value {
    json!({ "mayRead": read, "mayWrite": write, "mayAdmin": admin })
}
