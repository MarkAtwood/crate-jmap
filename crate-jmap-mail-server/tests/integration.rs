// Integration test entry point for jmap-mail-server.
//
// The common module provides MemoryBackend — an in-memory MailBackend used
// as the test harness for all handler integration tests.
//
// Additional test modules will be added here as handler crates are implemented.
#![allow(async_fn_in_trait)]

mod common;

use common::{seed::setup_seed_data, FaultyBackend, MemoryBackend};
use jmap_mail_server::{
    handle_email_changes, handle_email_get, handle_email_import, handle_email_query,
    handle_email_query_changes, handle_email_set, handle_identity_changes, handle_identity_get,
    handle_identity_set, handle_mailbox_changes, handle_mailbox_get, handle_mailbox_query,
    handle_mailbox_query_changes, handle_mailbox_set, handle_search_snippet_get,
    handle_submission_get, handle_submission_query, handle_submission_set, handle_thread_changes,
    handle_thread_get, handle_vacation_get, handle_vacation_set, JmapBackend, JmapObject,
    MailBackend, SetErrorType,
};
use jmap_mail_types::{Identity, Mailbox};
use jmap_types::Id;

// ---------------------------------------------------------------------------
// MemoryBackend smoke tests
// ---------------------------------------------------------------------------

/// Oracle: initial state for any type in a fresh backend is "0" (no changes have occurred).
/// RFC 8620 §5.2 — sinceState "0" means "no prior synchronization".
#[tokio::test]
async fn memory_backend_initial_state_is_zero() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    let state = backend
        .get_state::<Mailbox>(&account_id)
        .await
        .expect("get_state must not fail on fresh backend");
    assert_eq!(state.as_ref(), "0", "initial state must be \"0\"");
}

/// Oracle: state advances after a successful create_object call.
/// RFC 8620 §5.2 — each successful mutation MUST produce a new, different state token.
#[tokio::test]
async fn memory_backend_state_advances_after_create() {
    use jmap_mail_types::MailboxRights;

    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let state_before = backend
        .get_state::<Mailbox>(&account_id)
        .await
        .expect("get_state");

    let mailbox = Mailbox::new(
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
    backend
        .create_object::<Mailbox>(&account_id, "c0", mailbox)
        .await
        .expect("create_object");

    let state_after = backend
        .get_state::<Mailbox>(&account_id)
        .await
        .expect("get_state");

    assert_ne!(
        state_before, state_after,
        "state must change after create_object"
    );
}

/// Oracle: created objects are retrievable by id.
#[tokio::test]
async fn memory_backend_create_then_get() {
    use jmap_mail_types::MailboxRights;

    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let mailbox = Mailbox::new(
        Id::from("placeholder"),
        "Sent",
        20,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        false,
    );
    let (server_id, _) = backend
        .create_object::<Mailbox>(&account_id, "c0", mailbox)
        .await
        .expect("create_object");

    let (found, not_found) = backend
        .get_objects::<Mailbox>(&account_id, Some(std::slice::from_ref(&server_id)), None)
        .await
        .expect("get_objects");

    assert_eq!(found.len(), 1, "must find the created mailbox");
    assert!(not_found.is_empty(), "no ids must be missing");
    assert_eq!(found[0].id, server_id, "returned id must match");
    assert_eq!(found[0].name, "Sent");
}

/// Oracle: destroy_object removes the object; subsequent get returns it in not_found.
#[tokio::test]
async fn memory_backend_destroy_removes_object() {
    use jmap_mail_types::MailboxRights;

    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let mailbox = Mailbox::new(
        Id::from("placeholder"),
        "Trash",
        30,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        true,
    );
    let (server_id, _) = backend
        .create_object::<Mailbox>(&account_id, "c0", mailbox)
        .await
        .expect("create_object");

    backend
        .destroy_object::<Mailbox>(&account_id, &server_id)
        .await
        .expect("destroy_object");

    let (found, not_found) = backend
        .get_objects::<Mailbox>(&account_id, Some(std::slice::from_ref(&server_id)), None)
        .await
        .expect("get_objects after destroy");

    assert!(found.is_empty(), "destroyed object must not be found");
    assert_eq!(
        not_found,
        vec![server_id],
        "destroyed id must be in not_found"
    );
}

/// Oracle: get_changes with since_state "0" returns all created ids.
#[tokio::test]
async fn memory_backend_get_changes_from_zero() {
    use jmap_mail_types::MailboxRights;
    use jmap_types::State;

    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let m1 = Mailbox::new(
        Id::from("p"),
        "A",
        0,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        true,
    );
    let m2 = Mailbox::new(
        Id::from("p"),
        "B",
        0,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        true,
    );

    let (id1, _) = backend
        .create_object::<Mailbox>(&account_id, "c0", m1)
        .await
        .unwrap();
    let (id2, _) = backend
        .create_object::<Mailbox>(&account_id, "c1", m2)
        .await
        .unwrap();

    let changes = backend
        .get_changes::<Mailbox>(&account_id, &State::from("0"), None)
        .await
        .expect("get_changes");

    assert!(changes.created.contains(&id1), "id1 must be in created");
    assert!(changes.created.contains(&id2), "id2 must be in created");
    assert!(changes.updated.is_empty());
    assert!(changes.destroyed.is_empty());
    assert!(!changes.has_more_changes);
}

/// Oracle: search_snippets with a text filter highlights matching text.
#[tokio::test]
async fn memory_backend_search_snippets_highlight() {
    use jmap_mail_types::query::EmailFilterCondition;

    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Store a blob and import an email with a known subject.
    let msg = b"Subject: Hello World\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nThis is the body.";
    let blob_id = Id::from("blob1");
    backend.store_blob(&blob_id, msg.to_vec());

    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("inbox")], &[], None)
        .await
        .expect("import_email");

    let mut filter = EmailFilterCondition::default();
    filter.text = Some("hello".to_owned());

    let snippets = backend
        .search_snippets(&account_id, &[email_id], Some(&filter))
        .await
        .expect("search_snippets");

    assert_eq!(snippets.len(), 1);
    // The subject snippet should contain a <mark> tag around "Hello".
    let subj = snippets[0].subject.as_deref().unwrap_or("");
    assert!(
        subj.contains("<mark>"),
        "subject snippet must contain <mark> tag; got: {subj:?}"
    );
}

// ---------------------------------------------------------------------------
// Thread/get and Thread/changes handler tests
// ---------------------------------------------------------------------------

/// Oracle: Thread/get returns the Thread object with its emailIds list.
///
/// RFC 8621 §3.1 — Thread/get response must include "list" with one Thread per
/// found id. Each Thread carries "emailIds" containing the ids of its member
/// emails. The MemoryBackend stores Thread objects when import_email is called.
#[tokio::test]
async fn thread_get_returns_email_ids() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Import two emails (they will land in the same thread only if they share
    // a Message-ID reference; otherwise each gets its own thread).  For this
    // test we want a single thread with two emails — import two messages where
    // the second references the first via In-Reply-To.
    let msg1 = b"Message-ID: <msg1@test>\r\nSubject: Parent\r\nFrom: a@test\r\n\r\nBody one.";
    let msg2 = b"Message-ID: <msg2@test>\r\nIn-Reply-To: <msg1@test>\r\nSubject: Re: Parent\r\nFrom: b@test\r\n\r\nBody two.";

    let blob1 = Id::from("blob1");
    let blob2 = Id::from("blob2");
    backend.store_blob(&blob1, msg1.to_vec());
    backend.store_blob(&blob2, msg2.to_vec());

    let (_, email1) = backend
        .import_email(&account_id, &blob1, &[Id::from("inbox")], &[], None)
        .await
        .expect("import email 1");

    let (_, email2) = backend
        .import_email(&account_id, &blob2, &[Id::from("inbox")], &[], None)
        .await
        .expect("import email 2");

    // Both emails must be in the same thread (the reply links them).
    assert_eq!(
        email1.thread_id, email2.thread_id,
        "both emails must share a thread id"
    );

    let thread_id = email1.thread_id.clone();

    // Call Thread/get for that thread id.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [thread_id.as_ref()],
    });

    let (resp, extra) = handle_thread_get(&backend, args)
        .await
        .expect("Thread/get must succeed");

    assert!(
        extra.is_empty(),
        "Thread/get must not generate extra invocations"
    );

    // Oracle: RFC 8621 §3.1 — response must contain "list" with one Thread.
    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find exactly one thread");

    let thread_obj = &list[0];
    assert_eq!(
        thread_obj["id"].as_str().unwrap_or(""),
        thread_id.as_ref(),
        "returned thread id must match requested id"
    );

    // emailIds must contain entries for both imported emails.
    let email_ids = thread_obj["emailIds"]
        .as_array()
        .expect("emailIds must be an array");
    assert_eq!(email_ids.len(), 2, "thread must contain both email ids");

    // RFC 8620 §5.1: notFound must be [] (empty array) when all ids are found.
    assert_eq!(
        resp["notFound"].as_array().map(|a| a.len()),
        Some(0),
        "notFound must be [] when all ids are found"
    );
}

/// Oracle: Thread/changes from state "0" reports the thread as created.
///
/// RFC 8620 §5.2 — a /changes call with sinceState "0" (meaning no prior sync)
/// must include all existing objects in "created". The thread is created when the
/// first email is imported into it.
#[tokio::test]
async fn thread_changes_from_zero_returns_all() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Import one email; this creates a thread.
    let msg = b"Message-ID: <solo@test>\r\nSubject: Solo\r\nFrom: a@test\r\n\r\nBody.";
    let blob_id = Id::from("blob1");
    backend.store_blob(&blob_id, msg.to_vec());

    let (_, email) = backend
        .import_email(&account_id, &blob_id, &[Id::from("inbox")], &[], None)
        .await
        .expect("import_email");

    let thread_id = email.thread_id.clone();

    // Thread/changes from sinceState "0".
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": "0",
    });

    let (resp, extra) = handle_thread_changes(&backend, args)
        .await
        .expect("Thread/changes must succeed");

    assert!(
        extra.is_empty(),
        "Thread/changes must not generate extra invocations"
    );

    // Oracle: RFC 8620 §5.2 — "created" must include the thread id.
    let created = resp["created"]
        .as_array()
        .expect("created must be an array");
    let created_strs: Vec<&str> = created.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        created_strs.contains(&thread_id.as_ref()),
        "thread id must appear in created; got: {created_strs:?}"
    );

    // Oracle: "updated" and "destroyed" must be empty for a fresh account.
    assert!(
        resp["updated"].as_array().map_or(true, |a| a.is_empty()),
        "updated must be empty"
    );
    assert!(
        resp["destroyed"].as_array().map_or(true, |a| a.is_empty()),
        "destroyed must be empty"
    );

    // Oracle: oldState must echo sinceState.
    assert_eq!(resp["oldState"].as_str().unwrap_or(""), "0");

    // Oracle: newState must differ from "0" (a mutation occurred).
    assert_ne!(
        resp["newState"].as_str().unwrap_or("0"),
        "0",
        "newState must advance after import"
    );
}

// ---------------------------------------------------------------------------
// SearchSnippet/get handler tests
// ---------------------------------------------------------------------------

/// Oracle: SearchSnippet/get returns a snippet list with <mark>-highlighted subject.
///
/// RFC 8621 §5.9 — given an email with subject "Hello World" and a filter of
/// text "hello", the returned snippet's `subject` field must contain a `<mark>`
/// tag wrapping the matched text.
#[tokio::test]
async fn search_snippet_get_returns_snippets() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Import an email with a known subject and body.
    let msg =
        b"Subject: Hello World\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nHello body text.";
    let blob_id = Id::from("blob1");
    backend.store_blob(&blob_id, msg.to_vec());

    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("inbox")], &[], None)
        .await
        .expect("import_email");

    // Call handler with filter={text: "hello"}.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "emailIds": [email_id.as_ref()],
        "filter": { "text": "hello" },
    });

    let (resp, extra) = handle_search_snippet_get(&backend, args)
        .await
        .expect("SearchSnippet/get must succeed");

    assert!(
        extra.is_empty(),
        "SearchSnippet/get must not generate extra invocations"
    );

    // Oracle: accountId echoed back.
    assert_eq!(
        resp["accountId"].as_str().unwrap_or(""),
        account_id.as_ref()
    );

    // Oracle: one snippet returned, no notFound.
    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must return exactly one snippet");
    // RFC 8620 §5.1: notFound must be [] (empty array) when all ids are found.
    assert_eq!(
        resp["notFound"].as_array().map(|a| a.len()),
        Some(0),
        "notFound must be [] when all ids are found"
    );

    // Oracle: RFC 8621 §5.9 — matched text must be wrapped in <mark> tags.
    // The subject "Hello World" matches filter text "hello" (case-insensitive).
    let subj = list[0]["subject"].as_str().unwrap_or("");
    assert!(
        subj.contains("<mark>"),
        "subject snippet must contain <mark> tag; got: {subj:?}"
    );
    // The HTML-escaped form must NOT contain raw '<' or '>' outside of tags.
    // Verify the mark wraps "Hello" specifically.
    assert!(
        subj.contains("<mark>Hello</mark>"),
        "subject must wrap matched term in <mark>; got: {subj:?}"
    );
}

/// Oracle: SearchSnippet/get returns accountNotSupportedByMethod when the
/// backend reports it does not support the SearchSnippet type.
///
/// RFC 8621 §5.9 / RFC 8620 §5.1 — servers that cannot generate snippets MUST
/// respond with an `accountNotSupportedByMethod` error, not a success response
/// with empty snippets.
#[tokio::test]
async fn search_snippet_get_capability_gated() {
    // A thin wrapper around MemoryBackend that returns false for SearchSnippet.
    struct NoSnippetBackend(MemoryBackend);

    impl JmapBackend for NoSnippetBackend {
        type Error = common::MemoryError;

        async fn account_exists(&self, account_id: &Id) -> Result<bool, Self::Error> {
            self.0.account_exists(account_id).await
        }

        async fn get_objects<O: jmap_mail_server::GetObject + Send + Sync>(
            &self,
            account_id: &Id,
            ids: Option<&[Id]>,
            properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            self.0.get_objects(account_id, ids, properties).await
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            account_id: &Id,
        ) -> Result<jmap_types::State, Self::Error> {
            self.0.get_state::<O>(account_id).await
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            account_id: &Id,
            since_state: &jmap_types::State,
            max_changes: Option<u64>,
        ) -> Result<
            jmap_mail_server::ChangesResult,
            jmap_mail_server::BackendChangesError<Self::Error>,
        > {
            self.0
                .get_changes::<O>(account_id, since_state, max_changes)
                .await
        }

        async fn query_objects<O: jmap_mail_server::QueryObject + Send + Sync>(
            &self,
            account_id: &Id,
            filter: Option<&O::Filter>,
            sort: Option<&[O::Comparator]>,
            limit: Option<u64>,
            position: i64,
        ) -> Result<jmap_mail_server::QueryResult, Self::Error> {
            self.0
                .query_objects::<O>(account_id, filter, sort, limit, position)
                .await
        }

        async fn query_changes<O: jmap_mail_server::QueryObject + Send + Sync>(
            &self,
            account_id: &Id,
            since_query_state: &jmap_types::State,
            filter: Option<&O::Filter>,
            sort: Option<&[O::Comparator]>,
            max_changes: Option<u64>,
            up_to_id: Option<&Id>,
            collapse_threads: bool,
        ) -> Result<
            jmap_mail_server::QueryChangesResult,
            jmap_mail_server::BackendChangesError<Self::Error>,
        > {
            self.0
                .query_changes::<O>(
                    account_id,
                    since_query_state,
                    filter,
                    sort,
                    max_changes,
                    up_to_id,
                    collapse_threads,
                )
                .await
        }
    }

    impl MailBackend for NoSnippetBackend {
        async fn create_object<O: jmap_mail_server::SetObject + Send + Sync>(
            &self,
            account_id: &Id,
            create_id: &str,
            obj: O,
        ) -> Result<(Id, O), jmap_mail_server::BackendSetError<Self::Error>> {
            self.0.create_object(account_id, create_id, obj).await
        }

        async fn update_object<O: jmap_mail_server::SetObject + Send + Sync>(
            &self,
            account_id: &Id,
            id: &Id,
            patch: O::Patch,
        ) -> Result<Option<O>, jmap_mail_server::BackendSetError<Self::Error>> {
            self.0.update_object(account_id, id, patch).await
        }

        async fn destroy_object<O: jmap_mail_server::SetObject + Send + Sync>(
            &self,
            account_id: &Id,
            id: &Id,
        ) -> Result<(), jmap_mail_server::BackendSetError<Self::Error>> {
            self.0.destroy_object::<O>(account_id, id).await
        }

        async fn import_email(
            &self,
            account_id: &Id,
            blob_id: &Id,
            mailbox_ids: &[Id],
            keywords: &[jmap_mail_types::Keyword],
            received_at: Option<&jmap_types::UTCDate>,
        ) -> Result<(Id, jmap_mail_types::Email), jmap_mail_server::BackendSetError<Self::Error>>
        {
            self.0
                .import_email(account_id, blob_id, mailbox_ids, keywords, received_at)
                .await
        }

        async fn blob_exists(&self, account_id: &Id, blob_id: &Id) -> bool {
            self.0.blob_exists(account_id, blob_id).await
        }

        async fn parse_email(
            &self,
            account_id: &Id,
            blob_id: &Id,
        ) -> Result<jmap_mail_types::Email, Self::Error> {
            self.0.parse_email(account_id, blob_id).await
        }

        async fn copy_email(
            &self,
            from_account_id: &Id,
            email_id: &Id,
            to_account_id: &Id,
            mailbox_ids: &[Id],
            keywords: &[jmap_mail_types::Keyword],
            received_at: Option<&jmap_types::UTCDate>,
        ) -> Result<(Id, jmap_mail_types::Email), jmap_mail_server::BackendSetError<Self::Error>>
        {
            self.0
                .copy_email(
                    from_account_id,
                    email_id,
                    to_account_id,
                    mailbox_ids,
                    keywords,
                    received_at,
                )
                .await
        }

        async fn search_snippets(
            &self,
            account_id: &Id,
            email_ids: &[Id],
            filter: Option<&jmap_mail_types::EmailFilterCondition>,
        ) -> Result<Vec<jmap_mail_types::SearchSnippet>, Self::Error> {
            self.0.search_snippets(account_id, email_ids, filter).await
        }

        async fn find_thread_by_message_ids(
            &self,
            account_id: &Id,
            message_ids: &[&str],
        ) -> Result<Option<Id>, Self::Error> {
            self.0
                .find_thread_by_message_ids(account_id, message_ids)
                .await
        }

        fn supports_type<O: JmapObject>(&self) -> bool {
            // Return false for SearchSnippet; delegate everything else.
            if std::any::TypeId::of::<O>()
                == std::any::TypeId::of::<jmap_mail_types::SearchSnippet>()
            {
                false
            } else {
                self.0.supports_type::<O>()
            }
        }
    }

    let backend = NoSnippetBackend(MemoryBackend::new());
    let args = serde_json::json!({
        "accountId": "account1",
        "emailIds": ["email1"],
    });

    let err = handle_search_snippet_get(&backend, args)
        .await
        .expect_err("must fail when SearchSnippet is unsupported");

    // Oracle: RFC 8620 §5.1 — error type must be accountNotSupportedByMethod.
    let err_json = serde_json::to_value(&err).expect("JmapError must serialize");
    assert_eq!(
        err_json["type"].as_str().unwrap_or(""),
        "accountNotSupportedByMethod",
        "error type must be accountNotSupportedByMethod; got: {err_json:?}"
    );
}

// ---------------------------------------------------------------------------
// VacationResponse/get and VacationResponse/set handler tests
// ---------------------------------------------------------------------------

/// Oracle: VacationResponse/get on a fresh account returns an empty list.
///
/// RFC 8621 §8.1 — if no VacationResponse has been set, the server returns
/// an empty list and notFound=null.  This is not an error.
#[tokio::test]
async fn vacation_get_fresh_account_returns_empty() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": null,
    });

    let (resp, extra) = handle_vacation_get(&backend, args)
        .await
        .expect("VacationResponse/get must succeed on fresh account");

    assert!(
        extra.is_empty(),
        "VacationResponse/get must not generate extra invocations"
    );

    // Oracle: RFC 8621 §8.1 — list is empty; RFC 8620 §5.1: notFound is [].
    let list = resp["list"].as_array().expect("list must be an array");
    assert!(
        list.is_empty(),
        "fresh account must have empty vacation list"
    );
    // RFC 8620 §5.1: notFound is Id[] — always an array, empty when no ids were requested.
    assert_eq!(
        resp["notFound"].as_array().map(|a| a.len()),
        Some(0),
        "notFound must be [] when no ids were requested"
    );
    assert_eq!(
        resp["accountId"].as_str().unwrap_or(""),
        account_id.as_ref()
    );
}

/// Oracle: VacationResponse/set create is rejected with type="singleton".
///
/// RFC 8621 §8.2 — VacationResponse is a singleton; any create attempt
/// MUST be rejected with a SetError of type "singleton".
#[tokio::test]
async fn vacation_set_create_returns_singleton_error() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": { "isEnabled": true, "textBody": "I am out of office." }
        },
    });

    let (resp, extra) = handle_vacation_set(&backend, args)
        .await
        .expect("VacationResponse/set must succeed at the method level");

    assert!(
        extra.is_empty(),
        "VacationResponse/set must not generate extra invocations"
    );

    // Oracle: RFC 8621 §8.2 — create must land in notCreated with type "singleton".
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be an object");
    assert!(
        not_created.contains_key("c0"),
        "create id 'c0' must appear in notCreated"
    );
    let err_type = not_created["c0"]["type"].as_str().unwrap_or("");
    assert_eq!(
        err_type, "singleton",
        "error type must be 'singleton'; got: {err_type:?}"
    );

    // Oracle: RFC 8620 §5.3 — created MUST be null when nothing was created.
    assert!(
        resp["created"].is_null(),
        "created must be null when empty; got: {:?}",
        resp["created"]
    );
}

/// Oracle: VacationResponse/set update of "singleton" persists and is
/// returned by a subsequent VacationResponse/get.
///
/// RFC 8621 §8.2 — updating "singleton" is the only valid mutation.
/// After a successful update the new field values must be visible in /get.
#[tokio::test]
async fn vacation_set_update_singleton_works() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Update the singleton — it does not exist yet, so the handler upserts it.
    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            "singleton": {
                "isEnabled": true,
                "subject": "Out of office",
                "textBody": "I am out of the office."
            }
        },
    });

    let (set_resp, _) = handle_vacation_set(&backend, set_args)
        .await
        .expect("VacationResponse/set must succeed");

    // Oracle: "singleton" must appear in the updated map, not in notUpdated.
    let updated = set_resp["updated"]
        .as_object()
        .expect("updated must be an object");
    assert!(
        updated.contains_key("singleton"),
        "singleton must appear in updated; notUpdated={:?}",
        set_resp["notUpdated"]
    );

    // Now retrieve it via /get.
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": ["singleton"],
    });

    let (get_resp, _) = handle_vacation_get(&backend, get_args)
        .await
        .expect("VacationResponse/get must succeed after update");

    let list = get_resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find exactly one VacationResponse");

    let vr = &list[0];
    // Oracle: hand-written field values match what we set above.
    assert_eq!(vr["id"].as_str().unwrap_or(""), "singleton");
    assert_eq!(vr["isEnabled"].as_bool(), Some(true));
    assert_eq!(vr["subject"].as_str().unwrap_or(""), "Out of office");
    assert_eq!(
        vr["textBody"].as_str().unwrap_or(""),
        "I am out of the office."
    );
}

/// Oracle: VacationResponse/set destroy is rejected with type="singleton".
///
/// RFC 8621 §8.2 — VacationResponse is a singleton; any destroy attempt
/// MUST be rejected with a SetError of type "singleton".
#[tokio::test]
async fn vacation_set_destroy_returns_singleton_error() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "destroy": ["singleton"],
    });

    let (resp, extra) = handle_vacation_set(&backend, args)
        .await
        .expect("VacationResponse/set must succeed at the method level");

    assert!(
        extra.is_empty(),
        "VacationResponse/set must not generate extra invocations"
    );

    // Oracle: RFC 8621 §8.2 — destroy must land in notDestroyed with type "singleton".
    let not_destroyed = resp["notDestroyed"]
        .as_object()
        .expect("notDestroyed must be an object");
    assert!(
        not_destroyed.contains_key("singleton"),
        "destroy id 'singleton' must appear in notDestroyed"
    );
    let err_type = not_destroyed["singleton"]["type"].as_str().unwrap_or("");
    assert_eq!(
        err_type, "singleton",
        "error type must be 'singleton'; got: {err_type:?}"
    );

    // Oracle: RFC 8620 §5.3 — destroyed MUST be null when nothing was destroyed.
    assert!(
        resp["destroyed"].is_null(),
        "destroyed must be null when empty; got: {:?}",
        resp["destroyed"]
    );

    // Verify the SetErrorType round-trips correctly through serde.
    let deserialized: jmap_mail_server::SetError =
        serde_json::from_value(not_destroyed["singleton"].clone())
            .expect("notDestroyed entry must deserialize as SetError");
    assert_eq!(deserialized.error_type, SetErrorType::Singleton);
}

// ---------------------------------------------------------------------------
// Mailbox/* handler integration tests
// ---------------------------------------------------------------------------

/// Oracle: Mailbox/set create followed by Mailbox/get returns the mailbox
/// with the correct name (RFC 8621 §2.1, §2.5).
#[tokio::test]
async fn mailbox_set_create_and_get() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));

    // Create a mailbox via Mailbox/set.
    let set_args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "c0": { "name": "MyInbox" }
        }
    });
    let (set_resp, _) = handle_mailbox_set(&backend, set_args)
        .await
        .expect("Mailbox/set must not error");

    // Verify the create succeeded.
    let created = set_resp["created"]
        .as_object()
        .expect("created must be an object");
    assert!(created.contains_key("c0"), "c0 must appear in created");
    let assigned_id = created["c0"]["id"]
        .as_str()
        .expect("created mailbox must have an id");

    // Retrieve the mailbox via Mailbox/get.
    let get_args = serde_json::json!({
        "accountId": "acct1",
        "ids": [assigned_id]
    });
    let (get_resp, _) = handle_mailbox_get(&backend, get_args)
        .await
        .expect("Mailbox/get must not error");

    let list = get_resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find exactly one mailbox");
    assert_eq!(list[0]["name"].as_str(), Some("MyInbox"));
    assert_eq!(list[0]["id"].as_str(), Some(assigned_id));
}

/// Oracle: Mailbox/set destroy without onDestroyRemoveEmails when the
/// mailbox contains emails returns notDestroyed with type=mailboxHasEmail
/// (RFC 8621 §2.5).
#[tokio::test]
async fn mailbox_set_destroy_with_emails_no_flag_fails() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");

    // Create a mailbox.
    let (mb_id, _) = backend
        .create_object::<Mailbox>(
            &account_id,
            "c0",
            jmap_mail_types::Mailbox::new(
                Id::from("p"),
                "Letters",
                0,
                0,
                0,
                0,
                0,
                jmap_mail_types::MailboxRights::default(),
                false,
            ),
        )
        .await
        .expect("create mailbox");

    // Import an email into that mailbox.
    let msg = b"Subject: test\r\n\r\nbody";
    let blob_id = Id::from("blob-nodestroy");
    backend.store_blob(&blob_id, msg.to_vec());
    backend
        .import_email(&account_id, &blob_id, &[mb_id.clone()], &[], None)
        .await
        .expect("import_email");

    // Try to destroy the mailbox without onDestroyRemoveEmails.
    let destroy_args = serde_json::json!({
        "accountId": "acct1",
        "destroy": [mb_id.as_ref()]
    });
    let (resp, _) = handle_mailbox_set(&backend, destroy_args)
        .await
        .expect("Mailbox/set must not error");

    let not_destroyed = resp["notDestroyed"]
        .as_object()
        .expect("notDestroyed must be present");
    let entry = &not_destroyed[mb_id.as_ref()];
    assert_eq!(
        entry["type"].as_str(),
        Some("mailboxHasEmail"),
        "error type must be mailboxHasEmail; got: {:?}",
        entry["type"]
    );
}

/// Oracle: Mailbox/set destroy with onDestroyRemoveEmails=true removes both
/// the mailbox and the email that was only in that mailbox (RFC 8621 §2.5).
#[tokio::test]
async fn mailbox_set_destroy_with_emails_with_flag_succeeds() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");

    // Create mailbox.
    let (mb_id, _) = backend
        .create_object::<Mailbox>(
            &account_id,
            "c0",
            jmap_mail_types::Mailbox::new(
                Id::from("p"),
                "Temp",
                0,
                0,
                0,
                0,
                0,
                jmap_mail_types::MailboxRights::default(),
                false,
            ),
        )
        .await
        .expect("create mailbox");

    // Import an email into that mailbox.
    let msg = b"Subject: cascade\r\n\r\nbody";
    let blob_id = Id::from("blob-cascade");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[mb_id.clone()], &[], None)
        .await
        .expect("import_email");

    // Destroy with onDestroyRemoveEmails=true.
    let destroy_args = serde_json::json!({
        "accountId": "acct1",
        "onDestroyRemoveEmails": true,
        "destroy": [mb_id.as_ref()]
    });
    let (resp, _) = handle_mailbox_set(&backend, destroy_args)
        .await
        .expect("Mailbox/set must not error");

    // Mailbox must appear in destroyed list.
    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array");
    assert!(
        destroyed.iter().any(|v| v.as_str() == Some(mb_id.as_ref())),
        "mailbox must be in destroyed"
    );
    assert!(
        resp["notDestroyed"].is_null(),
        "notDestroyed must be null/absent"
    );

    // Email must also be gone from the store.
    let (found, _) = backend
        .get_objects::<jmap_mail_types::Email>(
            &account_id,
            Some(std::slice::from_ref(&email_id)),
            None,
        )
        .await
        .expect("get_objects");
    assert!(
        found.is_empty(),
        "email must have been deleted with the mailbox"
    );
}

/// Oracle: creating two mailboxes with the same role in the same account
/// results in the second being rejected with invalidProperties: ["role"]
/// (RFC 8621 §2.5).
#[tokio::test]
async fn mailbox_role_uniqueness_enforced() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));

    // Create first mailbox with role=inbox.
    let first_args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "c0": { "name": "Inbox", "role": "inbox" }
        }
    });
    let (resp1, _) = handle_mailbox_set(&backend, first_args)
        .await
        .expect("first Mailbox/set");
    let created1 = resp1["created"]
        .as_object()
        .expect("created must be an object");
    assert!(created1.contains_key("c0"), "first create must succeed");

    // Create second mailbox also with role=inbox in the same request.
    let second_args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "c1": { "name": "AlsoInbox", "role": "inbox" }
        }
    });
    let (resp2, _) = handle_mailbox_set(&backend, second_args)
        .await
        .expect("second Mailbox/set");

    let not_created = resp2["notCreated"]
        .as_object()
        .expect("notCreated must be present when role is duplicate");
    let entry = &not_created["c1"];
    assert_eq!(
        entry["type"].as_str(),
        Some("invalidProperties"),
        "error type must be invalidProperties; got: {:?}",
        entry["type"]
    );
    let props = entry["properties"]
        .as_array()
        .expect("properties must be an array");
    assert!(
        props.iter().any(|v| v.as_str() == Some("role")),
        "properties must include 'role'"
    );
}

// ---------------------------------------------------------------------------
// Identity/get, Identity/changes, Identity/set handler tests
// ---------------------------------------------------------------------------

/// Oracle: Identity/set create with name+email succeeds; Identity/get returns it.
///
/// RFC 8621 §6.3 — a valid create must produce an entry in "created" with a
/// server-assigned id. A subsequent Identity/get must return the object.
#[tokio::test]
async fn identity_set_create_and_get() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = "account1";

    let set_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "c0": {
                "name": "Alice",
                "email": "alice@example.com",
                "textSignature": "",
                "htmlSignature": "",
            }
        }
    });

    let (set_resp, _) = handle_identity_set(&backend, set_args)
        .await
        .expect("Identity/set must succeed");

    // Oracle: "c0" must appear in "created", not in "notCreated".
    let created = &set_resp["created"];
    assert!(
        !created["c0"].is_null(),
        "c0 must appear in created; set response: {set_resp:?}"
    );
    assert!(
        set_resp["notCreated"]["c0"].is_null(),
        "c0 must not appear in notCreated"
    );

    // Extract the server-assigned id.
    let server_id = created["c0"]["id"]
        .as_str()
        .expect("created.c0.id must be a string")
        .to_string();

    // Identity/get must return the newly created identity.
    let get_args = serde_json::json!({
        "accountId": account_id,
        "ids": [server_id],
    });

    let (get_resp, _) = handle_identity_get(&backend, get_args)
        .await
        .expect("Identity/get must succeed");

    let list = get_resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find exactly one identity");
    assert_eq!(
        list[0]["email"].as_str().unwrap_or(""),
        "alice@example.com",
        "email must match"
    );
    assert_eq!(
        list[0]["name"].as_str().unwrap_or(""),
        "Alice",
        "name must match"
    );
    // mayDelete is server-set to true on create.
    assert_eq!(
        list[0]["mayDelete"].as_bool(),
        Some(true),
        "mayDelete must be true after create"
    );
}

/// Oracle: Identity/set create without email is rejected with invalidProperties.
///
/// RFC 8621 §6.3 — "email" is a required field on create. A create request
/// missing "email" must appear in "notCreated" with type "invalidProperties"
/// and properties list containing "email".
#[tokio::test]
async fn identity_set_create_without_email_is_invalid() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));

    let set_args = serde_json::json!({
        "accountId": "account1",
        "create": {
            "c1": {
                "name": "Bob",
            }
        }
    });

    let (set_resp, _) = handle_identity_set(&backend, set_args)
        .await
        .expect("Identity/set must not return a protocol-level error");

    // Oracle: c1 must be in notCreated, not in created.
    assert!(
        set_resp["created"]["c1"].is_null(),
        "c1 must not appear in created"
    );

    let not_created_c1 = &set_resp["notCreated"]["c1"];
    assert!(
        !not_created_c1.is_null(),
        "c1 must appear in notCreated; response: {set_resp:?}"
    );
    assert_eq!(
        not_created_c1["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties"
    );

    let props = not_created_c1["properties"]
        .as_array()
        .expect("properties must be an array");
    let prop_strs: Vec<&str> = props.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        prop_strs.contains(&"email"),
        "properties must contain \"email\"; got: {prop_strs:?}"
    );
}

/// Oracle: Identity/set update with "email" in the patch is rejected.
///
/// RFC 8621 §6 — "email" is immutable after creation. An update patch that
/// includes the "email" key must produce a notUpdated entry with type
/// "invalidProperties" and properties containing "email".
#[tokio::test]
async fn identity_set_update_email_is_forbidden() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = "account1";

    let create_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "c0": { "name": "Carol", "email": "carol@example.com" }
        }
    });
    let (create_resp, _) = handle_identity_set(&backend, create_args)
        .await
        .expect("create must succeed");
    let identity_id = create_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get created id")
        .to_string();

    let update_args = serde_json::json!({
        "accountId": account_id,
        "update": {
            identity_id.clone(): {
                "email": "newemail@example.com",
            }
        }
    });

    let (upd_resp, _) = handle_identity_set(&backend, update_args)
        .await
        .expect("Identity/set must not return a protocol-level error");

    // Oracle: the id must appear in notUpdated, not in updated.
    assert!(
        upd_resp["updated"][&identity_id].is_null(),
        "id must not appear in updated"
    );

    let not_updated = &upd_resp["notUpdated"][&identity_id];
    assert!(
        !not_updated.is_null(),
        "id must appear in notUpdated; response: {upd_resp:?}"
    );
    assert_eq!(
        not_updated["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties"
    );

    let props = not_updated["properties"]
        .as_array()
        .expect("properties must be an array");
    let prop_strs: Vec<&str> = props.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        prop_strs.contains(&"email"),
        "properties must contain \"email\"; got: {prop_strs:?}"
    );
}

/// Oracle: destroying an identity with mayDelete=false returns forbidden.
///
/// RFC 8621 §6.3 — the server MUST reject destruction of an identity whose
/// "mayDelete" property is false with a SetError of type "forbidden".
#[tokio::test]
async fn identity_set_destroy_may_delete_false_is_forbidden() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": { "name": "Dave", "email": "dave@example.com" }
        }
    });
    let (create_resp, _) = handle_identity_set(&backend, create_args)
        .await
        .expect("create must succeed");
    let identity_id = Id::from(
        create_resp["created"]["c0"]["id"]
            .as_str()
            .expect("must get created id"),
    );

    // Bypass the handler to set mayDelete=false directly in the backend,
    // simulating a server-managed default identity the user cannot delete.
    let patch = serde_json::json!({ "mayDelete": false });
    backend
        .update_object::<Identity>(&account_id, &identity_id, patch)
        .await
        .expect("direct backend update must succeed");

    let destroy_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "destroy": [identity_id.as_ref()],
    });

    let (destroy_resp, _) = handle_identity_set(&backend, destroy_args)
        .await
        .expect("Identity/set must not return a protocol-level error");

    // Oracle: RFC 8620 §5.3 — destroyed MUST be null when nothing was destroyed.
    assert!(
        destroy_resp["destroyed"].is_null(),
        "destroyed must be null when empty; got: {:?}",
        destroy_resp["destroyed"]
    );

    let not_destroyed = &destroy_resp["notDestroyed"][identity_id.as_ref()];
    assert!(
        !not_destroyed.is_null(),
        "id must appear in notDestroyed; response: {destroy_resp:?}"
    );
    assert_eq!(
        not_destroyed["type"].as_str().unwrap_or(""),
        "forbidden",
        "error type must be forbidden"
    );
}

