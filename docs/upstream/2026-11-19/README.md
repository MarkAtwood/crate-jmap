# Upstream drafts from crate-jmap (2026-11-19)

Nothing here has been filed, posted or sent. Mark reviews and sends.

| Draft | Goes to | Status | crate-jmap decisions |
|---|---|---|---|
| [serverfail-seterror.md](serverfail-seterror.md) | JMAP WG mailing list (jmap@ietf.org) | draft, not sent | 38 |

Notes:
- crate-jmap keeps per-object `serverFail` SetErrors until the discussion settles. The helper is `jmap_server::server_fail_value_from_backend`, which drops the backend error's `Display` text, and 19 server source files emit it.
- The blobext-03 quote is from its Security Considerations; its per-create rejections go in `notCreated` (§8.1 for Blob/convert).
