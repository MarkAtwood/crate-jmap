# jmap-chat-mimi-client

Client side of draft-atwood-jmap-chat-mimi-00, the
`urn:ietf:params:jmap:chat:mimi` capability with which an MLS client uses a
JMAP Chat server as its MIMI provider. An extension trait over
`jmap-base-client`.

## What it is

| Item | Spec |
|---|---|
| `JmapMimiExt::with_mimi_session`, `SessionClient` | session binding to the primary `urn:ietf:params:jmap:chat:mimi` account |
| `SessionClient::mimi_capability` | §Account-Level Capability Object |
| `SessionClient::key_package_get` | §KeyPackage/get |
| `SessionClient::key_package_set` | §KeyPackage/set (create and destroy; the server rejects every update) |
| `SessionClient::key_material_fetch` | §KeyMaterial/fetch |
| `SessionClient::room_create` | §Room/create |
| `SessionClient::room_update` | §Room/update |
| `SessionClient::room_send` | §Room/send |
| `SessionClient::room_unbind` | §Room/unbind |
| `SessionClient::group_info_fetch` | §GroupInfo/fetch |
| `account_has_left` | §Chat.roomId: a commit removed the last of the account's clients, so call `Room/unbind` |
| `verify_group_info_response`, `VerifiedGroupInfoResponse`, `GroupInfoResponseError` | §GroupInfo/fetch: verify the GroupInfoResponse signature with `hub_sender` |
| `group_info_names_hub_sender` | §GroupInfo/fetch: the decrypted GroupInfo's `external_senders` contains `hub_sender` |

## What it's for

An MLS client of a MIMI provider. The client owns its MLS state (for
example an openmls `MlsGroup`) and its crypto provider; this crate carries
its MLS messages to the server and checks what comes back. It holds no MLS
secrets.

Not here yet: decrypting `encrypted_groupinfo_and_tree`.

## How to use

```rust,ignore
use jmap_chat_mimi_client::{
    account_has_left, group_info_names_hub_sender, verify_group_info_response, JmapMimiExt,
};

let sc = client.with_mimi_session(client.fetch_session().await?);
let user_uri = sc.mimi_capability()?.expect("MIMI account").user_uri;

// After merging a commit:
if account_has_left(group.members(), &user_uri) {
    sc.room_unbind(room_id).await?;
}

// Before an external join:
let fetched = sc.group_info_fetch(room_id, signed_request).await?;
let response_octets = base64url_decode(fetched.response.as_str());
let verified = verify_group_info_response(provider.crypto(), &response_octets)?;
// ... decrypt verified.encrypted_groupinfo_and_tree to a VerifiableGroupInfo ...
assert!(group_info_names_hub_sender(&group_info, &verified.hub_sender));
```

## Dependencies

`jmap-types`, `jmap-base-client`, `jmap-chat-mimi-types`, `serde`,
`serde_json`, crates.io `openmls =0.9.1` and `tls_codec =0.5.0`.

`openmls` is a public dependency, re-exported as
`jmap_chat_mimi_client::openmls`; use that path rather than pinning your
own copy. A new `openmls` minor version means a major release of this
crate.
