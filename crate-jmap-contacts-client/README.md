# jmap-contacts-client

Typed client methods for JMAP Contacts ([RFC 9610]). Depends
on [`jmap-base-client`] for transport, authentication, and session management.

## Usage

```rust
use jmap_base_client::JmapClient;
use jmap_contacts_client::{JmapContactsExt, AddressBookSetParams};
use jmap_contacts_types::ContactCard;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build a base client (handles auth, HTTP, session fetch).
    let client = JmapClient::new("https://jmap.example.com/.well-known/jmap")?;

    // 2. Fetch the JMAP session document.
    let session = client.fetch_session().await?;

    // 3. Bind the session to a contacts client.
    let contacts = client.with_contacts_session(session);

    // 4. Fetch all address books.
    let response = contacts.address_book_get(None, None).await?;
    for ab in &response.list {
        println!("{}: {}", ab.id, ab.name);
    }

    // 5. Fetch all contact cards.
    let cards = contacts.contact_card_get(None, None).await?;
    for card in &cards.list {
        if let Some(id) = &card.id {
            println!("card id={}", id);
        }
    }

    // 6. Create a new ContactCard.
    //    JSContact sub-objects (name, emails, etc.) must be built as
    //    serde_json::Value — no typed builder is provided (see Known Limitations).
    let create_card = json!({
        "newCard": {
            "addressBookIds": { "ab-id-here": true },
            "version": "1.0",
            "uid": "urn:uuid:f81d4fae-7dec-11d0-a765-00a0c91e6bf6",
            "name": {
                "components": [
                    { "kind": "given", "value": "Jane" },
                    { "kind": "surname", "value": "Smith" }
                ],
                "isOrdered": true
            },
            "emails": {
                "e1": {
                    "contexts": { "work": true },
                    "address": "jane@example.com"
                }
            }
        }
    });
    let set_resp = contacts
        .contact_card_set(Some(create_card), None, None)
        .await?;
    if let Some(created) = set_resp.created {
        if let Some(card) = created.get("newCard") {
            println!("created card id={:?}", card.id);
        }
    }

    // 7. Dismiss a share notification (destroy-only).
    contacts.share_notification_set(Some(vec!["notif-id-1"])).await?;

    Ok(())
}
```

After constructing a `SessionClient` via `with_contacts_session`, all JMAP
Contacts methods are available without passing `&Session` on every call. If the
session expires, re-fetch with `JmapClient::fetch_session` and construct a new
`SessionClient`.

## Registered methods

All method implementations live on `SessionClient` in the `methods/` submodules.

| Method | Function | Returns |
|---|---|---|
| `AddressBook/get` | `address_book_get` | `GetResponse<AddressBook>` |
| `AddressBook/changes` | `address_book_changes` | `ChangesResponse` |
| `AddressBook/set` | `address_book_set` | `SetResponse<AddressBook>` |
| `ContactCard/get` | `contact_card_get` | `GetResponse<ContactCard>` |
| `ContactCard/changes` | `contact_card_changes` | `ChangesResponse` |
| `ContactCard/set` | `contact_card_set` | `SetResponse<ContactCard>` |
| `ContactCard/copy` | `contact_card_copy` | `SetResponse<ContactCard>` |
| `ContactCard/query` | `contact_card_query` | `QueryResponse` |
| `ContactCard/queryChanges` | `contact_card_query_changes` | `QueryChangesResponse` |

Note: `AddressBook/query` and `AddressBook/queryChanges` are not implemented —
RFC 9610 does not define these methods.

### AddressBook/set — extra arguments

`address_book_set` accepts an optional `AddressBookSetParams` for the
contacts-specific method-level arguments:

```rust
pub struct AddressBookSetParams {
    /// When true, ContactCards belonging only to a destroyed AddressBook
    /// are also destroyed; cards in multiple books are detached.
    pub on_destroy_remove_contents: Option<bool>,

    /// Designates one AddressBook as the account default after all other
    /// operations succeed (RFC 9610 §2.3).
    pub on_success_set_is_default: Option<serde_json::Value>,
}
```

Pass `None` for `params` (or `Some(Default::default())`) when neither argument
is needed.

## How it works

Every method follows the same five-step pattern:

1. Validate arguments (empty-string guards return `InvalidArgument` before any
   network call).
2. Call `session_parts()` to extract `(api_url, account_id)` from the bound
   session.
3. Build the JMAP method arguments as a `serde_json::Value`.
4. Call `build_request(method_name, args, USING_CONTACTS)` to construct a
   single-method `JmapRequest`.
5. POST to the API URL and extract the typed response via
   `jmap_base_client::extract_response`.

The capability `using` array for all contacts requests is:
`["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:contacts"]`.

## Known Limitations

- Creating or updating a `ContactCard` requires the caller to build
  `serde_json::Value` for all JSContact sub-objects (`name`, `phones`,
  `addresses`, `emails`, etc.) manually. No typed builder or fluent API is
  provided. Use RFC 9553 as the schema when constructing these values.
- `contact_card_query` and `contact_card_query_changes` accept `filter` and
  `sort` as untyped `serde_json::Value`. Use `ContactCardFilterCondition` and
  `ContactCardComparator` from `jmap-contacts-types` and serialize them with
  `serde_json::to_value` before passing.
- No integration tests against a real JMAP server. Tests use direct
  serialization assertions against known JSON shapes from the spec.

## Crate family

```
jmap-types
    └── jmap-base-client         transport, auth, session
            └── jmap-contacts-client  ← this crate
                    (also depends on jmap-contacts-types for response types)
```

## References

- **[RFC 9610]** — JMAP Contacts (normative for all method
  names, argument shapes, and response formats)
- **[RFC 9553]** — JSContact (normative for ContactCard sub-object schemas)
- **[RFC 8620]** — JMAP Core (request format, response shapes, `/get`, `/set`,
  `/changes`, `/query`, `/queryChanges`, `/copy`)

[RFC 9610]: https://www.rfc-editor.org/rfc/rfc9610
[RFC 9553]: https://www.rfc-editor.org/rfc/rfc9553
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-base-client`]: ../crate-jmap-base-client

## License

MIT OR Apache-2.0
