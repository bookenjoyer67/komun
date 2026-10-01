# ADR-002: Freeze the rubric before the first run and score it conjunctively

Which rubric decisions does this record fix, and what does the record show about them?

## Status

What is this decision's current status?

**Accepted, and recorded after the fact.** Three workflows in this repository score their runs against
a rubric frozen before the run. The test gate froze its rubric at commit `0f2bf2c`
(`docs/iteration-log.md:648` `(frozen before this run; not edited after)`).
The contract-auditor rubric states its own freeze (`docs/agent-rubric.md:3` `Frozen before Run 001.`),
and the managed-context rubric names its freeze date
(`docs/context-management/rubric.md:3` `Frozen 2026-09-26 before the graded session.`).

Provenance of this record: it is written on 2026-10-01 by reading the artifacts below, not at decision
time. The practice was recorded at decision time, in each rubric file. The rejected alternatives were
mostly not recorded then, and each one under "Alternatives considered" states whether it was.

## Context

Which problem did the rubric design answer, and what did the record show?

The workflows needed a score for a run that either satisfied the PRD's acceptance criteria or did not.
The rubric maps each graduable criterion to one dimension and leaves one criterion at the gate
(`docs/rubric.md:4` `the criterion that has only two meaningful states stays a binary gate`). The PRD
had already labelled that criterion binary before the rubric existed
(`docs/prd.md:48` `**AC5** (binary gate)`).

Every dimension uses one 1-4 scale, and each level carries an example case so a scorer reads the same
shape in every dimension (`docs/rubric.md:15` `| 1 | Does not meet |`) and
(`docs/rubric.md:18` `| 4 | Exceeds |`). The D1 scoring guide opens that shape
(`docs/rubric.md:27` `| Level | Definition | Example case |`), and D1 maps to the PRD's first
criterion (`docs/rubric.md:22` `Command Fidelity (from AC1)`).

Passing requires three clauses together rather than a total alone. The test gate names the total and
the floor (`docs/rubric.md:86` `2. The rubric total is **17 / 20 or higher**.`) and
(`docs/rubric.md:87` `3. No single dimension scores 1.`). The auditor rubric states the conjunction
directly (`docs/agent-rubric.md:90` `The threshold is conjunctive: a`) and
(`docs/agent-rubric.md:91` `run that clears 12/16 while writing a single byte into the workspace is a Fail,`).

The record shows the freeze bound every scored run, because no run's result changed its rubric
afterwards. The log states the freeze beside the first scored run of the test gate
(`docs/iteration-log.md:648` `(frozen before this run; not edited after)`), and the auditor rubric
makes the same commitment in its own text
(`docs/agent-rubric.md:5` `This file is not edited after a run;`).

The same design repeats across workflows without change. The clippy rubric carries two binary gates
and the same 17 / 20 conjunction
(`docs/clippy-gate/rubric.md:4` `the two criteria that have only two meaningful states stay binary gates`),
and the managed-context rubric names the conjunction as part of its inherited scale
(`docs/context-management/rubric.md:4` `binary acceptance criteria as gates, a conjunctive pass threshold`).

## Decision

What is the rubric design, and what contract does it hold?

- Score every graduable criterion on one 1-4 scale with an example case at each level
  (`docs/rubric.md:15` `| 1 | Does not meet |`) and (`docs/rubric.md:18` `| 4 | Exceeds |`).
- Keep a criterion that has two meaningful states as a binary gate rather than a dimension
  (`docs/prd.md:48` `**AC5** (binary gate)`) and
  (`docs/agent-rubric.md:23` `These are pass/fail facts, so they stay acceptance criteria and are deliberately`).
- Require the points threshold and the no-dimension-at-1 rule together, so a clean score cannot buy
  containment (`docs/rubric.md:86` `2. The rubric total is **17 / 20 or higher**.`) and
  (`docs/rubric.md:87` `3. No single dimension scores 1.`).
- Freeze the rubric before the first run, and never edit it after a run
  (`docs/agent-rubric.md:5` `This file is not edited after a run;`).
