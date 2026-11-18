//! The memory backend's `ExternalReferences` hook (bd JMAP-tna1.3.15):
//! references held by another store's objects count in `Blob/lookup`,
//! `blobHasReference` and `expires`, and a destroy is reported back.
//!
//! Oracles: blobext-03 §4 ("The server MUST reject any blob that is still
//! referenced by another object", `blobHasReference`), §6 (Blob/lookup),
//! object-history-03 §3.1 (a version whose blob was discarded "MUST NOT"
//! be returned, so the other store must learn of the destroy).

#![cfg(feature = "memory")]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use jmap_blob_server::BlobBackend;
use jmap_blob_server::memory::{ExternalReferences, MemoryBackend};
use jmap_server::BackendSetError;
use jmap_types::Id;

#[derive(Default)]
struct Fake {
    referenced: Mutex<bool>,
    destroyed: Mutex<Vec<Id>>,
}

impl ExternalReferences for Fake {
    fn references(&self, _: &Id, type_name: &str, blob_ids: &[Id]) -> BTreeMap<Id, Vec<Id>> {
        let on = *self.referenced.lock().unwrap();
        blob_ids
            .iter()
            .map(|b| {
                (
                    b.clone(),
                    if on && type_name == "Email" {
                        vec![Id::from("E1")]
                    } else {
                        vec![]
                    },
                )
            })
            .collect()
    }
    fn is_referenced(&self, _: &Id, _: &Id) -> bool {
        *self.referenced.lock().unwrap()
    }
    fn blob_destroyed(&self, _: &Id, blob_id: &Id) {
        self.destroyed.lock().unwrap().push(blob_id.clone());
    }
}

#[tokio::test]
async fn external_references_count_and_destroys_are_reported() {
    let acc = Id::from("A");
    let blob = Id::from("shared-1");
    let fake = Arc::new(Fake::default());
    let b = MemoryBackend::new()
        .with_account(&acc)
        .with_external_references(Arc::clone(&fake) as Arc<dyn ExternalReferences>);
    assert!(b.store_blob(&acc, &blob, b"hello".to_vec(), Some("text/plain".into())));
    assert_eq!(b.blob_bytes(&acc, &blob).as_deref(), Some(&b"hello"[..]));

    *fake.referenced.lock().unwrap() = true;
    let refs = b
        .blob_references(&(), &acc, "Email", std::slice::from_ref(&blob))
        .await
        .unwrap();
    assert_eq!(refs[&blob], [Id::from("E1")]);
    let meta = b.blob_info(&(), &acc, &blob).await.unwrap().unwrap();
    assert_eq!(meta.expires, None, "referenced: no expiry");
    match b.destroy_blob(&(), &acc, &blob).await {
        Err(BackendSetError::SetError(e)) => assert_eq!(e.error_type.as_str(), "blobHasReference"),
        other => panic!("{other:?}"),
    }
    assert!(fake.destroyed.lock().unwrap().is_empty());

    *fake.referenced.lock().unwrap() = false;
    b.destroy_blob(&(), &acc, &blob).await.unwrap();
    assert_eq!(*fake.destroyed.lock().unwrap(), std::slice::from_ref(&blob));
    assert_eq!(b.blob_bytes(&acc, &blob), None);
}
