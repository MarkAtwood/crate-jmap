//! Slack OAuth tokens.
//!
//! Oracle: Slack "Tokens" (<https://docs.slack.dev/authentication/tokens/>):
//! bot tokens begin `xoxb-`, user tokens `xoxp-`, app-level tokens (Socket
//! Mode) `xapp-`; with token rotation an access token carries an `xoxe.`
//! prefix.
//!
//! Browser session tokens (`xoxc-`, used with the `d` cookie whose value
//! begins `xoxd-`) are what the Slack web client uses for its own session.
//! They are not issued to apps, and using them from another program is a
//! Terms of Service grey area (bd JMAP-hb8j), so [`SlackToken::new`]
//! refuses them.

use std::fmt;

/// What a [`SlackToken`] was issued as.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A bot token (`xoxb-`): sees the conversations the app's bot user is
    /// a member of.
    Bot,
    /// A user token (`xoxp-`): sees what the installing user sees.
    User,
    /// An app-level token (`xapp-`): only for `apps.connections.open`
    /// (Socket Mode).
    App,
}

/// Why a string is not a usable [`SlackToken`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// A browser session token (`xoxc-`) or session cookie (`xoxd-`). This
    /// crate never uses them.
    #[error(
        "browser session tokens (xoxc-/xoxd-) are not supported; use a bot or user OAuth token"
    )]
    BrowserSession,
    /// Not a bot, user or app-level token.
    #[error("not a Slack bot (xoxb-), user (xoxp-) or app-level (xapp-) token")]
    Unrecognized,
}

/// A Slack OAuth access token, held for the `Authorization: Bearer` header.
///
/// The `Debug` impl prints the kind only, never the secret.
#[derive(Clone)]
pub struct SlackToken {
    secret: String,
    kind: TokenKind,
}

impl SlackToken {
    /// Classify `secret` by its prefix.
    ///
    /// # Errors
    ///
    /// [`TokenError::BrowserSession`] for an `xoxc-` or `xoxd-` value and
    /// [`TokenError::Unrecognized`] for anything that is not an `xoxb-`,
    /// `xoxp-` or `xapp-` token (optionally behind the rotation prefix
    /// `xoxe.`).
    pub fn new(secret: impl Into<String>) -> Result<Self, TokenError> {
        let secret = secret.into();
        let bare = secret.strip_prefix("xoxe.").unwrap_or(&secret);
        let kind = if bare.starts_with("xoxc-") || bare.starts_with("xoxd-") {
            return Err(TokenError::BrowserSession);
        } else if bare.starts_with("xoxb-") {
            TokenKind::Bot
        } else if bare.starts_with("xoxp-") {
            TokenKind::User
        } else if bare.starts_with("xapp-") {
            TokenKind::App
        } else {
            return Err(TokenError::Unrecognized);
        };
        if bare.len() <= 5 || secret.chars().any(char::is_whitespace) {
            return Err(TokenError::Unrecognized);
        }
        Ok(Self { secret, kind })
    }

    /// What the token was issued as.
    pub fn kind(&self) -> TokenKind {
        self.kind
    }

    pub(crate) fn secret(&self) -> &str {
        &self.secret
    }
}

/// Never shows the secret.
impl fmt::Debug for SlackToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlackToken")
            .field("kind", &self.kind)
            .field("secret", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy(prefix: &str) -> String {
        // Generated per run; never a real credential.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        format!("{prefix}0000-test-{nanos:09}")
    }

    #[test]
    fn kinds_from_prefix() {
        assert_eq!(
            SlackToken::new(dummy("xoxb-")).map(|t| t.kind()),
            Ok(TokenKind::Bot)
        );
        assert_eq!(
            SlackToken::new(dummy("xoxp-")).map(|t| t.kind()),
            Ok(TokenKind::User)
        );
        assert_eq!(
            SlackToken::new(dummy("xapp-")).map(|t| t.kind()),
            Ok(TokenKind::App)
        );
        assert_eq!(
            SlackToken::new(dummy("xoxe.xoxp-")).map(|t| t.kind()),
            Ok(TokenKind::User)
        );
    }

    #[test]
    fn browser_session_tokens_are_refused() {
        assert_eq!(
            SlackToken::new(dummy("xoxc-")).map(|t| t.kind()),
            Err(TokenError::BrowserSession)
        );
        assert_eq!(
            SlackToken::new(dummy("xoxd-")).map(|t| t.kind()),
            Err(TokenError::BrowserSession)
        );
    }

    #[test]
    fn other_strings_are_refused() {
        for s in ["", "xoxb-", "hunter2", "xoxb- spaced"] {
            assert!(SlackToken::new(s).is_err(), "{s:?}");
        }
    }

    /// Debug redaction canary (workspace AGENTS.md §Security testing).
    #[test]
    fn debug_never_shows_the_secret() {
        let secret = dummy("xoxb-canary-");
        let token = SlackToken::new(secret.clone()).expect("bot token");
        let shown = format!("{token:?} {token:#?}");
        assert!(!shown.contains(&secret), "{shown}");
        assert!(!shown.contains("canary"), "{shown}");
        assert!(shown.contains("Bot"));
    }
}