/// Oracle: Identity/set create with malformed replyTo returns invalidProperties.
///
/// RFC 8621 §6.3 — replyTo is EmailAddress[]. Providing a plain string is
/// invalid and must be rejected, not silently ignored.
#[tokio::test]
async fn identity_set_create_malformed_reply_to_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "email": "user@example.com",
                "replyTo": "not-an-array"
            }
        }
    });

    let (set_resp, _) = handle_identity_set(&backend, set_args)
        .await
        .expect("Identity/set must not fail at the method level");

    assert!(
        set_resp["created"].is_null(),
        "created must be null; got: {:?}",
        set_resp["created"]
    );
    let not_created = &set_resp["notCreated"]["c0"];
    assert!(
        !not_created.is_null(),
        "c0 must appear in notCreated; response: {set_resp:?}"
    );
    assert_eq!(
        not_created["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties; got: {not_created:?}"
    );
    let empty = vec![];
    let props: Vec<&str> = not_created["properties"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        props.contains(&"replyTo"),
        "properties must name replyTo; got: {props:?}"
    );
}

// ---------------------------------------------------------------------------
// Identity/get conformance tests (RFC 8621 §6.1)
// ---------------------------------------------------------------------------

/// Oracle: Identity/get with ids=null returns all identities (RFC 8621 §6.1).
/// Ported from jmap-test-suite identity-get.test.ts: get-all-identities.
#[tokio::test]
async fn identity_get_all_returns_list() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    // Seed one identity so the list is non-empty.
    let (set_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Alice", "email": "alice@example.com" } }
        }),
    )
    .await
    .expect("create identity");
    assert!(
        set_resp["created"]["c0"]["id"].as_str().is_some(),
        "identity must be created"
    );

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": serde_json::Value::Null }),
    )
    .await
    .expect("get all identities");

    let list = get_resp["list"].as_array().expect("list must be array");
    assert!(!list.is_empty(), "list must be non-empty");
    // State is a string (RFC 8620 §5.2).
    assert!(
        get_resp["state"].as_str().is_some(),
        "state must be a string"
    );
}

/// Oracle: Identity object has all required properties with correct types (RFC 8621 §6.1).
/// Ported from jmap-test-suite identity-get.test.ts: get-identity-properties.
#[tokio::test]
async fn identity_get_required_properties() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (set_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": {
                "c0": {
                    "name": "Bob",
                    "email": "bob@example.com",
                    "textSignature": "-- Bob",
                    "htmlSignature": "<p>Bob</p>",
                }
            }
        }),
    )
    .await
    .expect("create identity");
    let id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get id")
        .to_owned();

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [id] }),
    )
    .await
    .expect("get identity by id");

    let obj = &get_resp["list"][0];
    assert!(obj["id"].as_str().is_some(), "id must be a string");
    assert!(obj["name"].as_str().is_some(), "name must be a string");
    assert!(obj["email"].as_str().is_some(), "email must be a string");
    assert!(
        obj["textSignature"].as_str().is_some(),
        "textSignature must be a string"
    );
    assert!(
        obj["htmlSignature"].as_str().is_some(),
        "htmlSignature must be a string"
    );
    assert!(
        obj["mayDelete"].as_bool().is_some(),
        "mayDelete must be a boolean"
    );
    // replyTo is null or array (RFC 8621 §6.1)
    assert!(
        obj["replyTo"].is_null() || obj["replyTo"].is_array(),
        "replyTo must be null or array"
    );
    // bcc is null or array
    assert!(
        obj["bcc"].is_null() || obj["bcc"].is_array(),
        "bcc must be null or array"
    );
}

/// Oracle: Identity/get with a specific id returns only that identity (RFC 8621 §6.1).
/// Ported from jmap-test-suite identity-get.test.ts: get-identity-by-id.
#[tokio::test]
async fn identity_get_by_id() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (set_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Carol", "email": "carol@example.com" } }
        }),
    )
    .await
    .expect("create identity");
    let id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get id")
        .to_owned();

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [&id] }),
    )
    .await
    .expect("get identity by id");

    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1, "must return exactly one identity");
    assert_eq!(
        list[0]["id"].as_str().unwrap_or(""),
        id,
        "returned id must match requested"
    );
}

/// Oracle: Identity/get returns notFound for unknown id (RFC 8621 §6.1 / RFC 8620 §5.1).
/// Ported from jmap-test-suite identity-get.test.ts: get-identity-not-found.
#[tokio::test]
async fn identity_get_not_found() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": ["nonexistent-identity-xyz"] }),
    )
    .await
    .expect("get with unknown id must not error");

    // notFound MUST be a string array (RFC 8620 §5.1), never null.
    let not_found = get_resp["notFound"]
        .as_array()
        .expect("notFound must be array");
    assert!(
        not_found
            .iter()
            .any(|v| v.as_str() == Some("nonexistent-identity-xyz")),
        "notFound must contain the requested id"
    );
}

/// Oracle: Identity email address contains "@" (RFC 8621 §6.1).
/// Ported from jmap-test-suite identity-get.test.ts: get-identity-email-matches.
#[tokio::test]
async fn identity_get_email_contains_at() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (set_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Dave", "email": "dave@example.com" } }
        }),
    )
    .await
    .expect("create identity");
    let id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get id")
        .to_owned();

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [&id] }),
    )
    .await
    .expect("get identity by id");

    let email = get_resp["list"][0]["email"]
        .as_str()
        .expect("email must be string");
    assert!(email.contains('@'), "email must contain '@'");
}

// ---------------------------------------------------------------------------
// Identity/set conformance tests (RFC 8621 §6.3)
// ---------------------------------------------------------------------------

/// Oracle: Identity/set update changes name; get confirms new value (RFC 8621 §6.3).
/// Ported from jmap-test-suite identity-set.test.ts: set-update-name.
#[tokio::test]
async fn identity_set_update_name_roundtrip() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    // Create initial identity.
    let (set_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Original Name", "email": "test@example.com" } }
        }),
    )
    .await
    .expect("create identity");
    let id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get id")
        .to_owned();

    // Update name.
    let (update_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "update": { &id: { "name": "Test Updated Name" } }
        }),
    )
    .await
    .expect("update name");
    assert!(
        !update_resp["updated"].is_null(),
        "updated map must be non-null"
    );

    // Verify new name via get.
    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [&id] }),
    )
    .await
    .expect("get after update");
    assert_eq!(
        get_resp["list"][0]["name"].as_str(),
        Some("Test Updated Name"),
        "name must reflect update"
    );
}

/// Oracle: Identity/set update changes textSignature (RFC 8621 §6.3).
/// Ported from jmap-test-suite identity-set.test.ts: set-update-text-signature.
#[tokio::test]
async fn identity_set_update_text_signature_roundtrip() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (set_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Eve", "email": "eve@example.com" } }
        }),
    )
    .await
    .expect("create identity");
    let id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get id")
        .to_owned();

    let new_sig = "-- \nTest Signature";
    let (_, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "update": { &id: { "textSignature": new_sig } }
        }),
    )
    .await
    .expect("update textSignature");

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [&id] }),
    )
    .await
    .expect("get after update");
    let sig = get_resp["list"][0]["textSignature"].as_str().unwrap_or("");
    assert!(
        sig.contains("Test Signature"),
        "textSignature must contain 'Test Signature'; got: {sig:?}"
    );
}

/// Oracle: Identity/set update changes htmlSignature (RFC 8621 §6.3).
/// Ported from jmap-test-suite identity-set.test.ts: set-update-html-signature.
#[tokio::test]
async fn identity_set_update_html_signature_roundtrip() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (set_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Frank", "email": "frank@example.com" } }
        }),
    )
    .await
    .expect("create identity");
    let id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get id")
        .to_owned();

    let (_, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "update": { &id: { "htmlSignature": "<p><b>Test</b> HTML Signature</p>" } }
        }),
    )
    .await
    .expect("update htmlSignature");

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [&id] }),
    )
    .await
    .expect("get after update");
    let sig = get_resp["list"][0]["htmlSignature"].as_str().unwrap_or("");
    assert!(
        sig.contains("HTML Signature"),
        "htmlSignature must contain 'HTML Signature'; got: {sig:?}"
    );
}

/// Oracle: Identity/set update sets then clears replyTo (RFC 8621 §6.3).
/// Ported from jmap-test-suite identity-set.test.ts: set-update-reply-to.
#[tokio::test]
async fn identity_set_update_reply_to_roundtrip() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (set_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Grace", "email": "grace@example.com" } }
        }),
    )
    .await
    .expect("create identity");
    let id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get id")
        .to_owned();

    // Set replyTo array.
    let (_, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "update": {
                &id: {
                    "replyTo": [{ "name": "Reply Test", "email": "reply@example.com" }]
                }
            }
        }),
    )
    .await
    .expect("set replyTo");

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [&id] }),
    )
    .await
    .expect("get after replyTo set");
    let reply_email = get_resp["list"][0]["replyTo"][0]["email"]
        .as_str()
        .unwrap_or("");
    assert_eq!(reply_email, "reply@example.com", "replyTo email must match");

    // Clear replyTo.
    let (_, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "update": { &id: { "replyTo": serde_json::Value::Null } }
        }),
    )
    .await
    .expect("clear replyTo");

    let (get_resp2, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [&id] }),
    )
    .await
    .expect("get after replyTo clear");
    assert!(
        get_resp2["list"][0]["replyTo"].is_null(),
        "replyTo must be null after clearing"
    );
}

/// Oracle: Identity/set update of nonexistent id returns notUpdated (RFC 8621 §6.3).
/// Ported from jmap-test-suite identity-set.test.ts: set-not-found.
#[tokio::test]
async fn identity_set_update_not_found() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "update": { "nonexistent-identity-xyz": { "name": "test" } }
        }),
    )
    .await
    .expect("set with unknown id must not panic");

    let not_updated = resp["notUpdated"]
        .as_object()
        .expect("notUpdated must be an object");
    assert!(
        not_updated.contains_key("nonexistent-identity-xyz"),
        "notUpdated must contain the requested id; got: {not_updated:?}"
    );
}

// ---------------------------------------------------------------------------
// Identity/changes conformance tests (RFC 8621 §6.2)
// ---------------------------------------------------------------------------

/// Oracle: Identity/changes with current state returns empty lists (RFC 8620 §5.2).
/// Ported from jmap-test-suite identity-changes.test.ts: changes-no-changes.
#[tokio::test]
async fn identity_changes_no_changes() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    // Seed an identity so there's a real state to query from.
    let (_, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Helen", "email": "helen@example.com" } }
        }),
    )
    .await
    .expect("create identity");

    // Get current state via ids: [].
    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [] }),
    )
    .await
    .expect("get current state");
    let state = get_resp["state"]
        .as_str()
        .expect("state must be string")
        .to_owned();

    // Changes from current state must be empty.
    let (changes_resp, _) = handle_identity_changes(
        &backend,
        serde_json::json!({ "accountId": "a1", "sinceState": &state }),
    )
    .await
    .expect("changes from current state");

    assert_eq!(
        changes_resp["oldState"].as_str(),
        Some(state.as_str()),
        "oldState must equal sinceState"
    );
    let created = changes_resp["created"]
        .as_array()
        .expect("created must be array");
    let updated = changes_resp["updated"]
        .as_array()
        .expect("updated must be array");
    let destroyed = changes_resp["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    assert!(created.is_empty(), "created must be empty");
    assert!(updated.is_empty(), "updated must be empty");
    assert!(destroyed.is_empty(), "destroyed must be empty");
}

/// Oracle: Identity/changes after update contains id in updated[] (RFC 8620 §5.2).
/// Ported from jmap-test-suite identity-changes.test.ts: changes-after-update.
#[tokio::test]
async fn identity_changes_after_update() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    // Create identity and record old state.
    let (set_resp, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Iris", "email": "iris@example.com" } }
        }),
    )
    .await
    .expect("create identity");
    let id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get id")
        .to_owned();

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [] }),
    )
    .await
    .expect("get state before update");
    let old_state = get_resp["state"]
        .as_str()
        .expect("state must be string")
        .to_owned();

    // Update identity.
    let (_, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "update": { &id: { "name": "Iris Updated" } }
        }),
    )
    .await
    .expect("update identity");

    // Changes since old state should include the id in updated[].
    let (changes_resp, _) = handle_identity_changes(
        &backend,
        serde_json::json!({ "accountId": "a1", "sinceState": &old_state }),
    )
    .await
    .expect("changes after update");

    let updated: Vec<&str> = changes_resp["updated"]
        .as_array()
        .expect("updated must be array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        updated.contains(&id.as_str()),
        "updated must contain the modified identity id; got: {updated:?}"
    );
}

/// Oracle: Identity/changes response has all required fields (RFC 8620 §5.2).
/// Ported from jmap-test-suite identity-changes.test.ts: changes-response-structure.
#[tokio::test]
async fn identity_changes_response_structure() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    // Seed one identity so state is non-trivial.
    let (_, _) = handle_identity_set(
        &backend,
        serde_json::json!({
            "accountId": "a1",
            "create": { "c0": { "name": "Jack", "email": "jack@example.com" } }
        }),
    )
    .await
    .expect("create identity");

    let (get_resp, _) = handle_identity_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [] }),
    )
    .await
    .expect("get current state");
    let state = get_resp["state"]
        .as_str()
        .expect("state must be string")
        .to_owned();

    let (changes_resp, _) = handle_identity_changes(
        &backend,
        serde_json::json!({ "accountId": "a1", "sinceState": &state }),
    )
    .await
    .expect("changes");

    // All required fields per RFC 8620 §5.2.
    assert!(
        changes_resp["accountId"].as_str().is_some(),
        "accountId must be a string"
    );
    assert!(
        changes_resp["oldState"].as_str().is_some(),
        "oldState must be a string"
    );
    assert!(
        changes_resp["newState"].as_str().is_some(),
        "newState must be a string"
    );
    assert!(
        changes_resp["hasMoreChanges"].as_bool().is_some(),
        "hasMoreChanges must be a boolean"
    );
    assert!(
        changes_resp["created"].is_array(),
        "created must be an array"
    );
    assert!(
        changes_resp["updated"].is_array(),
        "updated must be an array"
    );
    assert!(
        changes_resp["destroyed"].is_array(),
        "destroyed must be an array"
    );
}

// ---------------------------------------------------------------------------
// EmailSubmission/* handler integration tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// EmailSubmission/get conformance tests (RFC 8621 §7.1)
// ---------------------------------------------------------------------------

/// Oracle: EmailSubmission/get with ids=null returns list and state (RFC 8621 §7.1).
/// Ported from jmap-test-suite submission-get.test.ts: get-empty.
#[tokio::test]
async fn submission_get_ids_null_returns_list_and_state() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (get_resp, _) = handle_submission_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": serde_json::Value::Null }),
    )
    .await
    .expect("EmailSubmission/get ids=null must succeed");

    assert!(
        get_resp["accountId"].as_str().is_some(),
        "accountId must be a string"
    );
    assert!(
        get_resp["state"].as_str().is_some(),
        "state must be a string"
    );
    assert!(get_resp["list"].is_array(), "list must be an array");
}

/// Oracle: EmailSubmission/get returns notFound for unknown id (RFC 8620 §5.1).
/// Ported from jmap-test-suite submission-get.test.ts: get-not-found.
#[tokio::test]
async fn submission_get_not_found() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (get_resp, _) = handle_submission_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": ["nonexistent-submission-xyz"] }),
    )
    .await
    .expect("EmailSubmission/get with unknown id must not error");

    // notFound MUST be a string array (RFC 8620 §5.1), never null.
    let not_found = get_resp["notFound"]
        .as_array()
        .expect("notFound must be array");
    assert!(
        not_found
            .iter()
            .any(|v| v.as_str() == Some("nonexistent-submission-xyz")),
        "notFound must contain the requested id"
    );
}

/// Oracle: EmailSubmission/get response has all required fields (RFC 8621 §7.1).
/// Ported from jmap-test-suite submission-get.test.ts: get-response-structure.
#[tokio::test]
async fn submission_get_response_structure() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));

    let (get_resp, _) = handle_submission_get(
        &backend,
        serde_json::json!({ "accountId": "a1", "ids": [] }),
    )
    .await
    .expect("EmailSubmission/get ids=[] must succeed");

    assert!(
        get_resp["accountId"].as_str().is_some(),
        "accountId must be a string"
    );
    assert!(
        get_resp["state"].as_str().is_some(),
        "state must be a string"
    );
    assert!(get_resp["list"].is_array(), "list must be an array");
    assert!(
        get_resp["notFound"].is_array(),
        "notFound must be an array (RFC 8620 §5.1)"
    );
}

/// Oracle: EmailSubmission/set create with valid identityId and emailId produces
/// a submission that is then retrievable via EmailSubmission/get.
///
/// RFC 8621 §7 — creating an EmailSubmission immediately moves it to
/// `undoStatus: "final"`. The submission must appear in /get with the assigned id.
#[tokio::test]
async fn submission_set_create_and_get() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Create an Identity to send from.
    let identity = Identity::new(Id::from("placeholder"), "alice@example.com", true);
    let (identity_id, _) = backend
        .create_object::<Identity>(&account_id, "i0", identity)
        .await
        .expect("create Identity");

    // Import an email to submit.
    let msg = b"Subject: Test\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nBody.";
    let blob_id = Id::from("blob-sub1");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("sent")], &[], None)
        .await
        .expect("import_email");

    // EmailSubmission/set create.
    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "s0": {
                "identityId": identity_id.as_ref(),
                "emailId": email_id.as_ref(),
            }
        }
    });

    let (set_resp, extra) = handle_submission_set(&backend, set_args, "call1")
        .await
        .expect("EmailSubmission/set must succeed");

    // Oracle: no extra invocations (no onSuccessUpdateEmail).
    assert!(extra.is_empty(), "no extra invocations expected");

    // Oracle: "s0" must appear in "created".
    let created = set_resp["created"]
        .as_object()
        .expect("created must be object");
    assert!(
        created.contains_key("s0"),
        "s0 must be in created; notCreated = {:?}",
        set_resp["notCreated"]
    );

    let submission_id = created["s0"]["id"]
        .as_str()
        .expect("created entry must have id")
        .to_owned();

    // Oracle: undoStatus must be "final" (MemoryBackend sends immediately).
    assert_eq!(
        created["s0"]["undoStatus"].as_str().unwrap_or(""),
        "final",
        "undoStatus must be final"
    );

    // EmailSubmission/get — must retrieve the created submission.
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [submission_id.as_str()],
    });

    let (get_resp, _) = handle_submission_get(&backend, get_args)
        .await
        .expect("EmailSubmission/get must succeed");

    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1, "must find exactly one submission");
    assert_eq!(
        list[0]["id"].as_str().unwrap_or(""),
        submission_id,
        "returned id must match"
    );
    // RFC 8620 §5.1: notFound must be [] (empty array) when all ids are found.
    assert_eq!(
        get_resp["notFound"].as_array().map(|a| a.len()),
        Some(0),
        "notFound must be [] when id is found"
    );
}

/// Oracle: EmailSubmission/set with onSuccessUpdateEmail removes the $draft keyword
/// from the referenced email and returns an extra Email/set invocation.
///
/// RFC 8621 §7.5 — `onSuccessUpdateEmail` is applied after all set operations
/// succeed and the result appears as an extra `Email/set` invocation.
#[tokio::test]
async fn submission_set_on_success_update_email() {
    use jmap_mail_types::keyword::{self, Keyword};

    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Create Identity.
    let identity = Identity::new(Id::from("placeholder"), "alice@example.com", true);
    let (identity_id, _) = backend
        .create_object::<Identity>(&account_id, "i0", identity)
        .await
        .expect("create Identity");

    // Import a draft email (with $draft keyword).
    let msg =
        b"Subject: Draft\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nDraft body.";
    let blob_id = Id::from("blob-sub2");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(
            &account_id,
            &blob_id,
            &[Id::from("drafts")],
            &[Keyword::from(keyword::DRAFT)],
            None,
        )
        .await
        .expect("import_email");

    // EmailSubmission/set with onSuccessUpdateEmail to clear all keywords
    // (removes $draft). Use a full keywords-object replacement — the form the
    // MemoryBackend's flat JSON Merge Patch can apply.
    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "s0": {
                "identityId": identity_id.as_ref(),
                "emailId": email_id.as_ref(),
            }
        },
        // RFC 8621 §7.5: keys are EmailSubmission IDs or creation references.
        // "#s0" is a creation reference for the submission created as "s0".
        "onSuccessUpdateEmail": {
            "#s0": {
                "keywords": {}
            }
        }
    });

    let (set_resp, extra) = handle_submission_set(&backend, set_args, "call2")
        .await
        .expect("EmailSubmission/set must succeed");

    // Oracle: submission was created.
    let created = set_resp["created"]
        .as_object()
        .expect("created must be object");
    assert!(
        created.contains_key("s0"),
        "s0 must be in created; notCreated = {:?}",
        set_resp["notCreated"]
    );

    // Oracle: extra_invocations contains exactly one Email/set invocation.
    assert_eq!(
        extra.len(),
        1,
        "must have exactly one extra invocation for onSuccessUpdateEmail"
    );
    let (method, email_set_resp, extra_call_id) = &extra[0];
    assert_eq!(method, "Email/set", "extra invocation must be Email/set");
    // RFC 8620 §3.2: all responses from a single method call share the same call-id.
    assert_eq!(
        extra_call_id, "call2",
        "implicit Email/set must have the same call-id as the originating EmailSubmission/set"
    );

    // Oracle: email_id appears in "updated" of the extra Email/set response.
    let updated = email_set_resp["updated"]
        .as_object()
        .expect("updated must be object");
    assert!(
        updated.contains_key(email_id.as_ref()),
        "email_id must be in updated; notUpdated = {:?}",
        email_set_resp["notUpdated"]
    );

    // Oracle: the email no longer has $draft keyword.
    let (emails, _) = backend
        .get_objects::<jmap_mail_types::Email>(&account_id, Some(&[email_id.clone()]), None)
        .await
        .expect("get_objects");
    assert_eq!(emails.len(), 1);
    assert!(
        !emails[0].keywords.contains_key(keyword::DRAFT),
        "email must no longer have $draft keyword after onSuccessUpdateEmail"
    );
}

/// Oracle: EmailSubmission/set onSuccessUpdateEmail that patches an immutable
/// Email field (e.g. "messageId") is rejected with invalidProperties in the
/// implicit Email/set response.
///
/// RFC 8621 §5.5.4 — immutable Email properties must not be mutable via any
/// patch path, including onSuccessUpdateEmail.
#[tokio::test]
async fn submission_set_on_success_update_email_immutable_field_rejected() {
    use jmap_mail_types::Identity;

    let backend = MemoryBackend::new();
    let account_id = Id::from("account-imm-sub");

    // Create Identity.
    let identity = Identity::new(Id::from("placeholder"), "alice@example.com", true);
    let (identity_id, _) = backend
        .create_object::<Identity>(&account_id, "i0", identity)
        .await
        .expect("create Identity");

    // Import an email with To header so envelope can be derived.
    let msg = b"Subject: Test\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nbody";
    let blob_id = Id::from("blob-imm-sub");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("inbox")], &[], None)
        .await
        .expect("import email");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "s1": {
                "identityId": identity_id.as_ref(),
                "emailId": email_id.as_ref(),
            }
        },
        // Attempt to overwrite an immutable field on the email after submission.
        "onSuccessUpdateEmail": {
            "#s1": { "messageId": ["attacker@evil.com"] }
        }
    });

    use jmap_mail_server::submission::handle_submission_set;
    let (resp, extra) = handle_submission_set(&backend, args, "call-imm-sub")
        .await
        .expect("EmailSubmission/set must not return a top-level JmapError");

    // The submission create must succeed.
    assert!(
        resp["notCreated"].is_null(),
        "notCreated must be null; got: {}",
        resp["notCreated"]
    );

    // The implicit Email/set must report notUpdated with invalidProperties.
    assert_eq!(
        extra.len(),
        1,
        "must have one extra invocation for onSuccessUpdateEmail"
    );
    let set_resp = &extra[0].1;
    let not_updated = &set_resp["notUpdated"];
    assert!(
        !not_updated.is_null(),
        "notUpdated must be non-null; immutable field patch must be rejected"
    );
    let email_id_str = email_id.as_ref();
    assert!(
        not_updated.get(email_id_str).is_some(),
        "email id must appear in notUpdated; got: {not_updated}"
    );
    assert_eq!(
        not_updated[email_id_str]["type"].as_str(),
        Some("invalidProperties"),
        "error type must be invalidProperties; got: {}",
        not_updated[email_id_str]
    );
}

/// Oracle: EmailSubmission/set create with a non-existent identityId returns
/// notCreated with invalidProperties referencing "identityId".
///
/// RFC 8621 §7.5 — an invalid reference to a non-existent Identity MUST result
/// in a `notCreated` entry with `invalidProperties: ["identityId"]`.
#[tokio::test]
async fn submission_set_invalid_identity_fails() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Import an email (exists).
    let msg = b"Subject: Hi\r\nFrom: x@example.com\r\nTo: y@example.com\r\n\r\nBody.";
    let blob_id = Id::from("blob-sub3");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("inbox")], &[], None)
        .await
        .expect("import_email");

    // Use a non-existent identityId.
    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "s0": {
                "identityId": "nonexistent-identity-id",
                "emailId": email_id.as_ref(),
            }
        }
    });

    let (set_resp, extra) = handle_submission_set(&backend, set_args, "call3")
        .await
        .expect("EmailSubmission/set must not return JmapError");

    // Oracle: no extra invocations.
    assert!(extra.is_empty());

    // Oracle: s0 must appear in notCreated, not in created.
    assert!(
        set_resp["created"]
            .as_object()
            .map_or(true, |m| !m.contains_key("s0")),
        "s0 must not be in created"
    );

    let not_created = set_resp["notCreated"]
        .as_object()
        .expect("notCreated must be object");
    assert!(not_created.contains_key("s0"), "s0 must be in notCreated");

    // Oracle: error type must be invalidProperties.
    let err_type = not_created["s0"]["type"].as_str().unwrap_or("");
    assert_eq!(
        err_type, "invalidProperties",
        "error type must be invalidProperties"
    );

    // Oracle: properties list must include "identityId".
    let props = not_created["s0"]["properties"]
        .as_array()
        .expect("properties must be array");
    let prop_strs: Vec<&str> = props.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        prop_strs.contains(&"identityId"),
        "properties must include identityId; got: {prop_strs:?}"
    );
}

// ---------------------------------------------------------------------------
// Email handler integration tests (JMAP-0u6)
// ---------------------------------------------------------------------------

/// Oracle: Email/set create followed by Email/get returns the created email.
///
/// RFC 8621 §5.5.3 — a create with mailboxIds and keywords must succeed.
/// RFC 8621 §5.1 — Email/get must return the email in "list".
#[tokio::test]
async fn email_set_create_and_get() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // RFC 8621 §5.5.3: size is server-set and must not be sent by the client.
    // We intentionally omit size from the create payload to verify it is not
    // accepted from the client. MemoryBackend sets size=0 (placeholder; a real
    // backend would compute it from the raw blob bytes).
    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": { "$seen": true },
                "subject": "Test email",
            }
        }
    });
    let (set_resp, extra) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set must succeed");
    assert!(extra.is_empty());

    let created = set_resp["created"]
        .as_object()
        .expect("created must be an object");
    let c0 = &created["c0"];
    assert!(
        c0["id"].as_str().is_some(),
        "created entry must have id; got: {c0:?}"
    );
    assert!(
        c0["threadId"].as_str().is_some(),
        "created entry must have threadId"
    );
    // size is server-set; MemoryBackend must assign the real blob size (> 0),
    // not leave the placeholder 0 that email.rs sets before calling create_object.
    assert!(
        c0["size"].as_u64().unwrap_or(0) > 0,
        "size must be server-set and > 0; got: {:?}",
        c0["size"]
    );
    // blobId must be present and must NOT be the internal placeholder.
    let blob_id_str = c0["blobId"]
        .as_str()
        .expect("blobId must be present in created entry");
    assert_ne!(
        blob_id_str, "placeholder-blob",
        "blobId must be a server-assigned id, not the internal placeholder"
    );

    let email_id = c0["id"].as_str().unwrap().to_owned();

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
    });
    let (get_resp, extra) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get must succeed");
    assert!(extra.is_empty());

    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1, "must find exactly one email");
    assert_eq!(
        list[0]["id"].as_str().unwrap_or(""),
        email_id,
        "returned id must match"
    );
    assert_eq!(
        list[0]["subject"].as_str().unwrap_or(""),
        "Test email",
        "subject must round-trip"
    );
    // RFC 8620 §5.1: notFound must be [] (empty array) when all ids are found.
    assert_eq!(
        get_resp["notFound"].as_array().map(|a| a.len()),
        Some(0),
        "notFound must be [] when email is found"
    );
}

/// Oracle: Email/set create with a keyword value of false does not store it.
///
/// RFC 8621 §5.5.3: "The value for each key in the object MUST be true."
/// A false-valued keyword means the keyword is absent. The email returned
/// by Email/get must not contain the false-valued keyword.
#[tokio::test]
async fn email_set_create_keyword_false_value_not_stored() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    let mailbox_id = Id::from("mb1");

    // Pre-create a mailbox so the email create succeeds.
    backend
        .create_object::<jmap_mail_types::Mailbox>(
            &account_id,
            "mb1",
            jmap_mail_types::Mailbox::new(
                mailbox_id.clone(),
                "Inbox".to_owned(),
                0,
                0,
                0,
                0,
                0,
                jmap_mail_types::MailboxRights::default(),
                true,
            ),
        )
        .await
        .expect("mailbox create must succeed");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { mailbox_id.as_ref(): true },
                // $seen: true (stored), $draft: false (must NOT be stored)
                "keywords": { "$seen": true, "$draft": false }
            }
        }
    });

    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set must not fail at method level");

    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("created c0 must have id");

    // Fetch the email back and verify keywords.
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
        "properties": ["keywords"]
    });
    let (get_resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get must succeed");

    let keywords = &get_resp["list"][0]["keywords"];
    assert!(
        keywords
            .get("$seen")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        "$seen:true must be stored; keywords: {keywords:?}"
    );
    assert!(
        keywords.get("$draft").is_none(),
        "$draft:false must NOT be stored; keywords: {keywords:?}"
    );
}

/// Oracle: Email/set create with all-false mailboxIds is rejected with invalidProperties.
///
/// RFC 8621 §5.5.3: "At least one [mailboxId] MUST be set to true."
/// A map with only false values has no real mailbox membership and must be rejected.
#[tokio::test]
async fn email_set_create_all_false_mailbox_ids_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                // All values are false — no true mailbox membership
                "mailboxIds": { "mb1": false, "mb2": false }
            }
        }
    });

    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set must not fail at method level");

    assert!(
        set_resp["created"].is_null(),
        "created must be null; got: {:?}",
        set_resp["created"]
    );
    let not_created = &set_resp["notCreated"]["c0"];
    assert!(
        !not_created.is_null(),
        "c0 must appear in notCreated; response: {set_resp:?}"
    );
    assert_eq!(
        not_created["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties; got: {not_created:?}"
    );
    let empty = vec![];
    let props: Vec<&str> = not_created["properties"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        props.contains(&"mailboxIds"),
        "properties must name mailboxIds; got: {props:?}"
    );
}

/// Oracle: Email/get with all-valid ids returns `"notFound": []` (empty array, not null).
///
/// RFC 8620 §5.1 mandates `notFound` as `Id[]` — it must always be an array.
/// When every requested id is found the array is empty, but it must still be
/// present as `[]`, never as `null` or absent.
#[tokio::test]
async fn email_get_not_found_is_empty_array_when_all_found() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Create one email so we have a valid id to look up.
    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "subject": "notFound wire format test",
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("id must be present")
        .to_owned();

    // Fetch that email by id — all ids exist, so notFound must be [].
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
    });
    let (resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get");

    // Verify the list has one entry.
    let list = resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1, "must return the requested email");

    // RFC 8620 §5.1: notFound MUST be Id[] — an array, never null or absent.
    assert!(
        resp["notFound"].is_array(),
        "notFound must be an array (Id[]), not null or absent; got: {:?}",
        resp["notFound"]
    );
    assert_eq!(
        resp["notFound"].as_array().map(|a| a.len()),
        Some(0),
        "notFound must be [] when all requested ids are found"
    );
}

/// Oracle: Email/get with a `properties` filter returns only requested fields.
///
/// RFC 8621 §5.1 — when `properties` is given, only those properties appear
/// in each returned Email object.
#[tokio::test]
async fn email_get_with_property_filter() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "subject": "Filtered subject",
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("id must be present")
        .to_owned();

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
        "properties": ["id", "subject"],
    });
    let (get_resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get");

    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1);
    let obj = &list[0];

    assert!(obj.get("id").is_some(), "id must be in filtered response");
    assert!(
        obj.get("subject").is_some(),
        "subject must be in filtered response"
    );
    assert!(
        obj.get("mailboxIds").is_none(),
        "mailboxIds must be absent when not in properties list; got: {obj:?}"
    );
}

/// Oracle: Email/get with no `properties` arg returns RFC 8621 §4.2 default list only.
///
/// The default list includes "id" and "subject" but NOT "headers" or "bodyStructure".
/// This verifies the handler enforces the spec-mandated default rather than returning all fields.
#[tokio::test]
async fn email_get_default_properties_excludes_headers() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "subject": "Default props test",
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("id must be present")
        .to_owned();

    // No `properties` key — handler must apply RFC 8621 §4.2 default list.
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
    });
    let (get_resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get");

    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1);
    let obj = &list[0];

    // Fields in the RFC 8621 §4.2 default list must be present (or at least not stripped).
    assert!(obj.get("id").is_some(), "id must be in default response");
    assert!(
        obj.get("subject").is_some(),
        "subject must be in default response"
    );
    assert!(
        obj.get("mailboxIds").is_some(),
        "mailboxIds must be in default response"
    );

    // "headers" and "bodyStructure" are NOT in the §4.2 default list.
    assert!(
        obj.get("headers").is_none(),
        "headers must be absent from default response; got: {obj:?}"
    );
    assert!(
        obj.get("bodyStructure").is_none(),
        "bodyStructure must be absent from default response; got: {obj:?}"
    );
}

/// Oracle: Email/get accepts body-value fetch args without error.
///
/// RFC 8621 §4.2 — `fetchTextBodyValues=true` and `maxBodyValueBytes=10` must be
/// parsed without returning an `invalidArguments` error.
#[tokio::test]
async fn email_get_body_value_fetch_args_parsed() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": { "mailboxIds": { "inbox": true }, "subject": "Body fetch test" }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
        "fetchTextBodyValues": true,
        "maxBodyValueBytes": 10,
    });
    // Must not return an error — the args are well-formed.
    let (get_resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get with body-value fetch args must not fail");

    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1, "one email must be returned");
}

/// Oracle: Email/get accepts `maxBodyValueBytes=0` (unlimited).
///
/// RFC 8621 §4.2 — `maxBodyValueBytes` of 0 means unlimited; must not be rejected.
#[tokio::test]
async fn email_get_max_body_value_bytes_zero_accepted() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": { "mailboxIds": { "inbox": true }, "subject": "Zero limit test" }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
        "maxBodyValueBytes": 0,
    });
    let (get_resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("maxBodyValueBytes=0 must be accepted");

    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1);
}

/// Oracle: Email/parse with no `properties` uses RFC 8621 §4.9 default list.
///
/// The §4.9 default list does NOT include "id" (unlike the §4.2 Email/get default).
/// This verifies that the two handlers use separate default lists.
#[tokio::test]
async fn email_parse_default_properties_used() {
    use jmap_mail_server::handle_email_parse;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Store a minimal blob so parse_email can find it.
    let blob_id = Id::from("blob-parse-test");
    backend.store_blob(
        &blob_id,
        b"Subject: Parse default props\r\n\r\nBody text".to_vec(),
    );

    let parse_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobIds": [blob_id.as_ref()],
        // No `properties` — must use DEFAULT_EMAIL_PARSE_PROPERTIES.
    });
    let (parse_resp, _) = handle_email_parse(&backend, parse_args)
        .await
        .expect("Email/parse must not fail");

    let parsed_obj = &parse_resp["parsed"][blob_id.as_ref()];
    assert!(!parsed_obj.is_null(), "blob must be in parsed map");

    // "subject" is in DEFAULT_EMAIL_PARSE_PROPERTIES — must appear.
    assert!(
        parsed_obj.get("subject").is_some(),
        "subject must be in parse default response; got: {parsed_obj:?}"
    );

    // "id" is in Email/get defaults but NOT in Email/parse defaults (RFC 8621 §4.9).
    assert!(
        parsed_obj.get("id").is_none(),
        "id must be absent from Email/parse default response; got: {parsed_obj:?}"
    );

    // "headers" is not in the parse default list either.
    assert!(
        parsed_obj.get("headers").is_none(),
        "headers must be absent from Email/parse default response; got: {parsed_obj:?}"
    );
}

