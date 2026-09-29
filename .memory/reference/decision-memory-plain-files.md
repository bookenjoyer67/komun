---
classification: internal
project: proj-komun
doc_type: decision
---

# Decision: agent memory is plain files inside the repository

Where does Komun's persistent agent memory live, and which alternatives were rejected?

Memory is plain files under `.memory/` committed with the code, not a database and not a volume mounted from outside the repository (`docs/memory-architecture.md:210` `Put `.memory/` outside the repository, in a mounted volume.`). Memory beside the code can be diffed against it and cited by commit SHA (`docs/memory-architecture.md:212` `artifacts sit in the working tree with commit SHAs behind them`). The tree has three layers, and the largest one is queried rather than loaded (`docs/memory-architecture.md:100` `The agent queries this layer by keyword and reads one or two hits.`).

## Which alternatives were rejected, and on what evidence?

- Reject a mounted volume outside the repository: it cannot be versioned with the code, and a wrong mount is the scope-leak failure (`docs/memory-architecture.md:214` `set up wrongly is exactly the scope-leak failure mode`).
- Reject keeping everything in `CLAUDE.md`, because that file is read every session whether or not it is relevant. This repository measured the cost: a 2.2 run compacted (`docs/memory-architecture.md:205` `session compacted from 166,700 tokens to 12,837`), and the summary dropped the unresolved-questions field.
- Defer, not reject, a vector store: Module 3 builds the MCP retrieval tool with its own evaluation, and the sandbox image carried no Python on 2026-09-28 (`docs/memory-architecture.md:221` `so a Python MCP server needs an image`).
- Reject an append-only decision log, because an audit log is not a memory system (`docs/memory-architecture.md:226` `log is not a memory system. Layer 1 holds current state`).

The accepted cost is human review, since entries must be reviewed before every commit (`docs/memory-architecture.md:215` `entries must be reviewed before every commit`).
