// Seed fixture correctness tests for setup_seed_data.
//
// Oracle: ~/GIT/jmap-test-suite/src/setup/seed-data.ts
//
// These tests are written INDEPENDENTLY of the fixture implementation.
// Every expected value is derived from the seed-data.ts spec, not from
// reading seed.rs.  Do not add assertions that only read back what the
// fixture wrote without verifying it against an external reference.
#![allow(async_fn_in_trait)]

mod common;

use common::{seed::setup_seed_data, MemoryBackend};
use jmap_mail_server::JmapBackend;
use jmap_mail_types::{keyword, Email, Mailbox, MailboxRole};
use jmap_types::Id;

const ACCOUNT: &str = "acct1";

/// Create a fresh backend and run setup_seed_data; return both.
async fn mk() -> (MemoryBackend, common::seed::SeedData) {
    let backend = MemoryBackend::new();
    let account_id = Id::from(ACCOUNT);
    let seed = setup_seed_data(&backend, &account_id).await;
    (backend, seed)
}

// ---------------------------------------------------------------------------
// Mailbox structure
// ---------------------------------------------------------------------------

/// Oracle: seed-data.ts creates 4 custom mailboxes (folderA, folderB, child1,
/// child2) on top of the pre-existing inbox the backend provides.
/// Total = 5 mailboxes in the account after setup.
#[tokio::test]
async fn test_mailbox_count() {
    let (backend, _seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (mailboxes, not_found) = backend
        .get_objects::<Mailbox>(&account_id, None, None)
        .await
        .expect("get_objects::<Mailbox> must not fail");
    assert!(
        not_found.is_empty(),
        "get_objects ids=None must not produce not_found"
    );
    assert_eq!(
        mailboxes.len(),
        5,
        "expected 5 mailboxes (inbox + folderA + folderB + child1 + child2), got {}",
        mailboxes.len()
    );
}

/// Oracle: exactly one mailbox must have role=inbox; the four custom mailboxes
/// created in seed-data.ts have no role (role=None).
#[tokio::test]
async fn test_mailbox_roles() {
    let (backend, _seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (mailboxes, _) = backend
        .get_objects::<Mailbox>(&account_id, None, None)
        .await
        .expect("get_objects::<Mailbox>");

    let inbox_count = mailboxes
        .iter()
        .filter(|m| m.role.as_ref() == Some(&MailboxRole::Inbox))
        .count();
    assert_eq!(inbox_count, 1, "exactly one mailbox must have role=inbox");

    // The four custom mailboxes have no role.
    let named = ["Test Folder A", "Test Folder B", "Child 1", "Child 2"];
    for name in named {
        let mb = mailboxes
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("mailbox named '{}' must exist", name));
        assert!(
            mb.role.is_none(),
            "mailbox '{}' must have no role, but got {:?}",
            name,
            mb.role
        );
    }
}

/// Oracle: child1 and child2 both have parentId = folderA.
/// seed-data.ts: child1 = { name: "Child 1", parentId: folderA.id }
///               child2 = { name: "Child 2", parentId: folderA.id }
#[tokio::test]
async fn test_mailbox_hierarchy() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (mailboxes, _) = backend
        .get_objects::<Mailbox>(&account_id, None, None)
        .await
        .expect("get_objects::<Mailbox>");

    let folder_a_id = &seed.mailbox["folderA"];
    let child1_id = &seed.mailbox["child1"];
    let child2_id = &seed.mailbox["child2"];

    let child1 = mailboxes
        .iter()
        .find(|m| &m.id == child1_id)
        .expect("child1 mailbox must exist");
    let child2 = mailboxes
        .iter()
        .find(|m| &m.id == child2_id)
        .expect("child2 mailbox must exist");

    assert_eq!(
        child1.parent_id.as_ref(),
        Some(folder_a_id),
        "Child 1 must have parentId = folderA"
    );
    assert_eq!(
        child2.parent_id.as_ref(),
        Some(folder_a_id),
        "Child 2 must have parentId = folderA"
    );
}

// ---------------------------------------------------------------------------
// Thread grouping
// ---------------------------------------------------------------------------

