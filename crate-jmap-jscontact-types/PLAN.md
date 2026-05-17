# jmap-jscontact-types — Implementation Plan

RFC 9553 JSContact typed sub-types for the `jmap-*` crate family.
Pure types — no method handlers, no async, no network I/O.

## Crate family position

```
(no JMAP dep)
    └── jmap-jscontact-types  ← this crate
            └── jmap-contacts-types (path-dep + re-export)
```

## What this crate is

The JSContact sub-object types defined in RFC 9553. They are embedded
inside the `ContactCard` JMAP object (`jmap-contacts-types`) and have
no JMAP identity of their own.

This crate is structurally analogous to `jmap-jscalendar-types`: both
expose RFC-defined wire-format sub-types that consuming JMAP extension
type crates re-export. Unlike `jmap-jscalendar-types`, this crate has
no `jmap-types` dependency — JSContact sub-objects do not reference
JMAP primitives (such as `Id`) at the wire level; they use their own
RFC 9553 §1.4.1 `Id` type, which is just a `String`.

## What this crate is not

- Not the JMAP Contacts binding (that is `jmap-contacts-types`)
- Not a vCard parser or jCard converter (RFC 9555 is out of scope)
- Not opinionated about vCard semantics — only the wire-format JSON
  shape defined by RFC 9553

## Dependencies

```toml
serde      = { workspace = true }
serde_json = { workspace = true }
```

No other dependencies. No `jmap-types`.

## Public API

Single module (`src/lib.rs`). All public structs are `#[non_exhaustive]`
and derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`. Wire-format JSON uses
`#[serde(rename_all = "camelCase")]`. The JSContact `@type` discriminator
is mapped to an `Option<String>` field named `at_type` with
`#[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]`.

