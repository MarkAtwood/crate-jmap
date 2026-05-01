//! Shared test infrastructure — MemoryBackend in-memory MailBackend implementation.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code warnings are suppressed because not all items are used in every binary.
#![allow(dead_code)]
#![allow(async_fn_in_trait)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use jmap_mail_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapObject,
    MailBackend, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};
use jmap_mail_types::{
    query::{EmailFilter, Filter, Operator},
    Email, EmailAddress, EmailFilterCondition, Keyword, SearchSnippet,
};
use jmap_types::{Id, State, UTCDate};

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// A change log entry for one state transition.
#[derive(Clone)]
struct ChangeEntry {
    /// The state counter AFTER this change.
    new_state: u64,
    created: Vec<Id>,
    updated: Vec<Id>,
    destroyed: Vec<Id>,
}

/// Shared inner state, behind Arc<Mutex>.
#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → serialized object`
    objects: HashMap<(String, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(String, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(String, String), Vec<ChangeEntry>>,
    /// blob_id → raw bytes (used by import_email and parse_email)
    blobs: HashMap<Id, Vec<u8>>,
}

impl Inner {
    fn current_state(&self, type_name: &str, account_id: &str) -> u64 {
        *self
            .states
            .get(&(type_name.to_owned(), account_id.to_owned()))
            .unwrap_or(&0)
    }

    fn bump_state(&mut self, type_name: &str, account_id: &str) -> u64 {
        let entry = self
            .states
            .entry((type_name.to_owned(), account_id.to_owned()))
            .or_insert(0);
        *entry += 1;
        *entry
    }

    fn objects_mut(
        &mut self,
        type_name: &str,
        account_id: &str,
    ) -> &mut HashMap<Id, serde_json::Value> {
        self.objects
            .entry((type_name.to_owned(), account_id.to_owned()))
            .or_default()
    }

    fn objects_ref(
        &self,
        type_name: &str,
        account_id: &str,
    ) -> Option<&HashMap<Id, serde_json::Value>> {
        self.objects
            .get(&(type_name.to_owned(), account_id.to_owned()))
    }
}

// ---------------------------------------------------------------------------
// IdFate: per-ID fate tracker for RFC 8620 §5.6 deduplication
// ---------------------------------------------------------------------------

/// Per-ID fate tracker for RFC 8620 §5.6 ID deduplication across change log entries.
///
/// Rules across multiple entries in a single /changes window:
/// - created+updated → Created (update does not change that the object is new to the client)
/// - created+destroyed → removed from map (client never knew the object)
/// - updated+destroyed → Destroyed (client must remove it)
/// - updated+updated → Updated (deduplicated)
enum IdFate {
    Created,
    Updated,
    Destroyed,
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// Maximum number of objects returned when `ids=None` in [`MemoryBackend::get_objects`].
const MAX_FETCH_ALL: usize = 500;

/// In-memory [`MailBackend`] for integration tests and examples.
///
/// **Known limitation**: the internal change log grows without bound. This is
/// intentional for unit tests (which are short-lived).
#[derive(Clone, Default)]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a blob so that [`import_email`](MemoryBackend::import_email) can find it.
    pub fn store_blob(&self, blob_id: Id, bytes: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap();
        inner.blobs.insert(blob_id, bytes);
    }
}

// ---------------------------------------------------------------------------
// MemoryError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct MemoryError(pub String);

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryBackend error: {}", self.0)
    }
}

impl std::error::Error for MemoryError {}

// ---------------------------------------------------------------------------
// MailBackend impl
// ---------------------------------------------------------------------------

impl MailBackend for MemoryBackend {
    type Error = MemoryError;

    // -----------------------------------------------------------------------
    // get_objects
    // -----------------------------------------------------------------------

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[O::Property]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        let inner = self.inner.lock().unwrap();
        let store = match inner.objects_ref(O::TYPE_NAME, account_id.as_ref()) {
            Some(s) => s,
            None => return Ok((vec![], ids.map(|s| s.to_vec()).unwrap_or_default())),
        };

        let mut found = Vec::new();
        let mut not_found = Vec::new();

        if let Some(ids) = ids {
            for id in ids {
                match store.get(id) {
                    Some(val) => {
                        let obj: O = serde_json::from_value(val.clone()).map_err(|e| {
                            MemoryError(format!("deserialize {}: {e}", O::TYPE_NAME))
                        })?;
                        found.push(obj);
                    }
                    None => not_found.push(id.clone()),
                }
            }
        } else {
            for val in store.values().take(MAX_FETCH_ALL) {
                let obj: O = serde_json::from_value(val.clone())
                    .map_err(|e| MemoryError(format!("deserialize {}: {e}", O::TYPE_NAME)))?;
                found.push(obj);
            }
        }

