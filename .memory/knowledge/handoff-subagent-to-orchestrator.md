# Handoff — Subagent → Orchestrator

The result one subagent returns to the orchestrator. This file is a template: replace every bracketed
placeholder before returning, and keep the five section headings as they stand. The orchestrator reads a
result in this shape and evaluates it against the "Required output format" section of the brief it sent.

## Placeholder conventions

- `[UPPER_SNAKE_CASE]` — a field to fill. Every placeholder is replaced before the result is returned.
- `[OPTIONAL: …]` — an optional field. Write `None` when it does not apply, and keep the line.
- `<…>` — a literal value from the run: an `entry_id`, a gate command, a count, a path.
- Write a section body as a bullet list, one fact per bullet. Do not return the section as a paragraph.
- Print output verbatim. A summary word — `reviewed`, `checked`, `confirmed` — is not output.
- Never return placeholder text verbatim. A result with an unfilled placeholder is a rejected result.

## What was done

- Role: `[ROLE_NAME]` — definition `.claude/agents/[ROLE_FILE].md`
- Run id: `[RUN_ID]`
- Steps taken: `[STEPS_TAKEN]`
- Tools called: `[TOOL_CALLS]` — one line per tool, with the arguments that matter
- Files written: `[FILES_WRITTEN]` — `None` when the role holds no write tool
- Gates or commands run: `[GATES_RUN]` — `None` when the role holds no runner
- Deviations from the brief: `[DEVIATIONS]` — `None` when the work followed the brief step for step
- Work left undone and why: `[OUTSTANDING]` — `None` when the brief is finished

## What was produced

- Result in one sentence: `[RESULT_SENTENCE]`
- Artifacts: `[ARTIFACT_LIST]` — each named by path, by `entry_id`, or by both
- Storage entries: `[ENTRY_IDS]` — each with its `entry_type` and `classification`
- Evidence: `[EVIDENCE_BLOCK]` — the raw output, the counts, or the `path:line` and its literal text
- Acceptance criteria met: `[AC_IDS]` — with the artifact that settles each one
- Sources for retrieved or searched claims: `[SOURCE_DOCUMENT_AND_CHUNK]` — `None` when the role ran no lookup
- Verdict, for a reviewing or testing role: `[VERDICT]` — `None` for a role that returns no verdict

## Rubric self-score (optional)

- Fill this section only when the brief asks for a self-score. Otherwise write `None` under this heading.
- Dimensions to score: `[DIMENSION_NAMES]` — scale `[SCALE]`
- `[DIMENSION_NAME]`: score `[SCORE]` — evidence `[EVIDENCE_LINE]`
- `[DIMENSION_NAME]`: score `[SCORE]` — evidence `[EVIDENCE_LINE]`
- Total: `[TOTAL]` of `[MAX]`
- A score without its evidence line is not a self-score. Delete the score rather than assert it.

## Open questions

- `[QUESTION]` — what would settle it: `[ARTIFACT_OR_LOOKUP]` — owner: `[ROLE_OR_HUMAN]`
- Write `None` when the work left no question open.
- Return a question rather than guessing: an answer asserted without its artifact is a defect the
  orchestrator counts against the result.

## Blockers

- `[BLOCKER]` — the artifact or fact it blocks: `[BLOCKED_WORK]` — the attempt that failed and its exact
  output: `[FAILURE_OUTPUT]` — what would clear it: `[RESOLVER]`
- Write `None` when nothing blocks the work.
- Report a failed tool call exactly as the tool returned it. An unnamed failure reads as a pass.
- A blocker the role cannot clear stops the run: the orchestrator escalates it to the human at the next
  checkpoint rather than routing it back to the same role.
