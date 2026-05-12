//! Fetch and print Mailbox objects from a synthetic JMAP server.
//!
//! Spins up an in-process `wiremock` server that returns a hand-written
//! `Mailbox/get` response covering the common RFC 8621 §2.1 mailbox
//! roles (`inbox`, `sent`, `drafts`, `trash`, `junk`, `archive`). The
//! client side calls
//! [`jmap_mail_client::SessionClient::mailbox_get`] with `ids: None`
//! to fetch all mailboxes and prints a compact summary.
//!
//! Demonstrates the round-trip:
//! 1. `JmapClient::new_plain` against a `wiremock::MockServer` URL,
//! 2. construct a `Session` shaped per RFC 8620 §2,
//! 3. `JmapMailExt::with_mail_session` → `SessionClient`,
//! 4. `mailbox_get(None, None)` → `GetResponse<Mailbox>`.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example mailbox_list -p jmap-mail-client
//! ```
//!
//! NOT FOR PRODUCTION — synthetic mock-server fixtures only, no auth, no TLS.

use jmap_base_client::{ClientConfig, JmapClient, NoneAuth};
use jmap_mail_client::JmapMailExt;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Hand-written `Mailbox/get` response covering the standard RFC 8621
/// §2.1 mailbox roles plus one role-less custom folder. Counts are
/// arbitrary plausible values.
fn mailbox_get_response_body() -> serde_json::Value {
    let full_rights = json!({
        "mayReadItems": true,
        "mayAddItems": true,
        "mayRemoveItems": true,
        "maySetSeen": true,
        "maySetKeywords": true,
        "mayCreateChild": true,
        "mayRename": true,
        "mayDelete": true,
        "maySubmit": true,
    });
    // Inbox is special — most servers disallow rename/delete on it.
    let mut inbox_rights = full_rights.clone();
    inbox_rights["mayRename"] = json!(false);
    inbox_rights["mayDelete"] = json!(false);

    let mk_mailbox = |id, name, role, sort_order, total, unread, rights: &serde_json::Value| {
        let mut m = json!({
            "id": id,
            "name": name,
            "parentId": null,
            "sortOrder": sort_order,
            "totalEmails": total,
            "unreadEmails": unread,
            "totalThreads": total,
            "unreadThreads": unread,
            "myRights": rights,
            "isSubscribed": true,
        });
        if let Some(r) = role {
            m["role"] = json!(r);
        }
        m
    };

    json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Mailbox/get",
            {
                "accountId": "A13824",
                "state": "mb-7",
                "list": [
                    mk_mailbox("mb-1", "Inbox", Some("inbox"), 0, 142, 7, &inbox_rights),
                    mk_mailbox("mb-2", "Sent", Some("sent"), 10, 89, 0, &full_rights),
                    mk_mailbox("mb-3", "Drafts", Some("drafts"), 20, 3, 0, &full_rights),
                    mk_mailbox("mb-4", "Trash", Some("trash"), 30, 28, 0, &full_rights),
                    mk_mailbox("mb-5", "Junk", Some("junk"), 40, 0, 0, &full_rights),
                    mk_mailbox("mb-6", "Archive", Some("archive"), 50, 1207, 0, &full_rights),
                    mk_mailbox("mb-7", "Receipts", None, 60, 41, 0, &full_rights),
                ],
                "notFound": null,
            },
            "r1"
        ]]
    })
}

/// Build a [`jmap_base_client::Session`] whose `apiUrl` points at the mock
/// server. Mirrors the test-helpers session shape (RFC 8620 §2.1).
fn make_session(server: &MockServer) -> jmap_base_client::Session {
    let json = json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:mail": {}
        },
        "accounts": {
            "A13824": {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:mail": {} }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:mail": "A13824" },
        "username": "john@example.com",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}", server.uri()),
        "state": "s1"
    });
    serde_json::from_value(json).expect("Session must deserialize from RFC 8620 §2.1 shape")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mailbox_get_response_body()))
        .mount(&server)
        .await;

    let client = JmapClient::new_plain(NoneAuth, &server.uri(), ClientConfig::default())?;
    let sc = client.with_mail_session(make_session(&server));

    let resp = sc.mailbox_get(None, None).await?;
    println!(
        "account {} state {} — {} mailbox(es), {} not_found",
        resp.account_id,
        resp.state.as_ref(),
        resp.list.len(),
        resp.not_found.as_ref().map_or(0, Vec::len),
    );
    println!(
        "{:<10} {:<10} {:<10} {:>8} {:>8}",
        "id", "name", "role", "total", "unread"
    );
    for m in &resp.list {
        let role: &str = m.role.as_ref().map_or("(none)", |r| r.to_wire_str());
        println!(
            "{:<10} {:<10} {:<10} {:>8} {:>8}",
            m.id.as_ref(),
            m.name,
            role,
            m.total_emails,
            m.unread_emails,
        );
    }
    Ok(())
}
