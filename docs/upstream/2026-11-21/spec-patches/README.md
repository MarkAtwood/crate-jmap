# Spec patch: WSS and Scene spec gaps (bd JMAP-h05q, decision 66)

`wss-scene-gaps.patch` is a unified diff against jmap-chat-spec at
`813ad11e4dc201f1109bce53b4b73435f122204e` ("mimi: F12 Message/query needs
anchorOffset 1"). Apply it from the root of the jmap-chat-spec repository
with `git apply wss-scene-gaps.patch`. It is a draft for Mark's review and
has not been committed to the spec repository.

It touches four drafts (chat-wss-00, vtc-wss-00, scene-00, scene-wss-00)
and two guides that would otherwise contradict them
(`jmap-chat-wss-guide.md`, `jmap-scene-wss-guide.md`). Item 6 of the bead
is handled in code (JMAP-24aw) and is not in the patch. Line numbers below
refer to the spec HEAD above; code locations are crate-jmap at `94e4963f`.

No new BCP 14 keyword is added. Each change is plain prose that fixes an
open point, or narrows or qualifies an existing SHOULD or MAY.

## Mark's answers

### 1. chat-wss: `typing: false` is exempt from the typing rate limit

`draft-atwood-jmap-chat-wss-00.md:255`:

> no more than one `ChatTypingEvent` per sender per chat per 3 seconds
> SHOULD be delivered; events above this rate MAY be silently dropped.

Added: "A `ChatTypingEvent` with `typing: false` is exempt from this limit:
the server does not drop it for rate and does not count it, so that a
recipient does not go on showing a sender who has stopped typing."

Guide: `jmap-chat-wss-guide.md:266` and `:367-376` repeated the
undifferentiated limit. They now say that a `typing: false` is never dropped
for rate.

**Code differs (expected):** `crate-jmap-chat-server/src/websocket.rs:136`
documents "Dropped, including a `typing: false` inside the window", and
`forward` (`:431-441`) offers every typing event to the limiter keyed
`(accountId, chatId, senderId)`. Decision 66 says chat-server implements
this after the spec change.

**Gap left open (outside the four drafts):** the exemption is outbound
only. `draft-atwood-jmap-chat-00.md:1207` still allows the inbound
`Chat/typing` limit to discard calls ("calls that exceed this rate MAY be
silently discarded without error"), and that includes a `typing: false`.
The federation `Peer/typing` limit (`draft-atwood-jmap-chat-federation-00.md:789`)
does too. A stop sent within 3 s of a start can therefore still be lost
before it reaches the chat-wss fan-out. chat-wss:255 still calls its limit
"consistent with" those two. If the exemption should hold end to end, the
same sentence belongs in chat-00 and chat-federation-00 (new bead).

### 2. Outbound limits count per recipient connection; Scene interaction stays per sending user

- chat-wss:255 (typing) and :302 (presence): "counting the events delivered
  on each recipient connection ... SHOULD be delivered on a connection".
- vtc-wss: new paragraph after :334: the `VTCMediaStateEvent` and
  `VTCActiveSpeakerEvent` limits count per recipient connection. The ring
  limits (:331-332) are JMAP-VTC's per-caller and per-callee limits and are
  left alone.
- scene-wss:806-807: the `SceneObjectEvent` update limit gets "on each
  recipient connection". The rest of that list item is rewrapped, with no
  change of wording. :813-815, `SceneInteractionEvent`: "counted per sending
  user over all of that user's connections rather than per recipient".

Code matches. All three limiters are created inside each connection's
`forward` task, so they count per connection (`limit.rs:6-7`: "Each
connection's forwarding task owns its limiters, so a limit counts the events
delivered to one connection"):

- chat `crate-jmap-chat-server/src/websocket.rs:416-419`
- vtc `crate-jmap-vtc-server/src/websocket.rs:454-465`
- scene `crate-jmap-scene-server/src/websocket.rs:578-583`

The interaction limiter lives in the shared `SceneEventHub`
(`crate-jmap-scene-server/src/websocket.rs:108-112, 120, 168-176`), keyed
`(accountId, regionId, userId)`, and is checked before the frame is
published (`:509-516`).

Guide: `jmap-scene-wss-guide.md:616-617` ("Both are per outbound
connection") and `:629` (key `(connectionId, regionId, userId)`)
contradicted the per-user interaction limit. :111-112 listed the
interaction limit as a per-recipient drop. All three now say the limit is
per sending user. `jmap-chat-wss-guide.md:368-371` already said per outbound
connection.

Note: the hub's limit covers WebSocket frames only. `SceneObject/interact`
applies its own limit through `SceneBackend::accept_interaction`. The draft
does not say whether the two paths share one budget, and the patch does not
either.

### 3. vtc-wss: who receives `VTCRingEvent` and `VTCCallEndEvent`

New paragraph after vtc-wss:312 (which only says per-user events "are
delivered based on the authenticated user's identity"): neither event names
its recipient, so the server addresses it. A `VTCRingEvent` goes only to the
connections of the ringed user, the `userId` of the target's VTCParticipant
(vtc-00:379). A `VTCCallEndEvent` goes only to the connections of users with
a VTCParticipant record in the call, "as {{JMAP-VTC}} directs for each
`endReason`". That last clause keeps vtc-00:1033 and :1881, under which
`answered_elsewhere` goes only to the answering user's other devices. No
wire field is added.

Code: `VtcEventHub::publish_to_user`
(`crate-jmap-vtc-server/src/websocket.rs:126-135`) addresses an event to one
user. `Shared::admits` (`:255-262`) delivers an addressed event only when
`caller_user_id` for the hosting account equals that user, and drops a
per-user event that was published unaddressed. Which users to address is
the deployment's choice, made where it calls `publish_to_user` (doc,
`:126-129`: "a ring to each callee, a call end to each user it ends the ring
or call for"). crate-jmap has no call site outside tests
(`tests/websocket_e2e.rs:268-270`), so nothing in crate-jmap yet checks the
participants-only rule for `VTCCallEndEvent`.

## Written as crate-jmap implements them

### 4. chat-wss: event `accountId` is the recipient's account

chat-wss:233-234 and :270-271 said "The id of the account that contains
the Chat / ChatContact". Each owner has their own Chat record (chat-00:1205,
"each local participant"), so the patch says "the recipient's account". The
server sends a separate event for each recipient account, with `chatId` and
`senderId` (or `contactId`) as they are in that account, and delivers it
only on connections of users who have access to that account.

Code: `ChatEventHub` docs (`crate-jmap-chat-server/src/websocket.rs:81-85`):
"Publish one event per recipient account, with that account as
`accountId` ... A connection takes the events whose `accountId` is one of
its accounts". The check is at `:212` and `:225`, and
`should_deliver_typing` (`src/ephemeral.rs:205-218`) requires
`event.account_id == account_id`. The fan-out itself is the deployment's.
`handle_chat_typing` (`src/chat.rs:2064`) hands an `OutboundEvent::Typing`
to the backend, and no crate-jmap code publishes typing events to the hub.

### 5. scene-wss: what a `SceneInteractionEvent` frame carries, and malformed frames

RFC 8887 §4.3.1: "A server MUST return an appropriate JSON Problem Details
object (Section 4.3.4) for any request-level errors (e.g., an invalid JMAP
object ...)". A malformed frame is an invalid JMAP object, so dropping it
silently is an exception to 4.3.1 and has to be stated as one. scene-wss
already updates RFC 8887 (:105-125). The patch adds one paragraph there: the
server returns no `RequestError` for a message whose `@type` is
`"SceneInteractionEvent"`, even one whose other members are malformed. A
message that is not a JSON object, or that exceeds `maxSizeRequest`, is
handled as RFC 8887 specifies. The xref points at
`{{scene-interaction-event}}` (Section 4.3), not
`{{interaction-submission}}`, because that section renders as "Section
4.3.1" right next to "RFC 8887 Section 4.3.1".

scene-wss:422, "A frame carries the properties defined above", now
continues: the server relays only those properties, so any other member,
an `id` included, is not relayed, and an absent `data` is read as `null`.
scene-wss:432-433 ("If any check fails, the server discards the frame")
now also covers a malformed frame and cites the update section.

Code:
- `crate-jmap-scene-server/src/websocket.rs:495-517`: a frame that does not
  deserialize returns silently (`:496-498`). `frame.extra.clear()` (`:502`)
  drops every member that is not a defined property, `id` included.
  `interact` always returns `Ok(())` (`:538-541`), so no `RequestError` is
  sent.
- `SceneInteractionEvent.data` is a plain `Option`
  (`crate-jmap-scene-types/src/event.rs:150`), so serde reads an absent
  `data` as `None`. `userId` is `#[serde(default)]` (`:141`).
- The size and JSON exceptions match
  `crate-jmap-websocket/src/server/mod.rs:306-323`. `maxSizeRequest` gives
  `limit`, text that is not JSON gives `notJSON`, and a value that is not an
  object gives `notRequest`. All three happen before `@type` is read.

### 7. Credential expiry: what the existing SHOULD NOT requires

chat-wss:134, vtc-wss:631-634 and scene-wss:1318-1321 say "A server that
does not close the connection ... SHOULD NOT deliver further ephemeral
events on that connection". Each now adds: "Keeping the connection open
therefore requires a check of the connection's credentials before each
ephemeral event is delivered; a server that does not make that check closes
the connection when credentials expire." This adds no keyword. It states
what the existing SHOULD NOT implies.

Code: the three stream handlers cannot see expiry and leave it to the
deployment (`crate-jmap-chat-server/src/websocket.rs:21-25`,
`crate-jmap-vtc-server/src/websocket.rs:29-32`,
`crate-jmap-scene-server/src/websocket.rs:51-53`).

**Code gap:** `crate-jmap-websocket` has no way to close a running
`Connection` with 1008. `Connection::run` (`server/mod.rs:731`) ends only on
a client Close, the end of `incoming`, or a binary frame (1003). A
deployment can end `incoming`, but `run` then returns without sending a
Close frame. The only other route is a custom `Sink` that injects
`Outbound::Close { code: 1008, .. }`. The drafts' "SHOULD use ... 1008" is
therefore not reachable with the provided `tungstenite::serve`. Worth a
crate-jmap bead (for example, a close or shutdown handle on `Connection`).

### 8. scene-wss: which accounts an observer subscription's checks use

scene-wss:612-615 says an id "matches that id in every account the
authenticated user can access". Added: when an observer subscription is
enabled, the server makes its checks (the `forbidden` refusal) "in the
accounts it chooses from those the user can access, typically the user's
own account; delivery is not limited to those accounts."

Code: `SceneBackend::observer_accounts`
(`crate-jmap-scene-server/src/backend.rs:868-891`) defaults to the
connection's own account, and the memory backend returns every registered
account (`memory.rs:82`). It is used only at enable
(`websocket.rs:468-475`). Delivery (`ephemeral.rs:275-321`) checks the
event's hosting account, access, `filter.region_ids().covers` and
`observable()` without consulting `observer_accounts`. That is why the text
says delivery is not limited to those accounts.

**Code differs from item 11 here:** only regions found at enable time are
passed to `set_observing` (`websocket.rs:462-489`). Suppose the user
observes a region through a listed id that was not counted: it lives in an
account outside `observer_accounts`, or the user had no access at enable
and gains it later. Events are then delivered, but the user is not counted
in that region's `observerCount`. The patch does not try to describe this.
It is a crate-jmap fix (recompute the observed set at delivery, or
re-check on access changes).

### 9. `allowObservers` false to true resumes delivery

scene-wss:641-643 said events "stop" when `allowObservers` goes `false` or
the user loses access. Now: the entry stays in the subscription, and events
resume without a new `SceneStreamEnable` once the region admits the
observer again. A policy decline is added to the list (item 10). The same
condition is added at :738-740 (Subscription Scope) and in the
delivery-time authorization list at :1299-1302. scene-00:439
(`allowObservers`) adds "setting it back to `true` resumes delivery to the
observer subscriptions that still list the region".

Code: `observable()` (`crate-jmap-scene-server/src/ephemeral.rs:421`)
reads `allow_observers` per event. The subscription is not changed when the
flag flips, and the `(account, region)` pair stays in `Shared::observing`.

Guide: `jmap-scene-wss-guide.md:78-79` now mentions the policy decline and
the resume.

### 10. A policy decline applies at enable and at delivery

scene-wss:627-629 (the "MAY also decline observers" sentence) adds: "Like
`allowObservers`, a decline applies both when the subscription is enabled
and when each event is delivered."

Code: `may_observe_region` is called at enable (`observer_regions`,
`ephemeral.rs:449`) and per event in `observable()` (`ephemeral.rs:421`).
The doc at `backend.rs:849-853` says "and again at delivery time by
`should_deliver`".

### 11. When `observerCount` is recomputed

scene-wss:645-650 said only when the count drops. It now says the server
recomputes the count when a subscription starts or stops covering the
region (`SceneStreamEnable`, `SceneStreamDisable`, connection close), when
an avatar enters or leaves, and when the region is updated (for example
`allowObservers` or the access policy). While the region does not admit
observers, the count is `0`.

Code: memory backend docs (`crate-jmap-scene-server/src/memory.rs:84-90`)
and `recount` (`:479`), called from `set_observing` (`:1913`), avatar entry
(`:550`) and region and avatar updates (`:1615-1625`). `observer_count`
(`:607-620`) returns 0 while `allowObservers` is `false` or the region is
declined, and otherwise counts users who have access and no active avatar.
The trait contract is at `backend.rs:904-913`.

**Code differs, minor:** a recount happens only on those events. A
decline-policy change (`decline_observers`, a test hook) or an access
change that is not a region update (for example a Space membership change
for an `accessPolicy: "space"` region) leaves the stored count stale until
the next trigger. The draft's "for example" list does not promise more.

### 12. The owner and admin SHOULD NOT is deployment policy

scene-00:2765 ("The server SHOULD NOT hide avatars ... from the region
owner and administrators") adds: "This recommendation applies to the
deployment's rule; no protocol mechanism exempts the region owner and
administrators from it, so a rule that hides an avatar from them hides it
as it would from any other caller."

Code: `SceneBackend::avatar_view` (`backend.rs:773-777`): "The hook 'SHOULD
NOT hide avatars ... from the region owner and administrators', which is
the implementation's to honor." The handlers never exempt owners or admins.

### 13. Empty broadcast `event` is `invalidProperties`

In the scene-00:1604 SetError table, the `invalidProperties` row adds
"`event` is the empty string".

Code: `crate-jmap-scene-server/src/broadcast.rs:146-176`. `parse` rejects
`event` unless it is a non-empty string ("An empty `event` is refused too,
since it names no event").

### 14. Absent `data` on `SceneRegionEvent` reads as `null`

scene-wss:462-465 adds "An absent `data` is equivalent to `null`." The same
rule is added for interaction frames in item 5.

Code: `SceneRegionEvent.data`
(`crate-jmap-scene-types/src/event.rs:209`) is a plain `Option`, so serde
gives `None` when the field is absent. `SceneRegionBroadcast.data` has
`#[serde(default)]` (`broadcast.rs:31`), and the broadcast handler accepts
an absent `data` (`broadcast.rs:162`).

## Render check

Both copies were made with `git archive` of HEAD plus `.refcache`, in a
scratch directory that was deleted afterwards. Each touched draft was
rendered with `kdrfc -3` (kramdown-rfc 1.7.43, plus xml2rfc from
`/data/ietf-tools`), and `idnits --offline` was run on the XML, once before
the patch and once after.

- kdrfc and xml2rfc: exit 0 both times. The only output was "Created file".
  No warnings.
- idnits: exit 0 both times, no errors. Counts by type were identical
  before and after except `DEPRECATED_ELEMENT` (the `<spanx>` that
  kramdown-rfc emits for inline code), which rose by 2 in chat-wss (83 to
  85), vtc-wss (84 to 86) and scene-wss (166 to 168). That class of warning
  already existed. The count goes up because new paragraphs contain code
  spans. scene-00 did not change (675). `MISSING_BCP14_TAGS` did not change
  in any draft, which confirms that no keyword was added.
- `git apply --check` of the patch against a clean HEAD export: applies.
