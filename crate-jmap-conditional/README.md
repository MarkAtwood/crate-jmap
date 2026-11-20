# jmap-conditional

JMAP conditional and atomic Foo/set ([draft-ietf-jmap-conditional-02]) in
one dependency: re-exports the `conditional` module of [`jmap-types`]
always and the conditional items of [`jmap-server`] with the `server`
feature.

## What it is

A facade with no code of its own. The extension has no `-types`,
`-client` or `-server` crates: it is implemented as a module of
`jmap-types` and a set of items in `jmap-server`, and this crate
re-exports exactly those.

| Feature | Re-export |
|---|---|
| (none) | `jmap_conditional::types` (= `jmap_types::conditional`), `jmap_conditional::CAPABILITY` (kept from 0.0.1) |
| `server` | `jmap_conditional::server`: `ConditionalBackend`, `SetScope`, `SetScopeRequest`, `SetScopeId`, `SetMode`, `SetOp`, `SetSupport`, `BeginSetError`, `CommitError`, `CommitFuture`, `RollbackFuture`, `DeferredFailure`, `RegistrationError`, `current_set_scope`, `serialized_writes_scope`, `register_push_subscription_conditional` |
| `scoped-state` | `server`, plus `ScopedState`, `ScopeGate`, `ScopedGuard` (enables `jmap-server/scoped-state`) |

- `types`: the capability (`JMAP_CONDITIONAL_URI`, `ConditionalCapability`),
  the `ifUnchangedBy` and `atomic` arguments (`ConditionalSetArgs`), and
  the `stateMismatch`, `atomicFailure` and `cannotApplyAtomically` errors
  (`AtomicFailure`, `ATOMIC_FAILURE`, `CANNOT_APPLY_ATOMICALLY`).
- `server`: the backend hook a deployment implements to open a per-call
  scope, and the opt-in registration types.
- `scoped-state`: the gated, snapshotting state the in-repo memory
  backends build `begin_set` on. Demonstration quality.

## What it's for

A server that wants `ifUnchangedBy` preconditions and all-or-nothing
`atomic` Foo/set implements `ConditionalBackend` and calls each extension
crate's `register_<x>_conditional` after its `register_<x>_handlers`. A
client builds `ConditionalSetArgs` and merges them into its Foo/set call.
Applications that want those items under the extension's name depend on
this crate; workspace crates use `jmap-types` and `jmap-server` directly.

## How to use

```toml
[dependencies]
jmap-conditional = { version = "0.5", features = ["server"] }
```

```rust
use jmap_conditional::types::{ConditionalSetArgs, JMAP_CONDITIONAL_URI};

assert_eq!(JMAP_CONDITIONAL_URI, "urn:ietf:params:jmap:conditional");
assert_eq!(jmap_conditional::CAPABILITY, JMAP_CONDITIONAL_URI);
let args: ConditionalSetArgs = serde_json::from_str(
    r#"{"ifUnchangedBy": {"M1": {"name": "Inbox"}}, "atomic": true}"#,
)
.unwrap();
assert!(args.atomic);
```

## How it works

`types` is a `pub use` of `jmap_types::conditional`, and each `server`
item is a `pub use` of the same item at the `jmap-server` root, so
`jmap_conditional::server::X` is the same item as `jmap_server::X`. The
dispatcher evaluates `ifUnchangedBy` and applies `atomic` around the
registered Foo/set handler, inside the scope the backend opens
(docs/design/conditional.md §5.2). The re-exported crates version in
lockstep with this one on the shared `jmap-*` line.

## Gotchas

- Support is opt-in per Foo/set method (docs/design/DECISIONS.md
  decisions 1 and 17). The `register_<x>_conditional` functions live in
  each extension server crate (`jmap-mail-server`,
  `jmap-calendars-server`, …) and are not re-exported here; only
  `jmap-server`'s own `register_push_subscription_conditional` is.
- `Dispatcher::attach_set_support`, `conditional_ready` and
  `conditional_gaps` are methods of `jmap_server::Dispatcher`. Advertise
  the capability only once `conditional_ready` is true: it covers every
  Foo/set the server offers (§2.1).
- The `JmapError` constructors and readers (`state_mismatch_failed`,
  `atomic_failure`, `cannot_apply_atomically`, `failed`,
  `atomic_failure_details`) are inherent methods of `jmap_types::JmapError`.
- Depending on this crate and on `jmap-types` or `jmap-server` at a
  different version gives two copies of each type, which do not unify.
- Version 0.5.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-ietf-jmap-conditional-02] — JMAP conditional and atomic Foo/set
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-conditional-02]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-conditional-02
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-types`]: https://crates.io/crates/jmap-types
[`jmap-server`]: https://crates.io/crates/jmap-server
