//! FileNode blob download with `sha256` verification
//! (draft-atwood-jmap-cid-00 §FileNode Extension, "Client Usage").
//!
//! The draft says clients "SHOULD compute the SHA-256 digest of the received
//! bytes and compare it against the FileNode's `sha256`", and that a null
//! `sha256` means either a directory or a legacy file, so "Clients MUST check
//! the FileNode type to distinguish these cases".

use jmap_base_client::{ClientError, DownloadBlobSessionParams};
use jmap_filenode_types::{FileNode, NodeType};

use super::SessionClient;

/// The bytes of a FileNode blob and whether their digest was checked.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileNodeDownload {
    /// The digest of the bytes equals the FileNode's non-null `sha256`.
    Verified(bytes::Bytes),
    /// The FileNode is a file whose `sha256` is null or absent (a legacy
    /// entry, or a server without `urn:ietf:params:jmap:cid`). The draft says
    /// clients "SHOULD treat such downloads as unverified".
    Unverified(bytes::Bytes),
}

impl SessionClient {
    /// Download the blob of a `File` FileNode and verify it against the
    /// node's `sha256`.
    ///
    /// When `node.sha256` is `Some`, the downloaded digest is compared in
    /// constant time by [`JmapClient::download_blob`](jmap_base_client::JmapClient::download_blob).
    /// `{name}` is the node's `name` and `{type}` its `mediaType`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`] if `node.node_type` is not
    ///   `Some(NodeType::File)` (a directory "has no associated blob and
    ///   should not be downloaded"; an untyped partial object cannot be told
    ///   apart from one), or if a `File` node has no `blobId`.
    /// - [`ClientError::InvalidSession`] if the bound session has no primary
    ///   account for `urn:ietf:params:jmap:filenode`.
    /// - [`ClientError::BlobIntegrityMismatch`] if `node.sha256` is `Some` and
    ///   differs from the digest of the bytes received. The draft says the
    ///   client "SHOULD treat the downloaded content as untrusted".
    /// - Any transport error from
    ///   [`JmapClient::download_blob`](jmap_base_client::JmapClient::download_blob).
    pub async fn filenode_download(
        &self,
        node: &FileNode,
    ) -> Result<FileNodeDownload, ClientError> {
        if node.node_type != Some(NodeType::File) {
            return Err(ClientError::InvalidArgument(
                "filenode_download: node @type is not File".into(),
            ));
        }
        let blob_id = node.blob_id.as_ref().ok_or_else(|| {
            ClientError::InvalidArgument("filenode_download: File node has no blobId".into())
        })?;
        let account_id = self.filenode_account_id()?;
        let bytes = self
            .client
            .download_blob_session(
                &self.session,
                DownloadBlobSessionParams {
                    account_id,
                    blob_id: blob_id.as_ref(),
                    name: &node.name,
                    accept_type: node.media_type.as_deref(),
                    expected_sha256: node.sha256.as_ref(),
                },
            )
            .await?;
        Ok(match node.sha256 {
            Some(_) => FileNodeDownload::Verified(bytes),
            None => FileNodeDownload::Unverified(bytes),
        })
    }
}
