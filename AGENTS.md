# Agent Instructions

This is a **Cargo workspace** for the `jmap-*` Rust crate family (RFC 8620, RFC 8621, and
the JMAP Chat extension). All crates live in `crate-jmap-*/` subdirectories.

Read the crate's `PLAN.md` before touching its code.

## What this workspace builds

A **library kit** for building JMAP servers and clients in Rust. Not a JMAP server you
run. Not a turnkey product. Building blocks for someone else (kith, stoa, third-party
consumers) to assemble into a server.

Concretely the workspace ships:

- **Wire-format type crates** (`jmap-types`, `jmap-mail-types`, `jmap-chat-types`,
  etc.) — sync, runtime-agnostic, model the specs.
- **Handler-library server crates** (`jmap-server` foundation + 8 extension servers) —
  method handlers + backend traits + reference `MemoryBackend` impls. **No HTTP, no
  SSE, no WebSocket, no auth integration, no persistence, no main.rs.** All transport
  / multi-tenancy / auth / storage is the consumer's responsibility.
- **Client crates** (`jmap-base-client` + 6 extension clients) — method bindings,
  session fetch, auth (as types, not as an integration), SSE/WS push connections.
- **MIME adapter** (`jmap-mime`) for the mail family.

What the workspace explicitly does NOT ship:

- A binary you run to get a working JMAP server (with one exception, the test jig
  below).
- HTTP / SSE / WS transport helpers on the `*-server` crates (`serve_axum()`, etc.).
- Production-grade `MemoryBackend` implementations. The reference impls are
  in-memory only, intentionally feature-gated, and explicitly demonstration-quality.
- Auth integrations (OAuth flow handlers, JWT issuance, mTLS, etc.).
- Persistence backends. Anything that touches disk or a database.
- Configuration / CLI / env-var machinery.

The downstream **consumers** of this kit (Mark's own products like kith and stoa, plus
the reference impl at `~/PROJECT/crate-jmapchat-server/`, plus future third-party users)
bring all of the above. The kit is the published foundation; the consumers are the
products.

### The one exception: `jmap-testjig`

A single deliberately-minimal workspace member, `crate-jmap-testjig/` (epic
bd:JMAP-cf7p), exists as a `publish = false` binary crate that wires the kit's pieces
(dispatcher + 8 extension handlers + reference MemoryBackends) into a running
HTTP/SSE/WS process for the workspace's **own** integration testing and demonstration.
It is the only workspace member with `axum` / `tokio-tungstenite` deps. It is loudly
documented as NOT FOR PRODUCTION: in-memory only, single-user, hardcoded bearer auth,
no persistence. Its purpose is to support workspace integration tests and contributor
smoke-testing — not to be a JMAP server anyone deploys.

**Do not propose growing transport / persistence / auth / multi-tenancy into the
`*-server` crates "for symmetry" or "for completeness".** The transport-less posture
is intentional. The consumer-brings-everything posture is intentional. If a future
pressure makes the kit-posture feel wrong, surface it as an explicit
workspace-architectural decision bead — do not drift toward it via incremental scope
creep on individual feature beads.

## Crate Map

| Directory | Crate | Role |
|---|---|---|
| `crate-jmap-types/` | `jmap-types` | Shared wire types — foundation, no async |
| `crate-jmap-jscalendar-types/` | `jmap-jscalendar-types` | RFC 8984 JSCalendar typed sub-types — shared foundation, consumed by Calendars and Tasks. No async, no JMAP dep |
| `crate-jmap-jscontact-types/` | `jmap-jscontact-types` | RFC 9553 JSContact typed sub-types — shared foundation, consumed by Contacts. No async, no JMAP dep |
| `crate-jmap-mail-types/` | `jmap-mail-types` | RFC 8621 data types, no async |
| `crate-jmap-chat-types/` | `jmap-chat-types` | JMAP Chat extension types, no async |
| `crate-jmap-server/` | `jmap-server` | Dispatcher + parse + HTTP helpers |
| `crate-jmap-base-client/` | `jmap-base-client` | RFC 8620 base client: auth, session, blob, SSE, WebSocket |
| `crate-jmap-mime/` | `jmap-mime` | MIME adapter: mime-tree → jmap-mail-types (greenfield) |
| `crate-jmap-mail-server/` | `jmap-mail-server` | RFC 8621 method handlers (greenfield) |
| `crate-jmap-mail-client/` | `jmap-mail-client` | RFC 8621 client methods (greenfield) |
| `crate-jmap-chat-server/` | `jmap-chat-server` | JMAP Chat method handlers (greenfield) |
| `crate-jmap-chat-client/` | `jmap-chat-client` | JMAP Chat client methods (greenfield) |
| `crate-jmap-metadata-types/` | `jmap-metadata-types` | draft-ietf-jmap-metadata data types — `Metadata`, `Annotation`, `ImapMetadata`, `WebDavMetadata`, `MetadataFilterCondition`, `MetadataCapability`. No async |
| `crate-jmap-metadata-server/` | `jmap-metadata-server` | draft-ietf-jmap-metadata method handlers — `Metadata/get/changes/set/query/queryChanges` + `MetadataBackend` trait |
| `crate-jmap-metadata-client/` | `jmap-metadata-client` | draft-ietf-jmap-metadata client methods — `Metadata/get/changes/set/query/queryChanges` |
| `crate-jmap-cid-types/` | `jmap-cid-types` | draft-atwood-jmap-cid-00 data types — `CidCapability` (`urn:ietf:params:jmap:cid`) and `Sha256` typed wire shape. Blob/FileNode integrity, no async |

## Dependency Tree

