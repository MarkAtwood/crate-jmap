# jmap-chat-slack

Presents one Slack workspace as a JMAP Chat Space. `SlackBridge` is a
`jmap-chat-server` `ChatBackend`: it mirrors the workspace into an embedded
in-memory JMAP store through the Slack Web API and Events API, and sends
JMAP writes (Message create, edit, delete, reactions) back to Slack.

## What it is

A support crate (decision 51, bd JMAP-hb8j). It does network I/O and holds
a Slack token; no `*-types` or `*-server` crate depends on it.

| Item | What it does |
|---|---|
| `SlackBridge::connect` | `auth.test`, then the account and its Space; the token's user is the account owner |
| `SlackBridge::initial_sync` | `users.list`, `conversations.list`, `conversations.members`, `conversations.history`, `conversations.replies`, all paginated, into ChatContacts, Chats and Messages; rerunning reconciles |
| `SlackBridge::apply_event` | one Events API `event` into the store (table below) |
| `SlackBridge` hook-trait impls | the `ChatBackend` the handlers call; writes go to Slack first |
| `client::SlackClient` | the Web API methods above plus `users.info`, `conversations.info`, `chat.postMessage`, `chat.update`, `chat.delete`, `reactions.add`, `reactions.remove`, `apps.connections.open`; 429 `Retry-After` handling (`RetryPolicy`) |
| `events` | `verify_signature` (v0 HMAC-SHA256, five-minute window), `parse_http_event` (`url_verification`, `event_callback`, `app_rate_limited`), `parse_socket_frame` / `ack` (Socket Mode) |
| `mrkdwn` | `to_jmap` (Slack text → `text/plain` or `application/jmap-chat-rich`, with `mentions` / `broadcastMentions`), `from_jmap` (the reverse), `escape` / `unescape` |
| `SlackToken` | bot (`xoxb-`), user (`xoxp-`) or app-level (`xapp-`) token; refuses browser session tokens (`xoxc-` / `xoxd-`); redacted `Debug` |

### Mapping