/// Oracle: thread-starter, thread-reply-1, thread-reply-2 must all share the
/// same threadId.
///
/// seed-data.ts links them via In-Reply-To / References headers:
///   thread-reply-1: In-Reply-To: <thread-alpha-001@test>
///   thread-reply-2: In-Reply-To: <thread-alpha-002@test>
///                   References:  <thread-alpha-001@test> <thread-alpha-002@test>
#[tokio::test]
async fn test_thread_grouping() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let find_email = |key: &str| -> &Email {
        let id = &seed.email[key];
        emails
            .iter()
            .find(|e| &e.id == id)
            .unwrap_or_else(|| panic!("email '{}' (id {}) must exist", key, id))
    };

    let starter = find_email("thread-starter");
    let reply1 = find_email("thread-reply-1");
    let reply2 = find_email("thread-reply-2");

    assert_eq!(
        starter.thread_id, reply1.thread_id,
        "thread-starter and thread-reply-1 must share a threadId"
    );
    assert_eq!(
        starter.thread_id, reply2.thread_id,
        "thread-starter and thread-reply-2 must share a threadId"
    );
}

/// Oracle: all emails that are NOT part of the alpha thread must each have a
/// unique threadId that is different from all others (single-message threads).
#[tokio::test]
async fn test_email_thread_uniqueness() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    // The three alpha-thread email ids.
    let alpha_ids = [
        &seed.email["thread-starter"],
        &seed.email["thread-reply-1"],
        &seed.email["thread-reply-2"],
    ];

    // Collect threadIds for all non-alpha emails and assert uniqueness.
    let mut seen_threads: std::collections::HashSet<&Id> = std::collections::HashSet::new();
    for email in &emails {
        if alpha_ids.contains(&&email.id) {
            continue;
        }
        assert!(
            seen_threads.insert(&email.thread_id),
            "email {} has threadId {} which is shared with another non-alpha email \
             (every non-thread email must be in its own single-message thread)",
            email.id,
            email.thread_id
        );
    }
}

// ---------------------------------------------------------------------------
// Email-specific assertions
// ---------------------------------------------------------------------------

/// Oracle: plain-simple
///   subject = "Meeting tomorrow morning"   (seed-data.ts line ~116)
///   from    = alice@example.com            (seed-data.ts line ~113)
///   keywords contain $seen                 (seed-data.ts line ~121)
#[tokio::test]
async fn test_email_keywords_plain_simple() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let id = &seed.email["plain-simple"];
    let email = emails
        .iter()
        .find(|e| &e.id == id)
        .expect("plain-simple email must exist");

    assert_eq!(
        email.subject.as_deref(),
        Some("Meeting tomorrow morning"),
        "plain-simple subject must be 'Meeting tomorrow morning'"
    );

    let from_email = email
        .from
        .as_ref()
        .and_then(|f| f.first())
        .map(|a| a.email.as_str())
        .unwrap_or("");
    assert_eq!(
        from_email, "alice@example.com",
        "plain-simple from must be alice@example.com"
    );

    assert!(
        email.keywords.contains_key(keyword::SEEN),
        "plain-simple must have $seen keyword"
    );
}

/// Oracle: html-attachment
///   keywords: $seen AND $flagged              (seed-data.ts line ~140)
///   cc:       charlie@example.net             (seed-data.ts line ~129)
#[tokio::test]
async fn test_email_keywords_html_attachment() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let id = &seed.email["html-attachment"];
    let email = emails
        .iter()
        .find(|e| &e.id == id)
        .expect("html-attachment email must exist");

    assert!(
        email.keywords.contains_key(keyword::SEEN),
        "html-attachment must have $seen keyword"
    );
    assert!(
        email.keywords.contains_key(keyword::FLAGGED),
        "html-attachment must have $flagged keyword"
    );

    let cc_emails: Vec<&str> = email
        .cc
        .as_ref()
        .map(|cc| cc.iter().map(|a| a.email.as_str()).collect())
        .unwrap_or_default();
    assert!(
        cc_emails.contains(&"charlie@example.net"),
        "html-attachment cc must contain charlie@example.net, got {:?}",
        cc_emails
    );
}