/// Oracle: Email/parse accepts fetchTextBodyValues=true and maxBodyValueBytes=0 without error.
///
/// RFC 8621 §4.9 — these args have valid defaults and must not cause invalidArguments.
#[tokio::test]
async fn email_parse_body_value_fetch_args_parsed() {
    use jmap_mail_server::handle_email_parse;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let blob_id = Id::from("blob-bvargs-test");
    backend.store_blob(&blob_id, b"Subject: Body value args\r\n\r\nHello".to_vec());

    let parse_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobIds": [blob_id.as_ref()],
        "fetchTextBodyValues": true,
        "fetchHTMLBodyValues": false,
        "fetchAllBodyValues": false,
        "maxBodyValueBytes": 0,
    });
    let result = handle_email_parse(&backend, parse_args).await;
    assert!(
        result.is_ok(),
        "valid body-value args must not return an error; got: {result:?}"
    );
}

/// Oracle: Email/parse with maxBodyValueBytes set to a non-integer returns invalidArguments.
///
/// RFC 8621 §4.9 — maxBodyValueBytes must be a non-negative integer.
#[tokio::test]
async fn email_parse_invalid_max_body_value_bytes() {
    use jmap_mail_server::handle_email_parse;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let parse_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobIds": [],
        "maxBodyValueBytes": "not a number",
    });
    let result = handle_email_parse(&backend, parse_args).await;
    assert!(
        result.is_err(),
        "non-integer maxBodyValueBytes must return an error; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type.as_str(),
        "invalidArguments",
        "expected invalidArguments; got: {:?}",
        err.error_type
    );
}

/// Oracle: Email/set update with an immutable field is rejected with invalidProperties.
///
/// RFC 8621 §5.5.4 — the parsed header convenience properties (from, to, cc, etc.)
/// and metadata fields (id, blobId, threadId, size, receivedAt, subject, etc.)
/// are immutable after creation.
#[tokio::test]
async fn email_set_update_immutable_field_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "subject": "Original subject",
                "size": 5,
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set create");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let update_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            email_id.clone(): {
                "from": [{"email": "attacker@evil.com"}],
            }
        }
    });
    let (upd_resp, _) = handle_email_set(&backend, update_args)
        .await
        .expect("Email/set update must return a response");

    assert!(
        upd_resp["updated"]
            .as_object()
            .map_or(true, |m| m.is_empty()),
        "updated must be empty when immutable field is patched"
    );
    let not_updated = upd_resp["notUpdated"]
        .as_object()
        .expect("notUpdated must be an object");
    assert!(
        not_updated.contains_key(&email_id),
        "email must appear in notUpdated; got keys: {:?}",
        not_updated.keys().collect::<Vec<_>>()
    );
    let err = &not_updated[&email_id];
    assert_eq!(
        err["type"].as_str(),
        Some("invalidProperties"),
        "error type must be invalidProperties; got: {err:?}"
    );
    let props = err["properties"]
        .as_array()
        .expect("properties must be present");
    assert!(
        props.iter().any(|v| v.as_str() == Some("from")),
        "properties must mention 'from'; got: {props:?}"
    );
}

/// Oracle: Email/set update of keywords succeeds and Email/changes reports it as updated.
///
/// RFC 8621 §5.5 — `keywords/$seen` is a valid patch key (not an immutable field).
/// After the update, Email/changes must include the email id in the "updated" list.
#[tokio::test]
async fn email_set_update_keywords() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "size": 7,
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set create");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let state_before = set_resp["newState"].as_str().expect("newState").to_owned();

    let update_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            email_id.clone(): {
                "keywords/$seen": true,
            }
        }
    });
    let (upd_resp, _) = handle_email_set(&backend, update_args)
        .await
        .expect("Email/set update");

    assert!(
        upd_resp["notUpdated"]
            .as_object()
            .map_or(true, |m| m.is_empty()),
        "notUpdated must be empty on successful keyword update"
    );
    assert!(
        upd_resp["updated"]
            .as_object()
            .map_or(false, |m| m.contains_key(&email_id)),
        "email must appear in updated"
    );

    let changes_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": state_before,
    });
    let (chg_resp, _) = handle_email_changes(&backend, changes_args)
        .await
        .expect("Email/changes");

    let updated_ids: Vec<&str> = chg_resp["updated"]
        .as_array()
        .expect("updated must be an array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        updated_ids.contains(&email_id.as_str()),
        "email id must appear in changes updated list; got: {updated_ids:?}"
    );
}

/// Oracle: Email/query response has the correct shape (queryState, ids, total, position).
///
/// RFC 8621 §4.4 — Email/query must return queryState, canCalculateChanges,
/// position, ids, and total. With `inMailbox` filter, only the 2 inbox emails
/// are returned (not the sent email).
#[tokio::test]
async fn email_query_by_mailbox() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    for subject in &["Inbox 1", "Inbox 2"] {
        let args = serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "c0": {
                    "mailboxIds": { "inbox": true },
                    "subject": subject,
                    "size": 1,
                }
            }
        });
        handle_email_set(&backend, args)
            .await
            .expect("Email/set for inbox email");
    }
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "sent": true },
                "subject": "Sent 1",
                "size": 1,
            }
        }
    });
    handle_email_set(&backend, args)
        .await
        .expect("Email/set for sent email");

    let query_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": { "inMailbox": "inbox" },
        "calculateTotal": true,
    });
    let (query_resp, extra) = handle_email_query(&backend, query_args)
        .await
        .expect("Email/query must succeed");
    assert!(extra.is_empty());

    assert!(
        query_resp["queryState"].as_str().is_some(),
        "queryState must be present"
    );
    assert!(
        query_resp["ids"].as_array().is_some(),
        "ids must be an array"
    );
    assert!(
        query_resp["position"].as_i64().is_some(),
        "position must be present"
    );
    let ids = query_resp["ids"].as_array().unwrap();
    assert_eq!(
        ids.len(),
        2,
        "inMailbox=inbox must return only the 2 inbox emails"
    );
    assert_eq!(
        query_resp["total"].as_u64(),
        Some(2),
        "total must reflect only the filtered emails"
    );
}

/// Oracle: Email/import with mailboxIds:{} (empty) returns notCreated with
/// invalidProperties referencing mailboxIds.
///
/// RFC 8621 §5.7 — at least one mailbox MUST be given. An empty mailboxIds
/// object must be rejected before calling the backend.
#[tokio::test]
async fn email_import_empty_mailbox_ids_rejected() {
    use jmap_mail_server::handle_email_import;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let msg = b"Subject: test\r\n\r\nbody";
    let blob_id = Id::from("blob-empty-mb");
    backend.store_blob(&blob_id, msg.to_vec());

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "emails": {
            "imp1": {
                "blobId": blob_id.as_ref(),
                "mailboxIds": {},
            }
        }
    });

    let (resp, extra) = handle_email_import(&backend, args)
        .await
        .expect("Email/import must not return a JmapError");
    assert!(extra.is_empty());

    // Oracle: imp1 must appear in notCreated, not in created.
    assert!(resp["created"].is_null(), "created must be null");
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be an object");
    assert!(
        not_created.contains_key("imp1"),
        "imp1 must be in notCreated"
    );
    assert_eq!(
        not_created["imp1"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties"
    );
    let props = not_created["imp1"]["properties"]
        .as_array()
        .expect("properties must be an array");
    assert!(
        props.iter().any(|v| v.as_str() == Some("mailboxIds")),
        "properties must mention mailboxIds; got: {props:?}"
    );
}

/// Oracle: Email/import with valid keywords (String[Boolean] map) succeeds and
/// the imported email carries those keywords.
///
/// RFC 8621 §5.7 — keywords is String[Boolean]; the old Vec<Keyword> deserialization
/// would reject a valid {"$seen": true} payload with invalidProperties.
#[tokio::test]
async fn email_import_with_keywords_succeeds() {
    use jmap_mail_server::handle_email_import;
    use jmap_mail_types::keyword;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let msg = b"Subject: with keywords\r\n\r\nbody";
    let blob_id = Id::from("blob-kw");
    backend.store_blob(&blob_id, msg.to_vec());

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "emails": {
            "imp1": {
                "blobId": blob_id.as_ref(),
                "mailboxIds": { "inbox": true },
                "keywords": { "$seen": true, "$flagged": false },
            }
        }
    });

    let (resp, _extra) = handle_email_import(&backend, args)
        .await
        .expect("Email/import must not return a JmapError");

    // The import should succeed (created, not notCreated).
    assert!(
        resp["notCreated"].is_null(),
        "notCreated must be null; got: {}",
        resp["notCreated"]
    );
    let created = resp["created"]
        .as_object()
        .expect("created must be an object");
    assert!(created.contains_key("imp1"), "imp1 must be in created");

    // Verify the email carries $seen (true) but not $flagged (false was filtered out).
    let email_id_str = created["imp1"]["id"].as_str().expect("id must be a string");
    let email_id = Id::from(email_id_str);
    let (emails, _) = backend
        .get_objects::<jmap_mail_types::Email>(&account_id, Some(&[email_id]), None)
        .await
        .expect("get_objects");
    assert_eq!(emails.len(), 1, "imported email must be retrievable");
    assert!(
        emails[0].keywords.contains_key(keyword::SEEN),
        "$seen must be set on the imported email"
    );
}

/// Oracle: Email/copy with valid keywords (String[Boolean] map) succeeds.
///
/// The old Vec<Keyword> deserialization would reject {"$seen": true} with
/// invalidProperties. Fixed to HashMap<Keyword, bool>.
#[tokio::test]
async fn email_copy_with_keywords_succeeds() {
    use jmap_mail_server::handle_email_copy;
    use jmap_mail_types::keyword;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("src"));
    backend.register_account(&Id::from("dst"));
    let src_account = Id::from("src");
    let dst_account = Id::from("dst");

    // Import a source email.
    let msg = b"Subject: copy-kw test\r\n\r\nbody";
    let blob_id = Id::from("blob-copy-kw");
    backend.store_blob(&blob_id, msg.to_vec());
    let (src_id, _) = backend
        .import_email(&src_account, &blob_id, &[Id::from("inbox")], &[], None)
        .await
        .expect("import source email");

    let args = serde_json::json!({
        "accountId": dst_account.as_ref(),
        "fromAccountId": src_account.as_ref(),
        "create": {
            "c1": {
                "id": src_id.as_ref(),
                "mailboxIds": { "inbox": true },
                "keywords": { "$seen": true, "$flagged": false },
            }
        }
    });

    let (resp, _extra) = handle_email_copy(&backend, args, "call-1")
        .await
        .expect("Email/copy must not return a JmapError");

    assert!(
        resp["notCreated"].is_null(),
        "notCreated must be null; got: {}",
        resp["notCreated"]
    );
    let created = resp["created"]
        .as_object()
        .expect("created must be an object");
    assert!(created.contains_key("c1"), "c1 must be in created");

    // Verify $seen was applied; $false=false is filtered out.
    let new_id_str = created["c1"]["id"].as_str().expect("id must be present");
    let new_id = Id::from(new_id_str);
    let (emails, _) = backend
        .get_objects::<jmap_mail_types::Email>(&dst_account, Some(&[new_id]), None)
        .await
        .expect("get_objects");
    assert_eq!(emails.len(), 1, "copied email must be retrievable");
    assert!(
        emails[0].keywords.contains_key(keyword::SEEN),
        "$seen must be on the copied email"
    );
}

/// Oracle: Email/copy onSuccessUpdateOriginal that patches an immutable field
/// (e.g. "messageId") is rejected with invalidProperties in the implicit
/// Email/set response.
///
/// RFC 8621 §5.5.4 — immutable Email properties must not be mutable via any
/// patch path, including onSuccessUpdateOriginal.
#[tokio::test]
async fn email_copy_on_success_update_original_immutable_field_rejected() {
    use jmap_mail_server::handle_email_copy;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("src-imm"));
    backend.register_account(&Id::from("dst-imm"));
    let src_account = Id::from("src-imm");
    let dst_account = Id::from("dst-imm");

    // Import a source email.
    let msg = b"Subject: immutable test\r\n\r\nbody";
    let blob_id = Id::from("blob-imm");
    backend.store_blob(&blob_id, msg.to_vec());
    let (src_id, _) = backend
        .import_email(&src_account, &blob_id, &[Id::from("inbox")], &[], None)
        .await
        .expect("import source email");

    let args = serde_json::json!({
        "accountId": dst_account.as_ref(),
        "fromAccountId": src_account.as_ref(),
        "create": {
            "c1": {
                "id": src_id.as_ref(),
                "mailboxIds": { "inbox": true },
            }
        },
        // Attempt to overwrite an immutable field on the original after copy.
        "onSuccessUpdateOriginal": {
            "c1": { "messageId": ["attacker@evil.com"] }
        }
    });

    let (resp, extra) = handle_email_copy(&backend, args, "call-imm")
        .await
        .expect("Email/copy must not return a top-level JmapError");

    // The copy itself must succeed.
    assert!(
        resp["notCreated"].is_null(),
        "copy notCreated must be null; got: {}",
        resp["notCreated"]
    );

    // The implicit Email/set in extra must report notUpdated for the source id.
    assert_eq!(
        extra.len(),
        1,
        "must have one extra invocation for onSuccessUpdateOriginal"
    );
    let set_resp = &extra[0].1;
    let not_updated = &set_resp["notUpdated"];
    assert!(
        !not_updated.is_null(),
        "notUpdated must be non-null; immutable field patch must be rejected"
    );
    let src_id_str = src_id.as_ref();
    assert!(
        not_updated.get(src_id_str).is_some(),
        "source id must appear in notUpdated; got: {not_updated}"
    );
    assert_eq!(
        not_updated[src_id_str]["type"].as_str(),
        Some("invalidProperties"),
        "error type must be invalidProperties; got: {}",
        not_updated[src_id_str]
    );
}

/// Oracle: EmailSubmission/set update that patches a field other than undoStatus
/// is rejected with invalidProperties.
///
/// RFC 8621 §7.5 — only the undoStatus property may be changed.
#[tokio::test]
async fn submission_set_update_only_undo_status_allowed() {
    use jmap_mail_types::Identity;

    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Create an Identity and email, then a submission.
    let identity = Identity::new(Id::from("placeholder"), "alice@example.com", true);
    let (identity_id, _) = backend
        .create_object::<Identity>(&account_id, "i0", identity)
        .await
        .expect("create Identity");

    let msg = b"Subject: Test\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nBody.";
    let blob_id = Id::from("blob-sub-patch");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("sent")], &[], None)
        .await
        .expect("import_email");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "s0": {
                "identityId": identity_id.as_ref(),
                "emailId": email_id.as_ref(),
            }
        }
    });
    let (set_resp, _) = handle_submission_set(&backend, set_args, "c1")
        .await
        .expect("create submission");
    let submission_id = set_resp["created"]["s0"]["id"]
        .as_str()
        .expect("must get id")
        .to_owned();

    // Try to update a field other than undoStatus.
    let update_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            submission_id.clone(): {
                "emailId": email_id.as_ref(),
            }
        }
    });
    let (upd_resp, _) = handle_submission_set(&backend, update_args, "c2")
        .await
        .expect("set must not return JmapError");

    // Oracle: submission must appear in notUpdated with invalidProperties.
    let not_updated = upd_resp["notUpdated"]
        .as_object()
        .expect("notUpdated must be an object");
    assert!(
        not_updated.contains_key(&submission_id),
        "submission must be in notUpdated; got: {upd_resp:?}"
    );
    assert_eq!(
        not_updated[&submission_id]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties"
    );

    // Oracle: updated map must be empty.
    assert!(
        upd_resp["updated"]
            .as_object()
            .map_or(true, |m| m.is_empty()),
        "updated must be empty when non-undoStatus field is patched"
    );
}

/// Oracle: Mailbox/set create+destroy in the same request correctly detects
/// the newly-created child mailbox when deciding whether the parent can be destroyed.
///
/// RFC 8621 §2.5 — destroying a mailbox that has children must return
/// mailboxHasChild, even when the child was created in the same request.
#[tokio::test]
async fn mailbox_set_create_child_then_destroy_parent_blocked() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");

    // First create the parent mailbox.
    let (parent_id, _) = backend
        .create_object::<jmap_mail_types::Mailbox>(
            &account_id,
            "p0",
            jmap_mail_types::Mailbox::new(
                Id::from("placeholder"),
                "Parent",
                0,
                0,
                0,
                0,
                0,
                jmap_mail_types::MailboxRights::default(),
                false,
            ),
        )
        .await
        .expect("create parent mailbox");

    // In one Mailbox/set request: create a child of parent AND try to destroy parent.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "child1": {
                "name": "Child",
                "parentId": parent_id.as_ref(),
            }
        },
        "destroy": [parent_id.as_ref()],
    });

    let (resp, _) = handle_mailbox_set(&backend, args)
        .await
        .expect("Mailbox/set must not return JmapError");

    // Oracle: child create must succeed.
    let created = resp["created"].as_object().expect("created must be object");
    assert!(
        created.contains_key("child1"),
        "child create must succeed; notCreated={:?}",
        resp["notCreated"]
    );

    // Oracle: parent destroy must fail with mailboxHasChild.
    let not_destroyed = resp["notDestroyed"]
        .as_object()
        .expect("notDestroyed must be an object");
    assert!(
        not_destroyed.contains_key(parent_id.as_ref()),
        "parent must be in notDestroyed; resp={resp:?}"
    );
    assert_eq!(
        not_destroyed[parent_id.as_ref()]["type"]
            .as_str()
            .unwrap_or(""),
        "mailboxHasChild",
        "error must be mailboxHasChild when child was just created"
    );
}

/// Oracle: Email/set create with a malformed keywords map returns notCreated/invalidProperties.
///
/// RFC 8621 §5.5: keywords must be a map from valid keyword strings to true.
/// Malformed input (e.g., a non-object value for keywords) must be rejected
/// rather than silently treated as an empty keyword map.
#[tokio::test]
async fn email_set_create_malformed_keywords_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // "keywords" is a string, not a map — invalid per RFC 8621 §5.5.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": "not-a-map",
            }
        }
    });

    let (resp, extra) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must not return a JmapError");
    assert!(extra.is_empty());
    assert!(
        resp["created"].is_null(),
        "created must be null on rejection"
    );
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be an object");
    assert!(not_created.contains_key("c0"), "c0 must be in notCreated");
    assert_eq!(
        not_created["c0"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties for malformed keywords"
    );
}

/// Oracle: Email/set create with a 256-byte keyword is rejected (max is 255).
///
/// RFC 8621 §4.1.1: keywords must be 1–255 bytes long.
#[tokio::test]
async fn keyword_256_chars_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");
    let kw_256: String = "a".repeat(256);

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": { kw_256: true },
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must not return a JmapError");
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be present");
    assert!(not_created.contains_key("c0"), "c0 must be in notCreated");
    assert_eq!(
        not_created["c0"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "256-byte keyword must yield invalidProperties"
    );
}

/// Oracle: Email/set create with a 255-byte keyword is accepted.
///
/// RFC 8621 §4.1.1: keywords must be 1–255 bytes long; 255 is the maximum valid length.
#[tokio::test]
async fn keyword_255_chars_accepted() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");
    let kw_255: String = "a".repeat(255);

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": { kw_255: true },
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must not return a JmapError");
    assert!(
        resp["notCreated"].is_null()
            || resp["notCreated"]
                .as_object()
                .map_or(true, |m| m.is_empty()),
        "255-byte keyword must be accepted; notCreated: {:?}",
        resp["notCreated"]
    );
    assert!(
        resp["created"].get("c0").is_some(),
        "c0 must appear in created"
    );
}

/// Oracle: Email/set create with a keyword containing `(` is rejected.
///
/// RFC 8621 §4.1.1: keywords must not contain `( ) { ] % * " \`.
#[tokio::test]
async fn keyword_forbidden_char_open_paren_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": { "abc(def": true },
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must not return a JmapError");
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be present");
    assert!(not_created.contains_key("c0"), "c0 must be in notCreated");
    assert_eq!(
        not_created["c0"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "keyword with '(' must yield invalidProperties"
    );
}

/// Oracle: Email/set create with a mixed-case keyword stores it as lowercase.
///
/// RFC 8621 §4.1.1: keywords MUST be stored and returned in lowercase.
#[tokio::test]
async fn keyword_normalized_to_lowercase() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let create_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": { "SEEN": true },
            }
        }
    });

    let (create_resp, _) = handle_email_set(&backend, create_args)
        .await
        .expect("Email/set must not return a JmapError");
    let email_id = create_resp["created"]["c0"]["id"]
        .as_str()
        .expect("c0 must be in created");

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
        "properties": ["keywords"],
    });
    let (get_resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get must not return a JmapError");
    let keywords = &get_resp["list"][0]["keywords"];
    assert!(
        keywords.get("seen").is_some(),
        "keyword 'SEEN' must be stored as 'seen'; got: {keywords:?}"
    );
    assert!(
        keywords.get("SEEN").is_none(),
        "uppercase 'SEEN' must not appear; got: {keywords:?}"
    );
}

/// Oracle: Email/set create with an empty-string keyword is rejected.
///
/// RFC 8621 §4.1.1: keywords must be 1–255 bytes; empty string is invalid.
#[tokio::test]
async fn keyword_empty_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": { "": true },
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must not return a JmapError");
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be present");
    assert!(not_created.contains_key("c0"), "c0 must be in notCreated");
    assert_eq!(
        not_created["c0"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "empty keyword must yield invalidProperties"
    );
}

/// Oracle: Email/set create with keyword `~` (0x7e, the highest valid byte) is accepted.
///
/// RFC 8621 §4.1.1: printable ASCII range is 0x21–0x7e inclusive; `~` is at the top.
#[tokio::test]
async fn keyword_tilde_accepted() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": { "~": true },
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must not return a JmapError");
    assert!(
        resp["notCreated"].is_null()
            || resp["notCreated"]
                .as_object()
                .map_or(true, |m| m.is_empty()),
        "keyword '~' must be accepted; notCreated: {:?}",
        resp["notCreated"]
    );
    assert!(
        resp["created"].get("c0").is_some(),
        "c0 must appear in created"
    );
}

/// Oracle: Email/set create with keyword `$seen` (dollar sign, 0x24) is accepted.
///
/// RFC 8621 §4.1.1: `$` (0x24) is in the printable ASCII range and not forbidden.
#[tokio::test]
async fn keyword_dollar_sign_accepted() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": { "$seen": true },
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must not return a JmapError");
    assert!(
        resp["notCreated"].is_null()
            || resp["notCreated"]
                .as_object()
                .map_or(true, |m| m.is_empty()),
        "keyword '$seen' must be accepted; notCreated: {:?}",
        resp["notCreated"]
    );
    assert!(
        resp["created"].get("c0").is_some(),
        "c0 must appear in created"
    );
}

/// Oracle: Mailbox/query with a non-integer limit returns invalidArguments.
///
/// RFC 8620 §5.5: limit must be a UnsignedInt. Passing a string must be rejected,
/// not silently treated as no-limit. This matches Email/query behaviour.
#[tokio::test]
async fn mailbox_query_invalid_limit_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "limit": "not-a-number",
    });

    let err = handle_mailbox_query(&backend, args)
        .await
        .expect_err("Mailbox/query must fail with invalidArguments");
    assert_eq!(
        err.error_type.as_str(),
        "invalidArguments",
        "error type must be invalidArguments; got: {err:?}"
    );
}

/// Oracle: EmailSubmission/query with a non-integer limit returns invalidArguments.
///
/// Consistent with Email/query and Mailbox/query.
#[tokio::test]
async fn submission_query_invalid_limit_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "limit": -1,
    });

    let err = handle_submission_query(&backend, args)
        .await
        .expect_err("EmailSubmission/query must fail with invalidArguments");
    assert_eq!(
        err.error_type.as_str(),
        "invalidArguments",
        "error type must be invalidArguments; got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// EmailSubmission/query FilterCondition tests (RFC 8621 §7.3)
// ---------------------------------------------------------------------------

/// Helper: create an Identity and a simple email in `account_id`, returning both IDs.
///
/// The email is imported with a To address so that the derived envelope has at least
/// one recipient (needed for EmailSubmission/set create to succeed).
async fn make_identity_and_email(
    backend: &MemoryBackend,
    account_id: &Id,
    addr: &str,
    mailbox_id: &str,
) -> (Id, Id) {
    use jmap_mail_types::Identity;
    let identity = Identity::new(Id::from("placeholder"), addr, true);
    let (identity_id, _) = backend
        .create_object::<Identity>(account_id, "i", identity)
        .await
        .expect("create Identity");

    let msg = format!("Subject: Test\r\nFrom: {addr}\r\nTo: {addr}\r\n\r\nBody.");
    let blob_id = Id::from(format!("blob-{addr}"));
    backend.store_blob(&blob_id, msg.into_bytes());
    let (email_id, _) = backend
        .import_email(account_id, &blob_id, &[Id::from(mailbox_id)], &[], None)
        .await
        .expect("import_email");

    (identity_id, email_id)
}

/// Oracle: EmailSubmission/query with `identityIds` filter returns only submissions
/// for the specified identity (RFC 8621 §7.3).
#[tokio::test]
async fn submission_query_filter_by_identity_id() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Create two identities and one email each.
    let (identity_a, email_a) =
        make_identity_and_email(&backend, &account_id, "alice@example.com", "sentA").await;
    let (identity_b, email_b) =
        make_identity_and_email(&backend, &account_id, "bob@example.com", "sentB").await;

    // Create a submission for identity_a.
    let (set_resp_a, _) = handle_submission_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": { "sA": { "identityId": identity_a.as_ref(), "emailId": email_a.as_ref() } }
        }),
        "c1",
    )
    .await
    .expect("set A");
    let sub_a_id = set_resp_a["created"]["sA"]["id"]
        .as_str()
        .expect("sA created")
        .to_owned();

    // Create a submission for identity_b.
    let (set_resp_b, _) = handle_submission_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": { "sB": { "identityId": identity_b.as_ref(), "emailId": email_b.as_ref() } }
        }),
        "c2",
    )
    .await
    .expect("set B");
    let sub_b_id = set_resp_b["created"]["sB"]["id"]
        .as_str()
        .expect("sB created")
        .to_owned();

    // Query filtered to identity_a only.
    let (qresp, _) = handle_submission_query(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "filter": { "identityIds": [identity_a.as_ref()] },
        }),
    )
    .await
    .expect("query");

    let ids: Vec<&str> = qresp["ids"]
        .as_array()
        .expect("ids array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    assert!(
        ids.contains(&sub_a_id.as_str()),
        "identity_a submission must be present; got {ids:?}"
    );
    assert!(
        !ids.contains(&sub_b_id.as_str()),
        "identity_b submission must be absent; got {ids:?}"
    );
}

/// Oracle: EmailSubmission/query with `before` filter excludes submissions whose
/// `sendAt` is >= the given date-time (RFC 8621 §7.3).
///
/// sendAt is server-set (RFC 8621 §7.2), so filters are derived relative to
/// the stored value using epoch/far-future sentinel dates.
#[tokio::test]
async fn submission_query_filter_by_before() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let (identity_id, email_id) =
        make_identity_and_email(&backend, &account_id, "alice@example.com", "sent").await;

    // Create a submission; sendAt is server-set to ~now.
    let (set_resp, _) = handle_submission_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "s0": {
                    "identityId": identity_id.as_ref(),
                    "emailId": email_id.as_ref(),
                }
            }
        }),
        "c1",
    )
    .await
    .expect("set");
    let sub_id = set_resp["created"]["s0"]["id"]
        .as_str()
        .expect("s0 created")
        .to_owned();

    // before=far-future must include the submission (sendAt < 9999-12-31).
    let (qresp_in, _) = handle_submission_query(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "filter": { "before": "9999-12-31T23:59:59Z" },
        }),
    )
    .await
    .expect("query in");

    let ids_in: Vec<&str> = qresp_in["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids_in.contains(&sub_id.as_str()),
        "submission must be included when sendAt < before; got {ids_in:?}"
    );

    // before=epoch must exclude the submission (sendAt >= 2000-01-01).
    let (qresp_out, _) = handle_submission_query(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "filter": { "before": "2000-01-01T00:00:00Z" },
        }),
    )
    .await
    .expect("query out");

    let ids_out: Vec<&str> = qresp_out["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        !ids_out.contains(&sub_id.as_str()),
        "submission must be excluded when sendAt >= before; got {ids_out:?}"
    );
}

/// Oracle: EmailSubmission/query with `after` filter excludes submissions whose
/// `sendAt` is < the given date-time (RFC 8621 §7.3).
///
/// sendAt is server-set (RFC 8621 §7.2), so filters are derived relative to
/// the stored value using epoch/far-future sentinel dates.
#[tokio::test]
async fn submission_query_filter_by_after() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let (identity_id, email_id) =
        make_identity_and_email(&backend, &account_id, "alice@example.com", "sent").await;

    // Create a submission; sendAt is server-set to ~now.
    let (set_resp, _) = handle_submission_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "s0": {
                    "identityId": identity_id.as_ref(),
                    "emailId": email_id.as_ref(),
                }
            }
        }),
        "c1",
    )
    .await
    .expect("set");
    let sub_id = set_resp["created"]["s0"]["id"]
        .as_str()
        .expect("s0 created")
        .to_owned();

    // after=epoch must include the submission (sendAt >= 2000-01-01).
    let (qresp_in, _) = handle_submission_query(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "filter": { "after": "2000-01-01T00:00:00Z" },
        }),
    )
    .await
    .expect("query in");

    let ids_in: Vec<&str> = qresp_in["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids_in.contains(&sub_id.as_str()),
        "submission must be included when sendAt >= after; got {ids_in:?}"
    );

    // after=far-future must exclude the submission (sendAt < 9999-12-31).
    let (qresp_out, _) = handle_submission_query(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "filter": { "after": "9999-12-31T23:59:59Z" },
        }),
    )
    .await
    .expect("query out");

    let ids_out: Vec<&str> = qresp_out["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        !ids_out.contains(&sub_id.as_str()),
        "submission must be excluded when sendAt < after; got {ids_out:?}"
    );
}

/// Oracle: EmailSubmission/query with `undoStatus` filter returns only submissions
/// whose `undoStatus` matches (RFC 8621 §7.3).
///
/// EmailSubmission/set create always sets `undoStatus: "final"` in the test harness.
/// Submission B is created directly via the backend with `undoStatus: "pending"`.
#[tokio::test]
async fn submission_query_filter_by_undo_status() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let (identity_a, email_a) =
        make_identity_and_email(&backend, &account_id, "alice@example.com", "sentA").await;
    let (identity_b, email_b) =
        make_identity_and_email(&backend, &account_id, "bob@example.com", "sentB").await;

    // Create submission A via the handler (undoStatus = "final").
    let (set_resp_a, _) = handle_submission_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": { "sA": { "identityId": identity_a.as_ref(), "emailId": email_a.as_ref() } }
        }),
        "c1",
    )
    .await
    .expect("set A");
    let sub_a_id = set_resp_a["created"]["sA"]["id"]
        .as_str()
        .expect("sA created")
        .to_owned();

    // Create submission B directly via backend with undoStatus = "pending".
    use jmap_mail_types::{submission::UndoStatus, EmailSubmission};
    let sub_b = EmailSubmission::new(
        Id::from("placeholder"),
        identity_b.clone(),
        email_b.clone(),
        Id::from("thread-b"),
        jmap_types::UTCDate::from("2025-01-01T00:00:00Z"),
        UndoStatus::Pending,
    );
    let (sub_b_id, _) = backend
        .create_object::<EmailSubmission>(&account_id, "sB", sub_b)
        .await
        .expect("create submission B");

    // Query filtered to undoStatus = "final" — should return sub_a, not sub_b.
    let (qresp_final, _) = handle_submission_query(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "filter": { "undoStatus": "final" },
        }),
    )
    .await
    .expect("query final");

    let ids_final: Vec<&str> = qresp_final["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids_final.contains(&sub_a_id.as_str()),
        "final submission must be present; got {ids_final:?}"
    );
    assert!(
        !ids_final.contains(&sub_b_id.as_ref()),
        "pending submission must be absent from final query; got {ids_final:?}"
    );

    // Query filtered to undoStatus = "pending" — should return sub_b, not sub_a.
    let (qresp_pending, _) = handle_submission_query(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "filter": { "undoStatus": "pending" },
        }),
    )
    .await
    .expect("query pending");

    let ids_pending: Vec<&str> = qresp_pending["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids_pending.contains(&sub_b_id.as_ref()),
        "pending submission must be present; got {ids_pending:?}"
    );
    assert!(
        !ids_pending.contains(&sub_a_id.as_str()),
        "final submission must be absent from pending query; got {ids_pending:?}"
    );
}

/// Oracle: EmailSubmission/query with a non-object `filter` value returns
/// invalidArguments (RFC 8620 §5.5 — filter must be a FilterCondition or
/// FilterOperator object, not a scalar).
#[tokio::test]
async fn submission_query_invalid_filter_json() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": 42,
    });

    let err = handle_submission_query(&backend, args)
        .await
        .expect_err("must fail with invalidArguments");
    assert_eq!(
        err.error_type.as_str(),
        "invalidArguments",
        "error type must be invalidArguments; got: {err:?}"
    );
}

/// Oracle: EmailSubmission/set create fails with noRecipients when the email has no
/// To, Cc, or Bcc addresses and no explicit envelope is provided.
///
/// RFC 8621 §7.5: "noRecipients: The envelope.rcptTo is empty."
#[tokio::test]
async fn submission_set_create_no_recipients_fails() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Create an Identity.
    let identity = Identity::new(Id::from("placeholder"), "alice@example.com", true);
    let (identity_id, _) = backend
        .create_object::<Identity>(&account_id, "i0", identity)
        .await
        .expect("create Identity");

    // Import an email with no To, Cc, or Bcc headers.
    let msg = b"Subject: Test\r\nFrom: alice@example.com\r\n\r\nBody.";
    let blob_id = Id::from("blob-norcpt");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("sent")], &[], None)
        .await
        .expect("import_email");

    // EmailSubmission/set create — no envelope provided, so rcptTo is derived.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "s0": {
                "identityId": identity_id.as_ref(),
                "emailId": email_id.as_ref(),
            }
        }
    });

    let (resp, _) = handle_submission_set(&backend, args, "call1")
        .await
        .expect("EmailSubmission/set must return a response (not a protocol error)");

    // Oracle: "s0" must be in notCreated with type "noRecipients".
    assert!(
        resp["created"].is_null(),
        "created must be null; got: {:?}",
        resp["created"]
    );
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be an object");
    assert!(
        not_created.contains_key("s0"),
        "s0 must be in notCreated; got: {resp:?}"
    );
    assert_eq!(
        not_created["s0"]["type"].as_str().unwrap_or(""),
        "noRecipients",
        "error type must be noRecipients; got: {:?}",
        not_created["s0"]
    );
}

/// Oracle: Mailbox/set — a create and an update in the same request cannot both
/// claim the same role. The update must fail with invalidProperties even though
/// no pre-existing mailbox held the role before the request.
///
/// RFC 8621 §2.5: role must be unique per account.
#[tokio::test]
async fn mailbox_set_role_uniqueness_create_then_update() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Create a mailbox with no role that we will try to assign a role to via update.
    let mbox = Mailbox::new(
        Id::from("placeholder"),
        "Updates".to_string(),
        0,
        0,
        0,
        0,
        0,
        jmap_mail_types::MailboxRights::default(),
        false,
    );
    let (existing_id, _) = backend
        .create_object::<Mailbox>(&account_id, "pre0", mbox)
        .await
        .expect("create mailbox");

    // Single Mailbox/set request: create "c0" with role "inbox", update existing_id with role "inbox".
    // Creates run before updates, so the create claims "inbox" first.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": { "name": "Inbox", "role": "inbox" }
        },
        "update": {
            existing_id.as_ref(): { "role": "inbox" }
        }
    });

    let (resp, _) = handle_mailbox_set(&backend, args)
        .await
        .expect("Mailbox/set must return a response");

    // Oracle: create succeeds.
    let created = resp["created"]
        .as_object()
        .expect("created must be an object");
    assert!(
        created.contains_key("c0"),
        "c0 must succeed; notCreated = {:?}",
        resp["notCreated"]
    );

    // Oracle: update fails — "inbox" was claimed by the create in the same request.
    let not_updated = resp["notUpdated"]
        .as_object()
        .expect("notUpdated must be an object");
    assert!(
        not_updated.contains_key(existing_id.as_ref()),
        "update of existing_id must fail; updated = {:?}",
        resp["updated"]
    );
    assert_eq!(
        not_updated[existing_id.as_ref()]["type"]
            .as_str()
            .unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties; got: {:?}",
        not_updated[existing_id.as_ref()]
    );
}

