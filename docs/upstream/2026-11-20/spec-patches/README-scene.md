# scene-cordite-requests.patch

Five JMAP Scene requests from the project-cordite review (crate-jmap
bd JMAP-8loa, decision 58). This is a draft for Mark's review. Nothing is
committed in jmap-chat-spec or crate-jmap.

Revision 2 (2026-11-20) applies Mark's answers to the first draft's open
questions; see "Mark's answers" at the end.

- **Base:** jmap-chat-spec `55246b1` (HEAD on 2026-11-20). The last commit
  that touched any of the five files is `f2e88ed`. `git apply --check`
  passes against `55246b1`.
- **Apply:** `git -C /data/PROJECT/jmap-chat-spec apply /data/PROJECT/crate-jmap/docs/upstream/2026-11-20/spec-patches/scene-cordite-requests.patch`
- **Files:** `draft-atwood-jmap-scene-00.md` (+115/-12),
  `draft-atwood-jmap-scene-wss-00.md` (+148/-19), and the guides that would
  otherwise contradict the drafts: `jmap-scene-wss-guide.md`,
  `jmap-scene-board-games-guide.md`, `jmap-scene-implementer-guide.md`.
- **Render check:** each draft was rendered with `kdrfc -3` (kramdown-rfc
  1.7.43 + xml2rfc) using the repo's `.refcache`, before and after the
  patch, in a scratch copy of HEAD (revision 2 was checked the same way).
  Neither run printed a warning. `idnits`
  reports the same findings before and after: scene-00 has 7 warnings and 6
  comments, scene-wss-00 has 4 warnings and 4 comments. All of them are
  pre-existing (draft references not yet posted, object-history -03 stale,
  em dashes, external normative references). The scratch copy was deleted
  afterwards.
- **Governance:** the patch edits the core drafts. Mark made this an
  explicit exception to PROJECT.md rule 3 (two independent implementations
  before a core change) on 2026-11-20, recorded as crate-jmap decision 60.
  PROJECT.md itself is not edited.
- **Compatibility:** every addition is optional. A new capability field
  that is absent means `false`, and a new filter property that the server
  does not support gets `unsupportedFilter`, which the draft already
  requires. Unknown event `@type` values are already ignored (Scene WSS
  L990). A server built to today's text still conforms.
- **IANA:** no change. The draft registers capabilities, data types, object
  URI types and `contentPolicy` (scene-00 L2973-L3054). The patch adds no
  capability URN, data type or error code. Capability properties and
  WebSocket `@type` values have no registry.

Line numbers below are in jmap-chat-spec `55246b1`.

## 1. Region-wide events from the server: `SceneRegion/broadcast` and `SceneRegionEvent`

**Problem.** Scene WSS has three event types, and each one names a user,
avatar or object as its source. `SceneInteractionEvent` needs "an active
avatar ... in the region" and "a SceneObject in that region with
`interactable: true`" (WSS L416-L419). The server also "sets `userId` on the
relayed event to the authenticated user's userId" (WSS L411-L413). Cordite
decision 7 therefore puts the kill feed on a match-state SceneObject and
depends on `SceneObjectEvent` followed by `/get`, which is limited to two
updates per second (WSS L699-L704).

**Change.**
- scene-00 adds the `SceneRegion/broadcast` method beside
  `SceneObject/interact`. It takes `{accountId, events: Id[SceneRegionBroadcast]}`,
  where each event is `{regionId, event, data}` and `event` is a
  reverse-domain name. The response is `accepted` / `notAccepted`, the same
  shape as `/interact` (L2100-L2195). The SetErrors are `notFound` (no access),
  `forbidden` (caller is not the region owner or an admin), `rateLimit`, and
  `invalidProperties`. The server MAY also originate region events itself.
- A new optional account capability field, `supportsRegionBroadcast`.
- scene-wss adds the server-to-client event `SceneRegionEvent`
  `{accountId, regionId, event, data}`. The patch also adds it to
  `eventTypes`, the event summary table and the blocked-sender rules. It has
  no originating user, so blocked-sender suppression does not apply. It is
  not filtered for visibility, and the text says it carries only what every
  user with access to the region may see.

