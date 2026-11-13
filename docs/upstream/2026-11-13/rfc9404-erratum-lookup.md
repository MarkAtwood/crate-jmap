<!--
DRAFT, not submitted.
Target: RFC Editor errata report for RFC 9404 (https://www.rfc-editor.org/errata.php, "Report New Errata").
Checked 2026-11-13: https://www.rfc-editor.org/errata/rfc9404 reports "No matching errata found."
crate-jmap decision: 21(ii).
-->

**RFC:** 9404, JSON Meta Application Protocol (JMAP) Blob Management Extension

**Type:** Technical

**Section:** 4.3.1

**Original Text:**

```
   Response:

   [
     "Blob/lookup",
     {
       "list": [
         {
           "id": "Gd2f81008cf07d2425418f7f02a3ca63a8bc82003",
           "matchedIds": {
             "Mailbox": [
               "M54e97373",
               "Mcbe6b662"
             ],
             "Thread": [
               "T1530616e"
             ],
             "Email": [
               "E16e70a73eb4",
               "E84b0930cf16"
             ]
           }
         }
       ],
       "notFound": [
         "not-a-blob"
       ]
     },
     "R1"
   ]
```

**Corrected Text:**

```
   Response:

   [
     "Blob/lookup",
     {
       "list": [
         {
           "id": "Gd2f81008cf07d2425418f7f02a3ca63a8bc82003",
           "matchedIds": {
             "Mailbox": [
               "M54e97373",
               "Mcbe6b662"
             ],
             "Thread": [
               "T1530616e"
             ],
             "Email": [
               "E16e70a73eb4",
               "E84b0930cf16"
             ]
           }
         },
         {
           "id": "not-a-blob",
           "matchedIds": {
             "Mailbox": [],
             "Thread": [],
             "Email": []
           }
         }
       ]
     },
     "R1"
   ]
```

**Notes:**

The example contradicts the normative text in Section 4.3, which says:

```
   If a blob is not visible to a user or does not exist on the server at
   all, then the server MUST still return an empty array for each type
   as this doesn't leak any information about whether the blob is on the
   server but not visible to the requesting user.
```

The example's "notFound" array is exactly the leak that paragraph rules out: a server that lists unknown blobIds in "notFound" but returns empty arrays for inaccessible ones tells the client which blobs exist. Section 4.3 also defines no "notFound" property in the response, which lists only "list". An implementer copying the example would build a server that violates the MUST.

The corrected example follows Section 4.3: a BlobInfo with an empty array for each requested type, and no "notFound". draft-ietf-jmap-blobext-03, which is intended to obsolete RFC 9404, keeps the same rule (Section 6) and its Blob/lookup example (Section 9.3) already returns empty arrays for such a blob.

Found while implementing RFC 9404 in crate-jmap (https://github.com/MarkAtwood/crate-jmap), which follows the Section 4.3 text.
