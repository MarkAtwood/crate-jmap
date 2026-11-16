//! [`SessionClient`]: the blobext-03 methods bound to one session and
//! account.

use std::collections::BTreeMap;

use jmap_base_client::{
    ClientError, JmapClient, JmapRequestBuilder, JmapUrlTemplate, Session, extract_response,
    to_args,
};
use jmap_blob_types::{
    BlobAccountCapability, BlobConvertArgs, BlobConvertResponse, BlobCreateObject, BlobGetArgs,
    BlobGetResponse, BlobLookupArgs, BlobLookupResponse, BlobProperty, BlobRef, BlobSetArgs,
    BlobSetResponse, ConvertRequest, DataSourceProperty, DigestAlgorithm, JMAP_BLOB2_URI,
};
use jmap_types::{Id, PatchObject, State, UTCDate};
use serde_json::{Map, Value};

const CORE: &str = "urn:ietf:params:jmap:core";
const CALL_ID: &str = "0";

/// The arguments of a `Blob/set` (blobext-03 §4) other than `accountId`.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlobSetInput {
    /// `ifInState`.
    pub if_in_state: Option<State>,
    /// `create`: creation id → the blob to create.
    pub create: BTreeMap<Id, BlobCreateObject>,
    /// `update`: blob → PatchObject (only `expires` may change, §7).
    pub update: BTreeMap<BlobRef, PatchObject>,
    /// `destroy`.
    pub destroy: Vec<BlobRef>,
    /// Any other argument.
    pub extra: Map<String, Value>,
}

/// The arguments of a `Blob/get` (blobext-03 §5) other than `accountId`
/// and `ids`. Unset fields are left out, so the server's defaults apply.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlobGetInput {
    /// `properties`.
    pub properties: Option<Vec<BlobProperty>>,
    /// `offset` of the range to read.
    pub offset: Option<u64>,
    /// `length` of the range to read.
    pub length: Option<u64>,
    /// `dataSourceProperties` of each chunk.
    pub data_source_properties: Option<Vec<DataSourceProperty>>,
}

/// The blobext-03 methods bound to a session, from
/// [`JmapBlobExt::with_blob_session`](crate::JmapBlobExt::with_blob_session).
///
/// Each call targets the session's primary `blob2` account, or the account
/// [`for_account`](Self::for_account) names.
#[derive(Clone)]
pub struct SessionClient {
    client: JmapClient,
    session: Session,
    account: Option<Id>,
}