| Slack | JMAP Chat (draft-atwood-jmap-chat-00) |
|---|---|
| workspace | one Space (`name` from `auth.test` `team`); `members` the owner and every active user; `uncategorizedChannelIds` the channels |
| the token's user | the account owner: `senderId` / reaction `senderId` / member id = accountId; `ownerUserId` `slack:<team>:<user>` |
| user, bot | ChatContact; `identity` `slack:<team>:<id>`, `login` the display name (`profile.display_name`, then real name, then handle) |
| public or private channel | `channel` Chat in the Space (`name`, `topic`) |
| DM (`im`) | `direct` Chat, `contactId` the other user |
| group DM (`mpim`) | `group` Chat, `members` from `conversations.members`, `name` the other members' names |
| message `ts` | Message (`sentAt` = `receivedAt` = the `ts` time); ids are minted ULIDs, oldest first |
| `thread_ts` reply | `replyTo` and `threadRootId` = the parent (Slack threads are one level); `supportsThreads: true` |
| `edited` | `editedAt`; no `editHistory` |
| deleted message | tombstone: `deletedAt`, `deletedForAll: true`, empty `body` |
| reaction | `reactions/<senderReactionId>`; `emoji` is Unicode for common names (`thumbsup` → 👍), `:name:` otherwise |
| `*b*`, `_i_`, `*_bi_*`, `` `c` ``, ```` ``` ```` blocks, `>` quotes | `bold`, `italic`, `bold-italic`, `code`, `codeblock`, `blockquote` spans |
| `<@U…>` | `mention` span and a `mentions` entry |
| `<!here>`, `<!channel>`, `<!everyone>` | `broadcast` span and `broadcastMentions` `here` / `everyone` |
| `<url\|text>` | `link` span (never a `jmap:` URI) |

| Event | Effect |
|---|---|
| `message` (plain, `bot_message`, `thread_broadcast`, `me_message`, `file_share`) | Message created; a known `ts` is reconciled |
| `message` / `message_changed` | body, mentions, `editedAt`, reactions reconciled |
| `message` / `message_deleted` | tombstone |
| `reaction_added`, `reaction_removed` | reaction added / removed |
| `channel_created` | channel Chat, listed in the Space |
| `channel_rename`, `group_rename` | Chat `name` |
| `channel_archive` / `_unarchive` (and `group_…`) | Message creates refused / allowed |
| `channel_deleted`, `group_deleted` | Chat and its Messages destroyed |
| `member_joined_channel` | the owner: conversation imported with history; others: ChatContact, Space member, group `members` |
| `member_left_channel` | the owner: Chat and its Messages destroyed; others: group `members` |
| `team_join`, `user_change` | ChatContact created / `login` refreshed |

### What does not map

- Huddles, canvases, lists, workflows, Block Kit `blocks`, legacy
  `attachments`, files and their uploads, pins, bookmarks, scheduled
  messages, channel sections, user groups, custom statuses, presence,
  read cursors (`conversations.mark`), Slack Connect, slash commands.
- Strikethrough (`~x~`) arrives as plain text; channel links, user groups
  and `<!date…>` arrive as their display text.
- Message create with `attachments`, `actions`, `senderExpiresAt`,
  `burnOnRead`, `expiresAfterReadSeconds` or `topicId`, or a `bodyType`
  other than `text/plain` and `application/jmap-chat-rich`, is
  `invalidProperties` naming the field. The `admins` broadcast scope has no
  Slack token and is dropped.
- Chat, Space, SpaceInvite, SpaceBan, ChatBan and CustomEmoji creation;
  Space/set updates; Chat destroy; ChatBan and kicks: `forbidden`. Chat/set
  updates are limited to local preferences (`muted`, `muteUntil`,
  `receiveTypingIndicators`, `receiptSharing`, `messageExpirySeconds`,
  `metadata`).
- ChatContact, ReadPosition and PresenceStatus writes, and local-only
  Message updates (`readAt`, a `deletedAt` without `deletedForAll`), stay
  in the store and are not sent.

### How failures surface

A JMAP write calls Slack first and stores nothing unless Slack accepts it.
A refusal becomes the entry's SetError:

| Slack | SetError |
|---|---|
| 429 past the `RetryPolicy` (retries or `max_wait`), `ratelimited` | `rateLimit`, with `serverRetryAfter` |
| `not_in_channel`, `channel_not_found`, `is_archived`, `restricted_action*`, `cant_update_message`, `cant_delete_message`, `edit_window_closed`, `missing_scope`, `not_allowed_token_type`, `invalid_auth`, `token_revoked`, … | `forbidden` |
| `message_not_found`, `thread_not_found` | `notFound` |
| `msg_too_long`, `too_many_attachments` | `tooLarge` |
| `too_many_reactions` | `overQuota` |
| `no_text` | `invalidProperties` `body` |
| transport error, 5xx, an unknown code | `serverFail` (the backend's `Other`) |

`already_reacted` on add, `no_reaction` on remove and `message_not_found`
on delete count as success. Sync and event failures are `BridgeError`s;
`initial_sync` lists conversations Slack will not read in
`SyncReport::skipped` and goes on.

## What it's for

A deployment that wants JMAP Chat clients on a Slack workspace it runs:
build the bridge, register it with `jmap_chat_server::register_chat_handlers`
and feed it Events API deliveries. Sibling support crates are
`jmap-platform-push` and `jmap-web-push`; the backend contract is
`jmap-chat-server`'s hook traits (docs/design/chat-backend-split.md).

## How to use

```toml
[dependencies]
jmap-chat-slack = "0.5"
```

```rust,no_run
use std::sync::Arc;
use jmap_chat_slack::client::SlackClient;
use jmap_chat_slack::events::{self, HttpEvent, SigningSecret};
use jmap_chat_slack::{BridgeConfig, SlackBridge, SlackToken};
use jmap_server::Dispatcher;
use jmap_types::Id;

# async fn run(token: &str, secret: &str, ts: &str, sig: &str, body: &[u8])
#     -> Result<(), Box<dyn std::error::Error>> {
let client = SlackClient::new(SlackToken::new(token)?)?;
let bridge = SlackBridge::connect(client, BridgeConfig::new(Id::from("a1"))).await?;
let report = bridge.initial_sync().await?;
println!("{} messages, {} skipped", report.messages, report.skipped.len());

