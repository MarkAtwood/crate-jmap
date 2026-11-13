# Design: draft-ietf-jmap-conditional-02 in crate-jmap (bd:JMAP-y1yu)

Source: `/data/PROJECT/IETF/specs/draft-ietf-jmap-conditional-02.txt` (26 Oct 2026). All wire
names below are quoted from it. Design only; no code in the repo was changed.

Status: final. Mark's decisions of 2026-11-13 are recorded in `docs/design/DECISIONS.md`. This
note applies decisions 1 (opt-in `ConditionalBackend` registration), 2 (conditional arguments
without the URI), 6(b) (`atomic` covers object-history restores), 7 (error codes as `&str`
consts), 8 (generic `SetCall` with restore options) and 17 (the separate attach call), checks
decision 14 (shared data-type registry) where it touches this capability, and follows decisions 16
(PushSubscription validation without a hook) and 19 (Blob details) where other notes meet it.
Every place a decision changed the draft is marked
"(decision N)". Companion notes: `docs/design/object-history.md`, `docs/design/blobext.md`,
`docs/design/emailpush.md`.

## 1. Summary

Capability `urn:ietf:params:jmap:conditional` adds two arguments to every `Foo/set`:
`ifUnchangedBy` (per-object PatchObject preconditions, failure = method-level `stateMismatch`
with optional `failed`) and `atomic` (all-or-nothing, failure = `atomicFailure`, or
`cannotApplyAtomically` when the backend cannot). No new crate. Wire types and the two error-code
constants go in `jmap-types` (decision 7). The whole evaluation lives once in the `jmap-server`
**dispatcher**, which reads the objects by calling the type's own registered `Foo/get` handler.
Isolation and atomicity go through a separate `ConditionalBackend: JmapBackend` trait whose
`begin_set` hook has no default.

The feature is **opt-in and non-breaking** (decision 1): every existing `register_*_handlers`
keeps its signature and bounds. A deployment opts in by also calling the crate's
`register_<x>_conditional`, which requires `B: ConditionalBackend` and attaches conditional
support to that crate's `/set` methods. The capability is advertised only by a deployment that
opted in for every registered `/set`. The in-repo memory reference backends implement
`ConditionalBackend`, and their tests and the testjig use the opt-in path. One dispatcher check
rejects `atomic: true` or a non-empty `ifUnchangedBy` sent without the URI in `using`, on every
deployment (decision 2).

## 2. Spec coverage

