//! JMAP Scene client support (draft-atwood-jmap-scene-00).
//!
//! [`JmapSceneExt`] binds the `/get`, `/changes`, `/set`, `/query` and
//! `/queryChanges` methods of `SceneAsset`, `SceneRegion`, `SceneObject`
//! and `SceneAvatar` (draft-atwood-jmap-scene-00 §Methods) on top of
//! `jmap-base-client`. Wire types come from `jmap-scene-types`.
//!
//! ```rust,no_run
//! # use jmap_scene_client::JmapSceneExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_scene_session(session);
//! let regions = sc.scene_region_get(None, None).await?;
//! # Ok(())
//! # }
//! ```
//!
//! [`AssetFetcher`] fetches a peer-supplied `assetUri` (SceneAsset or
//! SceneObject) under the draft's §assetUri Is Untrusted rules:
//!
//! - "Clients MUST validate that `assetUri` uses a permitted scheme
//!   (`https:`) before fetching": any other scheme fails with
//!   [`AssetFetchError::NotHttps`] before a connection is made.
//! - "Clients MUST NOT follow redirects to non-HTTPS endpoints": such a
//!   redirect fails with [`AssetFetchError::Transport`] and the target is
//!   never contacted.
//! - "Clients SHOULD verify asset integrity using the `sha256` field on
//!   SceneAsset when available": pass it to [`AssetFetcher::fetch`], which
//!   fails with [`AssetFetchError::DigestMismatch`] when the body differs.
//!
//! ```rust,no_run
//! use jmap_cid_types::Sha256;
//! use jmap_scene_client::AssetFetcher;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let fetcher = AssetFetcher::new()?;
//! let sha256: Sha256 =
//!     "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".parse()?;
//! let glb = fetcher
//!     .fetch("https://cdn.example.com/assets/sculpture-001.glb", Some(&sha256), 2_483_712)
//!     .await?;
//! # Ok(()) }
//! ```
//!
//! The bytes are untrusted: §Asset Parsing requires clients to "parse
//! assets in a sandboxed context".
//!
//! [`SceneWsExt`] adds draft-atwood-jmap-scene-wss-00 ephemeral Scene
//! events to a `jmap_base_client::WsSession`: send `SceneStreamEnable`,
//! `SceneStreamDisable` and `SceneInteractionEvent` frames, and receive
//! [`SceneWsFrame`]s, with every non-Scene frame passed through unchanged.
//!
//! # Re-exported types
//!
//! [`ClientError`] and the shared method response types (`GetResponse`,
//! `SetResponse`, `ChangesResponse`, `QueryResponse` and the rest) are
//! re-exports from `jmap-base-client` and `jmap-types`, not types defined
//! here. The `jmap-*` workspace crates are versioned and released together:
//! depend on the same version of this crate, `jmap-base-client` and
//! `jmap-types`.

#![forbid(unsafe_code)]

use jmap_cid_types::Sha256;
use reqwest::redirect;
use sha2::Digest;

pub use reqwest;

mod methods;
mod ws;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, ChangesResponse, GetResponse, PartialObject, QueryChangesResponse, QueryResponse,
    SessionClient, SetError, SetResponse,
};
pub use ws::{parse_scene_ws_frame, SceneWsExt, SceneWsFrame};

