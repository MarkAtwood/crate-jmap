# Toy-consumer check, wave of decisions 41-52 (bd JMAP-698j.3)

Decision 51 makes the freeze audit's toy-consumer step a standing
end-of-wave step (AGENTS.md "End of each feature wave: toy-consumer
check") and runs it now. Four probe crates, kept under
`docs/toy-consumers/`, were built outside the workspace by
`scripts/toy-consumers.sh` against crate-jmap at baabd1a7, using only
public API: no `#[doc(hidden)]` items, no `test-helpers` features, no
`*_for_test` functions. Each one asserts what it exercises and runs
offline (loopback servers only).

| Probe | What it does | Built | Ran |
|---|---|---|---|
| `toy-chat-db-backend` | `DbBackend` wraps chat `MemoryBackend`: own `account_exists` (an account directory), hand-written `ChatRestrictions` (`may_create_message` refuses read-only Chats), `Delegate*` for the other nine hook traits and `DelegateJmapReads`; `register_chat_handlers` on a `Dispatcher`; `Chat/set`, `Message/set` (one create refused), `Message/query` with a `where` filter (`$and` of a field test and `$text`), `accountNotFound` from the directory | yes | yes |
| `toy-multi-server` | One front `Dispatcher<Id>`: FileNode (per-user `MemoryBackend<Id>`, two users, with object history and conditional-02 `atomic`) and Graph registered directly; Mail (with `jmap-sieve`, `jmap-mdn` and `jmap-emailpush` facades, `PushSubscription/set` with `emailPush`, Mailbox object history), Tasks (`Task/query` with `inTaskLists`, `after`/`before`, `timeZone`, `expandRecurrences`) and VTC bridged from inner dispatchers; one request touching each family; bob sees only the shared folder and may not rename it | yes | yes; Graph and VTC stopped at `accountNotFound` (b2) until the rerun below |
| `toy-client-app` | jmap-chat-client `message_query` with `where` and jmap-tasks-client `task_query` with `TaskQueryParams` over HTTP to a loopback server (chat and tasks memory backends); jmap-graph-client `supports_provenance`; jmap-scene-client `send_scene_stream_enable` with a Session over a loopback `ws://` (`connect_ws_allow_plaintext`); jmap-platform-push `FcmPusher` and `ApnsPusher` from generated dummy keys and a `Message`, held as `Box<dyn PlatformPusher>`, nothing sent | yes | yes |
| `toy-slack-bridge` | `SlackToken`, `SlackClient::with_base_url` to a loopback Slack Web API stand-in, `BridgeConfig`, `SlackBridge::connect`; a signed Events API body through `events::verify_signature` and `parse_http_event`; `apply_event` with `channel_created` and `message`; the Chat and Message read back with `Chat/get` and `Message/get` | yes | yes |

## Findings

Severity: **high** blocks a consumer with public API only; **medium**
needs a workaround a consumer has to discover by reading source;
**low** is friction or inconsistency.

| # | Crate | Item | What I needed | What I had to do | Suggested fix | Severity | Status |
|---|---|---|---|---|---|---|---|
| b1 | jmap-server, the reference backends | `CallerCtx` of the memory backends: `()` (mail, tasks, calendars, contacts), `Id` (graph, filenode `MemoryBackend<Id>`), `String` (vtc, scene) | One `Dispatcher<C>` for several families | A `Bridge` `JmapHandler<Id>` per method that forwards a one-call request to an inner `Dispatcher` of the backend's `CallerCtx` (with `createdIds` and `using`), marked with `register_history_aware` on the front. Conditional-02 is lost for bridged families: the front has no `SetSupport` for their `/set` (it needs a `ConditionalBackend` of the front's `CallerCtx`) and refuses `atomic` before the bridge runs (asserted in the probe) | Give the reference backends one caller type (generic `C` with a `MemoryCaller`-style trait, as filenode does), or add a dispatcher mount that maps the caller and carries set support and history marks (`Dispatcher::mount(inner, map_caller)`) | high | open |
| b2 | jmap-graph-server, jmap-vtc-server, jmap-scene-server (`memory`) | `MemoryBackend::register_account` is `#[doc(hidden)]`; no `with_account` or other public way to add an account | A Graph account with provenance; a VTC account | Nothing works: every call is `accountNotFound`. Graph's public `register_provenance` has no account to act on | A public builder like mail's: `with_account(account, owner, capability)` (graph), `with_account(account, capability)` (vtc, scene), and the `register_edit`/`register_search`/`advertise` knobs a deployment needs | high | fixed (JMAP-fhvk) |
| b3 | jmap-object-history-types | `ObjectHistoryAccountCapability` (`#[non_exhaustive]`, no constructor) | The capability for `with_object_history` | `serde_json::from_value(json!({ "maxHistoryDuration": .., "supportedTypes": [..] }))` (the umbrella's own doc example does the same) | `ObjectHistoryAccountCapability::new(supported_types)` and `with_max_history_duration` | medium | fixed (JMAP-m59l) |
| b5 | jmap-mail-server | `Mailbox/set` create (handler builds `MailboxRights::default()`); the memory backend stores and returns it | A Mailbox the account owner can use | Nothing: the owner's new Mailbox has every `myRights` false (and `isSubscribed` false), on `/set` and `/get`. A backend that stores the handler's object as is and names a principal would have the owner's destroy refused by the handler's defense-in-depth check | The backend fills `myRights` for the caller on create (the owner has every right), as filenode and graph do; the handler should not put all-false rights on the wire | medium | open |
| a1 | jmap-server | `JmapAccounts` has no `Delegate*` companion | A wrapper that overrides `account_exists` and keeps the inner backend's other `JmapAccounts` answers | Forward by hand each `JmapAccounts` method the inner backend overrides; nothing lists which (chat `MemoryBackend` overrides `metadata_info`, found by reading source or the impl listing). A forgotten one silently falls back to the trait default | A `DelegateJmapAccounts` whose methods default to the target and can be overridden one by one, or document on each reference backend which `JmapAccounts` methods it overrides | medium | fixed (JMAP-saks) |
| a2 | jmap-chat-server | Hook traits delegate whole (decision 39) | Override one `ChatRestrictions` method | Implement `ChatRestrictions` by hand and forward its other two methods to the inner backend | Show the pattern in the `Delegate*` docs (one overridden method, the rest forwarded); no API change | low | recommendation below (JMAP-14yr) |
| a3 | jmap-chat-server, -mail-, -tasks-, -calendars-, -contacts-, -sharing-server (`memory`) | `with_account` documented as "Same as [`Self::register_account`]", a `#[doc(hidden)]` item (a dead link in rendered docs); sharing's `new_with_accounts` told new code to prefer the hidden method | Learn what `with_account` does | Read source | Document `with_account` on its own terms | low | fixed |
| a4 | jmap-server | `JmapAccounts::metadata_info` returns `jmap_metadata_types::DataTypeMetadataInfo`, not re-exported | Forward `metadata_info` in a wrapper | Add `jmap-metadata-types` as a direct dependency | Re-export it at the jmap-server root (Crate Map rule 6) or say so on the method; a re-export policy call after 5aa871e0 | low | fixed (JMAP-14yr) |
| b4 | jmap-server | `Dispatcher` cannot list its registered methods | Bridge every method an inner dispatcher serves | Spell the method names out | `Dispatcher::methods() -> impl Iterator<Item = &str>` | low | fixed (JMAP-14yr) |
| b6 | jmap-tasks-server (`memory`) | `Task/query` limits: no time-zone database, no recurrence engine | `Task/query` with `timeZone` and `expandRecurrences` | A non-recurring task in the query's time zone; `Europe/Paris` was `unsupportedFilter`, a recurring task `cannotCalculateOccurrences`. Said only on the `query_tasks` impl method | Say it in the `memory` module docs | low | fixed |
| b7 | jmap-filenode-server, jmap-blob-server | `register_filenode_conditional` and `register_blob_conditional` return `Result<(), RegistrationError>`; the other ten `register_*_conditional` return `()` | Register conditional support uniformly | `.expect(..)` on two of them | One shape across families at the next breaking release | low | recommendation below (JMAP-14yr) |
| c1 | jmap-chat-server | `Message/query` `unsupportedFilter` without a `description` | Know why a `where` query failed (no `chatId`) | Read `check_message_filter` | Name the rule in the description (`chatId` or `hasMention: true` required, `text` unsupported, `threadRootId` without threads) | low | fixed (JMAP-14yr) |
| c2 | jmap-tasks-client | `TaskQueryParams` is not `#[non_exhaustive]` (built with `..Default::default()`), while `jmap_base_client::SetOptions` is | Build the params | Struct literal with `..TaskQueryParams::default()` | Decide one convention for `*Params` structs (AGENTS.md Public API rules 3 and 5) | low | recommendation below (JMAP-14yr) |
| d1 | jmap-chat-slack | `SlackClient::with_base_url(&url::Url)`, `from_client(reqwest::Client, ..)`, `new` → `reqwest::Error`: public dependencies not re-exported (jmap-platform-push re-exports both) | Point the client at a Slack stand-in | Add `url` as a direct dependency | `pub use reqwest; pub use url;` | low | fixed |

Not findings: `SlackBridge` is built only through `connect`, which calls
`auth.test`; `SlackClient::with_base_url` is documented for a local test
double, and the probe uses one. `send_scene_stream_enable` needs a
`WsSession`; `connect_ws_allow_plaintext` is the documented route for a
loopback `ws://` rig.

## Fixed here

- d1: jmap-chat-slack re-exports `reqwest` and `url` (public-API snapshot,
  CHANGELOG and Crate Map updated).
- a3: `with_account` docs in six memory backends, and sharing's
  `new_with_accounts`.
- b6: the Tasks memory backend's `Task/query` limits in its module docs.

## Rerun after the fixes

`scripts/toy-consumers.sh` was rerun after the fixes below: all four
probes build, run and pass clippy. The probes now use the new public API:
`toy-multi-server` gives Graph (with provenance) and VTC accounts with
`with_account`, creates a Graph whose owner's `myRights.mayCurate` is
`true`, reads VTC calls, builds the history capability with
`ObjectHistoryAccountCapability::new` and bridges every inner method with
`Dispatcher::methods`; `toy-chat-db-backend` implements
`DelegateJmapAccounts` (overriding only `account_exists`, and dropping
its `jmap-metadata-types` dependency) and checks the `Message/query`
`unsupportedFilter` description.

## Fixed after the check

- b2 (JMAP-fhvk): the Graph, VTC and Scene memory backends have a public
  `with_account` builder (graph `(account, owner, capability)`, vtc and
  scene `(account, capability)`), and `register_account` is its documented
  `&self` form. Graph's `register_edit`, `register_search` and
  `advertise`, and VTC's and Scene's `advertise` (Scene's renamed from the
  hidden `advertise_capability`), are public too. The multi-server probe
  now creates a Graph with provenance (the owner's `myRights.mayCurate`
  is `true`) and reads VTC calls instead of stopping at `accountNotFound`.
- b3 (JMAP-m59l): `ObjectHistoryAccountCapability::new(max_history_duration,
  supported_types)`, taking both properties §2.1 says the value "MUST
  contain", as `SieveAccountCapability::new` takes its required ones. No
  builder setters: no capability struct in the workspace has them. The
  umbrella's doc example, the types crate README and the testjig use it.
- a1 (JMAP-saks): `jmap_server::DelegateJmapAccounts`. A wrapper implements
  it instead of `JmapAccounts`: `type Target`, `fn jmap_accounts`, and only
  the methods it changes; each method defaults to the target's, and a
  blanket impl provides `JmapAccounts`. `principal_id` and
  `accept_language` have no `self`, so their defaults call
  `<Target as JmapAccounts>::principal_id(caller)` (the caller context is
  the target's). The chat-server conformance wrapper, jmap-chat-slack's
  `SlackBridge` and the chat-db probe use it.
- a4 (JMAP-14yr): `jmap_server::DataTypeMetadataInfo` re-exports
  `jmap_metadata_types::DataTypeMetadataInfo`, the return type of
  `JmapAccounts::metadata_info`.
- b4 (JMAP-14yr): `Dispatcher::methods()`, the registered method names in
  sorted order. The multi-server probe bridges with it.
- c1 (JMAP-14yr): the chat `Message/query` and `/queryChanges` filter
  refusals carry a description naming the rule (`chatId` or
  `hasMention: true` required, `text` unsupported, `threadRootId` needs
  `supportsThreads`). Other handlers and memory backends that return a bare
  `unsupportedFilter` (graph, filenode, calendars, the history stores) are
  left as they are; they were not part of the finding.

## Recommendations (JMAP-14yr), no API change made

- a2, per-trait delegation boilerplate: keep decision 39 (hook traits
  delegate whole). The `Delegate*` hook-trait docs should show the pattern
  the chat-db probe uses: implement the one trait by hand, override the
  method that changes, and forward the others to the target. A
  `DelegateJmapAccounts`-style trait with per-method defaults for each of
  the ten chat hook traits would remove the forwarding, but doubles every
  method name in scope (method-call ambiguity) and is a larger design
  change than the finding warrants.
- b7, `register_filenode_conditional` and `register_blob_conditional`
  return `Result<(), RegistrationError>` while the other ten return `()`:
  at the next breaking release make all twelve return
  `Result<(), RegistrationError>` (registration can fail for any family
  once a method is registered twice), changing the canonical
  `register_mail_conditional` first. Not done now: it breaks ten crates'
  signatures for consistency alone.
- c2, `TaskQueryParams` has public fields and `Default` but is not
  `#[non_exhaustive]`, while `jmap_base_client::SetOptions` is: Public API
  rule 5 already exempts builder-style `*Input`/`*Patch` structs built
  with struct literals. Extend that exemption to client `*Params` structs
  built with `..Default::default()` and say so in AGENTS.md, rather than
  making them `#[non_exhaustive]` (which forbids the struct-literal form
  outside the crate and would need setters on every one).

## Left for beads

b1, b2 (high); b3, b5, a1 (medium); a2, a4, b4, b7, c1, c2 (low). Each is
an API-design change: a new public constructor or builder, a new trait,
a dispatcher method, or a wire-visible change in a reference backend.
Still open after the fixes above: b1 and b5.