**Why this design.** It reuses the permission rule the draft already has for
region-wide writes. Updating the region, including its `worldState`, is
limited to "the region owner and does not have deployment-defined
administrative privileges" → `forbidden` (L1240). A broadcast is an
ephemeral, region-wide write, so it gets the same gate. The method mirrors
`/interact`, so clients and servers reuse the same request and response
code.

**Alternatives considered.**
- *Relax `SceneInteractionEvent`* (make `objectId` optional and let `userId`
  be null). Rejected because it changes the meaning of an existing event type
  that clients already parse, and the interaction rate limit (five per user
  per region per second, WSS L706-L710) is the wrong limit for this traffic.
- *A client-to-server frame*, like interactions (WSS L396-L428). Left out
  because a frame gets no response, so a game server could not tell when it
  was rate-limited. This could be added later.
- *Status quo* (a match-state SceneObject). This still works, and the games
  guide keeps it as an option.

## 2. Observer subscription without an avatar: `observe` in `SceneStreamEnable`

**Problem.** "All Scene ephemeral events are per-region: they are delivered
only for regions ... for which the user has an active avatar" (WSS
L689-L692), and the same rule is repeated in Event Authorization (WSS
L1186). A spectator has to enter the region. That counts toward
`activeAvatarCount` and `maxAvatarsPerRegion`, and it announces the
spectator with a `SceneAvatarEvent` (cordite decision 12).

**Change.**
- scene-wss: `SceneStreamEnable` gets an optional `observe` (Boolean)
  property. When it is `true`, each region in `regionIds` delivers events
  while the user has *access* to the region (scene-00 Region Access,
  L2682). An active avatar is not needed. Observing creates no avatar, does
  not change `activeAvatarCount`, and sends no `SceneAvatarEvent`.
  Visibility filtering still applies, and submitting an interaction still
  requires an avatar.
- `observe: true` with `regionIds: null` is `invalidArguments`, following
  the existing `eventTypes` error rule (L562-L567).
- **Owner control, `SceneRegion.allowObservers`** (Boolean, default
  `false`). Only regions with `allowObservers: true` can be observed. It is
  an ordinary region property, so the region owner and admins update it
  under the existing rule (L1240). Setting it to `false` stops delivery to
  current observers; they learn of it from the SceneRegion change.
  - *Why default `false`:* no existing or newly created region becomes
    observable unless its owner opts in. Users who entered a region under
    today's rules keep the guarantee that the avatars they see are everyone
    who receives the region's events. A default of `true` would silently
    expose every region on a server that turns the feature on.
- **A refused observe is an error.** When a listed region the user can
  access has `allowObservers: false`, or the server declines observers by
  policy (the server keeps that right), `SceneStreamEnable` fails with a
  `RequestError` of type `urn:ietf:params:jmap:error:forbidden` that has a
  `regionIds` member listing the refused regions. The subscription is left
  unchanged, as for the existing `invalidArguments` case (L562-L567). A
  region the user cannot access, or that does not exist, is *not* refused.
  Its entry is kept and produces no events, as today (L542-L548), so the
  error does not reveal whether the region exists.
- **`SceneRegion.observerCount`** (UnsignedInt, server-set, read-only):
  the number of distinct users observing the region without an active
  avatar. Everyone with region access sees it, so participants know they
  are watched, though not by whom. A change to it is a SceneRegion update,
  like `activeAvatarCount`: `SceneRegion/changes` reports the region in
  `updated`, the SceneRegion state string advances, and push sends a
  `StateChange`. The existing SceneRegion coalescing (State-Change
  Frequency, L2857) applies and now mentions `observerCount`. The
  count drops when a subscription stops covering the region (a new enable,
  disable, connection close, or the user entering with an avatar).
  Region create sets it to `0`.
- Both region properties appear only when the server advertises
  `supportsObserve`.
- The WSS capability value changes from "an empty JSON object `{}`" (L139)
  to an object with one optional field, `supportsObserve`. `{}` keeps its
  current meaning.
- A new Security subsection, "Observers", says that an observer sees
  nothing that entering the region would not show, but it does not appear
  as an avatar. Observation is off unless the owner allows it, and
  `observerCount` shows how many are watching.
- The `ChatStreamEnable` `"scene"` path stays limited to regions with an
  active avatar.