| § | Requirement | Where / why |
|---|---|---|
| 2.1 | Session `capabilities` has `"urn:ietf:params:jmap:conditional"`, value MUST be `{}` | `jmap-types::conditional::{JMAP_CONDITIONAL_URI, ConditionalCapability}`; Session is the consumer's job. Advertise only when `Dispatcher::conditional_ready()` is true (opted in, no gaps) (decision 1); testjig advertises it |
| 2.1 | With the URI in `using`, MUST honour `ifUnchangedBy` "on every Foo/set method it implements" | Dispatcher path for every `*/set` with attached `SetSupport`; `Dispatcher::conditional_gaps()` lists the ones that would break this, and `conditional_ready()` is false while any exist (decision 1); testjig asserts ready |
| 2.1 | Without the URI in `using` | Spec-silent. `atomic: true` or a non-empty `ifUnchangedBy` → `invalidArguments` on every `*/set`, opted in or not (decision 2); default values accepted and stripped |
| 2.1, 4.3 | MUST honour `atomic` or reject with `cannotApplyAtomically`; MUST NOT apply an atomic method in part | `ConditionalBackend::begin_set` → `BeginSetError::CannotApplyAtomically`; the handler never runs without a scope |
| 2.1 | Generic, applies to all types | One implementation in `jmap-server::conditional` |
| 3 | `ifUnchangedBy: "Id[PatchObject\|null]"` (default `{}`) | `jmap-types::conditional::IfUnchangedBy`; `null` argument = default (workspace `optional_arg` convention) |
| 3 | Satisfied iff every pointer's current value equals the given value | Direct comparison (6.1: "MAY instead compare the referenced values directly") |
| 3 | JSON `null` matches a property that is not present | `null` matches an absent member or a member whose value is `null` |
| 3 | "Comparison uses the same representation the server would return ... from Foo/get" | Objects read by calling the registered `Foo/get` handler with the same caller and `using` |
| 3 | Pointer whose parent does not exist: never satisfied, even `null`; rejected as rule 4 | Parent walk; missing, array or non-object parent → `stateMismatch` (rule 4 allows it) |
| 3 | `null` precondition = object does not exist or is not visible; `{}` = exists and visible | `Foo/get` `notFound` ⇒ nonexistent; id in `list` ⇒ exists |
| 3 | `atomic: "Boolean"` (default false) | `null` = default; any other non-boolean → `invalidArguments` (not `bool_arg`'s permissive coercion: a coerced `false` would apply an atomic request in part) |
| 3.1 r1 | Evaluated against state at method start | Inside the scope, before the handler runs (so before an object-history restore phase) |
| 3.1 r2 | Keys are ids of the method's type; `#creation` keys stand for the produced id; no such object of this type ⇒ nonexistent | `resolve_creation_ref` over `CallContext::created_ids`; unresolved ⇒ nonexistent; an id of another type ⇒ `Foo/get` `notFound`. A `#c1` created by the same method is unresolved by rule 1 |
| 3.1 r3 | MAY reference server-set / read-only properties | No `CLIENT_SETTABLE` filter |
| 3.1 r4 | Invalid pointer or malformed PatchObject: MUST reject the whole method (`invalidArguments` or `stateMismatch`) | Request-only faults → `invalidArguments`; faults that depend on the stored object → `stateMismatch`. The spec allows either and "need not distinguish", so this is a design choice |
| 3.1 r5 | `ifInState` first, then preconditions; either fails ⇒ `stateMismatch`, no change | Dispatcher compares `ifInState` with the `/get` response `state` before evaluating. Types whose `/get` has no `state` keep their own `ifInState` rule in the handler (§5.2 step 6) |
| 3.2 | Any unsatisfied ⇒ MUST make no change, method-level `stateMismatch`, state string MUST be unchanged | Returned before the handler runs; scope rolled back, nothing written |
| 3.3 | `stateMismatch` MAY carry `failed: "Id[]"`; error "discloses no object data" | Sent when every failing key has an `Id`; omitted (MAY) when a failing key is an unresolved creation id, so a non-Id never appears in an `Id[]`. `description` never quotes values. Carried in `JmapError.extra` (decision 7) |
| 4 | MUST commit every create/update/destroy or none | Scope `commit`/`rollback`; any `not*` entry ⇒ rollback. Restores too (decision 6(b)) |
| 4.1 | Constraints checked once against the resulting state | `SetMode::Atomic`: backend defers cross-object checks (uniqueness, cycles, single default/active, children-on-destroy, **quotas and caps**) to `commit`; handlers skip their defense-in-depth cross-object pre-checks when `CallContext::atomic` |
| 4.2 | Any SetError ⇒ MUST make no change, MUST be `atomicFailure`, state MUST be unchanged; MAY carry `notCreated`/`notUpdated`/`notDestroyed`; MAY report only the first | Dispatcher moves the handler's `not*` maps (plus deferred commit failures) into the error; succeeded ids omitted. `notRestored` carried too (decision 6(b), spec gap raised upstream) |
| 4.2 | Failed precondition is `stateMismatch` even with `atomic` | Preconditions run before the handler |
| 4.3 | Cannot apply ⇒ MUST `cannotApplyAtomically`, no change; MUST NOT fall back | `begin_set` refusal; backends MAY refuse per type or size |
| 4.3 | Unknown error treated as `serverFail` by old clients | `CANNOT_APPLY_ATOMICALLY` const, decoded as `MethodErrorType::Other` (decision 7); base-client rustdoc |
| 5.1–5.9 | Examples | Tests (§10) |
| 6.1 | Evaluate and apply "within the same lock or transaction" | `begin_set` `Isolated` contract; required hook, no default |
| 6.2 | Transactional store: one transaction, deferred constraints, rollback | `SetScope` contract; memory `ScopedState` |
| 7 | Implementation status | N/A (editorial) |
| 8.1–8.3 | IANA: capability, `atomicFailure`, `cannotApplyAtomically` | `JMAP_CONDITIONAL_URI`, `ATOMIC_FAILURE`, `CANNOT_APPLY_ATOMICALLY` consts in `jmap-types::conditional`; no `MethodErrorType` variants (decision 7) |
| 9.1 | MUST require the same permissions as `Foo/get`; unreadable property MUST give `forbidden`; invisible object ⇒ `stateMismatch` | The dispatcher runs the `set_guard` for `"<type>/get"` (guard `Err` → `forbidden`); `forbidden` when `Foo/get` rejects the property (PushSubscription `url`/`keys`), when the `/get` handler marks it unreadable (`HandlerOutput::with_unreadable`), or when `ConditionalBackend::hidden_properties` names it; checked before any value |
| 9.2 | Existing `/set` limits apply; MAY reject a too-large atomic method | `maxObjectsInSet` unchanged (handler; restores counted per object-history's decision 6(a) helper); `SetScopeRequest` carries the args so the backend can refuse; `ifUnchangedBy` size bounded by `/get`'s `maxObjectsInGet` (`requestTooLarge`) |
| 9.3 | Cost comparable to update validation | N/A |

## 3. Crate placement

No new crate. Extend:

- `jmap-types` (foundation types): `conditional` module (URI, capability, argument and
  `AtomicFailure` types, the `ATOMIC_FAILURE`/`CANNOT_APPLY_ATOMICALLY` consts and their
  `JmapError` constructors and readers) and `JmapError.extra` (decision 7). The capability is not
  tied to a data type, so it does not belong in an extension `*-types` crate. No async.
- `jmap-server` (foundation server): `conditional` module (args, evaluator, dispatcher integration,
  the decision-2 check, `ConditionalBackend`, `SetScope`, `SetSupport`),
  `Dispatcher::attach_set_support`/`conditional_gaps`/`conditional_ready` (decision 1),
  `HandlerOutput` unreadable marks, `CallContext.atomic`, and `ScopedState<T>` (feature
  `scoped-state`).
- Every crate that registers a `/set`: mail, chat, calendars, tasks, contacts, filenode, sharing,
  vtc, scene, graph (including `GraphNodeOp/set`), chat-mimi (`KeyPackage/set`), plus
  `jmap-server::push` (`PushSubscription/set`). Change: a new opt-in `register_<x>_conditional`
  fn (decision 1), the `ConditionalBackend` impl for the memory backend, memory backend on
  `ScopedState`, atomic final-state checks. `register_<x>_handlers` is untouched.
- `jmap-chat-tasks-server`: a `hidden_properties` helper for Task `chatId`.
- `jmap-base-client`: generic `SetCall` carrying conditional and object-history `restore`
  options (decision 8), `Session::supports_conditional`, error decoding.
- `jmap-filenode-client`: `filenode_set_conditional` (decision 8).
- `jmap-testjig`: opt in, advertise, readiness assertion, HTTP end-to-end tests.

Dependency edges: none new. `jmap-server` adds tokio feature `sync` behind `scoped-state`
(`task_local!` needs only `rt`, already on). Each `*-server` `memory` feature enables
`jmap-server/scoped-state`. testjig stays `publish = false`. `jmap-metadata-server` and the binding
crates (`chat-*-server`, `scene-vtc-server`, `graph-projection-server`) register no `/set`; writes
they make from inside a consumer backend join the open scope automatically.

Why the dispatcher, not per-crate handlers: there is no shared `handle_set` (about 45 hand-written
`/set` handlers), but every type has a registered `Foo/get` whose output is by definition the §3
representation, including per-caller visibility, `metadata` projection, Email `header:*` forms and
PushSubscription's `forbidden` on `url`/`keys`. Reusing it gives §3/§9.1 semantics with no per-type
evaluator and no drift from `/get`. It also lets the opt-in compose with object-history: the
dispatcher wraps whichever `/set` handler is registered, plain or `_with_history`.

## 4. Wire types (`jmap-types`)

```rust
// crate-jmap-types/src/conditional.rs
/// §2.1, §8.1.
pub const JMAP_CONDITIONAL_URI: &str = "urn:ietf:params:jmap:conditional";
/// §4.2, §8.2. Extension method error, built with `JmapError::custom` (decision 7).
pub const ATOMIC_FAILURE: &str = "atomicFailure";
/// §4.3, §8.3. Extension method error, built with `JmapError::custom` (decision 7).
pub const CANNOT_APPLY_ATOMICALLY: &str = "cannotApplyAtomically";

/// §2.1: "The value of this property MUST be an empty object." `extra` per the extras policy.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConditionalCapability {
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// §3 `ifUnchangedBy: "Id[PatchObject|null]"`. Keys are `String`, not `Id`: §3.1 rule 2 allows a
/// `#`-prefixed creation id, which `Id`'s validating Deserialize rejects (same choice as
/// `SetResponse::created`). `None` = the JSON `null` "object does not exist" precondition.
pub type IfUnchangedBy = BTreeMap<String, Option<PatchObject>>;

/// The two §3 arguments, for clients to merge into `Foo/set` arguments.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalSetArgs {
    /// "ifUnchangedBy"
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub if_unchanged_by: IfUnchangedBy,
    /// "atomic"
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub atomic: bool,
}
impl ConditionalSetArgs {
    /// True when both arguments are at their defaults (nothing to send, no URI needed).
    pub fn is_default(&self) -> bool;
}

/// §4.2 `atomicFailure` details: "notCreated", "notUpdated", "notDestroyed" of the Foo/set shape.
/// `extra` keeps anything else, in particular object-history `notRestored` (decision 6(b)).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtomicFailure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_created: Option<BTreeMap<String, SetError>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_updated: Option<BTreeMap<Id, SetError>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_destroyed: Option<BTreeMap<Id, SetError>>,
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
impl AtomicFailure {
    /// "notRestored" (decision 6(b) stopgap; not named by either draft yet). None if absent or
    /// malformed.
    pub fn not_restored(&self) -> Option<BTreeMap<Id, SetError>>;
    pub fn with_not_restored(self, not_restored: BTreeMap<Id, SetError>) -> Self;
}

// Inherent constructors and readers, defined in conditional.rs (same crate as JmapError).
impl JmapError {
    pub fn state_mismatch_failed(failed: Vec<Id>) -> Self;   // §3.3 "failed": "Id[]"
    pub fn atomic_failure(details: AtomicFailure) -> Self;   // custom(ATOMIC_FAILURE) + extra
    pub fn cannot_apply_atomically() -> Self;                // custom(CANNOT_APPLY_ATOMICALLY)
    pub fn failed(&self) -> Option<Vec<Id>>;                  // typed readers over `extra`;
    pub fn atomic_failure_details(&self) -> Option<AtomicFailure>; // None unless type is
                                                                   // ATOMIC_FAILURE and well formed
}
```

`notRestored` stays in `AtomicFailure.extra` behind a typed accessor rather than becoming a field:
it is a stopgap name that neither draft defines yet (decision 6(b)), so the typed surface does not
commit to it.

`error.rs` changes (decision 7):

```rust
// MethodErrorType: unchanged. Its documented rule stays: RFC 8620 codes are variants, extension
// codes are `Other`. "atomicFailure" and "cannotApplyAtomically" decode to `Other(..)` as today.
pub struct JmapError { /* type, description, limit, limit_name as today */
    /// RFC 8620 §3.6.2 "Other properties may be present": conditional §3.3 `failed`, the §4.2
    /// maps and `notRestored`.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
```

The two codes follow the existing extension-error precedent: `&str` consts used with
`JmapError::custom`, like graph `OPS_COMPACTED` and `QUERY_TOO_COMPLEX` and scene
`CONTENT_POLICY` (decision 7). This is non-breaking: no `MethodErrorType` variant is added, so
nothing that matches `Other("atomicFailure")` changes behaviour.

`JmapError.extra` is still needed under decision 7. The consts only replace the `type` variants;
`stateMismatch`'s `failed` and the `atomicFailure` maps (`notCreated`, `notUpdated`,
`notDestroyed`, `notRestored`) are extra properties of the method error, and `JmapError` has no
place for them today. `JmapError` is `#[non_exhaustive]`, so the field is additive. `extra` is a
`Map` (24 bytes with the default BTreeMap), not typed `Option` fields, to keep `JmapError` small in
every handler `Result`. The size check is a unit test, not a static assert: a consumer that turns
on `serde_json/preserve_order` changes `Map`'s size. `serde_json::Map` is `Eq`, so the existing
`Eq` derive holds.

## 5. Server design (`jmap-server`)

### 5.1 Registration

Opt-in, composable, and with no change to any existing registration fn (decision 1):

```rust
/// Conditional support for one Foo/set method (draft-ietf-jmap-conditional-02).
pub struct SetSupport<C> {
    data_type: &'static str,                       // "Mailbox"; Foo/get is "<data_type>/get"
    properties: [&'static [&'static str]; 2],      // O::SERVER_SET, O::CLIENT_SETTABLE
    backend: Arc<dyn DynConditional<C>>,           // object-safe adapter over B: ConditionalBackend
}
impl<C> SetSupport<C> {
    pub fn new<O: SetObject, B: ConditionalBackend<CallerCtx = C>>(backend: Arc<B>) -> Self;
}
impl<C> Dispatcher<C> {
    /// Attach conditional support to an already registered "<type>/set". Errors if `method` is
    /// not registered or does not end in "/set". `register` of the same name later drops the
    /// support, so an overriding handler must opt in again (it then shows in `conditional_gaps`).
    pub fn attach_set_support(&mut self, method: &str, support: SetSupport<C>)
        -> Result<(), RegistrationError>;
    /// Registered "*/set" methods without SetSupport or without a registered "*/get".
    pub fn conditional_gaps(&self) -> Vec<String>;
    /// At least one SetSupport attached and `conditional_gaps()` empty. A server MUST NOT
    /// advertise the capability unless this is true (§2.1 "every Foo/set").
    pub fn conditional_ready(&self) -> bool;
}
```

Per crate: one public opt-in fn beside the unchanged `register_<x>_handlers`:

```rust
/// Opt in to draft-ietf-jmap-conditional-02 for this crate's registered Foo/set methods.
/// Call after `register_<x>_handlers` or `register_<x>_handlers_with_history`.
pub fn register_<x>_conditional<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where B: XBackend + ConditionalBackend + 'static;
```

It attaches `SetSupport::new::<Type, B>` to each of the crate's `/set` methods that is registered
and skips those that are not (feature-gated or separately registered sets: mail's
`SieveScript/set` under `register_sieve_handlers`, graph's `GraphNodeOp/set` under
`register_graph_edit_handlers`), so one call covers every registration combination of the crate.
`jmap-server::push` gets `register_push_subscription_conditional<B: PushSubscriptionBackend +
ConditionalBackend>`. A consumer that never calls these fns sees no API or bound change.

This follows decision 1's pattern (an opt-in registration path with the extra trait bound; the
existing fn unchanged) as a separate additive attach call, confirmed by decision 17 after the
MDN/Sieve/graph-edit precedent; there are no `register_<x>_handlers_with_conditional` variants.
Object-history already adds `register_<x>_handlers_with_history`, and a second `_with_` axis would
need `_with_history_and_conditional` too. Because the dispatcher wraps the handler rather than the
handler calling the evaluator, the attach call works unchanged over whichever `/set` is registered,
plain or history-aware, and `conditional_ready()` gates advertisement (decisions 1, 17).

`properties` is needed because the generic `handle_get` does not reject unknown property names
(`handlers.rs` "ponytail" at the `properties` parse), so `/get` alone cannot enforce rule 4.

`CallContext` gains `pub atomic: bool` (non-exhaustive, additive). `HandlerOutput` gains a
non-wire `unreadable: BTreeMap<Id, BTreeSet<String>>` with `with_unreadable()`, set by `/get`
handlers that blank or drop a property per caller (SceneRegion `invitedUserIds`, VTCLivestream
`streamKey`/`streamUri`); the dispatcher never serializes it. The draft's
`JmapHandler::set_support` and `ClosureHandler::new_set` are dropped: support lives in the
dispatcher, keyed by method name (decision 1).

### 5.2 Dispatch flow (`conditional::run`)

Lookups that need `&self` (the attached `SetSupport`, the `"<type>/get"` handler `Arc`, and the
guard result for `"<type>/get"`) happen before `task::spawn`; everything else runs inside the
spawned task. Applies to every method whose name ends in `/set`.

1. `using` lacks the URI (decision 2): if `atomic` is present with any value other than `null` or
   `false`, or `ifUnchangedBy` is present with any value other than `null` or `{}`, return
   `invalidArguments` ("ifUnchangedBy/atomic require urn:ietf:params:jmap:conditional in using")
   and do not call the handler. Otherwise remove both keys and take today's path. This runs on
   every deployment, opted in or not. It covers decision 2's two cases (`atomic: true`, non-empty
   `ifUnchangedBy`) and also malformed values, which no handler reads anyway. Precedent: the mail
   server rejects metadata writes without the metadata URI (`crate-jmap-mail-server/src/helpers.rs`,
   `check_metadata_entry`).