```
jmap-types      — shared wire types: Id, JmapRequest/Response, ResultReference, JmapError. No async.
    ├── jmap-server         — dispatcher, parse_request, ResultReference resolution, HTTP helpers.
    ├── jmap-base-client    — RFC 8620 base client: auth, session fetch, blob, SSE, WebSocket.
    │       ├── jmap-chat-client   — JMAP Chat method implementations.
    │       └── jmap-mail-client   — RFC 8621 method implementations.
    ├── jmap-mail-types     — RFC 8621 data types: Email, Mailbox, Thread, etc. No async.
    │       ├── jmap-mime        — MIME parser adapter: mime-tree → jmap-mail-types. No async.
    │       ├── jmap-mail-server   — RFC 8621 method handlers, MailBackend trait.
    │       └── (jmap-mail-client also depends on this)
    └── jmap-chat-types     — JMAP Chat extension types: Chat, Message, Space, etc. No async.
            ├── jmap-chat-server   — Chat method handlers, ChatBackend trait.
            └── (jmap-chat-client also depends on this)

jmap-jscalendar-types  — RFC 8984 JSCalendar typed sub-types: LocalDateTime, Duration,
                         RecurrenceRule, Location, Participant, Alert, etc. No JMAP dep, no async.
    ├── jmap-calendars-types   — consumes + re-exports as `jscalendar` module alias.
    └── jmap-tasks-types       — (planned, JMAP-yfpq) will consume the same shared sub-types.

jmap-jscontact-types   — RFC 9553 JSContact typed sub-types: Name, EmailAddress, Phone,
                         Address, Organization, Anniversary, etc. No JMAP dep, no async.
    └── jmap-contacts-types    — consumes + re-exports as `jscontact` module alias.

jmap-metadata-types    — draft-ietf-jmap-metadata data types: Metadata, Annotation,
                         ImapMetadata, WebDavMetadata, MetadataFilterCondition,
                         MetadataCapability. No async. Depends on jmap-types only.
    ├── jmap-metadata-server   — Metadata/* method handlers, MetadataBackend trait.
    └── jmap-metadata-client   — Metadata/* client method bindings.

jmap-cid-types         — draft-atwood-jmap-cid-00 data types: CidCapability
                         (urn:ietf:params:jmap:cid) plus the Sha256 typed wire
                         shape (lowercase hex, 64 chars). No async. Depends on
                         jmap-types only. Independent of any single consumer
                         extension — feeds Blob upload responses and FileNode
                         objects in a future jmap-base-client / jmap-filenode-types
                         revision.
```

Type crates (`*-types`) have no async deps. Server crates may depend on tokio/http.

## Canonical Templates (cookie-cutter consistency)

The 30 `jmap-*` crates are deliberately cookie-cutter siblings: every type
crate looks like every other type crate, every server crate looks like
every other server crate, every client crate looks like every other client
crate, **modulo only the differences mandated by the relevant RFC or
draft**. Identical idioms, identical helper names, identical doc-comment
style, identical test layout. The differences should be the specific
JMAP capability the crate covers, nothing else.

To enforce that, certain crates are anointed as **canonical templates** for
their family. When you change a non-canonical sibling and the change
diverges from the canonical template, the rule is: **change the canonical
first, then propagate**. When you change the canonical, the rule is:
**propagate the change to every sibling in the same pass** (or file a
follow-up sweep bead before merging).

| Family | Canonical | Siblings (must mirror) |
|---|---|---|
| Foundation types | `jmap-types` | (none — sole foundation) |
| Extension types | `jmap-mail-types` | `jmap-chat-types`, `jmap-calendars-types`, `jmap-tasks-types`, `jmap-contacts-types`, `jmap-filenode-types`, `jmap-sharing-types`, `jmap-metadata-types` |
| Foundation server | `jmap-server` | (none — sole foundation) |
| Extension server | `jmap-mail-server` | `jmap-chat-server`, `jmap-calendars-server`, `jmap-tasks-server`, `jmap-contacts-server`, `jmap-filenode-server`, `jmap-metadata-server`, `jmap-sharing-server` |
| Foundation client | `jmap-base-client` | (none — sole foundation) |
| Extension client | `jmap-mail-client` | `jmap-chat-client`, `jmap-calendars-client`, `jmap-tasks-client`, `jmap-contacts-client`, `jmap-filenode-client`, `jmap-metadata-client`, `jmap-sharing-client` |

`jmap-chat-types` is *also* a canonical reference for the JMAP Chat draft
specifically (its wire format is normative for that extension), even
though the broader extension-types family takes its idiom shape from
`jmap-mail-types`.

Each canonical crate's `AGENTS.md` carries a short canonical-template
banner reminding contributors of the propagation rule. **The previous
"LOCKED — explicit permission required" framing was misleading**: those
banners were never about API stability lockdown; they were about
divergence prevention. The new wording makes the consistency intent
explicit.

## Build & Test

```bash
# Check all crates
cargo check --workspace

# Run all tests
cargo test --workspace

# Check a single crate
cargo check -p jmap-types
cargo test -p jmap-server

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
```

**Pre-commit gate — run all of these before any commit:**
```bash
cargo fmt --all
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

## Source Material

### Specs (normative)

All live at `~/PROJECT/jmap-chat-spec/references/`:

| File | Covers |
|---|---|
| `rfc8620.txt` | JMAP base protocol — wire types, ResultReference, Session, error codes |
| `rfc8621.txt` | JMAP for Mail — Email, Mailbox, Thread, Identity, EmailSubmission |
| `draft-atwood-jmap-chat-*.md` | JMAP Chat extension (in `~/PROJECT/jmap-chat-spec/`) |
| `draft-ietf-jmap-*.txt` | Other IETF JMAP extensions (calendars, contacts, blob, etc.) |

When implementing anything, read the relevant RFC section first. Do not guess at wire field names.

### Reference Implementations (local — read, do not modify)

| Path | What to look for |
|---|---|
| `~/PROJECT/crate-jmapchat-server/jmapchat-server/` | Handler/backend pattern, `StorageBackend` trait, `RefStore`, dispatch tests |
| `~/PROJECT/crate-jmapchat-server/jmapchat-types/` | Type idioms: `Clearable<T>`, `#[non_exhaustive]`, serde rename conventions |
| `~/PROJECT/crate-jmapchat-client/` | Client-side type usage |
| `~/PROJECT/kith/crates/kith-core/` | Original `JmapError`, `JmapRequest/Response`, `ResultReference` source |
| `~/PROJECT/kith/crates/kith-jmap/` | Original dispatcher, `parse_request`, ResultReference resolution |
| `~/PROJECT/stoa/crates/mail/` | JMAP mail consumer — `dispatch.rs`, session/capability structs |

