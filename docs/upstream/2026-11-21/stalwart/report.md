<!--
Posted 2026-11-21 by Mark (via the Discourse API) as https://support.stalw.art/t/2470 (Stalwart takes bug
reports there, not as GitHub issues). crate-jmap decision 67; bd JMAP-cyub.5.
Captured 2026-11-21 against Stalwart 1.1.1 (image digest in
scripts/stalwart-up.sh). Quote checked against RFC 8620 (line 2023 of the RFC text).
The other deviation (unknownCapability answered as notRequest) is
already reported and fixed upstream: support.stalw.art topic 2445,
maintainer reply 2026-11-21, ships in the next weekly release. The account id `b` and the state strings are from
that run.
-->

**Title:** Identity/set and EmailSubmission/set responses missing oldState/newState

Stalwart 1.1.1, official Docker image, default RocksDB store, JMAP over HTTP.

Identity/set responses never include `oldState`. When an Identity/set or EmailSubmission/set changes nothing (for example a destroy of an unknown id), the response has neither `oldState` nor `newState`. RFC 8620 §5.3 requires both in every /set response (`oldState` may be null). Mailbox/set and Email/set return them as expected.

Requests use `urn:ietf:params:jmap:core`, `:mail` and `:submission`.

Update:

```json
["Identity/set",{"accountId":"b","update":{"b":{"name":"Alice A"}}},"c0"]
```
```json
["Identity/set",{"accountId":"b","newState":"sai","updated":{"b":null}},"c0"]
```

Destroy of unknown ids:

```json
["Identity/set",{"accountId":"b","destroy":["nonexistent"]},"c0"]
["EmailSubmission/set",{"accountId":"b","destroy":["nonexistent"]},"c0"]
```
```json
["Identity/set",{"accountId":"b","notDestroyed":{"nonexistent":{"type":"notFound"}}},"c0"]
["EmailSubmission/set",{"accountId":"b","notDestroyed":{"nonexistent":{"type":"notFound"}}},"c0"]
```

The same destroy on Mailbox/set:

```json
["Mailbox/set",{"accountId":"b","oldState":"sae","newState":"sae","notDestroyed":{"nonexistent":{"type":"notFound"}}},"c0"]
```

A client that treats `newState` as required can't parse the no-op responses.

Found with crate-jmap's interop tests: https://github.com/MarkAtwood/crate-jmap
