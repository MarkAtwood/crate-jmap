# JMAP.Net fixtures (bd JMAP-48o0)

Copied unchanged from JMAP.Net (https://github.com/JMAP-Net/JMAP.Net) at
commit db60d475989e0f732f743c814718cf184621d71f,
`src/JMAP.Net.Tests/Fixtures/`. JMAP.Net is MIT licensed:

> MIT License. Copyright (c) 2025 JMAP.Net. Permission is hereby granted,
> free of charge, to any person obtaining a copy of this software and
> associated documentation files, to deal in the Software without
> restriction, subject to the condition that the copyright notice and this
> permission notice are included in all copies or substantial portions.

They are that library's serializer output: its test suite compares its
serializer against these files (2026-11-19, .NET 10.0.404: 165 of 166 tests
pass; the one failure is an HTTP concurrency-limit test, not a fixture).
They were computed once; no .NET is needed to run the Rust tests.

JMAP.Net targets draft-ietf-jmap-calendars-26 and RFC 8620. Where it and
this workspace disagree, the draft text decides, and the test asserts the
disagreement with its citation.