The `PLAN.md` in each crate identifies exactly which files and line numbers to draw from.

### Broader Ecosystem

For Rust crates not in `~/PROJECT`, check `~/GIT` and `~/WORK` before reaching for the network.

## Conventions & Patterns

- **Path deps**: each crate references siblings via `path = "../crate-jmap-*"` — do not
  change to version deps until publishing.
- **Test oracles**: tests must use independent fixtures (hand-written JSON from RFC examples,
  or OpenSSL/pyca output). Never derive expected values from the code under test.
- **No async in type crates**: `jmap-types`, `jmap-mail-types`, `jmap-chat-types` must not
  depend on tokio or any async runtime.
- **`crate-jmapchat-*` dirs** (outside this workspace): reference/inspiration only — not
  members of this workspace and not to be modified here.
- **Crate naming**: crate name = `jmap-*`, directory name = `crate-jmap-*`.
- **`#[forbid(unsafe_code)]`** at every crate root.
- **No `.unwrap()` or `.expect()`** in library code — propagate errors with `?`.
- **Wire format**: camelCase JSON — `#[serde(rename_all = "camelCase")]` on all structs.
- **Licensing**: the workspace `Cargo.toml` declares
  `license = "MIT OR Apache-2.0"` at the workspace level, and every crate
  inherits via `license.workspace = true`. **Do NOT add `LICENSE-MIT` or
  `LICENSE-APACHE` files** to any crate or to the repo root. The TOML
  metadata is sufficient for crates.io and `cargo deny`. Do not "fix"
  this convention — it is intentional.
- **Sloppy-Value pattern for IETF-defined nested objects**: type crates use
  `Option<serde_json::Value>` for fields whose value shape is defined by an
  external IETF spec (JSCalendar / RFC 8984, JSContact / RFC 9553, etc.)
  and is large or extensible. Each affected crate's `PLAN.md` documents
  the per-field rationale (e.g.
  `crate-jmap-calendars-types/PLAN.md` §1–§8,
  `crate-jmap-contacts-types/PLAN.md` §10). Do not "type out" these
  sloppy fields without explicit user approval — doing so creates large
  public types that drift as the upstream specs evolve. The preferred
  hybrid is the calendars approach: keep the public field as
  `serde_json::Value` for round-trip fidelity, and add parallel typed
  sub-types in a sibling module (e.g. `jscalendar.rs`) that consumers
  can opt into via `serde_json::from_value`.