**Why this design.** Scene already gives every user with region access the
full state through `/get` and push. Access "covers both seeing the region
with its objects and avatars and entering it" (L2682). Gating the
ephemeral stream on the same access check adds no new permission concept.

**Alternatives considered.**
- *A spectator role on SceneAvatar* (for example `isObserver`). It still
  creates a record, takes up a slot, and appears in `SceneAvatar/query`.
  Rejected.
- *Observer on by default for `regionIds` lists.* This would silently change
  what current clients receive. Rejected.
- *Deployment policy only, with silent refusal* (the first draft). Mark
  asked for control in the protocol and a clear error.
- *An observer list instead of a count.* This would reveal who is watching;
  Mark asked for a count.

## 3. Avatar visibility contract

**Problem.** The visibility contract covers SceneObject only: "The server
SHOULD NOT include a SceneObject ..." (L2664) and "The server SHOULD apply
visibility filtering to limit the objects returned" (L2666). Any user with
region access can read every avatar's `position`. In a competitive round
that is a wallhack, so cordite decision 11 stops reconciling positions
during rounds. Scene already hides avatars in two specific cases, E2EE-bound
regions (L2741) and blocked contacts (L2757-L2765), but it has no general
rule.

**Change.** A new subsection, Visibility Contract → **Avatar Visibility**
(`{#avatar-visibility}`):
- A server MAY hide avatars, or withhold some of their properties, per
  caller. The rule is deployment-defined. The E2EE and blocked-contact
  rules become instances of it. A caller always sees their own avatar in
  full. The server SHOULD NOT hide anything from the region owner or admins,
  who eject avatars and maintain `regionState`.
- **Hidden avatar:** treated as nonexistent. `/get` reports `notFound`,
  `/query` leaves it out, and `/set` update returns `notFound`.
- **Withheld properties:** only `position`, `orientation`, `worldState` and
  `regionState` can be withheld. For position and orientation the server
  returns a value that leaks nothing, such as the spawn point or the last
  value the caller was allowed to see. For the two state objects it leaves
  out the withheld top-level keys. This follows the `invitedUserIds`
  precedent, which "returns `[]`" to other callers (L530). `/query` filters
  and sorts use the values returned to the caller, so a `withinRadius`
  query cannot find a hidden position.
- **`/changes`, `/queryChanges` and push:** a server that hides avatars to
  protect information SHOULD answer from the caller's view. An avatar
  entering the caller's view is reported in `created`, one leaving it in
  `destroyed`, and an avatar is reported in `updated` only when a value
  returned to the caller changes. The state string stays one per account,
  and it advances when the caller's view changes, so `StateChange` push
  fires. `destroyed` means "left your view", not "left the region"
  (presence is `leftAt`). A server that hides avatars only for blocked
  contacts or E2EE MAY keep reporting changes for the whole account, which
  is today's model (L2674). L2674 gains the text "except as
  {{avatar-visibility}} describes for SceneAvatar".
- `SceneAvatar/get` (L2203) and `SceneAvatar/query` (L2578) point to the new
  subsection. Security "Visibility and Information Leakage" (L2753) now
  covers avatars and says that region `worldState` and region events are
  never filtered.
- scene-wss Visibility Filtering: a `SceneAvatarEvent` about an avatar
  hidden from the recipient is not delivered.

**Why this design.** It follows the JMAP rule that a record a caller cannot
see behaves as if it does not exist. The `notFound`-not-`forbidden` rule is
already applied across Scene (L2693). The per-caller `/changes` follows
approach B of the implementer guide (§8, "Per-user visibility filtering").
That approach is the only one that does not leak the existence and movement
of hidden avatars through `/changes`. Approach A remains for the existing
cases, so current servers still conform.

**Alternatives considered.**
- *Make `position` nullable.* This changes an existing non-null type
  (`Number[3]`, L702). Rejected.
- *Account-wide `/changes` for avatars as well.* The ids and change times in
  `updated` would tell an opponent who is alive and moving. Rejected for
  the information-protecting case.
- *Per-user state strings.* This goes further than RFC 8620's model of one
  state string per account. It was not needed: a per-caller `/changes` body
  under an account-wide state string is enough. Mark chose to keep one
  state string per account.

