//! Canonical seed-data fixture for integration tests.
//!
//! [`setup_seed_data`] populates a [`super::MemoryBackend`] with a fixed set of
//! mailboxes and emails derived from the jmap-test-suite seed-data spec.
//! All timestamps are relative to the fixed baseline `2026-01-01T00:00:00Z`
//! so tests are fully deterministic.

use std::collections::HashMap;

use jmap_mail_types::{keyword, Keyword, Mailbox, MailboxRights, MailboxRole};
use jmap_types::{Id, UTCDate};

use super::MemoryBackend;
use jmap_mail_server::MailBackend;

// ---------------------------------------------------------------------------
// Public return type
// ---------------------------------------------------------------------------

/// IDs assigned by the backend after seeding.
///
/// All maps are keyed by the fixture's logical name (e.g. `"inbox"`,
/// `"plain-simple"`, `"thread-alpha"`).
pub struct SeedData {
    /// Logical name → assigned mailbox Id.
    pub mailbox: HashMap<&'static str, Id>,
    /// Logical name → assigned email Id.
    pub email: HashMap<&'static str, Id>,
    /// Logical name → assigned thread Id.
    pub thread: HashMap<&'static str, Id>,
}

// ---------------------------------------------------------------------------
// Timestamp helpers (all relative to baseline 2026-01-01T00:00:00Z)
// ---------------------------------------------------------------------------

/// Pre-computed UTC timestamps.  The baseline is 2026-01-01T00:00:00Z.
/// `days_ago(n)` = baseline minus exactly n × 86400 seconds.
/// `hours_ago_from_days_ago_1` = days_ago(1) minus 1 hour.
mod ts {
    pub const DAYS_AGO_30: &str = "2025-12-02T00:00:00Z";
    pub const DAYS_AGO_10: &str = "2025-12-22T00:00:00Z";
    pub const DAYS_AGO_9: &str = "2025-12-23T00:00:00Z";
    pub const DAYS_AGO_8: &str = "2025-12-24T00:00:00Z";
    pub const DAYS_AGO_7: &str = "2025-12-25T00:00:00Z";
    pub const DAYS_AGO_6: &str = "2025-12-26T00:00:00Z";
    pub const DAYS_AGO_5: &str = "2025-12-27T00:00:00Z";
    pub const DAYS_AGO_4: &str = "2025-12-28T00:00:00Z";
    pub const DAYS_AGO_3: &str = "2025-12-29T00:00:00Z";
    pub const DAYS_AGO_2: &str = "2025-12-30T00:00:00Z";
    pub const DAYS_AGO_1: &str = "2025-12-31T00:00:00Z";
    /// days_ago(1) minus 1 hour = 2025-12-30T23:00:00Z
    pub const DAYS_AGO_1_MINUS_1H: &str = "2025-12-30T23:00:00Z";
}

fn date(s: &str) -> UTCDate {
    UTCDate::from(s)
}

// ---------------------------------------------------------------------------
// Keyword helpers
// ---------------------------------------------------------------------------

fn kw(name: &str) -> Keyword {
    Keyword::from(name)
}

fn seen() -> Keyword {
    kw(keyword::SEEN)
}
fn flagged() -> Keyword {
    kw(keyword::FLAGGED)
}
fn answered() -> Keyword {
    kw(keyword::ANSWERED)
}
fn forwarded() -> Keyword {
    kw(keyword::FORWARDED)
}

// ---------------------------------------------------------------------------
// RFC 5322 message builders
// ---------------------------------------------------------------------------

/// Build a minimal plain-text RFC 5322 message.
///
/// Headers are separated from the body by a single CRLF blank line.
/// All header lines use CRLF terminators per RFC 5322 §2.1.
fn plain_message(headers: &[(&str, &str)], body: &str) -> Vec<u8> {
    let mut out = String::new();
    for (name, value) in headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out.push_str(body);
    out.into_bytes()
}

