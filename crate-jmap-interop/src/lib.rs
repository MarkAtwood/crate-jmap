//! Opt-in interop tests of the `jmap-*` clients against an external JMAP
//! server (bd:JMAP-cyub).
//!
//! The tests live in `tests/`. Every test is `#[ignore]`d, so
//! `cargo test --workspace` never needs a server. To run them, start a
//! server and point the tests at it through the environment:
//!
//! ```text
//! scripts/stalwart-up.sh
//! . target/interop/stalwart.env
//! cargo test -p jmap-interop -- --ignored
//! scripts/stalwart-down.sh
//! ```
//!
//! This library holds the shared helpers: reading the server settings from
//! the environment ([`InteropEnv`]), building an authenticated
//! [`JmapClient`] for a test account, and making names unique per run.
//!
//! Tests assert protocol facts (RFC 8620, RFC 8621), never one server's
//! choices, so the same tests can run against any conforming server with
//! two Basic-auth accounts that can mail each other.

#![forbid(unsafe_code)]

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use jmap_base_client::{BasicAuth, ClientConfig, ClientError, JmapClient};

/// Environment variable with the server's base URL, for example
/// `http://127.0.0.1:18080`. The Session is fetched from
/// `<base>/.well-known/jmap` (RFC 8620 §2.2).
pub const URL_VAR: &str = "JMAP_INTEROP_URL";
/// Environment variable with the first test account's username.
pub const USER_VAR: &str = "JMAP_INTEROP_USER";
/// Environment variable with the first test account's password.
pub const PASSWORD_VAR: &str = "JMAP_INTEROP_PASSWORD";
/// Environment variable with the second test account's username.
pub const USER2_VAR: &str = "JMAP_INTEROP_USER2";
/// Environment variable with the second test account's password.
pub const PASSWORD2_VAR: &str = "JMAP_INTEROP_PASSWORD2";

/// Username and password of one test account, sent with HTTP Basic auth.
///
/// `Debug` does not print the password.
#[derive(Clone)]
pub struct Credentials {
    /// The login name, which for most servers is the account's email
    /// address.
    pub username: String,
    password: String,
}

impl Credentials {
    /// The password.
    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// A required environment variable is unset or empty while
/// [`URL_VAR`] is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingVar(pub &'static str);

impl fmt::Display for MissingVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{URL_VAR} is set but {} is not", self.0)
    }
}

impl std::error::Error for MissingVar {}

/// The external server under test and its two accounts.
#[derive(Debug, Clone)]
pub struct InteropEnv {
    /// Base URL of the server (scheme, host and port, no path).
    pub base_url: String,
    /// The first account (alice with `scripts/stalwart-up.sh`).
    pub user: Credentials,
    /// The second account (bob), the recipient of submitted mail.
    pub user2: Credentials,
}

