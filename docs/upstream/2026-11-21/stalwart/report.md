<!--
Draft for Mark to post at https://support.stalw.art (Stalwart takes bug
reports there, not as GitHub issues). crate-jmap decision 67; bd JMAP-cyub.5.
Captured 2026-11-21 against Stalwart 1.1.1 (image digest in
scripts/stalwart-up.sh). Quotes checked against RFC 8620 (lines 1087 and
2023 of the RFC text). The account id `b` and the state strings are from
that run.
-->

**Title:** JMAP: two RFC 8620 conformance gaps (`unknownCapability`, and `oldState`/`newState` in `/set` responses)

### Summary

Running crate-jmap's interop tests (https://github.com/MarkAtwood/crate-jmap) against Stalwart turned up two places where responses differ from RFC 8620:

1. A request whose `using` lists an unsupported capability gets `notRequest` instead of `unknownCapability` (RFC 8620 §3.6.1).
2. Some `Identity/set` and `EmailSubmission/set` responses omit `oldState` and/or `newState` (RFC 8620 §5.3).

### Environment

- Stalwart 1.1.1 (official container image), default RocksDB store.
- Plain JMAP over HTTP with Basic auth; no client library involved.

---

### 1. Unsupported capability: `notRequest` instead of `unknownCapability`

RFC 8620 §3.6.1 defines a specific error for this case:

> `urn:ietf:params:jmap:error:unknownCapability`: The client included a capability in the "using" property of the request that the server does not support.

A client uses it to tell "this server lacks an extension" apart from "my request is malformed"; `notRequest` says the second.

Request:

```http
POST /jmap/ HTTP/1.1
Content-Type: application/json

{"using":["urn:ietf:params:jmap:core","urn:example:nope"],
 "methodCalls":[["Core/echo",{"hello":true},"c0"]]}
```

Actual:

```http
HTTP/1.1 400 Bad Request

{"type":"urn:ietf:params:jmap:error:notRequest","status":400,
 "detail":"{\"using\":[\"urn:ietf:params:jmap:core\",\"urn:example:nope\"],\"methodCalls\":[[\"Core/echo\",{\"hello\":true},\"c0\"]]}"}
```

Expected: the same status with `"type":"urn:ietf:params:jmap:error:unknownCapability"` (the `detail` wording is up to the server; the current one echoes the whole request body back, which is not much of an explanation).

---

### 2. `Identity/set` and `EmailSubmission/set` omit `oldState` / `newState`

RFC 8620 §5.3 says every `Foo/set` response has:

> - **oldState**: `String|null` The state string that would have been returned by "Foo/get" before making the requested changes, or null if the server doesn't know what the previous state string was.
> - **newState**: `String` The state string that will now be returned by "Foo/get".

Neither is optional: `newState` is always present, and `oldState` is present but may be `null`. `Mailbox/set` and `Email/set` follow this; `Identity/set` and `EmailSubmission/set` do not:

- `Identity/set` never includes `oldState`.
- A successful `EmailSubmission/set` create includes `newState` but not `oldState`.
- An `Identity/set` or `EmailSubmission/set` that changes nothing (for example, only a failed destroy) includes neither.

Clients that follow RFC 8620 strictly fail to parse these responses (the required `newState` is missing), and clients that keep state for `Foo/changes` lose track of it.

In each request below, `using` is `["urn:ietf:params:jmap:core","urn:ietf:params:jmap:mail","urn:ietf:params:jmap:submission"]`.

Update an Identity:

```json
["Identity/set",{"accountId":"b","update":{"b":{"name":"Alice A"}}},"c0"]
```

```json
["Identity/set",{"accountId":"b","newState":"sai","updated":{"b":null}},"c0"]
```

(no `oldState`)

Destroy a non-existent Identity, then a non-existent EmailSubmission:

```json
["Identity/set",{"accountId":"b","destroy":["nonexistent"]},"c0"]
["EmailSubmission/set",{"accountId":"b","destroy":["nonexistent"]},"c0"]
```

```json
["Identity/set",{"accountId":"b","notDestroyed":{"nonexistent":{"type":"notFound"}}},"c0"]
["EmailSubmission/set",{"accountId":"b","notDestroyed":{"nonexistent":{"type":"notFound"}}},"c0"]
```

(neither `oldState` nor `newState`)

For comparison, `Mailbox/set` with the same no-op destroy is correct:

```json
["Mailbox/set",{"accountId":"b","oldState":"sae","newState":"sae","notDestroyed":{"nonexistent":{"type":"notFound"}}},"c0"]
```

Expected: every `Identity/set` and `EmailSubmission/set` response includes `oldState` (the state before, or `null`) and `newState` (the current state, unchanged when nothing changed), as `Mailbox/set` does.