let mut dispatcher: Dispatcher<()> = Dispatcher::new();
jmap_chat_server::register_chat_handlers(&mut dispatcher, Arc::new(bridge.clone()));

// In the Events API endpoint:
let now = std::time::SystemTime::now();
events::verify_signature(&SigningSecret::new(secret), ts, body, sig, now)?;
match events::parse_http_event(body)? {
    HttpEvent::UrlVerification { challenge } => { /* answer with `challenge` */ }
    HttpEvent::Callback { event, .. } => {
        bridge.apply_event(&event).await?;
    }
    _ => {}
}
# Ok(()) }
```

With Socket Mode, open the WebSocket to the URL from
`apps_connections_open` on a client built with the app-level token, send
`events::ack(envelope_id)` for each `SocketFrame::Event`, then pass the
payload's event to `apply_event`.

## How it works

- `SlackBridge` embeds `jmap_chat_server::memory::MemoryBackend` for JMAP
  state, ids, state strings and change logs, and keeps the Slack ↔ JMAP id
  mapping (conversation ↔ Chat, `(channel, ts)` ↔ Message, user ↔
  ChatContact, `(message, user, emoji)` ↔ `senderReactionId`) beside it.
- It implements `ChatStore` (the write path), `ChatSpaces`,
  `ChatRestrictions`, `ChatModeration` and `ChatSettings` itself, and
  forwards `JmapReads`, `SpacePolicy`, `ChatAccessPolicy`,
  `ChatMessageLifecycle`, `ChatEvents` and `ChatResolvers` with the
  `Delegate*` traits. `CallerCtx` is `()`, as for `MemoryBackend`.
- One async mutex guards the mapping. JMAP writes, events and the initial
  sync take it in turn, which also orders the echo of a write after the
  write: the echoed `message`, `message_changed` (same text) and
  `reaction_added` events come back `Unchanged`.
- A body changes on a `message_changed` only when Slack reports a new
  `edited.ts` and text other than what the bridge sent, so unfurls and
  reply-count updates do not touch it.
- `SlackClient` posts every method form-encoded with
  `Authorization: Bearer`, follows no redirects, times out after 30 s,
  sleeps out a 429's `Retry-After` up to `RetryPolicy::max_wait` and
  `max_retries` times, and never puts the token in an error or `Debug`.
- `#[non_exhaustive]` on every public struct and enum; no `unsafe`.

## Gotchas

- **Scope: a workspace's own internal app.** The deployer creates a Slack
  app in their own workspace and supplies its bot or user OAuth token.
  Internal customer-built apps keep the Tier 3 limits (50+ requests a
  minute, `limit` up to 1,000) on `conversations.history` and
  `conversations.replies`. Slack's 2025-05-29 change cut those two methods
  to 1 request a minute and 15 objects per request for apps commercially
  distributed outside the Slack Marketplace (from 2025-05-29 for new apps
  and installs, 2025-06-30 for the terms of older apps); a backfill at that
  rate is impractical, so distributing this bridge as a commercial
  non-Marketplace app is out of scope. See
  <https://docs.slack.dev/changelog/2025/05/29/rate-limit-changes-for-non-marketplace-apps>.
- Browser session tokens (`xoxc-`, `xoxd-`) are refused: they are the web
  client's own session, not issued to apps.
- A bot token sees only the conversations the bot is a member of; a user
  token sees what that user sees. Conversations the token cannot read are
  in `SyncReport::skipped` (`not_in_channel`, `missing_scope`). Workspace
  admin approval may be needed to install the app. Scopes used:
  `channels:history`, `groups:history`, `im:history`, `mpim:history`,
  the matching `:read` scopes, `users:read`, `chat:write`,
  `reactions:read`, `reactions:write`.
- The store is in memory: a restart means a new `connect` and a full
  `initial_sync`, and new JMAP ids. Clients must resync.
- `initial_sync` holds the mapping for its whole run, so JMAP writes wait
  for it. A large workspace takes many Tier 2/3 calls; `history_oldest`
  bounds the backfill.
- Reaction `sentAt` from history is the message time (Slack keeps no
  reaction time); from events it is `event_ts`. Slack lists a limited
  number of users per emoji; when the list is short of `count`, the
  bridge keeps the reactions it already has for that emoji.