2. `take_args`: remove `ifUnchangedBy`/`atomic`. `null` = default. Wrong JSON types, an entry that is
   neither object nor `null`, a non-`#` key failing `Id::new_validated`, `check_patch_paths` errors
   (prefix clash, bad escape, depth) → `invalidArguments`. Both empty/false → today's path.
3. No `SetSupport` attached → `invalidArguments` ("conditional Foo/set not supported"). Reached
   when a deployment that did not opt in (or opted in only partly) lets the URI through its
   `check_known_capabilities` list; a deployment that advertises only when `conditional_ready()`
   never reaches it (decision 1).
4. `begin_set(caller, SetScopeRequest { scope, method, data_type, account_id, mode, args })`, mode
   `Atomic` if `atomic` else `Isolated`. `CannotApplyAtomically` → that error; `Other` → redacted
   `serverFail` (`server_fail_from_backend`). Everything below runs under `SET_SCOPE.scope(id, ..)`
   (tokio `task_local!`); every early return rolls back, and so does drop (panics).
5. If `ifUnchangedBy` is non-empty: the guard for `"<type>/get"` must pass, else `forbidden`
   (§9.1 "the same permissions ... as ... Foo/get"). Resolve keys (`#c` via `created_ids`;
   unresolved ⇒ nonexistent). Call the registered `"<type>/get"` handler with
   `{accountId (if the set args had one), ids, properties: top-level names of all pointers + "id"}`
   and the same caller/`using`. A method error from it (`forbidden`, `invalidArguments`,
   `requestTooLarge`, `accountNotFound`) is returned as is.