/// Build a `multipart/mixed` message with an HTML text part and a base64
/// attachment part.
fn multipart_mixed_html_pdf(
    headers: &[(&str, &str)],
    html_body: &str,
    attachment_name: &str,
    attachment_b64: &str,
) -> Vec<u8> {
    let boundary = "----=_Part_fixture_boundary_001";
    let mut out = String::new();
    for (name, value) in headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str(&format!(
        "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n"
    ));
    out.push_str("\r\n");
    // HTML part
    out.push_str(&format!("--{boundary}\r\n"));
    out.push_str("Content-Type: text/html; charset=UTF-8\r\n");
    out.push_str("\r\n");
    out.push_str(html_body);
    out.push_str("\r\n");
    // PDF attachment part
    out.push_str(&format!("--{boundary}\r\n"));
    out.push_str(&format!(
        "Content-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"{attachment_name}\"\r\nContent-Transfer-Encoding: base64\r\n"
    ));
    out.push_str("\r\n");
    out.push_str(attachment_b64);
    out.push_str("\r\n");
    out.push_str(&format!("--{boundary}--\r\n"));
    out.into_bytes()
}

/// Build a `multipart/alternative` message with plain-text and HTML parts.
fn multipart_alternative(headers: &[(&str, &str)], plain_body: &str, html_body: &str) -> Vec<u8> {
    let boundary = "----=_Part_fixture_boundary_002";
    let mut out = String::new();
    for (name, value) in headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str(&format!(
        "Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n"
    ));
    out.push_str("\r\n");
    // Plain part
    out.push_str(&format!("--{boundary}\r\n"));
    out.push_str("Content-Type: text/plain; charset=UTF-8\r\n");
    out.push_str("\r\n");
    out.push_str(plain_body);
    out.push_str("\r\n");
    // HTML part
    out.push_str(&format!("--{boundary}\r\n"));
    out.push_str("Content-Type: text/html; charset=UTF-8\r\n");
    out.push_str("\r\n");
    out.push_str(html_body);
    out.push_str("\r\n");
    out.push_str(&format!("--{boundary}--\r\n"));
    out.into_bytes()
}

// ---------------------------------------------------------------------------
// setup_seed_data
// ---------------------------------------------------------------------------

