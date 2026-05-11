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
and derive `Debug, Clone, PartialEq, Serialize, Deserialize` (plus
`Eq, Hash` where the inner types permit it). Wire-format JSON uses
`#[serde(rename_all = "camelCase")]`. The JSContact `@type` discriminator
is mapped to a `String` field named `at_type` with `#[serde(rename = "@type")]`.

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
