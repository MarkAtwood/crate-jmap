# jmap-sharing-client — Implementation Plan

JMAP Sharing method implementations on top of `jmap-base-client`.

## Spec

- `~/PROJECT/jmap-chat-spec/references/rfc9670.txt` — normative

## Crate Family Position

```
jmap-types
    └── jmap-base-client
            └── jmap-sharing-client  ← this crate
```

## What This Crate Is

Extension trait `JmapSharingExt` over `jmap_base_client::JmapClient` that adds typed
methods for `Principal/*` and `ShareNotification/*`.

## Planned Public API

```rust
pub trait JmapSharingExt {
    async fn principal_get(&self, account_id: &Id, ids: Option<&[Id]>)
        -> Result<GetResponse<Principal>, ClientError>;
    async fn principal_query(&self, account_id: &Id, req: PrincipalQueryRequest)
        -> Result<QueryResponse, ClientError>;
    async fn share_notification_get(&self, account_id: &Id, ids: Option<&[Id]>)
        -> Result<GetResponse<ShareNotification>, ClientError>;
    async fn share_notification_set(&self, account_id: &Id, req: SetRequest<ShareNotification>)
        -> Result<SetResponse<ShareNotification>, ClientError>;
    // ... all Principal and ShareNotification methods
}
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-client/` — identical extension trait pattern.
