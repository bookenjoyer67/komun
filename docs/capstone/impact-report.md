# Impact Report: Komun Pre-Merge Quality Gate

Date: 2026-10-01. Post-capstone results against the Module 1 baseline across five axes: quality, review latency, defect rate, cycle time, cost per run. Every figure is cited to its source line. Two axes are not directly measured here and are marked so, not estimated.

## Executive summary

The largest measured gain is the conversion of the prose and citation conformance step from an agent to a deterministic script. One agentic pass cost "45m13s of wall clock for nine operator messages" (`docs/calibration-log.md:104` `45m13s of wall clock for nine operator messages`) and "$8.37 at the CLI's own accounting, over 80 distinct API requests" (`docs/calibration-log.md:106` `$8.37 at the CLI's own accounting, over 80 distinct API requests`). The replacement runs in "0.844s, 0.424s and 0.422s across the three runs" (`docs/calibration-log.md:115` `0.844s, 0.424s and 0.422s across the three runs`) at "$0.00" (`docs/calibration-log.md:117` `$0.00`) with "zero and no model call" (`docs/calibration-log.md:116` `zero and no model call`).

A second gain came from a test-gate prompt revision (`docs/iteration-log.md:1324` `Cycle time fell from 4m14s to 2m33s (−40%), cost from $0.4845 to $0.1635 (−66%), and model requests from 10 to 5`). Governance moved with it. The policy gate now holds "90 tests in the policy gate, made up of 75 permission tests plus 15 validator tests" (`docs/capstone/one-pager.md:31` `90 tests in the policy gate, made up of 75 permission tests plus 15 validator tests.`), and all ten red-team probes are now blocked (`docs/capstone/one-pager.md:37` `8 were blocked on the first run; P7 and P10 were not.`).

Quality, cycle time and cost are measured. Review latency and defect rate are not directly measured here, and no defect rate is offered, because the record carries no denominator over time.

## Before and after across the five axes

| Axis | Module 1 baseline | Post-capstone | Measured here? |
|---|---|---|---|
| Quality | Conformance scored 11 of 12 (`docs/iteration-log.md:640` `11 / 12, PASS`); Komun "158 passed, 0 failed, 0 ignored" (`AGENTS.md:202` `158 passed, 0 failed, 0 ignored`); vitest "82 tests in 7 files" (`AGENTS.md:206` `82 tests in 7 files`) | "289 resolved at the cited line" of 299 (`docs/calibration-log.md:119` `299 citations checked, 289 resolved at the cited line`); 90-test policy suite (`docs/capstone/one-pager.md:31`); 10 of 10 probes blocked | Yes, by rubric score and citation-resolution rate |
| Review latency | Only a reviewer-workload figure, "Review latency: ≈4m08s" (`docs/clippy-gate/iteration-log.md:49` `Review latency: ≈4m08s`) | Not directly measured. Proxy: the deterministic check's "0.422s" (`docs/calibration-log.md:115` `0.844s, 0.424s and 0.422s across the three runs`), the text-review portion no longer needing an agent. Fair only for the manual-review portion the script replaces, not for a human accept/reject decision | No. Proxy named |
| Defect rate | Caught defects recorded: "`test` exit 101 (156 passed, 2 failed)" (`docs/calibration-log.md:166` `` `test` exit 101 (156 passed, 2 failed) ``) | Same character: "8 harness defects found" (`docs/iteration-log.md:195` `8 harness defects found`) and 10 conformance findings (`docs/calibration-log.md:119`) | No. A rate needs a denominator the record does not carry. Proxy: caught defects per run, as counts |
| Cycle time | Test gate "4 minutes 14 seconds" (`docs/iteration-log.md:1361` `4 minutes 14 seconds`); conformance "45m13s of wall clock" (`docs/calibration-log.md:104`) | "2 minutes 33 seconds" (`docs/iteration-log.md:1318` `2 minutes 33 seconds`); "0.844s, 0.424s and 0.422s" (`docs/calibration-log.md:115`); regression "a total of 7,051s" (`docs/capstone/one-pager.md:29` `a total of 7,051s`) | Yes, all three |
| Cost per run | "$0.4845" per test-gate run (`docs/iteration-log.md:1363` `$0.4845`); "$8.37" per conformance pass (`docs/calibration-log.md:106`) | "$0.1635" (`docs/iteration-log.md:1320` `$0.1635`); "$0.00" (`docs/calibration-log.md:117`) | Yes for both steps; no regression cost, "cost is unmeasurable here and no estimate is offered" (`docs/calibration-log.md:150` `cost is unmeasurable here and no estimate is offered`) |

## Improvement cycles

Two cycles have both halves on the record: the change, and the rerun that confirmed it.

