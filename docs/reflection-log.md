# Reflection log

Which existing records does this log package, and what does each entry show?

This log was assembled retrospectively from records that already carry their evidence. It adds no claim those records do not carry. Read it as an index into them.

## Which records does this log draw on?

Which files feed the entries below?

- Draw on `docs/iteration-log.md` for runs 001 to 008 (`docs/iteration-log.md:1` `Iteration Log`).
- Draw on `docs/calibration-log.md` for the near-miss patterns (`docs/calibration-log.md:18` `Ten patterns follow, each named and each cited to the line that evidences it`).
- Draw on `docs/memory-architecture.md` for the memory layers and their enforcement (`docs/memory-architecture.md:157` `## Enforcement`).
- Draw on the conversion ADR for the deterministic replacement (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:1` `# ADR-001: Convert the prose and citation conformance check from agent to deterministic code`).
- Draw on `eval/red-team-results.md` for the policy-bypass probes (`eval/red-team-results.md:7` `Eight prompts were blocked on their first run.`).
- Draw on the live `.memory/` layer for scope and index (`.memory/SCOPE.md:3` `Project: Komun`).
- Read the citation rule before any entry (`docs/DOC-STYLE.md:40` `A bare location is a v1 form and fails R2 in v2`).

## Entry 1 — Which agent-role change rewrote the contract auditor's evidence rules?

Which run exposed the defect, and what did its reruns move?

- Record the source run as workflow 3's first contract audit (`docs/iteration-log.md:1085` `## Run 001 (workflow 3`).
- Name the agent and the version under test (`docs/iteration-log.md:1088` `Agent: komun-contract-auditor, version`).
- Name the defect class as two citation faults under D2 (`docs/iteration-log.md:1108` `at least one cited pointer does not resolve and one count does not reproduce`).
- Cite the first fault, a pointer that does not resolve (`docs/iteration-log.md:1124` `the citation for the ed25519 verdict does not resolve`).
- Cite the second fault, a count that does not reproduce (`docs/iteration-log.md:1134` `a stated count does not reproduce`).
- State the first change as an Evidence rules section added to `.claude/agents/komun-contract-auditor.md` (`docs/iteration-log.md:1157` `agent: komun-contract-auditor v0.1.1 -- require verbatim evidence for every verdict`).
- Read the added section in place (`.claude/agents/komun-contract-auditor.md:32` `## Evidence rules`).
- Record the after for D2, now at level 3 (`docs/iteration-log.md:992` `Level 3 is now met everywhere`).
- Record the rubric total moving from 14 to 15 of 16 (`docs/iteration-log.md:995` `| **Total** | **14 / 16** | **15 / 16** |`).
- State the second change as a verification pass that prints the literal output (`docs/iteration-log.md:894-895` `verify the report's own citations and numbers`).
- Read the verification pass in place (`.claude/agents/komun-contract-auditor.md:46` `## Verification pass (do this before printing)`).
- Record the second after for D2, now at level 4 (`docs/iteration-log.md:819` `| D2 Evidence Traceability | 2 | 2* | **4** |`).
- Record the rubric total at 16 of 16 (`docs/iteration-log.md:822` `**16 / 16**`).

## Entry 2 — Which skill file was added, and what starvation does it answer?

Which near-miss produced the skill, and what held after the addition?

- Date the session from the capstone package (`docs/capstone/impact-report.md:3` `Date: 2026-10-01`).
- Name the near-miss as a starved planning role (`docs/calibration-log.md:30` `a planning role denied its reads plans from a ten-line brief`).
- Carry the run evidence behind that near-miss (`docs/iteration-log.md:412-413` `its plan rested entirely on the ten-line quotation`).
- Read the skill's own rationale, which names it (`.claude/skills/write-child-brief/SKILL.md:15` `the starvation near-miss recorded as NM-2`).
- State the change as the new skill definition (`.claude/skills/write-child-brief/SKILL.md:2` `name: write-child-brief`).
- Record the governance update that admits the skill (`docs/governance-policy.md:48` `the repository ships two skill files`).
- Record the after as the policy suite staying green (`docs/capstone/video/demo-runbook.md:523` `Verified on 2026-10-01: 90 passed in 1.00s`).
- Record that suite's size (`docs/capstone/one-pager.md:31` `90 tests in the policy gate, made up of 75 permission tests plus 15 validator tests.`).
- Record no rerun for the skill's activation: not recorded at decision time.

## Entry 3 — Which memory-layer policy change hardened the write path?

Which drill failed, and what did the hardened path then hold?

- Record the source run as workflow 5's memory failure-mode testing (`docs/iteration-log.md:535` `## Run 003 (workflow 5 — failure-mode testing of the memory system)`).
- Record the run's verdict, three safeguards of three (`docs/iteration-log.md:535` `3 / 3 safeguards held`).
- Name the failing drill, a planted secret (`docs/iteration-log.md:571` `Failure mode 3`).
- Carry the observation that the mode bits bound nothing (`docs/iteration-log.md:630` `root ignores those bits`).
- Carry the measured conclusion behind that (`docs/iteration-log.md:631` `A guardrail that does not bind the process doing the writing is not a guardrail`).
- State the first change as a classification check placed first in the write policy (`CLAUDE.md:64` `Before writing any content to a memory file, classify it:`).
- Read the architecture's matching clause (`docs/memory-architecture.md:190` `an explicit four-level classification check placed first in the write policy`).
- Read the run's own change ledger for that edit (`docs/iteration-log.md:581` `an explicit four-level classification check added as the first rule of the write`).
- State the second change as a versioned pre-commit hard stop (`docs/iteration-log.md:588` `scripts/hooks/pre-commit`).
- Read the enforcement split that names it as a hard stop (`docs/memory-architecture.md:162` `Secrets at commit: the pre-commit hook blocks commits matching common credential patterns`).
- Record the after as the hook blocking the commit with exit 1 (`docs/iteration-log.md:575` `was blocked by the new pre-commit hook with exit 1 and nothing written to history`).
- Read the standing secret rule the change enforces (`docs/memory-architecture.md:151` `Must never appear in any memory file.`).

## Entry 4 — Which config change made the gate counts readable?

Which defect did the config edit close, and what did the selftest then report?

- Record the source run as workflow 7, the gate's own output summaries (`docs/iteration-log.md:86` `Run 007 (workflow 7 — the gate's own output summaries)`).
- Carry the before, that no role could read the payload (`docs/iteration-log.md:110` `No role could read it`).
- Carry the counts that existed only inside that payload (`docs/iteration-log.md:136` `{"hunks": 213, "files": 35, "added": 1101, "removed": 2475}`).
- Name the closed defect (`docs/iteration-log.md:158` `Defect 4 closed`).
- State the change as a summary rule added to `agentic.config.json` (`docs/iteration-log.md:120` `agentic.config.json gained a summary object per command`).
- Read the server half that reports the counts twice (`docs/iteration-log.md:123` `The server counts before the clamp and reports the result twice`).
- Name the second defect closed in the same run (`docs/iteration-log.md:160` `Defect 6 closed`).
- Record the before for the selftest predicate (`docs/iteration-log.md:294` `SELFTEST_RESULT passed=34 total=34`).
- Record the after, the selftest at 40 of 40 (`docs/iteration-log.md:49` `SELFTEST_RESULT passed=40 total=40`).

## Entry 5 — Which agent step became a deterministic script?

What did the agentic pass cost, and what does the script cost now?

- Read the classification that marks the step converted (`docs/step-classification.md:43` `Step: prose and citation conformance check — CONVERTED`).
- State why the failure class was mechanical (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:37` `class is mechanical rather than motivational.`).
- Carry the last agentic defect of that class (`docs/iteration-log.md:852` `M6 — one citation is one line off (D2, minor).`).
- State the change as the deterministic replacement (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:49` `Replace the agentic step with scripts/validate_doc_conformance_deterministic.py`).
- Record the before cycle time (`docs/calibration-log.md:104` `45m13s of wall clock for nine operator messages`).
- Record the before spend (`docs/calibration-log.md:106` `$8.37`).
- Record the after cycle times (`docs/calibration-log.md:115` `0.844s, 0.424s and 0.422s across the three runs`).
- Record the after spend as zero (`docs/calibration-log.md:117` `$0.00`).
- Record the no-model clause behind that zero (`docs/calibration-log.md:116` `zero and no model call`).
- Record the digest that proves repeatability (`docs/calibration-log.md:118` `15f1c690dbfdb0e1c19f78237836ce1669b47de47c176a98dfe56a943cc61af5`).
- Record the integration result, four runs passing (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:10` `the integrated end-to-end regression passed on 2026-09-29 across four runs`).
- Record the coverage of the integrated runs (`docs/calibration-log.md:119` `299 citations checked, 289 resolved at the cited line`).

## What is this log's provenance?

Which records was this log assembled from, and what does it add?

The assembly was retrospective: the runs and drills happened first, and this file quotes them after the fact.

- State that this log was assembled after the runs it describes, not during them.
- Name the sources: `docs/iteration-log.md`, `docs/calibration-log.md`, `docs/memory-architecture.md`, the ADR set under `docs/adr/`, `eval/red-team-results.md` and the live `.memory/` layer.
- State that every entry quotes its source at the cited line rather than paraphrasing it.
- State that this log introduces no claim those records do not already carry.
- State that any decision whose reasoning was not recorded at decision time is marked as such, not composed.
- State that this file is the only file the assembly adds, and that it makes no git commit.