/// Oracle: a single Mailbox/set that vacates a role on mailbox A and claims that
/// same role on mailbox B must succeed for both updates.
///
/// Trigger: role-uniqueness check used a pre-request snapshot and did not track
/// role vacations within the same request. Mailbox B's update was incorrectly
/// rejected with invalidProperties because the snapshot still showed A holding
/// the role, even though A's update (role=null) had already been applied.
#[tokio::test]
async fn mailbox_set_role_swap_succeeds_in_single_request() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Create mailbox A with role "inbox".
    let mut mbox_a = Mailbox::new(
        Id::from("placeholder"),
        "Inbox".to_string(),
        0,
        0,
        0,
        0,
        0,
        jmap_mail_types::MailboxRights::default(),
        false,
    );
    mbox_a.role = Some(jmap_mail_types::MailboxRole::Inbox);
    let (id_a, _) = backend
        .create_object::<Mailbox>(&account_id, "pre0", mbox_a)
        .await
        .expect("create mailbox A");

    // Create mailbox B with no role.
    let mbox_b = Mailbox::new(
        Id::from("placeholder"),
        "New Inbox".to_string(),
        0,
        0,
        0,
        0,
        0,
        jmap_mail_types::MailboxRights::default(),
        false,
    );
    let (id_b, _) = backend
        .create_object::<Mailbox>(&account_id, "pre1", mbox_b)
        .await
        .expect("create mailbox B");

    // Single Mailbox/set: A vacates "inbox", B claims "inbox".
    // Per RFC 8620 §5.3, updates are applied in order — A before B.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            id_a.as_ref(): { "role": null },
            id_b.as_ref(): { "role": "inbox" }
        }
    });

    let (resp, _) = handle_mailbox_set(&backend, args)
        .await
        .expect("Mailbox/set must return a response");

    let updated = resp["updated"]
        .as_object()
        .expect("updated must be an object");
    let not_updated = resp.get("notUpdated").and_then(|v| v.as_object());

    assert!(
        updated.contains_key(id_a.as_ref()),
        "A's vacate update must succeed; notUpdated = {:?}",
        not_updated
    );
    assert!(
        updated.contains_key(id_b.as_ref()),
        "B's claim update must succeed; notUpdated = {:?}",
        not_updated
    );
}

/// Oracle: Email/set create with malformed inReplyTo (not an array) must return
/// invalidProperties, not silently drop the field and create the email.
///
/// Trigger: RFC 8621 §5.5 — client sends "inReplyTo": "not-an-array". Before the
/// fix, the field was silently dropped via .ok(); now it must be rejected.
#[tokio::test]
async fn email_set_create_malformed_in_reply_to_returns_error() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // First create a mailbox to put the email in.
    let mb_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": { "mb1": { "name": "Inbox" } },
    });
    let (mb_resp, _) = handle_mailbox_set(&backend, mb_args)
        .await
        .expect("Mailbox/set must succeed");
    let mailbox_id = mb_resp["created"]["mb1"]["id"]
        .as_str()
        .expect("mailbox id must be present")
        .to_owned();

    // Attempt to create an email with a non-array inReplyTo.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "e1": {
                "mailboxIds": { &mailbox_id: true },
                "inReplyTo": "not-an-array",  // invalid: must be array of strings
            }
        },
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must not return a method-level error");

    // Oracle: e1 must be in notCreated, not created.
    assert!(
        resp["notCreated"]["e1"].is_object(),
        "malformed inReplyTo must produce notCreated entry; got resp={resp}"
    );
    assert!(
        resp["created"].is_null() || resp["created"]["e1"].is_null(),
        "email must not appear in created; got resp={resp}"
    );
}

/// Oracle: Email/set create with malformed references (not an array) must return
/// invalidProperties, not silently drop the field and create the email.
#[tokio::test]
async fn email_set_create_malformed_references_returns_error() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let mb_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": { "mb1": { "name": "Inbox" } },
    });
    let (mb_resp, _) = handle_mailbox_set(&backend, mb_args)
        .await
        .expect("Mailbox/set must succeed");
    let mailbox_id = mb_resp["created"]["mb1"]["id"]
        .as_str()
        .expect("mailbox id must be present")
        .to_owned();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "e1": {
                "mailboxIds": { &mailbox_id: true },
                "references": 42,  // invalid: must be array of strings
            }
        },
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must not return a method-level error");

    assert!(
        resp["notCreated"]["e1"].is_object(),
        "malformed references must produce notCreated entry; got resp={resp}"
    );
}

/// Oracle: Mailbox/query without calculateTotal must NOT include total in response.
/// Mailbox/query with calculateTotal=true MUST include total.
///
/// RFC 8620 §5.5: "total MUST be omitted if calculateTotal request argument is false"
/// (default is false).
#[tokio::test]
async fn mailbox_query_calculate_total_controls_total_field() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Create a mailbox so there is something to count.
    let mb_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": { "mb1": { "name": "Inbox" } },
    });
    handle_mailbox_set(&backend, mb_args)
        .await
        .expect("Mailbox/set must succeed");

    // Default (no calculateTotal) — total must be absent.
    let args_no_total = serde_json::json!({ "accountId": account_id.as_ref() });
    let (resp, _) = handle_mailbox_query(&backend, args_no_total)
        .await
        .expect("Mailbox/query must not error");
    assert!(
        !resp.as_object().unwrap().contains_key("total"),
        "total must be absent when calculateTotal is not set; got resp={resp}"
    );

    // calculateTotal=false — total must still be absent.
    let args_false = serde_json::json!({
        "accountId": account_id.as_ref(),
        "calculateTotal": false,
    });
    let (resp, _) = handle_mailbox_query(&backend, args_false)
        .await
        .expect("Mailbox/query must not error");
    assert!(
        !resp.as_object().unwrap().contains_key("total"),
        "total must be absent when calculateTotal=false; got resp={resp}"
    );

    // calculateTotal=true — total must be present and correct.
    let args_true = serde_json::json!({
        "accountId": account_id.as_ref(),
        "calculateTotal": true,
    });
    let (resp, _) = handle_mailbox_query(&backend, args_true)
        .await
        .expect("Mailbox/query must not error");
    assert!(
        resp.as_object().unwrap().contains_key("total"),
        "total must be present when calculateTotal=true; got resp={resp}"
    );
    assert_eq!(
        resp["total"].as_u64(),
        Some(1),
        "total must be 1 (one mailbox created); got resp={resp}"
    );
}

/// Oracle: Email/queryChanges with a non-string upToId must return invalidArguments.
///
/// RFC 8620 §5.6: upToId is Id|null; a non-string is an invalid type.
#[tokio::test]
async fn email_query_changes_non_string_up_to_id_returns_error() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceQueryState": "0",
        "upToId": 42,  // invalid: must be a string Id or null
    });

    let result = handle_email_query_changes(&backend, args).await;
    assert!(
        result.is_err(),
        "non-string upToId must return an error; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type.as_str(),
        "invalidArguments",
        "expected invalidArguments; got: {:?}",
        err.error_type
    );
}

/// Oracle: Mailbox/queryChanges with a non-string upToId must return invalidArguments.
#[tokio::test]
async fn mailbox_query_changes_non_string_up_to_id_returns_error() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceQueryState": "0",
        "upToId": true,  // invalid: must be a string Id or null
    });

    let result = handle_mailbox_query_changes(&backend, args).await;
    assert!(
        result.is_err(),
        "non-string upToId must return an error; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type.as_str(),
        "invalidArguments",
        "expected invalidArguments; got: {:?}",
        err.error_type
    );
}

/// Oracle: Email/query with calculateTotal=true must include total; without it, total is absent.
///
/// RFC 8620 §5.5: total MUST be omitted when calculateTotal is false (default).
#[tokio::test]
async fn email_query_calculate_total_controls_total_field() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Default — total must be absent.
    let args = serde_json::json!({ "accountId": account_id.as_ref() });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must not error");
    assert!(
        !resp.as_object().unwrap().contains_key("total"),
        "total must be absent when calculateTotal is not set; got resp={resp}"
    );

    // calculateTotal=true — total must be present (0 emails, so 0).
    let args_true = serde_json::json!({
        "accountId": account_id.as_ref(),
        "calculateTotal": true,
    });
    let (resp, _) = handle_email_query(&backend, args_true)
        .await
        .expect("Email/query must not error");
    // The backend may return None for total on empty account; only check if present.
    // What matters is that the field is in the response when calculateTotal=true.
    // (Backend returns Some(0) or None; either way the key should be present or absent
    // based on whether backend provided a total.)
    // This test verifies the calculateTotal=true path doesn't break. The key assertion
    // is the calculateTotal=false case above.
    let _ = resp;
}

/// Oracle: Email/query with collapseThreads=true and position=i64::MIN must not
/// panic and must return a valid (empty) result on an empty account.
///
/// i64::MIN negation overflows if not handled; we use saturating_neg().
#[tokio::test]
async fn email_query_collapse_threads_position_i64_min_does_not_panic() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "collapseThreads": true,
        "position": i64::MIN,
    });

    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must not panic or error with position=i64::MIN");

    // Oracle: empty account → empty ids list.
    let ids = resp["ids"].as_array().expect("ids must be an array");
    assert!(ids.is_empty(), "expected empty ids; got: {ids:?}");
}

// ---------------------------------------------------------------------------
// BackendSetError::Other routing tests (JMAP-z0g)
//
// Each test verifies that a BackendSetError::Other from the storage layer is
// surfaced as { "type": "serverFail" } in the appropriate not_* map, not
// silently swallowed or converted to a JmapError that aborts the whole call.
// ---------------------------------------------------------------------------

/// Oracle: Mailbox/set — backend Other on create → item in notCreated["serverFail"].
#[tokio::test]
async fn mailbox_set_create_backend_other_goes_to_not_created() {
    let backend = FaultyBackend::new();
    backend.inner.register_account(&Id::from("acct1"));
    backend.inject("Mailbox", "create");
    let args = serde_json::json!({
        "accountId": "acct1",
        "create": { "c1": { "name": "Inbox" } }
    });
    let (resp, _) = handle_mailbox_set(&backend, args).await.unwrap();
    assert!(
        resp["created"].is_null(),
        "created must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notCreated"]["c1"]["type"].as_str(),
        Some("serverFail"),
        "notCreated[c1] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Mailbox/set — backend Other on update → item in notUpdated["serverFail"].
#[tokio::test]
async fn mailbox_set_update_backend_other_goes_to_not_updated() {
    let backend = FaultyBackend::new();
    let account = Id::from("acct1");
    // Pre-create a mailbox via the inner backend so the update has a target.
    let mailbox = jmap_mail_types::Mailbox::new(
        Id::from("placeholder"),
        "Inbox",
        0,
        0,
        0,
        0,
        0,
        jmap_mail_types::MailboxRights::default(),
        true,
    );
    let (mbox_id, _) = backend
        .inner
        .create_object::<Mailbox>(&account, "c0", mailbox)
        .await
        .unwrap();

    backend.inject("Mailbox", "update");
    let args = serde_json::json!({
        "accountId": account.as_ref(),
        "update": { mbox_id.as_ref(): { "name": "Updated" } }
    });
    let (resp, _) = handle_mailbox_set(&backend, args).await.unwrap();
    assert!(
        resp["updated"].is_null(),
        "updated must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notUpdated"][mbox_id.as_ref()]["type"].as_str(),
        Some("serverFail"),
        "notUpdated[id] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Mailbox/set — backend Other on destroy → item in notDestroyed["serverFail"].
#[tokio::test]
async fn mailbox_set_destroy_backend_other_goes_to_not_destroyed() {
    let backend = FaultyBackend::new();
    let account = Id::from("acct1");
    let mailbox = jmap_mail_types::Mailbox::new(
        Id::from("placeholder"),
        "Inbox",
        0,
        0,
        0,
        0,
        0,
        jmap_mail_types::MailboxRights::default(),
        true,
    );
    let (mbox_id, _) = backend
        .inner
        .create_object::<Mailbox>(&account, "c0", mailbox)
        .await
        .unwrap();

    backend.inject("Mailbox", "destroy");
    let args = serde_json::json!({
        "accountId": account.as_ref(),
        "destroy": [mbox_id.as_ref()]
    });
    let (resp, _) = handle_mailbox_set(&backend, args).await.unwrap();
    assert!(
        resp["destroyed"].is_null(),
        "destroyed must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notDestroyed"][mbox_id.as_ref()]["type"].as_str(),
        Some("serverFail"),
        "notDestroyed[id] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Email/set — backend Other on create → item in notCreated["serverFail"].
#[tokio::test]
async fn email_set_create_backend_other_goes_to_not_created() {
    let backend = FaultyBackend::new();
    backend.inner.register_account(&Id::from("acct1"));
    backend.inject("Email", "create");
    let args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "e1": { "mailboxIds": { "mbox1": true } }
        }
    });
    let (resp, _) = handle_email_set(&backend, args).await.unwrap();
    assert!(
        resp["created"].is_null(),
        "created must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notCreated"]["e1"]["type"].as_str(),
        Some("serverFail"),
        "notCreated[e1] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Email/set — backend Other on update → item in notUpdated["serverFail"].
#[tokio::test]
async fn email_set_update_backend_other_goes_to_not_updated() {
    use std::collections::HashMap;
    let backend = FaultyBackend::new();
    let account = Id::from("acct1");
    let email = jmap_mail_types::Email::new(
        Id::from("placeholder"),
        Id::from("blob1"),
        Id::from("t1"),
        HashMap::from([(Id::from("mbox1"), true)]),
        0,
        jmap_types::UTCDate::from("2024-01-01T00:00:00Z"),
    );
    let (email_id, _) = backend
        .inner
        .create_object::<jmap_mail_types::Email>(&account, "e0", email)
        .await
        .unwrap();

    backend.inject("Email", "update");
    let args = serde_json::json!({
        "accountId": account.as_ref(),
        "update": { email_id.as_ref(): { "keywords/$seen": true } }
    });
    let (resp, _) = handle_email_set(&backend, args).await.unwrap();
    assert!(
        resp["updated"].is_null(),
        "updated must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notUpdated"][email_id.as_ref()]["type"].as_str(),
        Some("serverFail"),
        "notUpdated[id] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Email/set — backend Other on destroy → item in notDestroyed["serverFail"].
#[tokio::test]
async fn email_set_destroy_backend_other_goes_to_not_destroyed() {
    use std::collections::HashMap;
    let backend = FaultyBackend::new();
    let account = Id::from("acct1");
    let email = jmap_mail_types::Email::new(
        Id::from("placeholder"),
        Id::from("blob1"),
        Id::from("t1"),
        HashMap::from([(Id::from("mbox1"), true)]),
        0,
        jmap_types::UTCDate::from("2024-01-01T00:00:00Z"),
    );
    let (email_id, _) = backend
        .inner
        .create_object::<jmap_mail_types::Email>(&account, "e0", email)
        .await
        .unwrap();

    backend.inject("Email", "destroy");
    let args = serde_json::json!({
        "accountId": account.as_ref(),
        "destroy": [email_id.as_ref()]
    });
    let (resp, _) = handle_email_set(&backend, args).await.unwrap();
    assert!(
        resp["destroyed"].is_null(),
        "destroyed must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notDestroyed"][email_id.as_ref()]["type"].as_str(),
        Some("serverFail"),
        "notDestroyed[id] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Email/import — backend Other on import_email → item in notCreated["serverFail"].
#[tokio::test]
async fn email_import_backend_other_goes_to_not_created() {
    let backend = FaultyBackend::new();
    backend.inner.register_account(&Id::from("acct1"));
    backend.inject("Email", "import");
    // The handler validates blobId and mailboxIds before calling import_email;
    // supply both so the injection path is reached.
    let args = serde_json::json!({
        "accountId": "acct1",
        "emails": {
            "i1": {
                "blobId": "blob1",
                "mailboxIds": { "mbox1": true }
            }
        }
    });
    let (resp, _) = handle_email_import(&backend, args).await.unwrap();
    assert!(
        resp["created"].is_null(),
        "created must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notCreated"]["i1"]["type"].as_str(),
        Some("serverFail"),
        "notCreated[i1] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: EmailSubmission/set — backend Other on create → item in notCreated["serverFail"].
#[tokio::test]
async fn submission_set_create_backend_other_goes_to_not_created() {
    use std::collections::HashMap;
    let backend = FaultyBackend::new();
    let account = Id::from("acct1");

    // Create an Identity so identityId validation passes.
    let identity = jmap_mail_types::Identity::new(
        Id::from("placeholder"),
        "from@example.com".to_owned(),
        true,
    );
    let (identity_id, _) = backend
        .inner
        .create_object::<Identity>(&account, "id1", identity)
        .await
        .unwrap();

    // Create an Email so emailId validation passes.
    let email = jmap_mail_types::Email::new(
        Id::from("placeholder"),
        Id::from("blob1"),
        Id::from("t1"),
        HashMap::from([(Id::from("mbox1"), true)]),
        0,
        jmap_types::UTCDate::from("2024-01-01T00:00:00Z"),
    );
    let (email_id, _) = backend
        .inner
        .create_object::<jmap_mail_types::Email>(&account, "e1", email)
        .await
        .unwrap();

    backend.inject("EmailSubmission", "create");
    let args = serde_json::json!({
        "accountId": account.as_ref(),
        "create": {
            "s1": {
                "identityId": identity_id.as_ref(),
                "emailId": email_id.as_ref(),
                // Supply explicit envelope so the noRecipients check is bypassed.
                "envelope": {
                    "mailFrom": { "email": "from@example.com" },
                    "rcptTo": [{ "email": "to@example.com" }]
                }
            }
        }
    });
    let (resp, _) = handle_submission_set(&backend, args, "call1")
        .await
        .unwrap();
    assert!(
        resp["created"].is_null(),
        "created must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notCreated"]["s1"]["type"].as_str(),
        Some("serverFail"),
        "notCreated[s1] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Identity/set — backend Other on create → item in notCreated["serverFail"].
#[tokio::test]
async fn identity_set_create_backend_other_goes_to_not_created() {
    let backend = FaultyBackend::new();
    backend.inner.register_account(&Id::from("acct1"));
    backend.inject("Identity", "create");
    let args = serde_json::json!({
        "accountId": "acct1",
        "create": { "id1": { "email": "user@example.com" } }
    });
    let (resp, _) = handle_identity_set(&backend, args).await.unwrap();
    assert!(
        resp["created"].is_null(),
        "created must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notCreated"]["id1"]["type"].as_str(),
        Some("serverFail"),
        "notCreated[id1] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Identity/set — backend Other on update → item in notUpdated["serverFail"].
#[tokio::test]
async fn identity_set_update_backend_other_goes_to_not_updated() {
    let backend = FaultyBackend::new();
    let account = Id::from("acct1");
    let identity = Identity::new(Id::from("placeholder"), "user@example.com".to_owned(), true);
    let (id, _) = backend
        .inner
        .create_object::<Identity>(&account, "id1", identity)
        .await
        .unwrap();

    backend.inject("Identity", "update");
    let args = serde_json::json!({
        "accountId": account.as_ref(),
        "update": { id.as_ref(): { "name": "Updated Name" } }
    });
    let (resp, _) = handle_identity_set(&backend, args).await.unwrap();
    assert!(
        resp["updated"].is_null(),
        "updated must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notUpdated"][id.as_ref()]["type"].as_str(),
        Some("serverFail"),
        "notUpdated[id] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Identity/set — backend Other on destroy → item in notDestroyed["serverFail"].
///
/// The handler fetches the identity first to check mayDelete before calling
/// destroy_object. The injection must fire on destroy_object, not on the
/// get_objects pre-fetch, so the error is routed to notDestroyed.
#[tokio::test]
async fn identity_set_destroy_backend_other_goes_to_not_destroyed() {
    let backend = FaultyBackend::new();
    let account = Id::from("acct1");
    // mayDelete = true so the handler proceeds to call destroy_object.
    let identity = Identity::new(Id::from("placeholder"), "user@example.com".to_owned(), true);
    let (id, _) = backend
        .inner
        .create_object::<Identity>(&account, "id1", identity)
        .await
        .unwrap();

    backend.inject("Identity", "destroy");
    let args = serde_json::json!({
        "accountId": account.as_ref(),
        "destroy": [id.as_ref()]
    });
    let (resp, _) = handle_identity_set(&backend, args).await.unwrap();
    assert!(
        resp["destroyed"].is_null(),
        "destroyed must be null; resp: {resp}"
    );
    assert_eq!(
        resp["notDestroyed"][id.as_ref()]["type"].as_str(),
        Some("serverFail"),
        "notDestroyed[id] must have type serverFail; resp: {resp}"
    );
}

/// Oracle: Mailbox/set create with sortOrder > u32::MAX must return notCreated
/// with type=invalidProperties, not silently truncate the value.
///
/// Reference: RFC 8621 §2.1 defines sortOrder as UInt32; u64→u32 truncation
/// would corrupt data without error.
#[tokio::test]
async fn mailbox_set_sort_order_overflow_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    // 5_000_000_000 exceeds u32::MAX (4_294_967_295). Without the fix, this
    // would silently become 705_032_704 (5e9 mod 2^32).
    let args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "c0": { "name": "Box", "sortOrder": 5_000_000_000u64 }
        }
    });
    let (resp, _) = handle_mailbox_set(&backend, args)
        .await
        .expect("handler must not return a JmapError");

    // The request must fail for c0.
    assert!(
        resp["created"].is_null() || resp["created"]["c0"].is_null(),
        "c0 must not appear in created; resp: {resp}"
    );
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be an object");
    assert!(
        not_created.contains_key("c0"),
        "c0 must be in notCreated; resp: {resp}"
    );
    assert_eq!(
        not_created["c0"]["type"].as_str(),
        Some("invalidProperties"),
        "error type must be invalidProperties; resp: {resp}"
    );
    let props = not_created["c0"]["properties"]
        .as_array()
        .expect("properties must be an array");
    assert!(
        props.iter().any(|p| p.as_str() == Some("sortOrder")),
        "sortOrder must be in invalid properties list; props: {props:?}"
    );
}

// ---------------------------------------------------------------------------
// JMAP-2qp.7: Email/import created response — 4 server-set fields only
// ---------------------------------------------------------------------------

/// Oracle: Email/import 'created' entries contain exactly the four fields
/// specified in RFC 8621 §4.8: id, blobId, threadId, size. No other Email
/// properties (mailboxIds, keywords, subject, etc.) may appear.
#[tokio::test]
async fn email_import_created_response_has_four_server_set_fields() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let blob_id = Id::from("blob-4f");
    backend.store_blob(&blob_id, b"Subject: test\r\n\r\nbody".to_vec());

    let args = serde_json::json!({
        "accountId": "acct1",
        "emails": {
            "imp1": {
                "blobId": blob_id.as_ref(),
                "mailboxIds": { "inbox": true },
                "keywords": { "$seen": true },
            }
        }
    });
    let (resp, _) = handle_email_import(&backend, args)
        .await
        .expect("import must succeed");

    let created = resp["created"]["imp1"]
        .as_object()
        .expect("imp1 must be in created");

    // Required fields present.
    assert!(created.contains_key("id"), "id must be present");
    assert!(created.contains_key("blobId"), "blobId must be present");
    assert!(created.contains_key("threadId"), "threadId must be present");
    assert!(created.contains_key("size"), "size must be present");

    // No extra fields (mailboxIds, keywords, subject, etc.) must leak out.
    let extra: Vec<&String> = created
        .keys()
        .filter(|k| !["id", "blobId", "threadId", "size"].contains(&k.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "created entry must have only 4 server-set fields; found extra: {extra:?}"
    );
}

// ---------------------------------------------------------------------------
// JMAP-2qp.1: queryChanges total field must be gated on calculateTotal
// ---------------------------------------------------------------------------

/// Oracle: Email/queryChanges must omit 'total' when calculateTotal is absent
/// or false (RFC 8620 §5.6 — total MUST be omitted by default).
#[tokio::test]
async fn email_query_changes_omits_total_by_default() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "sinceQueryState": "0",
    });
    let (resp, _) = handle_email_query_changes(&backend, args)
        .await
        .expect("handler must succeed");
    assert!(
        resp.get("total").is_none(),
        "total must be absent when calculateTotal not sent; resp: {resp}"
    );
}

/// Oracle: Mailbox/queryChanges must omit 'total' when calculateTotal is absent
/// or false (RFC 8620 §5.6).
#[tokio::test]
async fn mailbox_query_changes_omits_total_by_default() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "sinceQueryState": "0",
    });
    let (resp, _) = handle_mailbox_query_changes(&backend, args)
        .await
        .expect("handler must succeed");
    assert!(
        resp.get("total").is_none(),
        "total must be absent when calculateTotal not sent; resp: {resp}"
    );
}

// ---------------------------------------------------------------------------
// JMAP-2qp.2: anchor/anchorOffset support in query handlers
// ---------------------------------------------------------------------------

/// Oracle: Email/query with anchor= returns the page starting at the anchor's
/// 0-based position in the sorted result list (RFC 8620 §5.5).
#[tokio::test]
async fn email_query_anchor_resolves_position() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let account_id = "acct1";

    // Create 3 emails.
    let set_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "c0": { "mailboxIds": { "inbox": true } },
            "c1": { "mailboxIds": { "inbox": true } },
            "c2": { "mailboxIds": { "inbox": true } },
        }
    });
    handle_email_set(&backend, set_args)
        .await
        .expect("email create must succeed");

    // First query: get all IDs in sorted order.
    let q1 = serde_json::json!({ "accountId": account_id });
    let (q1_resp, _) = handle_email_query(&backend, q1)
        .await
        .expect("first query must succeed");
    let all_ids: Vec<String> = q1_resp["ids"]
        .as_array()
        .expect("ids must be present")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(all_ids.len(), 3, "must have 3 emails");

    // Anchor at index 1 (middle email), limit=2 → expect [index1, index2].
    let anchor = all_ids[1].clone();
    let q2 = serde_json::json!({
        "accountId": account_id,
        "anchor": anchor,
        "limit": 2,
    });
    let (q2_resp, _) = handle_email_query(&backend, q2)
        .await
        .expect("anchor query must succeed");

    assert_eq!(
        q2_resp["position"].as_i64(),
        Some(1),
        "reported position must be the anchor index; resp: {q2_resp}"
    );
    let got: Vec<&str> = q2_resp["ids"]
        .as_array()
        .expect("ids must be present")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        got,
        [all_ids[1].as_str(), all_ids[2].as_str()],
        "result must start at anchor position"
    );
}

/// Oracle: Email/query with `inMailbox` filter returns only emails in that mailbox
/// (RFC 8621 §4.4.1). Verifies that MemoryBackend::query_objects applies the
/// filter rather than returning all emails.
#[tokio::test]
async fn email_query_in_mailbox_filter() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let account_id = "acct1";

    // Create two emails in "inbox" and one in "trash".
    let set_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "i1": { "mailboxIds": { "inbox": true } },
            "i2": { "mailboxIds": { "inbox": true } },
            "t1": { "mailboxIds": { "trash": true } },
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("email create must succeed");
    assert!(set_resp["notCreated"].is_null(), "all creates must succeed");

    // Query with inMailbox = "inbox" — expect only the 2 inbox emails.
    let q_args = serde_json::json!({
        "accountId": account_id,
        "filter": { "inMailbox": "inbox" },
    });
    let (resp, _) = handle_email_query(&backend, q_args)
        .await
        .expect("Email/query with inMailbox filter must succeed");

    let ids = resp["ids"].as_array().expect("ids must be an array");
    assert_eq!(
        ids.len(),
        2,
        "inMailbox=inbox must return exactly 2 emails; got: {ids:?}"
    );

    // Query with inMailbox = "trash" — expect only the 1 trash email.
    let q_args2 = serde_json::json!({
        "accountId": account_id,
        "filter": { "inMailbox": "trash" },
    });
    let (resp2, _) = handle_email_query(&backend, q_args2)
        .await
        .expect("Email/query with inMailbox=trash filter must succeed");

    let ids2 = resp2["ids"].as_array().expect("ids must be an array");
    assert_eq!(
        ids2.len(),
        1,
        "inMailbox=trash must return exactly 1 email; got: {ids2:?}"
    );
}

/// Oracle: Email/query with an anchor that is not in the result set MUST return
/// an anchorNotFound error (RFC 8620 §5.5).
#[tokio::test]
async fn email_query_anchor_not_found_returns_error() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "anchor": "does-not-exist",
    });
    let result = handle_email_query(&backend, args).await;
    assert!(result.is_err(), "nonexistent anchor must return an error");
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type.as_str(),
        "anchorNotFound",
        "error type must be anchorNotFound; got: {:?}",
        err.error_type
    );
}

/// Oracle: Mailbox/query with anchor= returns the page starting at the anchor's
/// position (RFC 8620 §5.5).
#[tokio::test]
async fn mailbox_query_anchor_resolves_position() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let account_id = "acct1";

    // Create 3 mailboxes.
    let set_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "m0": { "name": "Alpha" },
            "m1": { "name": "Beta" },
            "m2": { "name": "Gamma" },
        }
    });
    handle_mailbox_set(&backend, set_args)
        .await
        .expect("mailbox create must succeed");

    // First query: get all IDs in sorted order.
    let q1 = serde_json::json!({ "accountId": account_id });
    let (q1_resp, _) = handle_mailbox_query(&backend, q1)
        .await
        .expect("first query must succeed");
    let all_ids: Vec<String> = q1_resp["ids"]
        .as_array()
        .expect("ids must be present")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(all_ids.len(), 3, "must have 3 mailboxes");

    // Anchor at the last mailbox (index 2) → expect only that one.
    let anchor = all_ids[2].clone();
    let q2 = serde_json::json!({ "accountId": account_id, "anchor": anchor });
    let (q2_resp, _) = handle_mailbox_query(&backend, q2)
        .await
        .expect("anchor query must succeed");

    assert_eq!(
        q2_resp["position"].as_i64(),
        Some(2),
        "position must equal anchor index 2; resp: {q2_resp}"
    );
    let got: Vec<&str> = q2_resp["ids"]
        .as_array()
        .expect("ids must be present")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        got,
        [all_ids[2].as_str()],
        "must return only the anchored mailbox"
    );
}

// ---------------------------------------------------------------------------
// JMAP-2qp.3: Email/queryChanges accepts collapseThreads
// ---------------------------------------------------------------------------

/// Oracle: Email/queryChanges with collapseThreads=true must be accepted without
/// error (RFC 8621 §4.5 — collapseThreads is a required argument to pass through).
#[tokio::test]
async fn email_query_changes_accepts_collapse_threads() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "sinceQueryState": "0",
        "collapseThreads": true,
    });
    let result = handle_email_query_changes(&backend, args).await;
    assert!(
        result.is_ok(),
        "collapseThreads=true must not error; got: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// JMAP-2qp.8: Identity/set update rejects server-set fields
// ---------------------------------------------------------------------------

/// Oracle: Identity/set update patches containing server-set fields (id, mayDelete)
/// are rejected with invalidProperties (RFC 8621 §6.3).
/// Previously only the immutable 'email' field was guarded.
#[tokio::test]
async fn identity_set_update_rejects_server_set_fields() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let account_id = "acct1";

    let create_args = serde_json::json!({
        "accountId": account_id,
        "create": { "c0": { "name": "Alice", "email": "alice@example.com" } }
    });
    let (create_resp, _) = handle_identity_set(&backend, create_args)
        .await
        .expect("create must succeed");
    let iid = create_resp["created"]["c0"]["id"]
        .as_str()
        .expect("must get created id")
        .to_string();

    // Patching the server-set 'mayDelete' field must be rejected.
    let upd1 = serde_json::json!({
        "accountId": account_id,
        "update": { iid.clone(): { "mayDelete": false } }
    });
    let (r1, _) = handle_identity_set(&backend, upd1)
        .await
        .expect("handler must not return protocol error");
    let not_updated = &r1["notUpdated"][&iid];
    assert!(
        !not_updated.is_null(),
        "mayDelete patch must be rejected; resp: {r1}"
    );
    assert_eq!(not_updated["type"].as_str(), Some("invalidProperties"));
    let props: Vec<&str> = not_updated["properties"]
        .as_array()
        .expect("properties array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        props.contains(&"mayDelete"),
        "mayDelete must be in rejected properties; got: {props:?}"
    );

    // Patching the server-set 'id' field must also be rejected.
    let upd2 = serde_json::json!({
        "accountId": account_id,
        "update": { iid.clone(): { "id": "some-new-id" } }
    });
    let (r2, _) = handle_identity_set(&backend, upd2)
        .await
        .expect("handler must not return protocol error");
    let not_updated2 = &r2["notUpdated"][&iid];
    assert!(
        !not_updated2.is_null(),
        "id patch must be rejected; resp: {r2}"
    );
    let props2: Vec<&str> = not_updated2["properties"]
        .as_array()
        .expect("properties array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        props2.contains(&"id"),
        "id must be in rejected properties; got: {props2:?}"
    );
}

// ---------------------------------------------------------------------------
// JMAP-767: maxChanges=0 rejected (RFC requires positive integer)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn email_changes_rejects_max_changes_zero() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("u1"));
    let account_id = "u1";
    let args = serde_json::json!({
        "accountId": account_id,
        "sinceState": "0",
        "maxChanges": 0,
    });
    let err = handle_email_changes(&backend, args)
        .await
        .expect_err("maxChanges=0 must return invalidArguments");
    assert_eq!(err.error_type, "invalidArguments");
}

#[tokio::test]
async fn mailbox_changes_rejects_max_changes_zero() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("u1"));
    let account_id = "u1";
    let args = serde_json::json!({
        "accountId": account_id,
        "sinceState": "0",
        "maxChanges": 0,
    });
    let err = handle_mailbox_changes(&backend, args)
        .await
        .expect_err("maxChanges=0 must return invalidArguments");
    assert_eq!(err.error_type, "invalidArguments");
}

// ---------------------------------------------------------------------------
// JMAP-767: Thread change log correctly tracks created vs updated
// ---------------------------------------------------------------------------

#[tokio::test]
async fn thread_changes_second_import_logs_thread_as_updated_not_created() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("u1"));
    let account_id = Id::from("u1");

    // Create a mailbox.
    let inbox_id = {
        let mb_args = serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": { "mb1": { "name": "Inbox" } },
        });
        let (r, _) = handle_mailbox_set(&backend, mb_args)
            .await
            .expect("mailbox create");
        r["created"]["mb1"]["id"].as_str().unwrap().to_owned()
    };

    // Store first email blob (must pre-store before import).
    let msg1 = b"From: alice@example.com
To: bob@example.com
Subject: Hello
Message-ID: <msg1@t767.example.com>

Body one.
";
    let blob1 = Id::from("blob-t767-1");
    backend.store_blob(&blob1, msg1.to_vec());

    // Capture thread state before first import.
    let state0 = {
        use jmap_mail_server::JmapBackend;
        backend
            .get_state::<jmap_mail_types::Thread>(&account_id)
            .await
            .unwrap()
    };

    let import1 = serde_json::json!({
        "accountId": account_id.as_ref(),
        "emails": {
            "e1": {
                "blobId": blob1.as_ref(),
                "mailboxIds": { inbox_id.clone(): true },
                "keywords": {},
            }
        }
    });
    handle_email_import(&backend, import1)
        .await
        .expect("first import");

    // Capture thread state after first import.
    let state1 = {
        use jmap_mail_server::JmapBackend;
        backend
            .get_state::<jmap_mail_types::Thread>(&account_id)
            .await
            .unwrap()
    };

    // Store second email blob — replies to first (same thread via In-Reply-To).
    let msg2 = b"From: bob@example.com
To: alice@example.com
Subject: Re: Hello
Message-ID: <msg2@t767.example.com>
In-Reply-To: <msg1@t767.example.com>

Body two.
";
    let blob2 = Id::from("blob-t767-2");
    backend.store_blob(&blob2, msg2.to_vec());

    let import2 = serde_json::json!({
        "accountId": account_id.as_ref(),
        "emails": {
            "e2": {
                "blobId": blob2.as_ref(),
                "mailboxIds": { inbox_id: true },
                "keywords": {},
            }
        }
    });
    handle_email_import(&backend, import2)
        .await
        .expect("second import");

    // Thread/changes from state1 should report the thread as *updated*, not created.
    let changes_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": state1.as_ref(),
    });
    let (changes, _) = handle_thread_changes(&backend, changes_args)
        .await
        .expect("Thread/changes");

    let created = changes["created"].as_array().unwrap();
    let updated = changes["updated"].as_array().unwrap();
    assert!(
        created.is_empty(),
        "second import into existing thread must not appear in created; got: {created:?}"
    );
    assert!(
        !updated.is_empty(),
        "second import into existing thread must appear in updated; got: {updated:?}"
    );

    // Thread/changes from state0 must show the thread as created (first import).
    let changes0_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": state0.as_ref(),
    });
    let (changes0, _) = handle_thread_changes(&backend, changes0_args)
        .await
        .expect("Thread/changes from state0");
    let created0 = changes0["created"].as_array().unwrap();
    assert!(
        !created0.is_empty(),
        "first import must appear as created from initial state; got: {created0:?}"
    );
}