impl std::fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClient")
            .field("api_url", &self.session.api_url.as_str())
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl SessionClient {
    pub(crate) fn new(client: JmapClient, session: Session) -> Self {
        Self {
            client,
            session,
            account: None,
        }
    }

    /// This client, targeting `account_id` instead of the primary `blob2`
    /// account.
    #[must_use]
    pub fn for_account(mut self, account_id: Id) -> Self {
        self.account = Some(account_id);
        self
    }

    /// The account the calls target: the one [`for_account`](Self::for_account)
    /// named, else `primaryAccounts["urn:ietf:params:jmap:blob2"]`.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`] when neither exists.
    pub fn blob_account_id(&self) -> Result<Id, ClientError> {
        if let Some(account) = &self.account {
            return Ok(account.clone());
        }
        self.session
            .primary_account_id(JMAP_BLOB2_URI)
            .cloned()
            .ok_or_else(|| {
                ClientError::InvalidSession(format!("no primary account for {JMAP_BLOB2_URI}"))
            })
    }

    /// The account's `blob2` capability object (blobext-03 §2.1), or
    /// `None` when the account does not advertise it.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`] without an account, or when the
    /// object does not parse.
    pub fn blob_capability(&self) -> Result<Option<BlobAccountCapability>, ClientError> {
        let account = self.blob_account_id()?;
        let Some(value) = self
            .session
            .accounts
            .get(&account)
            .and_then(|a| a.account_capabilities.get(JMAP_BLOB2_URI))
        else {
            return Ok(None);
        };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| ClientError::InvalidSession(format!("{JMAP_BLOB2_URI} capability: {e}")))
    }

    /// The upload URL template for this account: the capability's
    /// `uploadUrl` when the account has one, else the session's
    /// (blobext-03 §2.1). Pass it to
    /// [`JmapClient::upload_blob`](jmap_base_client::JmapClient::upload_blob).
    ///
    /// # Errors
    ///
    /// As [`blob_capability`](Self::blob_capability).
    pub fn upload_url(&self) -> Result<JmapUrlTemplate, ClientError> {
        Ok(self
            .blob_capability()?
            .and_then(|c| c.upload_url)
            .map_or_else(|| self.session.upload_url.clone(), JmapUrlTemplate::new))
    }

    /// The first of the server's `supportedDigestAlgorithms` (its order of
    /// preference) that `client_supported` includes, or `None`.
    ///
    /// # Errors
    ///
    /// As [`blob_capability`](Self::blob_capability).
    pub fn preferred_digest_algorithm(
        &self,
        client_supported: &[DigestAlgorithm],
    ) -> Result<Option<DigestAlgorithm>, ClientError> {
        Ok(self.blob_capability()?.and_then(|c| {
            c.supported_digest_algorithms
                .into_iter()
                .find(|a| client_supported.contains(a))
        }))
    }

    /// The `supportedTypeNames` `Blob/lookup` accepts on this account;
    /// empty when the capability is absent.
    ///
    /// # Errors
    ///
    /// As [`blob_capability`](Self::blob_capability).
    pub fn lookup_type_names(&self) -> Result<Vec<String>, ClientError> {
        Ok(self
            .blob_capability()?
            .map(|c| c.supported_type_names)
            .unwrap_or_default())
    }

    /// Whether `value`, a base64 digest the server reported, is `alg`'s
    /// digest of `bytes`; `None` when the crate has no built-in digester
    /// for `alg` (it has `sha`, `sha-256` and `sha-512`).
    pub fn verify_digest(alg: &DigestAlgorithm, bytes: &[u8], value: &str) -> Option<bool> {
        let mut digest = alg.builtin_digester()?;
        digest.update(bytes);
        Some(digest.finish() == value)
    }

    /// `Blob/set` (blobext-03 §4).
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`] without an account,
    /// [`ClientError::MethodError`] for a method-level error, and the
    /// transport and parse errors of
    /// [`JmapClient::call`](jmap_base_client::JmapClient::call).
    pub async fn blob_set(&self, input: BlobSetInput) -> Result<BlobSetResponse, ClientError> {
        let BlobSetInput {
            if_in_state,
            create,
            update,
            destroy,
            extra,
        } = input;
        let mut args = BlobSetArgs::new(self.blob_account_id()?);
        args.if_in_state = if_in_state;
        args.create = (!create.is_empty()).then_some(create);
        args.update = (!update.is_empty()).then_some(update);
        args.destroy = (!destroy.is_empty()).then_some(destroy);
        args.extra = extra;
        self.call("Blob/set", to_args(&args)?, &[]).await
    }

    /// `Blob/set` creating `create`.
    ///
    /// # Errors
    ///
    /// As [`blob_set`](Self::blob_set).
    pub async fn blob_create(
        &self,
        create: BTreeMap<Id, BlobCreateObject>,
    ) -> Result<BlobSetResponse, ClientError> {
        self.blob_set(BlobSetInput {
            create,
            ..BlobSetInput::default()
        })
        .await
    }

    /// `Blob/set` setting `expires` on each of `ids` (blobext-03 §7):
    /// `None` asks for no specific expiry (`null`).
    ///
    /// # Errors
    ///
    /// As [`blob_set`](Self::blob_set).
    pub async fn blob_touch(
        &self,
        ids: Vec<BlobRef>,
        expires: Option<UTCDate>,
    ) -> Result<BlobSetResponse, ClientError> {
        let value = expires.map_or(Value::Null, |d| Value::String(d.as_ref().to_owned()));
        let mut input = BlobSetInput::default();
        for id in ids {
            let mut patch = PatchObject::new();
            patch
                .as_map_mut()
                .insert("expires".to_owned(), value.clone());
            input.update.insert(id, patch);
        }
        self.blob_set(input).await
    }

    /// `Blob/set` destroying `ids`.
    ///
    /// # Errors
    ///
    /// As [`blob_set`](Self::blob_set).
    pub async fn blob_destroy(&self, ids: Vec<BlobRef>) -> Result<BlobSetResponse, ClientError> {
        self.blob_set(BlobSetInput {
            destroy: ids,
            ..BlobSetInput::default()
        })
        .await
    }

    /// `Blob/get` (blobext-03 §5) of `ids`.
    ///
    /// # Errors
    ///
    /// As [`blob_set`](Self::blob_set).
    pub async fn blob_get(
        &self,
        ids: Vec<BlobRef>,
        input: BlobGetInput,
    ) -> Result<BlobGetResponse, ClientError> {
        let mut args = BlobGetArgs::new(self.blob_account_id()?, ids);
        args.properties = input.properties;
        args.offset = input.offset;
        args.length = input.length;
        args.data_source_properties = input.data_source_properties;
        self.call("Blob/get", to_args(&args)?, &[]).await
    }

    /// `Blob/lookup` (blobext-03 §6): which objects of `type_names`
    /// reference each of `ids`.
    ///
    /// `using` gains each name's capability from the JMAP Data Types
    /// registry (`jmap_types::data_types`) and `extra_using` (for a type the
    /// registry does not list).
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidArgument`] for empty `ids` or `type_names`, or
    /// a registered type whose capability the session does not advertise
    /// (blobext-03 §2.1: the client must be able to use the type); otherwise
    /// as [`blob_set`](Self::blob_set).
    pub async fn blob_lookup(
        &self,
        ids: Vec<Id>,
        type_names: Vec<String>,
        extra_using: &[&str],
    ) -> Result<BlobLookupResponse, ClientError> {
        if ids.is_empty() || type_names.is_empty() {
            return Err(ClientError::InvalidArgument(
                "blob_lookup: ids and type_names must not be empty".into(),
            ));
        }
        let mut using: Vec<&str> = extra_using.to_vec();
        for name in &type_names {
            let Some(entry) = jmap_types::data_types::data_type(name) else {
                continue;
            };
            if !self.session.capabilities.contains_key(entry.capability) {
                return Err(ClientError::InvalidArgument(format!(
                    "blob_lookup: the session does not advertise {} for {name}",
                    entry.capability
                )));
            }
            using.push(entry.capability);
        }
        let args = BlobLookupArgs::new(self.blob_account_id()?, type_names, ids);
        self.call("Blob/lookup", to_args(&args)?, &using).await
    }

    /// `Blob/convert` (blobext-03 §8) creating `create`.
    ///
    /// # Errors
    ///
    /// As [`blob_set`](Self::blob_set).
    pub async fn blob_convert(
        &self,
        create: BTreeMap<Id, ConvertRequest>,
    ) -> Result<BlobConvertResponse, ClientError> {
        let args = BlobConvertArgs::new(self.blob_account_id()?, create);
        self.call("Blob/convert", to_args(&args)?, &[]).await
    }

    /// One `method` call with core, blob2 and `extra_using` in `using`.
    async fn call<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        args: Value,
        extra_using: &[&str],
    ) -> Result<R, ClientError> {
        let mut using = vec![CORE, JMAP_BLOB2_URI];
        for uri in extra_using {
            if !using.contains(uri) {
                using.push(uri);
            }
        }
        let mut builder = JmapRequestBuilder::new(&using);
        builder.add_call(method, args, CALL_ID)?;
        let response = self
            .client
            .call_session(&self.session, &builder.build()?)
            .await?;
        extract_response(&response, CALL_ID)
    }
}
