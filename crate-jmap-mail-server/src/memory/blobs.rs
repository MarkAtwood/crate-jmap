//! Sharing a blob store with another memory backend (a deployment that
//! also serves draft-ietf-jmap-blobext-03 or RFC 9404 from its own store,
//! as the testjig does), and the blob references this backend's objects
//! hold for that store.
//!
//! Without [`MemoryBackend::with_blob_store`] the backend keeps blobs in a
//! map of its own, as before. With it, every blob the backend writes (a
//! created Email's message, an MDN) goes to that store, and every blob it
//! reads (Email/import, Email/parse, body parts, MDN/parse) comes from
//! there, so an uploaded blob can be imported and a blob destroyed there
//! is gone here. Two stores that call each other share one scope gate
//! ([`MemoryBackend::with_scope_gate`]).
//!
//! The other direction is [`MemoryBackend::current_blob_references`],
//! [`MemoryBackend::blob_has_current_reference`] and
//! [`MemoryBackend::blob_destroyed`], for the blob store's reference
//! hook. They count current objects only: object-history versions are not
//! references (draft-ietf-jmap-object-history-03 §3.1), and a version
//! whose blob is destroyed is dropped.

use std::collections::BTreeMap;
use std::sync::Arc;

use jmap_mail_types::Email;
use jmap_server::{JmapObject, ScopeGate, ScopedState};
use jmap_types::Id;
use serde_json::Value;

use super::{Inner, MemoryBackend};
use jmap_server::MemoryCaller;

/// A blob store [`MemoryBackend`] reads and writes instead of its own map
/// ([`MemoryBackend::with_blob_store`]).
///
/// The backend calls it with its own state released, so an implementation
/// may call back into the backend's reference methods.
pub trait ExternalBlobStore: Send + Sync {
    /// The whole of `blob_id` in `account_id`, or `None` when there is no
    /// such blob.
    fn read(&self, account_id: &Id, blob_id: &Id) -> Option<Vec<u8>>;

    /// Store `bytes` as `blob_id` (an id this backend minted) in
    /// `account_id`, with media type `media_type`.
    fn store(&self, account_id: &Id, blob_id: &Id, bytes: Vec<u8>, media_type: &str);
}

/// The optional [`ExternalBlobStore`], with a `Debug` that names it.
#[derive(Clone, Default)]
pub(super) struct BlobStore(pub(super) Option<Arc<dyn ExternalBlobStore>>);

impl std::fmt::Debug for BlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "BlobStore(Some)"
        } else {
            "BlobStore(None)"
        })
    }
}

/// Whether `v` holds `"blobId": blob_id` anywhere (an Email's own blob,
/// a body part's, a `SieveScript`'s).
fn mentions_blob(v: &Value, blob_id: &str) -> bool {
    match v {
        Value::Object(m) => m.iter().any(|(k, v)| {
            (k == "blobId" && v.as_str() == Some(blob_id)) || mentions_blob(v, blob_id)
        }),
        Value::Array(a) => a.iter().any(|v| mentions_blob(v, blob_id)),
        _ => false,
    }
}

impl<C: MemoryCaller> MemoryBackend<C> {
    /// Read and write blobs through `store` instead of the backend's own
    /// map. Call before cloning the backend and before storing any blob.
    #[must_use]
    pub fn with_blob_store(mut self, store: Arc<dyn ExternalBlobStore>) -> Self {
        self.blob_store = BlobStore(Some(store));
        self
    }

    /// Take `gate` for this backend's conditional scopes, shared with the
    /// state of a backend this one calls ([`ScopedState::with_gate`]):
    /// with separate gates, two scopes opened in opposite orders deadlock.
    /// Call before cloning the backend.
    #[must_use]
    pub fn with_scope_gate(self, gate: Arc<ScopeGate>) -> Self {
        let value = std::mem::take(&mut *self.inner.lock_unscoped());
        Self {
            inner: ScopedState::with_gate(value, gate),
            blob_store: self.blob_store,
            caller: std::marker::PhantomData,
        }
    }

    /// The gate this backend's conditional scopes take.
    pub fn scope_gate(&self) -> Arc<ScopeGate> {
        Arc::clone(self.inner.gate())
    }