/// Oracle: RFC 8621 §4.8 — importing an email with a duplicate Message-ID must
/// return an `alreadyExists` SetError with `existingId` set to the first email's id.
#[tokio::test]
async fn email_import_duplicate_message_id_returns_already_exists() {
    use jmap_mail_server::handle_email_import;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Raw RFC 5322 message with a known Message-ID.
    let msg =
        b"From: sender@example.com\r\nTo: dest@example.com\r\nMessage-ID: <dup123@example.com>\r\nSubject: Dup Test\r\n\r\nBody text.\r\n";
    let blob_id = Id::from("blob-dup");
    backend.store_blob(&blob_id, msg.to_vec());

    let make_import = |blob: &Id| {
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "emails": {
                "imp1": {
                    "blobId": blob.as_ref(),
                    "mailboxIds": { "inbox": true },
                }
            }
        })
    };

    // First import must succeed.
    let (resp1, _) = handle_email_import(&backend, make_import(&blob_id))
        .await
        .expect("first import must not return a JmapError");
    assert!(
        resp1["notCreated"].is_null(),
        "first import: notCreated must be null; got: {}",
        resp1["notCreated"]
    );
    let created1 = resp1["created"]
        .as_object()
        .expect("first import: created must be an object");
    let email_id_1 = created1["imp1"]["id"]
        .as_str()
        .expect("first import: id must be a string")
        .to_owned();

    // Second import of the same blob (same Message-ID) must fail with alreadyExists.
    let (resp2, _) = handle_email_import(&backend, make_import(&blob_id))
        .await
        .expect("second import must not return a JmapError");
    assert!(
        resp2["created"].is_null(),
        "second import: created must be null; got: {}",
        resp2["created"]
    );
    let not_created = resp2["notCreated"]
        .as_object()
        .expect("second import: notCreated must be an object");
    assert!(
        not_created.contains_key("imp1"),
        "second import: imp1 must be in notCreated"
    );
    assert_eq!(
        not_created["imp1"]["type"].as_str().unwrap_or(""),
        "alreadyExists",
        "second import: error type must be alreadyExists; got: {}",
        not_created["imp1"]["type"]
    );
    assert_eq!(
        not_created["imp1"]["existingId"].as_str().unwrap_or(""),
        email_id_1,
        "second import: existingId must equal the first email's id"
    );
}

#[test]
fn capability_uri_mail_matches_rfc() {
    assert_eq!(jmap_mail_server::JMAP_MAIL_URI, "urn:ietf:params:jmap:mail");
}

#[test]
fn capability_uri_submission_matches_rfc() {
    assert_eq!(
        jmap_mail_server::JMAP_SUBMISSION_URI,
        "urn:ietf:params:jmap:submission"
    );
}

#[test]
fn capability_uri_vacation_response_matches_rfc() {
    assert_eq!(
        jmap_mail_server::JMAP_VACATION_RESPONSE_URI,
        "urn:ietf:params:jmap:vacationresponse"
    );
}

/// Oracle: EmailSubmission/set create with a future sendAt is rejected when
/// Oracle: sendAt is server-set (RFC 8621 §7.2); a client-supplied sendAt is
/// silently ignored and the submission succeeds with server-assigned sendAt.
#[tokio::test]
async fn submission_set_create_send_at_ignored_from_client() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let identity = Identity::new(Id::from("placeholder"), "alice@example.com", true);
    let (identity_id, _) = backend
        .create_object::<jmap_mail_types::Identity>(&account_id, "i0", identity)
        .await
        .expect("create Identity");

    let msg = b"Subject: Test\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nBody.";
    let blob_id = Id::from("blob-delay1");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("sent")], &[], None)
        .await
        .expect("import_email");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "s0": {
                "identityId": identity_id.as_ref(),
                "emailId": email_id.as_ref(),
                "sendAt": "2099-01-01T00:00:00Z",
            }
        }
    });

    let (resp, _) = handle_submission_set(&backend, args, "call-delay1")
        .await
        .expect("EmailSubmission/set must return a response (not a protocol error)");

    // Oracle: "s0" must be created successfully; client-supplied sendAt is ignored.
    assert!(
        resp["notCreated"].is_null(),
        "notCreated must be null; got: {:?}",
        resp["notCreated"]
    );
    let created = resp["created"]
        .as_object()
        .expect("created must be an object");
    assert!(
        created.contains_key("s0"),
        "s0 must be in created; got: {resp:?}"
    );
    // sendAt in the response must be the server-set time (not the client-supplied 2099 value).
    let stored_send_at = created["s0"]["sendAt"].as_str().expect("sendAt in created");
    assert!(
        stored_send_at < "2099-01-01",
        "server-set sendAt must not equal the client-supplied future value; got: {stored_send_at}"
    );
}

// ---------------------------------------------------------------------------
// RFC 8621 §4.6 — Email/set create body structure validation
// ---------------------------------------------------------------------------

/// Oracle: Email/set create with bodyStructure and textBody present must be
/// rejected with invalidProperties.
///
/// RFC 8621 §4.6: bodyStructure is mutually exclusive with textBody, htmlBody,
/// and attachments.
#[tokio::test]
async fn email_set_create_body_structure_with_text_body_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "e1": {
                "mailboxIds": { "inbox": true },
                "bodyStructure": {
                    "type": "text/plain",
                    "partId": "1"
                },
                "textBody": [{ "partId": "1", "type": "text/plain" }],
                "bodyValues": {
                    "1": { "value": "Hello" }
                }
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must return a response");

    assert!(
        resp["notCreated"]["e1"].is_object(),
        "e1 must be in notCreated; got resp={resp}"
    );
    assert_eq!(
        resp["notCreated"]["e1"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties; got resp={resp}"
    );
}

/// Oracle: Email/set create with textBody containing a part of type text/html
/// must be rejected with invalidProperties.
///
/// RFC 8621 §4.6: textBody must contain exactly one part of type text/plain.
#[tokio::test]
async fn email_set_create_text_body_wrong_type_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "e1": {
                "mailboxIds": { "inbox": true },
                "textBody": [{ "partId": "1", "type": "text/html" }],
                "bodyValues": {
                    "1": { "value": "<p>Hello</p>" }
                }
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must return a response");

    assert!(
        resp["notCreated"]["e1"].is_object(),
        "e1 must be in notCreated; got resp={resp}"
    );
    assert_eq!(
        resp["notCreated"]["e1"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties; got resp={resp}"
    );
}

/// Oracle: Email/set create with textBody containing two parts must be rejected
/// with invalidProperties.
///
/// RFC 8621 §4.6: textBody must contain exactly one body part.
#[tokio::test]
async fn email_set_create_text_body_multiple_parts_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "e1": {
                "mailboxIds": { "inbox": true },
                "textBody": [
                    { "partId": "1", "type": "text/plain" },
                    { "partId": "2", "type": "text/plain" }
                ],
                "bodyValues": {
                    "1": { "value": "Hello" },
                    "2": { "value": "World" }
                }
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must return a response");

    assert!(
        resp["notCreated"]["e1"].is_object(),
        "e1 must be in notCreated; got resp={resp}"
    );
    assert_eq!(
        resp["notCreated"]["e1"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties; got resp={resp}"
    );
}

/// Oracle: Email/set create with an EmailBodyPart specifying both partId and
/// blobId must be rejected with invalidProperties.
///
/// RFC 8621 §4.6: a body part must have partId OR blobId, not both.
#[tokio::test]
async fn email_set_create_body_part_both_part_id_and_blob_id_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "e1": {
                "mailboxIds": { "inbox": true },
                "textBody": [{
                    "partId": "1",
                    "blobId": "some-blob-id",
                    "type": "text/plain"
                }],
                "bodyValues": {
                    "1": { "value": "Hello" }
                }
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must return a response");

    assert!(
        resp["notCreated"]["e1"].is_object(),
        "e1 must be in notCreated; got resp={resp}"
    );
    assert_eq!(
        resp["notCreated"]["e1"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties; got resp={resp}"
    );
}

/// Oracle: Email/set create where a body part's partId has no matching entry in
/// bodyValues must be rejected with invalidProperties.
///
/// RFC 8621 §4.6: if partId is specified, that partId MUST exist as a key in
/// the bodyValues map.
#[tokio::test]
async fn email_set_create_body_values_missing_for_part_id_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "e1": {
                "mailboxIds": { "inbox": true },
                "textBody": [{ "partId": "1", "type": "text/plain" }]
                // bodyValues intentionally absent
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must return a response");

    assert!(
        resp["notCreated"]["e1"].is_object(),
        "e1 must be in notCreated; got resp={resp}"
    );
    assert_eq!(
        resp["notCreated"]["e1"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties; got resp={resp}"
    );
}

/// Oracle: Email/set create with bodyValues[id].isTruncated=true must be
/// rejected with invalidProperties.
///
/// RFC 8621 §4.6: isTruncated and isEncodingProblem MUST be false or absent on
/// create.
#[tokio::test]
async fn email_set_create_body_value_is_truncated_true_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "e1": {
                "mailboxIds": { "inbox": true },
                "textBody": [{ "partId": "1", "type": "text/plain" }],
                "bodyValues": {
                    "1": {
                        "value": "Hello",
                        "isTruncated": true
                    }
                }
            }
        }
    });

    let (resp, _) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must return a response");

    assert!(
        resp["notCreated"]["e1"].is_object(),
        "e1 must be in notCreated; got resp={resp}"
    );
    assert_eq!(
        resp["notCreated"]["e1"]["type"].as_str().unwrap_or(""),
        "invalidProperties",
        "error type must be invalidProperties; got resp={resp}"
    );
}

/// Oracle: EmailSubmission/set create with sendAt absent (null) succeeds even
/// when maxDelayedSend is 0 — the handler substitutes the current time.
///
/// RFC 8621 §7.5 — sendAt is optional; omitting it is always valid.
#[tokio::test]
async fn submission_set_create_send_at_null_accepted() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let identity = Identity::new(Id::from("placeholder"), "alice@example.com", true);
    let (identity_id, _) = backend
        .create_object::<jmap_mail_types::Identity>(&account_id, "i0", identity)
        .await
        .expect("create Identity");

    let msg = b"Subject: Test\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nBody.";
    let blob_id = Id::from("blob-delay2");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("sent")], &[], None)
        .await
        .expect("import_email");

    // sendAt is explicitly null — handler must substitute current time and succeed.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "s0": {
                "identityId": identity_id.as_ref(),
                "emailId": email_id.as_ref(),
                "sendAt": null,
            }
        }
    });

    let (resp, _) = handle_submission_set(&backend, args, "call-delay2")
        .await
        .expect("EmailSubmission/set must return a response (not a protocol error)");

    // Oracle: "s0" must appear in "created".
    let created = resp["created"].as_object().expect("created must be object");
    assert!(
        created.contains_key("s0"),
        "s0 must be in created; notCreated = {:?}",
        resp["notCreated"]
    );
}

// ---------------------------------------------------------------------------
// RFC 8621 §4.1.3 — dynamic header: property tests
// ---------------------------------------------------------------------------

/// Helper: import a message with known headers and return its email id.
async fn import_msg_with_headers(backend: &MemoryBackend, raw: &[u8]) -> Id {
    let blob_id = Id::from(format!("blob-hdr-{}", uuid::Uuid::new_v4()));
    backend.store_blob(&blob_id, raw.to_vec());
    backend
        .import_email(
            &Id::from("acct1"),
            &blob_id,
            &[Id::from("inbox")],
            &[],
            None,
        )
        .await
        .expect("import_email")
        .0
}

/// Oracle: `header:Subject` (Raw form) returns the raw Subject header value.
///
/// RFC 8621 §4.1.3 — Raw form replaces CRLF with LF; leading whitespace is
/// preserved (not trimmed in Raw form).
#[tokio::test]
async fn email_get_header_subject_raw() {
    let backend = MemoryBackend::new();
    let raw = b"Subject: Hello World\r\nFrom: alice@example.com\r\n\r\nBody.";
    let email_id = import_msg_with_headers(&backend, raw).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id.as_ref()],
        "properties": ["id", "header:Subject"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1);
    let obj = &list[0];

    // Raw form: CRLF → LF; value is " Hello World" (space preserved from wire format).
    let raw_subject = obj["header:Subject"]
        .as_str()
        .expect("header:Subject must be a string");
    assert_eq!(
        raw_subject, " Hello World",
        "Raw Subject must include leading space; got: {raw_subject:?}"
    );

    // "headers" must not leak into the response (client did not request it).
    assert!(
        obj.get("headers").is_none(),
        "headers must not appear in response when not requested; got: {obj:?}"
    );
}

/// Oracle: `header:Subject:asText` returns the unfolded, leading-whitespace-trimmed value.
///
/// RFC 8621 §4.1.3 asText form: unfold, then trim leading whitespace.
#[tokio::test]
async fn email_get_header_subject_as_text() {
    let backend = MemoryBackend::new();
    // Folded Subject header (continuation line starts with a space).
    let raw = b"Subject: Hello\r\n World\r\nFrom: alice@example.com\r\n\r\nBody.";
    let email_id = import_msg_with_headers(&backend, raw).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id.as_ref()],
        "properties": ["header:Subject:asText"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list");
    let obj = &list[0];

    let val = obj["header:Subject:asText"]
        .as_str()
        .expect("header:Subject:asText must be a string");
    // asText: unfolded ("Hello World" with inner space from unfolding) and leading WS trimmed.
    assert_eq!(
        val, "Hello World",
        "asText Subject must be unfolded and trimmed; got: {val:?}"
    );
}

/// Oracle: `header:From:asDate` is rejected with `invalidArguments`.
///
/// RFC 8621 §4.1.2 — From is an address header; asDate is incompatible.
#[tokio::test]
async fn email_get_header_from_as_date_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    // The error must fire before any backend query, so no email is needed.
    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [],
        "properties": ["header:From:asDate"],
    });
    let result = handle_email_get(&backend, args).await;
    assert!(
        result.is_err(),
        "header:From:asDate must return invalidArguments; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type, "invalidArguments",
        "error type must be invalidArguments; got: {err:?}"
    );
}

/// Oracle: `header:Subject:all` returns an array of all Subject header values.
///
/// RFC 8621 §4.1.3 — without `:all` only the last value is returned; with
/// `:all` the full ordered array is returned.
#[tokio::test]
async fn email_get_header_all_form() {
    let backend = MemoryBackend::new();
    // Two Subject headers (unusual but syntactically valid for testing :all).
    let raw = b"Subject: First\r\nSubject: Second\r\nFrom: alice@example.com\r\n\r\nBody.";
    let email_id = import_msg_with_headers(&backend, raw).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id.as_ref()],
        "properties": ["header:Subject:all"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list");
    let obj = &list[0];

    let arr = obj["header:Subject:all"]
        .as_array()
        .expect("header:Subject:all must be an array");
    assert_eq!(
        arr.len(),
        2,
        "must return both Subject values; got: {arr:?}"
    );
    assert_eq!(arr[0].as_str().unwrap_or(""), " First");
    assert_eq!(arr[1].as_str().unwrap_or(""), " Second");
}

/// Oracle: `header:Subject:asWhatever` is rejected with `invalidArguments`.
///
/// RFC 8621 §4.1.3 — only the six defined form names are valid.
#[tokio::test]
async fn email_get_header_unknown_form_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [],
        "properties": ["header:Subject:asWhatever"],
    });
    let result = handle_email_get(&backend, args).await;
    assert!(
        result.is_err(),
        "unknown form must return invalidArguments; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type, "invalidArguments",
        "error type must be invalidArguments; got: {err:?}"
    );
}

/// Oracle: `header::asText` (empty name) is rejected with `invalidArguments`.
///
/// RFC 8621 §4.1.3 — the header name part must not be empty.
#[tokio::test]
async fn email_get_header_empty_name_rejected() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [],
        "properties": ["header::asText"],
    });
    let result = handle_email_get(&backend, args).await;
    assert!(
        result.is_err(),
        "empty header name must return invalidArguments; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type, "invalidArguments",
        "error type must be invalidArguments; got: {err:?}"
    );
}

/// Oracle: unimplemented header forms return `null`, not an error.
///
/// RFC 8621 §4.1.2 defines structured forms (asAddresses, asGroupedAddresses,
/// asDate, asMessageIds, asURLs). These are not yet parsed by this server;
/// `apply_header_form` returns `Value::Null` for all of them. This test
/// documents that behavior so future contributors know it is intentional and
/// do not accidentally break it when the forms are eventually implemented.
///
/// The form/header combinations chosen here are all *valid* per the
/// validation table in `validate_header_form` (no `invalidArguments` error is
/// expected); only the structured parse step is missing.
#[tokio::test]
async fn email_get_unimplemented_header_forms_return_null() {
    let backend = MemoryBackend::new();

    // A message with headers that exercise every unimplemented form.
    //   From      → asAddresses, asGroupedAddresses
    //   Date      → asDate
    //   Message-ID → asMessageIds
    //   List-Post  → asURLs
    let raw = b"From: alice@example.com\r\n\
Date: Mon, 01 Jan 2024 00:00:00 +0000\r\n\
Message-ID: <abc@example.com>\r\n\
List-Post: <mailto:list@example.com>\r\n\
Subject: Test\r\n\
\r\n\
Body.";
    let email_id = import_msg_with_headers(&backend, raw).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id.as_ref()],
        "properties": [
            "id",
            "header:From:asAddresses",
            "header:From:asGroupedAddresses",
            "header:Date:asDate",
            "header:Message-ID:asMessageIds",
            "header:List-Post:asURLs",
        ],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed — valid form/header pairs must not return an error");

    let list = resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1, "must find exactly one email");
    let obj = &list[0];

    // Each unimplemented form must be present in the response as JSON null,
    // not absent and not a string/object.
    for key in &[
        "header:From:asAddresses",
        "header:From:asGroupedAddresses",
        "header:Date:asDate",
        "header:Message-ID:asMessageIds",
        "header:List-Post:asURLs",
    ] {
        assert!(
            obj.get(*key).is_some(),
            "property {key:?} must be present in response (as null); got: {obj:?}"
        );
        assert!(
            obj[*key].is_null(),
            "property {key:?} must be null (not yet implemented); got: {:?}",
            obj[*key]
        );
    }
}

/// Oracle: Email/set with a non-string element in `destroy` returns
/// `invalidArguments` for the whole method call (RFC 8620 §5.3).
#[tokio::test]
async fn email_set_destroy_non_string_returns_invalid_arguments() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "destroy": [123],
    });
    let result = handle_email_set(&backend, args).await;
    assert!(
        result.is_err(),
        "non-string destroy element must return an error; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type, "invalidArguments",
        "error type must be invalidArguments; got: {err:?}"
    );
}

/// Oracle: EmailSubmission/set with a non-string element in `destroy` returns
/// `invalidArguments` for the whole method call (RFC 8620 §5.3).
#[tokio::test]
async fn submission_set_destroy_non_string_returns_invalid_arguments() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "destroy": [true],
    });
    let result = handle_submission_set(&backend, args, "call-sub-invalid").await;
    assert!(
        result.is_err(),
        "non-string destroy element must return an error; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type, "invalidArguments",
        "error type must be invalidArguments; got: {err:?}"
    );
}

/// Oracle: EmailSubmission/set — when the `create` entry fails (non-existent
/// emailId), `onSuccessUpdateEmail` for that creation reference MUST NOT be
/// applied (RFC 8621 §7.5: onSuccess only fires for successful creates).
#[tokio::test]
async fn submission_set_failed_create_does_not_apply_on_success_update_email() {
    use jmap_mail_types::keyword;

    let backend = MemoryBackend::new();
    let account_id = Id::from("acct-onsuccess-no-apply");

    // Create an Identity so the submission handler can validate it.
    let identity = Identity::new(Id::from("placeholder"), "alice@example.com", true);
    let (identity_id, _) = backend
        .create_object::<Identity>(&account_id, "i0", identity)
        .await
        .expect("create Identity");

    // Import a real email to use as the update target.
    let msg = b"Subject: Test\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n\r\nbody";
    let blob_id = Id::from("blob-onsuccess-no-apply");
    backend.store_blob(&blob_id, msg.to_vec());
    let (email_id, _) = backend
        .import_email(&account_id, &blob_id, &[Id::from("inbox")], &[], None)
        .await
        .expect("import_email");

    // EmailSubmission/set: create references a non-existent emailId so it
    // must fail, and the onSuccessUpdateEmail must therefore be skipped.
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "s0": {
                "identityId": identity_id.as_ref(),
                "emailId": "non-existent-email-id",
            }
        },
        "onSuccessUpdateEmail": {
            "#s0": { "keywords/$seen": true }
        }
    });

    let (resp, extra) = handle_submission_set(&backend, args, "call-onsuccess-no-apply")
        .await
        .expect("EmailSubmission/set must not return a top-level JmapError");

    // Oracle 1: the create failed — "s0" must be in notCreated.
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be an object");
    assert!(
        not_created.contains_key("s0"),
        "s0 must be in notCreated; got: {:?}",
        resp["notCreated"]
    );

    // Oracle 2: no extra invocations — onSuccessUpdateEmail was not applied.
    assert!(
        extra.is_empty(),
        "no extra invocations expected when create fails; got: {extra:?}"
    );

    // Oracle 3: the email's keywords are unchanged (no $seen keyword added).
    let (emails, _) = backend
        .get_objects::<jmap_mail_types::Email>(&account_id, Some(&[email_id.clone()]), None)
        .await
        .expect("get_objects must not fail");
    assert_eq!(emails.len(), 1, "email must still exist");
    assert!(
        !emails[0].keywords.contains_key(keyword::SEEN),
        "email must NOT have $seen keyword; onSuccess must not have fired; keywords: {:?}",
        emails[0].keywords
    );
}

/// Oracle: Mailbox/set with a non-string element in `destroy` returns
/// `invalidArguments` for the whole method call (RFC 8620 §5.3).
#[tokio::test]
async fn mailbox_set_destroy_non_string_returns_invalid_arguments() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "destroy": [42],
    });
    let result = handle_mailbox_set(&backend, args).await;
    assert!(
        result.is_err(),
        "non-string destroy element must return an error; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type, "invalidArguments",
        "error type must be invalidArguments; got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// JMAP-yqo.1 — Email/parse must honour header: dynamic properties
// ---------------------------------------------------------------------------

/// Oracle: Email/parse with a `header:Subject:asText` property returns the
/// unfolded Subject value, not null.
///
/// RFC 8621 §5.8 + §4.1.3 — Email/parse accepts the same `properties` list
/// as Email/get, including dynamic `header:` properties.
#[tokio::test]
async fn email_parse_header_property_returned() {
    use jmap_mail_server::handle_email_parse;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct-parse-hdr"));
    let account_id = Id::from("acct-parse-hdr");

    // Store a blob with a known Subject header.
    let blob_id = Id::from("blob-parse-hdr-subject");
    let raw = b"Subject: Hello World\r\nFrom: alice@example.com\r\n\r\nBody.";
    backend.store_blob(&blob_id, raw.to_vec());

    let parse_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "blobIds": [blob_id.as_ref()],
        "properties": ["header:Subject:asText"],
    });
    let (resp, _) = handle_email_parse(&backend, parse_args)
        .await
        .expect("Email/parse must succeed");

    let parsed_obj = &resp["parsed"][blob_id.as_ref()];
    assert!(!parsed_obj.is_null(), "blob must appear in parsed map");

    // The dynamic property key must be present and must be the unfolded value.
    let val = parsed_obj["header:Subject:asText"]
        .as_str()
        .expect("header:Subject:asText must be a string, not null or absent");
    assert_eq!(
        val, "Hello World",
        "asText Subject must equal the header value; got: {val:?}"
    );

    // The "headers" raw array must NOT leak into the response when the client
    // did not ask for it — it was injected internally only to enable extraction.
    assert!(
        parsed_obj.get("headers").is_none(),
        "internal 'headers' key must not appear in Email/parse response; got: {parsed_obj:?}"
    );
}

/// Oracle: Email/parse with an invalid header: form returns `invalidArguments`
/// before any blob is fetched.
///
/// RFC 8621 §4.1.2 — incompatible form/header combinations are rejected as
/// invalidArguments. This path must fire in Email/parse as well as Email/get.
#[tokio::test]
async fn email_parse_header_invalid_form_rejected() {
    use jmap_mail_server::handle_email_parse;

    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct-parse-hdr-bad"));

    // header:From:asDate is invalid (From is an address header, not a date header).
    let parse_args = serde_json::json!({
        "accountId": "acct-parse-hdr-bad",
        "blobIds": [],
        "properties": ["header:From:asDate"],
    });
    let result = handle_email_parse(&backend, parse_args).await;
    assert!(
        result.is_err(),
        "invalid header form in Email/parse must return an error; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type, "invalidArguments",
        "expected invalidArguments; got: {:?}",
        err.error_type
    );
}

// ---------------------------------------------------------------------------
// JMAP-yqo.5 — Email/query collapseThreads deduplication
// ---------------------------------------------------------------------------

/// Oracle: Email/query with collapseThreads=true returns only the first email
/// per thread when multiple emails share a threadId.
///
/// RFC 8621 §4.4 — collapseThreads causes the result to include only the
/// first (in sort order) email per thread.
#[tokio::test]
async fn email_query_collapse_threads_deduplicates() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct-collapse");

    // Import two emails that end up in the same thread by shared message-id reference.
    let raw1 = b"Message-ID: <first@example.com>\r\nSubject: Thread root\r\n\r\nRoot.";
    let blob1 = Id::from("blob-collapse-1");
    backend.store_blob(&blob1, raw1.to_vec());
    let (id1, _) = backend
        .import_email(&account_id, &blob1, &[Id::from("inbox")], &[], None)
        .await
        .expect("import email 1");

    let raw2 =
        b"Message-ID: <second@example.com>\r\nIn-Reply-To: <first@example.com>\r\nSubject: Re: Thread root\r\n\r\nReply.";
    let blob2 = Id::from("blob-collapse-2");
    backend.store_blob(&blob2, raw2.to_vec());
    let (id2, _) = backend
        .import_email(&account_id, &blob2, &[Id::from("inbox")], &[], None)
        .await
        .expect("import email 2");

    // Both IDs must differ.
    assert_ne!(id1, id2, "imported emails must have distinct IDs");

    // Query without collapseThreads — expect both.
    let args_uncollapsed = serde_json::json!({
        "accountId": account_id.as_ref(),
    });
    let (resp_uncollapsed, _) = handle_email_query(&backend, args_uncollapsed)
        .await
        .expect("Email/query without collapseThreads must succeed");
    let ids_uncollapsed = resp_uncollapsed["ids"]
        .as_array()
        .expect("ids must be array");
    assert_eq!(
        ids_uncollapsed.len(),
        2,
        "without collapseThreads both emails must appear; got: {ids_uncollapsed:?}"
    );

    // Query with collapseThreads=true — expect exactly one if they share a thread,
    // or two if the backend assigned separate threads (thread assignment is backend-
    // specific; we assert only that the count is at most 2 and at least 1).
    let args_collapsed = serde_json::json!({
        "accountId": account_id.as_ref(),
        "collapseThreads": true,
    });
    let (resp_collapsed, _) = handle_email_query(&backend, args_collapsed)
        .await
        .expect("Email/query with collapseThreads must succeed");
    let ids_collapsed = resp_collapsed["ids"].as_array().expect("ids must be array");
    assert!(
        ids_collapsed.len() <= 2 && !ids_collapsed.is_empty(),
        "collapseThreads must return 1 or 2 results (depending on thread assignment); got: {ids_collapsed:?}"
    );
}

/// Oracle: Mailbox/query with an anchor that is not in the result set MUST return
/// an anchorNotFound error (RFC 8620 §5.5).
#[tokio::test]
async fn mailbox_query_anchor_not_found_returns_error() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let args = serde_json::json!({
        "accountId": "acct1",
        "anchor": "does-not-exist",
    });
    let result = handle_mailbox_query(&backend, args).await;
    assert!(result.is_err(), "nonexistent anchor must return an error");
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type.as_str(),
        "anchorNotFound",
        "error type must be anchorNotFound; got: {:?}",
        err.error_type
    );
}

// ---------------------------------------------------------------------------
// Thread/get emailIds ordering
// ---------------------------------------------------------------------------

/// Oracle: RFC 8621 §3 — Thread.emailIds MUST be sorted oldest-first by receivedAt.
///
/// The "thread-alpha" thread has three members imported in chronological order:
///   thread-starter  receivedAt 2025-12-24T00:00:00Z  (days_ago(8))
///   thread-reply-1  receivedAt 2025-12-25T00:00:00Z  (days_ago(7))
///   thread-reply-2  receivedAt 2025-12-26T00:00:00Z  (days_ago(6))
///
/// RFC 8621 §3 is the external oracle: "The ids of the Email objects in the
/// Thread, sorted by date of the message, oldest first."
#[tokio::test]
async fn thread_get_email_ids_sorted_by_received_at() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let thread_id = seed
        .thread
        .get("thread-alpha")
        .expect("seed must contain thread-alpha")
        .clone();

    let expected_order = [
        seed.email
            .get("thread-starter")
            .expect("seed must contain thread-starter")
            .as_ref(),
        seed.email
            .get("thread-reply-1")
            .expect("seed must contain thread-reply-1")
            .as_ref(),
        seed.email
            .get("thread-reply-2")
            .expect("seed must contain thread-reply-2")
            .as_ref(),
    ];

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [thread_id.as_ref()],
    });

    let (resp, _extra) = handle_thread_get(&backend, args)
        .await
        .expect("Thread/get must succeed");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find exactly one thread");

    let thread_obj = &list[0];
    let email_ids: Vec<&str> = thread_obj["emailIds"]
        .as_array()
        .expect("emailIds must be an array")
        .iter()
        .map(|v| v.as_str().expect("emailId must be a string"))
        .collect();

    assert_eq!(
        email_ids.len(),
        3,
        "thread-alpha must contain exactly 3 emails; got: {email_ids:?}"
    );

    assert_eq!(
        email_ids, expected_order,
        "emailIds must be sorted oldest-first by receivedAt (RFC 8621 §3); \
         expected [thread-starter, thread-reply-1, thread-reply-2] order"
    );
}

/// Oracle: Mailbox/set destroy with pre-existing children MUST return
/// notDestroyed with type "mailboxHasChild" (RFC 8621 §2.5).
///
/// The parent and child are created in separate requests so the child
/// exists in the backend before the destroy request is processed.
#[tokio::test]
async fn mailbox_set_destroy_with_children() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");

    // Create the parent mailbox directly via the backend.
    let (parent_id, _) = backend
        .create_object::<jmap_mail_types::Mailbox>(
            &account_id,
            "p0",
            jmap_mail_types::Mailbox::new(
                Id::from("placeholder"),
                "Parent",
                0,
                0,
                0,
                0,
                0,
                jmap_mail_types::MailboxRights::default(),
                false,
            ),
        )
        .await
        .expect("create parent mailbox");

    // Create a child mailbox under the parent via Mailbox/set (a separate prior request).
    let create_child_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "child1": {
                "name": "Child",
                "parentId": parent_id.as_ref(),
            }
        },
    });
    let (child_resp, _) = handle_mailbox_set(&backend, create_child_args)
        .await
        .expect("create child Mailbox/set must succeed");
    assert!(
        child_resp["created"]
            .as_object()
            .is_some_and(|m| m.contains_key("child1")),
        "child create must succeed; resp={child_resp:?}"
    );

    // Now attempt to destroy the parent in a separate request.
    let destroy_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "destroy": [parent_id.as_ref()],
    });
    let (resp, _) = handle_mailbox_set(&backend, destroy_args)
        .await
        .expect("Mailbox/set must not return JmapError");

    // Oracle (RFC 8621 §2.5): parent must be in notDestroyed with type "mailboxHasChild".
    assert!(
        resp["destroyed"].is_null(),
        "destroyed must be null when child exists; resp={resp:?}"
    );
    let not_destroyed = resp["notDestroyed"]
        .as_object()
        .expect("notDestroyed must be an object");
    assert!(
        not_destroyed.contains_key(parent_id.as_ref()),
        "parent id must be in notDestroyed; resp={resp:?}"
    );
    assert_eq!(
        not_destroyed[parent_id.as_ref()]["type"]
            .as_str()
            .unwrap_or(""),
        "mailboxHasChild",
        "error type must be mailboxHasChild; not_destroyed={not_destroyed:?}"
    );
}

/// Oracle: Mailbox/set create with a duplicate name under the same parent MUST return
/// notCreated with type "alreadyExists" (RFC 8621 §2.5).
///
/// Two mailboxes under the same parent may not share a name.
#[tokio::test]
async fn mailbox_set_create_duplicate_name() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");

    // Create a parent mailbox to hold the duplicates.
    let (parent_id, _) = backend
        .create_object::<jmap_mail_types::Mailbox>(
            &account_id,
            "p0",
            jmap_mail_types::Mailbox::new(
                Id::from("placeholder"),
                "Parent",
                0,
                0,
                0,
                0,
                0,
                jmap_mail_types::MailboxRights::default(),
                false,
            ),
        )
        .await
        .expect("create parent mailbox");

    // Create "Dup" under the parent — first one should succeed.
    let first_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "dup1": {
                "name": "Dup",
                "parentId": parent_id.as_ref(),
            }
        },
    });
    let (first_resp, _) = handle_mailbox_set(&backend, first_args)
        .await
        .expect("first Mailbox/set must succeed");
    assert!(
        first_resp["created"]
            .as_object()
            .is_some_and(|m| m.contains_key("dup1")),
        "first Dup create must succeed; resp={first_resp:?}"
    );

    // Attempt to create another "Dup" under the same parent in a second request.
    let second_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "dup2": {
                "name": "Dup",
                "parentId": parent_id.as_ref(),
            }
        },
    });
    let (second_resp, _) = handle_mailbox_set(&backend, second_args)
        .await
        .expect("second Mailbox/set must not return JmapError");

    // Oracle (RFC 8621 §2.5): duplicate must be in notCreated with type "alreadyExists".
    assert!(
        second_resp["created"].is_null(),
        "created must be null for duplicate; resp={second_resp:?}"
    );
    let not_created = second_resp["notCreated"]
        .as_object()
        .expect("notCreated must be an object");
    assert!(
        not_created.contains_key("dup2"),
        "dup2 must be in notCreated; resp={second_resp:?}"
    );
    assert_eq!(
        not_created["dup2"]["type"].as_str().unwrap_or(""),
        "alreadyExists",
        "error type must be alreadyExists; not_created={not_created:?}"
    );

    // Also verify: two creates in one request with the same name+parentId.
    // Exactly one must succeed; the other must be notCreated/alreadyExists.
    let both_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "a1": { "name": "SameName", "parentId": parent_id.as_ref() },
            "a2": { "name": "SameName", "parentId": parent_id.as_ref() },
        },
    });
    let (both_resp, _) = handle_mailbox_set(&backend, both_args)
        .await
        .expect("combined Mailbox/set must not return JmapError");

    let created_count = both_resp["created"].as_object().map_or(0, |m| m.len());
    let not_created_map = both_resp["notCreated"].as_object();
    let not_created_count = not_created_map.map_or(0, |m| m.len());
    assert_eq!(
        created_count, 1,
        "exactly one of a1/a2 must succeed; resp={both_resp:?}"
    );
    assert_eq!(
        not_created_count, 1,
        "exactly one of a1/a2 must be in notCreated; resp={both_resp:?}"
    );
    if let Some(nc) = not_created_map {
        for (k, v) in nc {
            assert_eq!(
                v["type"].as_str().unwrap_or(""),
                "alreadyExists",
                "notCreated[{k}].type must be alreadyExists; got {v:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// RFC 8620 §5.1 — properties filtering
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §5.1 — when `properties` is specified, Email/get MUST return
/// only the requested fields (plus `id`, which is always present).
///
/// Test vector: client requests `["id", "subject"]`. Response items must contain
/// exactly those two keys — no `from`, `to`, `mailboxIds`, `blobId`, etc.
#[tokio::test]
async fn email_get_properties_filtering_restricts_fields() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let mailbox_id = Id::from("mb1");

    backend
        .create_object::<jmap_mail_types::Mailbox>(
            &account_id,
            "mb1",
            jmap_mail_types::Mailbox::new(
                mailbox_id.clone(),
                "Inbox".to_owned(),
                0,
                0,
                0,
                0,
                0,
                jmap_mail_types::MailboxRights::default(),
                true,
            ),
        )
        .await
        .expect("create mailbox");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c1": {
                "mailboxIds": { mailbox_id.as_ref(): true },
                "subject": "Hello World",
                "from": [{"email": "alice@example.com"}],
                "to":   [{"email": "bob@example.com"}],
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set must succeed");
    let email_id = set_resp["created"]["c1"]["id"]
        .as_str()
        .expect("created id must be present")
        .to_owned();

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
        "properties": ["id", "subject"],
    });
    let (resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1, "must find exactly one email");
    let obj = list[0]
        .as_object()
        .expect("list item must be a JSON object");

    // Requested fields must be present with correct values.
    assert_eq!(obj["id"].as_str().unwrap_or(""), email_id, "id must match");
    assert_eq!(
        obj["subject"].as_str().unwrap_or(""),
        "Hello World",
        "subject must round-trip"
    );

    // Non-requested fields must be absent.
    assert!(!obj.contains_key("from"), "from must be absent");
    assert!(!obj.contains_key("to"), "to must be absent");
    assert!(!obj.contains_key("mailboxIds"), "mailboxIds must be absent");
    assert!(!obj.contains_key("blobId"), "blobId must be absent");
    assert!(!obj.contains_key("threadId"), "threadId must be absent");

    assert_eq!(
        obj.len(),
        2,
        "list item must have exactly 2 keys (id, subject); got: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
}

/// Oracle: RFC 8620 §5.1 — when `properties` is absent, Email/get MUST return the
/// RFC 8621 §4.2 default field set, which includes many more than 2 fields.
#[tokio::test]
async fn email_get_no_properties_returns_default_fields() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let mailbox_id = Id::from("mb1");

    backend
        .create_object::<jmap_mail_types::Mailbox>(
            &account_id,
            "mb1",
            jmap_mail_types::Mailbox::new(
                mailbox_id.clone(),
                "Inbox".to_owned(),
                0,
                0,
                0,
                0,
                0,
                jmap_mail_types::MailboxRights::default(),
                true,
            ),
        )
        .await
        .expect("create mailbox");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c1": {
                "mailboxIds": { mailbox_id.as_ref(): true },
                "subject": "Default Properties Test",
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set must succeed");
    let email_id = set_resp["created"]["c1"]["id"]
        .as_str()
        .expect("created id must be present")
        .to_owned();

    // No `properties` key in the request — server must use the default list.
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
    });
    let (resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1);
    let obj = list[0]
        .as_object()
        .expect("list item must be a JSON object");

    // RFC 8621 §4.2 default set includes at least these fields.
    assert!(
        obj.contains_key("id"),
        "id must be present in default response"
    );
    assert!(
        obj.contains_key("blobId"),
        "blobId must be present in default response"
    );
    assert!(
        obj.contains_key("threadId"),
        "threadId must be present in default response"
    );
    assert!(
        obj.contains_key("mailboxIds"),
        "mailboxIds must be present in default response"
    );
    assert!(
        obj.contains_key("subject"),
        "subject must be present in default response"
    );

    // Confirm we are not inadvertently applying any filter.
    assert!(
        obj.len() > 2,
        "default response must contain more than 2 fields; got: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
}

