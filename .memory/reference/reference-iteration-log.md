---
classification: internal
project: proj-komun
doc_type: reference
---

# Reference: what an iteration-log entry must record

What does `docs/iteration-log.md` require of one run, and how are entries kept honest?

Every run of a workflow gets one entry, most recent first, and entries are immutable (`docs/iteration-log.md:4` `Entries are never deleted or rewritten, and the commits that add them are never squashed.`). The heading fixes the run number, the workflow, the date and the result (`docs/iteration-log.md:198` `— 2026-09-28 — 3 / 3 safeguards held`).

## Which parts must the entry carry?

- Run metadata: the system under test, the invocation that produced the evidence, and the artifacts.
- Evidence kept outside the repository, named by path (`docs/iteration-log.md:205` `Evidence kept outside the repo: `~/komun-agent-exercise-2-4/drill1-observe-run.txt` (full session`).
- Each failure with what was observed rather than assumed, and the fix or the decision not to act.
- The changes made in the run, file by file (`docs/iteration-log.md:243` `Changes made in this run:`).

## How are citations kept valid when the log grows?

New entries are prepended, which shifts every existing line number (`docs/iteration-log.md:209` `were prepended to the top of the log, which shifted every citation`). Repair every drifted pointer in the same commit that adds the entry, then read each cited line back (`docs/iteration-log.md:212` `Every pointer that drifted was repaired in the same commit that adds these`).

## What must an end-to-end run record?

Name any step that failed, the cause the evidence supports, and the fix applied (`docs/integration-test.txt:197` `naming any step that failed, the cause you can actually support, and the`). A run below its threshold is still valid evidence, and a score is never adjusted to fit the run (`docs/rubric.md:90` `Scores are assigned against the PRD's intended behavior, not adjusted to fit a run.`).
