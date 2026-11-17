//! [`LegacySessionClient`]: RFC 9404 `Blob/upload`, `Blob/get` and
//! `Blob/lookup` under `urn:ietf:params:jmap:blob`, for servers that do
//! not advertise blobext-03's `blob2` (docs/design/blobext.md §8,
//! docs/design/DECISIONS.md decision 21(v)).

use std::collections::BTreeMap;

use jmap_base_client::{ClientError, JmapClient, Session, to_args};
use jmap_blob_types::{
    BlobGetArgs, BlobGetResponse, BlobLookupArgs, BlobProperty, BlobRef, BlobUploadArgs,
    BlobUploadCallResponse, JMAP_BLOB_URI, JMAP_BLOB2_URI, Rfc9404AccountCapability,
    Rfc9404LookupResponse, UploadObject,
};
use jmap_types::Id;

use crate::methods::{call_with, lookup_using};

/// The RFC 9404 methods bound to a session, from
/// [`JmapBlobExt::with_blob_legacy_session`](crate::JmapBlobExt::with_blob_legacy_session).
///
/// Each call targets the session's primary `urn:ietf:params:jmap:blob`
/// account, or the account [`for_account`](Self::for_account) names, and
/// its `using` is core plus `urn:ietf:params:jmap:blob` (plus, for
/// `Blob/lookup`, the type capabilities); never `blob2`, which a request
/// must not name alongside `blob` (blobext-03 §2.1).
#[derive(Clone)]
pub struct LegacySessionClient {
    client: JmapClient,
    session: Session,
    account: Option<Id>,
}

impl std::fmt::Debug for LegacySessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LegacySessionClient")
            .field("api_url", &self.session.api_url.as_str())
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl LegacySessionClient {
    pub(crate) fn new(client: JmapClient, session: Session) -> Self {
        Self {
            client,
            session,
            account: None,
        }
    }

    /// This client, targeting `account_id` instead of the primary `blob`
    /// account.
    #[must_use]
    pub fn for_account(mut self, account_id: Id) -> Self {
        self.account = Some(account_id);
        self
    }

    /// The account the calls target: the one [`for_account`](Self::for_account)
    /// named, else `primaryAccounts["urn:ietf:params:jmap:blob"]`.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`] when neither exists.
    pub fn blob_account_id(&self) -> Result<Id, ClientError> {
        if let Some(account) = &self.account {
            return Ok(account.clone());
        }
        self.session
            .primary_account_id(JMAP_BLOB_URI)
            .cloned()
            .ok_or_else(|| {
                ClientError::InvalidSession(format!("no primary account for {JMAP_BLOB_URI}"))
            })
    }

    /// The account's `urn:ietf:params:jmap:blob` capability object (RFC
    /// 9404 §3.1), or `None` when the account does not advertise it.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`] without an account, or when the
    /// object does not parse.
    pub fn blob_capability(&self) -> Result<Option<Rfc9404AccountCapability>, ClientError> {
        let account = self.blob_account_id()?;
        let Some(value) = self
            .session
            .accounts
            .get(&account)
            .and_then(|a| a.account_capabilities.get(JMAP_BLOB_URI))
        else {
            return Ok(None);
        };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| ClientError::InvalidSession(format!("{JMAP_BLOB_URI} capability: {e}")))
    }

    /// `Blob/upload` (RFC 9404 §4.1) creating `create`.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`] without an account; any transport
    /// or method error from the call. Per-blob failures are in
    /// [`BlobUploadCallResponse::not_created`].
    pub async fn blob_upload(
        &self,
        create: BTreeMap<Id, UploadObject>,
    ) -> Result<BlobUploadCallResponse, ClientError> {
        let args = BlobUploadArgs::new(self.blob_account_id()?, create);
        self.call("Blob/upload", to_args(&args)?, &[]).await
    }

    /// `Blob/get` (RFC 9404 §4.2) of `ids`, reading `properties` (`data`,
    /// `data:asText`, `data:asBase64`, `digest:<alg>`, `size`; `None` for
    /// the server's default) from the `offset`/`length` range.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidArgument`] for a blobext-03-only property
    /// (`imageData`, `chunks`), which RFC 9404 does not define; otherwise
    /// as [`blob_upload`](Self::blob_upload).
    pub async fn blob_get(
        &self,
        ids: Vec<BlobRef>,
        properties: Option<Vec<BlobProperty>>,
        offset: Option<u64>,
        length: Option<u64>,
    ) -> Result<BlobGetResponse, ClientError> {
        if let Some(p) = properties
            .iter()
            .flatten()
            .find(|p| matches!(p, BlobProperty::ImageData | BlobProperty::Chunks))
        {
            return Err(ClientError::InvalidArgument(format!(
                "blob_get: {p:?} is not an RFC 9404 Blob/get property"
            )));
        }
        let mut args = BlobGetArgs::new(self.blob_account_id()?, ids);
        args.properties = properties;
        args.offset = offset;
        args.length = length;
        self.call("Blob/get", to_args(&args)?, &[]).await
    }

    /// `Blob/lookup` (RFC 9404 §4.3): which objects of `type_names`
    /// reference each of `ids`.
    ///
    /// `using` gains each name's capability from the JMAP Data Types
    /// registry (`jmap_types::data_types`) and `extra_using` (for a type the
    /// registry does not list): §4.3 requires "the capability that defines
    /// each type".
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidArgument`] for empty `ids` or `type_names`,
    /// `urn:ietf:params:jmap:blob2` in `extra_using` (blobext-03 §2.1), or a
    /// registered type whose capability the session does not advertise;
    /// otherwise as [`blob_upload`](Self::blob_upload).
    pub async fn blob_lookup(
        &self,
        ids: Vec<Id>,
        type_names: Vec<String>,
        extra_using: &[&str],
    ) -> Result<Rfc9404LookupResponse, ClientError> {
        if ids.is_empty() || type_names.is_empty() {
            return Err(ClientError::InvalidArgument(
                "blob_lookup: ids and type_names must not be empty".into(),
            ));
        }
        if extra_using.contains(&JMAP_BLOB2_URI) {
            return Err(ClientError::InvalidArgument(format!(
                "blob_lookup: {JMAP_BLOB2_URI} must not be used with {JMAP_BLOB_URI}"
            )));
        }
        let using = lookup_using(&self.session, &type_names, extra_using)?;
        let args = BlobLookupArgs::new(self.blob_account_id()?, type_names, ids);
        self.call("Blob/lookup", to_args(&args)?, &using).await
    }

    /// One `method` call with core, `urn:ietf:params:jmap:blob` and
    /// `extra_using` in `using`.
    async fn call<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        args: serde_json::Value,
        extra_using: &[&str],
    ) -> Result<R, ClientError> {
        call_with(
            &self.client,
            &self.session,
            JMAP_BLOB_URI,
            method,
            args,
            extra_using,
        )
        .await
    }
}