The `@type` field is `Option<String>` rather than bare `String` because
RFC 9553 §1.3.4 permits omitting `@type` whenever the type is implied by
its position in the enclosing object (i.e. the field's type signature
is unambiguous, or matches the `defaultType` attribute documented in
the property's `[Object Definition]`). The most-cited case in this
crate is `Anniversary.date` (§2.8.1), whose type signature is
`PartialDate|Timestamp (defaultType: PartialDate)`: a `PartialDate`
value MAY omit `@type` entirely, a `Timestamp` value MUST set
`@type: "Timestamp"`. The `AnniversaryDate` deserialize impl at
`src/lib.rs:977` depends on this distinction.

This shape diverges from the sibling `jmap-jscalendar-types`, which uses
bare `at_type: String`. The divergence is spec-driven and intentional:
RFC 8984 marks every JSCalendar `@type` as `(mandatory)` (with zero
`defaultType` annotations), so `jmap-jscalendar-types` correctly mirrors
that with bare `String`. The workspace canonical-templates rule says
"every type crate looks like every other type crate, **modulo only the
differences mandated by the relevant RFC or draft**" — this is exactly
that case. Resolved by `bd:JMAP-sgrr.3`.

### Object types

| Type(s) | RFC 9553 § | Notes |
|---|---|---|
| `Name`, `NameComponent` | §2.2.1 | Person/entity name with optional components |
| `Nickname` | §2.2.2 | Alternative names |
| `Organization`, `OrgUnit` | §2.2.3 | Company/org with hierarchical units |
| `SpeakToAs`, `Pronouns` | §2.2.4 | How to address the entity |
| `Title` | §2.2.5 | Job title or role |
| `EmailAddress` | §2.3.1 | Email per RFC 5322 addr-spec |
| `OnlineService` | §2.3.2 | Messaging/social-media handle |
| `Phone` | §2.3.3 | Phone number, optionally URI-formatted |
| `LanguagePref` | §2.3.4 | Preferred languages (BCP 47 tags) |
| `Calendar` | §2.4.1 | Calendaring resource (extends `Resource`) |
| `SchedulingAddress` | §2.4.2 | iTIP scheduling URI |
| `Address`, `AddressComponent` | §2.5.1 | Postal/geographic address |
| `CryptoKey` | §2.6.1 | Public key / certificate resource |
| `Directory` | §2.6.2 | Directory service resource |
| `Link` | §2.6.3 | Generic resource link |
| `Media` | §2.6.4 | Photo / sound / logo resource |
| `Anniversary`, `PartialDate`, `Timestamp` | §2.8.1 | Memorable dates |
| `Note`, `Author` | §2.8.3 | Free-text notes with optional authorship |
| `PersonalInfo` | §2.8.4 | Expertise / hobby / interest |
| `Relation` | §2.1.8 | `relatedTo` value type |

### Resource-derived types

RFC 9553 §1.4.4 defines the abstract `Resource` data type with common
fields (`@type`, `kind`, `uri`, `mediaType`, `contexts`, `pref`, `label`).
Calendar (§2.4.1), CryptoKey (§2.6.1), Directory (§2.6.2), Link (§2.6.3),
and Media (§2.6.4) extend Resource.

These are modelled in this crate as five concrete struct types, each
embedding the Resource common fields directly. There is no public
`Resource` trait or struct — the abstraction is documentation-only per
the RFC and inheritance is irrelevant on the wire.

### Types intentionally not exposed

The bead description mentions "Localization" and "Keyword" as types in
the 22-type list. These are not separate RFC 9553 object types:

- `localizations` (§2.7.1) has value type `PatchObject`, which is just
  a JSON object map of pointer-strings to arbitrary values (§1.4.3).
  There is no `Localization` object type in the RFC. The `ContactCard`
  in `jmap-contacts-types` already exposes `localizations` as
  `Option<HashMap<String, PatchObject>>` using `jmap_types::PatchObject`;
  this crate adds nothing useful.

- `keywords` (§2.8.2) has value type `Boolean`, i.e. the map is
  `String[Boolean]`. There is no `Keyword` object type. The `ContactCard`
  exposes `keywords` as `Option<serde_json::Value>` (which is a JSON
  object map of strings to `true`); a typed wrapper offers no
  additional structure.

Both are documented above for traceability. Future revisions of this
crate may add newtype wrappers if a downstream consumer requests them.

## Module layout

Single `lib.rs` for now. Following `jmap-jscalendar-types`' precedent
(570 LOC in one file), this crate is expected to be roughly the same
size: 20+ types with simple Resource-style field overlap.

If a future bead splits this into sub-modules (e.g. `name.rs`,
`address.rs`, `resource.rs`, `anniversary.rs`), it will be a follow-up
once the initial single-file layout is verified.

## Spec reference

```
~/PROJECT/jmap-chat-spec/references/rfc9553.txt   ← normative
```

## History

- bd:JMAP-sehw is the parent epic.
- bd:JMAP-sehw.1 (this bead) creates the crate scaffolding and the
  typed sub-types.
- bd:JMAP-sehw.2 (follow-up) migrates `jmap-contacts-types` to consume
  this crate via path-dep + re-export.
- bd:JMAP-sehw.3 (follow-up) updates workspace `AGENTS.md` (crate map
  + dep tree) and `crate-jmap-contacts-types/PLAN.md` to point at this
  crate's PLAN for the hybrid-design rationale.

## Round-trip test policy

Each typed sub-type has a round-trip test using hand-written RFC 9553
example JSON (one of the spec's worked examples per type). The oracle
is the RFC, not the code under test — expected JSON is hardcoded from
the figure number cited in the doc comment.

## Type-design constraints

### Classifier-attribute strings are bare `Option<String>` / `String`

Several types carry a `kind` (or analogous classifier) field whose
RFC 9553 spec enumerates a list of values:

- `NameComponent.kind` — `given`, `surname`, `middle`, ...
- `Title.kind` — `title`, `role`
- `Calendar.kind` — `calendar`, `freeBusy`
- `Anniversary.kind` — `birth`, `death`, `wedding`
- `PersonalInfo.kind` — `expertise`, `hobby`, `interest`
- `Directory.kind`, `Link.kind`, `Media.kind`,
  `AddressComponent.kind`

Every one of these is typed as bare `Option<String>` or `String`, NOT
as a `#[non_exhaustive] enum { Variant, …, Other(String) }`. This is
deliberate per the workspace policy "externally-owned-schema
classifier strings" exclusion in workspace `AGENTS.md`. Real-world
emitters (Outlook, Google, Apple, vCard converters, localized
clients) routinely send values outside the RFC 9553 enumeration; a
typed enum would do no real programmatic dispatch and pay a
maintenance cost for every spec revision.

A future contributor proposing "type these properly" as an enum
should be redirected to the workspace `AGENTS.md` exclusion section
and the README §Gotchas note at lines 99-107. The bare-`String`
shape is intentional, not an oversight.

Tracked by `bd:JMAP-sgrr.11`.

### Integer width policy (`UnsignedInt` fields)

RFC 9553 §1.4.2 defines `UnsignedInt` as the integer range `0..=2^53-1`
(JSON safe-integer range). Workspace convention (per the canonical
extension-types template `jmap-mail-types`) is to choose the Rust
integer width by realistic value range, not by lifting the RFC floor:

| Realistic range | Rust type | Examples |
|---|---|---|
| Counters, byte sizes potentially > 2^32 | `u64` | `Email.size`, `min_size`, `max_size` |
| Bounded small integers, ordering, counts | `u32` | `Mailbox.sort_order`, `total_emails`, JSContact `pref` |

Concrete choices in this crate:

- `pref` (Nickname, Pronouns, EmailAddress, OnlineService, Phone,
  LanguagePref, Calendar, SchedulingAddress, Address, CryptoKey,
  Directory, Link, Media): `u32`. RFC 9553 §1.5.3 bounds `pref` to
  `1..=100`; `u8` would suffice but `u32` matches the workspace pattern
  for small bounded unsigned counters.
- `list_as` (Directory, PersonalInfo): `u32`. Positional ordering;
  realistic values are tiny.
- `year`, `month`, `day` (PartialDate): `u32`. Gregorian dates fit
  trivially; the highest representable year is ~4.29 billion, far
  above any realistic JSContact use.

A spec-conformant peer sending a `UnsignedInt` value in `2^32..=2^53-1`
into one of these fields will fail to deserialize. The fields above
have no realistic trigger today, but the constraint is documented here
so a future contributor adding a new `UnsignedInt` field with a
larger realistic range knows to choose `u64`.

### Extras-preservation policy (JMAP-lbdy)

Every public `Deserialize` struct that appears on the JMAP wire carries
an `extra` field per the workspace extras-preservation policy:

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

In scope (each has a round-trip preservation test):

- `Name`, `Nickname`, `Organization`, `OrgUnit`, `SpeakToAs`,
  `Pronouns`, `Title`, `EmailAddress`, `OnlineService`, `Phone`,
  `LanguagePref`, `Calendar`, `SchedulingAddress`, `Address`,
  `CryptoKey`, `Directory`, `Link`, `Media`, `Anniversary`, `Note`,
  `Author`, `PersonalInfo`, `Relation` (23 types in `lib.rs`).

Out of scope:

- `AnniversaryDate` — outer dispatch enum; extras live on variant
  structs.

The four formerly Hash-derived value types (`NameComponent`,
`AddressComponent`, `PartialDate`, `Timestamp`) had their `Hash`
derive dropped under JMAP-lbdy.12 option A so the extras-preservation
policy applies uniformly. No callsite in the workspace uses these
types as `HashMap`/`HashSet` keys; the lost `Hash` is unreferenced.

### New-type rule

Any new public `Deserialize` struct added to this crate that appears on
the JMAP wire MUST include the `extra` field from day one with the
documented serde attributes and at least one round-trip preservation
test. New types MUST NOT derive `Hash` if they carry an `extra` field —
`serde_json::Map` does not implement `Hash`. If `Hash` is genuinely
needed on a new type, file a bead and discuss before proceeding.

### `serde_json` version-pin contract

The crate re-exports `serde_json::Value` and `serde_json::Map` in its
public API in two places:

1. The `extra: serde_json::Map<String, serde_json::Value>` flatten
   field on every wire-format struct (23 structs).
2. The `AnniversaryDate::Unknown(serde_json::Value)` variant
   (`src/lib.rs:969`).

Consumers MUST coordinate their `serde_json` major version with this
crate's. A `serde_json` 2.0 release would require a major-version
bump of this crate too: any direct caller that pattern-binds an
`AnniversaryDate::Unknown(v)` or destructures `extra` would recompile
against a different upstream `Value`/`Map` type identity.

This is per the workspace Sloppy-Value and extras-preservation
policies (see workspace `AGENTS.md`). Wrapping the `Value` in an
opaque newtype would hide the version pin but would diverge from the
workspace pattern used by ~30 sibling crates, so it is intentionally
not done.
