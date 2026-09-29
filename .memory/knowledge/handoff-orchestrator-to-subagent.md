# Handoff — Orchestrator → Subagent

The brief the orchestrator sends to one subagent. This file is a template: replace every bracketed
placeholder before sending, and keep the five section headings as they stand. The definitions in
`.claude/agents/` read their input in this shape, and the orchestrator's evaluation gate checks the
returned result against the "Required output format" section below.

## Placeholder conventions

- `[UPPER_SNAKE_CASE]` — a field to fill. Every placeholder is replaced before the brief is sent.
- `[OPTIONAL: …]` — an optional field. Write `None` when it does not apply, and keep the line.
- `<…>` — a literal value taken from the run: an `entry_id`, a gate command, a repository path.
- Write a section body as a bullet list, one fact per bullet. Do not send the section as a paragraph.
- Never send placeholder text verbatim. A brief with an unfilled placeholder is a rejected brief.
- Keep placeholder names unchanged, so a template revision is a single edit in one file.

## Role context

- Subagent: `[ROLE_NAME]`
- Definition: `.claude/agents/[ROLE_FILE].md`
- Run id: `[RUN_ID]`
- Position in the sequence: `[N]` of `[TOTAL]` — after `[PRECEDING_ROLE]`, before `[FOLLOWING_ROLE]`
- Why this role owns the task: `[ONE_SENTENCE_REASON]`
- Autonomy in force: `[LOW | MEDIUM | HIGH]`
- Checkpoint status: `[PLAN_APPROVED | RELEASE_APPROVED | NOT_APPLICABLE]`

## Task brief

- Requested change, in one sentence: `[CHANGE_SENTENCE]`
- Work this role does: `[WORK_ITEMS]`
- Out of scope for this role: `[OUT_OF_SCOPE_ITEMS]`
- What the previous role produced: `[PREVIOUS_RESULT_SUMMARY]`
- First action to take: `[FIRST_ACTION]`
- Stop condition: `[STOP_CONDITION]` — return to the orchestrator at this point rather than continuing

## Input materials

- Repository path: `[REPO_PATH]`
- Files to read: `[FILE_LIST]`
- Plan entry: `entry_id` `<PLAN_ENTRY_ID>`
- Decision entries: `<DECISION_ENTRY_IDS>`
- Test-result entry: `<TEST_RESULT_ENTRY_ID>`
- Review entry: `<REVIEW_ENTRY_ID>`
- [OPTIONAL: reference-corpus queries to run as prescribed — [QUERY_LIST]]
- Storage scope: `project_id` `proj-komun`, `calling_role` `[ROLE_NAME]`, classification ceiling `internal`
- Rules in force: `AGENTS.md`, `docs/DOC-STYLE.md`, `.memory/knowledge/coding-standards.md`

## Acceptance criteria

- `[AC1]` — `[CRITERION_TEXT]` — settled by `[ARTIFACT_OR_COMMAND_THAT_SETTLES_IT]`
- `[AC2]` — `[CRITERION_TEXT]` — settled by `[ARTIFACT_OR_COMMAND_THAT_SETTLES_IT]`
- [OPTIONAL: AC3 — [CRITERION_TEXT] — settled by [ARTIFACT_OR_COMMAND_THAT_SETTLES_IT]]
- Write every criterion as a check, not as a goal: a criterion names what is inspected and what settles it.
- A criterion the role cannot check is a brief defect: return the criterion to the orchestrator.

## Required output format

- Shape: the result follows `.memory/knowledge/handoff-subagent-to-orchestrator.md` section for section.
- Sections required: `What was done`; `What was produced`; `Rubric self-score (optional)`;
  `Open questions`; `Blockers`.
- Artifacts required: `[ARTIFACT_LIST]` — each named by path or by `entry_id`
- Evidence required: `[EVIDENCE]` — verbatim output, counts, and the `entry_id` of every stored entry
- Criteria to report against: `[AC_IDS]`
- [OPTIONAL: fill the Rubric self-score section, dimensions [DIMENSION_NAMES], scale 1-4]
- The orchestrator rejects a result that omits what was produced or the criteria it claims, and returns it
  to the same role once with the defect named.