    /// The bytes of `blob_id`: from the shared store when there is one,
    /// else the backend's own map.
    pub(super) async fn blob_bytes(&self, account_id: &Id, blob_id: &Id) -> Option<Vec<u8>> {
        match &self.blob_store.0 {
            Some(store) => store.read(account_id, blob_id),
            None => self.inner.lock().await.blobs.get(blob_id).cloned(),
        }
    }

    /// Store `bytes` as `blob_id`: in the shared store when there is one
    /// (and then before `inner` is taken), else in `inner`'s map.
    pub(super) fn put_blob(
        &self,
        inner: Option<&mut Inner>,
        account_id: &Id,
        blob_id: Id,
        bytes: Vec<u8>,
        media_type: &str,
    ) {
        match (&self.blob_store.0, inner) {
            (Some(store), _) => store.store(account_id, &blob_id, bytes, media_type),
            (None, Some(inner)) => {
                inner.blobs.insert(blob_id, bytes);
            }
            (None, None) => {
                self.inner.lock_unscoped().blobs.insert(blob_id, bytes);
            }
        }
    }

    /// For each of `blob_ids`, the current `type_name` objects of
    /// `account_id` that reference it (blobext-03 §6): an Email whose own
    /// blob or a body part's blob it is, the Mailboxes and the Thread of
    /// such Emails, and a `SieveScript`'s script blob. Current objects only
    /// (object-history-03 §3.1). For a shared blob store's reference hook;
    /// does not wait on the scope gate.
    pub fn current_blob_references(
        &self,
        account_id: &Id,
        type_name: &str,
        blob_ids: &[Id],
    ) -> BTreeMap<Id, Vec<Id>> {
        let inner = self.inner.lock_unscoped();
        let of = |t: &str| {
            inner
                .objects
                .get(t)
                .and_then(|m| m.get(account_id.as_ref()))
        };
        let mut out = BTreeMap::new();
        for blob_id in blob_ids {
            let emails: Vec<(&Id, &Value)> = of(Email::TYPE_NAME)
                .into_iter()
                .flatten()
                .filter(|(_, v)| mentions_blob(v, blob_id.as_ref()))
                .collect();
            let mut ids: Vec<Id> = match type_name {
                "Email" => emails.iter().map(|(id, _)| (*id).clone()).collect(),
                "Mailbox" => emails
                    .iter()
                    .filter_map(|(_, v)| v.get("mailboxIds")?.as_object())
                    .flat_map(|m| m.keys().map(|k| Id::from(k.as_str())))
                    .collect(),
                "Thread" => emails
                    .iter()
                    .filter_map(|(_, v)| v.get("threadId")?.as_str().map(Id::from))
                    .collect(),
                other => of(other)
                    .into_iter()
                    .flatten()
                    .filter(|(_, v)| mentions_blob(v, blob_id.as_ref()))
                    .map(|(id, _)| id.clone())
                    .collect(),
            };
            ids.sort();
            ids.dedup();
            out.insert(blob_id.clone(), ids);
        }
        out
    }

    /// Whether a current object of any type in `account_id` references
    /// `blob_id`; such a blob cannot be destroyed (blobext-03 §4
    /// `blobHasReference`). Does not wait on the scope gate.
    pub fn blob_has_current_reference(&self, account_id: &Id, blob_id: &Id) -> bool {
        let inner = self.inner.lock_unscoped();
        inner.objects.values().any(|accounts| {
            accounts
                .get(account_id.as_ref())
                .is_some_and(|m| m.values().any(|v| mentions_blob(v, blob_id.as_ref())))
        })
    }

    /// `blob_id` was destroyed in the shared store: discard the history of
    /// every object whose newest retained version references it. Every
    /// version of an Email shares its blob, so a destroyed Email whose
    /// blob is gone leaves `includeDestroyed`, `onlyDestroyed` and
    /// `restore` (object-history-03 §3.1, -04 §3.1). Does not wait on the
    /// scope gate.
    pub fn blob_destroyed(&self, account_id: &Id, blob_id: &Id) {
        let mut inner = self.inner.lock_unscoped();
        if let Some(history) = inner.history.as_mut() {
            for t in super::history::SUPPORTED {
                history.purge_where(t, account_id, |v| mentions_blob(v, blob_id.as_ref()));
            }
        }
    }
}
