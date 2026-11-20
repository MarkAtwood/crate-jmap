//! A thin Slack Web API client: one method call per function, form-encoded
//! `POST https://slack.com/api/<method>` with `Authorization: Bearer`.
//!
//! Oracles:
//! - Web API basics: every response is a JSON object with a boolean `ok`;
//!   `"ok": false` carries an `error` code
//!   (<https://docs.slack.dev/apis/web-api/>).
//! - Rate limits (<https://docs.slack.dev/apis/web-api/rate-limits/>): "HTTP
//!   429 Too Many Requests" with a `Retry-After` header giving the number of
//!   seconds to wait before retrying that method for that workspace.
//! - Pagination (<https://docs.slack.dev/apis/web-api/pagination/>):
//!   `cursor` in, `response_metadata.next_cursor` out; an empty
//!   `next_cursor` ends the collection.

use std::fmt;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, RETRY_AFTER};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::token::{SlackToken, TokenKind};

/// The production Web API base, `https://slack.com/api/`.
pub const API_BASE: &str = "https://slack.com/api/";

/// The per-request timeout of the client [`SlackClient::new`] builds.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How a [`SlackClient`] answers `429 Too Many Requests`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// How many times one call is retried after a 429 before it fails with
    /// [`SlackError::RateLimited`].
    pub max_retries: u32,
    /// The longest `Retry-After` the client sleeps out. A longer one fails
    /// the call at once with [`SlackError::RateLimited`], so a JMAP request
    /// is not held for minutes.
    pub max_wait: Duration,
}

impl Default for RetryPolicy {
    /// Three retries, waits of up to 60 s.
    fn default() -> Self {
        Self {
            max_retries: 3,
            max_wait: Duration::from_secs(60),
        }
    }
}

impl RetryPolicy {
    /// A policy with `max_retries` retries of at most `max_wait` each.
    pub const fn new(max_retries: u32, max_wait: Duration) -> Self {
        Self {
            max_retries,
            max_wait,
        }
    }
}

/// A failed Web API call.
///
/// No variant carries the token. `Display` names the method and Slack's
/// error code, never message text.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SlackError {
    /// The request did not complete (connection, TLS, timeout).
    #[error("{method}: transport error")]
    Transport {
        /// The Web API method.
        method: String,
        /// The underlying error.
        #[source]
        source: reqwest::Error,
    },
    /// Slack answered 429 and the [`RetryPolicy`] gave up.
    #[error("{method}: rate limited, retry after {retry_after:?}")]
    RateLimited {
        /// The Web API method.
        method: String,
        /// The last `Retry-After` Slack sent.
        retry_after: Duration,
    },
    /// Slack answered `"ok": false`.
    #[error("{method}: {code}")]
    Api {
        /// The Web API method.
        method: String,
        /// Slack's `error` code, such as `channel_not_found`.
        code: String,
    },
    /// An HTTP status other than 2xx or 429.
    #[error("{method}: HTTP {status}")]
    Status {
        /// The Web API method.
        method: String,
        /// The status.
        status: StatusCode,
    },
    /// The response was not the JSON the method documents.
    #[error("{method}: malformed response")]
    Decode {
        /// The Web API method.
        method: String,
    },
    /// The method needs another kind of token (for example
    /// `apps.connections.open` needs an app-level token).
    #[error("{method}: needs a {needed:?} token")]
    WrongToken {
        /// The Web API method.
        method: String,
        /// The token kind the method needs.
        needed: TokenKind,
    },
}

impl SlackError {
    /// Slack's `error` code for [`SlackError::Api`].
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api { code, .. } => Some(code),
            _ => None,
        }
    }
}

/// One page of a cursor-paginated collection.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    /// The items on this page.
    pub items: Vec<T>,
    /// The cursor of the next page; `None` on the last page.
    pub next_cursor: Option<String>,
}