**Cycle 1, test-gate prompt revision.** Made from run evidence: the baseline run failed containment, "Three violations: `npm install` run twice (PRD out-of-scope)" (`docs/iteration-log.md:1358` `Three violations: `npm install` run twice (PRD out-of-scope)`). Rerun, in the same log: "4m14s to 2m33s", "$0.4845 to $0.1635", model requests "from 10 to 5" (`docs/iteration-log.md:1324` `Cycle time fell from 4m14s to 2m33s (−40%), cost from $0.4845 to $0.1635 (−66%), and model requests from 10 to 5`). Same step, same inputs, both numbers recorded. Measured.

**Cycle 2, deterministic conversion.** Made from run evidence: the same defect class recurred even after the agent was told to quote evidence (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:37` `class is mechanical rather than motivational.`). That step cost 45m13s and $8.37 per pass (`docs/calibration-log.md:104` `45m13s of wall clock for nine operator messages`). Rerun: three script runs over one input produced one digest, `15f1c690dbfdb0e1c19f78237836ce1669b47de47c176a98dfe56a943cc61af5` (`docs/calibration-log.md:118` `15f1c690dbfdb0e1c19f78237836ce1669b47de47c176a98dfe56a943cc61af5`). The integrated regression "passed on 2026-09-29 across four runs" (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:10` `the integrated end-to-end regression passed on 2026-09-29 across four runs`). Measured.

**Carried, not confirmed.** Two governance fixes, NM-3 (union permissions) and NM-4 (self-approved provenance), have no rerun. The log states "Rerun evidence: none yet." (`docs/iteration-log.md:448` `Rerun evidence: none yet.`). For the second: "Rerun evidence: none yet; the checkpoint-2 entry for this run carries the disclosure instead" (`docs/iteration-log.md:469` `Rerun evidence: none yet; the checkpoint-2 entry for this run carries the disclosure instead.`). They are fixes without confirming reruns, stated as such.

## What is measured and what is projected

Measured gains (both numbers recorded, same step, same inputs): the conversion's 45m13s to 0.422s and $8.37 to $0.00; the prompt revision's 4m14s to 2m33s and $0.4845 to $0.1635; the regression's per-run wall clocks; 10 of 10 red-team probes; the 90-test policy suite. Derived, not a new measurement: "roughly 6,400 times faster" is 2,713 s divided by 0.42 s, a ratio of two recorded numbers (`docs/capstone/one-pager.md:26` `roughly 6,400 times faster`).

Projected gains, each with its assumption stated:

- [PROJECTED] "with $8.37 removed per run" describes future passes; the assumption is one conformance pass per merged change, and the record carries no count of changes, so no annual figure is claimed (`docs/capstone/one-pager.md:26` `with $8.37 removed per run`).
- [PROJECTED] The regression will be costable once the broker reports usage; today "No cost figure exists for the four-run regression, because the broker reports no usage." (`docs/capstone/one-pager.md:43` `No cost figure exists for the four-run regression, because the broker reports no usage.`).
- [PROJECTED] Rollback safety is a design claim, not a result: "not yet exercised" (`docs/capstone/deck.md:202` `not yet exercised`); the assumption is that one `git revert` of the conversion commit undoes it.

## Honest limitations

- Review latency and defect rate are not directly measured post-capstone, as marked above. The latency figures the repo keeps measure a scorer's "verification workload and not the wall-clock time to a decision." (`docs/iteration-log.md:722` `above measures the verification workload and not the wall-clock time to a decision.`).
- The conformance checker has two known false positives from wrapped-literal pairing (`docs/calibration-log.md:121` `Two of the ten findings are false positives from the`), pinned by tests rather than repaired.
- `fmt` is red at HEAD and unattributed: "Keep `fmt` red in all four runs, unattributed to either change and pre-existing at `HEAD`" (`docs/calibration-log.md:170` ``Keep `fmt` red in all four runs, unattributed to either change and pre-existing at `HEAD`.``).
- The sandbox boundary has a stated ceiling: "The sandbox protects nothing if the host is compromised, because the broker runs as a normal user on that host." (`docs/capstone/one-pager.md:44` `The sandbox protects nothing if the host is compromised, because the broker runs as a normal user on that host.`).
- The conformance validator reports pre-existing citation drift in documents the capstone did not touch. Measured with the repository's own validator against the HEAD revision of `docs/governance-policy.md`: 27 `CIT-LINE-DRIFT` citations, 133 of 160 citations resolved at the cited line, plus 1 rule finding. The working-tree copy, which carries an unrelated uncommitted seven-line change, shows the same 27 drifts across 161 checked and 134 resolved. The drift is pre-existing at HEAD and introduced by no capstone edit; it is reported so the conformance gate is not read as a clean bill of health.
