//! Per-user rights and discoverability for the memory backend
//! (draft-ietf-jmap-filenode-20 §5 "Access Control").
//!
//! The account owner, and a caller with no principal, "has all rights" on
//! every node and can discover all of them. Every other user's rights come
//! from `shareWith`: "a node's myRights is the union of the rights granted
//! to the user by the shareWith entries on the node and on all its
//! ancestors", and a File or `SymLink` takes "those in its parent's
//! myRights". A node is discoverable when it or an ancestor grants
//! `mayRead`, or when it or a descendant "has a shareWith entry that grants
//! the user any right".

use std::collections::{BTreeSet, HashMap, HashSet};

use jmap_filenode_types::{FileNode, FilesRights, NodeType};
use jmap_types::Id;

/// What one user sees of one discoverable node.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Sight {
    /// The user's rights on the node: its own for a Directory, its
    /// parent's for a File or `SymLink` (-20 §5).
    pub(super) rights: FilesRights,
}

impl Sight {
    /// Whether the user may read every property of the node (-20 §3.1.2
    /// `mayRead`).
    pub(super) fn readable(&self) -> bool {
        self.rights.may_read
    }
}

/// The nodes a user can discover, with the rights the user holds on each.
/// A node missing from the map is not discoverable by the user.
pub(super) type View = HashMap<Id, Sight>;

/// Who a read or write is for, within one account.
#[derive(Clone, Copy, Debug)]
pub(super) struct Viewer<'a> {
    /// The caller's principal, `None` when the caller carries none.
    pub(super) user: Option<&'a Id>,
    /// The account owner.
    pub(super) owner: &'a Id,
    /// Whether [`FileNode`] has the draft-ietf-jmap-metadata-05 properties, so
    /// that `maySetMetadata` is present in `myRights` (-05 §4.1).
    pub(super) metadata: bool,
}

impl Viewer<'_> {
    /// The sharee this viewer stands for, or `None` for the account owner
    /// and for a caller without a principal, who have all rights.
    pub(super) fn sharee(&self) -> Option<&Id> {
        self.user.filter(|u| *u != self.owner)
    }
}

/// Every right, as the account owner holds them (-20 §5: the owner "has
/// all rights").
pub(super) fn all_rights(metadata: bool) -> FilesRights {
    let mut r = FilesRights::default();
    r.may_read = true;
    r.may_add_children = true;
    r.may_rename = true;
    r.may_delete = true;
    r.may_modify_content = true;
    r.may_share = true;
    r.may_set_metadata = metadata.then_some(true);
    r
}

/// Whether `r` grants any right: the test of the second discoverability
/// rule ("a shareWith entry that grants the user any right", -20 §5).
fn grants_any(r: &FilesRights) -> bool {
    r.may_read
        || r.may_add_children
        || r.may_rename
        || r.may_delete
        || r.may_modify_content
        || r.may_share
        || r.may_set_metadata == Some(true)
}

/// `acc` with every right `r` grants added: the union of -20 §5.
fn add_rights(acc: &mut FilesRights, r: &FilesRights) {
    acc.may_read |= r.may_read;
    acc.may_add_children |= r.may_add_children;
    acc.may_rename |= r.may_rename;
    acc.may_delete |= r.may_delete;
    acc.may_modify_content |= r.may_modify_content;
    acc.may_share |= r.may_share;
    if r.may_set_metadata == Some(true) {
        acc.may_set_metadata = Some(true);
    }
}

/// The `shareWith` entry `node` holds for `user`, if any.
fn entry_for<'a>(node: &'a FileNode, user: &Id) -> Option<&'a FilesRights> {
    node.share_with.as_ref().and_then(|s| s.get(user))
}

/// The ids of every user that a `shareWith` among `nodes` names.
pub(super) fn sharees(nodes: &HashMap<Id, FileNode>) -> BTreeSet<Id> {
    nodes
        .values()
        .filter_map(|n| n.share_with.as_ref())
        .flat_map(|s| s.keys().cloned())
        .collect()
}

/// What `viewer` can discover among `nodes`, and its rights on each node.
pub(super) fn view(nodes: &HashMap<Id, FileNode>, viewer: Viewer<'_>) -> View {
    let Some(user) = viewer.sharee() else {
        let sight = Sight {
            rights: all_rights(viewer.metadata),
        };
        return nodes.keys().map(|id| (id.clone(), sight.clone())).collect();
    };

    // Rights inherited down the tree: the union over the node and its
    // ancestors. A cycle in a malformed tree ends the walk.
    let mut rights: HashMap<&Id, FilesRights> = HashMap::new();
    for start in nodes.keys() {
        let mut chain: Vec<&Id> = Vec::new();
        let mut seen: HashSet<&Id> = HashSet::new();
        let mut at = Some(start);
        let mut base = FilesRights::default();
        while let Some(id) = at {
            if let Some(known) = rights.get(id) {
                base = known.clone();
                break;
            }
            let Some(node) = nodes.get(id) else { break };
            if !seen.insert(id) {
                break;
            }
            chain.push(&node.id);
            at = node.parent_id.as_ref();
        }
        for id in chain.into_iter().rev() {
            if let Some(entry) = nodes.get(id).and_then(|n| entry_for(n, user)) {
                add_rights(&mut base, entry);
            }
            rights.insert(id, base.clone());
        }
    }

    // The second rule: a node with a granting entry, and its ancestors.
    let mut via_share: HashSet<&Id> = HashSet::new();
    for node in nodes.values() {
        if !entry_for(node, user).is_some_and(grants_any) {
            continue;
        }
        let mut at = Some(&node.id);
        while let Some(id) = at {
            if !via_share.insert(id) {
                break;
            }
            at = nodes.get(id).and_then(|n| n.parent_id.as_ref());
        }
    }

    let mut out = View::new();
    for (id, mut r) in rights {
        if !(r.may_read || via_share.contains(id)) {
            continue;
        }
        // -05 §4.1: `maySetMetadata` is present exactly when FileNode has
        // metadata.
        r.may_set_metadata = viewer.metadata.then_some(r.may_set_metadata == Some(true));
        out.insert(id.clone(), Sight { rights: r });
    }
    out
}

