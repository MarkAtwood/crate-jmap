//! A deployment whose `FileNodeBackend::files_rights` returns
//! `space_files_rights` reports the Space role mapping in `FileNode/get`
//! `myRights`.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! 33702a5 ("filenode Lnnn"):
//!
//! - L161: "the following table maps role permissions to the `FilesRights`
//!   flags ({{JMAP-FILENODE}}) the server reports in `myRights` and
//!   enforces on each FileNode in the Space file tree. Each row includes
//!   the grants of the rows above it. ... A flag no applicable row grants
//!   is `false`. ... `mayAddChildren` is always `false` on nodes whose
//!   `@type` does not permit children".
//! - L165-166: `"view"` "`mayRead` on every node"; `"send"` "The above,
//!   plus `mayAddChildren`".
//!
//! Records and expected rights are hand-written JSON from those lines.

mod common;

use common::{ACCOUNT, Deployment};
use jmap_filenode_server::handle_filenode_get;
use serde_json::{Value, json};

/// FilesRights with exactly the flags in `granted` true.
fn rights(granted: &[&str]) -> Value {
    let mut r = json!({
        "mayRead": false, "mayAddChildren": false, "mayRename": false,
        "mayDelete": false, "mayModifyContent": false, "mayShare": false,
    });
    for flag in granted {
        r[*flag] = json!(true);
    }
    r
}

/// L165-166 for a `"send"` member: `mayRead` and `mayAddChildren` on the
/// root and on a directory. The file has no `myRights`:
/// draft-ietf-jmap-filenode-20 §5 "A node whose @type does not permit
/// children has no myRights. The user's rights on it are those in its
/// parent's myRights". The ancestors `fetchParents` appends carry their
/// rights too. `loose` is outside every Space tree, so it keeps its stored
/// `myRights`.
#[tokio::test]
async fn get_reports_space_role_rights() {
    let deployment = Deployment::new(&["send"]);
    let args = json!({ "accountId": ACCOUNT, "ids": ["f1", "loose"], "fetchParents": true });
    let resp = handle_filenode_get(&deployment, &(), args, &[])
        .await
        .expect("get")
        .primary;
    let my_rights = |id: &str| {
        resp["list"]
            .as_array()
            .expect("list")
            .iter()
            .find(|n| n["id"] == id)
            .unwrap_or_else(|| panic!("{id} listed: {resp}"))["myRights"]
            .clone()
    };
    let read = "mayRead";
    let add = "mayAddChildren";
    assert_eq!(my_rights("root"), rights(&[read, add]), "{resp}");
    assert_eq!(my_rights("docs"), rights(&[read, add]), "{resp}");
    assert_eq!(my_rights("f1"), Value::Null, "{resp}");
    assert_eq!(my_rights("loose"), Value::Null, "{resp}");
}
