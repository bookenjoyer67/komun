# Memory Architecture

## What this workflow needs to remember

This agent helps maintain **Komun** — a single-server mutual-aid web app (Rust/Axum server, SvelteKit
SPA, client-side crypto in WASM) — by running its pre-merge quality gate and keeping the prose docs in
sync with the code. Across sessions it needs to remember the current phase of that work, the decisions
that constrain future edits, the measured gate baselines that reveal a regression, known environment
quirks, and the questions that are still open because no line in the repository settles them. It does
not need to store information the repository already carries — file contents, test output, migration
text, or documentation that a person can read in place. It must not store the database URL or SMTP
credentials from `config.toml`, anything from `.env*`, session tokens, the `claude-auth` credential, and
never any key material or message plaintext (`AGENTS.md:134` — "**Never log** keys, bundles, passwords,
derived keys, or message plaintext"; the schema keeps message content server-unreadable,
`migrations/001_schema.sql:227`).

## Layer 1: Project memory directory

`.memory/project/` — what the agent has learned or decided across sessions. It reads the index on
startup and writes entries when state changes.

**Belongs here**

- The current phase of the work: Module 2 of the agentic-engineer course is building the memory system
  itself, alongside the capstone's pre-merge quality gate (2026-09-28).
- The measured gate baselines, with their date and the toolchain they came from: `cargo test
  --workspace` → 158 passed / 0 failed / 0 ignored, `cargo clippy --release -- -D warnings` → zero
  warnings, `npx vitest run` → 82 tests in 7 files, measured 2026-09-25 on `rustc 1.95.0` /
  `node v22.23.2` (`AGENTS.md:201-207`). A future session detects a regression by comparing against
  these numbers.
- Known environment quirks: the agent sandbox image carries no `python3`, so any Python-based harness
  needs an image rebuild (measured 2026-09-28); files the container writes into the mounted repo are
  root-owned on the host until they are chown'd (observed 2026-09-28).
- Standing constraints an edit must respect: `migrations/001_schema.sql` is frozen and schema changes
  are additive files (`AGENTS.md:111-117`); the build order is `wasm-pack` → `npm run build` →
  `cargo build` (`AGENTS.md:77-81`).
- Open questions, carried as state until someone rules on them: whether the intended term is
  "Svelte 5" or "SvelteKit 2" (`AGENTS.md:44-47`), and that the worked example at
  `docs/DOC-STYLE.md:38` and `:68` (`rg -c 'fetch\(' web/src -> 37`) does not reproduce — 47
  occurrences across 18 files, none above 18 (measured 2026-09-28).

**Does not belong here**

- The contents of the files those facts are about, or any test or command output — both are already in
  the repository, or reproducible from it, and storing them makes the entry rot the moment the code
  moves. Reason: memory is for what cannot be re-derived cheaply.
- Per-task notes once the task is finished ("edited line 42 of `post.rs`") — the lesson's own
  distinction holds: an audit log is not a memory system.
- Anything from `config.toml` or `.env*` (`AGENTS.md:56-59`). Reason: secrets must never enter a layer
  that is designed to outlive the session and is designed to be committed.

- **Scope:** Project-scoped. This memory belongs to this repository only, and `.memory/SCOPE.md` names
  the project so a session can refuse to run against another project's memory.
- **Write permissions:** Agent-writable, but only through the index discipline in `CLAUDE.md`: check
  `MEMORY_INDEX.md` for an existing entry on the topic and update it rather than adding a duplicate. A
  human reviews entries before they are committed.
- **Pruning policy:** Entries tied to a feature branch are archived when that branch merges to `main`.
  Project-scoped entries (decisions, known quirks, current phase, gate baselines) are reviewed every
  90 days. Any entry a later entry supersedes is replaced by that entry in the same commit, never
  appended to.

## Layer 2: Knowledge files

`.memory/knowledge/` — stable rules a person maintains and the agent consults but never changes
(`chmod -R 444`, verified from inside the container in Step 6).

**Belongs here**

- The quality-gate standard: `cargo clippy --release -- -D warnings` must stay at zero warnings and
  `cargo test --workspace` must stay green (`AGENTS.md:197`).
- The documentation standard's authority rule: every claim carries a `path:line` containing the
  literal text it quotes, and a count carries the search that produced it (`docs/DOC-STYLE.md`).
- The frontend dialect rule: Svelte 5 runes only — `$state`, `$derived`, `$effect`, `$props`, with no
  `export let`, no `on:click` and no `$:` (`AGENTS.md:104-106`).
- The crypto boundary rule: no key material, key bundle, password, derived key or message plaintext is
  ever logged or written to any memory layer (`AGENTS.md:134`, `docs/CONVENTIONS.md:80-81`).
- The change-control rule: never edit `migrations/001_schema.sql`; add `002+` (`AGENTS.md:111-117`).

**Does not belong here**

- Project state that is supposed to change — the current phase, the current ticket, the latest
  measurement. Reason: this layer is the read-only, human-controlled half of the system; putting
  moving state in it forces the agent to treat a stale value as a standard.
- Anything the agent is expected to update during a session. Reason: a rule the agent can rewrite is
  not a guardrail, and the `444` permissions make that machine-enforced rather than advisory.
- Secrets, credentials and personal data, for the same reason as Layer 1.