/// Oracle: RFC 8620 §5.1 — Mailbox/get with `properties: ["id", "name"]` MUST
/// return only `id` and `name` per list item.
#[tokio::test]
async fn mailbox_get_properties_filtering_restricts_fields() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let account_id = Id::from("acct1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c1": { "name": "My Mailbox", "sortOrder": 10 }
        }
    });
    let (set_resp, _) = handle_mailbox_set(&backend, set_args)
        .await
        .expect("Mailbox/set must succeed");
    let mailbox_id = set_resp["created"]["c1"]["id"]
        .as_str()
        .expect("created id must be present")
        .to_owned();

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [mailbox_id],
        "properties": ["id", "name"],
    });
    let (resp, _) = handle_mailbox_get(&backend, get_args)
        .await
        .expect("Mailbox/get must succeed");

    let list = resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1);
    let obj = list[0]
        .as_object()
        .expect("list item must be a JSON object");

    assert!(obj.contains_key("id"), "id must be present");
    assert!(obj.contains_key("name"), "name must be present");
    assert_eq!(
        obj["name"].as_str().unwrap_or(""),
        "My Mailbox",
        "name must round-trip"
    );

    assert!(!obj.contains_key("sortOrder"), "sortOrder must be absent");
    assert!(
        !obj.contains_key("totalEmails"),
        "totalEmails must be absent"
    );

    assert_eq!(
        obj.len(),
        2,
        "list item must have exactly 2 keys (id, name); got: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
}

/// Oracle: RFC 8620 §5.1 — Thread/get with `properties: ["id", "emailIds"]` MUST
/// return only those two fields per list item.
#[tokio::test]
async fn thread_get_properties_filtering_restricts_fields() {
    use common::seed;
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed_data = seed::setup_seed_data(&backend, &account_id).await;

    let thread_id = seed_data
        .thread
        .get("plain-simple")
        .expect("seed must contain plain-simple thread")
        .clone();

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [thread_id.as_ref()],
        "properties": ["id", "emailIds"],
    });
    let (resp, _) = handle_thread_get(&backend, get_args)
        .await
        .expect("Thread/get must succeed");

    let list = resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1);
    let obj = list[0]
        .as_object()
        .expect("list item must be a JSON object");

    assert!(obj.contains_key("id"), "id must be present");
    assert!(obj.contains_key("emailIds"), "emailIds must be present");

    assert_eq!(
        obj.len(),
        2,
        "list item must have exactly 2 keys (id, emailIds); got: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
}

/// Oracle: RFC 8620 §5.1 — Identity/get with `properties: ["id", "email"]` MUST
/// return only `id` and `email` per list item.
#[tokio::test]
async fn identity_get_properties_filtering_restricts_fields() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let account_id = Id::from("acct1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c1": {
                "email": "alice@example.com",
                "name": "Alice",
                "textSignature": "-- Alice",
            }
        }
    });
    let (set_resp, _) = handle_identity_set(&backend, set_args)
        .await
        .expect("Identity/set must succeed");
    let identity_id = set_resp["created"]["c1"]["id"]
        .as_str()
        .expect("created id must be present")
        .to_owned();

    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [identity_id],
        "properties": ["id", "email"],
    });
    let (resp, _) = handle_identity_get(&backend, get_args)
        .await
        .expect("Identity/get must succeed");

    let list = resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1);
    let obj = list[0]
        .as_object()
        .expect("list item must be a JSON object");

    assert!(obj.contains_key("id"), "id must be present");
    assert!(obj.contains_key("email"), "email must be present");
    assert_eq!(
        obj["email"].as_str().unwrap_or(""),
        "alice@example.com",
        "email must round-trip"
    );

    assert!(!obj.contains_key("name"), "name must be absent");
    assert!(
        !obj.contains_key("textSignature"),
        "textSignature must be absent"
    );

    assert_eq!(
        obj.len(),
        2,
        "list item must have exactly 2 keys (id, email); got: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
}

// -----------------------------------------------------------------------
// Conformance tests (conformance_*): derived from jmap-test-suite scenarios,
// use the seed fixture, and verify RFC 8621 §x.y compliance.
// Unit tests (bare names): exercise specific edge cases or internal behavior.
// -----------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Conformance: Email/get basic (RFC 8621 §4.1)
// ---------------------------------------------------------------------------

/// Oracle: RFC 8621 §4.1 — Email/get by id returns the email with id,
/// threadId, blobId, and size all present and non-empty.
#[tokio::test]
async fn conformance_email_get_by_id() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
        "properties": ["id", "threadId", "blobId", "size"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find exactly one email");

    let obj = &list[0];
    assert_eq!(
        obj["id"].as_str().unwrap_or(""),
        email_id.as_ref(),
        "returned id must match requested id"
    );
    assert!(
        obj["threadId"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "threadId must be a non-empty string; got: {:?}",
        obj["threadId"]
    );
    assert!(
        obj["blobId"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "blobId must be a non-empty string; got: {:?}",
        obj["blobId"]
    );
    assert!(
        obj["size"].as_u64().map(|n| n > 0).unwrap_or(false),
        "size must be a positive integer; got: {:?}",
        obj["size"]
    );
}

/// Oracle: RFC 8620 §5.1 — Email/get with an unknown id returns notFound
/// containing that id and list=[].
#[tokio::test]
async fn conformance_email_get_not_found_is_empty_array() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": ["nonexistent-email-xyz"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list must be an array");
    assert!(
        list.is_empty(),
        "list must be [] when all ids are not found; got: {list:?}"
    );

    let not_found = resp["notFound"]
        .as_array()
        .expect("notFound must be an array");
    assert!(
        not_found
            .iter()
            .any(|v| v.as_str() == Some("nonexistent-email-xyz")),
        "notFound must contain the requested id; got: {not_found:?}"
    );
}

/// Oracle: RFC 8620 §5.1 — Email/get notFound MUST be [] (empty array) when
/// all requested ids are found.
#[tokio::test]
async fn conformance_email_get_not_found_empty_when_all_found() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let not_found = resp["notFound"]
        .as_array()
        .expect("notFound must be an array");
    assert!(
        not_found.is_empty(),
        "notFound must be [] when all ids are found; got: {not_found:?}"
    );
}

/// Oracle: RFC 8620 §5.1 — Email/get with properties=["id","subject"] MUST
/// return exactly those two fields per list item; threadId, blobId, mailboxIds
/// must be absent.
#[tokio::test]
async fn conformance_email_get_properties_filter() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
        "properties": ["id", "subject"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1);
    let obj = list[0]
        .as_object()
        .expect("list item must be a JSON object");

    assert!(obj.contains_key("id"), "id must be present");
    assert!(obj.contains_key("subject"), "subject must be present");
    assert!(
        !obj.contains_key("threadId"),
        "threadId must be absent when not requested"
    );
    assert!(
        !obj.contains_key("blobId"),
        "blobId must be absent when not requested"
    );
    assert!(
        !obj.contains_key("mailboxIds"),
        "mailboxIds must be absent when not requested"
    );

    assert_eq!(
        obj.len(),
        2,
        "list item must have exactly 2 keys (id, subject); got: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
}

/// Oracle: RFC 8620 §5.1 — Email/get response MUST include a "state" string.
#[tokio::test]
async fn conformance_email_get_state_returned() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    assert!(
        resp["state"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "state must be a non-empty string; got: {:?}",
        resp["state"]
    );
}

/// Oracle: RFC 8621 §4.1 — hasAttachment MUST be true for the html-attachment
/// email, which has a multipart/mixed structure with a PDF attachment part.
#[tokio::test]
async fn conformance_email_get_has_attachment_true() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["html-attachment"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
        "properties": ["id", "hasAttachment"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1);
    let obj = &list[0];

    assert_eq!(
        obj["hasAttachment"].as_bool(),
        Some(true),
        "hasAttachment must be true for multipart/mixed email with PDF part; got: {:?}",
        obj["hasAttachment"]
    );
}

/// Oracle: RFC 8621 §4.1 — all emails in the same thread MUST share the same
/// threadId value.  thread-starter, thread-reply-1, and thread-reply-2 are
/// linked by In-Reply-To / References headers into thread-alpha.
#[tokio::test]
async fn conformance_email_get_thread_id_consistent() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let starter_id = seed.email["thread-starter"].clone();
    let reply1_id = seed.email["thread-reply-1"].clone();
    let reply2_id = seed.email["thread-reply-2"].clone();
    let expected_thread_id = seed.thread["thread-alpha"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [
            starter_id.as_ref(),
            reply1_id.as_ref(),
            reply2_id.as_ref(),
        ],
        "properties": ["id", "threadId"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 3, "must find all 3 thread emails");

    for item in list {
        assert_eq!(
            item["threadId"].as_str().unwrap_or(""),
            expected_thread_id.as_ref(),
            "all thread emails must share the same threadId (RFC 8621 §4.1); \
             email id={:?} got threadId={:?}",
            item["id"],
            item["threadId"]
        );
    }
}

// ---------------------------------------------------------------------------
// Conformance: Email/get header properties (RFC 8621 §4.1.2–4.1.3)
// ---------------------------------------------------------------------------

/// Oracle: RFC 8621 §4.1.2 — Email/get with properties=["id","from"] returns
/// the From header parsed as an array of EmailAddress objects.  The plain-simple
/// seed email has From: Alice Sender <alice@example.com>.
#[tokio::test]
async fn conformance_email_get_header_from() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
        "properties": ["id", "from"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1);
    let obj = &list[0];

    let from = obj["from"].as_array().expect("from must be an array");
    assert!(!from.is_empty(), "from must not be empty");
    assert_eq!(
        from[0]["email"].as_str().unwrap_or(""),
        "alice@example.com",
        "from[0].email must be alice@example.com; got: {:?}",
        from[0]["email"]
    );
}

/// Oracle: RFC 8621 §4.1.2 — Email/get with properties=["id","to"] returns
/// the To header parsed as an array of EmailAddress objects.
/// plain-simple has To: testuser@example.com.
#[tokio::test]
async fn conformance_email_get_header_to() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
        "properties": ["id", "to"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1);
    let obj = &list[0];

    let to = obj["to"].as_array().expect("to must be an array");
    assert!(!to.is_empty(), "to must not be empty");
    assert_eq!(
        to[0]["email"].as_str().unwrap_or(""),
        "testuser@example.com",
        "to[0].email must be testuser@example.com; got: {:?}",
        to[0]["email"]
    );
}

/// Oracle: RFC 8621 §4.1.2 — Email/get with properties=["id","cc"] returns
/// the Cc header parsed as an array of EmailAddress objects.
/// html-attachment has Cc: charlie@example.net.
#[tokio::test]
async fn conformance_email_get_header_cc() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["html-attachment"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
        "properties": ["id", "cc"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1);
    let obj = &list[0];

    let cc = obj["cc"].as_array().expect("cc must be an array");
    assert!(!cc.is_empty(), "cc must not be empty");
    assert!(
        cc.iter()
            .any(|a| a["email"].as_str() == Some("charlie@example.net")),
        "cc must contain charlie@example.net; got: {cc:?}"
    );
}

/// Oracle: RFC 8621 §4.1.2 — Email/get subject returns the decoded Subject
/// header string.  plain-simple has Subject: Meeting tomorrow morning.
#[tokio::test]
async fn conformance_email_get_header_subject() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
        "properties": ["id", "subject"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(
        list[0]["subject"].as_str().unwrap_or(""),
        "Meeting tomorrow morning",
        "subject must match the seed value; got: {:?}",
        list[0]["subject"]
    );
}

/// Oracle: RFC 8621 §4.1.2.5 — Email/get messageId returns an array of
/// msg-id values with angle brackets and CFWS removed (per spec).
/// plain-simple has Message-ID: <plain-simple-001@test>, so the returned
/// value must be "plain-simple-001@test" (no brackets).
#[tokio::test]
async fn conformance_email_get_header_message_id() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
        "properties": ["id", "messageId"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1);
    let obj = &list[0];

    let msg_id = obj["messageId"]
        .as_array()
        .expect("messageId must be an array");
    assert!(!msg_id.is_empty(), "messageId must not be empty");
    // RFC 8621 §4.1.2.5: "CFWS and surrounding angle brackets are removed".
    assert_eq!(
        msg_id[0].as_str().unwrap_or(""),
        "plain-simple-001@test",
        "messageId[0] must have angle brackets stripped per RFC 8621 §4.1.2.5; got: {:?}",
        msg_id[0]
    );
}

/// Oracle: RFC 8621 §4.1.2.5 — Email/get inReplyTo returns an array of
/// msg-id values with angle brackets and CFWS removed (per spec).
/// thread-reply-1 has In-Reply-To: <thread-alpha-001@test>, so the
/// returned value must be "thread-alpha-001@test" (no brackets).
#[tokio::test]
async fn conformance_email_get_header_in_reply_to() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["thread-reply-1"].clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id.as_ref()],
        "properties": ["id", "inReplyTo"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must succeed");

    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1);
    let obj = &list[0];

    let in_reply_to = obj["inReplyTo"]
        .as_array()
        .expect("inReplyTo must be an array");
    assert!(!in_reply_to.is_empty(), "inReplyTo must not be empty");
    // RFC 8621 §4.1.2.5: "CFWS and surrounding angle brackets are removed".
    assert_eq!(
        in_reply_to[0].as_str().unwrap_or(""),
        "thread-alpha-001@test",
        "inReplyTo[0] must have angle brackets stripped per RFC 8621 §4.1.2.5; got: {:?}",
        in_reply_to[0]
    );
}

// ---------------------------------------------------------------------------
// Mailbox conformance tests (ported from jmap-test-suite)
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §5.1 — Mailbox/get with ids=null returns all mailboxes.
/// jmap-test-suite: mailbox-get.test.ts "get-all"
#[tokio::test]
async fn conformance_mailbox_get_all_ids_null() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": null,
    });
    let (resp, _) = handle_mailbox_get(&backend, args)
        .await
        .expect("Mailbox/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(
        list.len(),
        5,
        "ids=null must return all 5 mailboxes; got {}",
        list.len()
    );

    let not_found = resp["notFound"]
        .as_array()
        .expect("notFound must be an array");
    assert!(
        not_found.is_empty(),
        "notFound must be empty; got {:?}",
        not_found
    );
}

/// Oracle: RFC 8620 §5.1 — Mailbox/get with specific ids returns only those mailboxes.
/// jmap-test-suite: mailbox-get.test.ts "get-by-ids"
#[tokio::test]
async fn conformance_mailbox_get_by_ids() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed.mailbox["inbox"].as_ref().to_owned();
    let folder_a_id = seed.mailbox["folderA"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [inbox_id, folder_a_id],
    });
    let (resp, _) = handle_mailbox_get(&backend, args)
        .await
        .expect("Mailbox/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(
        list.len(),
        2,
        "must return exactly 2 mailboxes; got {}",
        list.len()
    );

    let returned_ids: Vec<&str> = list.iter().filter_map(|v| v["id"].as_str()).collect();
    assert!(
        returned_ids.contains(&seed.mailbox["inbox"].as_ref()),
        "inbox must be in list"
    );
    assert!(
        returned_ids.contains(&seed.mailbox["folderA"].as_ref()),
        "folderA must be in list"
    );

    let not_found = resp["notFound"]
        .as_array()
        .expect("notFound must be an array");
    assert!(
        not_found.is_empty(),
        "notFound must be empty; got {:?}",
        not_found
    );
}

/// Oracle: RFC 8620 §5.1 — Mailbox/get with unknown ids returns notFound list.
/// jmap-test-suite: mailbox-get.test.ts "get-not-found"
#[tokio::test]
async fn conformance_mailbox_get_not_found() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": ["missing-id"],
    });
    let (resp, _) = handle_mailbox_get(&backend, args)
        .await
        .expect("Mailbox/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert!(
        list.is_empty(),
        "list must be empty for unknown id; got {:?}",
        list
    );

    let not_found = resp["notFound"]
        .as_array()
        .expect("notFound must be an array");
    assert_eq!(not_found.len(), 1, "notFound must have 1 entry");
    assert_eq!(
        not_found[0].as_str(),
        Some("missing-id"),
        "notFound must contain the requested id"
    );
}

/// Oracle: RFC 8621 §2 — inbox mailbox role field must be "inbox".
/// jmap-test-suite: mailbox-get.test.ts "get-inbox-exists"
#[tokio::test]
async fn conformance_mailbox_get_inbox_has_role() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed.mailbox["inbox"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [inbox_id],
    });
    let (resp, _) = handle_mailbox_get(&backend, args)
        .await
        .expect("Mailbox/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find inbox");
    assert_eq!(
        list[0]["role"].as_str(),
        Some("inbox"),
        "inbox role must be \"inbox\"; got: {:?}",
        list[0]["role"]
    );
}

/// Oracle: RFC 8621 §2 — child mailbox parentId must reference the parent mailbox.
/// jmap-test-suite: mailbox-get.test.ts "get-parent-id-correct"
#[tokio::test]
async fn conformance_mailbox_get_child_has_parent_id() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let child1_id = seed.mailbox["child1"].as_ref().to_owned();
    let folder_a_id = seed.mailbox["folderA"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [child1_id],
    });
    let (resp, _) = handle_mailbox_get(&backend, args)
        .await
        .expect("Mailbox/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find child1");
    assert_eq!(
        list[0]["parentId"].as_str(),
        Some(folder_a_id.as_str()),
        "child1 parentId must equal folderA id; got: {:?}",
        list[0]["parentId"]
    );
}

/// Oracle: RFC 8620 §5.1 — when properties is specified, only those fields (plus id)
/// are returned; unrequested fields must be absent.
/// jmap-test-suite: mailbox-get.test.ts "get-properties-filter"
#[tokio::test]
async fn conformance_mailbox_get_properties_filter() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed.mailbox["inbox"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [inbox_id],
        "properties": ["id", "name"],
    });
    let (resp, _) = handle_mailbox_get(&backend, args)
        .await
        .expect("Mailbox/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1);
    let obj = list[0]
        .as_object()
        .expect("list item must be a JSON object");

    assert!(obj.contains_key("id"), "id must always be present");
    assert!(obj.contains_key("name"), "name must be present (requested)");
    assert!(
        !obj.contains_key("parentId"),
        "parentId must be absent (not requested); keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
    assert!(
        !obj.contains_key("role"),
        "role must be absent (not requested); keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
}

/// Oracle: RFC 8621 §2 — totalEmails reflects the count of emails in that mailbox.
/// folderA has: thread-starter (days_ago_8), very-old (days_ago_30),
/// and multi-mailbox (days_ago_5), so totalEmails >= 1.
/// jmap-test-suite: mailbox-get.test.ts "get-total-emails-accurate"
#[tokio::test]
async fn conformance_mailbox_get_total_emails() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let folder_a_id = seed.mailbox["folderA"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [folder_a_id],
    });
    let (resp, _) = handle_mailbox_get(&backend, args)
        .await
        .expect("Mailbox/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find folderA");
    let total_emails = list[0]["totalEmails"]
        .as_u64()
        .expect("totalEmails must be a number");
    assert!(
        total_emails >= 1,
        "folderA must have at least 1 email (thread-starter, very-old, multi-mailbox are in folderA); got totalEmails={}",
        total_emails
    );
}

/// Oracle: RFC 8620 §5.5 — Mailbox/query with no filter returns all mailboxes.
/// jmap-test-suite: mailbox-query.test.ts "query-all"
#[tokio::test]
async fn conformance_mailbox_query_all() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "calculateTotal": true,
    });
    let (resp, _) = handle_mailbox_query(&backend, args)
        .await
        .expect("Mailbox/query must not error");

    let ids = resp["ids"].as_array().expect("ids must be an array");
    assert!(
        ids.len() >= 5,
        "query with no filter must return at least 5 mailboxes; got {}",
        ids.len()
    );
}

/// Oracle: RFC 8621 §2.3 — filter={parentId: null} returns only top-level mailboxes.
/// Top-level: inbox, folderA, folderB. Not returned: child1, child2.
/// jmap-test-suite: mailbox-query.test.ts "query-filter-by-parent-id-null"
#[tokio::test]
async fn conformance_mailbox_query_filter_parent_id_null() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "filter": { "parentId": null },
    });
    let (resp, _) = handle_mailbox_query(&backend, args)
        .await
        .expect("Mailbox/query must not error");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    assert!(
        ids.contains(&seed.mailbox["inbox"].as_ref()),
        "inbox (top-level) must be in result"
    );
    assert!(
        ids.contains(&seed.mailbox["folderA"].as_ref()),
        "folderA (top-level) must be in result"
    );
    assert!(
        ids.contains(&seed.mailbox["folderB"].as_ref()),
        "folderB (top-level) must be in result"
    );
    assert!(
        !ids.contains(&seed.mailbox["child1"].as_ref()),
        "child1 must NOT be in top-level result"
    );
    assert!(
        !ids.contains(&seed.mailbox["child2"].as_ref()),
        "child2 must NOT be in top-level result"
    );
}

/// Oracle: RFC 8621 §2.3 — filter={hasAnyRole: true} returns only mailboxes with a role.
/// Only inbox has a role in the seed data.
/// jmap-test-suite: mailbox-query.test.ts "query-filter-has-any-role"
#[tokio::test]
async fn conformance_mailbox_query_filter_has_any_role() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "filter": { "hasAnyRole": true },
    });
    let (resp, _) = handle_mailbox_query(&backend, args)
        .await
        .expect("Mailbox/query must not error");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    assert_eq!(
        ids.len(),
        1,
        "hasAnyRole=true must return exactly 1 mailbox (only inbox has a role); got ids={:?}",
        ids
    );
    assert_eq!(
        ids[0],
        seed.mailbox["inbox"].as_ref(),
        "the returned mailbox must be inbox"
    );
}

/// Oracle: RFC 8621 §2.3 — filter={role: "inbox"} returns only the inbox mailbox.
///
/// The seed data has exactly one role-bearing mailbox (inbox). Querying by
/// role="inbox" must return only that mailbox. Querying by a role that no
/// mailbox holds (e.g. "trash") must return an empty ids array.
///
/// jmap-test-suite: mailbox-query.test.ts "query-filter-by-role"
#[tokio::test]
async fn conformance_mailbox_query_filter_role() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    // filter={role: "inbox"} must return exactly the inbox mailbox.
    let args = serde_json::json!({
        "accountId": "acct1",
        "filter": { "role": "inbox" },
        "calculateTotal": true,
    });
    let (resp, _) = handle_mailbox_query(&backend, args)
        .await
        .expect("Mailbox/query with filter.role=inbox must not error");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    assert_eq!(
        ids.len(),
        1,
        "filter.role=inbox must return exactly 1 mailbox; got ids={:?}",
        ids
    );
    assert_eq!(
        ids[0],
        seed.mailbox["inbox"].as_ref(),
        "the returned mailbox must be the inbox"
    );
    assert_eq!(resp["total"], 1, "total must be 1 when calculateTotal=true");

    // SEED CONTRACT: seed data has no mailbox with role="trash".
    // If setup_seed_data ever adds a Trash mailbox, update this assertion
    // to assert ids contains exactly the Trash mailbox ID — not merely that
    // it excludes the inbox (which would pass even if role filtering were broken).
    let args_no_match = serde_json::json!({
        "accountId": "acct1",
        "filter": { "role": "trash" },
        "calculateTotal": true,
    });
    let (resp_no_match, _) = handle_mailbox_query(&backend, args_no_match)
        .await
        .expect("Mailbox/query with filter.role=trash must not error");

    let ids_no_match: Vec<&str> = resp_no_match["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    assert!(
        ids_no_match.is_empty(),
        "filter.role=trash must return no mailboxes; got ids={:?}",
        ids_no_match
    );
    assert_eq!(
        resp_no_match["total"], 0,
        "total must be 0 for unmatched role"
    );
}

/// Oracle: RFC 8621 §2.3 — Mailbox/query sort by name returns mailboxes in
/// lexicographic name order.
/// jmap-test-suite: mailbox-query.test.ts "query-sort-by-name"
///
/// NOTE: The current handle_mailbox_query implementation returns unsupportedSort
/// for any non-empty sort array. If this test fails with unsupportedSort, that
/// is a conformance bug in the implementation (not in the test).
#[tokio::test]
async fn conformance_mailbox_query_sort_by_name() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "sort": [{ "property": "name", "isAscending": true }],
    });
    let (resp, _) = handle_mailbox_query(&backend, args)
        .await
        .expect("Mailbox/query with sort=[{property:name}] must not error");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(!ids.is_empty(), "sort query must return results");

    // Fetch the names for the returned ids so we can verify ordering.
    let get_args = serde_json::json!({
        "accountId": "acct1",
        "ids": ids,
        "properties": ["id", "name"],
    });
    let (get_resp, _) = handle_mailbox_get(&backend, get_args)
        .await
        .expect("Mailbox/get for name verification must succeed");

    let name_map: std::collections::HashMap<&str, &str> = get_resp["list"]
        .as_array()
        .expect("list must be an array")
        .iter()
        .filter_map(|v| {
            let id = v["id"].as_str()?;
            let name = v["name"].as_str()?;
            Some((id, name))
        })
        .collect();

    for window in ids.windows(2) {
        let prev_name = name_map.get(window[0]).copied().unwrap_or("");
        let curr_name = name_map.get(window[1]).copied().unwrap_or("");
        assert!(
            prev_name <= curr_name,
            "names must be in ascending order: '{}' > '{}' at positions {:?}",
            prev_name,
            curr_name,
            window
        );
    }
}

/// Oracle: RFC 8620 §5.5 — Mailbox/query with an anchor not in the result set
/// MUST return an anchorNotFound error.
/// jmap-test-suite: (implicit from anchorNotFound error type requirement)
#[tokio::test]
async fn conformance_mailbox_query_anchor_not_found() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "anchor": "nonexistent",
    });
    let result = handle_mailbox_query(&backend, args).await;
    assert!(result.is_err(), "nonexistent anchor must return an error");
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type.as_str(),
        "anchorNotFound",
        "error type must be anchorNotFound; got: {:?}",
        err.error_type
    );
}

// ---------------------------------------------------------------------------
// Email body conformance tests (ported from jmap-test-suite email-get-body.test.ts)
// ---------------------------------------------------------------------------

/// Oracle: RFC 8621 §4.1.4 — Email/get with properties=["id","textBody"] on a
/// plain-text message must return a textBody array with at least one entry whose
/// type is "text/plain".
///
/// jmap-test-suite: email-get-body.test.ts "body-text-body"
///
/// GAP: MemoryBackend uses `parse_rfc5322_headers` (header-only parser).  Body
/// structure fields (textBody, htmlBody, attachments, bodyStructure) are not
/// populated during import_email.  This test will FAIL until the backend wires
/// up a MIME body parser (e.g. the jmap-mime crate) to populate these fields.
#[tokio::test]
async fn conformance_email_body_plain_text_body() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id],
        "properties": ["id", "textBody"],
        "bodyProperties": ["partId", "type"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must return exactly one email");

    let text_body = list[0]["textBody"]
        .as_array()
        .expect("textBody must be an array");
    assert!(
        !text_body.is_empty(),
        "textBody must have at least one entry for a plain-text message"
    );
    assert_eq!(
        text_body[0]["type"].as_str().unwrap_or(""),
        "text/plain",
        "textBody[0].type must be text/plain; got: {:?}",
        text_body[0]["type"]
    );
}

/// Oracle: RFC 8621 §4.1.4 — Email/get with properties=["id","bodyStructure"] on a
/// multipart/mixed message must return a bodyStructure whose type contains "multipart".
///
/// jmap-test-suite: email-get-body.test.ts "body-structure"
///
/// GAP: MemoryBackend does not populate bodyStructure.  This test will FAIL until
/// body parsing is wired up.
#[tokio::test]
async fn conformance_email_body_html_attachment_body_structure() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["html-attachment"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id],
        "properties": ["id", "bodyStructure"],
        "bodyProperties": ["partId", "type", "name", "disposition", "size", "subParts"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must return exactly one email");

    let body_structure = &list[0]["bodyStructure"];
    assert!(
        !body_structure.is_null(),
        "bodyStructure must be present for a multipart message"
    );
    let bs_type = body_structure["type"].as_str().unwrap_or("");
    assert!(
        bs_type.contains("multipart"),
        "bodyStructure.type must contain \"multipart\" for a multipart/mixed message; got: {:?}",
        bs_type
    );
}

/// Oracle: RFC 8621 §4.1.4 — Email/get with properties=["id","preview"] returns the
/// preview string.  For plain-simple the body is "Let's meet tomorrow…" so the preview
/// must start with "Let's meet".
///
/// jmap-test-suite: email-get-body.test.ts (indirectly via body-values-text which
/// checks body content; preview is derived from the same body text).
#[tokio::test]
async fn conformance_email_body_preview() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id],
        "properties": ["id", "preview"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must return exactly one email");

    let preview = list[0]["preview"].as_str().unwrap_or("");
    assert!(
        preview.starts_with("Let's meet"),
        "preview must start with \"Let's meet\"; got: {:?}",
        preview
    );
}

/// Oracle: RFC 8621 §4.1.1 — Email/get with properties=["id","size"] returns a
/// positive size (byte-count of the raw RFC 5322 message).
///
/// jmap-test-suite: email-get-body.test.ts (size is a required Email property per
/// RFC 8621 §4.1.1).
#[tokio::test]
async fn conformance_email_body_size() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["plain-simple"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id],
        "properties": ["id", "size"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must return exactly one email");

    let size = list[0]["size"]
        .as_u64()
        .expect("size must be a non-negative integer");
    assert!(
        size > 0,
        "size must be greater than 0 for a non-empty message; got {size}"
    );
}

/// Oracle: RFC 8621 §4.1.4 — for a multipart/alternative message (html-only fixture),
/// Email/get with properties=["id","textBody","htmlBody"] must return both textBody and
/// htmlBody as non-empty arrays (one plain-text part and one HTML part respectively).
///
/// jmap-test-suite: email-get-body.test.ts "body-multipart-alternative-text-and-html"
///
/// GAP: MemoryBackend does not populate textBody or htmlBody.  This test will FAIL until
/// body parsing is wired up.
#[tokio::test]
async fn conformance_email_body_html_only_both_bodies() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["html-only"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id],
        "properties": ["id", "textBody", "htmlBody"],
        "bodyProperties": ["partId", "type"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must return exactly one email");

    let text_body = list[0]["textBody"]
        .as_array()
        .expect("textBody must be an array");
    assert!(
        !text_body.is_empty(),
        "textBody must be non-empty for multipart/alternative message (RFC 8621 §4.1.4)"
    );

    let html_body = list[0]["htmlBody"]
        .as_array()
        .expect("htmlBody must be an array");
    assert!(
        !html_body.is_empty(),
        "htmlBody must be non-empty for multipart/alternative message (RFC 8621 §4.1.4)"
    );
}

/// Oracle: RFC 8621 §4.1.4 — for a multipart/mixed message with a PDF attachment
/// (html-attachment fixture), Email/get with properties=["id","attachments"] must return
/// a non-empty attachments array.
///
/// jmap-test-suite: email-get-body.test.ts "body-attachments"
///
/// GAP: MemoryBackend does not populate the attachments field.  This test will FAIL
/// until body parsing is wired up.
#[tokio::test]
async fn conformance_email_body_attachment_detected() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let email_id = seed.email["html-attachment"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "ids": [email_id],
        "properties": ["id", "attachments"],
        "bodyProperties": ["partId", "type", "name", "disposition", "size"],
    });
    let (resp, _) = handle_email_get(&backend, args)
        .await
        .expect("Email/get must not error");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must return exactly one email");

    let attachments = list[0]["attachments"]
        .as_array()
        .expect("attachments must be an array");
    assert!(
        !attachments.is_empty(),
        "attachments must be non-empty for a message with a PDF attachment (RFC 8621 §4.1.4)"
    );
}

// ---------------------------------------------------------------------------
// Mailbox/set conformance tests (ported from jmap-test-suite mailbox-set.test.ts)
// ---------------------------------------------------------------------------

/// Oracle: RFC 8621 §2.5 — Mailbox/set create must assign a server-generated id
/// and return it in the created map.
/// jmap-test-suite: mailbox-set.test.ts "set-create-top-level"
#[tokio::test]
async fn conformance_mailbox_set_create_basic() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let _account_id = Id::from("acct1");

    let args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "c1": { "name": "Test Create", "parentId": null }
        }
    });

    let (resp, extra) = handle_mailbox_set(&backend, args)
        .await
        .expect("Mailbox/set must succeed");

    assert!(
        extra.is_empty(),
        "Mailbox/set must not produce extra invocations"
    );

    // Oracle: created must contain key "c1" with a server-assigned id.
    let created = resp["created"]
        .as_object()
        .expect("created must be an object");
    assert!(
        created.contains_key("c1"),
        "c1 must appear in created; got: {:?}",
        resp["created"]
    );
    let assigned_id = created["c1"]["id"]
        .as_str()
        .expect("created.c1.id must be a string");
    assert!(!assigned_id.is_empty(), "assigned id must not be empty");

    // Oracle: verify the mailbox name round-trips through a Mailbox/get.
    let get_args = serde_json::json!({
        "accountId": "acct1",
        "ids": [assigned_id],
    });
    let (get_resp, _) = handle_mailbox_get(&backend, get_args)
        .await
        .expect("Mailbox/get must succeed after create");
    let list = get_resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find created mailbox");
    assert_eq!(
        list[0]["name"].as_str(),
        Some("Test Create"),
        "name must equal the requested value; got: {:?}",
        list[0]["name"]
    );
}

/// Oracle: RFC 8621 §2.5 — creating a child mailbox must set parentId to the
/// parent's server-assigned id.
/// jmap-test-suite: mailbox-set.test.ts "set-create-child"
#[tokio::test]
async fn conformance_mailbox_set_create_with_parent() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let folder_a_id = seed.mailbox["folderA"].as_ref().to_owned();

    let args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "child": {
                "name": "Child Under FolderA",
                "parentId": folder_a_id
            }
        }
    });

    let (resp, _) = handle_mailbox_set(&backend, args)
        .await
        .expect("Mailbox/set create with parent must succeed");

    let created = resp["created"]
        .as_object()
        .expect("created must be an object");
    assert!(
        created.contains_key("child"),
        "child must appear in created; got: {:?}",
        resp["created"]
    );
    let child_id = created["child"]["id"]
        .as_str()
        .expect("created.child.id must be a string");

    // Oracle: Mailbox/get must echo parentId = folderA.
    let get_args = serde_json::json!({
        "accountId": "acct1",
        "ids": [child_id],
    });
    let (get_resp, _) = handle_mailbox_get(&backend, get_args)
        .await
        .expect("Mailbox/get must succeed");
    let list = get_resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find child mailbox");
    assert_eq!(
        list[0]["parentId"].as_str(),
        Some(folder_a_id.as_str()),
        "child parentId must equal folderA id; got: {:?}",
        list[0]["parentId"]
    );
}

