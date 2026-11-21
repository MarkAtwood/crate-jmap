<!--
Posted 2026-11-21 by Mark (via the Discourse API) as https://support.stalw.art/t/2474
(Issues, category 6) with the user API key in rbw. crate-jmap decisions
67/68; bd JMAP-cyub.5. Captured 2026-11-21 against Stalwart 1.1.1
(image digest in scripts/stalwart-up.sh), account id `b`. Forum searched
first: notJSON and createdIds are not reported; isSubscribed's default
was discussed in topics 1509 and 2073 (both treat false as intended);
Mark dropped it from this report (Stalwart chose it deliberately). RFC text checked: RFC 8620 lines 1091,
929-931, 986-991; RFC 8621 lines 696-701.
-->

**Title:** JMAP: notRequest instead of notJSON, and createdIds handling

Stalwart 1.1.1, official Docker image, default RocksDB store, JMAP over HTTP. Two small conformance points found with crate-jmap's interop tests.

**1. A body that isn't JSON gets notRequest, not notJSON (RFC 8620 §3.6.1).**

```
POST /jmap/   {not json
```
```
HTTP/1.1 400 Bad Request
{"type":"urn:ietf:params:jmap:error:notRequest","status":400,"detail":"{not json"}
```

§3.6.1 has `notJSON` for "the request did not parse as I-JSON"; `notRequest` is for JSON that doesn't match the Request type.

**2. createdIds (RFC 8620 §3.3).**

An empty `createdIds` in the request is not echoed in the response; §3.3 says "If given in the request, the response will also include a createdIds property."

```json
{"using":["urn:ietf:params:jmap:core"],"methodCalls":[["Core/echo",{},"c0"]],"createdIds":{}}
```

The response has `methodResponses` and `sessionState` but no `createdIds`. A non-empty map such as `{"k":"a"}` is echoed.

A `createdIds` value containing `-` makes the whole request fail, although `-` and `_` are valid Id characters (§1.2):

```json
{"using":["urn:ietf:params:jmap:core"],"methodCalls":[["Core/echo",{},"c0"]],"createdIds":{"k":"zz9-x"}}
```
```
HTTP/1.1 400 Bad Request
{"type":"urn:ietf:params:jmap:error:notRequest","status":400,"detail":"{\"using\":...,\"createdIds\":{\"k\":\"zz9-x\"}}"}
```

`{"k":"zz9x"}` works.

Found with crate-jmap's interop tests: https://github.com/MarkAtwood/crate-jmap
