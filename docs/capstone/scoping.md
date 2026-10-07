# Workflow selection and scoping: the Komun pre-merge quality gate

Each claim below carries a `path:line` citation followed by the literal on that line. Where the record does not say why a choice was made, this document says so rather than inventing a reason.

## The chosen workflow

A governed, multi-agent pre-merge quality gate over Komun (`docs/capstone/one-pager.md:3` `a governed, multi-agent pre-merge quality gate over the Komun codebase`), run as workflow 6 in the record (`docs/iteration-log.md:195` `8 harness defects found`). Seven roles run behind one launcher (`PORTING.md:9` `"valid": ["orchestrator", "planner", "implementer", "tester", "reviewer", "project-manager", "researcher"]`), and their grants are the design decision of record (`docs/routing-and-tool-grant-map.md:3` `Which roles run the pre-merge quality gate for`).

- Stakeholder: the person who owns the merge decision (`docs/capstone/plan-pipeline-validation.md:219` `checkpoint rulings, the commits and pushes, and the scope decisions`). The Module 1 PRD names the trigger's actor instead (`docs/prd.md:10` `A developer working from the repo root on the host, with the agent sandbox container running,`). Not recorded at decision time: a stakeholder named by title.
- Trigger: manual, once per run, in the Module 1 lab (`docs/prd.md:13` `This is a manual trigger, fired once per run.`); a pull-request event in the capstone pipeline (`.github/workflows/ci.yml:28` `  pull_request:`).
- Inputs: the pull-request diff, the repository, and the acceptance criteria each role is handed (`docs/routing-and-tool-grant-map.md:3`).
- Outputs: gate results, a review report, an audit trail, and a ticket update; the check surface is `docs/capstone/one-pager.md:17` `Seven deterministic gates check every change: test, clippy, fmt, policy, conformance, webcheck, webtest,`.

## The quantified baseline pain

The work was justified by measured cost. One agentic pass over the prose and citation surface cost `docs/calibration-log.md:104` `45m13s of wall clock for nine operator messages`, with `docs/calibration-log.md:105` `input 2,400 / output 164,438 / cache` and `docs/calibration-log.md:106` `$8.37 at the CLI's own accounting, over 80 distinct API requests`. The workspace test gate cost `docs/iteration-log.md:1361` `Cycle time: 4 minutes 14 seconds` at `docs/iteration-log.md:1363` `Cost per run: $0.4845`, revised by one prompt change to `docs/iteration-log.md:1318` `Cycle time: 2 minutes 33 seconds` at `docs/iteration-log.md:1320` `Cost per run: $0.1635`. Host-side review beside a cycle was `docs/clippy-gate/iteration-log.md:49` `Review latency: ≈4m08s`, and the four-run end-to-end regression totalled `docs/capstone/one-pager.md:29` `a total of 7,051s`.

The error rate that motivated the conversion was drift, not slowness. Run by a model, the same step produced a different report each time; as a script it produced one digest over three runs (`docs/calibration-log.md:118` `15f1c690dbfdb0e1c19f78237836ce1669b47de47c176a98dfe56a943cc61af5`) at `docs/calibration-log.md:116` `zero and no model call` and `docs/calibration-log.md:117` `$0.00`. The recurring defect classes were a citation to the wrong file (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:31` `a citation to the wrong file`), a quoted literal that does not reproduce, and a false positive where a correct citation reads as drift (`docs/calibration-log.md:78` `a pointer whose quoted literal wraps to the next line pairs with a neighbouring literal, so a correct citation reads as drift`).

## Why a custom agentic pipeline, not a simpler automation or a prebuilt agent

A plain CI check cannot express the part that needs judgment. Plan authoring stays with a model (`.claude/agents/planner.md:23` `autonomy: medium`), and the review verdict is not specifiable as a rule set, which is why the ADR refuses to convert it (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:79` `Convert the review verdict instead.`).

