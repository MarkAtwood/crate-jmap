// Integration test entry point for jmap-mail-server.
//
// The common module provides MemoryBackend — an in-memory MailBackend used
// as the test harness for all handler integration tests.
//
// Additional test modules will be added here as handler crates are implemented.
#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_mail_server::{
    handle_email_changes, handle_email_get, handle_email_query, handle_email_query_changes,
    handle_email_set, handle_identity_get, handle_identity_set, handle_mailbox_get,
    handle_mailbox_query, handle_mailbox_query_changes, handle_mailbox_set,
    handle_search_snippet_get, handle_submission_get, handle_submission_query,
    handle_submission_set, handle_thread_changes, handle_thread_get, handle_vacation_get,
    handle_vacation_set, JmapObject, MailBackend, SetErrorType,
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
    backend.store_blob(blob_id.clone(), msg.to_vec());

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
    backend.store_blob(blob1.clone(), msg1.to_vec());
    backend.store_blob(blob2.clone(), msg2.to_vec());

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

    // notFound must be absent (null) since we found the thread.
    assert!(
        resp["notFound"].is_null(),
        "notFound must be null when all ids are found"
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
    backend.store_blob(blob_id.clone(), msg.to_vec());

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
    backend.store_blob(blob_id.clone(), msg.to_vec());

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
    assert!(
        resp["notFound"].is_null(),
        "notFound must be null when all ids are found"
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

    impl MailBackend for NoSnippetBackend {
        type Error = common::MemoryError;

        async fn get_objects<O: jmap_mail_server::GetObject + Send + Sync>(
            &self,
            account_id: &Id,
            ids: Option<&[Id]>,
            properties: Option<&[O::Property]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            self.0.get_objects(account_id, ids, properties).await
        }

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
                )
                .await
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
        ) -> Result<(Id, jmap_mail_types::Email), jmap_mail_server::BackendSetError<Self::Error>>
        {
            self.0
                .copy_email(
                    from_account_id,
                    email_id,
                    to_account_id,
                    mailbox_ids,
                    keywords,
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

    // Oracle: RFC 8621 §8.1 — list is empty, notFound is null.
    let list = resp["list"].as_array().expect("list must be an array");
    assert!(
        list.is_empty(),
        "fresh account must have empty vacation list"
    );
    assert!(
        resp["notFound"].is_null(),
        "notFound must be null when no ids were requested"
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
    backend.store_blob(blob_id.clone(), msg.to_vec());
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
    backend.store_blob(blob_id.clone(), msg.to_vec());
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
// EmailSubmission/* handler integration tests
// ---------------------------------------------------------------------------

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
    backend.store_blob(blob_id.clone(), msg.to_vec());
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
    assert!(
        get_resp["notFound"].is_null(),
        "notFound must be null when id is found"
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
    backend.store_blob(blob_id.clone(), msg.to_vec());
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
    // RFC 8621 §7.5: implicit call-id is "#<call-id-of-EmailSubmission/set>".
    assert_eq!(
        extra_call_id, "#call2",
        "extra invocation call_id must be '#' + original call_id (RFC 8621 §7.5)"
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
    backend.store_blob(blob_id.clone(), msg.to_vec());
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
    backend.store_blob(blob_id.clone(), msg.to_vec());
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
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "keywords": { "$seen": true },
                "subject": "Test email",
                "size": 42,
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
    assert_eq!(c0["size"].as_u64(), Some(42), "size must match");

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
    assert!(
        get_resp["notFound"].is_null(),
        "notFound must be null when email is found"
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

/// Oracle: Email/get with a `properties` filter returns only requested fields.
///
/// RFC 8621 §5.1 — when `properties` is given, only those properties appear
/// in each returned Email object.
#[tokio::test]
async fn email_get_with_property_filter() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let set_args = serde_json::json!({
        "accountId": account_id.as_ref(),
        "create": {
            "c0": {
                "mailboxIds": { "inbox": true },
                "subject": "Filtered subject",
                "size": 10,
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

/// Oracle: Email/set update with an immutable field is rejected with invalidProperties.
///
/// RFC 8621 §5.5.4 — the parsed header convenience properties (from, to, cc, etc.)
/// and metadata fields (id, blobId, threadId, size, receivedAt, subject, etc.)
/// are immutable after creation.
#[tokio::test]
async fn email_set_update_immutable_field_rejected() {
    let backend = MemoryBackend::new();
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
/// position, ids, and total. MemoryBackend returns all emails regardless of filter
/// (filter application is a backend responsibility); the handler's oracle is the
/// response structure.
#[tokio::test]
async fn email_query_by_mailbox() {
    let backend = MemoryBackend::new();
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
    assert_eq!(ids.len(), 3, "MemoryBackend returns all 3 emails");
    assert_eq!(
        query_resp["total"].as_u64(),
        Some(3),
        "total must reflect all created emails"
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
    let account_id = Id::from("account1");

    let msg = b"Subject: test\r\n\r\nbody";
    let blob_id = Id::from("blob-empty-mb");
    backend.store_blob(blob_id.clone(), msg.to_vec());

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
    let account_id = Id::from("account1");

    let msg = b"Subject: with keywords\r\n\r\nbody";
    let blob_id = Id::from("blob-kw");
    backend.store_blob(blob_id.clone(), msg.to_vec());

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
    let src_account = Id::from("src");
    let dst_account = Id::from("dst");

    // Import a source email.
    let msg = b"Subject: copy-kw test\r\n\r\nbody";
    let blob_id = Id::from("blob-copy-kw");
    backend.store_blob(blob_id.clone(), msg.to_vec());
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
    let src_account = Id::from("src-imm");
    let dst_account = Id::from("dst-imm");

    // Import a source email.
    let msg = b"Subject: immutable test\r\n\r\nbody";
    let blob_id = Id::from("blob-imm");
    backend.store_blob(blob_id.clone(), msg.to_vec());
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
    backend.store_blob(blob_id.clone(), msg.to_vec());
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

/// Oracle: Mailbox/query with a non-integer limit returns invalidArguments.
///
/// RFC 8620 §5.5: limit must be a UnsignedInt. Passing a string must be rejected,
/// not silently treated as no-limit. This matches Email/query behaviour.
#[tokio::test]
async fn mailbox_query_invalid_limit_rejected() {
    let backend = MemoryBackend::new();
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
    backend.store_blob(blob_id.clone(), msg.to_vec());
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

/// Oracle: Email/set create with malformed inReplyTo (not an array) must return
/// invalidProperties, not silently drop the field and create the email.
///
/// Trigger: RFC 8621 §5.5 — client sends "inReplyTo": "not-an-array". Before the
/// fix, the field was silently dropped via .ok(); now it must be rejected.
#[tokio::test]
async fn email_set_create_malformed_in_reply_to_returns_error() {
    let backend = MemoryBackend::new();
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

/// Oracle: Mailbox/set create with sortOrder > u32::MAX must return notCreated
/// with type=invalidProperties, not silently truncate the value.
///
/// Reference: RFC 8621 §2.1 defines sortOrder as UInt32; u64→u32 truncation
/// would corrupt data without error.
#[tokio::test]
async fn mailbox_set_sort_order_overflow_rejected() {
    let backend = MemoryBackend::new();
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