- **Extras-preservation policy for vendor/site fields**: every public
  `Deserialize` struct that appears on the JMAP wire carries a catch-all
  `extra` field, and every wire-format **result** string enum carries an
  `Other(String)` variant. The combination preserves vendor / site /
  private-extension fields and unrecognised result values losslessly
  across deserialize / serialize round-trips. RFC 8620 §1.6 mandates
  silent-ignore of unknown fields; the spec floor permits data loss.
  Workspace policy is **preservation** because implementors and sites
  add custom data to JMAP types without waiting for IETF process.

  Relationship to the **Sloppy-Value** pattern above: Sloppy-Value
  applies to IETF-spec'd nested *objects* whose value shape is owned by
  an external spec; the extras pattern applies to vendor/site-private
  *fields* that are not declared by any spec and just need to round-trip.
  Both patterns coexist on the same struct.

  Field shape (decided 2026-05-09):

  ```rust
  #[non_exhaustive]
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct Email {
      pub id: Id,
      // ... typed fields ...

      /// Catch-all for vendor / site / private extension fields not
      /// covered by the typed fields above. Preserves unknown fields
      /// across deserialize/serialize round-trip per workspace policy.
      #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
      pub extra: serde_json::Map<String, serde_json::Value>,
  }
  ```

  Wire format is byte-identical when extras are empty
  (`skip_serializing_if`). Field name is lowercase `extra` (singular) and
  visibility is `pub`, matching the existing `MethodResponseError.extra`
  precedent in `jmap-types`.

  Result-enum forward-compat shape:

  ```rust
  #[non_exhaustive]
  #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub enum MailboxRole {
      Inbox,
      Sent,
      // ... known RFC 8621 §2.1.1 variants ...

      /// Forward-compat catch-all for vendor / site / future-spec roles.
      /// The wire string is preserved for round-trip.
      #[serde(other)]
      Other(String),
  }
  ```

  Note: `#[serde(other)]` only captures the *variant tag*; the matched
  wire string is the original. The variant identifier `Other` is a
  Rust-side naming choice that the wire format does not see — the
  attribute `#[serde(other)]` is what makes it the catch-all. All
  in-scope result enums remain `#[non_exhaustive]` so a future
  spec-defined variant addition is still a non-breaking change.

  **In scope** (apply both mechanisms):
  - Data object types and their nested wire sub-types (`Email`,
    `EmailAddress`, `EmailHeader`, `Calendar`, `CalendarEvent`,
    `ContactCard`, `Task`, `TaskList`, `Chat`, `Message`, `Space`,
    `FileNode`, `Principal`, etc.).
  - Standard response wrappers (`GetResponse<T>`, `SetResponse<T>`,
    `ChangesResponse`, `QueryResponse`, `QueryChangesResponse`).
  - Method-argument structs in `*-client` crates (e.g.
    `EmailSubmissionSetParams`, `MailboxSetParams`).
  - Wire-format **result** string enums (`MailboxRole`, `DeliveryState`,
    `NodeType`, `EmojiKind`, `ChatKind`, `ParticipantRole`, etc.).

  **Out of scope** (do NOT add extras / `Other` to these):
  - Filter and comparator algebra types — see the dedicated sub-section
    below.
  - Control string enums (`Operator`, `ComparatorProperty`) — see same.
  - Internal Rust API types: backend trait result types
    (`SetDefaultResult`, `ParseResult`, `AvailabilityError`,
    `BackendSetError`, `BackendChangesError`).
  - `MockBackend` state structs, `FaultyBackend`, `NoSnippetBackend`,
    `TrackingBackend`, anything under `tests/common/`.
  - Internal parsing helpers, dispatcher types, MIME tree types.
  - `jmap-types::PatchObject` — already a `Map<String, Value>`;
    redundant.
  - Property selector enums (server-side; not on the wire).
  - Newtypes wrapping a single value (`Id`, `UTCDate`, `Date`, `State`,
    `Keyword`).

  **Test discipline**: every in-scope type carries at least one
  round-trip preservation test. For structs, the test asserts an unknown
  field survives serialize. For result enums, the test asserts an
  unknown wire string deserialises into `Other(s)` and round-trips
  back to the same wire string. Tests use independent oracles per
  workspace test-integrity rules — never the code under test.

  **New types**: any new public `Deserialize` struct or result string
  enum added to an in-scope crate MUST include the appropriate
  mechanism from day one. The propagation epic is `JMAP-lbdy`; per-crate
  children `.1`–`.9` carry the canonical-template sweep.

  **Filter algebra and control enums are explicitly EXCLUDED**
  (decided 2026-05-10). Filter and comparator algebra types (`Filter<T>`,
  `FilterOperator<T>`, per-crate `FilterCondition` types,
  `EmailComparator`, `CalendarEventComparator`, etc.) and control string
  enums (`Operator`, `ComparatorProperty`) MUST NOT receive `extra`
  fields or `Other(String)` variants. Three reasons:

  1. **Silent-drop is a server-side query-correctness bug.** Unlike
     data-object extras, which round-trip mechanically through the
     client, an unknown filter clause like `{"acmeCorpPriority": "high"}`
     means nothing unless the server understands and indexes it. A
     query that silently drops a clause returns the wrong result set
     with no compile-time or runtime signal. Extras on filter
     conditions would let clients compile filters the server cannot
     honor.

  2. **`Filter<T>` is `#[serde(untagged)]` over a fixed variant set.**
     Adding `extra` to the fields of `EmailFilterCondition` would not
     let a vendor add a new variant shape (e.g. a new operator-shaped
     node). New variant shapes would have to come from extending the
     `Operator` enum, which is itself a control enum and falls under
     reason 3.

  3. **Control enums must dispatch on known variants.** `Operator`
     (`AND`/`OR`/`NOT`) and `ComparatorProperty` are not display values
     — backends implement matching logic per variant. `Other(String)`
     is meaningless here: a backend cannot honor `XAND` or
     `someUnknownSortKey`. The `#[non_exhaustive]` derives already
     give spec-level forward-compat for future RFC-defined operators;
     vendor-level forward-compat for control enums is incoherent.

  Vendors who need filterable extras have two paths:

  - **IETF-track** — use `draft-ietf-jmap-metadata` (currently
    draft-01, capability URI `urn:ietf:params:jmap:metadata`). It
    defines a companion `Annotation` object keyed by
    `(relatedType, relatedId)`, with schema discovery via the
    capability's `dataTypes` / `metadataTypes` / `maxDepth` properties
    and a `Metadata/query` filter (currently `textMatch` over vendor
    string properties — coarse but standardised). Workspace
    implementation tracker: `JMAP-06zp`.

  - **Pre-IETF escape** — vendors who need typed filter construction
    against custom server fields RIGHT NOW (before the metadata draft
    stabilises) can fork the per-crate `FilterCondition` type or use
    `serde_json::Value` for the filter tree. The hybrid sloppy-value
    pattern documented in `crate-jmap-calendars-types/PLAN.md` is the
    model.

  Neither path uses the workspace extras pattern. The exclusion holds.

  Per-crate rustdoc on each in-scope filter / comparator / control-enum
  type carries the same exclusion + dual-future-hook notice. The
  propagation epic that drove that rustdoc sweep is `JMAP-9wh7`
  (closed).

  **Externally-owned-schema classifier strings are also EXCLUDED**
  (decided 2026-05-12). Classifier-attribute strings on data-object
  sub-types in type crates that mirror an externally-owned schema
  (RFC 9553 JSContact, RFC 8984 JSCalendar) MUST NOT receive the
  typed-enum-with-`Other(String)` catch-all pattern.

  Examples in scope of this exclusion: `NameComponent.kind`,
  `Title.kind`, `Calendar.kind`, `Anniversary.kind`,
  `PersonalInfo.kind` in `jmap-jscontact-types`; `Participant.kind`,
  `RecurrenceRule.frequency`, `Alert.action`, `Location.relativeTo`
  in `jmap-jscalendar-types`.

  Rationale: RFC 9553 and RFC 8984 enumerate classifier values as if
  authoritative, but operational reality is that real-world
  implementers (Outlook, Google, Apple, vCard exporters, localized
  clients, third-party CalDAV servers, etc.) routinely send values
  outside the spec enumeration. Typing these into
  `enum { Variant1, ..., Other(String) }` creates a fiction where
  the catch-all does most of the real work and the named variants do
  effectively no programmatic dispatch. The maintenance cost
  (variant additions on every spec revision, `#[non_exhaustive]`
  surface, propagation through consumers) is paid for no programmatic
  benefit.

  Leave these fields as `Option<String>` or `String`. Consumers that
  care about specific values match string literals.

  This exclusion is narrow. It does NOT apply to:
  - JMAP-native result enums (`MailboxRole`, `DeliveryState`,
    `NodeType`, `EmojiKind`, `ChatKind`, `ParticipantRole`, etc.) —
    those stay in scope of the typed-enum-with-`Other(String)`
    mechanism. Consumers DO dispatch reliably on them.
  - Field shapes that are whole nested objects (whose value shape is
    owned by an external spec) — those follow the Sloppy-Value
    pattern documented earlier in this section.
  - Wire-protocol control values defined by RFC 8620 itself
    (`Operator`, `ComparatorProperty`) — those are governed by the
    Filter-algebra exclusion above.

  Cross-references:
  - `JMAP-sc1b.77` — the original jscalendar-types conversion that
    this exclusion makes wrong in retrospect (revert tracked at
    `JMAP-sc1b.111`).
  - `JMAP-sc1b.108` — the parallel jscontact-types propagation that
    triggered this policy discussion.