- **Scope:** Project-scoped and human-authored. It encodes the rules that come from `AGENTS.md`,
  `docs/DOC-STYLE.md` and `docs/CONVENTIONS.md`, for this repository only.
- **Write permissions:** Human only. The agent is read-only, enforced by filesystem permissions rather
  than by instruction, and the enforcement is verified inside the sandbox container because the mount
  can change how host permissions arrive.
- **Pruning policy:** Reviewed every 90 days and whenever `AGENTS.md` or `docs/DOC-STYLE.md` changes. A
  rule that has moved into code, a test or a config default is deleted from this layer rather than
  duplicated, because the code is then the authority.

## Layer 3: Indexed reference documents

`.memory/reference/` — larger or more numerous documents that would be too expensive to load every
session. The agent queries this layer by keyword and reads one or two hits.

**Belongs here**

- The contract-audit reports under `docs/contract-audit/`, which record what an earlier audit found and
  why each finding was accepted or rejected (`AGENTS.md:149`).
- The design rationale in `docs/ARCHITECTURE.md`, `docs/CRYPTO.md` and `docs/DATABASE.md` — for
  example why `matches.message` was dropped in migration `003` when the column it replaced had been
  plaintext (`migrations/003_drop_matches_message.sql:22`).
- The iteration-log entries for earlier runs, which are the evidence behind the baselines in Layer 1
  (`docs/iteration-log.md`).
- Past pull-request descriptions, where they record a decision the diff alone does not explain.

**Does not belong here**

- Anything the agent needs on every session — the gate commands, the build order, the current phase.
  Reason: an indexed layer costs a search to use, so hot rules belong in Layer 2 or in `CLAUDE.md`.
- Generated output and build artifacts (`web/build/`, `crates/wasm/pkg/`), which are gitignored and
  reproducible. Reason: an index over reproducible artifacts is a second copy that drifts.

- **Scope:** Project-scoped, human-written, agent read-only.
- **Write permissions:** Human only. The agent consults it by keyword and must not read the whole
  directory (stated in `CLAUDE.md`, and pinned by the same `444` permissions as Layer 2).
- **Pruning policy:** An entry is removed when the artifact it indexes is deleted or superseded, and
  the layer is reviewed every 90 days together with Layer 1, because an index that points at a deleted
  document is the failure mode this layer introduces.

## Allocation decision table

| Information type | Where it goes | Reason |
|---|---|---|
| How to run the gate (`cargo test --workspace`, `cargo clippy --release -- -D warnings`, `npm run check`) | Skill / `AGENTS.md` "Tests" — not memory | Procedural steps are a procedure, not memory; they belong where the agent already looks for commands |
| The database URL and SMTP credentials (`config.toml`, `.env`) | Environment only, gitignored — no memory layer | Secrets must never be written into a layer designed to outlive the session and be committed |
| The file being edited right now | Context window only | No value once the session ends |
| The measured gate baselines, date-stamped (158 tests / zero warnings / 82 vitest) | Layer 1, project memory | A future session needs them to detect a regression; they are current state, not a procedure |
| Past contract-audit reports and the migration rationale for `003` | Layer 3, indexed reference | Too many and too large to load every session; retrieved by keyword on demand |
| A superseded design, e.g. the plaintext `matches.message` column | Layer 1 as current state ("dropped in 003"), Layer 3 for the old rationale | The agent must not act on the superseded design, but the reason it was dropped stays auditable |
| Key material, key bundles, passwords, recovery codes, message plaintext | Nowhere | Standing rule: never logged, never stored, in any layer |

## Alternatives considered

**1. Keep everything in `CLAUDE.md`, with no `.memory/` tree.** Rejected. `CLAUDE.md` is read at the
start of every session whether or not its content is relevant, which is the "too much memory" failure
named in the lesson, and this repository already has evidence of the cost of one large context file: in
the Module 2.2 run on 2026-09-26 the session compacted from 166,700 tokens to 12,837, and the generated
summary dropped the unresolved-questions field entirely — the one field a future session most needs. A
small index plus opt-in reference layers keeps the startup read small and keeps the unresolved questions
in a file that is read deliberately.

**2. Put `.memory/` outside the repository, in a mounted volume.** Rejected for this system, and the
repository's own history argues against it: the 2.2 run's evidence is citable only because the
artifacts sit in the working tree with commit SHAs behind them (`docs/iteration-log.md:111-251`). Memory
kept outside version control cannot be diffed against the code whose behaviour it explains, and a mount
set up wrongly is exactly the scope-leak failure mode the next lesson drills. The cost of this choice
is real and accepted: entries must be reviewed before every commit, which is why the write policy in
`CLAUDE.md` requires it.

**3. A vector store or embedded retrieval database for the reference layer.** Deferred, not rejected.
Module 3 requires an MCP-backed retrieval tool, so the retrieval layer will be built there with its own
evaluation; building it now would mean maintaining two retrieval paths. There is also a concrete
blocker today: the agent sandbox image carries no `python3`, so a Python MCP server needs an image
rebuild before it can run at all (measured 2026-09-28). Until then the reference layer is greppable
markdown, which needs no embedding model and no extra dependency.

**4. Store every decision as an append-only log.** Rejected on the lesson's own distinction — an audit
log is not a memory system. Layer 1 holds current state, and a superseding entry replaces the entry it
supersedes in the same commit; the history of why a decision changed lives in Layer 3 and in the
repository's own commit log, where it is already complete.
