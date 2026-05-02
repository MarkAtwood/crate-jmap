# jmap-contacts-client — Implementation Plan

RFC 8620 JMAP Contacts method implementations on top of `jmap-base-client`.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-contacts-10.txt`

## Crate Family Position

```
jmap-types
    └── jmap-base-client
            └── jmap-contacts-client  ← this crate
```

## What This Crate Is

Extension trait `JmapContactsExt` over `jmap_base_client::JmapClient` that adds typed
methods for every JMAP Contacts operation.

## Planned Public API

```rust
pub trait JmapContactsExt {
    async fn address_book_get(&self, account_id: &Id, ids: Option<&[Id]>)
        -> Result<GetResponse<AddressBook>, ClientError>;
    async fn contact_get(&self, account_id: &Id, ids: Option<&[Id]>, props: &[&str])
        -> Result<GetResponse<Contact>, ClientError>;
    async fn contact_set(&self, account_id: &Id, req: SetRequest<Contact>)
        -> Result<SetResponse<Contact>, ClientError>;
    async fn contact_query(&self, account_id: &Id, req: ContactQueryRequest)
        -> Result<QueryResponse, ClientError>;
    // ... all AddressBook, Contact, ContactGroup methods
}
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-client/` — identical extension trait pattern.