6. `ifInState` (rule 5): if present and the `/get` response has `state`, compare; mismatch →
   `stateMismatch` without `failed`. If `/get` has no `state` (PushSubscription, Blob), leave the
   argument to the handler: RFC 8620 §7.2.2 says PushSubscription/set does "not take an "ifInState"
   argument" (ignored today), blobext-03 says Blob/set rejects a non-null one with
   `invalidArguments`.
7. For each entry: absent from `list` ⇒ nonexistent. Then, over all entries in order: a top-level
   name in neither `SERVER_SET ∪ CLIENT_SETTABLE` nor any returned object (admits `header:*`,
   `metadata`, `digest:*`) → `invalidArguments`; a referenced top-level name in `unreadable` or
   `hidden_properties` for a visible object → `forbidden`; then values. Readability is checked
   before any value so `forbidden` never depends on a value (§9.1).
8. Any unsatisfied → `stateMismatch`, with `failed` = the failing ids (resolved), or without
   `failed` if one failing key is an unresolved creation id.
9. Call the inner handler (plain or history-aware) with stripped args and `cx.atomic`. A
   history-aware handler runs its restore phase here, inside the scope.
10. Not atomic: `commit` (failure → `serverFail`); return the handler output unchanged.
11. Atomic: handler `Err(e)` → rollback, return `e`. `Ok(out)` with a non-empty `notCreated`/
    `notUpdated`/`notDestroyed`/`notRestored` → rollback, `atomicFailure` with those maps
    (`notRestored` via `with_not_restored`, decision 6(b)), `extra_invocations` dropped. Else
    `commit`: `Rejected(failures)` → `atomicFailure`, mapping each `SetOp::Create(id)` back to its
    creation id through the handler's `created` map and each `SetOp::Restore(id)` into
    `notRestored` (backend already rolled back); `Other` → `serverFail`. Success → output as is
    (dispatcher then harvests `createdIds`).

Value comparison (step 7): split the pointer per RFC 6901 with `~0`/`~1` decoding (make
`helpers::decode_pointer_token` `pub(crate)`); every parent must exist and be a JSON object, else
unsatisfied; leaf: expected `null` ⇔ member absent or `null`; otherwise `serde_json::Value`
equality with the `/get` value (`1` vs `1.0` differs, as applying the patch and comparing would).

Error mapping: arg shape / syntax / unknown property, or conditional args without the URI
(decision 2) → `invalidArguments`; unreadable or `/get` guard refusal → `forbidden`; `ifInState`
or precondition → `stateMismatch` (+`failed` only for preconditions); atomic SetErrors or deferred
constraints → `atomicFailure` (decision 7: `JmapError::atomic_failure`); backend refusal →
`cannotApplyAtomically`; storage errors → redacted `serverFail`.

### 5.3 Capability advertisement

Session building stays the consumer's. The kit gives `JMAP_CONDITIONAL_URI`,
`ConditionalCapability`, `conditional_gaps()` and `conditional_ready()`. A deployment advertises
the capability, and adds the URI to the `known` list of `check_known_capabilities`, only when it
opted in and `conditional_ready()` is true (decision 1). A deployment that did not opt in leaves
the URI unknown, so a request naming it gets `unknownCapability`. Session-level only (the spec
defines no account capability), like `urn:ietf:params:jmap:websocket`.

## 6. Backend hooks

A separate foundation trait, like `PushSubscriptionBackend: JmapBackend`, required only by the
opt-in `register_<x>_conditional` fns, so `JmapBackend`'s 52 in-tree impls and every consumer
backend that does not opt in are untouched (decision 1):