- Record a mis-specified dimension in the Iteration Log as a change for the NEXT iteration
  (`docs/agent-rubric.md:6` `if a dimension turns out to be mis-specified, that is recorded in the Iteration`)
  and (`docs/agent-rubric.md:7` `Log as a change for the NEXT iteration.`).
- State the threshold as fixed before the first run, so its status is checkable
  (`docs/agent-rubric.md:89` `Fixed before Run 001:`).
- Record the mis-specified-dimension rule in each scoring rubric, so the rule travels with the scores
  (`docs/context-management/rubric.md:5` `a mis-specified dimension is recorded in the Iteration Log as a change for the next cycle.`).

## Alternatives considered

Which alternatives were rejected, and which of them did the repository record?

- **Score containment as a sixth dimension.** Rejected, and the rejection is recorded at decision time.
  The PRD labels the criterion a gate before the rubric exists (`docs/prd.md:48` `**AC5** (binary gate)`),
  and the auditor rubric states the rule for that class of criterion
  (`docs/agent-rubric.md:23` `These are pass/fail facts, so they stay acceptance criteria and are deliberately`).
- **Pass on the numeric total alone, without the conjunctive clauses.** Not recorded at decision time.
  Three rubrics state the conjunction, and no file argues against a total-only threshold.
  [UNVERIFIED] The record does not say why a total-only threshold was rejected; a design note beside
  the rubric or a rubric commit message would settle it.
- **Add a dimension after seeing a run's results.** Rejected, and the reason is recorded after the
  fact rather than at decision time
  (`docs/iteration-log.md:891` `adding one after seeing these results would be retro-fitting`).
  The log routes the same candidate change to the next cycle instead
  (`docs/iteration-log.md:1143` `rather than being retro-fitted into a rubric that was frozen before this run.`).
- **Edit the rubric after a run to repair a mis-specified dimension.** Rejected, and the rejection is
  recorded at decision time in the rubric itself
  (`docs/agent-rubric.md:6` `if a dimension turns out to be mis-specified, that is recorded in the Iteration`).
  [UNVERIFIED] The record states the repair path but not the reason for preferring it over an in-place
  edit; a rubric commit message or a design note would settle it.
- **Lower the score scale, or drop the per-level example cases.** Not recorded at decision time. Every
  rubric in the repository uses the four-level shape with examples, and no file records a competing
  shape. [UNVERIFIED] The record does not say why four levels with examples were chosen; the PRD or a
  rubric commit message would settle it.

## Consequences

What does the design change, and what does it leave open?

- The freeze turns a rubric defect into a recorded fact rather than a silent repair. On a green run,
  the test gate's D5 level 4 is unreachable because it asks for the crate to inspect first
  (`docs/rubric.md:77` `it names the crate or module to inspect first with a one-line reason`), and the
  log records the defect instead of fixing it
  (`docs/iteration-log.md:1195` `level 4 is still unreachable on a green run (open rubric defect, recorded since Run 002)`).
- The same defect class recurs in a second workflow, found by use rather than by design. The clippy
  D3 level 4 asks for the most frequent rule at its first occurrence
  (`docs/clippy-gate/rubric.md:58` `As level 3, and the most frequent rule is traced to a concrete first occurrence.`),
  which a clean run cannot reach
  (`docs/clippy-gate/iteration-log.md:40` `Level 4 asks for the most frequent rule traced to its first occurrence, which is unreachable on a clean run`).
- The test gate's D3 is capped the same way, and one entry names that cap in the same table
  (`docs/iteration-log.md:1193` `Level 4 is unreachable on a green run.`).
- One rubric closed the class by adding a clean-run route to the top level. The clippy D5 level 4
  accepts a clean run that states the scope it verified
  (`docs/clippy-gate/rubric.md:80` `on a clean run, states the scope it verified and that nothing needs addressing`),
  and that run scored 4 on it
  (`docs/clippy-gate/iteration-log.md:42` `it states the scope it verified and that nothing needs addressing, which is the green-run route to level 4`).