impl InteropEnv {
    /// Read the settings from the process environment.
    ///
    /// Returns `Ok(None)` when [`URL_VAR`] is unset or empty: no server
    /// is configured.
    ///
    /// # Errors
    ///
    /// [`MissingVar`] naming the first of [`USER_VAR`], [`PASSWORD_VAR`],
    /// [`USER2_VAR`] and [`PASSWORD2_VAR`] that is unset or empty while
    /// [`URL_VAR`] is set.
    pub fn from_env() -> Result<Option<Self>, MissingVar> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// [`Self::from_env`] over any variable lookup, for tests of the
    /// parsing itself.
    ///
    /// # Errors
    ///
    /// As for [`Self::from_env`].
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Self>, MissingVar> {
        let get = |name: &'static str| lookup(name).filter(|v| !v.is_empty());
        let Some(base_url) = get(URL_VAR) else {
            return Ok(None);
        };
        let need = |name: &'static str| get(name).ok_or(MissingVar(name));
        Ok(Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            user: Credentials {
                username: need(USER_VAR)?,
                password: need(PASSWORD_VAR)?,
            },
            user2: Credentials {
                username: need(USER2_VAR)?,
                password: need(PASSWORD2_VAR)?,
            },
        }))
    }

    /// The settings from the environment, for a test that cannot run
    /// without them.
    ///
    /// # Panics
    ///
    /// When [`URL_VAR`] is unset or a credential variable is missing. The
    /// interop tests are `#[ignore]`d, so this only fires when someone
    /// asked for them with `--ignored`; the message says how to start a
    /// server.
    pub fn require() -> Self {
        match Self::from_env() {
            Ok(Some(env)) => env,
            Ok(None) => panic!(
                "{URL_VAR} is not set. Start a server with scripts/stalwart-up.sh, \
                 then `. target/interop/stalwart.env` (see docs/interop/README.md)"
            ),
            Err(e) => panic!("{e} (see docs/interop/README.md)"),
        }
    }

    /// A client for the server, authenticated as `who` with HTTP Basic
    /// auth.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidArgument`] when the username contains a
    /// colon or either credential is empty, or when [`Self::base_url`] is
    /// not an `http` or `https` URL without a path; [`ClientError::Http`]
    /// when the HTTP client cannot be built.
    pub fn client(&self, who: &Credentials) -> Result<JmapClient, ClientError> {
        let auth = BasicAuth::new(&who.username, &who.password)?;
        JmapClient::new_plain(auth, &self.base_url, ClientConfig::default())
    }

    /// Credentials with the first account's username and a wrong
    /// password, for authentication-failure tests.
    pub fn wrong_password(&self) -> Credentials {
        Credentials {
            username: self.user.username.clone(),
            password: format!("{}-wrong", self.user.password),
        }
    }
}

/// `prefix` followed by a suffix that differs on every call in every run,
/// for names of objects a test creates (Mailbox names, Email subjects) on
/// a server that keeps state across runs and tests.
pub fn unique(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{nanos:x}-{n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    const FULL: &[(&str, &str)] = &[
        (URL_VAR, "http://127.0.0.1:18080/"),
        (USER_VAR, "alice@example.org"),
        (PASSWORD_VAR, "a-secret"),
        (USER2_VAR, "bob@example.org"),
        (PASSWORD2_VAR, "b-secret"),
    ];

    #[test]
    fn unset_url_means_no_server() {
        assert_eq!(
            InteropEnv::from_lookup(lookup(&[])).map(|e| e.is_none()),
            Ok(true)
        );
        assert_eq!(
            InteropEnv::from_lookup(lookup(&[(URL_VAR, "")])).map(|e| e.is_none()),
            Ok(true)
        );
    }

    #[test]
    fn full_settings_parse_and_trim_trailing_slash() {
        let env = InteropEnv::from_lookup(lookup(FULL))
            .expect("all variables set")
            .expect("URL set");
        assert_eq!(env.base_url, "http://127.0.0.1:18080");
        assert_eq!(env.user.username, "alice@example.org");
        assert_eq!(env.user.password(), "a-secret");
        assert_eq!(env.user2.username, "bob@example.org");
        assert_eq!(env.user2.password(), "b-secret");
        assert_ne!(env.wrong_password().password(), env.user.password());
        env.client(&env.user).expect("client builds");
    }

    #[test]
    fn missing_credential_is_named() {
        let partial: Vec<(&str, &str)> = FULL
            .iter()
            .copied()
            .filter(|(k, _)| *k != PASSWORD2_VAR)
            .collect();
        assert_eq!(
            InteropEnv::from_lookup(lookup(&partial)).map(|_| ()),
            Err(MissingVar(PASSWORD2_VAR))
        );
    }

    #[test]
    fn debug_hides_password() {
        let env = InteropEnv::from_lookup(lookup(FULL)).unwrap().unwrap();
        let shown = format!("{env:?}");
        assert!(!shown.contains("a-secret") && !shown.contains("b-secret"));
    }

    #[test]
    fn unique_names_differ() {
        assert_ne!(unique("x"), unique("x"));
        assert!(unique("box").starts_with("box-"));
    }
}