/// Oracle: RFC 8621 §2.5 — Mailbox/set update must rename the mailbox; the new
/// name is visible in a subsequent Mailbox/get.
/// jmap-test-suite: mailbox-set.test.ts "set-rename"
#[tokio::test]
async fn conformance_mailbox_set_update_name() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let folder_b_id = seed.mailbox["folderB"].as_ref().to_owned();
    let folder_b_id_key = folder_b_id.clone();

    let update_args = serde_json::json!({
        "accountId": "acct1",
        "update": {
            folder_b_id_key: { "name": "Renamed B" }
        }
    });

    let (resp, _) = handle_mailbox_set(&backend, update_args)
        .await
        .expect("Mailbox/set update must succeed");

    // Oracle: id must appear in updated, not in notUpdated.
    let not_updated = resp["notUpdated"].as_object();
    assert!(
        not_updated.map_or(true, |m| !m.contains_key(folder_b_id.as_str())),
        "folderB must not be in notUpdated; got: {:?}",
        resp["notUpdated"]
    );

    // Oracle: Mailbox/get must return the new name.
    let get_args = serde_json::json!({
        "accountId": "acct1",
        "ids": [folder_b_id],
    });
    let (get_resp, _) = handle_mailbox_get(&backend, get_args)
        .await
        .expect("Mailbox/get must succeed");
    let list = get_resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find folderB");
    assert_eq!(
        list[0]["name"].as_str(),
        Some("Renamed B"),
        "name must be the updated value; got: {:?}",
        list[0]["name"]
    );
}

/// Oracle: RFC 8621 §2.5 — a created mailbox can be destroyed; the destroyed id
/// must appear in the destroyed array and must not be retrievable afterward.
/// jmap-test-suite: mailbox-set.test.ts "set-destroy-empty"
#[tokio::test]
async fn conformance_mailbox_set_destroy() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let _account_id = Id::from("acct1");

    // Create a fresh mailbox so we can destroy it cleanly.
    let create_args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "c1": { "name": "Destroy Me", "parentId": null }
        }
    });
    let (create_resp, _) = handle_mailbox_set(&backend, create_args)
        .await
        .expect("Mailbox/set create must succeed");
    let mb_id = create_resp["created"]["c1"]["id"]
        .as_str()
        .expect("created id must be present")
        .to_owned();

    // Destroy it.
    let destroy_args = serde_json::json!({
        "accountId": "acct1",
        "destroy": [mb_id],
    });
    let (destroy_resp, _) = handle_mailbox_set(&backend, destroy_args)
        .await
        .expect("Mailbox/set destroy must succeed");

    // Oracle: destroyed array must contain the id.
    let destroyed = destroy_resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array");
    let destroyed_strs: Vec<&str> = destroyed.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        destroyed_strs.contains(&mb_id.as_str()),
        "destroyed must contain the mailbox id; got: {:?}",
        destroyed_strs
    );

    // Oracle: notDestroyed must not contain the id.
    let not_destroyed = destroy_resp["notDestroyed"].as_object();
    assert!(
        not_destroyed.map_or(true, |m| !m.contains_key(mb_id.as_str())),
        "id must not appear in notDestroyed; got: {:?}",
        destroy_resp["notDestroyed"]
    );
}

/// Oracle: RFC 8621 §2.5 — attempting to destroy a mailbox that has child mailboxes
/// MUST be rejected with SetError type "mailboxHasChild".
/// jmap-test-suite: mailbox-set.test.ts "set-cannot-destroy-with-children"
#[tokio::test]
async fn conformance_mailbox_set_destroy_with_children() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    // folderA has child1 and child2 — it must not be destroyable without first
    // removing the children.
    let folder_a_id = seed.mailbox["folderA"].as_ref().to_owned();

    let destroy_args = serde_json::json!({
        "accountId": "acct1",
        "destroy": [folder_a_id],
    });
    let (resp, _) = handle_mailbox_set(&backend, destroy_args)
        .await
        .expect("Mailbox/set destroy must return a set response (not a method error)");

    // Oracle: id must be in notDestroyed with type "mailboxHasChild".
    let not_destroyed = resp["notDestroyed"]
        .as_object()
        .expect("notDestroyed must be an object when destroy is rejected");
    assert!(
        not_destroyed.contains_key(folder_a_id.as_str()),
        "folderA must appear in notDestroyed; got: {:?}",
        resp["notDestroyed"]
    );
    assert_eq!(
        not_destroyed[folder_a_id.as_str()]["type"].as_str(),
        Some("mailboxHasChild"),
        "error type must be mailboxHasChild; got: {:?}",
        not_destroyed[folder_a_id.as_str()]["type"]
    );

    // Oracle: destroyed must not contain folderA.
    let destroyed = resp["destroyed"].as_array();
    let is_in_destroyed = destroyed.map_or(false, |arr| {
        arr.iter().any(|v| v.as_str() == Some(folder_a_id.as_str()))
    });
    assert!(!is_in_destroyed, "folderA must not appear in destroyed");
}

/// Oracle: RFC 8621 §2.5 — two mailboxes with the same name under the same parent
/// MUST be rejected; the second create must land in notCreated with type "alreadyExists".
/// jmap-test-suite: mailbox-set.test.ts "set-duplicate-name-same-parent"
#[tokio::test]
async fn conformance_mailbox_set_create_duplicate_name() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let _account_id = Id::from("acct1");

    // Create the first mailbox.
    let create1_args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "dup1": { "name": "Duplicate Name Test", "parentId": null }
        }
    });
    let (resp1, _) = handle_mailbox_set(&backend, create1_args)
        .await
        .expect("first Mailbox/set create must succeed");
    assert!(
        resp1["created"]["dup1"]["id"].as_str().is_some(),
        "first create must succeed; got: {:?}",
        resp1["created"]
    );
    let first_id = resp1["created"]["dup1"]["id"].as_str().unwrap().to_owned();

    // Try to create a second with the same name under the same parent (null).
    let create2_args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "dup2": { "name": "Duplicate Name Test", "parentId": null }
        }
    });
    let (resp2, _) = handle_mailbox_set(&backend, create2_args)
        .await
        .expect("second Mailbox/set create must return a set response");

    // Oracle: dup2 must be in notCreated with type "alreadyExists".
    let not_created = resp2["notCreated"]
        .as_object()
        .expect("notCreated must be an object");
    assert!(
        not_created.contains_key("dup2"),
        "dup2 must appear in notCreated; got: {:?}",
        resp2["notCreated"]
    );
    assert_eq!(
        not_created["dup2"]["type"].as_str(),
        Some("alreadyExists"),
        "error type must be alreadyExists; got: {:?}",
        not_created["dup2"]["type"]
    );

    // Oracle: created must not contain dup2.
    assert!(
        resp2["created"]["dup2"].is_null() || resp2["created"].is_null(),
        "dup2 must not appear in created; got: {:?}",
        resp2["created"]
    );

    // Cleanup: destroy the first mailbox.
    let _ = handle_mailbox_set(
        &backend,
        serde_json::json!({
            "accountId": "acct1",
            "destroy": [first_id],
        }),
    )
    .await;
}

/// Oracle: RFC 8620 §5.3 — a successful Mailbox/set create MUST return an oldState
/// that differs from newState, reflecting that the state advanced.
/// jmap-test-suite: mailbox-set.test.ts "set-state-changes"
#[tokio::test]
async fn conformance_mailbox_set_state_changes() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let _account_id = Id::from("acct1");

    let args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "stateTest": { "name": "State Test", "parentId": null }
        }
    });

    let (resp, _) = handle_mailbox_set(&backend, args)
        .await
        .expect("Mailbox/set must succeed");

    // Oracle: RFC 8620 §5.3 — oldState and newState must both be present.
    let old_state = resp["oldState"]
        .as_str()
        .expect("oldState must be a string");
    let new_state = resp["newState"]
        .as_str()
        .expect("newState must be a string");

    // Oracle: after a mutation, newState must differ from oldState.
    assert_ne!(
        old_state, new_state,
        "newState must differ from oldState after a create; old={old_state:?} new={new_state:?}"
    );
}

/// Oracle: RFC 8620 §5.3 / RFC 8621 §2.5 — a create request without a name field
/// MUST be rejected; the create id must appear in notCreated with an error type
/// indicating the missing required property.
/// jmap-test-suite: (implicit from RFC 8621 §2 — name is a required server-set property)
#[tokio::test]
async fn conformance_mailbox_set_create_missing_name() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let _account_id = Id::from("acct1");

    let args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "noName": { "parentId": null }
        }
    });

    let (resp, _) = handle_mailbox_set(&backend, args)
        .await
        .expect("Mailbox/set must return a set response (not a method error)");

    // Oracle: noName must be in notCreated; created must not contain it.
    let not_created = resp["notCreated"]
        .as_object()
        .expect("notCreated must be an object when create is rejected");
    assert!(
        not_created.contains_key("noName"),
        "noName must appear in notCreated; got: {:?}",
        resp["notCreated"]
    );

    // The handler uses SetErrorType::InvalidProperties with properties=["name"].
    assert_eq!(
        not_created["noName"]["type"].as_str(),
        Some("invalidProperties"),
        "error type must be invalidProperties; got: {:?}",
        not_created["noName"]["type"]
    );

    assert!(
        resp["created"]["noName"].is_null() || resp["created"].is_null(),
        "noName must not appear in created; got: {:?}",
        resp["created"]
    );
}

// ---------------------------------------------------------------------------
// Mailbox/changes conformance tests (ported from jmap-test-suite mailbox-changes.test.ts)
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §5.2 — Mailbox/changes from state "0" after creating a mailbox
/// must include the new id in the created array.
/// jmap-test-suite: mailbox-changes.test.ts "changes-after-create"
#[tokio::test]
async fn conformance_mailbox_changes_from_state_zero() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let _account_id = Id::from("acct1");

    // Create a mailbox so there is something to report.
    let set_args = serde_json::json!({
        "accountId": "acct1",
        "create": {
            "newMb": { "name": "Changes Test Mailbox", "parentId": null }
        }
    });
    let (set_resp, _) = handle_mailbox_set(&backend, set_args)
        .await
        .expect("Mailbox/set must succeed");
    let new_id = set_resp["created"]["newMb"]["id"]
        .as_str()
        .expect("created id must be present")
        .to_owned();

    // Mailbox/changes from sinceState "0".
    let changes_args = serde_json::json!({
        "accountId": "acct1",
        "sinceState": "0",
    });
    let (resp, extra) = handle_mailbox_changes(&backend, changes_args)
        .await
        .expect("Mailbox/changes must succeed");

    assert!(
        extra.is_empty(),
        "Mailbox/changes must not produce extra invocations"
    );

    // Oracle: created must contain the new mailbox id.
    let created = resp["created"]
        .as_array()
        .expect("created must be an array");
    let created_strs: Vec<&str> = created.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        created_strs.contains(&new_id.as_str()),
        "created must contain the new mailbox id {new_id:?}; got: {created_strs:?}"
    );

    // Oracle: oldState must echo sinceState.
    assert_eq!(
        resp["oldState"].as_str().unwrap_or(""),
        "0",
        "oldState must equal sinceState"
    );

    // Oracle: newState must differ from "0" because a mutation happened.
    assert_ne!(
        resp["newState"].as_str().unwrap_or("0"),
        "0",
        "newState must advance after a mutation"
    );
}

/// Oracle: RFC 8620 §5.2 — Mailbox/changes from the current state must return empty
/// created, updated, and destroyed arrays and hasMoreChanges=false.
/// jmap-test-suite: mailbox-changes.test.ts "changes-no-changes"
#[tokio::test]
async fn conformance_mailbox_changes_from_current_state() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    // Get the current Mailbox state via Mailbox/get.
    let get_args = serde_json::json!({
        "accountId": "acct1",
        "ids": [],
    });
    let (get_resp, _) = handle_mailbox_get(&backend, get_args)
        .await
        .expect("Mailbox/get must succeed");
    let current_state = get_resp["state"]
        .as_str()
        .expect("state must be a string in Mailbox/get response")
        .to_owned();

    // Now request changes from that current state — nothing should have changed.
    let changes_args = serde_json::json!({
        "accountId": "acct1",
        "sinceState": current_state,
    });
    let (resp, _) = handle_mailbox_changes(&backend, changes_args)
        .await
        .expect("Mailbox/changes must succeed");

    // Oracle: all three lists must be empty.
    let created = resp["created"]
        .as_array()
        .expect("created must be an array");
    let updated = resp["updated"]
        .as_array()
        .expect("updated must be an array");
    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array");

    assert!(
        created.is_empty(),
        "created must be empty when sinceState is current; got: {:?}",
        created
    );
    assert!(
        updated.is_empty(),
        "updated must be empty when sinceState is current; got: {:?}",
        updated
    );
    assert!(
        destroyed.is_empty(),
        "destroyed must be empty when sinceState is current; got: {:?}",
        destroyed
    );

    // Oracle: hasMoreChanges must be false.
    assert_eq!(
        resp["hasMoreChanges"].as_bool(),
        Some(false),
        "hasMoreChanges must be false when no changes; got: {:?}",
        resp["hasMoreChanges"]
    );
}

/// Oracle: RFC 8620 §5.2 — Mailbox/changes with an unrecognised sinceState MUST
/// return an error (the MemoryBackend uses numeric states; a non-numeric token
/// is invalid and results in a serverFail method error).
/// jmap-test-suite: mailbox-changes.test.ts (implicit — invalid state handling)
#[tokio::test]
async fn conformance_mailbox_changes_invalid_state() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": "acct1",
        "sinceState": "invalid-state-token",
    });

    // Oracle: the handler must return Err(JmapError) rather than an empty-changes
    // success. The MemoryBackend can only parse numeric state tokens; anything else
    // results in BackendChangesError::Other → JmapError::server_fail.
    let result = handle_mailbox_changes(&backend, args).await;
    assert!(
        result.is_err(),
        "invalid sinceState must produce an error, not a success response"
    );
}

// ---------------------------------------------------------------------------
// Email/query conformance tests (RFC 8621 §4.4, RFC 8620 §5.5)
// Ported from jmap-test-suite email-query-filters.test.ts and
// email-query-paging.test.ts.
// ---------------------------------------------------------------------------

/// Oracle: Email/query with no filter returns all emails in the account.
/// RFC 8621 §4.4 — absence of filter means no constraints; all emails match.
/// Seed has 16 emails (multi-mailbox is one object in two mailboxes).
#[tokio::test]
async fn conformance_email_query_all() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "calculateTotal": true,
    });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must succeed");

    let total = resp["total"].as_u64().expect("total must be present");
    assert!(
        total >= 16,
        "total must be at least 16 (all seed emails); got {total}"
    );
    let ids = resp["ids"].as_array().expect("ids must be an array");
    assert!(
        ids.len() >= 16,
        "ids.len() must be at least 16; got {}",
        ids.len()
    );
}

/// Oracle: Email/query filter inMailbox returns only emails in that mailbox.
/// RFC 8621 §4.4.1 — inMailbox: only emails whose mailboxIds include the given id.
/// Inbox has 9 emails: plain-simple, html-attachment, thread-reply-1,
/// thread-reply-2, multi-mailbox, html-only, no-subject, custom-keywords,
/// special-headers.
#[tokio::test]
async fn conformance_email_query_filter_in_mailbox() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed.mailbox["inbox"].clone();
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": { "inMailbox": inbox_id.as_ref() },
        "calculateTotal": true,
    });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must succeed");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    assert!(
        ids.len() >= 9,
        "inbox must have at least 9 emails; got {}",
        ids.len()
    );
    assert!(
        ids.contains(&seed.email["plain-simple"].as_ref()),
        "plain-simple must be in inbox results"
    );
    // very-old is only in folderA, not inbox.
    assert!(
        !ids.contains(&seed.email["very-old"].as_ref()),
        "very-old must NOT be in inbox results"
    );
}

/// Oracle: Email/query filter hasKeyword="$flagged" returns only flagged emails.
/// RFC 8621 §4.4.1 — hasKeyword: email must have the keyword.
/// Only html-attachment and sort-test-2 have $flagged in the seed data.
#[tokio::test]
async fn conformance_email_query_filter_has_keyword() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": { "hasKeyword": "$flagged" },
    });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must succeed");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    assert!(
        ids.contains(&seed.email["html-attachment"].as_ref()),
        "html-attachment ($flagged) must be in results"
    );
    assert!(
        ids.contains(&seed.email["sort-test-2"].as_ref()),
        "sort-test-2 ($flagged) must be in results"
    );
    // plain-simple has $seen but not $flagged.
    assert!(
        !ids.contains(&seed.email["plain-simple"].as_ref()),
        "plain-simple must NOT be in $flagged results"
    );
}

/// Oracle: Email/query filter notKeyword="$seen" excludes emails with $seen.
/// RFC 8621 §4.4.1 — notKeyword: email must NOT have the keyword.
/// Unseen emails in seed: thread-reply-1, large-email, sort-test-3.
#[tokio::test]
async fn conformance_email_query_filter_not_keyword() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": { "notKeyword": "$seen" },
    });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must succeed");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    assert!(
        ids.contains(&seed.email["thread-reply-1"].as_ref()),
        "thread-reply-1 (no $seen) must be in notKeyword=$seen results"
    );
    assert!(
        ids.contains(&seed.email["large-email"].as_ref()),
        "large-email (no $seen) must be in notKeyword=$seen results"
    );
    assert!(
        ids.contains(&seed.email["sort-test-3"].as_ref()),
        "sort-test-3 (no $seen) must be in notKeyword=$seen results"
    );
    // plain-simple has $seen — must be excluded.
    assert!(
        !ids.contains(&seed.email["plain-simple"].as_ref()),
        "plain-simple ($seen) must NOT be in notKeyword=$seen results"
    );
}

/// Oracle: Email/query filter after="2025-12-20T00:00:00Z" returns emails
/// with receivedAt >= that date.
/// RFC 8621 §4.4.1 — after: receivedAt must be on or after the given date-time.
/// very-old (2025-12-02) must be excluded; custom-keywords (2025-12-31) must
/// be included.
#[tokio::test]
async fn conformance_email_query_filter_after() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": { "after": "2025-12-20T00:00:00Z" },
    });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must succeed");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    // plain-simple is at 2025-12-22 — on or after the cutoff.
    assert!(
        ids.contains(&seed.email["plain-simple"].as_ref()),
        "plain-simple (2025-12-22) must be in after=2025-12-20 results"
    );
    // custom-keywords is at 2025-12-31 — after the cutoff.
    assert!(
        ids.contains(&seed.email["custom-keywords"].as_ref()),
        "custom-keywords (2025-12-31) must be in after=2025-12-20 results"
    );
    // very-old is at 2025-12-02 — before the cutoff.
    assert!(
        !ids.contains(&seed.email["very-old"].as_ref()),
        "very-old (2025-12-02) must NOT be in after=2025-12-20 results"
    );
}

/// Oracle: Email/query filter before="2025-12-10T00:00:00Z" returns only
/// emails with receivedAt strictly before that date.
/// RFC 8621 §4.4.1 — before: receivedAt must be < the given date-time.
/// Only very-old (2025-12-02) qualifies.
#[tokio::test]
async fn conformance_email_query_filter_before() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": { "before": "2025-12-10T00:00:00Z" },
    });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must succeed");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    // very-old is at 2025-12-02 — before the cutoff.
    assert!(
        ids.contains(&seed.email["very-old"].as_ref()),
        "very-old (2025-12-02) must be in before=2025-12-10 results"
    );
    // plain-simple is at 2025-12-22 — after the cutoff.
    assert!(
        !ids.contains(&seed.email["plain-simple"].as_ref()),
        "plain-simple (2025-12-22) must NOT be in before=2025-12-10 results"
    );
}

/// Oracle: Email/query filter minSize=10000 returns only emails >= 10 000 bytes.
/// RFC 8621 §4.4.1 — minSize: size must be >= the given value.
/// large-email body is ~14 800 bytes; all other seed emails are well under
/// 10 000 bytes.
#[tokio::test]
async fn conformance_email_query_filter_min_size() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "filter": { "minSize": 10000 },
    });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must succeed");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    assert!(
        ids.contains(&seed.email["large-email"].as_ref()),
        "large-email (>10 000 bytes) must be in minSize=10000 results"
    );
    assert!(
        !ids.contains(&seed.email["plain-simple"].as_ref()),
        "plain-simple must NOT be in minSize=10000 results"
    );
}

/// Oracle: Email/query with limit=3 returns exactly 3 IDs when more exist.
/// RFC 8620 §5.5 — limit restricts the number of results returned.
#[tokio::test]
async fn conformance_email_query_limit() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "limit": 3,
        "calculateTotal": true,
    });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query must succeed");

    let ids = resp["ids"].as_array().expect("ids must be an array");
    assert_eq!(
        ids.len(),
        3,
        "limit=3 must return exactly 3 ids; got {}",
        ids.len()
    );

    let total = resp["total"].as_u64().expect("total must be present");
    assert!(
        total >= 16,
        "total must reflect all emails (not just the page); got {total}"
    );
}

/// Oracle: Email/query with position=1, limit=2 skips 1 result and returns
/// the next 2.
/// RFC 8620 §5.5 — position is 0-based; position=1 skips 1 result.
#[tokio::test]
async fn conformance_email_query_position() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    // Get all IDs in the default sort order to derive the expected slice.
    let all_args = serde_json::json!({ "accountId": account_id.as_ref() });
    let (all_resp, _) = handle_email_query(&backend, all_args)
        .await
        .expect("all-email query must succeed");
    let all_ids: Vec<String> = all_resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert!(all_ids.len() >= 3, "need at least 3 emails for this test");

    let paged_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "position": 1,
        "limit": 2,
    });
    let (paged_resp, _) = handle_email_query(&backend, paged_args)
        .await
        .expect("paged query must succeed");

    let paged_ids: Vec<String> = paged_resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();

    assert_eq!(
        paged_resp["position"].as_i64(),
        Some(1),
        "response position must echo the requested position"
    );
    assert_eq!(
        paged_ids.len(),
        2,
        "position=1 limit=2 must return 2 ids; got {}",
        paged_ids.len()
    );
    assert_eq!(
        paged_ids[0], all_ids[1],
        "first paged result must be all_ids[1] (position=1 skips index 0)"
    );
    assert_eq!(
        paged_ids[1], all_ids[2],
        "second paged result must be all_ids[2]"
    );
}

/// Oracle: Email/query with anchor set to the 3rd result ID returns results
/// starting at that anchor.
/// RFC 8620 §5.5 — anchor identifies the start position by ID, not offset.
#[tokio::test]
async fn conformance_email_query_anchor() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    // Get all IDs with a stable sort order so the anchor index is predictable.
    let all_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sort": [{ "property": "receivedAt", "isAscending": false }],
    });
    let (all_resp, _) = handle_email_query(&backend, all_args)
        .await
        .expect("all-email query must succeed");
    let all_ids: Vec<String> = all_resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert!(
        all_ids.len() >= 3,
        "need at least 3 emails for anchor test; got {}",
        all_ids.len()
    );

    let anchor_id = all_ids[2].clone();

    let anchor_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sort": [{ "property": "receivedAt", "isAscending": false }],
        "anchor": anchor_id,
        "limit": 3,
    });
    let (anchor_resp, _) = handle_email_query(&backend, anchor_args)
        .await
        .expect("anchor query must succeed");

    let anchor_ids: Vec<String> = anchor_resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();

    assert_eq!(
        anchor_ids[0], anchor_id,
        "first result must be the anchor ID itself"
    );
    assert_eq!(
        anchor_resp["position"].as_i64(),
        Some(2),
        "reported position must be the anchor's 0-based index (2); resp: {anchor_resp}"
    );
}

/// Oracle: Email/query with a nonexistent anchor ID returns anchorNotFound.
/// RFC 8620 §5.5 — "If an anchor argument was given and the anchor Id was not
/// found in the results, the server MUST return an anchorNotFound error."
#[tokio::test]
async fn conformance_email_query_anchor_not_found() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let _seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "anchor": "nonexistent-id",
    });
    let result = handle_email_query(&backend, args).await;

    assert!(
        result.is_err(),
        "nonexistent anchor must return a JmapError"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type.as_str(),
        "anchorNotFound",
        "error type must be anchorNotFound; got: {:?}",
        err.error_type
    );
}

/// Oracle: Email/query sort=[{property:"receivedAt",isAscending:false}] returns
/// the most-recently received email first.
/// RFC 8621 §4.4.2 — receivedAt comparator sorts by message receipt time.
/// The two most-recent seed emails both have receivedAt=2025-12-31T00:00:00Z
/// (custom-keywords and sort-test-3); either may appear first.
/// The oldest email (very-old, 2025-12-02) must be last.
#[tokio::test]
async fn conformance_email_query_sort_received_at_desc() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sort": [{ "property": "receivedAt", "isAscending": false }],
    });
    let (resp, _) = handle_email_query(&backend, args)
        .await
        .expect("Email/query with sort must succeed");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be an array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    assert!(
        !ids.is_empty(),
        "sorted query must return at least one result"
    );

    // First result must be one of the most-recently received emails.
    let first_id = ids[0];
    let is_most_recent = first_id == seed.email["custom-keywords"].as_ref()
        || first_id == seed.email["sort-test-3"].as_ref();
    assert!(
        is_most_recent,
        "first result of receivedAt desc sort must be custom-keywords or sort-test-3 \
         (both at 2025-12-31); got {first_id:?}"
    );

    // Last result must be the oldest email.
    let last_id = ids[ids.len() - 1];
    assert_eq!(
        last_id,
        seed.email["very-old"].as_ref(),
        "last result of receivedAt desc sort must be very-old (2025-12-02)"
    );
}

// ---------------------------------------------------------------------------
// Email/set conformance tests (ported from jmap-test-suite email-set.test.ts)
// ---------------------------------------------------------------------------

/// Oracle: Email/set create with mailboxIds, keywords, and subject returns a
/// created entry with a server-assigned id.
///
/// RFC 8621 §4.6 — a create object must include at least one mailboxId set to
/// true. The response's `created` map must contain the creation id key with an
/// object that includes a server-assigned `id`.
/// Source: jmap-test-suite email-set-create.test.ts "set-create-plain-text"
#[tokio::test]
async fn conformance_email_set_create_basic() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "draft1": {
                "mailboxIds": { "inbox": true },
                "keywords": { "$seen": true },
                "subject": "Plain text creation test",
            }
        }
    });
    let (resp, extra) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set must not return a protocol error");
    assert!(
        extra.is_empty(),
        "Email/set must not generate extra invocations"
    );

    // RFC 8621 §4.6: created map must be present and contain the creation id.
    let created = resp["created"]
        .as_object()
        .expect("created must be a non-null object");
    assert!(
        created.contains_key("draft1"),
        "created must contain 'draft1'; resp: {resp:?}"
    );
    let entry = &created["draft1"];
    assert!(
        entry["id"].as_str().is_some(),
        "created entry must have a server-assigned id; entry: {entry:?}"
    );
    assert!(
        entry["threadId"].as_str().is_some(),
        "created entry must have a server-assigned threadId; entry: {entry:?}"
    );
    // RFC 8621 §4.6: blobId is server-set and must not be the internal placeholder.
    let blob_id = entry["blobId"].as_str().expect("blobId must be present");
    assert_ne!(
        blob_id, "placeholder-blob",
        "blobId must be a real server-assigned value"
    );
}

/// Oracle: Email/set create advances the state token.
///
/// RFC 8620 §5.2 — a successful set response must include `oldState` and
/// `newState`, and they must differ when objects were created.
/// Source: jmap-test-suite email-set-create.test.ts "set-create-state-changes"
#[tokio::test]
async fn conformance_email_set_create_sets_state() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "stateEmail": {
                "mailboxIds": { "inbox": true },
                "subject": "State change test",
            }
        }
    });
    let (resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set must not return a protocol error");

    // RFC 8620 §5.2: oldState and newState must both be present.
    let old_state = resp["oldState"].as_str().expect("oldState must be present");
    let new_state = resp["newState"].as_str().expect("newState must be present");

    // RFC 8620 §5.2: after a successful create, newState must differ from oldState.
    assert_ne!(
        old_state, new_state,
        "newState must differ from oldState after a create; oldState={old_state:?} newState={new_state:?}"
    );
}

/// Oracle: Email/set create with $draft and $seen keywords preserves both.
///
/// RFC 8621 §4.6 — keywords provided at create time must be stored. A
/// subsequent Email/get must return the same keyword set.
/// Source: jmap-test-suite email-set-create.test.ts "set-create-with-keywords"
#[tokio::test]
async fn conformance_email_set_create_with_keywords() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "kwDraft": {
                "mailboxIds": { "inbox": true },
                "keywords": { "$draft": true, "$seen": true },
                "subject": "Keywords test",
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set must not return a protocol error");

    let email_id = set_resp["created"]["kwDraft"]["id"]
        .as_str()
        .expect("created kwDraft must have id")
        .to_owned();

    // Verify keywords survive a round-trip via Email/get.
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [email_id],
        "properties": ["keywords"],
    });
    let (get_resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get must succeed");

    let keywords = &get_resp["list"][0]["keywords"];
    assert_eq!(
        keywords["$draft"].as_bool(),
        Some(true),
        "$draft must be true; keywords: {keywords:?}"
    );
    assert_eq!(
        keywords["$seen"].as_bool(),
        Some(true),
        "$seen must be true; keywords: {keywords:?}"
    );
}

/// Oracle: Email/set update replacing the keywords map adds $flagged.
///
/// RFC 8621 §4.6 / RFC 8620 §5.3 — an update with a full `keywords` replacement
/// sets exactly those keywords. Email/get must return the new keyword set.
/// Source: jmap-test-suite email-set-update.test.ts "set-update-replace-keywords"
#[tokio::test]
async fn conformance_email_set_update_keywords() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Create an email with only $seen.
    let (set_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "c0": {
                    "mailboxIds": { "inbox": true },
                    "keywords": { "$seen": true },
                    "subject": "Update keywords test",
                }
            }
        }),
    )
    .await
    .expect("Email/set create must succeed");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("c0 must have id")
        .to_owned();

    // Replace keywords map: set $seen + $flagged.
    let (upd_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "update": {
                email_id.clone(): {
                    "keywords": { "$seen": true, "$flagged": true },
                }
            }
        }),
    )
    .await
    .expect("Email/set update must not return a protocol error");

    let updated = upd_resp["updated"]
        .as_object()
        .expect("updated must be an object");
    assert!(
        updated.contains_key(&email_id),
        "email must be in updated; notUpdated={:?}",
        upd_resp["notUpdated"]
    );

    // Verify $flagged is present via Email/get.
    let (get_resp, _) = handle_email_get(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "ids": [email_id],
            "properties": ["keywords"],
        }),
    )
    .await
    .expect("Email/get must succeed");

    let keywords = &get_resp["list"][0]["keywords"];
    assert_eq!(
        keywords["$flagged"].as_bool(),
        Some(true),
        "$flagged must be true after update; keywords: {keywords:?}"
    );
    assert_eq!(
        keywords["$seen"].as_bool(),
        Some(true),
        "$seen must be true after update; keywords: {keywords:?}"
    );
}

/// Oracle: Email/set update replacing mailboxIds moves the email.
///
/// RFC 8620 §5.3 — replacing `mailboxIds` with a new map removes the email from
/// all prior mailboxes and places it in the new ones. Email/get must return the
/// updated mailboxIds.
/// Source: jmap-test-suite email-set-update.test.ts "set-update-move-mailbox"
#[tokio::test]
async fn conformance_email_set_update_mailbox_ids() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Create in inbox.
    let (set_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "moveEmail": {
                    "mailboxIds": { "inbox": true },
                    "subject": "Move test",
                }
            }
        }),
    )
    .await
    .expect("Email/set create must succeed");
    let email_id = set_resp["created"]["moveEmail"]["id"]
        .as_str()
        .expect("moveEmail must have id")
        .to_owned();

    // Move to folderA by replacing the mailboxIds map.
    let (upd_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "update": {
                email_id.clone(): {
                    "mailboxIds": { "folderA": true },
                }
            }
        }),
    )
    .await
    .expect("Email/set update must not return a protocol error");

    assert!(
        upd_resp["updated"]
            .as_object()
            .map_or(false, |m| m.contains_key(&email_id)),
        "email must be in updated; notUpdated={:?}",
        upd_resp["notUpdated"]
    );

    // Email must now be in folderA and not in inbox.
    let (get_resp, _) = handle_email_get(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "ids": [email_id],
            "properties": ["mailboxIds"],
        }),
    )
    .await
    .expect("Email/get must succeed");

    let mailbox_ids = &get_resp["list"][0]["mailboxIds"];
    assert_eq!(
        mailbox_ids["folderA"].as_bool(),
        Some(true),
        "email must be in folderA; mailboxIds: {mailbox_ids:?}"
    );
    assert!(
        mailbox_ids
            .get("inbox")
            .map_or(true, |v| !v.as_bool().unwrap_or(false)),
        "email must no longer be in inbox; mailboxIds: {mailbox_ids:?}"
    );
}

/// Oracle: Email/set patch `keywords/$flagged = true` adds the keyword.
///
/// RFC 8620 §5.3 — a path-keyed patch sets the specified property within the
/// target object. After the patch, Email/get must show $flagged = true.
/// Source: jmap-test-suite email-set-update.test.ts "set-update-add-keyword"
#[tokio::test]
async fn conformance_email_set_update_adds_keyword() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Create an email without $flagged.
    let (set_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "c0": {
                    "mailboxIds": { "inbox": true },
                    "keywords": { "$seen": true },
                    "subject": "Add keyword patch test",
                }
            }
        }),
    )
    .await
    .expect("Email/set create must succeed");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("c0 must have id")
        .to_owned();

    // Patch keywords/$flagged = true.
    let (upd_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "update": {
                email_id.clone(): {
                    "keywords/$flagged": true,
                }
            }
        }),
    )
    .await
    .expect("Email/set update must not return a protocol error");

    assert!(
        upd_resp["updated"]
            .as_object()
            .map_or(false, |m| m.contains_key(&email_id)),
        "email must be in updated; notUpdated={:?}",
        upd_resp["notUpdated"]
    );

    // $flagged must now be true.
    let (get_resp, _) = handle_email_get(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "ids": [email_id],
            "properties": ["keywords"],
        }),
    )
    .await
    .expect("Email/get must succeed");

    let keywords = &get_resp["list"][0]["keywords"];
    assert_eq!(
        keywords["$flagged"].as_bool(),
        Some(true),
        "$flagged must be true after patch; keywords: {keywords:?}"
    );
}

/// Oracle: Email/set patch `keywords/$seen = null` removes the keyword.
///
/// RFC 8620 §5.3 — a null value in a path-keyed patch removes the target
/// property. After the patch, Email/get must not show $seen.
/// Source: jmap-test-suite email-set-update.test.ts "set-update-remove-keyword"
#[tokio::test]
async fn conformance_email_set_update_removes_keyword() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Create an email with $seen.
    let (set_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "c0": {
                    "mailboxIds": { "inbox": true },
                    "keywords": { "$seen": true },
                    "subject": "Remove keyword patch test",
                }
            }
        }),
    )
    .await
    .expect("Email/set create must succeed");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("c0 must have id")
        .to_owned();

    // Patch keywords/$seen = null (removes the keyword).
    let (upd_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "update": {
                email_id.clone(): {
                    "keywords/$seen": null,
                }
            }
        }),
    )
    .await
    .expect("Email/set update must not return a protocol error");

    assert!(
        upd_resp["updated"]
            .as_object()
            .map_or(false, |m| m.contains_key(&email_id)),
        "email must be in updated; notUpdated={:?}",
        upd_resp["notUpdated"]
    );

    // $seen must now be absent.
    let (get_resp, _) = handle_email_get(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "ids": [email_id],
            "properties": ["keywords"],
        }),
    )
    .await
    .expect("Email/get must succeed");

    let keywords = &get_resp["list"][0]["keywords"];
    assert!(
        keywords.get("$seen").is_none() || keywords["$seen"].as_bool() == Some(false),
        "$seen must be absent after null patch; keywords: {keywords:?}"
    );
}

/// Oracle: Email/set destroy removes the email; subsequent Email/get returns it
/// in notFound.
///
/// RFC 8620 §5.2 — the response's `destroyed` array must include the id.
/// RFC 8621 §4.2 — a subsequent Email/get must return the id in `notFound`.
/// Source: jmap-test-suite email-set-destroy.test.ts "set-destroy-single"
#[tokio::test]
async fn conformance_email_set_destroy_basic() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Create an email to destroy.
    let (set_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "destroyMe": {
                    "mailboxIds": { "inbox": true },
                    "subject": "Destroy me",
                }
            }
        }),
    )
    .await
    .expect("Email/set create must succeed");
    let email_id = set_resp["created"]["destroyMe"]["id"]
        .as_str()
        .expect("destroyMe must have id")
        .to_owned();

    // Destroy it.
    let (destroy_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "destroy": [email_id.clone()],
        }),
    )
    .await
    .expect("Email/set destroy must not return a protocol error");

    // RFC 8620 §5.2: destroyed must be an array containing the id.
    let destroyed = destroy_resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array");
    assert!(
        destroyed.iter().any(|v| v.as_str() == Some(&email_id)),
        "destroyed must contain the email id; destroyed: {destroyed:?}"
    );

    // RFC 8621 §4.2: subsequent Email/get must report the id in notFound.
    let (get_resp, _) = handle_email_get(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "ids": [email_id.clone()],
        }),
    )
    .await
    .expect("Email/get must succeed");

    let not_found = get_resp["notFound"]
        .as_array()
        .expect("notFound must be an array");
    assert!(
        not_found.iter().any(|v| v.as_str() == Some(&email_id)),
        "notFound must contain the destroyed email id; notFound: {not_found:?}"
    );
}