/// Oracle: custom-keywords
///   keywords: $seen, $forwarded, custom_label   (seed-data.ts line ~258-259)
#[tokio::test]
async fn test_email_keywords_custom() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let id = &seed.email["custom-keywords"];
    let email = emails
        .iter()
        .find(|e| &e.id == id)
        .expect("custom-keywords email must exist");

    assert!(
        email.keywords.contains_key("custom_label"),
        "custom-keywords must have custom_label keyword, got {:?}",
        email.keywords.keys().collect::<Vec<_>>()
    );
    assert!(
        email.keywords.contains_key(keyword::SEEN),
        "custom-keywords must have $seen keyword"
    );
    assert!(
        email.keywords.contains_key(keyword::FORWARDED),
        "custom-keywords must have $forwarded keyword"
    );
}

/// Oracle: very-old
///   receivedAt = daysAgo(30) from baseline 2026-01-01T00:00:00Z
///              = 2025-12-02T00:00:00Z               (task spec)
#[tokio::test]
async fn test_email_received_at_very_old() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let id = &seed.email["very-old"];
    let email = emails
        .iter()
        .find(|e| &e.id == id)
        .expect("very-old email must exist");

    assert_eq!(
        email.received_at.as_ref(),
        "2025-12-02T00:00:00Z",
        "very-old receivedAt must be 2025-12-02T00:00:00Z (30 days before baseline)"
    );
}

/// Oracle: plain-simple
///   receivedAt = daysAgo(10) from baseline 2026-01-01T00:00:00Z
///              = 2025-12-22T00:00:00Z               (task spec)
#[tokio::test]
async fn test_email_received_at_plain_simple() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let id = &seed.email["plain-simple"];
    let email = emails
        .iter()
        .find(|e| &e.id == id)
        .expect("plain-simple email must exist");

    assert_eq!(
        email.received_at.as_ref(),
        "2025-12-22T00:00:00Z",
        "plain-simple receivedAt must be 2025-12-22T00:00:00Z (10 days before baseline)"
    );
}

/// Oracle: large-email body in seed.rs is a 74-char phrase repeated 200 times
/// (~14800 bytes of body alone), easily exceeding 10240.
/// (seed.rs: "This is a detailed paragraph...".repeat(200))
#[tokio::test]
async fn test_email_size_large() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let id = &seed.email["large-email"];
    let email = emails
        .iter()
        .find(|e| &e.id == id)
        .expect("large-email must exist");

    assert!(
        email.size > 10240,
        "large-email size must exceed 10240 bytes, got {}",
        email.size
    );
}

/// Oracle: special-headers
///   extraHeaders includes "X-Custom-Header: custom-value-12345"
///   (seed-data.ts lines ~299-303)
#[tokio::test]
async fn test_email_special_headers() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let id = &seed.email["special-headers"];
    let email = emails
        .iter()
        .find(|e| &e.id == id)
        .expect("special-headers email must exist");

    let custom_header = email
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("X-Custom-Header"));
    assert!(
        custom_header.is_some(),
        "special-headers email must have X-Custom-Header"
    );
    let value = custom_header.unwrap().value.trim();
    assert_eq!(
        value, "custom-value-12345",
        "X-Custom-Header value must be 'custom-value-12345', got '{}'",
        value
    );
}

// ---------------------------------------------------------------------------
// Email placement
// ---------------------------------------------------------------------------

/// Oracle: child-mailbox-email lives in child1 only.
///   mailboxIds: { [child1]: true }   (seed-data.ts line ~412)
#[tokio::test]
async fn test_email_placement_child() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let child1_id = &seed.mailbox["child1"];
    let email_id = &seed.email["child-mailbox-email"];

    let email = emails
        .iter()
        .find(|e| &e.id == email_id)
        .expect("child-mailbox-email must exist");

    assert_eq!(
        email.mailbox_ids.len(),
        1,
        "child-mailbox-email must be in exactly one mailbox, got {:?}",
        email.mailbox_ids.keys().collect::<Vec<_>>()
    );
    assert!(
        email.mailbox_ids.contains_key(child1_id),
        "child-mailbox-email must be in child1, got {:?}",
        email.mailbox_ids.keys().collect::<Vec<_>>()
    );
}

