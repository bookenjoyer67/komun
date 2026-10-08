# ADR-003: Allocate agent memory across three in-repo layers, gated by human review

Where does the memory system keep its state, and what enforces the allocation?

## Status

What is this decision's current status?

**Accepted.** The three layers sit in the working tree, two hooks enforce them, and the failure-mode
test passed across all three safeguards (`docs/iteration-log.md:535` `3 / 3 safeguards held`).

## Context

What must the workflow remember, and what does the repository already carry?

The agent maintains Komun and must recall the current phase, the constraining decisions, the measured
gate baselines, environment quirks and open questions across sessions. It must not store what the
repository already carries, and nothing secret may enter any layer. The architecture states the test:
memory is for what cannot be re-derived cheaply (`docs/memory-architecture.md:46` `memory is for what cannot be re-derived cheaply.`).

One large context file does not hold this state. The Module 2.2 run compacted from 166,700 tokens to
12,837 (`docs/memory-architecture.md:205` `166,700 tokens to 12,837`), and the generated summary lost
the unresolved questions (`docs/memory-architecture.md:206` `dropped the unresolved-questions field entirely`).

Memory also cannot rest on an external mount. The repository argues that its artifacts are citable only
because they sit in the working tree with commit SHAs behind them
(`docs/memory-architecture.md:211-212` `citable only because the artifacts sit in the working tree with commit SHAs behind them`).

Two runs produced the system. Run 002 built the `.memory/` tree and the two hooks
(`docs/iteration-log.md:610` `Artifacts produced: docs/memory-architecture.md`). Run 003 tested it
against three failure modes (`docs/iteration-log.md:555` `Failure mode 1`;
`docs/iteration-log.md:564` `Failure mode 2`; `docs/iteration-log.md:571` `Failure mode 3`).

## Decision

What allocation was chosen, and what contract does each layer hold?

Keep memory as plain files inside the repository, split into three layers. The build run records the
choice in `decision-001.md` (`docs/iteration-log.md:614` `memory is plain files inside this repository`).

- Allocate layer 1 to `.memory/project/`: current state the agent has learned or decided across sessions
  (`docs/memory-architecture.md:19` `what the agent has learned or decided across sessions`). Write it
  through the index discipline in the policy (`CLAUDE.md:39` `read .memory/project/MEMORY_INDEX.md`).
- Hold layer 2 at `.memory/knowledge/`: stable rules a person maintains and the agent never changes
  (`docs/memory-architecture.md:64` `stable rules a person maintains and the agent consults but never changes`).
- Hold layer 3 at `.memory/reference/`: documents too large to load every session, queried by keyword
  (`docs/memory-architecture.md:100` `The agent queries this layer by keyword and reads one or two hits.`).
- Declare ownership at each root. `.memory/SCOPE.md` names the owning project
  (`docs/memory-architecture.md:159` `carries a SCOPE.md declaring its owning repo`), and this root
  declares it (`.memory/SCOPE.md:3` `Project: Komun`).

Hold layer 1 as read-write under human review (`CLAUDE.md:50` `Read on startup via MEMORY_INDEX.md`),
and layers 2 and 3 as agent read-only (`CLAUDE.md:54` `Read-only. Consult before making any decision`).

Enforce the write policy with hard stops rather than wording. Classify content before any write
(`CLAUDE.md:64` `Before writing any content to a memory file, classify it:`), and block credential-shaped
text at commit (`docs/memory-architecture.md:155` `the commit is blocked and nothing is written to history`).
Deny writes to layers 2 and 3 inside the permission layer
(`docs/memory-architecture.md:160` `denies any write to the read-only layers`), because the agent runs
as root (`.claude/hooks/guard-readonly-memory.sh:5` `the agent runs as root inside the sandbox container and root ignores file`).
Load memory before the first prompt, because advisory text executes nothing
(`.claude/hooks/load-memory.sh:5` `nothing executes it`).

## Alternatives considered

Which alternatives were weighed, and where were they recorded?

The architecture doc records each of the four alternatives at decision time, with its own reasoning
(`docs/memory-architecture.md:200` `Alternatives considered`). Reproduce each one with that evidence.

- **Keep everything in `CLAUDE.md`, with no `.memory/` tree.** Recorded at decision time
  (`docs/memory-architecture.md:202` `Keep everything in`). Rejected: the file is read every session,
  and one large context file already cost the unresolved questions
  (`docs/memory-architecture.md:206` `dropped the unresolved-questions field entirely`).
- **Put `.memory/` outside the repository, in a mounted volume.** Recorded at decision time
  (`docs/memory-architecture.md:210` `outside the repository, in a mounted volume`). Rejected: memory
  outside version control cannot be diffed against the code it explains. The cost is accepted and named
  (`docs/memory-architecture.md:215` `entries must be reviewed before every commit`).
- **A vector store for the reference layer.** Recorded at decision time and deferred, not rejected
  (`docs/memory-architecture.md:218` `Deferred, not rejected.`). The blocker is concrete: the sandbox
  image carries no `python3` (`docs/memory-architecture.md:221` `the agent sandbox image carries no python3`).
- **Store every decision as an append-only log.** Recorded at decision time
  (`docs/memory-architecture.md:225` `Store every decision as an append-only log`). Rejected on the
  distinction the repository repeats: an audit log is not a memory system
  (`docs/memory-architecture.md:48` `an audit log is not a memory system.`).