/// A conversation from `conversations.list` / `conversations.info`
/// (<https://docs.slack.dev/reference/methods/conversations.list/>).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Conversation {
    /// The conversation id (`C…`, `G…`, `D…`).
    pub id: String,
    /// The channel name; absent on IMs.
    #[serde(default)]
    pub name: Option<String>,
    /// A public or private channel.
    #[serde(default)]
    pub is_channel: bool,
    /// A private channel or (legacy) group.
    #[serde(default)]
    pub is_group: bool,
    /// A direct message.
    #[serde(default)]
    pub is_im: bool,
    /// A multi-person direct message.
    #[serde(default)]
    pub is_mpim: bool,
    /// A private conversation.
    #[serde(default)]
    pub is_private: bool,
    /// Archived (read-only).
    #[serde(default)]
    pub is_archived: bool,
    /// Whether the token's user is a member; IMs omit it.
    #[serde(default)]
    pub is_member: Option<bool>,
    /// Creation time, Unix seconds.
    #[serde(default)]
    pub created: i64,
    /// IMs: the other user.
    #[serde(default)]
    pub user: Option<String>,
    /// The channel topic.
    #[serde(default)]
    pub topic: Option<TextValue>,
    /// The channel purpose.
    #[serde(default)]
    pub purpose: Option<TextValue>,
}

impl Conversation {
    /// `true` for a multi-person DM. Slack marks some as `is_group` plus an
    /// `mpdm-` name rather than `is_mpim`.
    pub fn is_group_dm(&self) -> bool {
        self.is_mpim
            || (self.is_group && self.name.as_deref().is_some_and(|n| n.starts_with("mpdm-")))
    }
}

/// A `{ "value": … }` text object (topic, purpose).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TextValue {
    /// The text.
    #[serde(default)]
    pub value: String,
}

/// A message from `conversations.history` / `conversations.replies` or an
/// Events API `message` event
/// (<https://docs.slack.dev/reference/events/message/>).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SlackMessage {
    /// The message timestamp, its id within the conversation.
    pub ts: String,
    /// The subtype (`bot_message`, `thread_broadcast`, `channel_join`, …).
    #[serde(default)]
    pub subtype: Option<String>,
    /// The sending user.
    #[serde(default)]
    pub user: Option<String>,
    /// The sending bot, for bot messages.
    #[serde(default)]
    pub bot_id: Option<String>,
    /// The display name a bot message was posted under.
    #[serde(default)]
    pub username: Option<String>,
    /// The mrkdwn text.
    #[serde(default)]
    pub text: String,
    /// The thread parent's `ts`; equal to `ts` on the parent itself.
    #[serde(default)]
    pub thread_ts: Option<String>,
    /// Set once the message was edited.
    #[serde(default)]
    pub edited: Option<Edited>,
    /// Reactions, one entry per emoji name.
    #[serde(default)]
    pub reactions: Vec<ReactionSummary>,
    /// Replies in the thread this message is the parent of.
    #[serde(default)]
    pub reply_count: Option<u64>,
}

/// The `edited` object of a message.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Edited {
    /// The editing user.
    #[serde(default)]
    pub user: Option<String>,
    /// The time of the edit.
    pub ts: String,
}

/// One emoji's reactions on a message.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReactionSummary {
    /// The emoji name, without colons.
    pub name: String,
    /// The reacting users; may be fewer than `count`.
    #[serde(default)]
    pub users: Vec<String>,
    /// How many users reacted.
    #[serde(default)]
    pub count: u64,
}

/// A user from `users.list` / `users.info`
/// (<https://docs.slack.dev/reference/methods/users.info/>).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SlackUser {
    /// The user id (`U…` or `W…`).
    pub id: String,
    /// The handle.
    #[serde(default)]
    pub name: String,
    /// A deactivated user.
    #[serde(default)]
    pub deleted: bool,
    /// A bot user.
    #[serde(default)]
    pub is_bot: bool,
    /// The full name.
    #[serde(default)]
    pub real_name: Option<String>,
    /// The profile.
    #[serde(default)]
    pub profile: Option<Profile>,
}

impl SlackUser {
    /// The best display text: profile display name, then real name, then
    /// handle, then id.
    pub fn display(&self) -> &str {
        let profile = self.profile.as_ref();
        [
            profile.and_then(|p| p.display_name.as_deref()),
            profile.and_then(|p| p.real_name.as_deref()),
            self.real_name.as_deref(),
            Some(self.name.as_str()),
        ]
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
        .unwrap_or(&self.id)
    }
}

/// A user's profile.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Profile {
    /// The display name the user chose.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The full name.
    #[serde(default)]
    pub real_name: Option<String>,
}