**Residual leak, now stated in the draft.** Security "Visibility and
Information Leakage" gains a paragraph: because there is one state string
per account, a caller can tell that *some* SceneAvatar changed (a new state
string, or a `StateChange` with no ids it can see) even when the change is
hidden. It does not learn which avatar changed or how. Deployments can
reduce the leak by reconciling less often while information is hidden, or
by coalescing changes.

## 4. `worldStateEquals` filter on `SceneRegion/query`

**Problem.** The region filters are only `name`, `accessPolicy`, `viewHint`,
`hasSimulationUri`, `hasActiveAvatars`, `createdAfter` and `createdBefore`
(L1431-L1452). A lobby browser has to fetch every region and test
`worldState.gameSchema` on the client.

**Change.** A new optional filter, `worldStateEquals` (Object). Each property
is a top-level `worldState` key with a String, Number or Boolean value. A
region matches when it has every key with an equal value of the same JSON
type. Numbers are compared by numeric value. An empty object matches every
region. Any other value type gives `invalidArguments`, and a server that
does not support the filter returns `unsupportedFilter` (L1458). The patch
also adds an example query that uses the games guide's existing
`gameSchema` key.

**Why this design.** Top-level scalar equality covers discovery, the use
case that came up: the Baize and cordite `gameSchema`, `gameSchemaVersion`
and `corditeMode` keys. It does not need a path or expression language.
Region `worldState` is already readable by everyone with region access
(L539: "region-level application state must be visible to every user who
can enter the region"), so filtering on it reveals nothing new.

**Alternatives considered.**
- *JSON Pointer paths or a match expression.* More expressive, but larger
  to specify and to index. Not needed yet.
- *A `worldStateHasKey` filter.* Not needed for discovery.
- *The same filter for SceneObject and SceneAvatar.* No current request;
  kept out by Mark (answer 5).

## 5. Region-owned avatar state: `SceneAvatar.regionState`

**Problem.** "Users may only modify their own SceneAvatar record, except for
the eject case below" (L2709), and "An eject update patches only `leftAt`"
(L2711). The game server (the region owner) therefore cannot record a
player's team, health or money on that player's avatar. Cordite decision 8
and the games guide after `f2e88ed` use a per-player SceneObject owned by
the region owner instead.

**Change.**
- A new optional property, `SceneAvatar.regionState` (Object|null, default
  `null`), present only when the new account capability field
  `supportsAvatarRegionState` is `true`. Only the region owner, admins and
  the server itself change it. The avatar's user can read all of it (no
  owner-only keys). Its value is kept across leave and re-entry. One added
  sentence says that secret per-player state, which the player must not
  see, belongs in a SceneObject owned by the region owner, not in
  `regionState`.
- Avatar Permissions: region owners and admins MAY change `regionState` on
  any avatar in their region. An owner or admin update to another user's
  avatar may combine an eject with a `regionState` change, and patches
  nothing else. A user who changes their own `regionState`, or supplies it on
  create, gets `forbidden` unless they are also the owner or an admin. The
  matching rows are added to the create and update SetError tables, and
  "Updating Avatar State" (L2337) refers to the rule.
- `worldState` gains the sentence "Only the avatar's user changes it."
- State Reconciliation gains a bullet saying that per-avatar state owned by
  the simulation layer goes in `regionState`, not `worldState`. Without
  `regionState`, it goes on SceneObjects owned by the region owner. This is
  the explicit guidance the request asked for, whichever way the server
  goes.

**Why this design, and why not let the owner write `worldState`.** The
permission model already makes the region owner the authority over
everything in a region except the user's own choices. The owner may update
the region (L1240), may change any object in it ("2. The region owner",
L2702), and may eject any avatar (L2711). Extending that authority to one
dedicated avatar property is a small step within the same model. The
property has to be separate because `worldState` is the user's. If the owner
could write `worldState`, the user could overwrite the owner's keys (for
example, set their own health), or the owner could overwrite what the user
chose. Fixing that would need key-level ACLs. A separate property gives each
side one object that only it writes, and with Avatar Visibility the server
can withhold, for example, `regionState.money` from opponents.

**Alternatives considered.**
- *Owner or admin may patch `worldState`.* Rejected because it breaks the
  integrity of `worldState` (see above).
