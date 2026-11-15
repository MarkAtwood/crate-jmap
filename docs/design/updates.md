# Design: draft-atwood-jmap-updates-00 in crate-jmap (bd:JMAP-cayv)

Source: `/data/PROJECT/jmap-chat-spec/draft-atwood-jmap-updates-00.md` at jmap-chat-spec commit
2b85363 (Mark's own draft, tracked there as jmap-chat-spec-dhv2; not yet on datatracker). All
wire names below are quoted from it. Design only; no code in the repo was changed.

Status: final. Mark's decisions of 2026-11-15 are recorded in `docs/design/DECISIONS.md` as
decisions 27 to 31: 27 (metadata-style opt-in), 28 (queryState wrapper), 29 (libraries), 30 (view
`localizations` is null, gap G1) and 31 (the remaining recommendations of §12 adopted as written). Companion notes:
`docs/design/conditional.md` (dispatcher-level opt-in precedent), `docs/design/object-history.md`
(queryState encoding precedent, decisions 6(c) and 25).

## 1. Summary

The draft has three independent parts:

1. **Localized display text** (§3–§5): capability `urn:ietf:params:jmap:i18n` (Session `{}`,
   account `{preferredLanguages: String[]|null}`), JSCalendar-style `locale` and `localizations`
   on the localizable properties of Mailbox, Quota, AddressBook, Principal and Calendar, and a
   `language` argument (RFC 4647 language ranges) on their `/get`, `/query` and `/queryChanges`.
   This is the only part with a wire change.
2. **Text handling** (§6, SHOULD, no wire change): display-text profile, no server
   normalization, grapheme-boundary truncation, text filters follow the sort collation, IANA
   time zone ids.
3. **URIs and problem details** (§7, SHOULD, no wire change except problem-details bodies): RFC
   3986 URIs, RFC 3987 IRI mapping, exact-string capability URI comparison, RFC 9457 problem
   details from every JMAP HTTP resource, `title`/`detail` localized from Accept-Language.

No new crate. Foundation types (`LanguageTag`, `LanguageRange`, `Localizations`, the capability
types, RFC 4647 Lookup, check/prune/view helpers, problem details) go in `jmap-types`; the
argument parsing, gating, `/get` view, `/set` validation and pruning, and the queryState wrapper
go in `jmap-server` as shared helpers; each type's crate calls them from its own handlers.

**Opt-in shape (decision 27): the metadata precedent, not the conditional one.** The metadata
extension (draft-ietf-jmap-metadata-05), which is implemented, adds properties to existing
types, is gated by `using`, and asks the backend per account through
`JmapBackend::metadata_info::<O>()` (default `None`). i18n has the same shape: typed `locale` /
`localizations` fields on the five structs, a `JmapBackend::i18n_info` hook with a `None`
default, and handler-side helpers. The conditional note's separate attach call
(`register_<x>_conditional`, decisions 1 and 17) fits a feature that wraps every `/set`
generically in the dispatcher; i18n instead changes the projection, sort and filter of five
specific types, which their own handlers already do for metadata. Either way the feature is
non-breaking and the capability is advertised only by a deployment that opts in.

## 2. Spec coverage

| § | Requirement | Where / why |
|---|---|---|
| 3 | Session capability `urn:ietf:params:jmap:i18n`, value `{}` | `jmap_types::i18n::{JMAP_I18N_URI, I18nCapability}`; Session is the consumer's job; testjig advertises it |
| 3 | Account capability `{preferredLanguages: String[]\|null}` per account that supports localizations | `I18nAccountCapability { preferred_languages: Option<Vec<LanguageTag>> }`; present iff `i18n_info(caller, account)` is `Some` |
| 3 | Without the URI in `using`: neither return nor accept `locale`/`localizations`; `language` → `invalidArguments` | `jmap_server::i18n::I18nRequest::from_args` (one call at the top of each affected handler, Q2); `/get` strips the two properties; `/set` naming them → `invalidProperties` (same as metadata's `check_metadata_entry`) |
| 3 | URI in `using` but account lacks the account capability: `/set` naming them → `invalidProperties`; `language` → `invalidArguments` | Same helper, keyed on `i18n_info(..)` being `None` |
| 3.1 | `preferredLanguages` MAY drive out-of-request text; MUST NOT change `/get`/`/query` | Hook result is advertised only; handlers never read it. Rustdoc on the hook |
| 9 | SHOULD return `preferredLanguages: null` for accounts the user does not own | `I18nInfo::preferred_languages` doc: backend returns `None` unless `principal_id(caller)` owns the account; memory backend does so; testjig asserts |
| 4.1 | `locale: String\|null` (default null), `localizations: String[PatchObject]\|null` (default null) | `Option<LanguageTag>`, `Option<Localizations>` fields on Mailbox, AddressBook, Principal, Calendar (and Quota later); `#[serde(default, skip_serializing_if = "Option::is_none")]`, and the `/get` projection emits `null` explicitly when i18n is in `using` |
| 4.1 | PatchObject keys are localizable properties only; values of the property's type; null only where nullable | `Localizable` trait in `jmap-types`: `const LOCALIZABLE: &[LocalizableProperty]` (name + nullable); `check_localizations` |
| 4.1 | Keys are language tags; unique ignoring case; no key equals `locale` ignoring case; SHOULD store canonical case | `LanguageTag::parse` (RFC 5646 well-formed, Q5) + canonical case on store; check rejects duplicates |
| 4.1 | Base-value rules (length, non-empty, sibling uniqueness) apply to base; SHOULD apply length/syntax limits to localized values | `check_localizations` takes a per-type value check closure; Mailbox reuses its Net-Unicode / length check; uniqueness stays base-only |
| 4.1 | Invalid `localizations` → `invalidProperties` naming `localizations`; MAY cap the count | `check_localizations` → `SetError::InvalidProperties(["localizations"])`; cap via `I18nInfo::max_localizations` (backend caps convention) |
| 4.1 | Base value changed: SHOULD prune that property from each untouched language's PatchObject, drop empty PatchObjects, same update; echo `localizations` in `updated` | `prune_stale_localizations(old, new, patch)` in `jmap-types`; handlers call it on update and add `localizations` to the `updated` echo when it removed anything (Q-gap G2) |
| 4.1 | Server-set localizable property (Quota `name`): server fills `locale`/`localizations` | Quota backend (after JMAP-1igi); memory backend seeds examples |
| 4.2 | `language: String[]\|null` on `/get`, `/query`, `/queryChanges` of types with localizable properties | `LanguageRange` list in `I18nRequest`; types without localizable properties never accept it |
| 4.2.1 | `/get` view: RFC 4647 §3.4 Lookup over `localizations` keys + `locale`; fallback to `locale`; apply PatchObject; set `locale` to chosen tag; `*` handling | `jmap_types::i18n::lookup` and `localized_view(&mut Value, &[LocalizableProperty], &ranges)` on the serialized object, before `properties` filtering; the view returns `localizations: null` (decision 30) |
| 4.2.1 | State unchanged by language | Views are built after the backend read; `state` untouched |
| 4.2.1 | MUST NOT let Accept-Language change `/get`/`/query` when i18n in `using` | Handlers never consult `accept_language` (Q8) for data; the Quota RFC 9425 selection reads it only without i18n in `using` |
| 4.2.2 | `/query` sort/filter on localizable property compares the `/get`-with-`language` value | `QueryParams::language` / `QueryChangesParams::language` (Q3); Mailbox `name` filter+sort, Principal `name` filter+sort, Quota `name` filter+sort. AddressBook and Calendar have no `/query` |
| 4.2.2 | MAY pick a collation by language when the Comparator names none | Not done (MAY); default collation unchanged |
| 4.2.3 | `queryState` reflects `language`; mismatch on `/queryChanges` → `cannotCalculateChanges` | Handler-level wrapper around the backend's queryState (Q4) |
| 4.3 | WebSocket: SHOULD use the handshake's Accept-Language for every Request on that connection | Consumer's transport fills the CallerCtx from the handshake; `accept_language` doc (Q8); base-client WS `connect` option sends the header |
| 5 | Localizable properties per type; Identity, ParticipantIdentity, VacationResponse, ContactCard, CalendarEvent, ShareNotification excluded | §4 table; ShareNotification `name` filled from base value (sharing-server doc + test) |
| 5.2 | Quota: without i18n, `description` by Accept-Language (RFC 9425); with i18n, base + all localizations, or the `language` view | JMAP-cayv.11, after JMAP-1igi |
| 5.5 | Calendar `name`/`description` (unpublished draft) | Q9 |
| 6.2 | Display-text profile SHOULD; MAY reject with `invalidProperties` | `jmap_types::text::check_display_text(s, multiline)`; not enforced by default (Q-gap G4) |
| 6.3 | SHOULD NOT normalize stored text | Audit (JMAP-cayv.15): grep for `nfc`/`nfkc`/`normalize` in non-collation paths |
| 6.5 | Limits in octets; truncate at extended grapheme cluster boundaries | `jmap_types::text::truncate_graphemes(s, max_octets)` (Q7); used by Email `preview`, push payloads, chat previews |
| 6.6 | Text filters use the sort collation | Audit of every text `FilterCondition` (Mailbox `name`, Principal `name`/`text`, etc.) |
| 6.7 | IANA time zone ids | Audit of `timeZone` properties; no new validation |
| 7.1 | URI-valued properties SHOULD be RFC 3986 URIs, absolute unless allowed; templates expand to URIs | `jmap_types::uri::check_uri` (Q6); audit list in JMAP-cayv.14 |
| 7.1 | IRI → URI before sending (RFC 3987 §3.1), A-label hosts; MAY convert back for display | `iri_to_uri` / `uri_to_iri` helpers (Q6) |
| 7.1 | Capability URIs compared as exact strings | Audit: `using` matching is already `==` on `String` (helpers.rs `uses_metadata` etc.); test with a case-variant URI |
| 7.2 | RFC 7807 → RFC 9457 references | rustdoc and base-client `ProblemDetails` doc |
| 7.2 | Every HTTP resource SHOULD return problem details on 4xx/5xx (Session, API, upload, download, event source, WS handshake, extension endpoints) | `jmap_types::problem::ProblemDetails` builder + constructors per RFC 8620 / RFC 8887 type; the workspace has no HTTP, so testjig is the only resource owner |
| 7.2 | `title`/`detail` MAY be localized from Accept-Language with Content-Language | `ProblemDetails` carries an optional `content_language`; testjig localizes one type to exercise it |
| 8 | Clients SHOULD isolate bidi text; show base value next to localized on share actions | Client crates: rustdoc only (no UI in the workspace) |
| 10 | IANA capability | `JMAP_I18N_URI` const |

## 3. Crate placement

No new crate. Extend:

- `jmap-types`: new modules `i18n` (capability types, `LanguageTag`, `LanguageRange`,
  `Localizations`, `Localizable`, `LocalizableProperty`, `lookup`, `check_localizations`,
  `prune_stale_localizations`, `localized_view`), `text` (`check_display_text`,
  `truncate_graphemes`), `uri` (`check_uri`, `iri_to_uri`, `uri_to_iri`) and `problem`
  (`ProblemDetails`, RFC 8620 / 8887 type constants). No async (Key Rules).
- `jmap-server`: `JmapBackend::i18n_info` and `JmapBackend::accept_language` hooks (Q1, Q8),
  `i18n` helper module (`I18nRequest::from_args`, the `/get` projection, the `/set` check-and-prune
  call, the queryState wrapper), `QueryParams::language` and `QueryChangesParams::language`.
- `jmap-mail-types` / `-server`: Mailbox. `jmap-contacts-*`: AddressBook. `jmap-sharing-*`:
  Principal. `jmap-calendars-*`: Calendar (Q9). The quotas crate (JMAP-1igi): Quota.
- `jmap-base-client` and the four typed clients: `language` option, `locale`/`localizations`
  fields, `preferredLanguages` reader, WS handshake Accept-Language, RFC 9457 parsing.
- `jmap-testjig`: opt-in, problem details on every resource, end-to-end tests.

Chat adoption (Message `language`, Chat/Space localizable properties) is out of scope: it waits on
jmap-chat-spec-qb21.

## 4. Wire types (`jmap-types`)

```rust
pub const JMAP_I18N_URI: &str = "urn:ietf:params:jmap:i18n";

#[non_exhaustive] #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct I18nCapability {}                       // Session value {}

#[non_exhaustive] #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct I18nAccountCapability { pub preferred_languages: Option<Vec<LanguageTag>> }

/// RFC 5646 well-formed tag, stored in RFC 5646 §2.1.1 canonical case;
/// Eq/Hash/Ord are case-insensitive (equal to canonical-case equality).
pub struct LanguageTag(String);
impl LanguageTag { pub fn parse(s: &str) -> Result<Self, LanguageTagError>; pub fn as_str(&self) -> &str; }

/// RFC 4647 §2.1 basic language range ("*" or alnum subtags).
pub struct LanguageRange(String);

/// `localizations`: tag -> PatchObject of localizable properties.
pub struct Localizations(BTreeMap<LanguageTag, serde_json::Map<String, Value>>);

pub struct LocalizableProperty { pub name: &'static str, pub nullable: bool }
pub trait Localizable { const LOCALIZABLE: &'static [LocalizableProperty]; }

/// RFC 4647 §3.4 Lookup; None when nothing matches and no default.
pub fn lookup<'a>(ranges: &[LanguageRange], available: impl IntoIterator<Item = &'a LanguageTag>)
    -> Lookup<'a>;  // Matched(&tag) | Default
```

Deserialization of `Localizations` is lenient (any object), so `check_localizations` can name
`localizations` in an `invalidProperties` error instead of failing the whole create at serde.

`localized_view` works on the serialized `serde_json::Value` of an object (as
`MetadataProjection` does), so one function serves every type.

Problem details: `ProblemDetails { type_, status, title, detail, instance, limit, extra,
content_language }` with constructors for the RFC 8620 §3.6.1 request-level types
(`unknownCapability`, `notJSON`, `notRequest`, `limit`) and RFC 8887's. The base client already
has a WS-specific struct (`crate-jmap-base-client/src/ws/mod.rs:151`); JMAP-cayv.13 decides whether
it becomes a thin alias over the new type (non-breaking) or stays.

## 5. Server design (`jmap-server`)

### 5.1 Gating (Q1, Q2)

```rust
pub struct I18nRequest { ranges: Option<Vec<LanguageRange>>, enabled: bool }
impl I18nRequest {
    /// Reads and removes `language`; checks `using` and `i18n_info`.
    pub fn from_args<B: JmapBackend, O: Localizable>(backend, caller, account_id,
        using: &[String], args: &mut Map<String, Value>) -> Result<Self, JmapError>;
}
```

- `language` present and non-null without the URI in `using`, or on an account whose
  `i18n_info` is `None` → `invalidArguments` (draft §3, "as it would any unknown argument").
  `language: null` is the default and accepted (workspace `optional_arg` convention).
- `language` on a type that has no localizable properties → `invalidArguments` (unknown argument).
- `/set` create or update naming `locale` or `localizations` (or a `localizations/...` patch path)
  without the URI or on such an account → `invalidProperties` SetError (draft §3), per object.
- `/get` without the URI never returns the two properties, even when `properties` names them
  (as an unknown property: `invalidArguments`, RFC 8620 §5.1).

This is a per-handler call, not one dispatcher check, because only five types have the argument and
the result is needed inside the handler. The conditional decision 2 / 18 rule ("reject on every
deployment, opted in or not") is kept: `from_args` runs on every deployment.

### 5.2 `/get`

After the backend read and the metadata projection, before `properties` filtering:
`localized_view(obj, O::LOCALIZABLE, ranges)` for each object when `ranges` is `Some`. With
`ranges` `None` and i18n in `using`, emit `locale`/`localizations` (as `null` when absent).

### 5.3 `/set`

Create: `check_localizations` then store. Update: apply the patch to a copy, `check_localizations`
on the result, `prune_stale_localizations(old, &mut new, patch)`; if it removed anything, add
`localizations` to the `updated` echo (RFC 8620 §5.3 server-changed properties). Pruning runs
whether or not the request uses i18n (the draft's point is exactly the client that does not), but
the echo is only for i18n clients. The backend stores what the handler hands it, as today.

### 5.4 `/query`, `/queryChanges` (Q3, Q4)

The handler passes `ranges` through `QueryParams::language` (new `#[non_exhaustive]` field + builder,
non-breaking). The backend sorts and filters on `localized_value(obj, prop, ranges)`; a shared
helper in `jmap-types` gives it the value. A backend whose `i18n_info` is `Some` MUST honour
`language` (rustdoc); a backend that returns `None` never sees it.

queryState: the handler wraps the backend's state when `language` is non-null and unwraps it on
`/queryChanges` (Q4 has the encoding). A language mismatch, or a wrapped state sent without
`language` (or the reverse), → `cannotCalculateChanges`, before the backend is called.

### 5.5 Session

`jmap_server` has no Session builder; consumers (and testjig) add `I18nCapability` and, per account,
`I18nAccountCapability` from `i18n_info`. `I18nInfo::preferred_languages` is `None` for accounts the
caller does not own (draft §9).

## 6. Backend hooks

```rust
// JmapBackend (non-breaking, defaulted like metadata_info / principal_id)
fn i18n_info(&self, caller: &Self::CallerCtx, account_id: &Id) -> Option<I18nInfo> { None }
fn accept_language(caller: &Self::CallerCtx) -> Option<&str> { None }   // Q8

#[non_exhaustive] pub struct I18nInfo {
    pub preferred_languages: Option<Vec<LanguageTag>>,
    pub max_localizations: Option<usize>,
}
```

The `None` default is allowed by "Default bodies on spec-semantic backend methods": it says the
account has no localizations, which is true data, and the handlers then reject the extension's
arguments as the draft requires.

## 7. Memory reference backend scope

Mailbox, AddressBook, Principal, Calendar memory backends: store the two fields, return
`Some(I18nInfo)` with `preferred_languages` for owned accounts, honour `QueryParams::language` in
Mailbox and Principal sort/filter, cap 32 localizations per object. Default Mailboxes get `locale:
"en"` and no localizations (a MAY; keeps fixtures simple).

## 8. Client bindings

`jmap-base-client`: `GetCall`/`QueryCall`/`QueryChangesCall` gain `language(Vec<LanguageRange>)`;
the request builder adds the URI to `using` when it is set; `Session::preferred_languages(account)`;
WS `connect` takes an optional Accept-Language; `ProblemDetails` parsing per RFC 9457. Typed
clients (mail, contacts, sharing, calendars): `locale`/`localizations` come with the types.

## 9. Interactions, breaking changes

- Non-breaking throughout: new fields on `#[non_exhaustive]` structs, new defaulted trait methods,
  new builder setters. Public API snapshots change (additions only).
- Metadata: both extensions project the same `/get` object; order is metadata projection, then
  localized view, then `properties` filtering.
- Object history (decision 5): `locale`/`localizations` are ordinary properties of each version.
- Conditional (`ifUnchangedBy`): pointers into `localizations/...` compare the stored value, since
  conditional reads `Foo/get` without `language`.
- Quota RFC 9425 Accept-Language selection needs `accept_language` even without i18n.

## 10. Test plan

Oracles independent of the code (Key Rules): RFC 4647 §3.4 worked examples; RFC 5646 Appendix A
tags (well-formed and ill-formed); the draft's Appendix A examples verbatim; UAX 29
GraphemeBreakTest.txt lines for truncation; RFC 3986 Appendix and RFC 3987 §3.1 examples for URIs;
RFC 9457 §3 examples. Per type: gating matrix (no URI / URI without account capability / both) ×
(`/get`, `/set`, `/query`, `/queryChanges`); pruning cases (base changed, language touched as whole,
by key, by property; empty PatchObject removed; echo only to i18n clients); `language` view
fallbacks (`*` last, `*` first, no match → `locale`, null `locale`); queryState mismatch. testjig:
end-to-end plus problem details on each resource.

## 11. Bead breakdown

Already filed under JMAP-cayv; this note only adjusts their text once §12 is answered:
.2 jmap-types i18n module; .3 `/get` language + gating; .4 `/set` validation, pruning, echo;
.5 `/query`/`/queryChanges`; .6 Session capability; .7 Mailbox; .8 AddressBook; .9 Principal;
.10 Calendar; .11 Quota (after JMAP-1igi); .12 clients; .13 problem details; .14 URIs;
.15 text handling; .16 testjig.

Spec gaps in Mark's draft (fix in jmap-chat-spec, not upstream; spec-sync-scope):

- **G1 (decision 30).** A `/get` with `language` sets `locale` to the chosen tag but does not say
  what `localizations` holds in the view. Returned as stored, the chosen tag can be both `locale`
  and a key, which §4.1 forbids. Ruling: in a view, `localizations` is returned as `null` (a
  single-language view); draft §4.2.1 to say so.
- **G2.** "Includes `localizations` in the object it returns in `updated`": say it is the full
  value after pruning, not a patch.
- **G3.** `/set` with `locale` changed but base values unchanged: does that prune? Recommendation:
  no (no base value changed); say so.
- **G4.** §6.2 is SHOULD NOT on content, MAY reject. Recommendation: the crates provide the check
  and do not reject by default (a deployment choice), except Mailbox, which already checks
  Net-Unicode.

## 12. Questions settled

Q1 to Q9 are the bead's questions (a) to (i). Mark ruled on Q1 (decision 27), Q4 (28) and
Q5–Q7 (29); the others are the recommendations below, adopted as written (decision 31).

1. **Opt-in shape:** metadata-style `JmapBackend::i18n_info` hook (default `None`) plus handler
   helpers, not a `register_<x>_i18n` attach call (§1).
2. **Arguments without the URI:** per-handler `I18nRequest::from_args` on every deployment
   (§5.1), same outcome as decisions 2/18.
3. **Query:** `QueryParams::language` / `QueryChangesParams::language`, backend MUST honour when it
   reports i18n support. Generic post-processing in jmap-server cannot work: the handler pushes
   `limit`/`position` to the backend (`crate-jmap-mail-server/src/mailbox.rs:341`).
4. **queryState:** handler wraps the backend state as `<inner>|lang=<r1>,<r2>` (ranges in canonical
   case) when `language` is non-null. Range characters never include `|`, so splitting at the last
   `|lang=` is unambiguous; backend states are documented as not containing `|lang=`.
5. **BCP 47:** hand-rolled RFC 5646 ABNF well-formedness + §2.1.1 canonical case + RFC 4647 Lookup
   (decision 13's no-new-deps lean). ICU4X `icu_locale_core` is already in the tree but parses
   Unicode BCP 47 locale ids, which reject RFC 5646 extlang and grandfathered tags.
6. **RFC 3986:** `iri-string` 0.7 (already in Cargo.lock via reqwest/tower-http): strict RFC
   3986/3987 validation and the IRI→URI mapping; `idna` (already in the tree) for A-label hosts.
   The workspace `url` crate is WHATWG and stays for HTTP use.
7. **Graphemes:** ICU4X `icu_segmenter` 2.2 (same release and Unicode version as the
   `icu_casemap`/`icu_normalizer` already in `[workspace.dependencies]`), grapheme segmenter only.
   A hand-rolled UAX 29 scan needs the property tables, so it is not realistic.
8. **Accept-Language:** `JmapBackend::accept_language(caller) -> Option<&str>`, an associated fn with
   a `None` default, the same shape as `principal_id` (CallerCtx is a bare associated type with no
   trait to add an accessor to). The consumer fills it from the HTTP header, or from the WS handshake
   for every request on the connection.
9. **Calendars:** ship Calendar `name`/`description` behind the same opt-in now, after JMAP-lehj's
   -34 sync lands, citing the draft row "when it is published".