```rust
pub trait ConditionalBackend: JmapBackend {
    /// Required. §6.1 isolation and §4 atomicity.
    fn begin_set(&self, caller: &Self::CallerCtx, request: SetScopeRequest<'_>)
        -> impl Future<Output = Result<Box<dyn SetScope>, BeginSetError<Self::Error>>> + Send;
    /// §9.1. Default `Ok(empty)`: permission hook, fail-open (bd:JMAP-bghk).
    fn hidden_properties(&self, caller: &Self::CallerCtx, account_id: Option<&Id>,
        data_type: &str, ids: &[Id])
        -> impl Future<Output = Result<BTreeMap<Id, Vec<String>>, Self::Error>> + Send;
}
pub trait SetScope: Send {   // dropping without commit/rollback MUST roll back
    fn commit(self: Box<Self>) -> Pin<Box<dyn Future<Output = Result<(), CommitError>> + Send>>;
    fn rollback(self: Box<Self>) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}
#[non_exhaustive] pub enum SetMode { Isolated, Atomic }
#[non_exhaustive] pub enum BeginSetError<E> { CannotApplyAtomically, Other(E) }
#[non_exhaustive] pub enum CommitError { Rejected(Vec<DeferredFailure>), Other(Box<dyn Error + Send + Sync>) }
#[non_exhaustive] pub struct DeferredFailure { pub op: SetOp, pub error: SetError }
#[non_exhaustive] pub enum SetOp { Create(Id), Update(Id), Destroy(Id), Restore(Id) } // minted/stored ids
pub fn current_set_scope() -> Option<SetScopeId>;   // ambient scope of the running method
```

`begin_set` contract: open a lock or transaction over everything the method may read or write in
`account_id` (all types: cascades such as `onDestroyRemoveEmails` and the implicit
`onSuccessUpdateEmail` Email/set run inside it). Every backend call made while
`current_set_scope()` is `Some(scope)` belongs to it; a backend that spawns tasks captures the id
first (task-locals do not cross `spawn`). `get_state` inside the scope sees the scope's writes, so
the handler's `newState` is right. `Isolated`: no other writer commits to the account between the
precondition reads and the method's writes (§6.1). `Atomic` additionally: evaluate cross-object
constraints (uniqueness, cycles, single default/active, children-on-destroy, quotas and caps)
against the final state at `commit`, return violations as `Rejected` after rolling back, keep the
state string and change log unchanged on rollback (§4.2), and defer irreversible external effects
(mail send, outbound federation, push `StateChange`, stream stop, event delivery) to commit, or
refuse with `CannotApplyAtomically` (§4.3; Cyrus refuses immediate sends). Object-history
`restore_object` calls made in the scope are part of the atomic unit; a backend that cannot restore
inside its transaction refuses with `CannotApplyAtomically` (decision 6(b)). MAY refuse large
methods (§9.2).

Defaults, under the spec-method-defaults rule:

- `begin_set`: **no default.** A no-op `Isolated` scope lets a concurrent write land between check
  and apply, so the server answers success for a write §3.2 says must be `stateMismatch`: wrong data
  on the wire. Because the trait is required only on the opt-in path, the missing default costs
  nothing to a deployment that does not opt in (decision 1). `jmap-server` ships
  `serialized_writes_scope(request)` for backends that already serialize every write per account
  (refuses `Atomic`, no-op `Isolated`), documented as correct only under that condition; test
  mocks use it.
- `hidden_properties`: default `Ok(empty)` (permission hook, fail-open). Consumers that redact per
  caller override it; `jmap-chat-tasks-server` gains `task_hidden_properties` (Task `chatId` via
  `ChatBackend::may_view_chat`, the same decision as `redact_task_chat_ids`).

No new per-object hooks: extension `create_object`/`update_object`/`destroy_object` keep their
signatures; the scope is ambient. Backends that enforce invariants "atomically with the write"
(FileNode names/cycles) defer those checks to commit when their scope is `Atomic`; the backend
still enforces them, so this is not a handler/backend opt-in flag.

## 7. Memory reference backend scope

`jmap-server::ScopedState<T: Clone + Send>` (feature `scoped-state`) replaces each memory backend's
`Arc<Mutex<Inner>>`. Every in-repo memory backend implements `ConditionalBackend`, so the
reference servers can opt in (decision 1):

- `async fn lock(&self) -> ScopedGuard<'_, T>`: waits on the gate unless the current task owns the
  open scope, then takes the std mutex. All `self.inner.lock()` sites in async methods become
  `.lock().await`, reads included: a reader that saw an open atomic method's writes would see a
  change §4 says never happened, so read gating is required, not optional. Sync methods and seeding
  helpers use `lock_unscoped()`. With no scope ever opened (a deployment that did not opt in) the
  gate is always free, so behaviour is as today.
- The gate is a shared `Arc<ScopeGate>` (a `tokio::sync::Mutex<()>`), per backend by default, with
  `with_gate()` so a deployment can give all its memory backends one gate. Two backends with
  separate gates that call each other (binding crates) can otherwise deadlock on lock order; the
  testjig uses one shared gate.
- `async fn open(&self, req, validate) -> Box<dyn SetScope>`: takes the gate, records the owner,
  snapshots `T` (whole-store clone; demonstration quality; `Isolated` may skip the snapshot because
  nothing is written before the handler). `rollback`/drop restores the snapshot synchronously;
  `commit` in `Atomic` first runs the crate's `validate(&before, &after) -> Vec<DeferredFailure>`.
  `deferring()` tells per-object methods to skip cross-object checks.
- Each memory `Inner` adds `#[derive(Clone)]` (all are plain maps today, chat-mimi included: its
  `Inner` holds no openmls storage, so every memory backend supports both modes). Side-effect queues
  (VTC events, chat outbound, mimi hub requests) live in `Inner`, so rollback drops them. Memory
  object-history `VersionLog`s live in `Inner` too, so a rolled-back restore leaves no version
  behind (decision 6(b)).

Known final-state checks per crate (each bead audits its own): mail Mailbox name+parent, role,
parent cycle, SieveScript name and single active; filenode sibling names, move-under-self,
`nodeHasChildren`; contacts/calendars/tasks single default collection where specified; scene one
avatar per region, region nesting; vtc participant `alreadyExists`; graph edge endpoints after node
destroys; chat CustomEmoji/SpaceBan uniqueness; chat-mimi KeyPackageRef reuse and
`maxKeyPackages`; every crate's `overQuota` and per-account caps.

## 8. Client bindings (`jmap-base-client`)

