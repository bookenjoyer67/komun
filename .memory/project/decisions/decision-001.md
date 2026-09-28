# Decision 001 - File-based memory inside the repo

**Date:** 2026-09-28
**Review by:** 2026-12-27
**Status:** Active

**Decision:** Komun's persistent agent memory is plain files under `.memory/` committed to this
repository, rather than a database, a vector store, or a volume mounted from outside the repo.

**Rationale:** The half of memory that matters most is the part that explains code, so memory that
lives next to the code can be diffed against it and cited by commit SHA. The three layers separate
hot state (an index read at startup) from rule text that people maintain and from bulky reference
material that is searched instead of loaded, which keeps the startup read small. A Module 2.2 run on
2026-09-26 showed the cost of the alternative in this very repository: one large context document
compacted from 166,700 to 12,837 tokens and the generated summary dropped the unresolved-questions
field entirely.

**Alternatives rejected:** A vector or embedded database for retrieval was deferred, not rejected —
Module 3 requires an MCP-backed retrieval tool with its own evaluation, and the sandbox image has no
`python3` today, so a Python MCP server would need an image rebuild first. Storing memory outside the
repository in a mounted volume was rejected outright: it cannot be versioned with the code, and a
mount configured wrongly is precisely the scope-leak failure the next lesson drills. Keeping every
memory in `CLAUDE.md` was rejected because that file is read on every session whether or not it is
relevant.
