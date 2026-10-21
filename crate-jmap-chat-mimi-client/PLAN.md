# jmap-chat-mimi-client — Plan

Client side of draft-atwood-jmap-chat-mimi-00 (epic bd:JMAP-6k5d), on top
of `jmap-chat-mimi-types` and `jmap-base-client`.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-6k5d.8 | Crate scaffold, Room/* and GroupInfo/fetch bindings, `account_has_left`, GroupInfoResponse checks | landed |

## Decisions

- Bindings take and return the `jmap-chat-mimi-types` argument types; the
  account is the session's primary `urn:ietf:params:jmap:chat:mimi`
  account. `using` carries `urn:ietf:params:jmap:chat` as well, because the
  MIMI capability is meaningful only with it (draft §Capability).
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
- Decrypting `encrypted_groupinfo_and_tree` (HPKE with the client's
  `groupInfoPublicKey` private key) and parsing the GroupInfo, ratchet tree
  option and pending proposals inside it is left to the caller for now.
