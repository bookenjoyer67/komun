---
classification: internal
project: proj-komun
doc_type: runbook
---

# Runbook: the pre-merge release checklist

What must be true before a change reaches `main`, and who approves it?

The pre-merge gate is an orchestrated workflow with two required human checkpoints (`CLAUDE.md:126` `Two, and both are required: plan approval before any implementation work, and release approval before`). The run halts at each one, and nothing reaches `main` without the second (`CLAUDE.md:106` `Human checkpoint 2 — release approval. The run stops here until a human approves the merge.`).

## Which acceptance criteria must the run satisfy?

Every run must pass the workspace tests, report zero clippy warnings, keep the formatter clean, and pass the frontend checks (`` `CLAUDE.md:94` `reports zero warnings, `cargo fmt --check` is clean, `npm run check` and `npx vitest run` pass under `web/`, ``). Every changed prose file must also satisfy `docs/DOC-STYLE.md` (`` `CLAUDE.md:95` `and every changed prose file satisfies `docs/DOC-STYLE.md`. ``).

## In what order do the roles run?

The project manager opens the ticket, and the planner writes its plan to the `storage` server under project `proj-komun` (`CLAUDE.md:100` `writes the plan to the `storage` MCP server under`). Human checkpoint 1 approves that plan before any code is written (`CLAUDE.md:102` `Human checkpoint 1 — plan approval.`).

## What does the checklist say about a malformed handoff?

Return it to the same role once, naming the specific defect, and escalate on a second failure (`CLAUDE.md:113` `An output that fails the format returns to`). Escalate immediately when a change touches migrations or authentication code (`CLAUDE.md:119` `when the change touches `migrations/` or authentication code.`). A read-only gate run must leave the repository untouched, and a scored run passes at 17 of 20 or higher (`docs/rubric.md:86` `The rubric total is **17 / 20 or higher**.`).
