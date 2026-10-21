# jmap-chat-mimi-server

Server rules for draft-atwood-jmap-chat-mimi-00, the
`urn:ietf:params:jmap:chat:mimi` capability with which an MLS client uses a
JMAP Chat server as its MIMI provider. Backend-agnostic handlers for a
`jmap-server` `Dispatcher`; you supply storage through the `MimiBackend`
trait.

## What it is

| Item | Spec |
|---|---|
| `handle_key_package_get` | §KeyPackage/get (RFC 8620 §5.1) |
| `handle_key_package_set` | §KeyPackage/set: create, `forbidden` update, destroy |
| `check_key_package_create` | §KeyPackage/set create checks, §KeyPackage Object Fields `lastResort` |
| `signature_key_bound_elsewhere`, `key_package_ref_in_use`, `key_package_over_quota` | §KeyPackage/set account-wide rules |
| `MimiBackend`, `NewKeyPackage` | crypto provider, capability object, atomic KeyPackage create and destroy |
| `MimiBackend::claim_key_packages`, `KeyMaterialRequirements`, `KeyPackageFit` | §Handing Out KeyPackages: atomic claim, `clientUri`, KeyPackageRef retention |
| `memory::MemoryBackend` (`memory` feature) | in-memory reference backend |
| `register_mimi_handlers` | wires `KeyPackage/get` and `KeyPackage/set` |

## What it's for

A MIMI provider (mimid, or any JMAP Chat server that routes MLS messages
for its clients) wires these handlers next to `jmap-chat-server`. The crate
parses and verifies KeyPackages with `openmls`; it never holds MLS secrets
and never decrypts. `KeyMaterial/fetch`, `Room/*` and `GroupInfo/fetch` are
not here yet.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_chat_mimi_server::{memory::MemoryBackend, register_mimi_handlers};
use jmap_server::Dispatcher;
use openmls_rust_crypto::RustCrypto;

let backend = Arc::new(MemoryBackend::new(RustCrypto::default()));
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_mimi_handlers(&mut dispatcher, backend);
```

Advertise `urn:ietf:params:jmap:chat:mimi` in your Session with the same
account-level object that `MimiBackend::mimi_capability` returns.

## How it works

A create is decoded (`keyPackage` required, `lastResort` optional, nothing
else), then `check_key_package_create` decodes the base64url octets, parses
them as an RFC 9420 KeyPackage, verifies it with openmls and checks the
cipher suite against `cipherSuites`, the Basic credential and its identity
against `userUri`. It derives `lastResort` from the `app_data_dictionary`
extension's `last_resort_key_package` component and computes the
KeyPackageRef over the exact octets the client sent. Every failure is
`invalidProperties` naming `keyPackage`, or `lastResort` for a differing
claim.

`MimiBackend::create_key_package` then applies the account-wide rules in
one atomic step: the signature-key binding, KeyPackageRef reuse,
replacement of the client's previous last-resort KeyPackage, and
`maxKeyPackages`. These are uniqueness and count constraints over stored
state, so they belong in the backend's transaction, not in a handler-side
read that a concurrent create could race.

`MimiBackend::claim_key_packages` answers a key material request for the
owner the same way: in one atomic step it picks, for each client
(signature key bound to the account), a KeyPackage that
`KeyMaterialRequirements::fit` calls compatible, destroys it unless it is
the client's last-resort KeyPackage, and records its KeyPackageRef so a
later create of the same octets is refused. A client with only
incompatible KeyPackages is `nothingCompatible`; one with none left, or
only expired ones, is `keyMaterialExhausted`.

## Gotchas

- MSRV 1.91, set by `openmls` 0.9.1.
- Enable `extensions-draft` on every openmls-family crate in your build,
  for example `openmls_rust_crypto = { version = "=0.6.0", features =
  ["extensions-draft"] }`. This crate turns it on in `openmls`, which turns
  it on in `openmls_traits`, and a storage crate built without it then
  fails to compile.
- openmls 0.9.1 rejects a KeyPackage that carries a GREASE extension
  (RFC 9420 §13.5) with `UnsupportedExtension`, so such a KeyPackage is
  refused with `invalidProperties` naming `keyPackage` (bd:JMAP-6k5d.9).
- openmls checks the leaf node lifetime against the system clock and also
  refuses a lifetime that has not started yet.
- `lastResort` comes from the `app_data_dictionary` component of
  draft-ietf-mls-extensions-10 §6.4 only. openmls's
  `KeyPackage::last_resort`, which reads the older `last_resort` extension
  type, is not consulted.
- A KeyPackage refused by the per-KeyPackage checks never reaches the
  backend, so it binds no signature key.
- `KeyMaterialRequirements::fit` verifies the stored KeyPackage again, so
  a claim costs one signature verification per stored KeyPackage of the
  client examined.
- The `memory` backend keeps handed-out KeyPackageRefs forever; the draft
  lets a backend drop one once a Welcome naming it is delivered or its
  lifetime passes.

## References

- [draft-atwood-jmap-chat-mimi-00] — JMAP Chat MIMI provider
- [RFC 9420] — The Messaging Layer Security (MLS) Protocol
- [draft-ietf-mls-extensions-10] — The Messaging Layer Security (MLS) Extensions
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-chat-mimi-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 9420]: https://www.rfc-editor.org/rfc/rfc9420
[draft-ietf-mls-extensions-10]: https://datatracker.ietf.org/doc/draft-ietf-mls-extensions/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