- *Guidance only (status quo, per-player SceneObjects).* This works today
  and is kept as the fallback in the draft and the guide. Its cost is that
  each player's identity is split across two records, joined by a
  deployment key (`player`), and object visibility has to be computed per
  player.
- *Owner-only keys in `regionState`, hidden from the avatar's user.*
  Rejected by Mark: a player sees all of their own avatar. Secret
  per-player state goes on a SceneObject owned by the region owner.

## Guide changes (for consistency only)

- `jmap-scene-wss-guide.md`: adds `SceneRegionEvent` where the guide lists
  the event types ("all three" becomes four), adds observer and
  hidden-avatar cases to the suppression and delivery-time authorization
  lists (including `allowObservers` and the `forbidden` RequestError), and
  adds a client note to check `supportsObserve` before sending
  `observe: true`.
- `jmap-scene-board-games-guide.md`: the Spectators section mentions observer
  subscriptions, the owner's `allowObservers` opt-in and `observerCount`. The Doom damage, Quake hidden-information and
  Counter-Strike pattern sections use `regionState` where the server
  supports it, with the `f2e88ed` per-player SceneObject as the fallback.
  The Counter-Strike section also notes that round and kill events can be
  region events.
- `jmap-scene-implementer-guide.md` §8: the avatar-permission summary
  mentions `regionState`, and the per-user visibility recommendation notes
  that the draft now asks for approach B's `/changes` for avatars when
  hiding protects information.

## Implementation impact in crate-jmap

### `crate-jmap-scene-types`
- `region.rs`: add `SceneRegion.allow_observers` and `observer_count`.
  Both must be able to be absent (no `supportsObserve`), so they need
  `Option` with a skip rule; `observerCount` goes in `SERVER_SET`.
- `capability.rs`: add `SceneCapability.supports_region_broadcast` and
  `supports_avatar_region_state` (`#[serde(default)]`; omit when `false`).
  Add a struct for the WSS capability value with `supports_observe`; today
  the value is only the URI constant.
- `avatar.rs`: add `SceneAvatar.region_state`. It must be able to be
  *absent* (capability off) as well as `null`, so it needs
  `Option<Option<Map>>` or a skip rule. It is not in `SERVER_SET`.
- `event.rs`: add `SceneRegionEvent {account_id, region_id, event, data,
  extra}`, a `SceneEvent::Region` variant (the enum is `#[non_exhaustive]`,
  so this is additive) and a `REGION` const.
- `stream.rs`: add `"SceneRegionEvent"` to `SCENE_EVENT_TYPES` and
  `SceneEventType`, and add `SceneStreamEnable.observe: Option<bool>`.
- `query.rs`: add `SceneRegionFilterCondition.world_state_equals:
  Option<Map<String, Value>>`.
- New request/response types for `SceneRegion/broadcast`, mirroring
  `interact.rs`.

### `crate-jmap-scene-server`
- **Broadcast:** a new handler that mirrors `interact.rs`. It checks region
  access (`notFound`), then `may_update_region` (owner or admin, otherwise
  `forbidden`), then a new hook `accept_region_event` (rate limit and hand
  off to the transport; fail-open default `Ok(true)`). It is wired into the
  dispatch in `lib.rs` only when the capability is on.
- **`ephemeral.rs`:**
  - `replace_with` keeps `observe` and rejects `observe` with
    `regionIds: null` as `invalidArguments`.
  - `should_deliver` gets a `SceneEvent::Region` arm with no object and no
    origin, so blocked-sender suppression is skipped.
  - When `observe` is set, `has_active_avatar` is replaced by
    `may_access_region`, the region's `allowObservers`, and a new hook
    `may_observe_region` for the server's policy refusal (default
    `Ok(true)` under the fail-open posture, bd:JMAP-bghk).
  - `replace_with` becomes async (or takes a pre-computed refusal list):
    it checks each listed, accessible region for `allowObservers` and the
    policy hook, and returns a `forbidden` error carrying the refused
    `regionIds`. Inaccessible or missing regions are not refused. Today it
    only validates `eventTypes`.
