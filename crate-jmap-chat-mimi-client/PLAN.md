# jmap-chat-mimi-client — Plan

Client side of draft-atwood-jmap-chat-mimi-00 (epic bd:JMAP-6k5d), on top
of `jmap-chat-mimi-types` and `jmap-base-client`.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-6k5d.8 | Crate scaffold, Room/* and GroupInfo/fetch bindings, `account_has_left`, GroupInfoResponse checks | landed |
| JMAP-6k5d.12 | KeyPackage/get, KeyPackage/set and KeyMaterial/fetch bindings | landed |
| JMAP-6k5d.13 | Decrypt and parse the GroupInfoResponse GroupInfoRatchetTreeTBE | landed |

## Decisions

- Bindings take and return the `jmap-chat-mimi-types` argument types; the
  account is the session's primary `urn:ietf:params:jmap:chat:mimi`
  account. `using` carries `urn:ietf:params:jmap:chat` as well, because the
  MIMI capability is meaningful only with it (draft §Capability).
- `key_package_get` requests every property: `KeyPackage` has no
  optional field, so a `properties` subset would not decode.
  `key_package_set` takes `create` and `destroy` only, as the server
  rejects every update (draft §KeyPackage/set).
- A hub's `responseCode` or `status` refusal is a successful method call;
  only JMAP method errors (`invalidArguments`, `serverUnavailable`) are
  `ClientError::MethodError`.
- `account_has_left` takes the openmls `Member`s of the new epoch and
  compares Basic credential identities with `userUri`; a non-Basic leaf is
  never the account's, as the server accepts only Basic credentials
  (draft §KeyPackage/set).
- `verify_group_info_response` parses draft-ietf-mimi-protocol-06 §5.6 by
  hand with tls_codec and verifies RFC 9420 §5.1.2 VerifyWithLabel over the
  received octets in front of `signature`. openmls keeps the
  `ExternalSender` accessors crate-private, so its two fields are read
  here.
- `group_info_names_hub_sender` reads `external_senders` from the
  GroupContext extensions (RFC 9420 §12.1.8.1) of an unverified
  `VerifiableGroupInfo`; openmls verifies the GroupInfo signature at the
  external join.
- `decrypt_group_info_and_tree` opens `encrypted_groupinfo_and_tree` with
  RFC 9420 §5.1.3 DecryptWithLabel ("GroupInfo and ratchet_tree
  encryption", context `room_id`) under the HPKE algorithms of the
  response's `cipher_suite`, the only suite draft-ietf-mimi-protocol-06
  §5.6 puts in front of the ciphertext. openmls keeps its
  `decrypt_with_label` crate-private, so the EncryptContext is built here
  and passed to `OpenMlsCrypto::hpke_open`.
- A `full` RatchetTreeOption parses as openmls `RatchetTreeIn`, the
  `ratchet_tree` extension encoding (`optional<Node>` entries) that
  draft-mahy-mls-ratchet-tree-options-04 §3 says `full` carries; its
  struct writes `Node ratchet_tree<V>`, but its prose says "the complete
  `ratchet_tree` extension". An `httpsUrl` that is not UTF-8 is malformed.