- **A hosted memory service, holding state outside the tree entirely.** Not recorded at decision time.
  [UNVERIFIED] Reasoning: no artifact weighs a hosted service against the in-repo tree on diffability or
  credential exposure. A recorded comparison at decision time would settle it.

## Consequences

What does this allocation change, and what does it leave open?

- Keep startup small. The load hook inlines the index with its active entries, size-budgeted
  (`docs/iteration-log.md:623` `5.3 KB per session, size-budgeted`), under a byte cap
  (`.claude/hooks/load-memory.sh:22` `MAX_BYTES=20000`).
- Stop writes to layers 2 and 3 at the permission layer, not at a mode bit. The permission test failed:
  a root write returned 0 (`.claude/hooks/guard-readonly-memory.sh:6` `returned 0 as root and 1 as`).
- Block a credential inside memory reach before the commit object exists
  (`docs/calibration-log.md:48` `a write path open to any classification can persist a credential into files meant to be committed`;
  `docs/iteration-log.md:575` `was blocked by the new pre-commit hook with exit 1 and nothing written to history`).
- Halt on a borrowed mount instead of answering from it. The scope leak was reproduced
  (`docs/calibration-log.md:54` `memory mounted from another project looks identical to the right memory`),
  and the halt named the mismatch (`docs/iteration-log.md:567-568` `SCOPE.md declares project-b; the workspace is Komun`).
- Keep the read-only half out of the agent's authorship, because a rule the agent can rewrite is not a
  guardrail (`docs/memory-architecture.md:84-85` `a rule the agent can rewrite is not a guardrail`).
- Accept a search cost in layer 3. The reference layer is greppable markdown
  (`docs/memory-architecture.md:222` `the reference layer is greppable`), so no embedding model is needed
  until Module 3 builds one (`docs/memory-architecture.md:219` `MCP-backed retrieval tool`).
- Leave the vector layer open, deferred with its own evaluation rather than rejected
  (`docs/memory-architecture.md:218` `Deferred, not rejected.`).
- Leave one operational gap open. In-container `git` refuses the mounted workspace
  (`docs/iteration-log.md:598` `detected dubious ownership`), so memory entries that cite commit SHAs
  cannot be checked there.

## Evidence

Which artifacts settle this decision?

- Decision of record: `docs/memory-architecture.md`, holding the three layer sections, the allocation
  table (`docs/memory-architecture.md:127` `Allocation decision table`) and the four recorded
  alternatives (`docs/memory-architecture.md:200` `Alternatives considered`).
- Scope declaration: `.memory/SCOPE.md` (`.memory/SCOPE.md:3` `Project: Komun`), injected at startup
  (`.claude/hooks/load-memory.sh:26` `--- .memory/SCOPE.md ---`).
- Enforcement artifacts: `.claude/hooks/load-memory.sh` for startup reads,
  `.claude/hooks/guard-readonly-memory.sh` for write denial, and `scripts/hooks/pre-commit` for the
  credential scan.
- Policy: `CLAUDE.md`, memory configuration and write policy (`CLAUDE.md:62` `Write policy`).
- Build run: `docs/iteration-log.md:605` `## Run 002 (workflow 5`, which records the two failures found
  and fixed, starting with advice that was not a mechanism (`docs/iteration-log.md:617` `was advice, not a mechanism`).
- Permission defect and root-write measurement: the prescribed mode was corrected
  (`docs/iteration-log.md:629` `to 555 on directories and 444 on files`), and `touch` returned 0 as root
  and 1 as uid 1000 (`docs/iteration-log.md:630` `returned 0 as root and 1 as`).
- Failure-mode run: `docs/iteration-log.md:535` `3 / 3 safeguards held`, across stale entry
  (`docs/iteration-log.md:555` `Failure mode 1`), scope leak (`docs/iteration-log.md:564` `Failure mode 2`)
  and secret (`docs/iteration-log.md:571` `Failure mode 3`).
- Calibration: the near-misses and the controls they buy (`docs/calibration-log.md:88` `Cap every write at internal`;
  `docs/calibration-log.md:72` `a filesystem permission is not a guardrail when the writing process runs as root`;
  `docs/calibration-log.md:92` `Enforce the read-only memory layers with a hard stop`).
- Secret classes named for the layers: none of these ever reaches a file
  (`docs/memory-architecture.md:137` `never logged, never stored, in any layer`;
  `docs/memory-architecture.md:151` `Must never appear in any memory file.`), and the allocation table
  was amended to hold a pointer instead of a value
  (`docs/memory-architecture.md:139` `the first version of this entry held the key itself`).
- Provenance: this record derives from `docs/memory-architecture.md`, the two hooks under
  `.claude/hooks/`, and the Run 002 and Run 003 entries in `docs/iteration-log.md`
  (`docs/iteration-log.md:605` `## Run 002 (workflow 5`; `docs/iteration-log.md:535` `3 / 3 safeguards held`).

## Open risks

- The byte cap is a budget, not a guarantee. It bounds what the hook inlines, and an entry that outgrows it is truncated rather than refused.
- The read-only layers are enforced at the hook, because permission bits do not bind a root process (`.claude/hooks/guard-readonly-memory.sh:6` `returned 0 as root and 1 as`). A write path that bypasses the hook is outside the control.
- The allocation is judged against the workflows that exist. A role added later has no allocation until one is written for it.
