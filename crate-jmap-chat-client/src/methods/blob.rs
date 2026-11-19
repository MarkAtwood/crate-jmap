//! Blob/lookup and Blob/convert — urn:ietf:params:jmap:blob2
//!
//! Spec: draft-ietf-jmap-blobext-03 §6 (Blob/lookup), §8 (Blob/convert).
//!
//! These are chat-shaped calls over [`jmap_blob_client`], which holds the
//! blobext-03 bindings and types; [`SessionClient::blob_client`] gives the
//! full blob API (Blob/set, Blob/get, other recipes). Both calls target
//! the session's blob2 account; Blob/lookup also names the chat
//! capability, so its Message type name is a known data type (§6
//! `unknownDataType`).

use std::collections::BTreeMap;

use jmap_blob_client::{BlobConvertResponse, BlobLookupResponse, ConvertRequest, JmapBlobExt as _};
use jmap_blob_types::{ImageConvertRecipe, Recipe};
use jmap_types::Id;

use super::SessionClient;

/// The draft-ietf-jmap-blobext-03 capability URI.
const BLOB2_URI: &str = "urn:ietf:params:jmap:blob2";

impl SessionClient {
    /// The account the Blob methods target: the one named with
    /// [`for_account`](Self::for_account), else the session's primary
    /// account for `urn:ietf:params:jmap:blob2` (blobext-03 §2.1), which
    /// need not be the chat account.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    /// when no account was named and the session has no blob2 primary
    /// account.
    pub fn blob_account_id(&self) -> Result<&Id, jmap_base_client::ClientError> {
        if let Some(id) = self.explicit_account_id() {
            return Ok(id);
        }
        self.session.primary_account_id(BLOB2_URI).ok_or_else(|| {
            jmap_base_client::ClientError::InvalidSession(format!(
                "no primary account for {BLOB2_URI}"
            ))
        })
    }

    /// A [`jmap_blob_client::SessionClient`] on this client and session,
    /// targeting the account named with [`for_account`](Self::for_account)
    /// if any, else the session's primary blob2 account.
    pub fn blob_client(&self) -> jmap_blob_client::SessionClient {
        let blob = self.client.with_blob_session(self.session.clone());
        match self.explicit_account_id() {
            Some(id) => blob.for_account(id.clone()),
            None => blob,
        }
    }

    /// Reverse-lookup blobs: which objects of `type_names` (such as
    /// `"Message"`) reference each of `blob_ids` (blobext-03 §6).
    ///
    /// `using` is core, `urn:ietf:params:jmap:blob2`,
    /// `urn:ietf:params:jmap:chat`, and the capability of each registered
    /// type name; the call targets [`Self::blob_account_id`].
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `blob_ids` or `type_names` is empty, or a registered type
    ///   name's capability is not in the session.
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   as for [`Self::blob_account_id`].
    /// - Transport, parse and method-level errors, as for
    ///   [`jmap_blob_client::SessionClient::blob_lookup`].
    pub async fn blob_lookup(
        &self,
        blob_ids: &[Id],
        type_names: &[&str],
    ) -> Result<BlobLookupResponse, jmap_base_client::ClientError> {
        self.blob_client()
            .blob_lookup(
                blob_ids.to_vec(),
                type_names.iter().map(|&t| t.to_owned()).collect(),
                &[jmap_chat_types::JMAP_CHAT_URI],
            )
            .await
    }

    /// Convert `from_blob_id` to `content_type` with an `imageConvert`
    /// recipe (blobext-03 §8, §8.1), such as an `image/webp` thumbnail
    /// without downloading the original. `width` and `height` are optional
    /// maximum dimensions.
    ///
    /// The result is under the creation id [`super::CALL_ID`] in the
    /// response's `created` (or `not_created`: §8.1 `invalidProperties`,
    /// `tooLarge`, `conversionFailed`).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `content_type` is empty.
    /// - As [`Self::blob_lookup`] otherwise.
    pub async fn blob_convert(
        &self,
        from_blob_id: &Id,
        content_type: &str,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<BlobConvertResponse, jmap_base_client::ClientError> {
        if content_type.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "blob_convert: content_type may not be empty".into(),
            ));
        }
        let mut recipe = ImageConvertRecipe::new(from_blob_id.clone().into(), content_type);
        recipe.width = width.map(u64::from);
        recipe.height = height.map(u64::from);
        let create = BTreeMap::from([(
            Id::from(super::CALL_ID),
            ConvertRequest::new(Recipe::ImageConvert(recipe)),
        )]);
        self.blob_client().blob_convert(create).await
    }
}
