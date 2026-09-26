# Agent context boundary policy

Policy version: v0.1.0 (2026-09-26). This file is read automatically at the start of every session in
this directory. `AGENTS.md` is the project's cold-start guide; this file is the standing policy for how
context is managed across a session, and it applies to every agent working here.

## Context Boundary Procedure

At the start of each new phase of work, before any editing or analysis:

1. Restate the current task goal in one sentence.
2. List the rules currently in effect, verbatim, not paraphrased.
3. State explicitly which prior rules are no longer in effect, if any.
4. Name the specific artifact this phase works on.
5. Then proceed with the requested work.

A phase begins whenever the rules change, the target file changes, or the session moves from producing
work to revising work already produced. The user supplies the boundary text; if a phase starts without
one, ask for it before editing.

## Summarization Policy

- The summarization skill lives at `.claude/skills/summarize-session`. Invoke it when a phase ends and
  new rules are about to be introduced, at any other natural breakpoint, or when the user asks.
- The summary must copy rules and artifact text verbatim. A paraphrase of an acceptance criterion or of
  a document's current text is a defect, because the summary replaces the history it describes.
- Present the summary and wait for the user to confirm or correct it before doing any further work.

## Compaction Policy

Proactive summarization above is the first line of defence; compaction is the fallback. This repository
has not yet observed a `/compact` run, so what compaction reliably preserves here is deliberately
unrecorded rather than guessed. Session plan for the current run: boundaries and one summary, with
`/compact` used only if context usage actually crosses ~60% — and if that happens, the pre/post usage
figures and the recall probes go into the iteration log.