        Ok((found, not_found))
    }

    // -----------------------------------------------------------------------
    // create_object
    // -----------------------------------------------------------------------

    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize: {e}"))))?;
        // Use the object's existing id if it is a meaningful server-assigned
        // value (e.g. VacationResponse always uses "singleton"). Treat absent
        // or "placeholder" ids as a signal to assign a fresh UUID.
        let id = match val.get("id").and_then(|v| v.as_str()) {
            Some(s) if s != "placeholder" => Id::from(s),
            _ => {
                let uuid_id = Id::from(uuid::Uuid::new_v4().to_string());
                if let serde_json::Value::Object(ref mut map) = val {
                    map.insert(
                        "id".to_owned(),
                        serde_json::Value::String(uuid_id.to_string()),
                    );
                }
                uuid_id
            }
        };
        // Replace placeholder blobId with a server-assigned UUID. The Email/set
        // create handler sets blobId to "placeholder-blob" because it has no raw
        // bytes to hash; the backend is responsible for assigning the real value.
        // MemoryBackend uses a UUID since it does not store raw blobs on this
        // path. Real backends should store the blob and use a content hash here.
        if val.get("blobId").and_then(|v| v.as_str()) == Some("placeholder-blob") {
            if let serde_json::Value::Object(ref mut map) = val {
                let blob_uuid = Id::from(uuid::Uuid::new_v4().to_string());
                map.insert(
                    "blobId".to_owned(),
                    serde_json::Value::String(blob_uuid.to_string()),
                );
            }
        }
        let created_obj: O = serde_json::from_value(val.clone()).map_err(|e| {
            BackendSetError::Other(MemoryError(format!("deserialize after create: {e}")))
        })?;

        let mut inner = self.inner.lock().unwrap();
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(id.clone(), val);
        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .change_log
            .entry((O::TYPE_NAME.to_owned(), account_id.to_string()))
            .or_default()
            .push(ChangeEntry {
                new_state,
                created: vec![id.clone()],
                updated: vec![],
                destroyed: vec![],
            });

        Ok((id, created_obj))
    }

    // -----------------------------------------------------------------------
    // update_object
    // -----------------------------------------------------------------------

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        let patch_val: serde_json::Value = serde_json::to_value(patch)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

        let mut inner = self.inner.lock().unwrap();
        let store = inner.objects_mut(O::TYPE_NAME, account_id.as_ref());
        let existing = store
            .get_mut(id)
            .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?;

        // JMAP patch (RFC 8620 §5.3): keys may be "/" separated paths into nested
        // objects (e.g. "mailboxIds/abc123"). Null values remove the target key;
        // non-null values overwrite it. apply_jmap_patch handles both flat and
        // path-style keys so that cascade operations like mailboxIds/<id>: null work.
        if let serde_json::Value::Object(base) = existing {
            if let serde_json::Value::Object(patch_map) = patch_val {
                for (k, v) in patch_map {
                    apply_jmap_patch(base, &k, v);
                }
            }
        }

        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .change_log
            .entry((O::TYPE_NAME.to_owned(), account_id.to_string()))
            .or_default()
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![id.clone()],
                destroyed: vec![],
            });

        Ok(None) // MemoryBackend does not echo server-modified fields
    }

    // -----------------------------------------------------------------------
    // destroy_object
    // -----------------------------------------------------------------------

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();
        let store = inner.objects_mut(O::TYPE_NAME, account_id.as_ref());
        store
            .remove(id)
            .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?;
        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .change_log
            .entry((O::TYPE_NAME.to_owned(), account_id.to_string()))
            .or_default()
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![],
                destroyed: vec![id.clone()],
            });
        Ok(())
    }

    // -----------------------------------------------------------------------
    // get_state
    // -----------------------------------------------------------------------

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        let inner = self.inner.lock().unwrap();
        let n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
        Ok(State::from(n.to_string()))
    }

    // -----------------------------------------------------------------------
    // get_changes
    // -----------------------------------------------------------------------

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let since: u64 = since_state.as_ref().parse().map_err(|_| {
            BackendChangesError::Other(MemoryError(format!("invalid state token: {since_state}")))
        })?;

        // Snapshot relevant change log entries under a brief lock, then release.
        let (relevant, has_more, new_state) = {
            let inner = self.inner.lock().unwrap();
            let log = inner
                .change_log
                .get(&(O::TYPE_NAME.to_owned(), account_id.to_string()))
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            let limit = max_changes.map_or(usize::MAX, |n| n.min(usize::MAX as u64) as usize);
            // Binary search for the first entry with new_state > since.
            let start = log.partition_point(|e| e.new_state <= since);
            // Take limit+1 as sentinel to detect has_more.
            let mut entries: Vec<ChangeEntry> = log[start..]
                .iter()
                .take(limit.saturating_add(1))
                .cloned()
                .collect();
            let has_more = entries.len() > limit;
            if has_more {
                entries.pop();
            }
            // If nothing changed, new_state == since_state (client is already up to date).
            let new_state = entries
                .last()
                .map(|e| State::from(e.new_state.to_string()))
                .unwrap_or_else(|| since_state.clone());
            (entries, has_more, new_state)
        };

        // RFC 8620 §5.6 ID deduplication across the window.
        let mut fates: HashMap<Id, IdFate> = HashMap::new();
        for entry in &relevant {
            for id in &entry.created {
                fates.insert(id.clone(), IdFate::Created);
            }
            for id in &entry.updated {
                let fate = match fates.get(id) {
                    Some(IdFate::Created) => IdFate::Created,
                    Some(IdFate::Destroyed) => IdFate::Destroyed,
                    _ => IdFate::Updated,
                };
                fates.insert(id.clone(), fate);
            }
            for id in &entry.destroyed {
                match fates.remove(id) {
                    Some(IdFate::Created) => {} // created+destroyed in window → omit
                    Some(_) | None => {
                        fates.insert(id.clone(), IdFate::Destroyed);
                    }
                }
            }
        }

        let mut created = Vec::new();
        let mut updated = Vec::new();
        let mut destroyed = Vec::new();
        for (id, fate) in fates {
            match fate {
                IdFate::Created => created.push(id),
                IdFate::Updated => updated.push(id),
                IdFate::Destroyed => destroyed.push(id),
            }
        }

        Ok(ChangesResult::new(
            created, updated, destroyed, has_more, new_state,
        ))
    }

    // -----------------------------------------------------------------------
    // query_objects
    // -----------------------------------------------------------------------

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        // Collect and sort IDs outside the lock for deterministic ordering.
        // For Email objects, apply filter conditions in-process using a JSON
        // roundtrip (since O::Filter: Serialize, we can recover the typed filter).
        let email_filter: Option<EmailFilter> = if O::TYPE_NAME == "Email" {
            filter.and_then(|f| {
                serde_json::to_value(f)
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok())
            })
        } else {
            None
        };

        let (mut all_ids, state_n) = {
            let inner = self.inner.lock().unwrap();
            let ids: Vec<Id> = if let Some(ref ef) = email_filter {
                // Apply email filter: deserialize each stored object and check.
                inner
                    .objects_ref(O::TYPE_NAME, account_id.as_ref())
                    .map(|map| {
                        map.iter()
                            .filter_map(|(id, val)| {
                                let email: Email = serde_json::from_value(val.clone()).ok()?;
                                if email_matches_filter(&email, ef) {
                                    Some(id.clone())
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                inner
                    .objects_ref(O::TYPE_NAME, account_id.as_ref())
                    .map(|s| s.keys().cloned().collect())
                    .unwrap_or_default()
            };
            let state_n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
            (ids, state_n)
        };
        all_ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));

        let total = all_ids.len();
        let start = if position >= 0 {
            (position as usize).min(total)
        } else {
            let neg = (-position) as usize;
            total.saturating_sub(neg)
        };

        let ids: Vec<Id> = all_ids[start..]
            .iter()
            .take(limit.map_or(usize::MAX, |n| n.min(usize::MAX as u64) as usize))
            .cloned()
            .collect();

        Ok(QueryResult::new(
            ids,
            start as i64,
            Some(total as u64),
            State::from(state_n.to_string()),
            true,
        ))
    }

    // -----------------------------------------------------------------------
    // query_changes
    // -----------------------------------------------------------------------

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        // MemoryBackend does not track per-query result sets. Return the full
        // current id list as "added" when since_query_state == "0"; otherwise
        // return empty changes. Callers that need precise queryChanges tracking
        // should use get_changes + query_objects instead.
        let (mut all_ids, state_n) = {
            let inner = self.inner.lock().unwrap();
            let ids: Vec<Id> = inner
                .objects_ref(O::TYPE_NAME, account_id.as_ref())
                .map(|s| s.keys().cloned().collect())
                .unwrap_or_default();
            let state_n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
            (ids, state_n)
        };
        all_ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        let new_query_state = State::from(state_n.to_string());

        let added: Vec<AddedItem> = if since_query_state.as_ref() == "0" {
            all_ids
                .into_iter()
                .enumerate()
                .map(|(i, id)| AddedItem::new(id, i as u64))
                .collect()
        } else {
            vec![]
        };

        Ok(QueryChangesResult::new(
            since_query_state.clone(),
            new_query_state,
            None,
            vec![],
            added,
        ))
    }

    // -----------------------------------------------------------------------
    // import_email
    // -----------------------------------------------------------------------

    async fn import_email(
        &self,
        account_id: &Id,
        blob_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[Keyword],
        received_at: Option<&UTCDate>,
    ) -> Result<(Id, Email), BackendSetError<Self::Error>> {
        let bytes = {
            let inner = self.inner.lock().unwrap();
            inner.blobs.get(blob_id).cloned().ok_or_else(|| {
                BackendSetError::SetError(SetError::new(SetErrorType::BlobNotFound))
            })?
        };

        // Parse message headers from raw bytes (best-effort RFC 5322 parsing).
        let parsed = parse_rfc5322_headers(&bytes);

        // Assign thread: look for existing email with matching message-id.
        let thread_id = {
            let inner = self.inner.lock().unwrap();
            assign_thread_inner(&inner, account_id, &parsed.in_reply_to, &parsed.references)
        };

        // Build the Email object.
        let email_id = Id::from(uuid::Uuid::new_v4().to_string());
        let mailbox_map: HashMap<Id, bool> =
            mailbox_ids.iter().map(|id| (id.clone(), true)).collect();
        let kw_map: HashMap<Keyword, bool> = keywords.iter().map(|k| (k.clone(), true)).collect();

        let received = received_at
            .cloned()
            .unwrap_or_else(|| UTCDate::from("1970-01-01T00:00:00Z"));

        let mut email = Email::new(
            email_id.clone(),
            blob_id.clone(),
            thread_id.clone(),
            mailbox_map,
            bytes.len() as u64,
            received,
        );
        email.keywords = kw_map;
        email.subject = parsed.subject;
        email.message_id = parsed.message_id;
        email.in_reply_to = if parsed.in_reply_to.is_empty() {
            None
        } else {
            Some(parsed.in_reply_to)
        };
        email.references = if parsed.references.is_empty() {
            None
        } else {
            Some(parsed.references)
        };
        email.from = parsed.from;
        email.to = parsed.to;
        if let Some(preview) = parsed.preview {
            email.preview = Some(preview);
        }

        // Ensure the Thread object exists.
        let thread_val = serde_json::json!({
            "id": thread_id.to_string(),
            "emailIds": [email_id.to_string()]
        });

        // Serialize the email for storage.
        let email_val = serde_json::to_value(&email)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize email: {e}"))))?;

        {
            let mut inner = self.inner.lock().unwrap();

            // Insert or update Thread (append email_id if thread exists).
            let thread_store = inner.objects_mut("Thread", account_id.as_ref());
            let thread_existed = thread_store.contains_key(&thread_id);
            thread_store
                .entry(thread_id.clone())
                .and_modify(|v| {
                    if let Some(arr) = v.get_mut("emailIds").and_then(|a| a.as_array_mut()) {
                        arr.push(serde_json::Value::String(email_id.to_string()));
                    }
                })
                .or_insert(thread_val);

            // Insert Email.
            inner
                .objects_mut("Email", account_id.as_ref())
                .insert(email_id.clone(), email_val);

            // Bump state for both Email and Thread.
            let new_email_state = inner.bump_state("Email", account_id.as_ref());
            inner
                .change_log
                .entry(("Email".to_owned(), account_id.to_string()))
                .or_default()
                .push(ChangeEntry {
                    new_state: new_email_state,
                    created: vec![email_id.clone()],
                    updated: vec![],
                    destroyed: vec![],
                });
            let new_thread_state = inner.bump_state("Thread", account_id.as_ref());
            inner
                .change_log
                .entry(("Thread".to_owned(), account_id.to_string()))
                .or_default()
                .push(ChangeEntry {
                    new_state: new_thread_state,
                    created: if thread_existed {
                        vec![]
                    } else {
                        vec![thread_id.clone()]
                    },
                    updated: if thread_existed {
                        vec![thread_id]
                    } else {
                        vec![]
                    },
                    destroyed: vec![],
                });
        }

        Ok((email_id, email))
    }

    // -----------------------------------------------------------------------
    // find_thread_by_message_ids
    // -----------------------------------------------------------------------

    async fn find_thread_by_message_ids(
        &self,
        account_id: &Id,
        message_ids: &[&str],
    ) -> Result<Option<Id>, Self::Error> {
        if message_ids.is_empty() {
            return Ok(None);
        }
        let inner = self.inner.lock().unwrap();
        let store = match inner.objects_ref("Email", account_id.as_ref()) {
            Some(s) => s,
            None => return Ok(None),
        };
        for val in store.values() {
            if let Some(ids) = val.get("messageId").and_then(|v| v.as_array()) {
                for id in ids {
                    if let Some(s) = id.as_str() {
                        if message_ids.contains(&s) {
                            if let Some(tid) = val.get("threadId").and_then(|v| v.as_str()) {
                                return Ok(Some(Id::from(tid)));
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    // -----------------------------------------------------------------------
    // blob_exists / parse_email
    // -----------------------------------------------------------------------

    async fn blob_exists(&self, _account_id: &Id, blob_id: &Id) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.blobs.contains_key(blob_id)
    }

    async fn parse_email(&self, account_id: &Id, blob_id: &Id) -> Result<Email, Self::Error> {
        let bytes = {
            let inner = self.inner.lock().unwrap();
            inner
                .blobs
                .get(blob_id)
                .cloned()
                .ok_or_else(|| MemoryError(format!("blob not found: {blob_id}")))?
        };

        let parsed = parse_rfc5322_headers(&bytes);

        // parse_email does not store — use a synthetic id.
        let email_id = Id::from(format!("parse-{blob_id}"));
        // Assign a thread id based on account state but do not store the thread.
        let thread_id = {
            let inner = self.inner.lock().unwrap();
            assign_thread_inner(&inner, account_id, &parsed.in_reply_to, &parsed.references)
        };

        let mailbox_map = HashMap::new();
        let received = UTCDate::from("1970-01-01T00:00:00Z");
        let mut email = Email::new(
            email_id,
            blob_id.clone(),
            thread_id,
            mailbox_map,
            bytes.len() as u64,
            received,
        );
        email.subject = parsed.subject;
        email.message_id = parsed.message_id;
        email.in_reply_to = if parsed.in_reply_to.is_empty() {
            None
        } else {
            Some(parsed.in_reply_to)
        };
        email.references = if parsed.references.is_empty() {
            None
        } else {
            Some(parsed.references)
        };
        email.from = parsed.from;
        email.to = parsed.to;
        if let Some(preview) = parsed.preview {
            email.preview = Some(preview);
        }

        Ok(email)
    }

    // -----------------------------------------------------------------------
    // copy_email
    // -----------------------------------------------------------------------

    async fn copy_email(
        &self,
        from_account_id: &Id,
        email_id: &Id,
        to_account_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[Keyword],
        received_at: Option<&UTCDate>,
    ) -> Result<(Id, Email), BackendSetError<Self::Error>> {
        // Look up source email.
        let src_val = {
            let inner = self.inner.lock().unwrap();
            inner
                .objects_ref("Email", from_account_id.as_ref())
                .and_then(|s| s.get(email_id))
                .cloned()
                .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?
        };

        let src_email: Email = serde_json::from_value(src_val).map_err(|e| {
            BackendSetError::Other(MemoryError(format!("deserialize source email: {e}")))
        })?;

        // Assign thread in destination account.
        let thread_id = {
            let inner = self.inner.lock().unwrap();
            assign_thread_inner(
                &inner,
                to_account_id,
                src_email.in_reply_to.as_deref().unwrap_or(&[]),
                src_email.references.as_deref().unwrap_or(&[]),
            )
        };

        let new_id = Id::from(uuid::Uuid::new_v4().to_string());
        let mailbox_map: HashMap<Id, bool> =
            mailbox_ids.iter().map(|id| (id.clone(), true)).collect();
        let kw_map: HashMap<Keyword, bool> = keywords.iter().map(|k| (k.clone(), true)).collect();

        let mut new_email = Email::new(
            new_id.clone(),
            src_email.blob_id.clone(),
            thread_id.clone(),
            mailbox_map,
            src_email.size,
            received_at
                .cloned()
                .unwrap_or_else(|| src_email.received_at.clone()),
        );
        new_email.keywords = kw_map;
        new_email.subject = src_email.subject.clone();
        new_email.message_id = src_email.message_id.clone();
        new_email.in_reply_to = src_email.in_reply_to.clone();
        new_email.references = src_email.references.clone();
        new_email.from = src_email.from.clone();
        new_email.to = src_email.to.clone();
        new_email.preview = src_email.preview.clone();

        let email_val = serde_json::to_value(&new_email).map_err(|e| {
            BackendSetError::Other(MemoryError(format!("serialize copied email: {e}")))
        })?;
        let thread_val = serde_json::json!({
            "id": thread_id.to_string(),
            "emailIds": [new_id.to_string()]
        });

        {
            let mut inner = self.inner.lock().unwrap();
            let thread_existed = inner
                .objects_ref("Thread", to_account_id.as_ref())
                .is_some_and(|s| s.contains_key(&thread_id));
            inner
                .objects_mut("Thread", to_account_id.as_ref())
                .entry(thread_id.clone())
                .and_modify(|v| {
                    if let Some(arr) = v.get_mut("emailIds").and_then(|a| a.as_array_mut()) {
                        arr.push(serde_json::Value::String(new_id.to_string()));
                    }
                })
                .or_insert(thread_val);

            inner
                .objects_mut("Email", to_account_id.as_ref())
                .insert(new_id.clone(), email_val);

            let new_email_state = inner.bump_state("Email", to_account_id.as_ref());
            inner
                .change_log
                .entry(("Email".to_owned(), to_account_id.to_string()))
                .or_default()
                .push(ChangeEntry {
                    new_state: new_email_state,
                    created: vec![new_id.clone()],
                    updated: vec![],
                    destroyed: vec![],
                });
            let new_thread_state = inner.bump_state("Thread", to_account_id.as_ref());
            inner
                .change_log
                .entry(("Thread".to_owned(), to_account_id.to_string()))
                .or_default()
                .push(ChangeEntry {
                    new_state: new_thread_state,
                    created: if thread_existed {
                        vec![]
                    } else {
                        vec![thread_id.clone()]
                    },
                    updated: if thread_existed {
                        vec![thread_id]
                    } else {
                        vec![]
                    },
                    destroyed: vec![],
                });
        }

        Ok((new_id, new_email))
    }

    // -----------------------------------------------------------------------
    // search_snippets
    // -----------------------------------------------------------------------

    async fn search_snippets(
        &self,
        account_id: &Id,
        email_ids: &[Id],
        filter: Option<&EmailFilterCondition>,
    ) -> Result<Vec<SearchSnippet>, Self::Error> {
        let text_needle = filter.and_then(|f| f.text.as_deref());
        let subject_needle = filter.and_then(|f| f.subject.as_deref());
        let body_needle = filter.and_then(|f| f.body.as_deref());

        let inner = self.inner.lock().unwrap();
        let store = inner.objects_ref("Email", account_id.as_ref());

        let mut snippets = Vec::new();
        for id in email_ids {
            let mut snippet = SearchSnippet::new(id.clone());

            if let Some(store) = store {
                if let Some(val) = store.get(id) {
                    let subject = val.get("subject").and_then(|s| s.as_str()).unwrap_or("");
                    let preview = val.get("preview").and_then(|s| s.as_str()).unwrap_or("");

                    // Build subject snippet.
                    let subj_needle = subject_needle.or(text_needle);
                    if let Some(needle) = subj_needle {
                        if !subject.is_empty() {
                            snippet.subject = Some(highlight(subject, needle));
                        }
                    }

                    // Build preview snippet from preview or body needle.
                    let prev_needle = body_needle.or(text_needle);
                    if let Some(needle) = prev_needle {
                        if !preview.is_empty() {
                            snippet.preview = Some(highlight(preview, needle));
                        }
                    }
                }
            }

            snippets.push(snippet);
        }

        Ok(snippets)
    }

    // -----------------------------------------------------------------------
    // supports_type
    // -----------------------------------------------------------------------

    fn supports_type<O: JmapObject>(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Minimal parsed fields from an RFC 5322 message header block.
struct ParsedHeaders {
    subject: Option<String>,
    message_id: Option<Vec<String>>,
    in_reply_to: Vec<String>,
    references: Vec<String>,
    from: Option<Vec<EmailAddress>>,
    to: Option<Vec<EmailAddress>>,
    /// Short preview of the body (first 256 bytes of the text body, if any).
    preview: Option<String>,
}

/// Bare-minimum RFC 5322 header parser.
///
/// Reads raw bytes as UTF-8 (lossy), splits on the blank line that separates
/// headers from the body, and extracts the fields needed for threading and
/// snippet generation. Folded header lines (CRLF + whitespace) are unfolded.
///
/// This is intentionally simple — it handles the common cases in tests. A
/// production implementation would use a proper MIME library.
fn parse_rfc5322_headers(bytes: &[u8]) -> ParsedHeaders {
    let text = String::from_utf8_lossy(bytes);

    // Split headers from body at the first blank line.
    let (header_block, body_block) = if let Some(idx) = text.find("\r\n\r\n") {
        (&text[..idx], &text[idx + 4..])
    } else if let Some(idx) = text.find("\n\n") {
        (&text[..idx], &text[idx + 2..])
    } else {
        (text.as_ref(), "")
    };

    // Unfold header lines: CRLF or LF followed by whitespace = continuation.
    let unfolded = header_block
        .replace("\r\n ", " ")
        .replace("\r\n\t", " ")
        .replace("\n ", " ")
        .replace("\n\t", " ");

    let mut subject = None;
    let mut message_id: Option<Vec<String>> = None;
    let mut in_reply_to: Vec<String> = Vec::new();
    let mut references: Vec<String> = Vec::new();
    let mut from_header: Option<String> = None;
    let mut to_header: Option<String> = None;

    for line in unfolded.lines() {
        if let Some(rest) = line.strip_prefix("Subject:") {
            subject = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("subject:") {
            subject = Some(rest.trim().to_owned());
        } else if let Some(rest) = line
            .strip_prefix("Message-ID:")
            .or_else(|| line.strip_prefix("Message-Id:"))
        {
            let ids = extract_msg_ids(rest);
            if !ids.is_empty() {
                message_id = Some(ids);
            }
        } else if let Some(rest) = line.strip_prefix("In-Reply-To:") {
            in_reply_to = extract_msg_ids(rest);
        } else if let Some(rest) = line.strip_prefix("References:") {
            references = extract_msg_ids(rest);
        } else if let Some(rest) = line.strip_prefix("From:") {
            from_header = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("To:") {
            to_header = Some(rest.trim().to_owned());
        }
    }

    let from = from_header.as_deref().map(parse_address_list);
    let to = to_header.as_deref().map(parse_address_list);

    // Extract a short preview from the body.
    let preview = if body_block.trim().is_empty() {
        None
    } else {
        let trimmed = body_block.trim();
        let end = trimmed
            .char_indices()
            .take(256)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(trimmed.len());
        Some(trimmed[..end].to_owned())
    };

    ParsedHeaders {
        subject,
        message_id,
        in_reply_to,
        references,
        from,
        to,
        preview,
    }
}

/// Extract `<id>` tokens from a Message-ID / In-Reply-To / References value.
fn extract_msg_ids(s: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        if let Some(end) = rest.find('>') {
            ids.push(rest[..end].to_owned());
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
    ids
}

/// Very simple RFC 5322 address parser: handles `Display Name <addr>` and bare `addr`.
///
/// Splits on commas, strips whitespace, extracts `<>` if present.
fn parse_address_list(s: &str) -> Vec<EmailAddress> {
    s.split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            if let (Some(lt), Some(gt)) = (part.rfind('<'), part.rfind('>')) {
                if lt < gt {
                    let email = part[lt + 1..gt].trim().to_owned();
                    let name = part[..lt].trim().trim_matches('"').trim().to_owned();
                    let mut addr = EmailAddress::new(email);
                    if !name.is_empty() {
                        addr.name = Some(name);
                    }
                    return Some(addr);
                }
            }
            Some(EmailAddress::new(part.to_owned()))
        })
        .collect()
}

/// Assign a thread id for an email being imported or copied.
///
/// Searches existing emails in the account for a `message_id` that matches
/// any of the `in_reply_to` or `references` tokens. If found, reuses that
/// thread id. Otherwise returns a fresh id.
fn assign_thread_inner(
    inner: &Inner,
    account_id: &Id,
    in_reply_to: &[String],
    references: &[String],
) -> Id {
    let refs: Vec<&str> = in_reply_to
        .iter()
        .chain(references.iter())
        .map(|s| s.as_str())
        .collect();

    if !refs.is_empty() {
        if let Some(store) = inner.objects_ref("Email", account_id.as_ref()) {
            for val in store.values() {
                if let Some(msg_ids) = val.get("messageId").and_then(|v| v.as_array()) {
                    for msg_id in msg_ids {
                        if let Some(s) = msg_id.as_str() {
                            if refs.contains(&s) {
                                if let Some(tid) = val.get("threadId").and_then(|v| v.as_str()) {
                                    return Id::from(tid);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Id::from(uuid::Uuid::new_v4().to_string())
}

/// Highlight occurrences of `needle` in `haystack` using `<mark>…</mark>` tags.
///
/// Case-insensitive match. HTML-escapes `&`, `<`, `>` in the surrounding text.
fn highlight(haystack: &str, needle: &str) -> String {
    if needle.is_empty() {
        return html_escape(haystack);
    }
    let lower_needle = needle.to_lowercase();
    let needle_char_count = lower_needle.chars().count();
    // Lowercase the whole haystack once. Because lowercasing can change a char's
    // byte length (e.g. Ω (2 bytes) → ω (2 bytes), but Σ (2 bytes) → σ (2 bytes),
    // and some chars expand), we match positions in lower_haystack and convert them
    // to char counts, then re-locate those char counts in the original haystack.
    let lower_haystack = haystack.to_lowercase();
    let mut result = String::with_capacity(haystack.len() + 32);
    // Byte offsets into lower_haystack and haystack respectively.
    let mut lower_pos = 0usize; // position in lower_haystack
    let mut orig_pos = 0usize; // corresponding byte position in haystack
    // Build a parallel char-index table: lower_char_starts[i] = byte offset of
    // the i-th char in lower_haystack; orig_char_starts[i] = byte offset of
    // the i-th char in haystack.
    let lower_chars: Vec<usize> = lower_haystack.char_indices().map(|(i, _)| i).collect();
    let orig_chars: Vec<usize> = haystack.char_indices().map(|(i, _)| i).collect();
    // char_pos tracks which char index lower_pos corresponds to.
    let mut char_pos = 0usize;
    loop {
        match lower_haystack[lower_pos..].find(&lower_needle) {
            None => {
                result.push_str(&html_escape(&haystack[orig_pos..]));
                break;
            }
            Some(rel_lower_idx) => {
                // Byte offset in lower_haystack where the match starts.
                let abs_lower_idx = lower_pos + rel_lower_idx;
                // Count how many lower chars precede the match start from char_pos.
                let chars_before = lower_haystack[lower_pos..abs_lower_idx].chars().count();
                let match_char_start = char_pos + chars_before;
                let match_char_end = match_char_start + needle_char_count;
                // Byte offsets in original haystack.
                let orig_match_start = orig_chars
                    .get(match_char_start)
                    .copied()
                    .unwrap_or(haystack.len());
                let orig_match_end = orig_chars
                    .get(match_char_end)
                    .copied()
                    .unwrap_or(haystack.len());
                result.push_str(&html_escape(&haystack[orig_pos..orig_match_start]));
                result.push_str("<mark>");
                result.push_str(&html_escape(&haystack[orig_match_start..orig_match_end]));
                result.push_str("</mark>");
                // Advance past the match.
                let lower_match_end = lower_chars
                    .get(match_char_end)
                    .copied()
                    .unwrap_or(lower_haystack.len());
                orig_pos = orig_match_end;
                lower_pos = lower_match_end;
                char_pos = match_char_end;
            }
        }
    }
    result
}

/// Apply one JMAP patch key-value pair to a JSON object (RFC 8620 §5.3).
///
/// Keys may contain "/" separators naming a path into nested objects
/// (e.g. `"mailboxIds/abc123"`). Null values remove the target key; non-null
/// values overwrite or create it.  This is the JMAP patch format, which is
/// a superset of RFC 7396 flat merge-patch.
fn apply_jmap_patch(
    base: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: serde_json::Value,
) {
    if let Some(slash) = key.find('/') {
        let head = &key[..slash];
        let tail = &key[slash + 1..];
        if let Some(entry) = base.get_mut(head) {
            if let serde_json::Value::Object(inner) = entry {
                apply_jmap_patch(inner, tail, value);
            }
        } else if !value.is_null() {
            // Parent absent and value is non-null: create parent then set leaf.
            let mut inner = serde_json::Map::new();
            apply_jmap_patch(&mut inner, tail, value);
            base.insert(head.to_owned(), serde_json::Value::Object(inner));
        }
        // Parent absent and value is null: nothing to remove — no-op.
    } else if value.is_null() {
        base.remove(key);
    } else {
        base.insert(key.to_owned(), value);
    }
}

/// HTML-escape `&`, `<`, `>`.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ---------------------------------------------------------------------------
// FaultyBackend — injects BackendSetError::Other on demand
// ---------------------------------------------------------------------------

/// A thin wrapper around [`MemoryBackend`] that can inject
/// `BackendSetError::Other` for specific `(type_name, operation)` pairs.
///
/// Call [`FaultyBackend::inject`] before the operation under test. The first
/// matching call returns `BackendSetError::Other(MemoryError("injected …"))`;
/// the flag is cleared so subsequent calls go to the inner backend normally.
///
/// Valid `op` strings: `"create"`, `"update"`, `"destroy"`, `"import"`.
pub struct FaultyBackend {
    pub inner: MemoryBackend,
    failures:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<(&'static str, &'static str)>>>,
}

impl FaultyBackend {
    pub fn new() -> Self {
        Self {
            inner: MemoryBackend::new(),
            failures: Default::default(),
        }
    }

    /// Schedule a `BackendSetError::Other` for the next call to `op` on `type_name`.
    pub fn inject(&self, type_name: &'static str, op: &'static str) {
        self.failures.lock().unwrap().insert((type_name, op));
    }

    fn check(&self, type_name: &'static str, op: &'static str) -> bool {
        self.failures.lock().unwrap().remove(&(type_name, op))
    }
}

impl MailBackend for FaultyBackend {
    type Error = MemoryError;

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[O::Property]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        self.inner
            .get_objects::<O>(account_id, ids, properties)
            .await
    }

    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        if self.check(O::TYPE_NAME, "create") {
            return Err(BackendSetError::Other(MemoryError(
                "injected create error".to_owned(),
            )));
        }
        self.inner
            .create_object::<O>(account_id, create_id, obj)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        if self.check(O::TYPE_NAME, "update") {
            return Err(BackendSetError::Other(MemoryError(
                "injected update error".to_owned(),
            )));
        }
        self.inner.update_object::<O>(account_id, id, patch).await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        if self.check(O::TYPE_NAME, "destroy") {
            return Err(BackendSetError::Other(MemoryError(
                "injected destroy error".to_owned(),
            )));
        }
        self.inner.destroy_object::<O>(account_id, id).await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.inner.get_state::<O>(account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .get_changes::<O>(account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        self.inner
            .query_objects::<O>(account_id, filter, sort, limit, position)
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_query_state: &State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner
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

    async fn import_email(
        &self,
        account_id: &Id,
        blob_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[jmap_mail_types::Keyword],
        received_at: Option<&jmap_types::UTCDate>,
    ) -> Result<(Id, jmap_mail_types::Email), BackendSetError<Self::Error>> {
        if self.check("Email", "import") {
            return Err(BackendSetError::Other(MemoryError(
                "injected import error".to_owned(),
            )));
        }
        self.inner
            .import_email(account_id, blob_id, mailbox_ids, keywords, received_at)
            .await
    }

    async fn find_thread_by_message_ids(
        &self,
        account_id: &Id,
        message_ids: &[&str],
    ) -> Result<Option<Id>, Self::Error> {
        self.inner
            .find_thread_by_message_ids(account_id, message_ids)
            .await
    }

    async fn blob_exists(&self, account_id: &Id, blob_id: &Id) -> bool {
        self.inner.blob_exists(account_id, blob_id).await
    }

    async fn parse_email(
        &self,
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<jmap_mail_types::Email, Self::Error> {
        self.inner.parse_email(account_id, blob_id).await
    }

    async fn copy_email(
        &self,
        from_account_id: &Id,
        email_id: &Id,
        to_account_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[jmap_mail_types::Keyword],
        received_at: Option<&UTCDate>,
    ) -> Result<(Id, jmap_mail_types::Email), BackendSetError<Self::Error>> {
        self.inner
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
        self.inner
            .search_snippets(account_id, email_ids, filter)
            .await
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        self.inner.supports_type::<O>()
    }
}

// ---------------------------------------------------------------------------
// Email filter helpers (used by MemoryBackend::query_objects)
// ---------------------------------------------------------------------------

/// Apply a single `EmailFilterCondition` to an `Email`.
///
/// Only the fields most relevant for integration tests are implemented.
/// Unimplemented fields are silently treated as "no constraint" (always pass),
/// consistent with the note in RFC 8621 §4.4.1 that unspecified fields are
/// ignored.
fn email_matches_condition(email: &Email, cond: &EmailFilterCondition) -> bool {
    if let Some(ref mbox_id) = cond.in_mailbox {
        if !email.mailbox_ids.contains_key(mbox_id) {
            return false;
        }
    }
    if let Some(ref excluded) = cond.in_mailbox_other_than {
        // Email must be in at least one mailbox NOT in this list.
        let in_other = email.mailbox_ids.keys().any(|id| !excluded.contains(id));
        if !in_other {
            return false;
        }
    }
    if let Some(ref kw) = cond.has_keyword {
        if !email.keywords.contains_key(kw) {
            return false;
        }
    }
    if let Some(ref kw) = cond.not_keyword {
        if email.keywords.contains_key(kw) {
            return false;
        }
    }
    if let Some(want_attach) = cond.has_attachment {
        if email.has_attachment != want_attach {
            return false;
        }
    }
    // All specified conditions pass.
    true
}

/// Evaluate a full `EmailFilter` (which may be a logical combination of conditions).
fn email_matches_filter(email: &Email, filter: &EmailFilter) -> bool {
    match filter {
        Filter::Condition(cond) => email_matches_condition(email, cond),
        Filter::Operator(op) => match op.operator {
            Operator::And => op.conditions.iter().all(|f| email_matches_filter(email, f)),
            Operator::Or => op.conditions.iter().any(|f| email_matches_filter(email, f)),
            Operator::Not => !op.conditions.iter().any(|f| email_matches_filter(email, f)),
            _ => true, // unknown operator: no constraint
        },
        _ => true, // non_exhaustive: unknown variant, no constraint
    }
}