- **Set-as-map idiom on the wire stays as `HashMap<String, bool>`**
  (decided 2026-05-17, bd:JMAP-sgrr.18). Several IETF JMAP-family
  specs use the set-as-map idiom — a JSON object whose keys carry
  the meaning and whose values are always the literal `true` — for
  fields that semantically represent a set of strings. Examples:
  RFC 9553 §1.5.1 `contexts` on most JSContact sub-objects;
  RFC 9553 `Phone.features`; RFC 9553 `Relation.relation`; the
  analogous shapes in RFC 8984 JSCalendar; the keyword / member /
  addressBookIds maps on JMAP Contacts; etc.

  Workspace type crates model these fields as
  `Option<HashMap<String, bool>>` (or `HashMap<String, bool>` when
  mandatory), NOT as `HashSet<String>` with a serde adapter. The
  spec wire form permits the construction. Callers building a fresh
  value MUST set every value to `true` (per RFC 9553 §1.5.1 et al.);
  deserialize does not reject inputs that include `false` values,
  but downstream code that iterates the map should match on the key
  alone and treat the value as a redundant assertion.

  Why not `HashSet<String>` with a `{key: true}` serde adapter:

  1. **Cross-crate breaking change is large.** The idiom appears on
     16+ fields in `jmap-jscontact-types` alone, plus an unknown
     count across `jmap-jscalendar-types`, `jmap-contacts-types`,
     `jmap-calendars-types`, `jmap-mail-types`, `jmap-chat-types`,
     etc. A workspace-wide migration would be a coordinated
     breaking release across ~30 crates for a defensive-typing
     improvement.

  2. **The wire shape is the source of truth.** The kit's posture
     (types model the wire shape; semantic validity is the
     consumer's job) is consistent across other shape decisions
     (Sloppy-Value pattern, mandatory-as-Option for partial
     responses, externally-owned classifier-string exclusion).
     Tightening this one field shape would be the outlier.

  3. **The defensive cost of the current shape is low.** Iterating
     `HashMap<&String, &bool>` and matching on the key is no harder
     than iterating `HashSet<&String>`. Code that asserts
     `value == true` before treating the key as set-membership is a
     three-character match-arm.

  Future work: if a downstream consumer accumulates enough evidence
  that `false`-valued wire inputs cause real bugs, the right
  follow-up is a custom serde adapter at the kit level
  (`jmap-types`) that maps `HashMap<String, bool>` → `HashSet<String>`
  on deserialize-and-back. That would be a separate workspace epic
  driven by `jmap-mail-types` as the canonical extension-types
  template; do not start a piecemeal per-crate migration.

  Per-crate rustdoc / Gotchas need not call this out individually —
  every type crate that ships set-as-map fields can point to this
  AGENTS.md section.
- **Public-API construction: deserialize-from-JSON is the canonical
  path** (decided 2026-05-17, bd:JMAP-sgrr.27). Every public wire-
  format struct in the workspace is `#[non_exhaustive]` per workspace
  policy, which makes struct-literal construction (`Name { ... }`)
  a compile error from outside the defining crate. The kit
  deliberately does NOT compensate by adding `fn new(...) -> Self`
  constructors on every type, and the omission is intentional, not a
  gap.

  External consumers construct wire-format values by deserializing
  JSON: `serde_json::from_value::<Name>(json!({ "full": "..." }))`.
  This is the spec-aligned path because:

  1. **The spec is the construction oracle.** RFC examples ship as
     JSON. Tests in this workspace deserialize hand-written JSON
     fixtures from the RFC. A constructor would be the kit's
     interpretation of the spec, layered on top of the spec; the
     JSON path goes straight to the spec.

  2. **Constructors and partial-response `Option` fields don't
     compose.** Several mandatory-on-wire fields are modelled as
     `Option<...>` (e.g. `Calendar.kind`, `Calendar.uri` — see
     `crate-jmap-jscontact-types/src/lib.rs` "Design: optional
     fields and `Option<...>`"). A `Calendar::new(...)` constructor
     would either accept those fields (making the call site
     verbose) or default them to `None` (defeating the
     type-safety motive for having a constructor). The
     deserialize path side-steps this because the JSON itself
     carries the populated/omitted distinction.

  3. **`#[non_exhaustive]` is forward-compat infrastructure.** It
     exists so the kit can add fields without a breaking release.
     A constructor crystallises the field set at the time of
     authorship and either freezes the API (the kit can't add
     mandatory fields without a major release) or omits new
     fields (the constructor produces partial values silently). The
     deserialize path adapts automatically because consumers
     pin the kit version and serde handles new optional fields
     transparently.

  Per-crate `Default` impls MAY be added where the type has no
  mandatory fields and the all-`None`/empty value is meaningful
  (e.g. `Address::default()` for an empty address before
  population). They are not required by this policy.

  `#[doc(hidden)]` test-only constructors (e.g. `Email::new(...)`
  in `jmap-mail-types`) are an internal-API allowance and are NOT
  subject to this policy. They exist to support unit tests in the
  same crate; they do not surface as public construction API.

  Per-crate rustdoc need not call this out individually — every
  type crate can point to this AGENTS.md section.
- **TLS stack**: this workspace uses **rustls**, NOT native-tls / openssl.
  Both `reqwest` and `tokio-tungstenite` MUST be declared with
  `default-features = false` and only `rustls-tls-*` features enabled.
  Rationale: openssl pulls in C code and a recurring stream of CVEs
  (e.g. CVE-2026-42327, CVE-2026-44662 on rust-openssl 0.10.78). rustls is
  pure Rust on top of the RustCrypto stack, has a smaller attack surface,
  and aligns with this project's RustCrypto-first stance. Do not add
  `native-tls`, `default-tls`, or any feature that would re-introduce
  openssl as a transitive dependency. To verify, run
  `cargo tree -i openssl --workspace` — it MUST report
  "did not match any packages".

## Caller identity (foundation seam)

Locks in the workspace-wide answer to "how does a JMAP method know who
the caller is". Established by bd:JMAP-ga0q; the foundation method
landed in bd:JMAP-ga0q.1.

- **The seam**:
  `JmapBackend::principal_id(caller: &Self::CallerCtx) -> Option<&jmap_types::Id>`
  is the ONLY way the JMAP layer asks "who is the caller". No
  alternate path exists, no `caller_identity_blob()` escape hatch,
  no generic claims map. The return type is `Option<&Id>` — typed,
  borrowing the id from the caller context the HTTP/auth middleware
  populated.
- **Backends are canonical for permission enforcement.** Handlers do
  NO permission checking. Defense-in-depth handler-side pre-checks
  are allowed but the backend MUST re-verify atomically with the
  mutation. A handler that "trusts" a handler-side check and skips
  the backend re-check is a bug.
- **Identity is a foundation concept, not an extension feature.**
  Decide once in `jmap-server`; every extension inherits. Future
  extensions that need richer caller info (groups, claims, roles,
  device class) add those to their OWN backend trait, NOT to
  `JmapBackend`. Foundation provides the id; extensions own the
  meaning.
- **Federation does NOT bypass this seam.** The federation handler
  maps peer-signed identity to a local principal once, before
  invoking the JMAP method. JMAP method code sees a normal
  `principal_id()` return value. No second identity path exists.
- **`None` is deliberate, not an error.** A backend that returns
  `None` from `principal_id` is signalling "this deployment does
  not honor identity-dependent JMAP semantics". Such a backend
  CANNOT correctly implement chat role-hierarchy, calendar ACLs,
  sharing/myRights, per-user `$seen` on shared mailboxes, or
  metadata `isPrivate` visibility scoping. Test fixtures and
  single-user dev servers use the default `None`-returning impl
  and are fine; multi-user production deployments MUST override.

The first consumer is bd:JMAP-g7wu.2.4 (chat Space/set permission
enforcement). Future consumers: jmap-mail-server (RFC 8621 `$seen`
on shared mailboxes), jmap-calendars-server (calendar ACLs),
jmap-sharing-server (RFC 9670 myRights), jmap-metadata-server
(isPrivate visibility scoping).

## Backend caps and limits

Extension backend traits that expose implementation-defined caps
(per-Space content limits, per-account message size limits, etc.)
follow a workspace pattern that preserves per-account flexibility
without committing to spec-mandated values or to a client-visible
session-capability surface.

- **Shape**: `fn limits(&self, caller: &Self::CallerCtx, account_id: &Id) -> XxxLimits` —
  a sync default method on the extension's backend trait returning a
  struct of all related caps as a group. Each extension defines its
  own `XxxLimits` struct (e.g. `ChatLimits`, future `CalendarsLimits`).

- **Struct**: `#[non_exhaustive]` with a `Default` impl carrying
  conservative reference-impl-grade values. Production backends
  override the whole method, not individual fields.

- **Args are plumbed even when the default ignores them**. The default
  impl on the trait silently drops `caller` and `account_id`, but the
  signature accepts them so production backends can vary caps
  per-account (Free vs Pro tier, multi-tenant SaaS, etc.) without
  forcing a future API break.

- **No spec contract on values**. Workspace policy is that caps are
  NOT spec-mandated. New extensions SHOULD NOT add cap-advertising
  fields to their JMAP capability objects. The chat draft was revised
  on 2026-05-11 (jmap-chat-spec commit `80d5e11`, bead bd:JMAP-kt5k) to
  drop five aggregate-count cap-advertising fields from
  `urn:ietf:params:jmap:chat` (`maxGroupMembers`, `maxSpaceMembers`,
  `maxRolesPerSpace`, `maxChannelsPerSpace`, `maxCategoriesPerSpace`).
  Per-aggregate enforcement is now via standard `overQuota` SetError
  (RFC 8620 §5.3). The three per-message size/count caps that have
  RFC 8621 (`urn:ietf:params:jmap:mail`) analogs (`maxBodyBytes`,
  `maxAttachmentBytes`, `maxAttachmentsPerMessage`) were kept on the
  capability for consistency with that RFC. Workspace catch-up to the
  spec edit is tracked by bd:JMAP-v3ff.

- **Client visibility, when desired**: JMAP Quota
  (`urn:ietf:params:jmap:quotas`) is the cross-protocol mechanism for
  exposing dynamic caps + usage to clients. The workspace does not
  implement Quotas yet; when it does, backends with caps SHOULD
  surface them via Quota records rather than via per-capability
  fields.

- **Enforcement**: backend is canonical, per the "Caller identity
  (foundation seam)" rule above. Handlers MAY do defense-in-depth
  pre-checks before calling the backend, but the backend MUST
  re-verify atomically with the mutation.

- **Atomicity**: if a multi-entry add op (e.g. `addRoles` with 5
  entries) would exceed a cap, reject the entire entry, not the
  over-quota subset. Matches RFC 8620 `/set` atomicity at the target
  level.

**Precedent**: `ChatBackend::limits(&self, caller, account_id) -> ChatLimits`
(bd:JMAP-g7wu.2.4.8).

**Older pattern, NOT retroactively reshaped**: `jmap-mail-server`
predates this convention and exposes individual
`max_<thing>(caller, account_id)` methods (e.g. `max_body_value_bytes`,
`max_delayed_send_seconds`, `max_sieve_script_bytes`,
`max_collapse_threads_emails`). Mail-server's shape is not wrong; it
just predates the struct pattern. A deliberate consolidation bead is
required if/when mail-server is to migrate to the struct shape — do
not propagate the per-method shape to new extensions on the assumption
that mail-server is canonical for this specifically.

## Security testing

Two complementary tripwire patterns guard against credential-grade
secret literals appearing in places they shouldn't:

1. **Per-type `Debug` redaction canary** — a unit test that constructs
   the type with a canary literal under the test's control and asserts
   the canary does not appear in `format!("{value:?}")` output.
   Precedent: bd:JMAP-sc1b.79 (`BearerAuth`, `BasicAuth` in
   `crate-jmap-base-client`), bd:JMAP-sc1b.99 / .104 (`Session`,
   `AccountInfo`). Required when any new type wraps a credential
   string, a session token, a verification code, an invite code, or
   any other secret the workspace owns. Lives in the same module as
   the type.

2. **Log-capture canary** — an integration test that installs a
   thread-local buffering `tracing` subscriber, exercises the same
   code paths that a future contributor adding `tracing::*`
   instrumentation might write (e.g. `tracing::info!(auth = ?auth,
   ...)`, `tracing::debug!("{auth:?}")`), and asserts the captured
   buffer contains no canary literal. Tracks bd:JMAP-sc1b.102.
   Reference harness: `crate-jmap-base-client/tests/common/log_capture.rs`.
   Reference canaries: `crate-jmap-base-client/tests/log_redaction.rs`.
   Required when any new module emits `tracing::*` events with an
   argument that interpolates a secret-bearing type via `?` or `%`
   syntax — the test should pattern-match the existing reference test
   and supply a canary literal of the new type.

The two patterns are complementary, not interchangeable. Pattern 1
catches a future refactor that adds `#[derive(Debug)]` to a type that
should have a manual redacting impl. Pattern 2 catches a future
`tracing::*` call that interpolates a Display value (which Pattern 1
does not cover) or that bypasses the type's Debug impl via a custom
formatter.

When adding a new type or a new logging call site that crosses these
boundaries, file a follow-up bead if the canary test cannot land in
the same commit. Do not ship the type or the logging call without the
matching canary.

## README convention

Every crate README in this workspace has the same six-section shape. The
shape is content-defined, not just heading-defined: the section names
should be the literal text below, and the content should match the
purpose described.

```markdown
# crate-name

One-paragraph elevator pitch — what this crate contains, in plain
sentences. No badges, no logo, no buzzwords.

## What it is

Concrete description of the crate's contents. What types, traits,
functions, and modules live here. RFC / draft citations next to the
items they implement. Tables are fine when they map "thing → spec
section".

## What it's for

The use case. Who depends on this crate and why. The crate's role in
the workspace dependency graph (one or two sibling crates named, not
the full graph).

## How to use

A minimal runnable example. `Cargo.toml` snippet if non-obvious. The
shortest end-to-end "hello world" against the crate's primary API.
For server crates: the minimum to wire a backend + dispatch a method.
For type crates: parsing and serializing a representative wire value.

## How it works

A sketch of the internal model — key types, key traits, dispatch /
parse / handler flow as relevant. Not a `cargo doc` replacement; just
enough that a reader knows where to look. Include design constraints
that affect public-API choices ("sealed trait set", "`#[non_exhaustive]`
on every public struct", "no async", etc.).

## Gotchas

Known sharp edges. Feature gates with non-obvious activation rules.
MSRV quirks. Behavior that surprises a casual reader. Current
limitations the consumer needs to plan around (no automatic SSE
reconnect, no field validation on `Id`, etc.). Be specific — a vague
"there are caveats" line is worse than no section.

## References

Links/citations to the normative spec and any auxiliary specs.
Plain markdown links, no inline summaries. Include section anchors
where the crate implements a specific sub-protocol.

- [RFC 8620] — JMAP Core
- [RFC 8621] — JMAP for Mail

[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
```

**Authorship and license go in `Cargo.toml`, NEVER in the README.**
The workspace declares `authors`, `license`, and `repository` at the
`[workspace.package]` block and every crate inherits via
`authors.workspace = true` etc. A README with a `## License`, `##
Authors`, `## Authorship`, or "Licensed under …" footer is a bug —
delete the section, do not duplicate the metadata.

Section names are not optional. A crate-specific extra section (e.g.
"Extension trait pattern", "Filter extensibility") MAY appear, but it
goes **between** the six required sections in a position that makes
narrative sense — not in place of one of them. If a crate has nothing
useful to say in a section, write one honest sentence ("This crate has
no known gotchas at the current draft revision.") rather than skipping
the heading.

The convention sweep that brought all 32 crates onto this shape is
tracked under `JMAP-76wm`.

## Key Rules

- **`cargo test --workspace`** must pass before any commit.
- **No async** in `*-types` crates — no tokio, no futures.
- **`crate-jmapchat-*`** directories in `../` are reference/inspiration only.
- **Test oracles** must be independent of the code under test (RFC example JSON, OpenSSL output).
- **Do NOT add LICENSE files** — the workspace TOML `license = "MIT OR Apache-2.0"`
  declaration is the entire license-metadata story. See the Conventions list above.

## Non-Interactive Shell Commands

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` on this system.
Always use explicit non-interactive flags:

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Other commands that may prompt: `scp`/`ssh` — use `-o BatchMode=yes`; `apt-get` — use `-y`.

## Git Commit and Push Policy

Commit freely after completing logical units of work — no need to ask permission per
commit. Push freely too: just `git push`. The agent is the only thing pushing to
`origin/main`, so there is no `pull --rebase` ritual to dance through before each
push.

If a push is rejected for any reason — non-fast-forward, network, auth, anything —
**stop and ask the human**. Do NOT run `git pull --rebase`, do NOT merge, do NOT
force-push, do NOT `git reset` to "recover". Those operations are all in the same
family of history-rewriting moves that have destroyed work in this repo before, and
they must never be invoked from a script or by an agent without explicit human
authorization for that specific recovery action. Local commits are safe sitting on
the local branch; leave them there and ask.

Exceptions where you should still pause even on the happy path:
- `git push --force` to `main` — never without explicit user instruction.
- Any push that would land secrets, credentials, or `.env`-shaped files.
- Any commit that creates files the user explicitly did not ask for (the
  "don't make doc files unless asked" rule still applies regardless of push policy).

## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` for full workflow context.

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
```

Use `bd` for ALL task tracking — do NOT use TodoWrite or markdown TODO lists.
Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files.

**Beads is the only task and planning tool.** Do NOT use:
- TodoWrite / markdown TODO lists
- Scratchpad or audit files (`audit-*.md`, `plan-scratch.md`, or any similar throwaway planning file)
- MEMORY.md or any other markdown file as a knowledge store

The only permitted markdown planning artifact is a crate's `PLAN.md`, which is a permanent
design document checked into the repo — not a scratchpad. Use `bd remember` for persistent
knowledge and `bd create` for all task tracking.

### Turn questions into beads

When you hit an ambiguity you cannot resolve autonomously — an ambiguous
spec passage, two plausible API shapes, an unclear destructive-action
scope, a choice between non-equivalent refactors — **do not invoke the
interactive `question` tool**. Instead:

```bash
bd update <id> --add-label human --status open
bd comment <id> "<the question, including any options considered and your recommendation if you have one>"
```

…and move on to the next ready bead. The verbatim rule is: **"turn
questions into beads that say there is a question to be asked"**.

Rationale: `bd-spin`'s permission overlay denies the interactive
`question` tool, so invoking it from inside a `/do-beads` session
deadlocks the entire outer loop. Even in interactive sessions, a
`human`-labeled bead is a durable record the human can pick up
on their next pass, whereas an interactive question is lost when
the session ends. Stalling the ready frontier on one ambiguous
decision is the failure mode; filing-and-continuing is the
recovery.

Apply this rule recursively: any subagent you spawn must also
file-and-continue rather than ask.

### When NOT to file `human`

The `human` label is high-cost: every labeled bead consumes attention
on a future review pass and slows the autonomous frontier. Default to
**not** filing `human`. The label is appropriate only when a genuine
human-judgment call is unresolved AND the bead is not already
self-deciding by one of the four patterns below.

**1. Frontier-cycling escalation.** Do NOT auto-label a parent epic
`human` just because its only-open-child is `human`. That escalation
shifts cycling-avoidance burden onto the human-decision frontier
where it doesn't belong — the parent has no actual decision pending,
only its child does. Fix the cycling at the picker layer (skip
parents whose only-open-children are filter-blocked) rather than by
labeling them with semantics that aren't true.

**2. "You already decided this."** Do NOT label `human` on work the
human has explicitly deferred or scoped out. `deferred` is the
signal for parked backlog; `human` is the signal for unresolved
judgment. They are not interchangeable and they do not stack. If a
bead carries `deferred`, do NOT also add `human` — the
deferred-vs-human distinction is exactly the difference between
"park it" and "the human owes a decision".

**3. "Should we do the obvious thing."** Do NOT label `human` when
the bead body or its most recent comment already contains a clear
recommendation that the agent itself authored. If you write
"Recommendation: option (a). Rationale: ..." in a bead comment, the
recommendation IS the decision unless a high-stakes qualifier
applies: destructive action, public API breaking, security boundary,
or two equally-plausible options presented without a recommendation.
Filing `human` to ratify your own recommendation transfers cost
from the agent (who already did the analysis) to the human (who
now has to re-do it).

**4. "It might already be done."** Before filing `human` on a bead
that asks whether external state should change (spec edit needed,
upstream library version, third-party CVE, etc.), spend 60 seconds
checking whether the state already changed. `git log` the relevant
repo, check the upstream issue, run `cargo tree` — whatever the
cheap verification is. Most "should we still do X?" questions
evaporate when you actually look.

Recursive application: subagents spawned by `/do-beads` or any other
macro must apply the same four rules. A subagent that files `human`
to defer to its parent agent is filing the wrong label — that's an
intra-agent escalation, not a human-judgment call.

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete
until `git push` succeeds.

1. **File issues for remaining work** — create issues for anything needing follow-up
2. **Run quality gates** — `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`
3. **Update issue status** — close finished work, update in-progress items
4. **Push to remote**:
   ```bash
   git push                         # plain push only; do NOT add pull --rebase
   git status                       # MUST show "up to date with origin"
   ```
   If `git push` is rejected for any reason, **stop and ask the human**. Do not
   `pull --rebase`, do not merge, do not force-push, do not `git reset`. See the
   "Git Commit and Push Policy" section above for why.
5. **Clean up** — clear stashes, prune remote branches
6. **Hand off** — provide context for next session