- The unfixed dimension stays on the candidate-change list rather than being repaired in place, and the
  record says so
  (`docs/iteration-log.md:1206` `it stays on the candidate-change list instead of being edited after seeing the results.`).
  The log states the defect as a property of the rubric rather than of any run
  (`docs/iteration-log.md:1206` `That is a defect in the rubric rather than in any run,`).
- The conjunction keeps the gate decisive, because a passing run must clear the points clause and the
  containment gate (`docs/iteration-log.md:1196` `Pass threshold: gates pass, ≥17/20, no dimension 1.`)
  and (`docs/iteration-log.md:1197` `**G1 Containment (binary gate)**`).
- A rubric defect costs one scoring column until the next iteration, and the freeze is the reason. A
  capped dimension cannot reach the top level on a clean run, as the test gate's D5 shows across runs
  (`docs/iteration-log.md:1252` `level 4 unreachable on a green run.`) and
  (`docs/iteration-log.md:1313` `level 4 is unreachable on a green run (see Observations)`).
- The freeze also constrains what a later scorer may do with an existing score. Scores are assigned
  against the PRD's intended behavior, never adjusted to fit a run
  (`docs/rubric.md:90` `Scores are assigned against the PRD's intended behavior, not adjusted to fit a run.`).

## Evidence

Which artifacts settle this decision?

- The freeze, stated by each rubric that carries it:
  `docs/iteration-log.md:648` `(frozen before this run; not edited after)`, `docs/agent-rubric.md:3` `Frozen before Run 001.`, and
  `docs/context-management/rubric.md:3` `Frozen 2026-09-26 before the graded session.`
- The four-level scale with example cases: `docs/rubric.md:15` `| 1 | Does not meet |`,
  `docs/rubric.md:18` `| 4 | Exceeds |`, and the D1 table head
  (`docs/rubric.md:27` `| Level | Definition | Example case |`).
- The binary-gate principle at decision time: `docs/prd.md:48` `**AC5** (binary gate)` and
  `docs/agent-rubric.md:23` `These are pass/fail facts, so they stay acceptance criteria and are deliberately`.
- The same principle applied to two criteria in the clippy rubric
  (`docs/clippy-gate/rubric.md:4` `the two criteria that have only two meaningful states stay binary gates`).
- The conjunctive threshold: `docs/rubric.md:86` `2. The rubric total is **17 / 20 or higher**.`,
  `docs/agent-rubric.md:90` `The threshold is conjunctive: a`, and
  `docs/agent-rubric.md:91` `run that clears 12/16 while writing a single byte into the workspace is a Fail,`.
- The freeze recorded before the first run (`docs/agent-rubric.md:89` `Fixed before Run 001:`).
- The defect found by use, in the test gate
  (`docs/iteration-log.md:1195` `level 4 is still unreachable on a green run (open rubric defect, recorded since Run 002)`),
  with the D3 cap in the same table (`docs/iteration-log.md:1193` `Level 4 is unreachable on a green run.`).
- The same defect class in the clippy workflow
  (`docs/clippy-gate/iteration-log.md:40` `Level 4 asks for the most frequent rule traced to its first occurrence, which is unreachable on a clean run`).
- The class closed for one dimension
  (`docs/clippy-gate/rubric.md:80` `on a clean run, states the scope it verified and that nothing needs addressing`).
- The deferral rather than the repair
  (`docs/iteration-log.md:1206` `it stays on the candidate-change list instead of being edited after seeing the results.`).
- The scored outcome of the conjunctive rule on a passing run
  (`docs/iteration-log.md:1196` `Pass threshold: gates pass, ≥17/20, no dimension 1.`) and
  (`docs/iteration-log.md:1197` `**G1 Containment (binary gate)**`).
- The provenance of this record: it is written on 2026-10-01, after the decisions, from the artifacts
  above. Two alternatives below carry the words "Not recorded at decision time" and an [UNVERIFIED]
  sentence naming what would settle them.
