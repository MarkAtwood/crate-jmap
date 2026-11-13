# EmailPush (draft-ietf-jmap-emailpush-05): implementation design

Bead: JMAP-r44n (children r44n.1-.5). Spec: jmapio/jmap f8f44fd
`spec/emaildelivery/draft-ietf-jmap-emailpush.xml` (editor's -05, 2026-11-04);
posted -04 at `~/PROJECT/IETF/specs/draft-ietf-jmap-emailpush-04.txt`.

Status: final. Mark's decisions of 2026-11-13 are recorded in `docs/design/DECISIONS.md`.
This note applies decisions 3 (placement), 9 (config normalization) and 10 (`objectHistory`
in `properties`), and follows decisions 1, 2 and 8 where conditional-02 meets
PushSubscription. Every place a decision changed the draft is marked "(decision N)".
Companion notes: `docs/design/conditional.md`, `docs/design/object-history.md`,
`docs/design/blobext.md`.

## 1. Summary

EmailPush adds a mutable `emailPush` map (accountId to EmailPushConfig) to the RFC 8620
PushSubscription. When a message is delivered, the server pushes an `EmailPush` object to
the subscription URL only. The object holds the Emails that match the filter, with the
selected properties in priority order, split across pushes to fit the size limit.
Placement follows the chat-push precedent (`validate_chat_push`, `strip_unsupported_chat_push`,
`fit_chat_message_push` in `jmap-chat-server`) and the mdn/sieve feature precedent (decision 3):

- an `emailpush` feature in `jmap-mail-types` holds the wire types;
- an `emailpush` feature in `jmap-mail-server` holds validation, filter evaluation,
  projection and splitting;
- `jmap-server` push.rs gets a generic extension seam: a `using` gate and a required
  validation hook;
- `jmap-base-client` and `jmap-mail-client` get client helpers.

No new crates (decision 3). Stored configs are normalized with their defaults filled
(decision 9), and `objectHistory` is accepted in `properties` but dropped from the stored
list (decision 10).

## 2. Spec coverage

Section numbers are those of -05. "Consumer" means the deployment that owns transport and delivery.

| Requirement (-05) | Where | Notes |
|---|---|---|
| §1, §1.1, §1.2 | N/A | informative |
| §2.1 Session capability `urn:ietf:params:jmap:emailpush`, value "an empty object" | mail-types `JMAP_EMAILPUSH_URI`, `EmailPushCapability` | the consumer's Session builder advertises it, as for every capability |
| §2.1 supporting accounts "MUST have this capability in the account's "accountCapabilities""; "The value is an empty object." | mail-types `EmailPushAccountCapability`; rustdoc MUST on `EmailPushBackend::email_push_accounts`: return exactly the advertised accounts | the kit validates against the hook, so the two must agree |
| §2.1 may exist without `urn:ietf:params:jmap:mail` | `EmailPushAccount { mail: false, .. }`; `EmailPushBackend: PushSubscriptionBackend`, not `MailBackend` | IMAP-only deployments implement only the push traits (decision 3) |
| §3 `@type` "MUST be the string "EmailPush"" | mail-types `EmailPush` hand-written serde (`StateChange` pattern) | always emitted; other values rejected; absence accepted on decode |
| §3 `accountId`, `emails: Email[]`, `state: String\|null` | mail-types `EmailPush` | `state` always serialized; the kit forces `null` when `!account.mail` |
| §3 "delivered" = the conditions that change `EmailDelivery` (RFC 8621 §1.5), not JMAP creation or IMAP APPEND | consumer calls `email_pushes_for` only from its delivery path; memory `deliver_email` | `Email/import`, `Email/set` create (and object-history `restore`) and `Email/copy` never produce EmailPush (test) |
| §3 server "may include multiple Email objects" | `EmailPushBatch.emails` | batching window is the consumer's |
| §3 "MUST NOT exceed the size limit of the push service"; "MUST split them across multiple EmailPush objects" | `email_pushes_for` greedy packing, measured with `serde_json::to_vec`, the serializer `jmap_web_push::Sender::send` uses | `max_bytes` is the caller's; Web Push without padding is `jmap_web_push::MAX_PLAINTEXT` = 3993 (r44n.2) |
| §3 add properties "in this order"; on overflow "omit that property and all subsequent properties"; "MUST NOT omit properties ... unless that Email object, alone in an EmailPush, would exceed the size limit"; result is a prefix | an Email is truncated only when it is too big alone. It goes out alone, cut to the longest prefix that fits, and the cut stops at the first property that fails (r44n.2) | |
| §3 clients "MUST NOT rely on EmailPush objects as a complete record" | mail-client rustdoc | client-side |
| §3 RECOMMENDED: clients request only needed properties, most important first | mail-client rustdoc | client-side |
| §3 `state` = Email state "made after all of the Emails in the "emails" array were created" | `EmailPushBatch.state` = account Email state after the whole batch, used for every split part | a state read after the batch is also "after all" the Emails of each part |
| §4 `emailPush: Id[EmailPushConfig]\|null` (default: null) | `PushSubscription.extra["emailPush"]`; mail-types accessors; jmap-server shows `null` when the gated capability is in `using` | §5.1 |
| §4 mutable; null stops pushes | not in `PushSubscription::IMMUTABLE`; `apply_patch_object` handles `emailPush/A1/urgency`, and a null patch value removes the key | existing machinery |
| §4 "To get or set the "emailPush" property, the client MUST include" the capability in `using` | client: mail-client adds it. Server: jmap-server `PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES` gate | §5.1 |
| §4 sent "regardless of the value of the "types" property" | `email_pushes_for` ignores `types` | `types: []` test |
| §4 "MUST NOT send any EmailPush objects until the PushSubscription has been verified" | `email_pushes_for` calls `jmap_server::is_deliverable` itself (also covers `expires`) | cannot be forgotten |
| §4 lost access or capability: "MUST stop sending" and "MUST remove that account from the "emailPush" map" | stop: `email_pushes_for` takes the live `EmailPushAccount`. Remove: `prune_email_push`, called from `list_push_subscriptions` (strip on read, the `strip_unsupported_chat_push` precedent) and from the consumer's revocation path (persist) | r44n.3 |
| §4 only to the URL, not EventSource (RFC 8620 §7.3) or WebSocket (RFC 8887) | the kit has no transport; no SSE/WS code path builds EmailPush; testjig unchanged | rustdoc MUST |
| §4 `filter`: FilterOperator\|FilterCondition\|null; null pushes all | `EmailPushConfig.filter: Option<EmailFilter>` | -05 states no default for `filter`; an absent `filter` is stored as `null` (decision 9) and raised upstream (§12) |
| §4 config defaults; RFC 8620 §5.3 echo of server-changed values | `check_email_push_change` stores every config with `filter`, `properties` and `urgency` filled; `/get` returns that; `created`/`updated` echo it (decision 9) | §5.1 item 3, §5.2 step 5 |
| §4 evaluated "as it is immediately after delivery", incl. Sieve-assigned mailboxes and keywords; later changes push nothing | input contract of `DeliveredEmail` (post-Sieve Email) | Sieve runs in the consumer |
| §4 "MUST support FilterOperator objects with any of the "AND", "OR", and "NOT" operators" | `filter::email_matches` | r44n.4 |
| §4 with mail: "all the FilterCondition properties it supports for the "Email/query" method"; else at least hasKeyword, notKeyword, from, to, cc, bcc, subject, header, minSize, maxSize | the evaluator covers every RFC 8621 §4.4.1 condition and the metadata-05 conditions, a superset of anything a backend's Email/query can decode (`EmailFilterCondition` is `deny_unknown_fields`). For a non-mail account, the 10-condition minimum plus `EmailPushAccount.filter_conditions` | r44n.4 |
| §4 without mail, inMailbox/inMailboxOtherThan OPTIONAL; with RFC 8474 ObjectID "MUST support using mailbox ids discovered via IMAP" | `EmailPushAccount.object_ids` enables both | |
| §4 `properties` String[] (default: []); Email/get `properties` "except that the "bodyValues" property is not supported"; empty gives `{}`; "the "id" property is not included unless it is listed" | `project::project_email` (shared Email/get projection, default `bodyProperties`, no implicit id) | r44n.1 |
| §4 with mail: "all the properties it supports for the "Email/get" method"; else keywords, from, to, cc, subject, preview, receivedAt, sentAt; with ObjectID also id, threadId, mailboxIds | `project::check_email_property` + `EmailPushAccount` flags and `properties` extras | `header:*` forms accepted for mail accounts as Email/get does; for non-mail only if declared. `objectHistory` (object-history-03 §3.1) is accepted where this deployment's Email/get accepts it and dropped from the stored list, so it is never pushed and pushes stay a true prefix (decision 10) |
| §4 `urgency` String (default: "normal"), "MUST be an `urgency-option`" (RFC 8030 §5.3) | `jmap_types::Urgency` (closed set, rejects others); `OutboundEmailPush.urgency` feeds `jmap_web_push::PushOptions.urgency` | |
| §4 invalidProperties with "emailPush" when (1) "The change includes "emailPush"" and a key is not an emailpush account; (2) unsupported filter condition; (3) unsupported property "including "bodyValues""; (4) urgency not an urgency-option; (5) emailPush non-null and keys null, "the "properties" list also includes "keys"" | `emailpush::check_email_push_change` | (1) only when the change touches emailPush (r44n.3) |
| §4 "MUST include the path to each invalid value": `emailPush/{accountId}`, `.../filter`, `.../properties`, `.../urgency` | same; collects every bad path. A non-object config value is the invalid value at `emailPush/{accountId}` | r44n.5 |
| §5 Example | tests (§10) | the request's `p256dh` is RFC 8792-wrapped; unwrap in the fixture |
| §6 client "MUST specify encryption keys" and "ignore any push notification received that is not encrypted with those keys" | server: validate rule (5), and `email_pushes_for` also refuses a keyless subscription. Client: rustdoc MUST on `parse_push_payload`, which takes decrypted bytes | RFC 8291 decrypt is out of scope |
| §6 "MAY mitigate ... by padding" (RFC 8188) | N/A | `jmap-web-push` has no padding API; optional, no wire impact |
| §7 IANA | `JMAP_EMAILPUSH_URI` const | |

## 3. Crate placement

Extend existing crates; add no new crate (decision 3: an `emailpush` feature of
jmap-mail-types, jmap-mail-server and jmap-mail-client, after the mdn/sieve precedent;
`EmailPushBackend` does not extend `MailBackend`).

- `jmap-mail-types`: new feature `emailpush`. Private `mod emailpush` with the items
  re-exported at the crate root (the `mdn`/`sieve` layout in `lib.rs`). It uses `EmailFilter`
  (`query.rs`) and `jmap_types::{Urgency, PushSubscription}`. No new deps, no async.
- `jmap-mail-server`: feature `emailpush = ["jmap-mail-types/emailpush", "dep:jmap-metadata-server"]`
  and `pub mod emailpush` (the `mdn` precedent), with flat public items and private
  `validate`/`deliver` submodules. Two shared crate-private modules:
  - `filter.rs`, the Email filter evaluator, built under
    `cfg(any(feature = "memory", feature = "emailpush"))`;
  - `project.rs`, the Email/get property check and projection, moved out of `email.rs`.

  `jmap-metadata-server` stays optional (it pulls in `icu_casemap`/`icu_normalizer`). It
  is enabled by `memory` (as today) or `emailpush`, so plain mail-server users gain no deps.
  `EmailCondition` is the only public item from `filter.rs`; it is re-exported through
  `emailpush` (Public API rule 6, one path).
- `jmap-server`: generic PushSubscription extension seam: the `using` gate, the required
  validation hook, `using` plumbing, and a public filter-depth check. It stays mail-free,
  with no dependency on mail-types. The conditional-02 opt-in registration for
  `PushSubscription/set` (decision 1) is the conditional epic's, in the same file; §9 says
  how the two fit.
- `jmap-base-client`: generic `PushSubscription/get` and `/set` methods on `JmapClient`.
  Base-client has no method bindings today; `jmap-chat-client` has private chat-flavoured ones.
  Conditional or atomic PushSubscription/set goes through the conditional epic's generic
  `SetCall`, not through these bindings (decision 8).
- `jmap-mail-client`: feature `emailpush` with typed `emailPush` helpers and a push-payload decoder.
- `jmap-web-push`: no code change. `Sender::send<T: Serialize>` already takes `&EmailPush`.
  Update its docs. No `*-server` crate depends on it (support-crate rule).
- `jmap-types`: no change. `Urgency` (closed, hand-written serde) and `PushSubscription.extra` fit.

Dependency edges: no new crate-to-crate edge, and no cycle. metadata-server depends only on
metadata-types. Publish order is unchanged.

Why not `jmap-emailpush-{types,server,client}`: the extension is about 600 lines and entirely
about Email. It needs mail-types (`Email`, `EmailFilter`) and the mail-server Email/get
projection. A separate crate would force those internals public or duplicate them. IMAP-only
servers still work, because `EmailPushBackend` does not extend `MailBackend` (decision 3).

## 4. Wire types (`jmap-mail-types`, feature `emailpush`, root re-exports)

```rust
/// draft-ietf-jmap-emailpush-05 §2.1.
pub const JMAP_EMAILPUSH_URI: &str = "urn:ietf:params:jmap:emailpush";
/// The PushSubscription property §4 adds.
pub const EMAIL_PUSH_PROPERTY: &str = "emailPush";

/// §2.1 Session value: "an empty object".
#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EmailPushCapability {
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}
/// §2.1 accountCapabilities value: "The value is an empty object."
#[non_exhaustive] /* same shape */ pub struct EmailPushAccountCapability { pub extra: Map<String, Value> }

/// §4 EmailPushConfig. All three fields always serialize, `"filter": null` included
/// (defaults filled, decision 9).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailPushConfig {
    /// "filter": FilterOperator|FilterCondition|null. None = every delivery.
    #[serde(default)]
    pub filter: Option<EmailFilter>,
    /// "properties": String[] (default: []). Priority order. Open vocabulary
    /// (`header:*`, metadata), so String, as in Email/get.
    #[serde(default)]
    pub properties: Vec<String>,
    /// "urgency": String (default: "normal"); RFC 8030 §5.3 closed set.
    #[serde(default = "default_urgency")]
    pub urgency: Urgency,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// §3 EmailPush. Hand-written serde: "@type": "EmailPush" always emitted,
/// other values rejected, absence accepted (mirrors jmap_types::StateChange).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct EmailPush {
    /// "accountId"
    pub account_id: Id,
    /// "emails": Email[]. Each is a partial Email holding a prefix of the
    /// requested properties, so a JSON map, not `Email`.
    pub emails: Vec<Map<String, Value>>,
    /// "state": String|null. Always serialized.
    pub state: Option<State>,
    pub extra: Map<String, Value>,
}

/// Read and write `emailPush` in a PushSubscription's `extra`.
pub fn email_push(sub: &PushSubscription)
    -> Result<Option<BTreeMap<Id, EmailPushConfig>>, serde_json::Error>;
pub fn set_email_push(sub: &mut PushSubscription, v: Option<BTreeMap<Id, EmailPushConfig>>);
```

`EmailPushConfig` keeps an `extra` map under the extras-preservation policy, so unknown keys
in a config are kept. -05 lists invalidProperties only for `filter`, `properties` and
`urgency`. `EmailFilterCondition` is `deny_unknown_fields` and is excluded from the
extras policy (`query.rs`), so an unknown condition fails to decode and is reported as
`emailPush/{id}/filter`.

Normalized form (decision 9): the kit stores, returns from `/get` and echoes exactly
`serde_json::to_value(&EmailPushConfig)` of the decoded config. That is `filter` (`null` when
absent; -05 states no default, raised upstream), `properties` (`[]` when absent, in the order
sent, with `objectHistory` removed per decision 10), `urgency` (`"normal"` when absent) and any
`extra` keys. The serde defaults above are the single source of these values, so the
mail-types round-trip test pins them.

## 5. Server design

### 5.1 `jmap-server` push.rs: generic extension seam (r44n.3, r44n.5 prerequisites)

1. The `register_push_subscription_handlers` closures pass `cx.using` to both handlers.
2. New associated const next to the existing `PUSH_SUBSCRIPTION_EXTENSION_PROPERTIES` (which
   only widens the `/get` `properties` whitelist):
   `const PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES: &'static [(&'static str, &'static str)] = &[];`
   Each entry is a (property, capability URI) pair. A gated property is also a valid `/get`
   property name, so it need not appear in the old const as well. For a gated property P
   whose capability C is not in `using`:
   - `/get`: P is stripped from every object. P in `properties` gives `invalidArguments`
     (RFC 8620 §5.1: "If an invalid property is requested, the call MUST be rejected with
     an "invalidArguments" error.").
   - `/set`: a create that sends P, or a patch whose first path segment is P, gets
     `invalidProperties` naming P. This check runs before the other validation.

   When C is in `using`, an absent P is shown as `null` in `/get` and in the `created` echo
   (§4 "(default: null)", RFC 8620 §5.3). `chatPush` stays ungated, because
   draft-atwood-jmap-chat-push-00 has no `using` rule.
3. New hook, **required** (the hook rule settles this; see §6):
   ```rust
   fn check_push_subscription_change(&self, caller: &Self::CallerCtx,
       change: PushSubscriptionChange<'_>)
       -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

   #[non_exhaustive]
   pub struct PushSubscriptionChange<'a> {
       pub old: Option<&'a PushSubscription>,   // None on create
       pub new: &'a mut PushSubscription,       // patched/created; hook may normalize extra
       pub changed: &'a BTreeSet<String>,       // top-level names the create sent / the patch touched (first path segment)
       pub using: &'a [String],
   }
   ```
   The handler calls the hook after the RFC 8620 checks (url, verificationCode, immutables,
   expires) and before `create_`/`update_push_subscription`. The hook has no side effects,
   so conditional-02 `atomic` can run it in a validate-all phase.

   The hook call lives inside `handle_push_subscription_set`, not in a registration
   closure. Decision 1 leaves `register_push_subscription_handlers` unchanged and gives
   conditional-02 a separate opt-in registration that re-registers `PushSubscription/set`
   through `new_set`, last-wins, as object-history's `register_<x>_handlers_with_history`
   does. That opt-in handler must call the same `handle_push_subscription_set`, so the
   hook runs on both paths, inside the `begin_set` scope on the conditional one. The hook
   stays a method of `PushSubscriptionBackend` and is not given its own opt-in trait and
   registration. A second last-wins `/set` registration would let a deployment that
   registers the conditional path after the extension path silently lose EmailPush
   validation (decision 1 interaction; §9, §12 Q1).

   `created` and `updated` now echo every `extra` key whose value the hook changed. For
   example, `emailPush` normalized with its defaults filled (decision 9), or with
   `objectHistory` dropped from a `properties` list (decision 10), must be echoed: RFC 8620
   §5.3 `updated` holds "any property that changed in a way *not* explicitly requested".

   The hook check and the store are not one atomic step. An account revoked in between is
   handled by pruning (§5.2), not by a re-check.
4. `parse::nests_deeper_than` becomes `pub fn filter_nests_deeper_than(&Value, usize)`, and
   `MAX_FILTER_DEPTH` becomes pub. The dispatcher's depth check covers only a top-level
   `filter` argument, while a filter under `emailPush/{id}/filter` sits inside the
   (serde_json 128-level) request. It also runs again at every delivery, so it needs the
   same 32-level cap.

### 5.2 `jmap-mail-server` `emailpush` validation

`pub async fn check_email_push_change<B: EmailPushBackend>(backend, caller, change) -> Result<(), BackendSetError<B::Error>>`
is called from the backend's `check_push_subscription_change`. A backend that also supports
chatPush calls `validate_chat_push` there too. If `change.changed` lacks `"emailPush"`, only
rule (5) runs. That rule cannot fire on an update, because `keys` is immutable. Otherwise:

1. Read `extra["emailPush"]`. Absent or `null` passes. A non-object value adds the path `emailPush`.
2. Rule (5): if the value is non-null and `new.keys` is `None`, add `keys`.
3. Load `accounts = backend.email_push_accounts(caller)` once.
4. For each entry (key `A`):
   - If `A` is not in `accounts`, add `emailPush/A` (rule 1). A non-object value also adds
     `emailPush/A`.
   - `filter` must decode as `Option<EmailFilter>`, stay within `MAX_FILTER_DEPTH`, and use
     only conditions in `support(A)`. For a mail account that is every `EmailCondition`; for
     a non-mail account it is the 10 required conditions, plus `inMailbox*` when
     `object_ids`, plus `filter_conditions`. A metadata condition also needs
     `urn:ietf:params:jmap:metadata` in `using` and `metadata_info::<Email>(caller, A)`.
     Otherwise add `emailPush/A/filter`.
   - `properties` must decode as `Vec<String>`, and every name must pass
     `project::check_email_property(name, ctx(A))`. `bodyValues` always fails. Without
     mail, the allowed set is the 8-name minimum, plus `id`/`threadId`/`mailboxIds` when
     `object_ids`, plus the declared `properties` extras. `objectHistory` passes when
     `mail && email_history` (this deployment's Email/get accepts it, object-history-03
     §3.1); otherwise it is an unsupported name like any other (decision 10). Otherwise add
     `emailPush/A/properties`.
   - `urgency` must decode as `Urgency`. Otherwise add `emailPush/A/urgency`.
5. If any path was collected, return one SetError
   `invalidProperties { properties: ["emailPush", ...each path in map order] }`, with `keys`
   included for rule 5. Otherwise rewrite `extra["emailPush"]` as the normalized
   `BTreeMap<Id, EmailPushConfig>` (§4 normalized form, decision 9). Normalization removes
   every `objectHistory` entry from `properties` and keeps the other names in their order
   (decision 10). The stored list is then exactly what delivery projects, so each pushed
   Email is a true prefix of the stored list (§3 prefix rule), and the client sees the
   dropped name in the `created`/`updated` echo.

A change that touches `emailPush` (including a deep patch such as `emailPush/A/urgency`)
normalizes the whole map. A config that reached storage another way, for example written by
the consumer directly, stays as it is until the next `emailPush` write. Delivery skips
`objectHistory` anyway (§5.3).

`pub fn prune_email_push(sub: &mut PushSubscription, accounts: &BTreeMap<Id, EmailPushAccount>) -> bool`
removes keys that are not in `accounts`. When every key goes it keeps `{}`, because §4 says
"remove that account from the "emailPush" map", not set it to null. It returns whether
anything changed.

- `list_push_subscriptions` applies it to what it returns. This is strip on read, with no
  write in a read path, like chat's `strip_unsupported_chat_push`.
- The pruned map is persisted by the next update, because the `/set` handler patches the
  listed, already pruned object.
- The consumer's revocation path persists it at once.

### 5.3 `jmap-mail-server` `emailpush` delivery (r44n.1, r44n.2, r44n.4)

```rust
#[non_exhaustive] pub struct DeliveredEmail<'a> {
    pub email: &'a Email,  // post-delivery, post-Sieve; preview set; text bodyValues filled when text/body conditions may run
    pub thread_keywords: Option<&'a [&'a BTreeMap<Keyword, bool>]>, // every Email in its Thread; required for mail accounts
}
#[non_exhaustive] pub struct EmailPushBatch<'a> {          // params struct
    pub account_id: &'a Id,
    pub account: &'a EmailPushAccount,   // live, from email_push_accounts for the subscription's owner
    pub emails: &'a [DeliveredEmail<'a>],
    pub state: Option<&'a State>,   // Email state after the batch; emitted as null when !account.mail
    pub max_bytes: usize,           // limit on serde_json::to_vec(&EmailPush).len()
}
#[non_exhaustive] pub struct OutboundEmailPush { pub push: EmailPush, pub urgency: Urgency }
#[non_exhaustive] pub enum EmailPushError { LimitTooSmall, MissingThreadKeywords }

pub fn email_pushes_for(stored: &StoredPushSubscription, now_secs: i64, batch: &EmailPushBatch<'_>)
    -> Result<Vec<OutboundEmailPush>, EmailPushError>;
```

`email_pushes_for` returns `[]` in four cases: `is_deliverable` is false, `keys` is `None`,
`emailPush` has no entry for `account_id`, or no Email matches. Otherwise it runs these steps:

1. Filter with `filter::email_matches`. A thread condition with `thread_keywords: None`
   returns `MissingThreadKeywords`, never a silent pass.
2. Project each match with `project::project_email(email, &cfg.properties)`, the Email/get
   code path with default `bodyProperties`. Metadata is projected when listed, because
   validation already checked `using`. `id` is included only when listed. `objectHistory`
   is never projected: normalization has already removed it from stored configs, and
   `project_email` skips it for any config that reached storage another way, so the result
   is still a prefix of the stored list without it (decision 10).
3. Pack greedily in delivery order, starting a new EmailPush when appending would exceed
   `max_bytes`.
4. An Email that does not fit alone goes out alone. It is rebuilt by adding properties in
   list order until the next one would exceed `max_bytes`, and that property and all later
   ones are omitted.

`LimitTooSmall` means an EmailPush holding one `{}` does not fit. The kit returns it rather
than emitting an oversize push. Measurement needs about n+k serializations for n Emails and
k properties.

### 5.4 Filter evaluator (`jmap-mail-server` `filter.rs`, crate-private)

```rust
pub(crate) fn email_matches(input: &DeliveredEmail<'_>, filter: &EmailFilter) -> Result<bool, EmailPushError>;
pub(crate) fn conditions_used(filter: &EmailFilter) -> BTreeSet<EmailCondition>;
#[non_exhaustive] pub enum EmailCondition { InMailbox, InMailboxOtherThan, Before, After, MinSize, MaxSize,
    AllInThreadHaveKeyword, SomeInThreadHaveKeyword, NoneInThreadHaveKeyword, HasKeyword, NotKeyword,
    HasAttachment, Text, From, To, Cc, Bcc, Subject, Body, Header,
    MetadataExists, PrivateMetadataExists, MetadataTextContains, PrivateMetadataTextContains,
    MetadataTextEquals, PrivateMetadataTextEquals }   // re-exported as emailpush::EmailCondition
pub const EMAIL_PUSH_REQUIRED_CONDITIONS: [EmailCondition; 10]; // §4 list, verbatim order
```

Semantics follow RFC 8621 §4.4.1. The text conditions (`from`, `to`, `cc`, `bcc`, `subject`,
the `header` value, `text`, `body`) use a case-insensitive substring test, built on
metadata-server `unicode_casemap_prepare`. RFC 8621 §4.4.1 says text "SHOULD be matched in
a case-insensitive manner". `AND`/`OR`/`NOT` follow RFC 8620 §5.5. `Operator` and `Filter`
are `#[non_exhaustive]`, so an unknown variant is an error, not today's `_ => true`.

The memory `Email/query` switches to this evaluator. Today's private
`email_matches_condition` (`memory/mod.rs`) skips maxSize, from, to, cc, bcc, subject,
header, text, body and the three thread conditions, so they match every Email. After the
switch they are evaluated. The memory backend supplies the thread keywords.

### 5.5 Dispatcher, errors and capability

- EmailPush adds no methods and no `register_*` function, and
  `register_push_subscription_handlers` keeps its signature. The only other PushSubscription
  registration is conditional-02's opt-in path (decision 1), which the conditional epic adds
  in `jmap-server` push.rs and which reuses `handle_push_subscription_set` (§5.1 item 3).
- `atomic: true` or a non-empty `ifUnchangedBy` on PushSubscription/set without
  `urn:ietf:params:jmap:conditional` in `using` is `invalidArguments`, from the conditional
  dispatcher check, on every deployment whether or not it opted in (decision 2). EmailPush
  code does nothing for this.
- Every EmailPush validation failure is a `/set` SetError `invalidProperties`.
  - A `/get` request for `emailPush` without the capability in `using` is `invalidArguments`.
  - A patch through a missing parent (for example `emailPush/{pruned}/urgency`) is the
    existing `invalidPatch`.
  - Backend failures go through `backend_set_error_value` (redacted `serverFail`).
- Capability: the consumer's Session lists `JMAP_EMAILPUSH_URI` and includes it in the
  `known` set passed to `check_known_capabilities`. It adds `EmailPushAccountCapability`
  for exactly the accounts `email_push_accounts` returns. mail-server re-exports
  `JMAP_EMAILPUSH_URI` at its root, as it does `JMAP_MDN_URI`.

## 6. Backend hooks

Hook rule (AGENTS.md "Default bodies on spec-semantic backend methods"):

- No default where a no-op puts wrong data on the wire.
- Defaults are allowed for absent data or a request the spec lets the server ignore.
- Policy hooks fail open.

| Hook | Contract | Default? |
|---|---|---|
| `jmap_server::PushSubscriptionBackend::PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES` (const) | (property, capability) gate pairs. EmailPush backends list `(EMAIL_PUSH_PROPERTY, JMAP_EMAILPUSH_URI)` | `&[]`: nothing gated means RFC 8620 behaviour, so a default is fine |
| `PushSubscriptionBackend::check_push_subscription_change` | Validate and normalize the extension properties of one create or update. No side effects: it runs inside the conditional `begin_set` scope on the opt-in path (decision 1) and must give the same answer on both registration paths. Return a SetError to reject | **None.** A no-op would accept and echo any `emailPush`/`chatPush`, which is wrong wire data for a backend that has extensions. Backends without extensions write `async { Ok(()) }`. It stays on `PushSubscriptionBackend` rather than an opt-in trait (§5.1 item 3, §12 Q1) |
| `jmap_mail_server::emailpush::EmailPushBackend: PushSubscriptionBackend` :: `email_push_accounts(caller) -> Result<BTreeMap<Id, EmailPushAccount>, Error>` | The caller's accessible accounts whose accountCapabilities include emailpush. Each `#[non_exhaustive] EmailPushAccount` carries `mail`, `object_ids`, `email_history`, `filter_conditions` and `properties` extras. `email_history` is true exactly when the account's object-history capability lists `Email` in `supportedTypes`, so Email/get accepts `objectHistory` there (decision 10). Drives rule 1, `prune_email_push` and delivery. Not a `MailBackend` subtrait (decision 3) | **None.** An empty default would reject every config and strip every map, which is wrong `/get` output. `EmailPushAccount::new` sets `email_history: false`: a deployment without Email history then rejects `objectHistory`, which is what its Email/get supports |
| `PushSubscriptionBackend::list_push_subscriptions` (existing, required) | Rustdoc gains: EmailPush backends run `prune_email_push` on what they return (r44n.3) | n/a |
| `JmapBackend::metadata_info::<Email>` (existing) | Gates metadata properties and conditions | existing `None` default = absent data |

Consumer MUSTs (rustdoc on `EmailPushBackend`, as on `PushSubscriptionBackend`):

- Call `email_pushes_for` only for SMTP/LMTP-style deliveries, after Sieve, with the live
  `EmailPushAccount`.
- Send each result only to the subscription URL, for example with `jmap_web_push::Sender`
  and `PushOptions { urgency: Some(out.urgency), .. }`. Pass `max_bytes` no larger than
  `MAX_PLAINTEXT` minus any padding.
- When access to an account or its emailpush capability is revoked, call
  `prune_email_push` on every affected subscription and persist the result.

## 7. Memory reference backend (`jmap-mail-server` `memory` + `emailpush`)

- `MemoryBackend` (`CallerCtx = ()`, single owner, as in chat memory) implements
  `PushSubscriptionBackend` in a new `memory/push.rs`:
  - CSPRNG verification code and an `overQuota` cap;
  - PushVerification queued in an outbox `Vec`;
  - `check_push_subscription_change` calls `check_email_push_change`;
  - `list_push_subscriptions` strips with `prune_email_push`.

  It also implements `EmailPushBackend`. Every account returns
  `EmailPushAccount { mail: true, object_ids: true, email_history: false, .. }`. Once
  object-history's Email bead lands in the memory backend (object-history note H5),
  `email_history` follows its `object_history_capability` (follow-up bead, §11).
- New `test-helpers = ["memory"]` feature (AGENTS.md: `*_for_test` knobs never sit in plain
  `memory`):
  - `set_email_push_account_for_test` covers the no-mail, no-ObjectID, Email-history and
    revoked cases;
  - `set_push_max_bytes_for_test` exercises splitting.

  The self dev-dependency becomes
  `features = ["memory", "mdn", "sieve", "emailpush", "test-helpers"]`.
- `pub async fn deliver_email(&self, account_id, raw: &[u8], mailbox_ids, keywords) -> Result<(Id, Vec<(Id, OutboundEmailPush)>), _>`
  simulates SMTP delivery:
  - it reuses the import path;
  - it bumps the `Email` state and a new `EmailDelivery` state (memory has none today);
  - it runs `email_pushes_for` for each stored subscription, with
    `max_bytes = PUSH_PLAINTEXT_BYTES` (3993, documented as equal to
    `jmap_web_push::MAX_PLAINTEXT`, which it cannot import).

  It returns the outbox and does no network I/O. `Email/import` does not call it.
- The memory `Email/query` uses the shared evaluator (§5.4).
- The chat-server `MemoryBackend` moves its `validate_chat_push` calls out of create and
  update into `check_push_subscription_change`. Behaviour is unchanged.

## 8. Client bindings

- `jmap-base-client`: `JmapClient::push_subscription_get(ids, properties, extra_using)` and
  `push_subscription_set(create, update, destroy, extra_using)`, typed on
  `jmap_types::PushSubscription`. `extra_using` adds capability URIs such as
  `JMAP_EMAILPUSH_URI`. A follow-up bead moves `jmap-chat-client` onto them. They take no
  conditional arguments: a client sends conditional or atomic PushSubscription/set through
  the conditional epic's generic `SetCall` (it handles a `/set` without `accountId`), and
  the next breaking release's options parameter for typed `*_set` functions covers these
  bindings too (decision 8).
- `jmap-mail-client`, feature `emailpush = ["jmap-mail-types/emailpush"]`:
  - `EmailPushSubscriptionInput`: a create with required `keys` (§6), so a keyless create
    cannot be built.
  - `set_email_push(sub_id, Option<BTreeMap<Id, EmailPushConfig>>)` and
    `patch_email_push(sub_id, account, Option<EmailPushConfig>)`, both with the emailpush
    capability in `using`.
  - No re-export of the mail-types accessors (Public API rule 6). Callers use
    `jmap_mail_types::email_push`.
  - `parse_push_payload(&[u8]) -> PushPayload`, a `#[non_exhaustive]` enum: `StateChange`,
    `PushVerification`, `EmailPush` or `Other(Value)`. It dispatches on `@type` over
    decrypted bytes. The rustdoc states the §6 MUST to drop payloads not encrypted with the
    subscription's keys.
  - Rustdoc on the §3 "MUST NOT rely" rule and the RECOMMENDED property choice.

## 9. Interactions and breaking changes

**conditional-02 (JMAP-y1yu).** §2.1: the server "MUST honour the "ifUnchangedBy" argument
(Section 3) on every Foo/set method it implements", PushSubscription/set included.
Comparison "uses the same representation the server would return for that property from
Foo/get". Conditional support is opt-in (decision 1), so all of this applies only on a
deployment that registers the conditional PushSubscription path:

0. Wiring (decision 1). `register_push_subscription_handlers` keeps its signature and gains
   no `ConditionalBackend` bound; the conditional draft's "gains the bound" is superseded.
   The conditional epic adds an opt-in registration in `jmap-server` push.rs, bounded on
   `PushSubscriptionBackend + ConditionalBackend`, which calls
   `register_push_subscription_handlers` and then re-registers `PushSubscription/set` with
   `new_set` (no `accountId`: `SetScopeRequest.account_id = None`). That handler wraps the
   same `handle_push_subscription_set`, so the required `check_push_subscription_change`
   hook runs on both paths. That is why the hook stays on the base trait (§5.1 item 3). A
   deployment that does not opt in still gets the decision 2 rejection of conditional
   arguments sent without the URI (§5.5).
1. An `ifUnchangedBy` pointer under `emailPush` without emailpush in `using` is not a valid
   pointer for that request, so it is rejected under rule 4 (`invalidArguments`): the
   conditional evaluator's internal `PushSubscription/get` gets the same `using` and rejects
   the gated name (§5.1 item 2).
   Pointers to `url` or `keys` are a different case. §9.1 says a precondition "on a property
   the requesting client is not permitted to read ... MUST cause the method to fail with a
   "forbidden" error", and `/get` forbids those properties.
2. The comparison runs on the pruned, normalized `emailPush` (decision 9). Normalization
   makes `emailPush/A/urgency` and `emailPush/A/filter` comparable even when the client
   omitted them. A pointer to `emailPush/A/properties` compares against the stored list,
   which never contains `objectHistory` (decision 10).
3. Under `atomic`, `check_push_subscription_change` is the validate phase, so it stays free
   of side effects. PushSubscription/set has no `ifInState` (RFC 8620 §7.2.2), so §3.1
   rule 5 does not apply. The PushVerification that create sends is a side effect for
   conditional to defer; it is not specific to EmailPush.

Ordering: land the §5.1 seam (§11 E1) before the conditional epic's PushSubscription bead,
which restructures the same handler.

**object-history-03 (JMAP-tna1.3).**

- PushSubscription has "Supports History" No (§7.2), so history arguments do not apply.
- Object history §3.1: "The client MAY include "objectHistory" in the properties argument"
  of Email/get, which returns it only with `includeReplaced`/`includeDestroyed`. EmailPush
  has neither flag. Decision 10: `check_email_push_change` accepts `objectHistory` where
  Email/get does (`EmailPushAccount.email_history`) and normalization drops it from the
  stored `properties`, so it is never pushed and every pushed Email is a true prefix of the
  stored list. Where Email history is not offered, `objectHistory` is an unsupported name
  (`emailPush/A/properties`). This resolves the object-history note's Q6 for EmailPush. The
  upstream list (§12) suggests that emailpush exclude `objectHistory` as it excludes
  `bodyValues`.
- Email is in object-history's first round (decision 5), so the memory backend's
  `email_history` flag can follow its capability once object-history's Email bead lands
  (follow-up bead, §11).
- A `restore` on Email/set is a JMAP method call, not a delivery, so it produces no EmailPush.
- `onlyDestroyed`/`destroyedAfter`/`destroyedBefore` are query arguments, not
  FilterConditions, so the "all Email/query FilterConditions" MUST does not reach them.

**blobext-03 (JMAP-tna1.2).** No interaction.

- EmailPush defines no data type and no blob type.
- blobext adds no Email/get property.
- `blobId` is an ordinary Email property.

**Existing crates.**

- `chatPush` and `emailPush` coexist in `extra`.
- EmailPush does not replace the Email/EmailDelivery StateChange, so subscribers with
  matching `types` get both.
- The scene and vtc push helpers are unaffected.

**Breaking changes.**

| Crate | Change | Kind |
|---|---|---|
| jmap-server | `PushSubscriptionBackend` gains required `check_push_subscription_change` | **BREAKING**. In-tree implementors: chat memory and 2 test mocks. Out of tree: every consumer implementing the trait. This is the only breaking change on the PushSubscription surface: with decision 1 the conditional epic leaves `register_push_subscription_handlers` and `PushSubscriptionBackend` untouched (§12 Q1) |
| jmap-server | new const (defaulted), `PushSubscriptionChange`, pub `filter_nests_deeper_than`/`MAX_FILTER_DEPTH` | additive |
| jmap-server | `created`/`updated` echo hook-normalized extension keys (decision 9); gated properties stripped/rejected without capability | wire change for backends that gate or normalize |
| jmap-mail-server | memory `Email/query` now evaluates maxSize, from, to, cc, bcc, subject, header, text, body and the thread conditions | wire behaviour change (memory only; fixes a silent pass) |
| jmap-mail-server | `emailpush` and `test-helpers` features; `memory` adds `PushSubscriptionBackend` | additive |
| jmap-mail-types, jmap-mail-client, jmap-base-client | new feature/items/bindings | additive |

Each changed crate needs a public-api snapshot refresh (`scripts/public-api.sh`) and a
CHANGELOG entry.

## 10. Test plan

All oracles are hand-written JSON from the spec, never output of the code under test.

- **mail-types**
  - The §5 Session capabilities figure deserializes, and `"urn:ietf:params:jmap:emailpush": {}` round-trips.
  - The §5 EmailPush figure round-trips equal as a JSON value, and `"state": null` is emitted.
  - A wrong `@type` is rejected without echoing the value.
  - A missing `@type` is accepted.
  - Vendor extras on `EmailPush` and `EmailPushConfig` are preserved.
  - `{}` decodes to `filter: null, properties: [], urgency: "normal"` and serializes back
    to exactly `{"filter":null,"properties":[],"urgency":"normal"}` (decision 9).
  - `"urgent"` fails to decode.
- **jmap-server** (`tests/push_subscription_{get,set}.rs`, with a mock that gates `emailPush`)
  - Without the capability: `/get` strips `emailPush`, `properties:["emailPush"]` gives
    `invalidArguments`, and `/set` create and a `emailPush/A` patch give
    `invalidProperties ["emailPush"]`.
  - With it, an absent `emailPush` is `null` in `/get` and in `created`.
  - The hook sees `changed` exactly (create keys; patch first segments).
  - The hook's SetError reaches `notCreated`/`notUpdated`.
  - A normalized extra key is echoed in `created` and `updated`, and an unchanged one is not
    (decision 9).
  - The hook is called from `handle_push_subscription_set`, so a test that registers `/set`
    through any other path that calls that function still sees it. The conditional epic's
    PushSubscription bead adds the opt-in-path test (decision 1).
- **mail-server validate** (one test per bullet and path; r44n.5)
  - The §5 request is accepted for an account with `mail: false, object_ids: true`.
  - An unknown account gives `["emailPush","emailPush/X"]`.
  - `"A": null` gives `emailPush/A`.
  - A `{"foo":1}` filter gives `.../filter`.
  - `inMailbox` with neither mail nor ObjectID gives `.../filter`.
  - `bodyValues`, and `id` without mail and without ObjectID, give `.../properties`.
  - `"urgent"` gives `.../urgency`.
  - `keys` null gives `["emailPush","keys"]`.
  - Two bad accounts give both paths.
  - A filter nested 33 deep gives `.../filter`.
  - `header:Subject:asText` is accepted for a mail account and rejected for an undeclared
    non-mail account.
  - A metadata condition without metadata in `using` gives `.../filter`.
  - Normalization (decision 9): a create with `{"A": {}}` stores, returns from `/get` and
    echoes in `created` `{"A": {"filter": null, "properties": [], "urgency": "normal"}}`; a
    patch `emailPush/A: {"urgency": "high"}` echoes the whole normalized `emailPush` in
    `updated`; a patch that already sends the normalized form echoes nothing.
  - `objectHistory` (decision 10): with `mail: true, email_history: true`,
    `["from","objectHistory","subject"]` is accepted, stored as `["from","subject"]` and
    echoed that way; with `email_history: false` or `mail: false` it gives `.../properties`.
- **r44n.3** (memory): after revoking account R,
  - `/get` lacks R;
  - an update of `types` only succeeds and the stored map loses R;
  - a patch `emailPush/R` = config gives `["emailPush","emailPush/R"]`;
  - a patch `emailPush/R/urgency` gives `invalidPatch` (parent pruned);
  - deliveries to R push nothing.
- **mail-server filter** (r44n.4)
  - Each of the 10 required conditions gets a positive and a negative case.
  - AND, OR and NOT, including NOT over several conditions (RFC 8620 §5.5: none match).
  - The §5 OR filter matches by mailbox and by `$notify`.
  - A thread condition without `thread_keywords` gives an error, never `true`.
  - Memory `Email/query` regression tests for `from`, `subject`, `maxSize` (previously ignored).
- **mail-server deliver**
  - The §5 delivery yields exactly the spec's EmailPush figure (id `M699bb151ca4cbf1c24364a68`).
  - r44n.1: `properties: []` gives `[{}]`, and `["from"]` gives no `id`.
  - A config holding `objectHistory` (stored directly, bypassing validation) projects the
    other names in order and never emits `objectHistory` (decision 10).
  - r44n.2 splitting:
    - N small Emails over `max_bytes` give at least 2 pushes, every Email complete, each
      push at most `max_bytes` by `serde_json::to_vec`;
    - a lone oversized Email goes alone as a strict prefix;
    - a large property 2 followed by a tiny property 3 drops both (prefix, no skipping);
    - a small Email next to an oversized one is not truncated;
    - a tiny limit returns `LimitTooSmall`.
  - An unverified, expired or keyless subscription gets nothing, and `types: []` still gets EmailPush.
  - `state` is `null` when `mail: false`, even if the caller passes one.
  - `urgency` is carried to `OutboundEmailPush`.
  - Memory: `deliver_email` pushes, while `Email/import` and `Email/set` create do not.
- **mail-client**
  - `parse_push_payload` on the §5 EmailPush figure and on the RFC 8620 §7.1 and §7.2.2 examples.
  - The create input cannot omit keys (required field).

## 11. Bead breakdown

Children of JMAP-r44n. The existing children are reused: r44n.4 is the filter evaluator, r44n.5
the validation, r44n.3 the pruning, r44n.1 the delivery core and r44n.2 the splitting. Keys
E1-E17 are local to this note; another note refers to one as `emailpush:E1`. Beads in the same
wave can run in parallel worktrees.

| Key | Bead | Title | Crates (files) | Size | Deps | Wave |
|---|---|---|---|---|---|---|
| E1 | new | jmap-server: PushSubscription extension seam (using plumbing, capability gate, required `check_push_subscription_change` called from `handle_push_subscription_set`, normalized echo, pub filter depth); chat memory moves `validate_chat_push` into the hook; mocks | jmap-server (`push.rs`, `parse.rs`, `lib.rs`, tests), jmap-chat-server (`memory.rs` push impl) | M | - | 1 |
| E2 | new | mail-types: `emailpush` feature (URI, capabilities, `EmailPushConfig` with normalized serialization (decision 9), `EmailPush` serde, accessors) | jmap-mail-types (`emailpush.rs`, `lib.rs`, `Cargo.toml`) | S | - | 1 |
| E3 | r44n.4 | mail-server: crate-private Email filter evaluator, `EmailCondition`, required-condition list; memory Email/query uses it with thread keywords | jmap-mail-server (`filter.rs`, `memory/mod.rs` filter fns, `lib.rs` mod line) | M | - | 1 |
| E4 | new | mail-server: move the Email/get property check and projection into crate-private `project.rs`; no behaviour change | jmap-mail-server (`project.rs`, `email.rs`, `helpers.rs`, `lib.rs` mod line) | S | - | 1 |
| E11 | new | base-client: generic PushSubscription/get and /set on `JmapClient` | jmap-base-client (`push.rs`, `client.rs`) | S | - | 1 |
| E5 | new | mail-server: `emailpush` feature skeleton: `pub mod emailpush`, `EmailPushAccount` (incl. `email_history`, decision 10), `EmailPushBackend` (not a `MailBackend` subtrait, decision 3), `EmailPushError`, URI re-export, metadata-server optional-dep wiring | jmap-mail-server (`emailpush/mod.rs`, `lib.rs`, `Cargo.toml`) | S | E2, E3 | 2 |
| E12 | new | mail-client `emailpush` feature: typed helpers, `parse_push_payload` | jmap-mail-client (`methods/push.rs`, `lib.rs`, `Cargo.toml`) | S | E2, E11 | 2 |
| E6 | r44n.5 | mail-server: `check_email_push_change` with every invalidProperties path, normalization (decision 9), `objectHistory` accept-and-drop (decision 10) | jmap-mail-server (`emailpush/validate.rs`, `project.rs` check ctx) | M | E1, E4, E5 | 3 |
| E8 | r44n.1 | mail-server: `email_pushes_for` core: deliverability gates, filter, projection without implicit `id`, `state`, `urgency` | jmap-mail-server (`emailpush/deliver.rs`) | M | E3, E4, E5 | 3 |
| E7 | r44n.3 | mail-server: `prune_email_push`, `list_push_subscriptions` rustdoc, account check only when `emailPush` changed | jmap-mail-server (`emailpush/validate.rs`) | S | E5, E6 | 4 |
| E9 | r44n.2 | mail-server: split across pushes, then prefix truncation, `LimitTooSmall` | jmap-mail-server (`emailpush/deliver.rs`) | M | E8 | 4 |
| E10 | new | mail-server memory: PushSubscriptionBackend + EmailPushBackend, `deliver_email`, EmailDelivery state, `test-helpers` feature, §5 end-to-end and r44n.3 end-to-end tests | jmap-mail-server (`memory/push.rs` new, `memory/mod.rs` impl blocks, `Cargo.toml`) | L | E6, E7, E9 | 5 |
| E13 | new | Docs: AGENTS.md crate map rows, mail crates' README/PLAN.md, web-push docs, CHANGELOGs | AGENTS.md, READMEs, PLAN.md, CHANGELOGs | S | E10, E12 | 6 |
| E14 | new (follow-up) | chat-client onto base-client PushSubscription bindings | jmap-chat-client | S | E11 | later |
| E15 | new (follow-up bug) | Email/get rejects unknown property names with `invalidArguments` (RFC 8620 §5.1 MUST; today `filter_properties` drops them silently) | jmap-mail-server (`email.rs`/`project.rs`) | S | E4 | later |
| E16 | new (follow-up) | memory `EmailPushAccount.email_history` follows the memory object-history capability; `objectHistory` end-to-end (decision 10) | jmap-mail-server (`memory/push.rs`) | S | E10, object-history:H5 | later |
| E17 | new (follow-up chore) | Draft the emailpush upstream issues (§12 upstream list) | none (jmapio/jmap issue text) | S | - | later |

Each bead refreshes its own crate's public-api snapshot. File overlaps within a wave:

- Wave 1: E3 and E4 each add one `mod` line to mail-server `lib.rs`, a trivial rebase.
- Wave 3: E6 and E8 each add re-exports to `emailpush/mod.rs`. E6 also edits `project.rs`
  (the check context) while E8 calls `project_email` from it; land E6's `project.rs` change
  first or rebase.
- Wave 4: E7 follows E6 in `validate.rs`; E9 follows E8 in `deliver.rs`.

Cross-note ordering: E1 lands before the conditional epic's PushSubscription bead
(`docs/design/conditional.md` §11), which adds the opt-in registration of decision 1 over the
same `handle_push_subscription_set`. Object-history's cross-extension test bead (H12) needs
E10.

## 12. Open questions for Mark

Decided and removed: placement (decision 3), normalization of stored configs (decision 9),
`objectHistory` in `properties` (decision 10). All in `docs/design/DECISIONS.md`.

Remaining:

1. **Required push hook as a breaking change.** Decision 1 made conditional-02 opt-in so that
   `register_*_handlers` and the backend traits stay non-breaking. This note keeps
   `check_push_subscription_change` as a required method of `PushSubscriptionBackend`, a
   **BREAKING** change (in tree: chat memory and 2 test mocks). The decision 1 shape (a separate
   trait plus an opt-in `register_push_subscription_handlers_with_extensions`) is possible but
   composes badly with decision 1 itself: both opt-in paths would re-register
   `PushSubscription/set` last-wins, so registering the conditional path after the extension
   path would silently drop EmailPush and chatPush validation, unless a third combined
   registration exists. *Recommend keeping the required hook* (one break, one `/set` body). Confirm.
2. **`objectHistory` gating.** This note reads decision 10's "matching Email/get" as: accept
   `objectHistory` only where this deployment's Email/get accepts it (`mail && email_history`,
   i.e. the account's object-history capability lists `Email`), and reject it elsewhere as an
   unsupported property (§4 rule 3). The alternative is to accept it on every account. Confirm
   the gated reading.
3. **Delivery input type** (not covered by a decision): `DeliveredEmail` wraps
   `jmap_mail_types::Email`, whose `id`/`blobId`/`threadId`/`mailboxIds` are required, but a
   deployment with neither mail nor ObjectID has no JMAP ids for them. *Recommend keeping
   `&Email`*: validation rejects those properties and `inMailbox*` for such accounts, so
   whatever internal ids the backend fills in are never emitted or consulted. A partial-map
   input would lose typed filter evaluation.

### Upstream list (emailpush; from `docs/design/DECISIONS.md`)

- `filter` has no stated default; this kit treats an absent `filter` as `null` (decision 9).
- Suggest that §4 exclude `objectHistory` from `properties` as it excludes `bodyValues`, since
  EmailPush has no `includeReplaced`/`includeDestroyed` (decision 10).