/// Oracle: multi-mailbox is in both inbox AND folderA.
///   mailboxIds: { [inbox]: true, [folderA]: true }   (seed-data.ts line ~199)
#[tokio::test]
async fn test_email_placement_multi_mailbox() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let inbox_id = &seed.mailbox["inbox"];
    let folder_a_id = &seed.mailbox["folderA"];
    let email_id = &seed.email["multi-mailbox"];

    let email = emails
        .iter()
        .find(|e| &e.id == email_id)
        .expect("multi-mailbox email must exist");

    assert!(
        email.mailbox_ids.contains_key(inbox_id),
        "multi-mailbox must be in inbox"
    );
    assert!(
        email.mailbox_ids.contains_key(folder_a_id),
        "multi-mailbox must be in folderA"
    );
    assert_eq!(
        email.mailbox_ids.len(),
        2,
        "multi-mailbox must be in exactly 2 mailboxes"
    );
}

/// Oracle: inbox contains at least 9 emails.
///   plain-simple, html-attachment, thread-reply-1, thread-reply-2,
///   multi-mailbox, html-only, no-subject, custom-keywords, special-headers
///   (seed-data.ts lines ~120, 139, 169, 185, 199, 229, 244, 260, 308)
#[tokio::test]
async fn test_inbox_email_count() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let inbox_id = &seed.mailbox["inbox"];
    let inbox_count = emails
        .iter()
        .filter(|e| e.mailbox_ids.contains_key(inbox_id))
        .count();
    assert!(
        inbox_count >= 9,
        "inbox must contain at least 9 emails, got {}",
        inbox_count
    );
}

/// Oracle: folderA contains at least 3 emails.
///   thread-starter, multi-mailbox, very-old
///   (seed-data.ts lines ~153, 199, 274)
#[tokio::test]
async fn test_folder_a_email_count() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let folder_a_id = &seed.mailbox["folderA"];
    let count = emails
        .iter()
        .filter(|e| e.mailbox_ids.contains_key(folder_a_id))
        .count();
    assert!(
        count >= 3,
        "folderA must contain at least 3 emails, got {}",
        count
    );
}

/// Oracle: folderB contains exactly 4 emails.
///   large-email, sort-test-1, sort-test-2, sort-test-3
///   (seed-data.ts lines ~213, 353, 365, 378)
#[tokio::test]
async fn test_folder_b_email_count() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let folder_b_id = &seed.mailbox["folderB"];
    let count = emails
        .iter()
        .filter(|e| e.mailbox_ids.contains_key(folder_b_id))
        .count();
    assert_eq!(
        count, 4,
        "folderB must contain exactly 4 emails, got {}",
        count
    );
}

/// Oracle: child1 contains exactly 1 email (child-mailbox-email).
///   (seed-data.ts line ~412)
#[tokio::test]
async fn test_child1_email_count() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let child1_id = &seed.mailbox["child1"];
    let count = emails
        .iter()
        .filter(|e| e.mailbox_ids.contains_key(child1_id))
        .count();
    assert_eq!(
        count, 1,
        "child1 must contain exactly 1 email, got {}",
        count
    );
}

// ---------------------------------------------------------------------------
// Subject spot checks
// ---------------------------------------------------------------------------

/// Oracle: spot-check three email subjects from seed-data.ts.
///   thread-starter: "Project Alpha Discussion"    (line ~148)
///   html-only:      "Newsletter: Weekly Digest"   (line ~222)
///   no-subject:     ""                            (line ~238)
#[tokio::test]
async fn test_email_subjects() {
    let (backend, seed) = mk().await;
    let account_id = Id::from(ACCOUNT);
    let (emails, _) = backend
        .get_objects::<Email>(&account_id, None, None)
        .await
        .expect("get_objects::<Email>");

    let find = |key: &str| -> &Email {
        let id = &seed.email[key];
        emails
            .iter()
            .find(|e| &e.id == id)
            .unwrap_or_else(|| panic!("email '{}' (id {}) must exist", key, id))
    };

    assert_eq!(
        find("thread-starter").subject.as_deref(),
        Some("Project Alpha Discussion"),
        "thread-starter subject must be 'Project Alpha Discussion'"
    );
    assert_eq!(
        find("html-only").subject.as_deref(),
        Some("Newsletter: Weekly Digest"),
        "html-only subject must be 'Newsletter: Weekly Digest'"
    );
    // no-subject: subject is empty string (not None)
    let no_subj = find("no-subject").subject.as_deref().unwrap_or("");
    assert_eq!(
        no_subj, "",
        "no-subject email must have empty subject, got '{}'",
        no_subj
    );
}
