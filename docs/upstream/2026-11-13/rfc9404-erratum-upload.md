<!--
DRAFT, not submitted.
Target: RFC Editor errata report for RFC 9404 (https://www.rfc-editor.org/errata.php, "Report New Errata").
Checked 2026-11-13: https://www.rfc-editor.org/errata/rfc9404 reports "No matching errata found."
crate-jmap decision: 21(iii).
Type chosen: Editorial (reason in Notes). If Mark prefers Technical, the case for it is that the examples
are what implementers copy, and "The result is the same as for Foo/set" can be read as supporting them.
-->

**RFC:** 9404, JSON Meta Application Protocol (JMAP) Blob Management Extension

**Type:** Editorial

**Section:** 4.1.2 (and 4.2.2)

**Original Text:**

Section 4.1.2, both Blob/upload responses:

```
       "Blob/upload",
       {
         "oldState": null,
         "created": {
           "b4": {
```

```
       "Blob/upload",
       {
         "oldState": null,
         "created": {
           "cat": {
```

Section 4.2.2, the Blob/upload response:

```
       "Blob/upload",
       {
         "oldState": null,
         "created": {
           "b2": {
             "id": "G2aae6c35c94fcfb415dbe95f408b9ce91ee846ed",
             "size": 11,
             "type": "application/octet-stream"
           },
           "b1": {
             "id": "G72cfa4804194563685d9a4b695f7ba20e7739576",
             "size": 43,
             "type": "text/plain"
           }
         },
         "updated": null,
         "destroyed": null,
         "notCreated": null,
         "notUpdated": null,
         "notDestroyed": null,
         "accountId": "account1"
       },
```

**Corrected Text:**

Section 4.1.2, both Blob/upload responses:

```
       "Blob/upload",
       {
         "created": {
           "b4": {
```

```
       "Blob/upload",
       {
         "created": {
           "cat": {
```

Section 4.2.2, the Blob/upload response:

```
       "Blob/upload",
       {
         "created": {
           "b2": {
             "id": "G2aae6c35c94fcfb415dbe95f408b9ce91ee846ed",
             "size": 11,
             "type": "application/octet-stream"
           },
           "b1": {
             "id": "G72cfa4804194563685d9a4b695f7ba20e7739576",
             "size": 43,
             "type": "text/plain"
           }
         },
         "notCreated": null,
         "accountId": "account1"
       },
```

**Notes:**

Section 4.1 says:

```
   This is similar to a Foo/set in [RFC8620] in some ways.  However,
   blobs cannot be updated or deleted, so only create is allowed in the
   method call.  Also, blobs do not have state, so there is no state
   field present in the method response.
```

The Blob/upload responses in the Section 4.1.2 and 4.2.2 examples include "oldState": null, a state field the text says is not present, and the Section 4.2.2 response also includes the update and destroy maps ("updated", "destroyed", "notUpdated", "notDestroyed") for operations the text says are not allowed. The Section 4.1.1 example response has none of these and matches the text. The corrected examples drop those properties and keep "accountId", "created" and "notCreated", as in Section 4.1's "*Result*" ("with created and notCreated objects mapping from the creation id").

Type: Editorial, because the normative text is unambiguous and the extra properties are all null, so a client written to the text is not affected by them; the fix is confined to the examples. (A reader could argue Technical, since "The result is the same as for Foo/set in [RFC8620]" might be taken to support the examples; if the verifier reads it that way, the clarification would be to that sentence: "The result has the accountId, created, and notCreated properties of a Foo/set response".)

Found while implementing RFC 9404 in crate-jmap (https://github.com/MarkAtwood/crate-jmap), whose Blob/upload response has accountId, created and notCreated only, following the Section 4.1 text.