/// Whether `node` has a `myRights` of its own: only a node whose `@type`
/// permits children, of the types this backend stores only a Directory
/// (-20 §3.1.2).
fn is_container(node: &FileNode) -> bool {
    node.node_type == Some(NodeType::Directory)
}

/// `node` as a user with `sight` of it receives it from `FileNode/get`.
///
/// - `myRights` is the user's rights, on a Directory only (-20 §5: "A node
///   whose @type does not permit children has no myRights").
/// - `shareWith` "is null if the user requesting the object does not have
///   myRights.mayShare" (-20 §3.1.2).
/// - Without `mayRead`, only "id, @type, name, parentId, myRights,
///   isSubscribed, and shareWith" keep their values; every other property
///   is withheld (-20 §5), and `handle_filenode_get` sends each one as
///   null.
/// - `isSubscribed` is the user's own value, `subscribed` (-20 §3.1.2:
///   "This property is stored per user").
pub(super) fn shown(node: &FileNode, sight: &Sight, subscribed: Option<bool>) -> FileNode {
    let mut out = node.clone();
    if is_container(node) {
        out.my_rights = Some(sight.rights.clone());
        if !sight.rights.may_share {
            out.share_with = None;
        }
        out.is_subscribed = subscribed.or(node.is_subscribed);
    } else {
        out.my_rights = None;
    }
    if !sight.readable() {
        out.blob_id = None;
        out.target = None;
        out.size = None;
        out.sha256 = None;
        out.media_type = None;
        out.created = None;
        out.modified = None;
        out.accessed = None;
        out.changed = None;
        out.executable = None;
        out.role = None;
        out.space_id = None;
        out.metadata = None;
        out.private_metadata = None;
        out.extra = serde_json::Map::new();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(v: serde_json::Value) -> FileNode {
        serde_json::from_value(v).unwrap_or_else(|e| panic!("fixture: {e}"))
    }

    /// The Unix home layout of -20 §5: "/home/alice/shared/forBob", shared
    /// with Bob, beside "/home/alice/private".
    fn home() -> HashMap<Id, FileNode> {
        let read = json!({ "mayRead": true, "mayAddChildren": false, "mayRename": false,
                           "mayDelete": false, "mayModifyContent": false, "mayShare": false });
        [
            json!({ "id": "home", "@type": "Directory", "name": "home", "parentId": null }),
            json!({ "id": "alice", "@type": "Directory", "name": "alice", "parentId": "home" }),
            json!({ "id": "shared", "@type": "Directory", "name": "shared", "parentId": "alice" }),
            json!({ "id": "forBob", "@type": "Directory", "name": "forBob", "parentId": "shared",
                    "shareWith": { "bob": read } }),
            json!({ "id": "file1", "@type": "File", "name": "file1.jpg", "parentId": "forBob",
                    "blobId": "B1" }),
            json!({ "id": "private", "@type": "Directory", "name": "private", "parentId": "alice" }),
            json!({ "id": "loose", "@type": "File", "name": "loose", "parentId": null,
                    "blobId": "B2" }),
        ]
        .into_iter()
        .map(node)
        .map(|n| (n.id.clone(), n))
        .collect()
    }

    /// -20 §5: "This leads to a sharee seeing the full path to anything
    /// that they have access to, but nothing else", and "A node that is
    /// discoverable only through the second rule has mayRead false".
    #[test]
    fn sharee_sees_path_to_share_and_nothing_else() {
        let nodes = home();
        let owner = Id::from("alice-principal");
        let bob = Id::from("bob");
        let v = view(
            &nodes,
            Viewer {
                user: Some(&bob),
                owner: &owner,
                metadata: false,
            },
        );
        let mut seen: Vec<&str> = v.keys().map(Id::as_ref).collect();
        seen.sort_unstable();
        assert_eq!(seen, ["alice", "file1", "forBob", "home", "shared"]);
        assert!(!v[&Id::from("home")].readable());
        assert!(!v[&Id::from("shared")].readable());
        assert!(v[&Id::from("forBob")].readable());
        // -20 §5: a File's rights "are those in its parent's myRights".
        assert!(v[&Id::from("file1")].readable());
    }

    /// -20 §5: a top-level File "is discoverable only by the account owner,
    /// who has all rights on it".
    #[test]
    fn owner_sees_everything_with_all_rights() {
        let nodes = home();
        let owner = Id::from("alice-principal");
        let v = view(
            &nodes,
            Viewer {
                user: Some(&owner),
                owner: &owner,
                metadata: false,
            },
        );
        assert_eq!(v.len(), nodes.len());
        assert_eq!(v[&Id::from("loose")].rights, all_rights(false));
    }
}
