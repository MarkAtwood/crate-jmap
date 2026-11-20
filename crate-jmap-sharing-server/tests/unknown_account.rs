//! bd:JMAP-ayoz.36 (and ayoz.2): no backend mutation registers an
//! account. On an account nobody registered, create fails, update
//! and destroy are `notFound` (RFC 8620 §5.3: it has no objects), and
//! `account_exists` stays false (RFC 8620 §3.6.2 would otherwise stop
//! answering `accountNotFound` for it).
//!
//! Oracle: the two RFC 8620 sections above; expected outcomes hand-written.

mod common;

use common::MemoryBackend;
use jmap_server::{BackendSetError, JmapAccounts, SetErrorType};
use jmap_sharing_server::SharingBackend;
use jmap_sharing_types::Principal;
use jmap_types::Id;

fn object(id: &Id) -> Principal {
    let mut v: serde_json::Value = serde_json::json!({ "type": "individual", "name": "Ada", "description": null, "email": null, "timeZone": null, "capabilities": {}, "accounts": null });
    v["id"] = serde_json::json!(id);
    serde_json::from_value(v).expect("fixture must decode")
}

#[tokio::test]
async fn mutations_never_register_an_account() {
    let backend = MemoryBackend::<()>::default();
    let ghost = Id::from("ghost");

    let created = backend
        .create_object::<Principal, _>(&(), &ghost, "c", object)
        .await;
    assert!(
        matches!(created, Err(BackendSetError::Other(_))),
        "{created:?}"
    );

    let not_found = |r: Result<(), BackendSetError<_>>| matches!(r, Err(BackendSetError::SetError(se)) if se.error_type == SetErrorType::NotFound);
    assert!(not_found(
        backend
            .destroy_object::<Principal>(&(), &ghost, &Id::from("x"))
            .await
    ));
    assert!(not_found(
        backend
            .update_object::<Principal>(&(), &ghost, &Id::from("x"), Default::default())
            .await
            .map(|_| ())
    ));

    assert!(!backend.account_exists(&(), &ghost).await.unwrap());
}