/// Extension trait adding JMAP Scene methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_scene_client::JmapSceneExt;`
/// This trait is **sealed**: the crate adds an `impl` only for
/// [`jmap_base_client::JmapClient`], which keeps adding methods to the
/// trait a non-breaking change.
pub trait JmapSceneExt: sealed::Sealed {
    /// Bind this client to a JMAP session for use with Scene methods.
    ///
    /// The returned [`SessionClient`] captures the session at construction
    /// time. After re-fetching the session, construct a new `SessionClient`.
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts any [`jmap_base_client::Session`] and never
    /// fails. Each method call on the returned [`SessionClient`] returns
    /// [`ClientError::InvalidSession`] when the session has no primary
    /// account for `urn:ietf:params:jmap:scene`. Pre-check with
    /// [`session.primary_account_id("urn:ietf:params:jmap:scene")`](jmap_base_client::Session::primary_account_id)
    /// to guard at the binding site.
    fn with_scene_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapSceneExt for jmap_base_client::JmapClient {
    fn with_scene_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
            account_id: None,
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapSceneExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}

/// The most redirects [`AssetFetcher`] follows (reqwest's default).
pub const MAX_REDIRECTS: usize = 10;

/// Error from [`AssetFetcher`].
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AssetFetchError {
    /// `assetUri` is not an absolute URI.
    #[error("assetUri is not a valid URI")]
    InvalidUri(#[source] url::ParseError),
    /// `assetUri` does not use the `https:` scheme.
    #[error("assetUri scheme is not https")]
    NotHttps,
    /// The response status was not 2xx.
    #[error("HTTP status {0}")]
    Status(u16),
    /// The response body is longer than the caller's limit.
    #[error("asset exceeds {0} bytes")]
    TooLarge(usize),
    /// The body's SHA-256 differs from the expected `sha256`.
    #[error("asset sha256 {actual} does not match expected {expected}")]
    DigestMismatch {
        /// The `sha256` the caller expected.
        expected: Sha256,
        /// The SHA-256 of the fetched body.
        actual: Sha256,
    },
    /// The request failed before a complete response arrived: DNS, TLS,
    /// a redirect to a non-`https` URI or past [`MAX_REDIRECTS`],
    /// connection, timeout.
    #[error("HTTP request failed")]
    Transport(#[source] reqwest::Error),
}

/// Fetches `assetUri` values over HTTPS only.
#[derive(Debug, Clone)]
pub struct AssetFetcher {
    client: reqwest::Client,
}

impl AssetFetcher {
    /// A fetcher over reqwest's default client.
    ///
    /// # Errors
    ///
    /// [`AssetFetchError::Transport`] when reqwest cannot build the client.
    pub fn new() -> Result<Self, AssetFetchError> {
        Self::with_builder(reqwest::Client::builder())
    }

    /// A fetcher over `builder`.
    ///
    /// HTTPS-only and the redirect rule are set on `builder`, replacing
    /// any value it had for them. Timeouts, root certificates and other
    /// settings come from `builder`; reqwest's default has no timeout.
    ///
    /// # Errors
    ///
    /// [`AssetFetchError::Transport`] when reqwest cannot build the client.
    pub fn with_builder(builder: reqwest::ClientBuilder) -> Result<Self, AssetFetchError> {
        let redirects = redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if attempt.url().scheme() != "https" {
                attempt.error("redirect to a non-https URI")
            } else {
                attempt.follow()
            }
        });
        builder
            .https_only(true)
            .redirect(redirects)
            .build()
            .map(|client| Self { client })
            .map_err(AssetFetchError::Transport)
    }

    /// GETs `asset_uri` and returns its body.
    ///
    /// Reading stops with [`AssetFetchError::TooLarge`] once the body
    /// passes `max_bytes`; a SceneAsset's `size` is a natural limit. When
    /// `sha256` is given, the body must hash to it.
    ///
    /// # Errors
    ///
    /// [`AssetFetchError::InvalidUri`] or [`AssetFetchError::NotHttps`]
    /// before any connection; otherwise [`AssetFetchError::Status`],
    /// [`AssetFetchError::TooLarge`], [`AssetFetchError::DigestMismatch`]
    /// or [`AssetFetchError::Transport`].
    pub async fn fetch(
        &self,
        asset_uri: &str,
        sha256: Option<&Sha256>,
        max_bytes: usize,
    ) -> Result<Vec<u8>, AssetFetchError> {
        let url = reqwest::Url::parse(asset_uri).map_err(AssetFetchError::InvalidUri)?;
        if url.scheme() != "https" {
            return Err(AssetFetchError::NotHttps);
        }
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(AssetFetchError::Transport)?;
        let status = response.status();
        if !status.is_success() {
            return Err(AssetFetchError::Status(status.as_u16()));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(AssetFetchError::Transport)? {
            if body.len() + chunk.len() > max_bytes {
                return Err(AssetFetchError::TooLarge(max_bytes));
            }
            body.extend_from_slice(&chunk);
        }
        if let Some(expected) = sha256 {
            check_digest(&body, expected)?;
        }
        Ok(body)
    }
}

/// Fails with [`AssetFetchError::DigestMismatch`] unless `body` hashes to
/// `expected`.
fn check_digest(body: &[u8], expected: &Sha256) -> Result<(), AssetFetchError> {
    let actual = Sha256::from(<[u8; 32]>::from(sha2::Sha256::digest(body)));
    if actual == *expected {
        Ok(())
    } else {
        Err(AssetFetchError::DigestMismatch {
            expected: expected.clone(),
            actual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIPS 180-4 example vectors (NIST CSRC "SHA256.pdf" examples):
    /// "abc" and the 448-bit two-block message.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const TWO_BLOCK_MSG: &[u8] = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    const TWO_BLOCK: &str = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";

    #[test]
    fn fips_180_4_vectors_match() {
        check_digest(b"abc", &ABC.parse().unwrap()).unwrap();
        check_digest(TWO_BLOCK_MSG, &TWO_BLOCK.parse().unwrap()).unwrap();
    }

    #[test]
    fn mismatch_reports_both_digests() {
        let err = check_digest(TWO_BLOCK_MSG, &ABC.parse().unwrap()).unwrap_err();
        match err {
            AssetFetchError::DigestMismatch { expected, actual } => {
                assert_eq!(expected, ABC);
                assert_eq!(actual, TWO_BLOCK);
            }
            other => panic!("{other:?}"),
        }
    }
}