- **`observerCount`:** the transport knows which connections observe which
  regions, so it needs a backend hook such as
  `set_observing(caller, account, region, bool)`. Under the region's
  state lock, the backend counts distinct users without an active avatar,
  writes `observerCount`, and advances the SceneRegion state as
  `activeAvatarCount` updates do. It must be re-counted when an observer
  enters or leaves with an avatar and when a connection closes. Setting
  `allowObservers` to `false` clears the region's observers and resets the
  count. `region.rs` create sets `observerCount: 0` and rejects a client
  value as server-set; `allowObservers` is validated as a Boolean.
  - `SceneAvatarEvent` delivery also asks the new avatar visibility hook.
- **Avatar visibility:**
  - A new hook `is_avatar_visible(caller, account, &SceneAvatar)` (default
    `true`) joins the existing `is_user_blocked` and
    `may_see_region_avatars` in `impl Visible for SceneAvatar`
    (`helpers.rs` L332-L352).
  - A `redact` override for `SceneAvatar`, backed by a new hook
    `withhold_avatar_properties` (default: nothing withheld). The
    `Visible::redact` mechanism already exists for `invitedUserIds`. The
    override has to skip the caller's own avatar and the region owner and
    admins.
  - `SceneAvatar/query` (`avatar_matches`) has to evaluate spatial filters
    after redaction; today the backend's `query_objects` matches stored
    values.
  - Per-caller `/changes` and `/queryChanges`: the default handlers report
    changes for the whole account, which stays conforming for the
    blocked-contact and E2EE cases. A deployment that hides avatars
    competitively needs a per-caller view log. This is backend work, not
    handler work; a note in `backend.rs` and a test against the memory
    backend would cover the contract.
- **`regionState` (`avatar.rs`):**
  - `update_avatar` lets the owner or an admin patch `regionState` (and
    `leftAt`). Today it allows only `EJECT_PROPERTIES = ["leftAt"]` (L48,
    L533); either widen that list or add a `REGION_STATE` path that is
    checked with `may_eject_avatar`, or a renamed `may_manage_avatar` hook.
  - A user's own patch to `regionState` returns `forbidden` unless that user
    is the owner or an admin. The create path (`enter_region`) needs the
    same check.
  - When the capability is off, `regionState` is an unknown property
    (`invalidProperties`).
- **`worldStateEquals` (`region.rs` and `query.rs`):** validate it in
  `prepare_query` (an object of scalars, otherwise `invalidArguments`) and
  match it in `region_matches`. Compare Numbers with `as_f64()` so that `1`
  equals `1.0`.

### Other crates
- `crate-jmap-scene-client`: a `SceneRegion/broadcast` call, the `observe`
  flag on stream enable, and parsing for `SceneRegionEvent`.
  `crate-jmap-chat-scene-server` (the `ChatStreamEnable "scene"` delegation)
  needs no observer support. `crate-jmap-scene-vtc-server` is unchanged,
  because `may_see_region_avatars` stays.

## Mark's answers (2026-11-20) and what revision 2 changed

1. **Governance:** edit the core drafts as an explicit exception to
   PROJECT.md rule 3 (crate-jmap decision 60). The scene-00 and
   scene-wss-00 edits are kept, and PROJECT.md is not edited.
2. **Region-event audience:** kept as drafted. A region event goes to every
   subscriber of the region, and team-scoped information stays in
   `regionState` plus Avatar Visibility.
3. **Observer control:** `SceneRegion.allowObservers` (default `false`,
   writable by the owner and admins), a `forbidden` `RequestError` for a
   refused observe, and a server-set `SceneRegion.observerCount`. The
   server keeps its right to decline observers by policy. See section 2.
4. **Timing leak:** one state string per account is kept, and the Security
   Considerations now acknowledge the leak. See section 3.
5. **`worldStateEquals` scope:** kept as drafted, on regions only.
6. **`regionState` and its own user:** the player sees all of it. One added
   sentence sends secret per-player state to a SceneObject owned by the
   region owner. See section 5.
7. **Names:** kept as drafted (`SceneRegion/broadcast`, `regionState`,
   `worldStateEquals`, `observe`).

## Final (2026-11-20)

Mark chose `allowObservers` default `true` (the owner can turn it off) instead of the drafted `false`. The patch file here is the final diff, committed as jmap-chat-spec 93f2258.