A hook cannot see the whole surface. The repository does run a hard stop (`docs/memory-architecture.md:160` `denies any write to the read-only layers`; `:198` ``both the Git hook and the `PreToolUse` hook act at defined points outside the model's decision``), but it answers a single tool call. The citation surface spans repository-wide files re-executed against the tree (`docs/calibration-log.md:119` `299 citations checked, 289 resolved at the cited line, 10 citation findings, and 72 pointers skipped for want of a literal beside them.`). Not recorded at decision time: the record never weighs a PreToolUse hook against the pipeline as a rejected alternative.

The counterweight: work moved out of the agent where the task proved mechanical, stated as policy (`docs/capstone/plan-pipeline-validation.md:217` `a run that has produced the same shaped check twice becomes a script, then a gate, then a CI step`), and the conformance step is recorded as converted (`docs/step-classification.md:32` `agentic until this conversion, now deterministic`). The alternatives the record does consider are named: keep the agentic step, and extend the existing policy suite (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:75` `Keep the agentic step.`; `:87` ``Extend `eval/test_policy.py` instead of adding a script.``); for grants, letting the orchestrator run the gates (`docs/routing-and-tool-grant-map.md:86` ``Let the Orchestrator hold `mcp__gate__run_gate` and run the gates itself.``).

Not established: the record holds no comparison against a prebuilt or off-the-shelf review agent, so that alternative is absent rather than rejected on the record.

## Acceptance criteria, failure modes, demo requirements

Acceptance criteria, each tied to its enforcer:

- Containment (binary gate): `docs/clippy-gate/rubric.md:8` ``No path under `/workspace` other than `docs/clippy-report.md` was``.
- Threshold: `docs/rubric.md:86` `17 / 20 or higher`.
- Conformance coverage, enforced by the gate (`agentic.config.json:79` `"argv": ["python3", "scripts/run-conformance-gate.py"],`): `docs/calibration-log.md:119` `299 citations checked, 289 resolved at the cited line`.
- Policy compliance: `docs/capstone/one-pager.md:31` `95 tests in the policy gate, made up of 75 permission tests, 5 cost-control tests and 15 validator tests.`
- Boundary enforcement: `docs/capstone/one-pager.md:37` `8 were blocked on the first run; P7 and P10 were not.`

The workflow reduces what a reviewer must read; it does not remove review. Before it, a check meant `docs/capstone/one-pager.md:7` `pulling a human or a general-purpose agent through a long review.`

Failure modes: the ten near-miss patterns (`docs/calibration-log.md:14` `## Near-miss patterns for Module 4 governance`); a caught gate failure (`docs/calibration-log.md:166` `` `test` exit 101 (156 passed, 2 failed) ``); a gate red for a pre-existing reason (`docs/calibration-log.md:170` ``Keep `fmt` red in all four runs, unattributed to either change and pre-existing at `HEAD`.``); and a measurement gap (`docs/calibration-log.md:150` `cost is unmeasurable here and no estimate is offered.`).

Demo requirements: a live, recorded gate run with journals measured before and after (`docs/capstone/video/demo-runbook.md:39` `Capture the journal baseline in check 10 because several segments assert that a journal did not grow.`), captured output dated (`docs/capstone/video/demo-runbook.md:13` `Commands were executed and their output captured on 2026-10-01 unless a step says otherwise.`), no synthetic input (`docs/capstone/video/demo-runbook.md:7` `no command in this runbook starts a graphical`), and rollback stated as exercised (`docs/capstone/deck.md:202` `exercised 2026-10-03`).

## Module 1 to 4 artifact gaps

Recorded at the time: the Module 1 sandbox record is `setup.md` (`setup.md:3` `Agentic Engineer · Module 1 · Assignment 1.1, Exercise 2`). The two-agent scope contract is `session_tasks.md` (`session_tasks.md:3` `Scope contract for the two-agent session on the Komun repo.`). The capstone plan itself is tracked at `docs/capstone/plan-pipeline-validation.md`. The Module 1 QC plan is committed in this tree at `docs/capstone/mod1-pre-work-plan.md`, a verbatim copy carrying a provenance note, because `.hermes/plans/` is gitignored (`.gitignore:14` `.hermes/`) and a citation into it cannot travel (`docs/capstone/mod1-pre-work-plan.md:34` `this plan lives there and stays out of history`). The Module 1 lab gate keeps its own PRD, rubric and log under `docs/clippy-gate/`, and the repo-wide record is `docs/iteration-log.md`, whose per-workflow Run 001 entries begin 2026-09-24 (`docs/iteration-log.md:1332` `## Run 001`; `:1212` `19 / 20, PASS`).

Missing or late at capstone start:

- A capstone plan recorded before the work. The pipeline-validation plan is dated 2026-09-30 (`docs/capstone/plan-pipeline-validation.md:1` `validating the agentic pipeline on Komun`), after the four-run regression of 2026-09-29 (`docs/capstone/one-pager.md:29`). Whether an earlier capstone plan existed is not established.
- An end-to-end run log before run 001. Each workflow logs its own Run 001 from 2026-09-24; no multi-role end-to-end entry predates them. Not recorded at decision time.
- A cost model across the pipeline. Per-step figures exist; the four-run regression carries none (`docs/calibration-log.md:150` `cost is unmeasurable here and no estimate is offered.`).
- A stakeholder-facing artifact at the start. The one-pager, deck, impact report and demo runbook are dated 2026-10-01 (`docs/capstone/impact-report.md:3` `Date: 2026-10-01.`), produced after the runs.

Recorded at the time, in two parts. The pre-work state is the Module 1 QC plan's own verified-current-state section, checked 2026-09-24 09:13 CDT before any capstone work and committed here verbatim (`docs/capstone/mod1-pre-work-plan.md:18` `Verified current state`); the list above is assembled from that section and the artifacts' own dates. The near-miss log was written for Module 4 governance, not at the time of the decisions it judges.

## Why not a prebuilt agent, or a simpler script?

Measured on 2026-10-05, on the same task the pipeline's own checker performs: read the prose rules in
`docs/DOC-STYLE.md` and report every violation in one file, with rule code and line number. The
prebuilt agent is `claude -p` (v2.1.280) given one prompt, no allow-list, no routing map, no audit
journal and no classifier deciding whether it may run.

| | Prebuilt agent | Deterministic check |
|:--|:--|:--|
| Findings against the key | 48 and 53 unique, two runs | 20, both runs |
| Agreement with the key | 18 of 20, both runs (90%) | by construction, the key is its output |
| Extras beyond the key | 30 and 35 | 0 |
| Same answer twice | No. The two runs differ by 5 findings on one file and 35 on another | Yes. Three runs over one input produced one SHA-256 digest |
| Cost | $0.44-$0.68 per file, a 55% swing between identical runs | $0.00 |
| Latency | 97-136s per file | 0.42-0.84s |
| Audit trail | A prose list in the transcript | A JSON report plus a row in `.memory/gate-audit.log` |

Three honest limits on those figures. The key is the checker's own output, so the 90% measures
agreement with the checker rather than truth; whether the extras are real violations the regex rules
miss or noise was not adjudicated, and one sample attempt could not settle it because R3 counts words
per sentence, not per line. One run recorded here had a mis-specified task (prose rules against a file
whose findings are all citations) and is excluded from the table above.

What the table does settle is not accuracy. The prebuilt agent recovered 18 of 20 of the checker's
findings, so a single pass is broadly right. What it cannot do is give the same answer twice, say
what a call will cost, or let a reader see what was run: the two identical prompts differ by five
findings, cost moved 55% between them, and the output is a transcript rather than a journal row. No
gate can be built on that, because a gate needs a verdict that repeats and a figure that holds. The
deterministic check is not chosen for finding more; it is chosen because its answer is the same
answer, priced at zero, and recorded where a reviewer can reach it.
