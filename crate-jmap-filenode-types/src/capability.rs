//! draft-ietf-jmap-filenode-13 §2.1 — account-level capability object.
//!
//! Provides [`FileNodeCapability`] and the capability URI constant
//! [`JMAP_FILENODE_URI`].

use serde::{Deserialize, Serialize};

/// The JMAP capability URI for the FileNode extension.
///
/// Present as a key in both the session-level `capabilities` object (value:
/// empty object) and in each account's `accountCapabilities` object (value:
/// a [`FileNodeCapability`]).
pub const JMAP_FILENODE_URI: &str = "urn:ietf:params:jmap:filenode";

/// Account-level capability for the JMAP FileNode extension
/// (draft-ietf-jmap-filenode-13 §2.1).
///
/// The value of the `urn:ietf:params:jmap:filenode` key in `accountCapabilities`.
///
/// ## Nullable fields
///
/// Fields typed `Option<T>` with no `skip_serializing_if` are **required-and-nullable**:
/// they MUST appear in wire JSON even when the value is `null` (e.g. `"maxFileNodeDepth":null`
/// means no limit).  Fields with `skip_serializing_if = "Option::is_none"` are absent when
/// `None`, but none exist on this struct — all optional fields here are nullable.
///
/// ## `maxSizeFileNodeName` constraint
///
/// Per §2.1 the server MUST set this to at least 100.  This crate does not enforce
/// the constraint at the type level (that would require a newtype); the server
/// implementation is responsible for the invariant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeCapability {
    /// Maximum depth of the FileNode hierarchy (one more than the max ancestor count),
    /// or `null` for no limit.
    ///
    /// Always serialized (as `null` when `None`).
    pub max_file_node_depth: Option<u64>,

    /// Maximum length, in UTF-8 octets, for a FileNode name.  MUST be ≥ 100.
    pub max_size_file_node_name: u64,

    /// Characters forbidden in FileNode names, each character individually forbidden.
    /// `null` means the server imposes no character restrictions beyond Net-Unicode.
    ///
    /// Always serialized (as `null` when `None`).
    pub forbidden_name_chars: Option<String>,

    /// Complete names the server will not accept (compared case-insensitively).
    /// `null` means no specific names are forbidden.
    ///
    /// Always serialized (as `null` when `None`).
    pub forbidden_node_names: Option<Vec<String>>,

    /// All sort property values supported for the `"property"` field of a Comparator
    /// object in `FileNode/query`.
    pub file_node_query_sort_options: Vec<String>,

    /// If `true`, the user may create a FileNode with a null `parentId` in this account.
    pub may_create_top_level_file_node: bool,

    /// Web URL for the folder with the `"trash"` role, or `null` if unavailable.
    ///
    /// Always serialized (as `null` when `None`).
    pub web_trash_url: Option<String>,

    /// If `true`, the server treats file names as case-insensitive for all name
    /// collision checks (including the sibling uniqueness constraint and `onExists`
    /// handling).  The server preserves the original case as provided by the client.
    pub case_insensitive_names: bool,

    /// URI Template (level 1) for viewing a node on the web, with `{id}` variable.
    /// `null` if no web URL is available.
    ///
    /// Always serialized (as `null` when `None`).
    pub web_url_template: Option<String>,

    /// URI Template (level 1) for direct HTTP writes to a file node, with `{id}`
    /// variable.  `null` if direct HTTP writes are not supported.
    ///
    /// Always serialized (as `null` when `None`).
    pub web_write_url_template: Option<String>,
}