/// The `auth.test` answer
/// (<https://docs.slack.dev/reference/methods/auth.test/>).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AuthTest {
    /// The workspace id.
    pub team_id: String,
    /// The token's user (the bot user for a bot token).
    pub user_id: String,
    /// The workspace name.
    #[serde(default)]
    pub team: Option<String>,
    /// The workspace URL.
    #[serde(default)]
    pub url: Option<String>,
    /// The bot id, for a bot token.
    #[serde(default)]
    pub bot_id: Option<String>,
}

/// A message Slack accepted (`chat.postMessage`, `chat.update`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Posted {
    /// The conversation.
    pub channel: String,
    /// The message `ts`.
    pub ts: String,
}

/// A Slack Web API client for one token.
///
/// Cloning is cheap: clones share the connection pool.
#[derive(Clone)]
pub struct SlackClient {
    http: reqwest::Client,
    base: String,
    token: SlackToken,
    retry: RetryPolicy,
}

/// Never shows the token.
impl fmt::Debug for SlackClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlackClient")
            .field("base", &self.base)
            .field("token", &self.token)
            .field("retry", &self.retry)
            .finish()
    }
}

#[derive(Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize, Default)]
struct ResponseMetadata {
    #[serde(default)]
    next_cursor: Option<String>,
}

fn next_cursor(meta: Option<ResponseMetadata>) -> Option<String> {
    meta.and_then(|m| m.next_cursor).filter(|c| !c.is_empty())
}