- `Session::supports_conditional()` (capability present), after `supports_cid`.
- `SetCall` (decision 8): a generic `Foo/set` call for any type, carrying the conditional and the
  object-history options together.

  ```rust
  #[non_exhaustive]
  #[derive(Debug, Clone, Default, PartialEq)]
  pub struct SetCall {
      pub account_id: Option<Id>,
      pub if_in_state: Option<String>,
      pub create: BTreeMap<String, Map<String, Value>>,
      pub update: BTreeMap<Id, PatchObject>,
      pub destroy: Vec<Id>,
      pub conditional: ConditionalSetArgs,             // conditional-02 §3
      pub restore: BTreeMap<Id, Option<PatchObject>>,  // object-history-03 §5.1; empty = omitted
      pub extra: Map<String, Value>,                   // any other per-type argument
  }
  ```

  `JmapClient::set_call::<R: DeserializeOwned>(session, method, extra_using, &call)` builds a
  request whose `using` is core + `extra_using` + the conditional URI when `conditional` is not
  default, and decodes the primary response as `R` (`SetResponse<PartialObject>` for account
  types, with `restored`/`notRestored` in its `extra`; it cannot decode PushSubscription/set, which
  has no `accountId`). The object-history URI comes from the caller in `extra_using`:
  `jmap-base-client` does not depend on `jmap-object-history-types`, and the object-history
  client builds its restore call on `SetCall` and adds the URI (decision 8). The caller checks
  `supports_conditional()`; a server that does not advertise it answers `unknownCapability`.
- `ConditionalError::from_method_error(&JmapError) -> Option<ConditionalError>` with
  `StateMismatch { failed: Option<Vec<Id>> }`, `AtomicFailure(AtomicFailure)`,
  `CannotApplyAtomically`; it matches `MethodErrorType::StateMismatch` and
  `Other(s)` against the `ATOMIC_FAILURE`/`CANNOT_APPLY_ATOMICALLY` consts (decision 7). Rustdoc
  says an older client treats the last as `serverFail` (§4.3).
- Typed per-crate bindings: only FileNode, the spec's example type, gets
  `filenode_set_conditional` in `jmap-filenode-client` now (decision 8). It is a new function, not
  new fields on `FileNodeSetParams`: that struct is not `#[non_exhaustive]`, and `filenode_set`
  fixes `using` to `USING_FILENODE`. It wraps `SetCall`.
- Next breaking release (decision 8, follow-up bead C13): all 30 typed `*_set` fns gain one
  growable `#[non_exhaustive]` options parameter carrying the conditional and object-history
  options together, so later per-set options need no further signature change.

## 9. Interactions, breaking changes

**blobext (JMAP-tna1.2).** `Blob/set` lives in the new `jmap-blob-server`; it gets
`register_blob_conditional`, attaching `SetSupport::new::<BlobObject, B>` (BlobObject needs a
`SetObject` impl), or `conditional_gaps` flags it (decisions 1, 17). blobext bead B18 is only that
attach fn plus the memory scope: it does not re-implement evaluation in `set.rs`, and the
dispatcher's `begin_set(Atomic)` is the atomic hook, so there is no `blob_set_atomic` hook and no
`ClosureHandler::new_set` (decision 17). Preconditions read via `Blob/get` (`digest:<alg>` names
pass step 7 because `/get` returns them; `size`; `{}` and `null`). A pointer to `type` or
`expires`, which `Blob/get` never returns, gets `invalidArguments` under §3.1 rule 4: the internal
`Blob/get` rejects the unknown property name and step 5 returns that error as is (decision 19).
Blob/set's own `ifInState` rule stays in its handler (step 6). Atomic Blob/set: rollback discards
created blobs, including `noPersist` ones. `Blob/lookup` and upload: not `/set`, untouched by the
dispatcher. `Blob/convert` is not `/set` either; its own handler answers `invalidArguments` to
`atomic: true` or a non-empty `ifUnchangedBy` (decision 19).