/// Populate `backend` with the canonical seed-data fixture.
///
/// Creates 5 mailboxes and 16 emails. Returns the assigned Ids for all
/// objects so callers can reference them in assertions.
///
/// # Panics
///
/// Panics if any backend operation fails — this is test fixture setup code
/// where a failure is a bug in the test environment, not in the code under test.
pub async fn setup_seed_data(backend: &MemoryBackend, account_id: &Id) -> SeedData {
    let mut mailboxes: HashMap<&'static str, Id> = HashMap::new();
    let mut emails: HashMap<&'static str, Id> = HashMap::new();
    let mut threads: HashMap<&'static str, Id> = HashMap::new();

    // -----------------------------------------------------------------------
    // Mailboxes
    // -----------------------------------------------------------------------

    // inbox — role=inbox
    let mut inbox_mb = Mailbox::new(
        Id::from("placeholder"),
        "Inbox",
        10,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        true,
    );
    inbox_mb.role = Some(MailboxRole::Inbox);
    let (inbox_id, _) = backend
        .create_object::<Mailbox>(account_id, "c-inbox", inbox_mb)
        .await
        .expect("setup: create inbox");
    mailboxes.insert("inbox", inbox_id.clone());

    // folderA
    let folder_a_mb = Mailbox::new(
        Id::from("placeholder"),
        "Test Folder A",
        20,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        true,
    );
    let (folder_a_id, _) = backend
        .create_object::<Mailbox>(account_id, "c-folderA", folder_a_mb)
        .await
        .expect("setup: create folderA");
    mailboxes.insert("folderA", folder_a_id.clone());

    // folderB
    let folder_b_mb = Mailbox::new(
        Id::from("placeholder"),
        "Test Folder B",
        30,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        true,
    );
    let (folder_b_id, _) = backend
        .create_object::<Mailbox>(account_id, "c-folderB", folder_b_mb)
        .await
        .expect("setup: create folderB");
    mailboxes.insert("folderB", folder_b_id.clone());

    // child1 — parent=folderA
    let mut child1_mb = Mailbox::new(
        Id::from("placeholder"),
        "Child 1",
        10,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        true,
    );
    child1_mb.parent_id = Some(folder_a_id.clone());
    let (child1_id, _) = backend
        .create_object::<Mailbox>(account_id, "c-child1", child1_mb)
        .await
        .expect("setup: create child1");
    mailboxes.insert("child1", child1_id.clone());

    // child2 — parent=folderA
    let mut child2_mb = Mailbox::new(
        Id::from("placeholder"),
        "Child 2",
        20,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        true,
    );
    child2_mb.parent_id = Some(folder_a_id.clone());
    let (child2_id, _) = backend
        .create_object::<Mailbox>(account_id, "c-child2", child2_mb)
        .await
        .expect("setup: create child2");
    mailboxes.insert("child2", child2_id.clone());

    // -----------------------------------------------------------------------
    // Email 1: plain-simple
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Alice Sender <alice@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Meeting tomorrow morning"),
            ("Message-ID", "<plain-simple-001@test>"),
        ],
        "Let's meet tomorrow at 9am in the conference room.",
    );
    let blob_id = Id::from("blob-plain-simple");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[inbox_id.clone()],
            &[seen()],
            Some(&date(ts::DAYS_AGO_10)),
        )
        .await
        .expect("setup: import plain-simple");
    emails.insert("plain-simple", email_id);
    threads
        .entry("plain-simple")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 2: html-attachment
    // -----------------------------------------------------------------------

    let pdf_b64 =
        "JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2Jq\r\n";
    let bytes = multipart_mixed_html_pdf(
        &[
            ("From", "Bob Jones <bob@example.org>"),
            ("To", "testuser@example.com"),
            ("Cc", "charlie@example.net"),
            ("Subject", "Q3 Financial Report"),
            ("Message-ID", "<html-attach-001@test>"),
        ],
        "<html><body><h1>Q3 Report</h1><p>Please find the report attached.</p></body></html>",
        "report.pdf",
        pdf_b64,
    );
    let blob_id = Id::from("blob-html-attachment");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[inbox_id.clone()],
            &[seen(), flagged()],
            Some(&date(ts::DAYS_AGO_9)),
        )
        .await
        .expect("setup: import html-attachment");
    emails.insert("html-attachment", email_id);
    threads
        .entry("html-attachment")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 3: thread-starter
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "testuser@example.com"),
            ("To", "alice@example.com"),
            ("Subject", "Project Alpha Discussion"),
            ("Message-ID", "<thread-alpha-001@test>"),
        ],
        "I'd like to discuss the Project Alpha timeline.",
    );
    let blob_id = Id::from("blob-thread-starter");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[folder_a_id.clone()],
            &[seen()],
            Some(&date(ts::DAYS_AGO_8)),
        )
        .await
        .expect("setup: import thread-starter");
    emails.insert("thread-starter", email_id);
    let alpha_thread_id = email.thread_id.clone();
    threads.insert("thread-alpha", alpha_thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 4: thread-reply-1
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Alice Sender <alice@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Re: Project Alpha Discussion"),
            ("Message-ID", "<thread-alpha-002@test>"),
            ("In-Reply-To", "<thread-alpha-001@test>"),
            ("References", "<thread-alpha-001@test>"),
        ],
        "Sure, let's discuss. How about Thursday?",
    );
    let blob_id = Id::from("blob-thread-reply-1");
    backend.store_blob(&blob_id, bytes);
    let (email_id, _) = backend
        .import_email(
            account_id,
            &blob_id,
            &[inbox_id.clone()],
            &[],
            Some(&date(ts::DAYS_AGO_7)),
        )
        .await
        .expect("setup: import thread-reply-1");
    emails.insert("thread-reply-1", email_id);

    // -----------------------------------------------------------------------
    // Email 5: thread-reply-2
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Bob Jones <bob@example.org>"),
            ("To", "testuser@example.com, alice@example.com"),
            ("Subject", "Re: Project Alpha Discussion"),
            ("Message-ID", "<thread-alpha-003@test>"),
            ("In-Reply-To", "<thread-alpha-002@test>"),
            (
                "References",
                "<thread-alpha-001@test> <thread-alpha-002@test>",
            ),
        ],
        "Thursday works for me. I'll bring the presentation materials.",
    );
    let blob_id = Id::from("blob-thread-reply-2");
    backend.store_blob(&blob_id, bytes);
    let (email_id, _) = backend
        .import_email(
            account_id,
            &blob_id,
            &[inbox_id.clone()],
            &[answered()],
            Some(&date(ts::DAYS_AGO_6)),
        )
        .await
        .expect("setup: import thread-reply-2");
    emails.insert("thread-reply-2", email_id);

    // -----------------------------------------------------------------------
    // Email 6: multi-mailbox
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "David Cross <david@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Cross-filed document"),
            ("Message-ID", "<multi-mb-001@test>"),
        ],
        "This document should appear in multiple folders.",
    );
    let blob_id = Id::from("blob-multi-mailbox");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[inbox_id.clone(), folder_a_id.clone()],
            &[seen()],
            Some(&date(ts::DAYS_AGO_5)),
        )
        .await
        .expect("setup: import multi-mailbox");
    emails.insert("multi-mailbox", email_id);
    threads
        .entry("multi-mailbox")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 7: large-email
    // -----------------------------------------------------------------------

    let large_body = {
        let repeated = "This is a detailed paragraph of analysis text that covers various topics. "
            .repeat(200);
        format!("Start of analysis. {repeated}End of analysis.")
    };
    let bytes = plain_message(
        &[
            ("From", "Eve Large <eve@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Detailed analysis with data"),
            ("Message-ID", "<large-001@test>"),
        ],
        &large_body,
    );
    let blob_id = Id::from("blob-large-email");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[folder_b_id.clone()],
            &[],
            Some(&date(ts::DAYS_AGO_4)),
        )
        .await
        .expect("setup: import large-email");
    emails.insert("large-email", email_id);
    threads
        .entry("large-email")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 8: html-only
    // -----------------------------------------------------------------------

    let bytes = multipart_alternative(
        &[
            ("From", "Frank Newsletter <frank@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Newsletter: Weekly Digest"),
            ("Message-ID", "<html-only-001@test>"),
        ],
        "Weekly Digest - plain text version",
        "<html><body><h1>Weekly Digest</h1><p>Here is your <b>weekly digest</b> of news.</p></body></html>",
    );
    let blob_id = Id::from("blob-html-only");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[inbox_id.clone()],
            &[seen()],
            Some(&date(ts::DAYS_AGO_3)),
        )
        .await
        .expect("setup: import html-only");
    emails.insert("html-only", email_id);
    threads
        .entry("html-only")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 9: no-subject
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Grace Minimal <grace@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", ""),
            ("Message-ID", "<no-subj-001@test>"),
        ],
        "This message has no subject.",
    );
    let blob_id = Id::from("blob-no-subject");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[inbox_id.clone()],
            &[seen()],
            Some(&date(ts::DAYS_AGO_2)),
        )
        .await
        .expect("setup: import no-subject");
    emails.insert("no-subject", email_id);
    threads
        .entry("no-subject")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 10: custom-keywords
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Henry Tags <henry@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Tagged message"),
            ("Message-ID", "<custom-kw-001@test>"),
        ],
        "This message has custom keywords applied.",
    );
    let blob_id = Id::from("blob-custom-keywords");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[inbox_id.clone()],
            &[seen(), forwarded(), kw("custom_label")],
            Some(&date(ts::DAYS_AGO_1)),
        )
        .await
        .expect("setup: import custom-keywords");
    emails.insert("custom-keywords", email_id);
    threads
        .entry("custom-keywords")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 11: very-old
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Iris Archive <iris@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Archived correspondence"),
            ("Message-ID", "<old-001@test>"),
        ],
        "This is an old archived email from a month ago.",
    );
    let blob_id = Id::from("blob-very-old");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[folder_a_id.clone()],
            &[seen()],
            Some(&date(ts::DAYS_AGO_30)),
        )
        .await
        .expect("setup: import very-old");
    emails.insert("very-old", email_id);
    threads
        .entry("very-old")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 12: special-headers
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "List Admin <list-admin@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Mailing list post"),
            ("Message-ID", "<list-001@test>"),
            ("List-Post", "<mailto:list@example.com>"),
            ("X-Custom-Header", "custom-value-12345"),
        ],
        "This is a post from a mailing list.",
    );
    let blob_id = Id::from("blob-special-headers");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[inbox_id.clone()],
            &[seen()],
            Some(&date(ts::DAYS_AGO_1_MINUS_1H)),
        )
        .await
        .expect("setup: import special-headers");
    emails.insert("special-headers", email_id);
    threads
        .entry("special-headers")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 13: child-mailbox-email
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Nancy Nested <nancy@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "In nested folder"),
            ("Message-ID", "<child-001@test>"),
        ],
        "This email lives in a nested child mailbox.",
    );
    let blob_id = Id::from("blob-child-mailbox-email");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[child1_id.clone()],
            &[seen()],
            Some(&date(ts::DAYS_AGO_5)),
        )
        .await
        .expect("setup: import child-mailbox-email");
    emails.insert("child-mailbox-email", email_id);
    threads
        .entry("child-mailbox-email")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 14: sort-test-1
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Zara First <zara@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Alpha sort test"),
            ("Message-ID", "<sort-001@test>"),
        ],
        &"A".repeat(100),
    );
    let blob_id = Id::from("blob-sort-test-1");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[folder_b_id.clone()],
            &[seen()],
            Some(&date(ts::DAYS_AGO_3)),
        )
        .await
        .expect("setup: import sort-test-1");
    emails.insert("sort-test-1", email_id);
    threads
        .entry("sort-test-1")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 15: sort-test-2
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Amy Second <amy@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Beta sort test"),
            ("Message-ID", "<sort-002@test>"),
        ],
        &"B".repeat(500),
    );
    let blob_id = Id::from("blob-sort-test-2");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[folder_b_id.clone()],
            &[seen(), flagged()],
            Some(&date(ts::DAYS_AGO_2)),
        )
        .await
        .expect("setup: import sort-test-2");
    emails.insert("sort-test-2", email_id);
    threads
        .entry("sort-test-2")
        .or_insert_with(|| email.thread_id.clone());

    // -----------------------------------------------------------------------
    // Email 16: sort-test-3
    // -----------------------------------------------------------------------

    let bytes = plain_message(
        &[
            ("From", "Mike Third <mike@example.com>"),
            ("To", "testuser@example.com"),
            ("Subject", "Gamma sort test"),
            ("Message-ID", "<sort-003@test>"),
        ],
        &"C".repeat(50),
    );
    let blob_id = Id::from("blob-sort-test-3");
    backend.store_blob(&blob_id, bytes);
    let (email_id, email) = backend
        .import_email(
            account_id,
            &blob_id,
            &[folder_b_id.clone()],
            &[],
            Some(&date(ts::DAYS_AGO_1)),
        )
        .await
        .expect("setup: import sort-test-3");
    emails.insert("sort-test-3", email_id);
    threads
        .entry("sort-test-3")
        .or_insert_with(|| email.thread_id.clone());

    // Store the thread-alpha Id captured from the thread-starter email above.
    // (thread-reply-1 and thread-reply-2 join the same thread.)
    threads.insert("thread-alpha", alpha_thread_id);

    SeedData {
        mailbox: mailboxes,
        email: emails,
        thread: threads,
    }
}