impl SlackClient {
    /// A client for `token` over a reqwest client that follows no
    /// redirects (a bearer token never follows one) and times out after
    /// [`REQUEST_TIMEOUT`].
    ///
    /// # Errors
    ///
    /// The [`reqwest::Error`] when the HTTP client cannot be built.
    pub fn new(token: SlackToken) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()?;
        Ok(Self::from_client(http, token))
    }

    /// A client for `token` over `http`, as configured.
    pub fn from_client(http: reqwest::Client, token: SlackToken) -> Self {
        Self {
            http,
            base: API_BASE.to_owned(),
            token,
            retry: RetryPolicy::default(),
        }
    }

    /// This client calling `base` (which should end in `/`) instead of
    /// [`API_BASE`]; for a local test double.
    pub fn with_base_url(self, base: &Url) -> Self {
        let mut base = base.as_str().to_owned();
        if !base.ends_with('/') {
            base.push('/');
        }
        Self { base, ..self }
    }

    /// This client with `retry` instead of [`RetryPolicy::default`].
    pub fn with_retry_policy(self, retry: RetryPolicy) -> Self {
        Self { retry, ..self }
    }

    /// The token's kind.
    pub fn token_kind(&self) -> TokenKind {
        self.token.kind()
    }

    /// Call `method` with form `params`, retrying 429s per the
    /// [`RetryPolicy`], and decode the `"ok": true` body as `T`.
    ///
    /// # Errors
    ///
    /// Every [`SlackError`] variant but [`SlackError::WrongToken`].
    pub async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: &[(&str, &str)],
    ) -> Result<T, SlackError> {
        let url = format!("{}{method}", self.base);
        let mut attempt = 0;
        loop {
            let response = self
                .http
                .post(url.clone())
                .header(AUTHORIZATION, format!("Bearer {}", self.token.secret()))
                .form(params)
                .send()
                .await
                .map_err(|source| SlackError::Transport {
                    method: method.to_owned(),
                    source: source.without_url(),
                })?;
            let status = response.status();
            if status == StatusCode::TOO_MANY_REQUESTS {
                let retry_after = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map_or(Duration::from_secs(30), Duration::from_secs);
                if attempt >= self.retry.max_retries || retry_after > self.retry.max_wait {
                    return Err(SlackError::RateLimited {
                        method: method.to_owned(),
                        retry_after,
                    });
                }
                attempt += 1;
                tokio::time::sleep(retry_after).await;
                continue;
            }
            if !status.is_success() {
                return Err(SlackError::Status {
                    method: method.to_owned(),
                    status,
                });
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|source| SlackError::Transport {
                    method: method.to_owned(),
                    source: source.without_url(),
                })?;
            let envelope: Envelope =
                serde_json::from_slice(&bytes).map_err(|_| SlackError::Decode {
                    method: method.to_owned(),
                })?;
            if !envelope.ok {
                return Err(SlackError::Api {
                    method: method.to_owned(),
                    code: envelope.error.unwrap_or_else(|| "unknown_error".to_owned()),
                });
            }
            return serde_json::from_slice(&bytes).map_err(|_| SlackError::Decode {
                method: method.to_owned(),
            });
        }
    }

    /// `auth.test`: the workspace and user the token belongs to.
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn auth_test(&self) -> Result<AuthTest, SlackError> {
        self.call("auth.test", &[]).await
    }

    /// One page of `conversations.list`
    /// (<https://docs.slack.dev/reference/methods/conversations.list/>),
    /// archived conversations excluded. `types` is Slack's comma-separated
    /// list (`public_channel,private_channel,mpim,im`).
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn conversations_list(
        &self,
        types: &str,
        cursor: Option<&str>,
    ) -> Result<Page<Conversation>, SlackError> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            channels: Vec<Conversation>,
            #[serde(default)]
            response_metadata: Option<ResponseMetadata>,
        }
        let mut params = vec![
            ("types", types),
            ("exclude_archived", "true"),
            ("limit", "200"),
        ];
        if let Some(c) = cursor {
            params.push(("cursor", c));
        }
        let r: R = self.call("conversations.list", &params).await?;
        Ok(Page {
            items: r.channels,
            next_cursor: next_cursor(r.response_metadata),
        })
    }

    /// `conversations.info`
    /// (<https://docs.slack.dev/reference/methods/conversations.info/>).
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn conversations_info(&self, channel: &str) -> Result<Conversation, SlackError> {
        #[derive(Deserialize)]
        struct R {
            channel: Conversation,
        }
        let r: R = self
            .call("conversations.info", &[("channel", channel)])
            .await?;
        Ok(r.channel)
    }

    /// One page of `conversations.members`
    /// (<https://docs.slack.dev/reference/methods/conversations.members/>).
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn conversations_members(
        &self,
        channel: &str,
        cursor: Option<&str>,
    ) -> Result<Page<String>, SlackError> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            members: Vec<String>,
            #[serde(default)]
            response_metadata: Option<ResponseMetadata>,
        }
        let mut params = vec![("channel", channel), ("limit", "200")];
        if let Some(c) = cursor {
            params.push(("cursor", c));
        }
        let r: R = self.call("conversations.members", &params).await?;
        Ok(Page {
            items: r.members,
            next_cursor: next_cursor(r.response_metadata),
        })
    }

    /// One page of `conversations.history`
    /// (<https://docs.slack.dev/reference/methods/conversations.history/>),
    /// newest first. `oldest` is a `ts` lower bound.
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn conversations_history(
        &self,
        channel: &str,
        oldest: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<Page<SlackMessage>, SlackError> {
        let mut params = vec![("channel", channel), ("limit", "200")];
        if let Some(o) = oldest {
            params.push(("oldest", o));
        }
        if let Some(c) = cursor {
            params.push(("cursor", c));
        }
        self.message_page("conversations.history", &params).await
    }

    /// One page of `conversations.replies`
    /// (<https://docs.slack.dev/reference/methods/conversations.replies/>):
    /// the thread whose parent is `ts`, parent first.
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn conversations_replies(
        &self,
        channel: &str,
        ts: &str,
        cursor: Option<&str>,
    ) -> Result<Page<SlackMessage>, SlackError> {
        let mut params = vec![("channel", channel), ("ts", ts), ("limit", "200")];
        if let Some(c) = cursor {
            params.push(("cursor", c));
        }
        self.message_page("conversations.replies", &params).await
    }

    async fn message_page(
        &self,
        method: &str,
        params: &[(&str, &str)],
    ) -> Result<Page<SlackMessage>, SlackError> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            messages: Vec<SlackMessage>,
            #[serde(default)]
            response_metadata: Option<ResponseMetadata>,
        }
        let r: R = self.call(method, params).await?;
        Ok(Page {
            items: r.messages,
            next_cursor: next_cursor(r.response_metadata),
        })
    }

    /// One page of `users.list`
    /// (<https://docs.slack.dev/reference/methods/users.list/>).
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn users_list(&self, cursor: Option<&str>) -> Result<Page<SlackUser>, SlackError> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            members: Vec<SlackUser>,
            #[serde(default)]
            response_metadata: Option<ResponseMetadata>,
        }
        let mut params = vec![("limit", "200")];
        if let Some(c) = cursor {
            params.push(("cursor", c));
        }
        let r: R = self.call("users.list", &params).await?;
        Ok(Page {
            items: r.members,
            next_cursor: next_cursor(r.response_metadata),
        })
    }

    /// `users.info` (<https://docs.slack.dev/reference/methods/users.info/>).
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn users_info(&self, user: &str) -> Result<SlackUser, SlackError> {
        #[derive(Deserialize)]
        struct R {
            user: SlackUser,
        }
        let r: R = self.call("users.info", &[("user", user)]).await?;
        Ok(r.user)
    }

    /// `chat.postMessage`
    /// (<https://docs.slack.dev/reference/methods/chat.postMessage/>).
    /// `mrkdwn: false` disables Slack markup parsing of `text`.
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn chat_post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
        mrkdwn: bool,
    ) -> Result<Posted, SlackError> {
        let mut params = vec![
            ("channel", channel),
            ("text", text),
            ("mrkdwn", if mrkdwn { "true" } else { "false" }),
        ];
        if let Some(t) = thread_ts {
            params.push(("thread_ts", t));
        }
        self.call("chat.postMessage", &params).await
    }

    /// `chat.update`
    /// (<https://docs.slack.dev/reference/methods/chat.update/>).
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn chat_update(
        &self,
        channel: &str,
        ts: &str,
        text: &str,
    ) -> Result<Posted, SlackError> {
        self.call(
            "chat.update",
            &[("channel", channel), ("ts", ts), ("text", text)],
        )
        .await
    }

    /// `chat.delete`
    /// (<https://docs.slack.dev/reference/methods/chat.delete/>).
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn chat_delete(&self, channel: &str, ts: &str) -> Result<(), SlackError> {
        let _: serde_json::Value = self
            .call("chat.delete", &[("channel", channel), ("ts", ts)])
            .await?;
        Ok(())
    }

    /// `reactions.add`
    /// (<https://docs.slack.dev/reference/methods/reactions.add/>); `name`
    /// is the emoji name without colons.
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn reactions_add(
        &self,
        channel: &str,
        ts: &str,
        name: &str,
    ) -> Result<(), SlackError> {
        let _: serde_json::Value = self
            .call(
                "reactions.add",
                &[("channel", channel), ("timestamp", ts), ("name", name)],
            )
            .await?;
        Ok(())
    }

    /// `reactions.remove`
    /// (<https://docs.slack.dev/reference/methods/reactions.remove/>).
    ///
    /// # Errors
    ///
    /// As [`call`](Self::call).
    pub async fn reactions_remove(
        &self,
        channel: &str,
        ts: &str,
        name: &str,
    ) -> Result<(), SlackError> {
        let _: serde_json::Value = self
            .call(
                "reactions.remove",
                &[("channel", channel), ("timestamp", ts), ("name", name)],
            )
            .await?;
        Ok(())
    }

    /// `apps.connections.open`
    /// (<https://docs.slack.dev/reference/methods/apps.connections.open/>):
    /// the `wss://` URL of a Socket Mode connection. Needs an app-level
    /// (`xapp-`) token, so call it on a second client built with one.
    ///
    /// # Errors
    ///
    /// [`SlackError::WrongToken`] when this client's token is not an
    /// app-level token; otherwise as [`call`](Self::call).
    pub async fn apps_connections_open(&self) -> Result<Url, SlackError> {
        const METHOD: &str = "apps.connections.open";
        if self.token.kind() != TokenKind::App {
            return Err(SlackError::WrongToken {
                method: METHOD.to_owned(),
                needed: TokenKind::App,
            });
        }
        #[derive(Deserialize)]
        struct R {
            url: String,
        }
        let r: R = self.call(METHOD, &[]).await?;
        Url::parse(&r.url)
            .ok()
            .filter(|u| u.scheme() == "wss")
            .ok_or_else(|| SlackError::Decode {
                method: METHOD.to_owned(),
            })
    }
}

/// Collect every page of a cursor-paginated call.
pub(crate) async fn all_pages<T, F, Fut>(mut page: F) -> Result<Vec<T>, SlackError>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: std::future::Future<Output = Result<Page<T>, SlackError>>,
{
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let p = page(cursor.take()).await?;
        out.extend(p.items);
        match p.next_cursor {
            Some(c) => cursor = Some(c),
            None => return Ok(out),
        }
    }
}