- A write that needs several Slack calls (an edit plus reactions in one
  update) is not atomic on the Slack side: if a later call fails, the
  earlier change stays in Slack and the entry fails; the next event or
  resync brings the store back in line.
- Unknown mentioned users are looked up with `users.info`; a failed
  lookup leaves the mention as text.
- Events can arrive twice (Slack retries deliveries); applying one again
  is `Unchanged`. Events are not ordered with history: run
  `initial_sync` again after `app_rate_limited` or a long disconnect.
- Only the delay-seconds form of `Retry-After` is read; without one the
  client waits 30 s.

## References

- [draft-atwood-jmap-chat-00] — JMAP Chat (§Space, §Chat, §Message,
  §Reaction, §Rich Body Format, §Thread Model)
- [RFC 8620] §5.3 — SetError types
- Slack Web API: [rate limits], [pagination], [auth.test],
  [conversations.list], [conversations.info], [conversations.members],
  [conversations.history], [conversations.replies], [users.list],
  [users.info], [chat.postMessage], [chat.update], [chat.delete],
  [reactions.add], [reactions.remove], [apps.connections.open]
- Slack Events API: [Events API], [url_verification], [message],
  [message_changed], [message_deleted], [reaction_added],
  [reaction_removed], [channel_created], [channel_rename],
  [channel_archive], [channel_deleted], [member_joined_channel],
  [member_left_channel], [Socket Mode], [verifying requests]
- Slack: [formatting message text], [tokens], [2025-05-29 rate-limit change]

[draft-atwood-jmap-chat-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[rate limits]: https://docs.slack.dev/apis/web-api/rate-limits/
[pagination]: https://docs.slack.dev/apis/web-api/pagination/
[auth.test]: https://docs.slack.dev/reference/methods/auth.test/
[conversations.list]: https://docs.slack.dev/reference/methods/conversations.list/
[conversations.info]: https://docs.slack.dev/reference/methods/conversations.info/
[conversations.members]: https://docs.slack.dev/reference/methods/conversations.members/
[conversations.history]: https://docs.slack.dev/reference/methods/conversations.history/
[conversations.replies]: https://docs.slack.dev/reference/methods/conversations.replies/
[users.list]: https://docs.slack.dev/reference/methods/users.list/
[users.info]: https://docs.slack.dev/reference/methods/users.info/
[chat.postMessage]: https://docs.slack.dev/reference/methods/chat.postMessage/
[chat.update]: https://docs.slack.dev/reference/methods/chat.update/
[chat.delete]: https://docs.slack.dev/reference/methods/chat.delete/
[reactions.add]: https://docs.slack.dev/reference/methods/reactions.add/
[reactions.remove]: https://docs.slack.dev/reference/methods/reactions.remove/
[apps.connections.open]: https://docs.slack.dev/reference/methods/apps.connections.open/
[Events API]: https://docs.slack.dev/apis/events-api/
[url_verification]: https://docs.slack.dev/reference/events/url_verification/
[message]: https://docs.slack.dev/reference/events/message/
[message_changed]: https://docs.slack.dev/reference/events/message/message_changed/
[message_deleted]: https://docs.slack.dev/reference/events/message/message_deleted/
[reaction_added]: https://docs.slack.dev/reference/events/reaction_added/
[reaction_removed]: https://docs.slack.dev/reference/events/reaction_removed/
[channel_created]: https://docs.slack.dev/reference/events/channel_created/
[channel_rename]: https://docs.slack.dev/reference/events/channel_rename/
[channel_archive]: https://docs.slack.dev/reference/events/channel_archive/
[channel_deleted]: https://docs.slack.dev/reference/events/channel_deleted/
[member_joined_channel]: https://docs.slack.dev/reference/events/member_joined_channel/
[member_left_channel]: https://docs.slack.dev/reference/events/member_left_channel/
[Socket Mode]: https://docs.slack.dev/apis/events-api/using-socket-mode/
[verifying requests]: https://docs.slack.dev/authentication/verifying-requests-from-slack/
[formatting message text]: https://docs.slack.dev/messaging/formatting-message-text/
[tokens]: https://docs.slack.dev/authentication/tokens/
[2025-05-29 rate-limit change]: https://docs.slack.dev/changelog/2025/05/29/rate-limit-changes-for-non-marketplace-apps
