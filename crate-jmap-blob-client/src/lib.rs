//! JMAP Blob Management client: typed `Blob/set`, `Blob/get`,
//! `Blob/lookup` and `Blob/convert` calls (draft-ietf-jmap-blobext-03,
//! `urn:ietf:params:jmap:blob2`) over `jmap-base-client`
//! (docs/design/blobext.md §8).
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_blob_client::JmapBlobExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let blobs = client.with_blob_session(session);
//! let found = blobs
//!     .blob_lookup(vec!["Bfirst".into()], vec!["Email".to_owned()], &[])
//!     .await?;
//! # let _ = found;
//! # Ok(())
//! # }
//! ```
//!
//! Every call's `using` is core plus `urn:ietf:params:jmap:blob2`, plus
//! the capabilities of the data types a `Blob/lookup` names; never the RFC
//! 9404 `urn:ietf:params:jmap:blob`. For a server that only advertises RFC
//! 9404, [`JmapBlobExt::with_blob_legacy_session`] gives a
//! [`LegacySessionClient`] whose calls name `urn:ietf:params:jmap:blob`
//! and never `blob2` (blobext-03 §2.1 forbids both in one request). A conditional `Blob/set`
//! (draft-ietf-jmap-conditional-02) goes through
//! [`jmap_base_client::SetCall`] (docs/design/DECISIONS.md decision 8).
//!
//! # Re-exported types
//!
//! [`ClientError`] comes from `jmap-base-client`; the request and response
//! types are `jmap-blob-types`'. The `jmap-*` crates are released
//! together: depend on the same version of each.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod legacy;
mod methods;

pub use jmap_base_client::ClientError;
pub use jmap_blob_types::{
    BlobAccountCapability, BlobConvertResponse, BlobCreateObject, BlobGetResponse,
    BlobLookupResponse, BlobProperty, BlobRef, BlobSetResponse, BlobUploadCallResponse,
    ConvertRequest, DataSourceObject, DataSourceProperty, DigestAlgorithm,
    Rfc9404AccountCapability, Rfc9404LookupResponse, UploadObject,
};
pub use legacy::LegacySessionClient;
pub use methods::{BlobGetInput, BlobSetInput, SessionClient};

/// Extension trait adding draft-ietf-jmap-blobext-03 methods to
/// [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_blob_client::JmapBlobExt;`.
///
/// This trait is **sealed**: the crate implements it only for
/// [`jmap_base_client::JmapClient`], so adding methods stays non-breaking.
pub trait JmapBlobExt: sealed::Sealed {
    /// A [`SessionClient`] bound to this client and session.
    ///
    /// It accepts any session; a session without a primary account for
    /// `urn:ietf:params:jmap:blob2` fails each call with
    /// [`ClientError::InvalidSession`] unless
    /// [`for_account`](SessionClient::for_account) names one.
    fn with_blob_session(&self, session: jmap_base_client::Session) -> SessionClient;

    /// A [`LegacySessionClient`] for RFC 9404's `urn:ietf:params:jmap:blob`
    /// (`Blob/upload`, `Blob/get`, `Blob/lookup`), for servers that do not
    /// advertise `blob2`.
    ///
    /// It accepts any session; one without a primary account for
    /// `urn:ietf:params:jmap:blob` fails each call with
    /// [`ClientError::InvalidSession`] unless
    /// [`for_account`](LegacySessionClient::for_account) names one.
    fn with_blob_legacy_session(&self, session: jmap_base_client::Session) -> LegacySessionClient;
}

impl JmapBlobExt for jmap_base_client::JmapClient {
    fn with_blob_session(&self, session: jmap_base_client::Session) -> SessionClient {
        SessionClient::new(self.clone(), session)
    }

    fn with_blob_legacy_session(&self, session: jmap_base_client::Session) -> LegacySessionClient {
        LegacySessionClient::new(self.clone(), session)
    }
}

mod sealed {
    /// Sealing trait for [`super::JmapBlobExt`].
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