**object-history (JMAP-tna1.3).** `restore` runs "before any create, update, or destroy";
preconditions still come first (§3.1 r1), so `{"x": null}` holds for a destroyed `x` being
restored. The internal `Foo/get` passes no `includeDestroyed`/`includeReplaced`, so destroyed
objects are nonexistent and `objectHistory` is an unknown name (`invalidArguments`). The restore
phase runs inside the handler, so inside the scope. `atomic` covers restores, a `notRestored`
entry fails the method, and `atomicFailure` carries it as `notRestored` (decision 6(b): a
stopgap beyond §4.2's three maps, allowed by RFC 8620 §3.6.2's extra properties, raised with both
drafts' authors; see the upstream list in `docs/design/DECISIONS.md`). `SetOp::Restore` covers
deferred failures. Registration composes (decision 1): a deployment calls
`register_<x>_handlers_with_history` and then `register_<x>_conditional`, which attaches to the
history-aware `/set`. Conditional adds no shared `/set` pipeline and does not touch
`enforce_max_objects_in_set` (restores count toward `maxObjectsInSet` through object-history's own
helper, decision 6(a)); its only `helpers.rs` edit is one visibility change. Both epics add one
`mod`/re-export line to `jmap-server` `lib.rs` (object-history H3), a trivial rebase. `SetCall`
is the restore request vehicle for the object-history client (decision 8), so object-history H10
builds on bead C6. The data-type registry's "Supports History" column (decision 14) is
object-history's; conditional applies to every `/set` regardless of that column.

**Data-type registry (decision 14).** The shared registry in `jmap-types` has no conditional
column, and the spec defines none: conditional is generic over every `Foo/set`. The dispatcher
uses each `SetSupport`'s own `data_type` and does not read the registry, so no conditional bead
depends on the registry bead.

**emailpush (JMAP-r44n).** `PushSubscription/set` gets `register_push_subscription_conditional`
in `jmap-server::push` (no `accountId`: `SetScopeRequest.account_id = None`, scope keyed by
credentials); `register_push_subscription_handlers` keeps its signature (decision 1).
`PushSubscription/get` already answers `forbidden` for `url`/`keys`, which gives §9.1, as the final
emailpush note (§9) records. `emailPush` pointers without emailpush in `using` are
`invalidArguments` (the gated `/get` rejects the name). PushSubscriptionBackend gains no
validation hook (decision 16): EmailPush validation runs in the backend's
`create_`/`update_push_subscription`, so it runs inside the `begin_set` scope and a rejected config
under `atomic` fails the method and rolls back. There is one `PushSubscription/set` registration
and the attach call wraps it, so nothing can drop the validation last-wins (decisions 16, 17).
emailpush's push.rs seam (emailpush:E1) lands first; this epic's push bead follows it.

**/copy and other non-/set methods.** The spec extends only `Foo/set`. `Foo/copy`, `Email/import`,
`Blob/convert`, `Peer/*`, `Room/*`, `KeyMaterial/fetch`, `SceneObject/interact`,
`GraphNode/cursor`: args untouched by the dispatcher, and the decision-2 check does not apply to
them. Blob/convert's own handler rejects `atomic: true` or a non-empty `ifUnchangedBy` with
`invalidArguments` (decision 19). Implicit
`/set` responses built inside a handler (`onSuccessDestroyOriginal`, `onSuccessUpdateEmail`) carry
no conditional args. For an atomic `EmailSubmission/set` the implicit Email/set runs inside the
scope and rolls back with it, but its own `notUpdated` does not fail the submission (RFC 8621 §7.5
treats it as a separate call "made immediately after"). FileNode/copy's replace-then-copy gap
(JMAP-yin2) can use `begin_set(Atomic)` internally when the backend implements
`ConditionalBackend`.

**FileNode -20.** §3.2.3 already requires sibling-name uniqueness "against the result of all its
creates, updates and destroys together" (JMAP-6xas), and -20 removed `onExists: "unchanged"` in
favour of destroy+create with `ifUnchangedBy`. JMAP-6xas implements final-state evaluation for every
FileNode/set, atomic or not; the filenode bead then only adds rollback.

**Other crates.** metadata: `metadata/<ns>/k` pointers work because `/get` applies
`MetadataProjection` for the same `using`. cid: `sha256` (server-set) is a good CAS target.
sharing: `shareWith/<id>/mayRead` (example 5.3) is plain property comparison. graph-edit
`GraphNodeOp/set` (create-only, backend `seq`) needs the scope to roll back `seq`. chat-federation
`check_method_access` already refuses peer callers every non-`Peer/*` method.

**Compatibility.** No public API break (decisions 1 and 7):
- `jmap-types`: additive `conditional` module; `JmapError.extra` is a new field on a
  `#[non_exhaustive]` struct. `MethodErrorType` is unchanged, so `"atomicFailure"` and
  `"cannotApplyAtomically"` keep decoding to `Other(..)` (decision 7). Behaviour change
  (CHANGELOG "Changed"): decoded errors now keep unknown properties in `extra`, so a decoded error
  that carried extra properties no longer compares equal to a bare constructor.
- `jmap-server`: additive: `ConditionalBackend`, `SetScope` and friends,
  `Dispatcher::attach_set_support`/`conditional_gaps`/`conditional_ready`,
  `register_push_subscription_conditional`, `CallContext.atomic`, `HandlerOutput.unreadable`,
  `scoped-state` feature. `register_push_subscription_handlers` is unchanged (decision 1).
  Wire behaviour change (CHANGELOG "Changed", every deployment): a `*/set` with `atomic: true` or a
  non-empty `ifUnchangedBy` and no conditional URI in `using` now fails with `invalidArguments`
  instead of having those arguments silently ignored (decision 2).
- Every extension server: additive `register_<x>_conditional` and the memory `ConditionalBackend`
  impl; `register_<x>_handlers` unchanged (decision 1). Memory backends move to `ScopedState`
  internally; no wire change without an opened scope.
- public-api snapshots change (additions only) for `jmap-types`, `jmap-server`,
  `jmap-base-client`, `jmap-filenode-client`, `jmap-chat-tasks-server` and every converted server
  crate.
- The decision-8 options parameter on the typed `*_set` fns is the one breaking change, deferred
  to the next breaking release (bead C13).

## 10. Test plan

Oracles are hand-written JSON from the spec, never derived from the evaluator.

`jmap-types`: capability `{}` round trip; `ConditionalSetArgs` with `{"r": null, "f42": {}}` and
`is_default`; example 5.1 error JSON verbatim round trip and `failed()`; `atomicFailure` with all
three maps plus `notRestored` round trip and `not_restored()` (decision 6(b)); both codes decode to
`MethodErrorType::Other` and `JmapError::custom(ATOMIC_FAILURE)` equals the constructor's `type`
(decision 7); a decoded `JmapError` keeps unknown properties in `extra`; `JmapError` size test.

`jmap-server` dispatcher, with probe handlers (`Probe/get` + `Probe/set` over a tiny backend):
rule 1 (precondition sees pre-method values when the same call updates them); rule 2 (`#c1` from an
earlier call; `#c1` that failed ⇒ `null` holds, `{}` fails and the error has no `failed`; `#c1`
created in the same call ⇒ nonexistent); rule 3 (server-set property); rule 4 (prefix clash and
unknown property ⇒ `invalidArguments`; `participants/p1/name` with `null` and no parent ⇒
`stateMismatch`); rule 5 (`ifInState` mismatch ⇒ `stateMismatch` without `failed`, preconditions
not evaluated); §3.2 state string unchanged (get_state before/after); §9.1 invisible ⇒
`stateMismatch`, unreadable mark ⇒ `forbidden` even when the value would match, guard refusing
`Probe/get` ⇒ `forbidden`; `atomic: "true"` ⇒ `invalidArguments`, `atomic: null` ⇒ default;
backend refusing `Atomic` ⇒ `cannotApplyAtomically` and handler never called; atomic with one
SetError ⇒ `atomicFailure` carrying only the failing id; probe handler emitting `notRestored` under
`atomic` ⇒ rollback and `atomicFailure` with `notRestored` (decision 6(b));
`Rejected(Create(id))` from commit ⇒ `notCreated` keyed by the creation id;
`Rejected(Restore(id))` ⇒ `notRestored`; panic inside the scope ⇒ rollback via drop.
Decision 2: without the URI in `using`, `atomic: true` ⇒ `invalidArguments` and handler not
called; `ifUnchangedBy: {"x": {}}` ⇒ `invalidArguments`; `atomic: false`, `atomic: null`,
`ifUnchangedBy: {}` and `ifUnchangedBy: null` ⇒ handler called without those keys; the same
results on a dispatcher with no `SetSupport` attached at all. Decision 1: `conditional_gaps` lists
a `/set` without support and one without `/get`; `conditional_ready()` false with no support,
false with a gap, true when every `/set` is covered; `attach_set_support` on an unregistered method
errors; a later `register` of the same method drops the support; URI in `using` and no support ⇒
`invalidArguments`; `register_<x>_handlers` called alone behaves exactly as before for requests
without conditional args. `PushSubscription/set` via `register_push_subscription_conditional`:
precondition on `keys` ⇒ `forbidden`; `ifInState` still ignored; under `atomic`, a SetError from
the mock's `create_push_subscription` (where extension validation runs, decision 16) ⇒
`atomicFailure` and nothing stored.

`ScopedState`: unscoped `lock()` (read and write) blocks while another task holds a scope; owner
re-entry does not deadlock; two backends on one shared gate do not deadlock across calls; rollback
restores state and change log; drop rolls back; with no scope opened, `lock()` never waits.

Spec examples as integration tests (memory backends, registered with the opt-in fns):
- 5.1 force-with-lease on `blobId` (filenode): pass, then `stateMismatch` with `failed: ["f42"]`.
- 5.2 `Email/set` destroy with `{"keywords/$seen": null}` (mail): unread destroys; `$seen` blocks.
- 5.3 `shareWith/bob/mayRead` (filenode): a concurrent rename of `d3` does not fail it.
- 5.4 precondition on unchanged parent `d3` `name` (filenode).
- 5.5 `{"r": null}` rename (filenode).
- 5.6 `{"f42": {}}` with a create (filenode).
- 5.7 `changed` timestamp CAS (filenode `changed`).
- 5.8 atomic name swap (filenode): succeeds; with a third conflicting rename ⇒ `atomicFailure`
  naming the offender, neither renamed.
- 5.9 atomic replace (update `t` + destroy `r` + `{"r": {"blobId": "G_old"}}`): succeeds; stale
  `G_old` ⇒ `stateMismatch`, neither changed.
Per sibling server crate: one precondition test, one atomic rollback test on its main type, one
atomic test where a quota or cap is only met by the final state, and `conditional_ready()` true
after `register_<x>_handlers` + `register_<x>_conditional` on a fresh dispatcher.
testjig: 5.8 over HTTP; `conditional_ready()` true for the jig's dispatcher; Session advertises
the URI; decision 2 over HTTP (`atomic: true` without the URI ⇒ `invalidArguments`).
base-client: `SetCall` serialization against example 5.9 request JSON; `SetCall` with `restore`
serializes `restore` and leaves the conditional URI out when `conditional` is default (decision 8);
`ConditionalError` from the 5.1 error JSON and from `atomicFailure`/`cannotApplyAtomically` JSON
(decoded through `Other`); request `using` contains the URI only when conditional args are set.

## 11. Bead breakdown

Children of JMAP-y1yu, in order. Per-crate beads touch only their own directory and
`public-api/<crate>.txt`; AGENTS.md, README crate table and CHANGELOG go in bead C12.

| # | Title | Crates | Size | Depends |
|---|---|---|---|---|
| C1 | conditional wire types, `ATOMIC_FAILURE`/`CANNOT_APPLY_ATOMICALLY` consts and constructors (decision 7), `JmapError.extra` | jmap-types | S | – |
| C2 | `ConditionalBackend`/`SetScope`/`serialized_writes_scope`, `current_set_scope`, `SetSupport`, `Dispatcher::attach_set_support` (decision 1), `CallContext.atomic`, `HandlerOutput.unreadable` | jmap-server | M | C1 |
| C3 | dispatcher evaluator, decision-2 check, `/get` guard, atomic outcome mapping incl. `notRestored` (decision 6(b)), `conditional_gaps`/`conditional_ready`, probe tests | jmap-server (`conditional.rs`, `lib.rs`, one line in `helpers.rs`) | L | C2 |
| C4 | `ScopedState<T>` + `ScopeGate` (feature `scoped-state`) | jmap-server (`scoped_state.rs`, `Cargo.toml`) | M | C2; parallel with C3 (shared: `lib.rs` mod/re-export lines and the snapshot, regenerated at merge) |
| C5 | `register_push_subscription_conditional` attach call (decisions 1, 17) over the one registered `PushSubscription/set`; validation stays in the backend's create/update, no hook (decision 16); mocks | jmap-server (`push.rs`, tests) | S | C3; emailpush:E1 |
| C6 | base-client `SetCall` with conditional + restore options (decision 8), `set_call`, `supports_conditional`, `ConditionalError` | jmap-base-client | M | C1 (parallel with C2–C5) |
| C7 | mail-server (canonical): `register_mail_conditional` (decision 1), memory `ConditionalBackend` + ScopedState, Mailbox/SieveScript/quota final-state checks, pre-check skip under `cx.atomic`, example 5.2 | jmap-mail-server | L | C3, C4 |
| C8 | filenode-server: `register_filenode_conditional`, ScopedState, atomic rollback, examples 5.1, 5.3–5.9 | jmap-filenode-server | M | C7, JMAP-6xas |
| C9a–C9i | propagate: chat (L, incl. PushSubscription/set test on chat memory), calendars, tasks (+ `task_hidden_properties` in chat-tasks-server), contacts, sharing, vtc (`streamKey`/`streamUri` marks, event queue), scene (`invitedUserIds` mark), graph (op `seq`, both register paths), chat-mimi | one crate each (tasks: two) | M each | C7 (chat also C5) |
| C10 | filenode-client `filenode_set_conditional` (decision 8) | jmap-filenode-client | S | C6 |
| C11 | testjig: shared gate, opt in for the 7 registered crates, advertise on `conditional_ready()`, 5.8 and decision 2 over HTTP | jmap-testjig | S | C3, C7, C8, C9a–C9e |
| C12 | docs: AGENTS.md (conditional section, `ConditionalBackend` under the hook rule, opt-in registration pattern), README tables, CHANGELOG (Added, plus "Changed" for decision 2 and `JmapError.extra` decoding) | workspace | S | all of C1–C11 |
| C13 (follow-up, next breaking release) | one growable `#[non_exhaustive]` options parameter on all 30 typed `*_set` fns, carrying conditional and object-history options (decision 8) | the 10 typed client crates (calendars, chat-mimi, contacts, filenode, graph, mail, scene, sharing, tasks, vtc) | L | C6 |

JMAP-6xas (final-state sibling names for every FileNode/set) has no conditional dependency and can
land any time before C8. C5 no longer touches chat memory (the push opt-in is a new fn, decision 1),
so it does not conflict with C9a; C9a tests PushSubscription/set on chat memory after C5.

Cross-epic: blobext B18 (`Blob/set` attach call) and object-history's `_with_history` integration
tests depend on C2–C4; object-history H10 (client restore) builds on C6 (decision 8); emailpush E1
(push.rs seam) precedes C5. No conditional bead depends on the data-type registry (decision 14).

## 12. Open questions for Mark

Decided and removed: OQ1 hook placement (decision 1), OQ2 client surface (decision 8), OQ3
arguments without the URI (decision 2), OQ4 error codes (decision 7), OQ5 `notRestored` in
`atomicFailure` (decision 6(b)), blobext B18 versus this note (decision 17: no `blob_set_atomic`
hook, B18 is the attach call plus the memory scope), shape of the opt-in fn (decision 17: the
separate attach call `register_<x>_conditional`); see `docs/design/DECISIONS.md`.

Remaining: none.
