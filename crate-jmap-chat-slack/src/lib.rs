//! Presents one Slack workspace as a JMAP Chat Space.
//!
//! This is a support crate (workspace AGENTS.md §Support crates): it does
//! network I/O and holds Slack credentials, and no `*-types` or `*-server`
//! crate depends on it. It is scoped to a workspace's **own internal Slack
//! app** (decision 51, bd JMAP-hb8j): the deployer creates the app in their
//! workspace and supplies its bot or user OAuth token. Browser session
//! tokens are refused ([`SlackToken::new`]).
//!
//! [`SlackBridge`] implements every `jmap-chat-server` hook trait, so it is
//! a [`ChatBackend`](jmap_chat_server::ChatBackend) for
//! `register_chat_handlers`. JMAP state (ids, `/changes` logs, queries)
//! lives in an embedded [`MemoryBackend`](jmap_chat_server::memory::MemoryBackend);
//! the bridge fills it from Slack and sends JMAP writes to Slack:
//!
//! | Slack | JMAP Chat |
//! |---|---|
//! | workspace | one Space; the token's user is the account owner |
//! | public / private channel | `channel` Chat in the Space |
//! | DM (`im`) | `direct` Chat with `contactId` |
//! | group DM (`mpim`) | `group` Chat with `members` |
//! | user, bot | ChatContact (`identity` `slack:<team>:<user>`) |
//! | message | Message (`senderId` the accountId for the owner) |
//! | thread reply (`thread_ts`) | `replyTo` and `threadRootId` = the parent |
//! | reaction | `reactions/<senderReactionId>` |
//! | mrkdwn | `text/plain` or `application/jmap-chat-rich` spans |
//!
//! Modules: [`client`] (Web API calls with 429 `Retry-After` handling),
//! [`events`] (request signatures, Events API and Socket Mode envelopes),
//! [`mrkdwn`] (body conversion).
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use jmap_chat_slack::client::SlackClient;
//! use jmap_chat_slack::events::{self, HttpEvent, SigningSecret};
//! use jmap_chat_slack::{BridgeConfig, SlackBridge, SlackToken};
//! use jmap_server::Dispatcher;
//! use jmap_types::Id;
//!
//! # async fn run(token: &str, secret: &str, ts: &str, sig: &str, body: &[u8])
//! #     -> Result<(), Box<dyn std::error::Error>> {
//! let client = SlackClient::new(SlackToken::new(token)?)?;
//! let bridge = SlackBridge::connect(client, BridgeConfig::new(Id::from("a1"))).await?;
//! let report = bridge.initial_sync().await?;
//! println!("{} messages, {} skipped", report.messages, report.skipped.len());
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! jmap_chat_server::register_chat_handlers(&mut dispatcher, Arc::new(bridge.clone()));
//!
//! // In the Events API endpoint:
//! let now = std::time::SystemTime::now();
//! events::verify_signature(&SigningSecret::new(secret), ts, body, sig, now)?;
//! match events::parse_http_event(body)? {
//!     HttpEvent::UrlVerification { challenge } => { /* answer with `challenge` */ }
//!     HttpEvent::Callback { event, .. } => {
//!         bridge.apply_event(&event).await?;
//!     }
//!     _ => {}
//! }
//! # Ok(()) }
//! ```

#![forbid(unsafe_code)]

mod backend;
mod bridge;
pub mod client;
mod emoji;
pub mod events;
pub mod mrkdwn;
mod time;
mod token;

pub use backend::{MAX_BODY_BYTES, SUPPORTED_BODY_TYPES};
pub use bridge::{
    BridgeConfig, BridgeError, DEFAULT_CONVERSATION_TYPES, EventOutcome, SkippedConversation,
    SlackBridge, SyncReport,
};
pub use token::{SlackToken, TokenError, TokenKind};
