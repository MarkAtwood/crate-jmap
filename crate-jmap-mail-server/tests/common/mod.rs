//! Shared test infrastructure — MemoryBackend in-memory MailBackend implementation.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code warnings are suppressed because not all items are used in every binary.
#![allow(dead_code)]
#![allow(async_fn_in_trait)]

pub mod seed;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// MIME body parsing (jmap-mime + mime-tree) — used in import_email and parse_email.
use jmap_mime::message_to_jmap_body;

use jmap_mail_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, MailBackend, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
};
use jmap_mail_types::{
    query::{
        ComparatorProperty, EmailComparator, EmailFilter, EmailSubmissionFilter, Filter, Operator,
    },
    submission::{EmailSubmission, EmailSubmissionFilterCondition},
    Email, EmailAddress, EmailFilterCondition, EmailHeader, Keyword, SearchSnippet,
};
use jmap_types::{Id, State, UTCDate};

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// A change log entry for one state transition.
#[derive(Clone, Debug)]
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
    objects: HashMap<(&'static str, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(&'static str, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(&'static str, String), Vec<ChangeEntry>>,
    /// blob_id → raw bytes (used by import_email and parse_email)
    blobs: HashMap<Id, Vec<u8>>,
    /// account_id → (message_id_string → email_id) for duplicate detection in import_email
    message_id_index: HashMap<String, HashMap<String, Id>>,
}

impl Inner {
    fn current_state(&self, type_name: &'static str, account_id: &str) -> u64 {
        *self
            .states
            .get(&(type_name, account_id.to_owned()))
            .unwrap_or(&0)
    }

    fn bump_state(&mut self, type_name: &'static str, account_id: &str) -> u64 {
        let entry = self
            .states
            .entry((type_name, account_id.to_owned()))
            .or_insert(0);
        *entry += 1;
        *entry
    }

    fn objects_mut(
        &mut self,
        type_name: &'static str,
        account_id: &str,
    ) -> &mut HashMap<Id, serde_json::Value> {
        self.objects
            .entry((type_name, account_id.to_owned()))
            .or_default()
    }

    fn objects_ref(
        &self,
        type_name: &'static str,
        account_id: &str,
    ) -> Option<&HashMap<Id, serde_json::Value>> {
        self.objects.get(&(type_name, account_id.to_owned()))
    }

    /// Re-sort a Thread's `emailIds` by the `receivedAt` of each member email.
    ///
    /// RFC 8621 §3 requires `emailIds` to be sorted oldest-first by `receivedAt`.
    /// Called after every insertion of a new email into an existing thread.
    fn sort_thread_email_ids(&mut self, account_id: &str, thread_id: &Id) {
        // Collect current emailIds from the stored Thread JSON.
        let email_ids: Vec<String> = match self.objects_ref("Thread", account_id) {
            Some(store) => match store.get(thread_id) {
                Some(v) => v["emailIds"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|e| e.as_str().map(|s| s.to_owned()))
                            .collect()
                    })
                    .unwrap_or_default(),
                None => return,
            },
            None => return,
        };

        // Look up receivedAt for each email id from the Email store.
        let mut id_and_date: Vec<(String, i64)> = email_ids
            .into_iter()
            .map(|eid| {
                let epoch = self
                    .objects_ref("Email", account_id)
                    .and_then(|s| s.get(eid.as_str()))
                    .and_then(|v| v["receivedAt"].as_str())
                    .map(rfc3339_to_epoch_secs)
                    .unwrap_or(0);
                (eid, epoch)
            })
            .collect();

        // Sort ascending by UTC epoch so non-UTC offsets compare correctly.
        id_and_date.sort_by(|a, b| a.1.cmp(&b.1));

        // Write the sorted list back to the Thread object.
        if let Some(store) = self.objects.get_mut(&("Thread", account_id.to_owned())) {
            if let Some(thread_val) = store.get_mut(thread_id) {
                if let Some(arr) = thread_val["emailIds"].as_array_mut() {
                    *arr = id_and_date
                        .into_iter()
                        .map(|(eid, _)| serde_json::Value::String(eid))
                        .collect();
                }
            }
        }
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
#[derive(Debug, Clone)]
enum IdFate {
    Created,
    Updated,
    Destroyed,
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

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
    pub fn store_blob(&self, blob_id: &Id, bytes: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap();
        inner.blobs.insert(blob_id.clone(), bytes);
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
// JmapBackend impl (read-side)
// ---------------------------------------------------------------------------

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;

    // -----------------------------------------------------------------------
    // get_objects
    // -----------------------------------------------------------------------

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
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
            for val in store.values() {
                let obj: O = serde_json::from_value(val.clone())
                    .map_err(|e| MemoryError(format!("deserialize {}: {e}", O::TYPE_NAME)))?;
                found.push(obj);
            }
        }

        Ok((found, not_found))
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
                .get(&(O::TYPE_NAME, account_id.to_string()))
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
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        // Collect and sort IDs outside the lock for deterministic ordering.
        // For Email and EmailSubmission objects, apply filter conditions in-process
        // using a JSON roundtrip (since O::Filter: Serialize, we can recover the
        // typed filter).
        let email_filter: Option<EmailFilter> = if O::TYPE_NAME == "Email" {
            filter.and_then(|f| {
                serde_json::to_value(f)
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok())
            })
        } else {
            None
        };

        // Decode EmailComparator list for Email queries (JSON roundtrip via O::Comparator).
        let email_sort: Option<Vec<EmailComparator>> = if O::TYPE_NAME == "Email" {
            sort.and_then(|s| {
                serde_json::to_value(s)
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok())
            })
        } else {
            None
        };

        let submission_filter: Option<EmailSubmissionFilter> = if O::TYPE_NAME == "EmailSubmission"
        {
            filter.and_then(|f| {
                serde_json::to_value(f)
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok())
            })
        } else {
            None
        };

        // Pre-build the inMailboxOtherThan exclusion set once for top-level Condition
        // filters to avoid O(N×k) HashSet allocations inside the per-email loop.
        let top_level_excluded_set: Option<std::collections::HashSet<&Id>> =
            email_filter.as_ref().and_then(|ef| {
                if let Filter::Condition(cond) = ef {
                    cond.in_mailbox_other_than
                        .as_ref()
                        .map(|v| v.iter().collect())
                } else {
                    None
                }
            });

        // Collect (id, receivedAt) pairs so we can sort by receivedAt when requested.
        let (mut id_date_pairs, state_n) = {
            let inner = self.inner.lock().unwrap();
            let pairs: Vec<(Id, String)> = if let Some(ref ef) = email_filter {
                // Apply email filter: deserialize each stored object and check.
                inner
                    .objects_ref(O::TYPE_NAME, account_id.as_ref())
                    .map(|map| {
                        map.iter()
                            .filter_map(|(id, val)| {
                                let email: Email = serde_json::from_value(val.clone()).ok()?;
                                if email_matches_filter(&email, ef, top_level_excluded_set.as_ref())
                                {
                                    let received = val
                                        .get("receivedAt")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_owned();
                                    Some((id.clone(), received))
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else if let Some(ref sf) = submission_filter {
                // Pre-build per-condition sets once before the per-submission loop so
                // that HashSet allocation is O(1) per filter, not O(N) per submission.
                let top_level_sub_sets: Option<SubmissionConditionSets<'_>> =
                    if let Filter::Condition(cond) = sf {
                        Some(SubmissionConditionSets::from_condition(cond))
                    } else {
                        None
                    };
                // Apply submission filter: deserialize each stored object and check.
                inner
                    .objects_ref(O::TYPE_NAME, account_id.as_ref())
                    .map(|map| {
                        map.iter()
                            .filter_map(|(id, val)| {
                                let sub: EmailSubmission =
                                    serde_json::from_value(val.clone()).ok()?;
                                let matches = match (sf, &top_level_sub_sets) {
                                    (Filter::Condition(cond), Some(sets)) => {
                                        submission_matches_condition(&sub, cond, sets)
                                    }
                                    _ => submission_matches_filter(&sub, sf),
                                };
                                if matches {
                                    Some((id.clone(), String::new()))
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
                    .map(|s| {
                        s.iter()
                            .map(|(id, val)| {
                                let received = val
                                    .get("receivedAt")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_owned();
                                (id.clone(), received)
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let state_n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
            (pairs, state_n)
        };

        // Apply sort. When a receivedAt comparator is present, sort by epoch seconds
        // so that sub-second timestamps (e.g. "T00:00:00.123Z") sort correctly relative
        // to whole-second timestamps (e.g. "T00:00:00Z") — lexicographic order is wrong
        // because '.' (0x2E) < 'Z' (0x5A).  Ties broken by id string for stable ordering.
        let received_at_sort = email_sort.as_deref().and_then(|s| {
            s.iter()
                .find(|c| c.property == ComparatorProperty::ReceivedAt)
        });
        if let Some(cmp) = received_at_sort {
            let ascending = cmp.is_ascending;
            id_date_pairs.sort_by(|(id_a, date_a), (id_b, date_b)| {
                let epoch_a = rfc3339_to_epoch_secs(date_a);
                let epoch_b = rfc3339_to_epoch_secs(date_b);
                let ord = epoch_a.cmp(&epoch_b);
                let ord = if ascending { ord } else { ord.reverse() };
                ord.then_with(|| id_a.as_ref().cmp(id_b.as_ref()))
            });
        } else {
            id_date_pairs.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));
        }
        let all_ids: Vec<Id> = id_date_pairs.into_iter().map(|(id, _)| id).collect();

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
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        // Step 1: Validate since_query_state by parsing it as a u64 counter.
        // An unparseable token means the client supplied a state we never issued;
        // return cannotCalculateChanges (limit=0) per RFC 8620 §5.6.
        let _since: u64 = since_query_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::TooManyChanges { limit: 0 })?;

        // Step 2: Get the raw delta (created/updated/destroyed) since the given state.
        let changes = self
            .get_changes::<O>(account_id, since_query_state, None)
            .await?;
        let new_query_state = changes.new_state.clone();

        // Step 3: Get the current filtered+sorted result list (no pagination).
        let query_result = self
            .query_objects::<O>(account_id, filter, sort, None, 0)
            .await
            .map_err(BackendChangesError::Other)?;
        let current_result: Vec<Id> = query_result.ids;

        // Step 4: Build lookup sets.
        use std::collections::HashSet;
        let current_set: HashSet<&Id> = current_result.iter().collect();
        let created_set: HashSet<&Id> = changes.created.iter().collect();
        let updated_set: HashSet<&Id> = changes.updated.iter().collect();

        // Step 5: Compute removed — IDs that were destroyed or updated out of the filter.
        // An updated ID that still passes the filter is NOT removed; it appears in added instead.
        let mut removed: Vec<Id> = Vec::new();
        for id in changes.destroyed.iter().chain(changes.updated.iter()) {
            if !current_set.contains(id) {
                removed.push(id.clone());
            }
        }
        // Deduplicate removed (an id could appear in both destroyed and updated in theory).
        removed.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        removed.dedup();

        // Step 6: Compute added — IDs in created ∪ updated that are in the current result.
        // We iterate current_result (already sorted) to get correct positional indices.
        // Determine the up_to_id cutoff position in current_result (exclusive upper bound).
        let up_to_pos: Option<usize> =
            up_to_id.and_then(|target| current_result.iter().position(|id| id == target));

        let mut added: Vec<AddedItem> = Vec::new();
        for (pos, id) in current_result.iter().enumerate() {
            // If up_to_id is set, stop before reaching (and including) its position.
            if let Some(cutoff) = up_to_pos {
                if pos >= cutoff {
                    break;
                }
            }
            if created_set.contains(id) || updated_set.contains(id) {
                added.push(AddedItem::new(id.clone(), pos as u64));
            }
        }

        // Step 7: Apply max_changes — if total changes exceed the limit, return
        // cannotCalculateChanges (limit=0) per RFC 8620 §5.6.
        if let Some(max) = max_changes {
            let total_changes = removed.len() as u64 + added.len() as u64;
            if total_changes > max {
                return Err(BackendChangesError::TooManyChanges { limit: 0 });
            }
        }

        Ok(QueryChangesResult::new(
            since_query_state.clone(),
            new_query_state,
            None,
            removed,
            added,
        ))
    }
}

// ---------------------------------------------------------------------------
// MailBackend impl (write-side and mail-specific)
// ---------------------------------------------------------------------------

impl MailBackend for MemoryBackend {
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
        // Update size from the serialized JSON length. email.rs sets size=0 as a
        // placeholder (it has no raw bytes on the Email/set create path); the backend
        // is responsible for assigning the real value. MemoryBackend uses the
        // serialized-JSON byte length as a proxy — non-zero and stable within a test.
        if val.get("size").and_then(|v| v.as_u64()) == Some(0) {
            if let serde_json::Value::Object(ref mut map) = val {
                let json_size = serde_json::to_vec(&serde_json::Value::Object(map.clone()))
                    .map(|b| b.len() as u64)
                    .unwrap_or(1);
                map.insert(
                    "size".to_owned(),
                    serde_json::Value::Number(json_size.into()),
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
            .entry((O::TYPE_NAME, account_id.to_string()))
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
            .entry((O::TYPE_NAME, account_id.to_string()))
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
            .entry((O::TYPE_NAME, account_id.to_string()))
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

        // Build the Email object outside the lock (uses only local data).
        let email_id = Id::from(uuid::Uuid::new_v4().to_string());
        let mailbox_map: HashMap<Id, bool> =
            mailbox_ids.iter().map(|id| (id.clone(), true)).collect();
        let kw_map: HashMap<Keyword, bool> = keywords.iter().map(|k| (k.clone(), true)).collect();

        let received = received_at
            .cloned()
            .unwrap_or_else(|| UTCDate::from("1970-01-01T00:00:00Z"));

        // thread_id is a placeholder; set below after the lock is acquired.
        // We must build a placeholder email first so we can serialize it, then
        // patch in the real thread_id inside the lock.
        // Actually: build everything except thread_id, then acquire one lock for
        // duplicate check + thread assignment + insert (no TOCTOU window).
        let email_size = bytes.len() as u64;

        // Acquire a single lock that covers the duplicate check, thread assignment,
        // and the actual insert — eliminating the TOCTOU race window.
        let (email, email_id) = {
            let mut inner = self.inner.lock().unwrap();

            // Check for duplicate Message-ID (RFC 8621 §4.8).
            if let Some(msg_ids) = &parsed.message_id {
                if let Some(index) = inner.message_id_index.get(account_id.as_ref()) {
                    for msg_id in msg_ids {
                        if let Some(existing_id) = index.get(msg_id) {
                            return Err(BackendSetError::SetError(
                                SetError::new(SetErrorType::AlreadyExists)
                                    .with_existing_id(existing_id.clone()),
                            ));
                        }
                    }
                }
            }

            // Assign thread: look for existing email with matching message-id.
            let thread_id =
                assign_thread_inner(&inner, account_id, &parsed.in_reply_to, &parsed.references);

            // Build the full Email object now that we have the real thread_id.
            let mut email = Email::new(
                email_id.clone(),
                blob_id.clone(),
                thread_id.clone(),
                mailbox_map,
                email_size,
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
            email.cc = parsed.cc;
            email.headers = parsed.raw_headers;
            if let Some(preview) = parsed.preview {
                email.preview = Some(preview);
            }

            // Populate body structure fields using the MIME parser.
            if let Ok(parsed_msg) = mime_tree::parse(&bytes) {
                let part_counter = std::cell::Cell::new(0usize);
                let blob_id_str = blob_id.to_string();
                let body_fields = message_to_jmap_body(&parsed_msg, |_part| {
                    let i = part_counter.get();
                    part_counter.set(i + 1);
                    jmap_types::Id::from(format!("{blob_id_str}-part-{i}"))
                });
                email.text_body = body_fields.text_body;
                email.html_body = body_fields.html_body;
                email.attachments = body_fields.attachments.clone();
                email.body_structure = Some(body_fields.body_structure);
                email.has_attachment = !body_fields.attachments.is_empty();
                if email.preview.is_none() {
                    email.preview = body_fields.preview;
                }
            }

            // Ensure the Thread object exists.
            let thread_val = serde_json::json!({
                "id": thread_id.to_string(),
                "emailIds": [email_id.to_string()]
            });

            // Serialize the email for storage.
            let email_val = serde_json::to_value(&email).map_err(|e| {
                BackendSetError::Other(MemoryError(format!("serialize email: {e}")))
            })?;

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

            // Re-sort the thread's emailIds by receivedAt ascending (RFC 8621 §3).
            // Only needed when joining an existing thread; new threads have one element.
            if thread_existed {
                inner.sort_thread_email_ids(account_id.as_ref(), &thread_id);
            }

            // Update Message-ID index for future duplicate detection.
            if let Some(msg_ids) = &email.message_id {
                let account_index = inner
                    .message_id_index
                    .entry(account_id.to_string())
                    .or_default();
                for msg_id in msg_ids {
                    account_index.insert(msg_id.clone(), email_id.clone());
                }
            }

            // Bump state for both Email and Thread.
            let new_email_state = inner.bump_state("Email", account_id.as_ref());
            inner
                .change_log
                .entry(("Email", account_id.to_string()))
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
                .entry(("Thread", account_id.to_string()))
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

            (email, email_id)
        };

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
        email.cc = parsed.cc;
        email.headers = parsed.raw_headers;
        if let Some(preview) = parsed.preview {
            email.preview = Some(preview);
        }

        // Populate body structure fields using the MIME parser.
        if let Ok(parsed_msg) = mime_tree::parse(&bytes) {
            let part_counter = std::cell::Cell::new(0usize);
            let blob_id_str = blob_id.to_string();
            let body_fields = message_to_jmap_body(&parsed_msg, |_part| {
                let i = part_counter.get();
                part_counter.set(i + 1);
                jmap_types::Id::from(format!("{blob_id_str}-part-{i}"))
            });
            email.text_body = body_fields.text_body;
            email.html_body = body_fields.html_body;
            email.attachments = body_fields.attachments.clone();
            email.body_structure = Some(body_fields.body_structure);
            email.has_attachment = !body_fields.attachments.is_empty();
            if email.preview.is_none() {
                email.preview = body_fields.preview;
            }
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
        new_email.cc = src_email.cc.clone();
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

            // Re-sort the thread's emailIds by receivedAt ascending (RFC 8621 §3).
            if thread_existed {
                inner.sort_thread_email_ids(to_account_id.as_ref(), &thread_id);
            }

            let new_email_state = inner.bump_state("Email", to_account_id.as_ref());
            inner
                .change_log
                .entry(("Email", to_account_id.to_string()))
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
                .entry(("Thread", to_account_id.to_string()))
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
    // batch_destroy_emails
    // -----------------------------------------------------------------------

    async fn batch_destroy_emails(
        &self,
        account_id: &Id,
        email_ids: &[Id],
    ) -> Vec<(Id, Option<BackendSetError<Self::Error>>)> {
        let mut inner = self.inner.lock().unwrap();
        let account_str = account_id.to_string();
        let mut results = Vec::with_capacity(email_ids.len());
        for id in email_ids {
            let removed = inner
                .objects
                .get_mut(&("Email", account_str.clone()))
                .and_then(|store| store.remove(id))
                .is_some();
            let err = if removed {
                let new_state = inner.bump_state("Email", &account_str);
                inner
                    .change_log
                    .entry(("Email", account_str.clone()))
                    .or_default()
                    .push(ChangeEntry {
                        new_state,
                        created: vec![],
                        updated: vec![],
                        destroyed: vec![id.clone()],
                    });
                None
            } else {
                Some(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )))
            };
            results.push((id.clone(), err));
        }
        results
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
    cc: Option<Vec<EmailAddress>>,
    /// Short preview of the body (first 256 bytes of the text body, if any).
    preview: Option<String>,
    /// Raw header fields in order, for `Email.headers` (RFC 8621 §4.1.3).
    raw_headers: Vec<EmailHeader>,
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

    // Build raw_headers: fold continuation lines back into the preceding header.
    // A line beginning with whitespace is a continuation of the previous header value.
    let mut raw_headers: Vec<EmailHeader> = Vec::new();
    for line in header_block.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line — append to previous header value.
            if let Some(last) = raw_headers.last_mut() {
                last.value.push('\n');
                last.value.push_str(line);
            }
        } else if let Some(colon_pos) = line.find(':') {
            let name = line[..colon_pos].to_owned();
            let value = line[colon_pos + 1..].to_owned();
            raw_headers.push(EmailHeader::new(name, value));
        }
        // Lines with no colon and no leading whitespace (malformed) are skipped.
    }

    // Unfold header lines for the structured field extraction below.
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
    let mut cc_header: Option<String> = None;

    for line in unfolded.lines() {
        // RFC 5322 §2.2: header field names are case-insensitive. Split on the
        // first ':' and compare the field name case-insensitively.
        if let Some(colon) = line.find(':') {
            let name = &line[..colon];
            let rest = &line[colon + 1..];
            if name.eq_ignore_ascii_case("Subject") {
                subject = Some(rest.trim().to_owned());
            } else if name.eq_ignore_ascii_case("Message-ID") {
                let ids = extract_msg_ids(rest);
                if !ids.is_empty() {
                    message_id = Some(ids);
                }
            } else if name.eq_ignore_ascii_case("In-Reply-To") {
                in_reply_to = extract_msg_ids(rest);
            } else if name.eq_ignore_ascii_case("References") {
                references = extract_msg_ids(rest);
            } else if name.eq_ignore_ascii_case("From") {
                from_header = Some(rest.trim().to_owned());
            } else if name.eq_ignore_ascii_case("To") {
                to_header = Some(rest.trim().to_owned());
            } else if name.eq_ignore_ascii_case("Cc") {
                cc_header = Some(rest.trim().to_owned());
            }
        }
    }

    let from = from_header.as_deref().map(parse_address_list);
    let to = to_header.as_deref().map(parse_address_list);
    let cc = cc_header.as_deref().map(parse_address_list);

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
        cc,
        preview,
        raw_headers,
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

/// Parse an RFC 3339 timestamp string to seconds since the Unix epoch (UTC).
///
/// Handles both `Z` suffix and `+HH:MM` / `-HH:MM` offsets so that timestamps
/// with non-UTC offsets sort correctly by absolute UTC time.
///
/// Returns `0` for any string that cannot be parsed (treated as epoch origin
/// for sorting purposes — keeps the sort stable for malformed inputs).
///
/// Limitations (acceptable for test code):
/// - Does not validate calendar date/time fields (e.g. month 13 is accepted).
/// - Does not handle leap seconds.
/// - Year must be in the range 1970–9999.
fn rfc3339_to_epoch_secs(s: &str) -> i64 {
    try_rfc3339_to_epoch_secs(s).unwrap_or(0)
}

/// Inner fallible parser; returns `None` on any parse error.
fn try_rfc3339_to_epoch_secs(s: &str) -> Option<i64> {
    // Expected format: YYYY-MM-DDTHH:MM:SS[.fff](Z|+HH:MM|-HH:MM)
    // Length with Z offset: 20 chars; with millis+Z: 24 chars; with ±HH:MM: 25 chars.
    let s = s.trim();
    if s.len() < 20 {
        return None;
    }

    let year: i64 = s[0..4].parse().ok()?;
    if s.as_bytes()[4] != b'-' {
        return None;
    }
    let month: i64 = s[5..7].parse().ok()?;
    if s.as_bytes()[7] != b'-' {
        return None;
    }
    let day: i64 = s[8..10].parse().ok()?;
    if !matches!(s.as_bytes()[10], b'T' | b't') {
        return None;
    }
    let hour: i64 = s[11..13].parse().ok()?;
    if s.as_bytes()[13] != b':' {
        return None;
    }
    let minute: i64 = s[14..16].parse().ok()?;
    if s.as_bytes()[16] != b':' {
        return None;
    }
    let second: i64 = s[17..19].parse().ok()?;

    // Skip optional fractional seconds (.NNN or .NNNNNN etc.) before the offset.
    let frac_skip = if s.as_bytes().get(19) == Some(&b'.') {
        let frac_end = s[20..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| 20 + i)
            .unwrap_or(s.len());
        frac_end - 19
    } else {
        0
    };
    let offset_start = 19 + frac_skip;

    let offset_str = &s[offset_start..];
    let offset_secs: i64 = if offset_str.eq_ignore_ascii_case("z") {
        0
    } else if offset_str.len() == 6
        && (offset_str.starts_with('+') || offset_str.starts_with('-'))
        && offset_str.as_bytes()[3] == b':'
    {
        let sign: i64 = if offset_str.starts_with('-') { -1 } else { 1 };
        let oh: i64 = offset_str[1..3].parse().ok()?;
        let om: i64 = offset_str[4..6].parse().ok()?;
        sign * (oh * 3600 + om * 60)
    } else {
        return None;
    };

    // Days-since-epoch calculation using the proleptic Gregorian calendar.
    // Number of days from 1970-01-01 to year-01-01 (ignoring this year's months/days).
    let y = year - 1;
    let leap_days = y / 4 - y / 100 + y / 400;
    // 477 = number of leap days from year 1 to year 1969 inclusive (1969/4 - 1969/100 + 1969/400).
    let days_to_year_start = y * 365 + leap_days - (1969 * 365 + 477);

    // Days within the year up to the start of the month.
    let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut days_in_year: i64 = 0;
    for m in 0..(month - 1) as usize {
        let extra = if m == 1 && is_leap { 1 } else { 0 };
        days_in_year += MONTH_DAYS[m] + extra;
    }

    let total_days = days_to_year_start + days_in_year + (day - 1);
    let utc_secs = total_days * 86400 + hour * 3600 + minute * 60 + second - offset_secs;
    Some(utc_secs)
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

impl JmapBackend for FaultyBackend {
    type Error = MemoryError;

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        self.inner
            .get_objects::<O>(account_id, ids, properties)
            .await
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
}

// ---------------------------------------------------------------------------
// MailBackend impl for FaultyBackend (write-side and mail-specific)
// ---------------------------------------------------------------------------

impl MailBackend for FaultyBackend {
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
///
/// `excluded_set` is a pre-built `HashSet` for the `inMailboxOtherThan` check,
/// built once before the per-email loop to avoid O(N×k) allocations.
/// Pass `None` to have the set built on the spot (correct but not optimal).
fn email_matches_condition(
    email: &Email,
    cond: &EmailFilterCondition,
    excluded_set: Option<&std::collections::HashSet<&Id>>,
) -> bool {
    if let Some(ref mbox_id) = cond.in_mailbox {
        if !email.mailbox_ids.contains_key(mbox_id) {
            return false;
        }
    }
    if cond.in_mailbox_other_than.is_some() {
        // Email must be in at least one mailbox NOT in the exclusion list.
        // Use the pre-built set when available; build on demand otherwise.
        let owned: Option<std::collections::HashSet<&Id>>;
        let set: &std::collections::HashSet<&Id> = match excluded_set {
            Some(s) => s,
            None => {
                owned = Some(
                    cond.in_mailbox_other_than
                        .as_ref()
                        .unwrap()
                        .iter()
                        .collect(),
                );
                owned.as_ref().unwrap()
            }
        };
        let in_other = email.mailbox_ids.keys().any(|id| !set.contains(id));
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
    if let Some(ref before) = cond.before {
        // receivedAt must be strictly before `before` (epoch-seconds comparison avoids
        // the lexicographic trap where "T00:00:00.123Z" < "T00:00:00Z" despite .123 being later).
        let recv_epoch = rfc3339_to_epoch_secs(email.received_at.as_ref());
        let before_epoch = try_rfc3339_to_epoch_secs(before.as_ref()).unwrap_or(i64::MAX);
        if recv_epoch >= before_epoch {
            return false;
        }
    }
    if let Some(ref after) = cond.after {
        // receivedAt must be strictly after `after`.
        let recv_epoch = rfc3339_to_epoch_secs(email.received_at.as_ref());
        let after_epoch = try_rfc3339_to_epoch_secs(after.as_ref()).unwrap_or(i64::MIN);
        if recv_epoch <= after_epoch {
            return false;
        }
    }
    if let Some(min) = cond.min_size {
        if email.size < min {
            return false;
        }
    }
    // All specified conditions pass.
    true
}

/// Evaluate a full `EmailFilter` (which may be a logical combination of conditions).
///
/// `excluded_set` is a pre-built `HashSet` for `inMailboxOtherThan` in the
/// top-level condition. Pass `None` for nested conditions.
fn email_matches_filter(
    email: &Email,
    filter: &EmailFilter,
    excluded_set: Option<&std::collections::HashSet<&Id>>,
) -> bool {
    match filter {
        Filter::Condition(cond) => email_matches_condition(email, cond, excluded_set),
        Filter::Operator(op) => match op.operator {
            Operator::And => op
                .conditions
                .iter()
                .all(|f| email_matches_filter(email, f, None)),
            Operator::Or => op
                .conditions
                .iter()
                .any(|f| email_matches_filter(email, f, None)),
            Operator::Not => !op
                .conditions
                .iter()
                .any(|f| email_matches_filter(email, f, None)),
            _ => true, // unknown operator: no constraint
        },
        _ => true, // non_exhaustive: unknown variant, no constraint
    }
}

// ---------------------------------------------------------------------------
// EmailSubmission filter helpers (used by MemoryBackend::query_objects)
// ---------------------------------------------------------------------------

/// Pre-built lookup sets for a single `EmailSubmissionFilterCondition`.
///
/// Constructed once per filter (not once per submission) so that the
/// `identityIds`, `emailIds`, and `threadIds` HashSets are not re-allocated
/// on every iteration of the per-submission loop.
struct SubmissionConditionSets<'a> {
    identity_ids: Option<std::collections::HashSet<&'a Id>>,
    email_ids: Option<std::collections::HashSet<&'a Id>>,
    thread_ids: Option<std::collections::HashSet<&'a Id>>,
}

impl<'a> SubmissionConditionSets<'a> {
    fn from_condition(cond: &'a EmailSubmissionFilterCondition) -> Self {
        Self {
            identity_ids: cond.identity_ids.as_ref().map(|v| v.iter().collect()),
            email_ids: cond.email_ids.as_ref().map(|v| v.iter().collect()),
            thread_ids: cond.thread_ids.as_ref().map(|v| v.iter().collect()),
        }
    }
}

/// Apply a single `EmailSubmissionFilterCondition` to an `EmailSubmission`.
///
/// All fields are optional; unset fields are treated as "no constraint" per
/// RFC 8621 §7.3.
///
/// `sets` must be pre-built from the same `cond` via
/// `SubmissionConditionSets::from_condition` before the per-submission loop.
fn submission_matches_condition(
    sub: &EmailSubmission,
    cond: &EmailSubmissionFilterCondition,
    sets: &SubmissionConditionSets<'_>,
) -> bool {
    if let Some(ref id_set) = sets.identity_ids {
        if !id_set.contains(&sub.identity_id) {
            return false;
        }
    }
    if let Some(ref id_set) = sets.email_ids {
        if !id_set.contains(&sub.email_id) {
            return false;
        }
    }
    if let Some(ref id_set) = sets.thread_ids {
        if !id_set.contains(&sub.thread_id) {
            return false;
        }
    }
    if let Some(ref status) = cond.undo_status {
        if &sub.undo_status != status {
            return false;
        }
    }
    if let Some(ref before) = cond.before {
        // sendAt must be strictly before `before` (lexicographic ISO 8601 comparison).
        if sub.send_at.as_ref() >= before.as_ref() {
            return false;
        }
    }
    if let Some(ref after) = cond.after {
        // sendAt must be on or after `after`.
        if sub.send_at.as_ref() < after.as_ref() {
            return false;
        }
    }
    true
}

/// Evaluate a full `EmailSubmissionFilter` (which may be a logical combination).
///
/// For `Filter::Condition`, the caller is responsible for pre-building
/// `SubmissionConditionSets` before the per-submission loop and passing it here
/// via the inner helper; for operator nodes the sets are built on demand per
/// nested condition (the operator case is uncommon in tests).
fn submission_matches_filter(sub: &EmailSubmission, filter: &EmailSubmissionFilter) -> bool {
    match filter {
        Filter::Condition(cond) => {
            let sets = SubmissionConditionSets::from_condition(cond);
            submission_matches_condition(sub, cond, &sets)
        }
        Filter::Operator(op) => match op.operator {
            Operator::And => op
                .conditions
                .iter()
                .all(|f| submission_matches_filter(sub, f)),
            Operator::Or => op
                .conditions
                .iter()
                .any(|f| submission_matches_filter(sub, f)),
            Operator::Not => !op
                .conditions
                .iter()
                .any(|f| submission_matches_filter(sub, f)),
            _ => true, // unknown operator: no constraint
        },
        _ => true, // non_exhaustive: unknown variant, no constraint
    }
}
