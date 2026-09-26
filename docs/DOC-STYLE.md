# Documentation standard

Frozen 2026-09-26 for the 2.2 managed context run. This file is the rule set the `komun-docs-stylist`
agent applies to the repository's prose. It is versioned: a revision made inside a session is a new
version number plus a dated entry in the history at the bottom — never an in-place edit of an older
version's text.

Scope of a run: the files named in the run instruction, section by section. This run covers `AGENTS.md`
and `docs/DEVELOPMENT.md`.

## What counts as a claim

A claim is a sentence that asserts something about the repository: what the code, schema, config or
scripts do, what a file contains, what a command prints, or what must never be committed. Every claim
needs authority (R2). A sentence that only describes the document itself is not a claim.

## Rules (v1)

| Rule | Requirement | How a violation is detected |
|---|---|---|
| R1 | Every section opens with one purpose sentence. | the first line after the heading starts with "This section" |
| R2 | Every claim carries its authority in parentheses: a `path:line`, or a backticked command with its output. | a claim sentence with no parenthesised authority |
| R3 | A sentence is at most 25 words. | word count of the text between two sentence terminators |
| R4 | A rule is an imperative, and never hedges (`should`, `might`, `probably`, `may want to`). | a bullet whose first word is not a verb |
| R5 | Lists are flat: no nested bullets. | any bullet indented under another bullet |

## Authority forms

Use the shortest form that settles the claim:

- a location — `(crates/server/src/api/mod.rs:41)`
- a command and its result — `(cargo test --workspace -> 158 passed, 0 failed)`
- a count with the search that produced it — `(rg -c 'fetch\(' web/src -> 37)`

## A claim you cannot trace

Do not delete the claim, and do not invent authority for it. Mark the sentence with `[UNVERIFIED]`,
and add it to a `Claims needing verification` list at the end of that section, naming what would settle
it. That list is part of the run's evidence and the session summary must carry it forward verbatim.

## Version history

| Version | Date | Change |
|---|---|---|
| v1 | 2026-09-26 | Frozen before run 001 of the managed context run: R1–R5 above. |