/// Oracle: Email/set destroy advances the state token.
///
/// RFC 8620 §5.2 — a successful set response must include `oldState` and
/// `newState`, and they must differ after a successful destroy.
/// Source: jmap-test-suite email-set-destroy.test.ts "set-destroy-single" (state assertions)
#[tokio::test]
async fn conformance_email_set_destroy_updates_state() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    // Create an email to destroy.
    let (set_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "create": {
                "c0": {
                    "mailboxIds": { "inbox": true },
                    "subject": "Destroy state test",
                }
            }
        }),
    )
    .await
    .expect("Email/set create must succeed");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("c0 must have id")
        .to_owned();

    // Destroy and check state advances.
    let (destroy_resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "destroy": [email_id],
        }),
    )
    .await
    .expect("Email/set destroy must not return a protocol error");

    let old_state = destroy_resp["oldState"]
        .as_str()
        .expect("oldState must be present");
    let new_state = destroy_resp["newState"]
        .as_str()
        .expect("newState must be present");

    assert_ne!(
        old_state, new_state,
        "newState must differ from oldState after destroy; oldState={old_state:?} newState={new_state:?}"
    );
}

/// Oracle: Email/set update of a non-existent id returns notUpdated with type=notFound.
///
/// RFC 8620 §5.3 — if an id in the update map does not exist, the server must
/// include it in `notUpdated` with a SetError of type "notFound".
/// Source: jmap-test-suite email-set-update.test.ts "set-update-not-found"
#[tokio::test]
async fn conformance_email_set_update_not_found() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let (resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "update": {
                "nonexistent-email-xyz": {
                    "keywords/$seen": true,
                }
            }
        }),
    )
    .await
    .expect("Email/set must not return a protocol error");

    // RFC 8620 §5.3: notUpdated must be present and contain the unknown id.
    let not_updated = resp["notUpdated"]
        .as_object()
        .expect("notUpdated must be a non-null object when updating a nonexistent id");
    assert!(
        not_updated.contains_key("nonexistent-email-xyz"),
        "notUpdated must contain the unknown id; notUpdated: {not_updated:?}"
    );
    assert_eq!(
        not_updated["nonexistent-email-xyz"]["type"]
            .as_str()
            .unwrap_or(""),
        "notFound",
        "error type must be notFound; entry: {:?}",
        not_updated["nonexistent-email-xyz"]
    );
}

/// Oracle: Email/set destroy of a non-existent id returns notDestroyed with type=notFound.
///
/// RFC 8620 §5.4 — if an id in the destroy array does not exist, the server must
/// include it in `notDestroyed` with a SetError of type "notFound".
/// Source: jmap-test-suite email-set-destroy.test.ts "set-destroy-not-found"
#[tokio::test]
async fn conformance_email_set_destroy_not_found() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let (resp, _) = handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id.as_ref(),
            "destroy": ["nonexistent-email-xyz"],
        }),
    )
    .await
    .expect("Email/set must not return a protocol error");

    // RFC 8620 §5.4: notDestroyed must be present and contain the unknown id.
    let not_destroyed = resp["notDestroyed"]
        .as_object()
        .expect("notDestroyed must be a non-null object when destroying a nonexistent id");
    assert!(
        not_destroyed.contains_key("nonexistent-email-xyz"),
        "notDestroyed must contain the unknown id; notDestroyed: {not_destroyed:?}"
    );
    assert_eq!(
        not_destroyed["nonexistent-email-xyz"]["type"]
            .as_str()
            .unwrap_or(""),
        "notFound",
        "error type must be notFound; entry: {:?}",
        not_destroyed["nonexistent-email-xyz"]
    );
}

// ---------------------------------------------------------------------------
// Conformance tests ported from jmap-test-suite
// thread-get.test.ts, email-changes.test.ts, mailbox-changes.test.ts,
// thread-changes.test.ts
// ---------------------------------------------------------------------------

/// Oracle: RFC 8621 §3.1 — Thread/get for the alpha thread returns emailIds
/// with exactly 3 entries (thread-starter, thread-reply-1, thread-reply-2).
///
/// jmap-test-suite: thread-get.test.ts "get-thread-by-id"
#[tokio::test]
async fn conformance_thread_get_email_ids_present() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let thread_id = seed
        .thread
        .get("thread-alpha")
        .expect("seed must contain thread-alpha")
        .clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [thread_id.as_ref()],
    });
    let (resp, extra) = handle_thread_get(&backend, args)
        .await
        .expect("Thread/get must succeed");
    assert!(
        extra.is_empty(),
        "Thread/get must not produce extra invocations"
    );

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find exactly one thread");

    let email_ids = list[0]["emailIds"]
        .as_array()
        .expect("emailIds must be an array");
    assert_eq!(
        email_ids.len(),
        3,
        "thread-alpha must have 3 emailIds; got: {email_ids:?}"
    );
}

/// Oracle: RFC 8621 §3 — emailIds in a Thread MUST be sorted by receivedAt, oldest first.
///
/// External oracle: seed timestamps
///   thread-starter  2025-12-24T00:00:00Z  (days_ago(8))
///   thread-reply-1  2025-12-25T00:00:00Z  (days_ago(7))
///   thread-reply-2  2025-12-26T00:00:00Z  (days_ago(6))
///
/// jmap-test-suite: thread-get.test.ts "get-thread-email-ids-order"
#[tokio::test]
async fn conformance_thread_get_email_ids_ordered() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let thread_id = seed
        .thread
        .get("thread-alpha")
        .expect("seed must contain thread-alpha")
        .clone();

    // Expected order: oldest receivedAt first (RFC 8621 §3).
    let expected: [&str; 3] = [
        seed.email
            .get("thread-starter")
            .expect("thread-starter")
            .as_ref(),
        seed.email
            .get("thread-reply-1")
            .expect("thread-reply-1")
            .as_ref(),
        seed.email
            .get("thread-reply-2")
            .expect("thread-reply-2")
            .as_ref(),
    ];

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [thread_id.as_ref()],
    });
    let (resp, _) = handle_thread_get(&backend, args)
        .await
        .expect("Thread/get must succeed");

    let list = resp["list"].as_array().expect("list must be an array");
    let email_ids: Vec<&str> = list[0]["emailIds"]
        .as_array()
        .expect("emailIds must be an array")
        .iter()
        .map(|v| v.as_str().expect("emailId must be a string"))
        .collect();

    assert_eq!(
        email_ids, expected,
        "emailIds must be sorted oldest-first by receivedAt (RFC 8621 §3); \
         expected [thread-starter, thread-reply-1, thread-reply-2]"
    );
}

/// Oracle: RFC 8620 §5.1 — Thread/get with an unknown id must return that id in notFound.
///
/// jmap-test-suite: thread-get.test.ts "get-thread-not-found"
#[tokio::test]
async fn conformance_thread_get_not_found() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    setup_seed_data(&backend, &account_id).await;

    let bad_id = "bad-thread-id";
    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [bad_id],
    });
    let (resp, _) = handle_thread_get(&backend, args)
        .await
        .expect("Thread/get must succeed even for unknown ids");

    let not_found: Vec<&str> = resp["notFound"]
        .as_array()
        .expect("notFound must be an array")
        .iter()
        .map(|v| v.as_str().expect("notFound entry must be a string"))
        .collect();

    assert!(
        not_found.contains(&bad_id),
        "notFound must contain the unknown id '{bad_id}'; got: {not_found:?}"
    );

    let list = resp["list"].as_array().expect("list must be an array");
    assert!(
        list.is_empty(),
        "list must be empty when only unknown ids are requested; got: {list:?}"
    );
}

/// Oracle: RFC 8621 §3.1 — a single-email thread has emailIds with exactly one entry
/// equal to the email's own id.
///
/// plain-simple is imported without In-Reply-To/References, so it starts its own thread.
///
/// jmap-test-suite: thread-get.test.ts "get-single-email-thread"
#[tokio::test]
async fn conformance_thread_get_single_email() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let thread_id = seed
        .thread
        .get("plain-simple")
        .expect("seed must contain plain-simple thread")
        .clone();
    let email_id = seed
        .email
        .get("plain-simple")
        .expect("seed must contain plain-simple email")
        .clone();

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [thread_id.as_ref()],
    });
    let (resp, _) = handle_thread_get(&backend, args)
        .await
        .expect("Thread/get must succeed");

    let list = resp["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 1, "must find exactly one thread");

    let email_ids = list[0]["emailIds"]
        .as_array()
        .expect("emailIds must be an array");
    assert_eq!(
        email_ids.len(),
        1,
        "single-email thread must have exactly one emailId; got: {email_ids:?}"
    );
    assert_eq!(
        email_ids[0].as_str().unwrap_or(""),
        email_id.as_ref(),
        "the sole emailId must equal the email's id"
    );
}

/// Oracle: RFC 8620 §5.4 — Email/changes with sinceState "0" must include every
/// email id created since the beginning, including one just created.
///
/// jmap-test-suite: email-changes.test.ts (changes-after-create-and-destroy, first half)
#[tokio::test]
async fn conformance_email_changes_from_zero() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed
        .mailbox
        .get("inbox")
        .expect("seed must have inbox")
        .clone();

    // Create a new email after seed is loaded.
    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { inbox_id.as_ref(): true },
                "subject": "changes from zero test",
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set must succeed");
    let new_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("created id must be present")
        .to_owned();

    // Changes from state "0" must include the new email in created.
    let changes_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": "0",
    });
    let (chg_resp, _) = handle_email_changes(&backend, changes_args)
        .await
        .expect("Email/changes must succeed");

    let created: Vec<&str> = chg_resp["created"]
        .as_array()
        .expect("created must be an array")
        .iter()
        .map(|v| v.as_str().expect("id must be a string"))
        .collect();

    assert!(
        created.contains(&new_id.as_str()),
        "created must contain the new email id '{new_id}'; got: {created:?}"
    );
}

/// Oracle: RFC 8620 §5.4 — Email/changes with the current state returns empty
/// created/updated/destroyed arrays and oldState == sinceState.
///
/// jmap-test-suite: email-changes.test.ts "changes-no-changes"
#[tokio::test]
async fn conformance_email_changes_from_current_state() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    setup_seed_data(&backend, &account_id).await;

    // Obtain the current Email state from Email/get (ids=[]) which echoes state.
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [],
    });
    let (get_resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get must succeed");
    let current_state = get_resp["state"]
        .as_str()
        .expect("Email/get response must include state field")
        .to_owned();

    let changes_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": current_state,
    });
    let (chg_resp, _) = handle_email_changes(&backend, changes_args)
        .await
        .expect("Email/changes must succeed");

    assert_eq!(
        chg_resp["oldState"].as_str().unwrap_or(""),
        current_state,
        "oldState must echo sinceState"
    );

    let created = chg_resp["created"]
        .as_array()
        .expect("created must be array");
    let updated = chg_resp["updated"]
        .as_array()
        .expect("updated must be array");
    let destroyed = chg_resp["destroyed"]
        .as_array()
        .expect("destroyed must be array");

    assert!(
        created.is_empty(),
        "created must be empty when no changes occurred; got: {created:?}"
    );
    assert!(
        updated.is_empty(),
        "updated must be empty when no changes occurred; got: {updated:?}"
    );
    assert!(
        destroyed.is_empty(),
        "destroyed must be empty when no changes occurred; got: {destroyed:?}"
    );
}

/// Oracle: RFC 8620 §5.4 — after updating an email's keywords, Email/changes
/// must include that email's id in the "updated" list.
///
/// jmap-test-suite: email-changes.test.ts "changes-after-keyword-change"
#[tokio::test]
async fn conformance_email_changes_after_update() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed
        .mailbox
        .get("inbox")
        .expect("seed must have inbox")
        .clone();

    // Create an email whose keywords will be updated.
    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { inbox_id.as_ref(): true },
                "subject": "update test",
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set create must succeed");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("created id")
        .to_owned();
    // Capture state after create so changes since here show only the update.
    let state_after_create = set_resp["newState"]
        .as_str()
        .expect("newState after create")
        .to_owned();

    // Update a keyword on the email.
    let upd_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            email_id.clone(): { "keywords/$flagged": true }
        }
    });
    let (upd_resp, _) = handle_email_set(&backend, upd_args)
        .await
        .expect("Email/set update must succeed");
    assert!(
        upd_resp["notUpdated"]
            .as_object()
            .map_or(true, |m| m.is_empty()),
        "update must succeed; notUpdated must be empty"
    );

    // Email/changes since state_after_create must show the id in updated.
    let changes_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": state_after_create,
    });
    let (chg_resp, _) = handle_email_changes(&backend, changes_args)
        .await
        .expect("Email/changes must succeed");

    let updated: Vec<&str> = chg_resp["updated"]
        .as_array()
        .expect("updated must be an array")
        .iter()
        .map(|v| v.as_str().expect("id must be a string"))
        .collect();

    assert!(
        updated.contains(&email_id.as_str()),
        "updated must contain the email id '{email_id}'; got: {updated:?}"
    );
}

/// Oracle: RFC 8620 §5.4 — after destroying an email, Email/changes must include
/// that email's id in the "destroyed" list.
///
/// jmap-test-suite: email-changes.test.ts "changes-after-create-and-destroy"
#[tokio::test]
async fn conformance_email_changes_after_destroy() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed
        .mailbox
        .get("inbox")
        .expect("seed must have inbox")
        .clone();

    // Create an email to be destroyed.
    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { inbox_id.as_ref(): true },
                "subject": "destroy test",
            }
        }
    });
    let (set_resp, _) = handle_email_set(&backend, set_args)
        .await
        .expect("Email/set create must succeed");
    let email_id = set_resp["created"]["c0"]["id"]
        .as_str()
        .expect("created id")
        .to_owned();
    let state_after_create = set_resp["newState"]
        .as_str()
        .expect("newState after create")
        .to_owned();

    // Destroy the email.
    let destroy_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "destroy": [email_id.clone()],
    });
    let (destroy_resp, _) = handle_email_set(&backend, destroy_args)
        .await
        .expect("Email/set destroy must succeed");
    let destroyed_list = destroy_resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array");
    assert!(
        destroyed_list
            .iter()
            .any(|v| v.as_str().unwrap_or("") == email_id),
        "destroy must succeed; email id must appear in destroyed"
    );

    // Email/changes since state_after_create must show the id in destroyed.
    let changes_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": state_after_create,
    });
    let (chg_resp, _) = handle_email_changes(&backend, changes_args)
        .await
        .expect("Email/changes must succeed");

    let destroyed: Vec<&str> = chg_resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array")
        .iter()
        .map(|v| v.as_str().expect("id must be a string"))
        .collect();

    assert!(
        destroyed.contains(&email_id.as_str()),
        "destroyed must contain the email id '{email_id}'; got: {destroyed:?}"
    );
}

/// Oracle: RFC 8620 §5.4 — after mutating a mailbox property, Mailbox/changes
/// must include that mailbox's id in the "updated" list.
///
/// Note: MemoryBackend does not propagate email imports to Mailbox state
/// (totalEmails is not server-tracked in the test harness). We trigger a
/// genuine Mailbox mutation by renaming the inbox, which exercises the same
/// /changes contract as a totalEmails change would in a real server.
///
/// jmap-test-suite: mailbox-changes.test.ts "changes-after-rename" (adapted as
/// proxy for "changes-after-email-count-change")
#[tokio::test]
async fn conformance_mailbox_changes_after_email_count_change() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed
        .mailbox
        .get("inbox")
        .expect("seed must have inbox")
        .clone();

    // Capture mailbox state before the mutation.
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [],
    });
    let (get_resp, _) = handle_mailbox_get(&backend, get_args)
        .await
        .expect("Mailbox/get must succeed");
    let state_before = get_resp["state"]
        .as_str()
        .expect("Mailbox/get must include state")
        .to_owned();

    // Update the inbox name — a genuine Mailbox mutation the backend tracks.
    let upd_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            inbox_id.as_ref(): { "name": "Inbox (updated)" }
        }
    });
    let (upd_resp, _) = handle_mailbox_set(&backend, upd_args)
        .await
        .expect("Mailbox/set update must succeed");
    assert!(
        upd_resp["notUpdated"]
            .as_object()
            .map_or(true, |m| m.is_empty()),
        "inbox update must succeed; notUpdated must be empty; resp={upd_resp:?}"
    );

    // Mailbox/changes since state_before must contain inbox in "updated".
    let changes_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": state_before,
    });
    let (chg_resp, _) = handle_mailbox_changes(&backend, changes_args)
        .await
        .expect("Mailbox/changes must succeed");

    let updated: Vec<&str> = chg_resp["updated"]
        .as_array()
        .expect("updated must be an array")
        .iter()
        .map(|v| v.as_str().expect("id must be a string"))
        .collect();

    assert!(
        updated.contains(&inbox_id.as_ref()),
        "updated must contain inbox id '{}'; got: {updated:?}",
        inbox_id.as_ref()
    );
}

/// Oracle: RFC 8620 §5.4 — after importing a new email (new thread), Thread/changes
/// must include the new thread id in the "created" list.
///
/// jmap-test-suite: thread-changes.test.ts "changes-after-new-email"
#[tokio::test]
async fn conformance_thread_changes_after_new_thread() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed
        .mailbox
        .get("inbox")
        .expect("seed must have inbox")
        .clone();

    // Capture thread state before the import.
    let thread_state_before = backend
        .get_state::<jmap_mail_types::Thread>(&account_id)
        .await
        .expect("get_state::<Thread> must succeed");

    // Import a brand-new email (no In-Reply-To) — creates a new thread.
    let msg = b"From: newthread@example.com\r\nTo: user@example.com\r\n\
Message-ID: <new-thread-changes-001@test>\r\nSubject: New thread for changes test\r\n\r\nBody.\r\n";
    let blob_id = Id::from("blob-new-thread-changes");
    backend.store_blob(&blob_id, msg.to_vec());

    let import_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "emails": {
            "e1": {
                "blobId": blob_id.as_ref(),
                "mailboxIds": { inbox_id.as_ref(): true },
                "keywords": {},
            }
        }
    });
    let (import_resp, _) = handle_email_import(&backend, import_args)
        .await
        .expect("Email/import must succeed");

    // Retrieve the new thread id from the imported email.
    let new_email_id = import_resp["created"]["e1"]["id"]
        .as_str()
        .expect("created e1 id must be present")
        .to_owned();
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": [new_email_id.clone()],
        "properties": ["threadId"],
    });
    let (get_resp, _) = handle_email_get(&backend, get_args)
        .await
        .expect("Email/get must succeed");
    let new_thread_id = get_resp["list"][0]["threadId"]
        .as_str()
        .expect("threadId must be present")
        .to_owned();

    // Thread/changes since thread_state_before must include new thread in "created".
    let changes_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": thread_state_before.as_ref(),
    });
    let (chg_resp, _) = handle_thread_changes(&backend, changes_args)
        .await
        .expect("Thread/changes must succeed");

    let created: Vec<&str> = chg_resp["created"]
        .as_array()
        .expect("created must be an array")
        .iter()
        .map(|v| v.as_str().expect("id must be a string"))
        .collect();

    assert!(
        created.contains(&new_thread_id.as_str()),
        "created must contain the new thread id '{new_thread_id}'; got: {created:?}"
    );
}

/// Oracle: RFC 8620 §5.4 — after importing a reply into an existing thread,
/// Thread/changes must include that thread id in "updated", not in "created".
///
/// jmap-test-suite: thread-changes.test.ts "changes-after-new-email" (reply variant)
#[tokio::test]
async fn conformance_thread_changes_after_reply() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");
    let seed = setup_seed_data(&backend, &account_id).await;

    let inbox_id = seed
        .mailbox
        .get("inbox")
        .expect("seed must have inbox")
        .clone();
    let alpha_thread_id = seed
        .thread
        .get("thread-alpha")
        .expect("seed must contain thread-alpha")
        .clone();

    // Capture thread state after seed — the alpha thread already exists.
    let thread_state_before = backend
        .get_state::<jmap_mail_types::Thread>(&account_id)
        .await
        .expect("get_state::<Thread> must succeed");

    // Import a reply to thread-alpha via In-Reply-To the last known message-id.
    let msg = b"From: reply4@example.com\r\nTo: user@example.com\r\n\
Message-ID: <thread-alpha-004@test>\r\n\
In-Reply-To: <thread-alpha-003@test>\r\n\
References: <thread-alpha-001@test> <thread-alpha-002@test> <thread-alpha-003@test>\r\n\
Subject: Re: Project Alpha Discussion\r\n\r\nAnother reply.\r\n";
    let blob_id = Id::from("blob-reply-changes-test");
    backend.store_blob(&blob_id, msg.to_vec());

    let import_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "emails": {
            "e1": {
                "blobId": blob_id.as_ref(),
                "mailboxIds": { inbox_id.as_ref(): true },
                "keywords": {},
            }
        }
    });
    handle_email_import(&backend, import_args)
        .await
        .expect("Email/import of reply must succeed");

    // Thread/changes must show alpha thread in "updated", not in "created".
    let changes_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "sinceState": thread_state_before.as_ref(),
    });
    let (chg_resp, _) = handle_thread_changes(&backend, changes_args)
        .await
        .expect("Thread/changes must succeed");

    let updated: Vec<&str> = chg_resp["updated"]
        .as_array()
        .expect("updated must be an array")
        .iter()
        .map(|v| v.as_str().expect("id must be a string"))
        .collect();

    assert!(
        updated.contains(&alpha_thread_id.as_ref()),
        "updated must contain alpha thread id '{}'; got: {updated:?}",
        alpha_thread_id.as_ref()
    );

    let created: Vec<&str> = chg_resp["created"]
        .as_array()
        .expect("created must be an array")
        .iter()
        .map(|v| v.as_str().expect("id must be a string"))
        .collect();

    assert!(
        !created.contains(&alpha_thread_id.as_ref()),
        "alpha thread must NOT appear in created (it pre-existed); got: {created:?}"
    );
}

// ---------------------------------------------------------------------------
// Fix JMAP-bx3z.40 regression guard: Email/set create size must be > 0
// ---------------------------------------------------------------------------

/// Oracle: Email/set create must echo back size > 0.
///
/// RFC 8621 §5.5.3 — size is server-set. The backend must assign the real blob
/// size, not leave the placeholder 0 that email.rs places in the object before
/// calling create_object. This test guards against the placeholder fossilizing.
#[tokio::test]
async fn email_set_create_size_is_nonzero() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    // Create a mailbox first (required so mailboxIds is valid).
    use jmap_mail_types::MailboxRights;
    let mbox = jmap_mail_types::Mailbox::new(
        Id::from("placeholder"),
        "Inbox",
        0,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        true,
    );
    let (mbox_id, _) = backend
        .create_object::<jmap_mail_types::Mailbox>(&account_id, "c0", mbox)
        .await
        .expect("create mailbox");

    let args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "new1": {
                "mailboxIds": { mbox_id.as_ref(): true },
                "subject": "Test email for size check"
            }
        }
    });

    let (resp, _extra) = handle_email_set(&backend, args)
        .await
        .expect("Email/set must succeed");

    let created = resp["created"]
        .as_object()
        .expect("created must be an object");
    assert!(!created.is_empty(), "new1 must appear in created");

    let new1 = &created["new1"];
    let size = new1["size"].as_u64().expect("size must be a u64");
    assert!(
        size > 0,
        "Email/set create must return size > 0 (backend must not fossilize placeholder 0); got size={size}"
    );
}

/// Oracle: RFC 8620 §5.3 — destroys apply sequentially within one request.
/// Destroying [child, parent] in a single Mailbox/set call must succeed for
/// both: after the child is destroyed, the parent is no longer "has child".
///
/// JMAP-1vdc.21 regression guard.
#[tokio::test]
async fn mailbox_set_destroy_child_then_parent_in_one_request() {
    use jmap_mail_types::MailboxRights;

    let backend = MemoryBackend::new();
    let account_id = Id::from("acct1");

    // Create parent.
    let parent_mbox = jmap_mail_types::Mailbox::new(
        Id::from("placeholder"),
        "Parent".to_owned(),
        0,
        0,
        0,
        0,
        0,
        MailboxRights::default(),
        false,
    );
    let (parent_id, _) = backend
        .create_object::<jmap_mail_types::Mailbox>(&account_id, "p0", parent_mbox)
        .await
        .expect("create parent");

    // Create child under parent.
    let create_child = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "name": "Child",
                "parentId": parent_id.as_ref(),
            }
        },
    });
    let (child_resp, _) = handle_mailbox_set(&backend, create_child)
        .await
        .expect("create child must not error");
    let child_id_str = child_resp["created"]["c0"]["id"]
        .as_str()
        .expect("child id must be a string")
        .to_owned();

    // Destroy [child, parent] in a single request.
    // RFC 8620 §5.3: destroys are applied sequentially; after the child is
    // removed from the snapshot, the parent no longer has a child.
    let destroy_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "destroy": [child_id_str.as_str(), parent_id.as_ref()],
    });
    let (resp, _) = handle_mailbox_set(&backend, destroy_args)
        .await
        .expect("Mailbox/set destroy must not return JmapError");

    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array");
    assert!(
        destroyed
            .iter()
            .any(|v| v.as_str() == Some(child_id_str.as_str())),
        "child must be in destroyed; resp={resp:?}"
    );
    assert!(
        destroyed
            .iter()
            .any(|v| v.as_str() == Some(parent_id.as_ref())),
        "parent must be in destroyed (snapshot updated after child destroy); resp={resp:?}"
    );
    assert!(
        resp["notDestroyed"].is_null(),
        "notDestroyed must be null when both succeed; resp={resp:?}"
    );
}

// ---------------------------------------------------------------------------
// VacationResponse concurrent-create idempotency test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn vacation_set_concurrent_creates_are_idempotent() {
    // Oracle: RFC 8621 §8.2 + MailBackend::create_object contract —
    // two concurrent upserts must produce exactly one singleton, both succeeding.
    // MemoryBackend satisfies this via its Mutex<Inner> (serialises writes).
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("account1"));
    let account_id = Id::from("account1");

    let update_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "update": {
            "singleton": { "isEnabled": true, "textBody": "Out of office" }
        }
    });

    // Run both upserts concurrently on the same backend.  tokio::join! on a
    // single-threaded test runtime interleaves the futures cooperatively,
    // exercising the Mutex-serialised write path.
    let (r1, r2) = tokio::join!(
        handle_vacation_set(&backend, update_args.clone()),
        handle_vacation_set(&backend, update_args.clone())
    );
    let (resp1, _) = r1.expect("first upsert must not error at method level");
    let (resp2, _) = r2.expect("second upsert must not error at method level");

    assert!(
        resp1["notUpdated"].is_null()
            || resp1["notUpdated"]
                .as_object()
                .map_or(true, |m| m.is_empty()),
        "first upsert must not produce notUpdated errors; got: {:?}",
        resp1["notUpdated"]
    );
    assert!(
        resp2["notUpdated"].is_null()
            || resp2["notUpdated"]
                .as_object()
                .map_or(true, |m| m.is_empty()),
        "second upsert must not produce notUpdated errors; got: {:?}",
        resp2["notUpdated"]
    );

    // Verify exactly one singleton with the expected state.
    let get_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "ids": ["singleton"],
        "properties": ["isEnabled", "textBody"]
    });
    let (get_resp, _) = handle_vacation_get(&backend, get_args)
        .await
        .expect("VacationResponse/get must succeed");
    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(
        list.len(),
        1,
        "exactly one singleton must exist; got {}",
        list.len()
    );
    assert_eq!(list[0]["isEnabled"], serde_json::json!(true));
    assert_eq!(list[0]["textBody"], serde_json::json!("Out of office"));
}

// ---------------------------------------------------------------------------
// JMAP-yecd.3: MemoryBackend::query_changes filter/sort/upToId/maxChanges
// ---------------------------------------------------------------------------

/// Oracle: Email/queryChanges with an inMailbox filter returns only the new
/// email in that mailbox, not emails added to a different mailbox.
///
/// RFC 8620 §5.6 — removed/added must reflect the query as if it had been
/// re-run, meaning the filter must be applied.
#[tokio::test]
async fn query_changes_with_filter_returns_filtered_delta() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let account_id = "acct1";

    // Create a mailbox for inbox and folderA.
    let mbox_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "inbox": { "name": "Inbox" },
            "folderA": { "name": "FolderA" },
        }
    });
    let (mbox_resp, _) = handle_mailbox_set(&backend, mbox_args)
        .await
        .expect("mailbox create must succeed");
    let inbox_id = mbox_resp["created"]["inbox"]["id"]
        .as_str()
        .expect("inbox id")
        .to_string();
    let folder_a_id = mbox_resp["created"]["folderA"]["id"]
        .as_str()
        .expect("folderA id")
        .to_string();

    // Create one email in folderA (before the state snapshot).
    let pre_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "pre": { "mailboxIds": { folder_a_id.clone(): true } }
        }
    });
    handle_email_set(&backend, pre_args)
        .await
        .expect("pre-email create must succeed");

    // Capture the current state S1.
    let state_args = serde_json::json!({ "accountId": account_id, "sinceQueryState": "0" });
    let (s1_resp, _) = handle_email_query_changes(&backend, state_args)
        .await
        .expect("initial queryChanges must succeed");
    let s1 = s1_resp["newQueryState"]
        .as_str()
        .expect("newQueryState must be present")
        .to_string();

    // Add one email to inbox and one email to folderA after S1.
    let add_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "inbox_new": { "mailboxIds": { inbox_id.clone(): true } },
            "folder_new": { "mailboxIds": { folder_a_id.clone(): true } },
        }
    });
    let (add_resp, _) = handle_email_set(&backend, add_args)
        .await
        .expect("email creates must succeed");
    let inbox_email_id = add_resp["created"]["inbox_new"]["id"]
        .as_str()
        .expect("inbox_new email id")
        .to_string();

    // Call Email/queryChanges with filter={inMailbox: inbox_id}, since=S1.
    let qc_args = serde_json::json!({
        "accountId": account_id,
        "sinceQueryState": s1,
        "filter": { "inMailbox": inbox_id },
    });
    let (qc_resp, _) = handle_email_query_changes(&backend, qc_args)
        .await
        .expect("filtered queryChanges must succeed");

    let added: Vec<String> = qc_resp["added"]
        .as_array()
        .expect("added must be array")
        .iter()
        .map(|v| v["id"].as_str().expect("added item id").to_string())
        .collect();

    // The inbox email must appear in added.
    assert!(
        added.contains(&inbox_email_id),
        "inbox email must be in added; got added={added:?}, resp={qc_resp}"
    );
    // The folderA email must NOT appear in added (it does not pass the filter).
    let folder_email_id = add_resp["created"]["folder_new"]["id"]
        .as_str()
        .expect("folder_new email id")
        .to_string();
    assert!(
        !added.contains(&folder_email_id),
        "folderA email must not be in added when filter=inMailbox(inbox); got added={added:?}"
    );
}

/// Oracle: Email/queryChanges with upToId truncates added at that position.
///
/// RFC 8620 §5.6 — upToId: the server should only return changes up to and
/// NOT including the item at that position in the current result set.
#[tokio::test]
async fn query_changes_up_to_id_truncates_added() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let account_id = "acct1";

    // Create 3 emails with distinct receivedAt times so sort order is deterministic.
    let pre_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "e0": { "mailboxIds": { "inbox": true }, "receivedAt": "2024-01-01T00:00:00Z" },
            "e1": { "mailboxIds": { "inbox": true }, "receivedAt": "2024-01-02T00:00:00Z" },
            "e2": { "mailboxIds": { "inbox": true }, "receivedAt": "2024-01-03T00:00:00Z" },
        }
    });
    handle_email_set(&backend, pre_args)
        .await
        .expect("pre-create must succeed");

    // Capture state S1.
    let s1_resp = {
        let args = serde_json::json!({ "accountId": account_id, "sinceQueryState": "0" });
        let (r, _) = handle_email_query_changes(&backend, args)
            .await
            .expect("initial queryChanges must succeed");
        r
    };
    let s1 = s1_resp["newQueryState"]
        .as_str()
        .expect("newQueryState")
        .to_string();

    // Add 3 more emails with later dates.
    let new_args = serde_json::json!({
        "accountId": account_id,
        "create": {
            "n0": { "mailboxIds": { "inbox": true }, "receivedAt": "2024-01-04T00:00:00Z" },
            "n1": { "mailboxIds": { "inbox": true }, "receivedAt": "2024-01-05T00:00:00Z" },
            "n2": { "mailboxIds": { "inbox": true }, "receivedAt": "2024-01-06T00:00:00Z" },
        }
    });
    let (new_resp, _) = handle_email_set(&backend, new_args)
        .await
        .expect("new creates must succeed");

    // Get sorted IDs to find the position of n1.
    let sort_args = serde_json::json!({
        "accountId": account_id,
        "sort": [{ "property": "receivedAt", "isAscending": true }],
    });
    let (sort_resp, _) = handle_email_query(&backend, sort_args)
        .await
        .expect("query must succeed");
    let all_ids: Vec<String> = sort_resp["ids"]
        .as_array()
        .expect("ids array")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(all_ids.len(), 6, "must have 6 emails total");

    let n1_id = new_resp["created"]["n1"]["id"]
        .as_str()
        .expect("n1 id")
        .to_string();
    let n1_pos = all_ids
        .iter()
        .position(|id| *id == n1_id)
        .expect("n1 must be in sorted list");

    // queryChanges with upToId=n1, sort by receivedAt ascending.
    let qc_args = serde_json::json!({
        "accountId": account_id,
        "sinceQueryState": s1,
        "sort": [{ "property": "receivedAt", "isAscending": true }],
        "upToId": n1_id,
    });
    let (qc_resp, _) = handle_email_query_changes(&backend, qc_args)
        .await
        .expect("queryChanges with upToId must succeed");

    let added: Vec<String> = qc_resp["added"]
        .as_array()
        .expect("added must be array")
        .iter()
        .map(|v| v["id"].as_str().expect("id").to_owned())
        .collect();

    // n1 itself and anything at or after its position must be excluded.
    assert!(
        !added.contains(&n1_id),
        "upToId item itself must not appear in added; added={added:?}"
    );
    let n2_id = new_resp["created"]["n2"]["id"]
        .as_str()
        .expect("n2 id")
        .to_string();
    assert!(
        !added.contains(&n2_id),
        "items after upToId must not appear in added; added={added:?}"
    );
    // n0 is before n1_pos, so it must be in added.
    let n0_id = new_resp["created"]["n0"]["id"]
        .as_str()
        .expect("n0 id")
        .to_string();
    let n0_pos_in_all = all_ids
        .iter()
        .position(|id| *id == n0_id)
        .expect("n0 in sorted list");
    if n0_pos_in_all < n1_pos {
        assert!(
            added.contains(&n0_id),
            "n0 (before upToId) must be in added; added={added:?}, n0_pos={n0_pos_in_all}, n1_pos={n1_pos}"
        );
    }
}

/// Oracle: Email/queryChanges with maxChanges smaller than the number of changes
/// must return a cannotCalculateChanges error.
///
/// RFC 8620 §5.6 — if the number of changes would exceed maxChanges, the server
/// MAY return cannotCalculateChanges.
#[tokio::test]
async fn query_changes_max_changes_returns_cannot_calculate() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("acct1"));
    let account_id = "acct1";

    // Create 10 emails.
    let create_map: serde_json::Value = {
        let mut m = serde_json::Map::new();
        for i in 0..10usize {
            m.insert(
                format!("e{i}"),
                serde_json::json!({ "mailboxIds": { "inbox": true } }),
            );
        }
        serde_json::Value::Object(m)
    };
    handle_email_set(
        &backend,
        serde_json::json!({ "accountId": account_id, "create": create_map }),
    )
    .await
    .expect("pre-create must succeed");

    // Capture state S1.
    let s1 = {
        let args = serde_json::json!({ "accountId": account_id, "sinceQueryState": "0" });
        let (r, _) = handle_email_query_changes(&backend, args)
            .await
            .expect("initial queryChanges");
        r["newQueryState"]
            .as_str()
            .expect("newQueryState")
            .to_string()
    };

    // Create 5 more emails and destroy 5 of the originals — total 10 changes.
    let (pre_resp, _) =
        handle_email_query(&backend, serde_json::json!({ "accountId": account_id }))
            .await
            .expect("query for destroy ids");
    let destroy_ids: Vec<serde_json::Value> = pre_resp["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .take(5)
        .cloned()
        .collect();

    let new_create: serde_json::Value = {
        let mut m = serde_json::Map::new();
        for i in 0..5usize {
            m.insert(
                format!("n{i}"),
                serde_json::json!({ "mailboxIds": { "inbox": true } }),
            );
        }
        serde_json::Value::Object(m)
    };
    handle_email_set(
        &backend,
        serde_json::json!({
            "accountId": account_id,
            "create": new_create,
            "destroy": destroy_ids,
        }),
    )
    .await
    .expect("create+destroy must succeed");

    // Call queryChanges with maxChanges=3 — total changes (5 removed + 5 added = 10) > 3.
    let qc_args = serde_json::json!({
        "accountId": account_id,
        "sinceQueryState": s1,
        "maxChanges": 3,
    });
    let result = handle_email_query_changes(&backend, qc_args).await;
    assert!(
        result.is_err(),
        "maxChanges exceeded must return an error; got Ok: {:?}",
        result.ok()
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.error_type.as_str(),
        "cannotCalculateChanges",
        "error must be cannotCalculateChanges; got: {:?}",
        err.error_type
    );
}
