# Impact Report: Komun Pre-Merge Quality Gate

Date: 2026-10-01. Post-capstone results against the Module 1 baseline across five axes: quality, review latency, defect rate, cycle time, cost per run. Every figure is cited to its source line. Two axes are not directly measured here and are marked so, not estimated.

## Executive summary

The largest measured gain is the conversion of the prose and citation conformance step from an agent to a deterministic script. One agentic pass cost "45m13s of wall clock for nine operator messages" (`docs/calibration-log.md:104` `45m13s of wall clock for nine operator messages`) and "$8.37 at the CLI's own accounting, over 80 distinct API requests" (`docs/calibration-log.md:106` `$8.37 at the CLI's own accounting, over 80 distinct API requests`). The replacement runs in "0.844s, 0.424s and 0.422s across the three runs" (`docs/calibration-log.md:115` `0.844s, 0.424s and 0.422s across the three runs`) at "$0.00" (`docs/calibration-log.md:117` `$0.00`) with "zero and no model call" (`docs/calibration-log.md:116` `zero and no model call`).

A second gain came from a test-gate prompt revision (`docs/iteration-log.md:1324` `Cycle time fell from 4m14s to 2m33s (−40%), cost from $0.4845 to $0.1635 (−66%), and model requests from 10 to 5`). Governance moved with it. The policy gate now holds "95 tests in the policy gate, made up of 75 permission tests, 5 cost-control tests and 15 validator tests" (`docs/capstone/one-pager.md:31` `95 tests in the policy gate, made up of 75 permission tests, 5 cost-control tests and 15 validator tests.`), and all ten red-team probes are now blocked (`docs/capstone/one-pager.md:37` `8 were blocked on the first run; P7 and P10 were not.`).

Quality, cycle time and cost are measured. Review latency and defect rate are not directly measured here, and no defect rate is offered, because the record carries no denominator over time.

## Before and after across the five axes

| Axis | Module 1 baseline | Post-capstone | Measured here? |
|---|---|---|---|
| Quality | Conformance scored 11 of 12 (`docs/iteration-log.md:640` `11 / 12, PASS`); Komun "158 passed, 0 failed, 0 ignored" (`AGENTS.md:202` `158 passed, 0 failed, 0 ignored`); vitest "82 tests in 7 files" (`AGENTS.md:206` `82 tests in 7 files`) | "289 resolved at the cited line" of 299 (`docs/calibration-log.md:119` `299 citations checked, 289 resolved at the cited line`); 95-test policy suite (`docs/capstone/one-pager.md:31`); 10 of 10 probes blocked | Yes, by rubric score and citation-resolution rate |
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
- [MEASURED] Rollback safety was exercised on 2026-10-03 (`docs/capstone/deck.md:202` `exercised 2026-10-03`): the revert stops on 4 conflicts, all four files the conversion added. Removing them completes it, and the conformance gate then fails (`scripts/run-conformance-gate.py:206` `is missing, so there is nothing to run`), exit 2 against exit 0 at HEAD.

## Honest limitations

- Review latency and defect rate are not directly measured post-capstone, as marked above. The latency figures the repo keeps measure a scorer's "verification workload and not the wall-clock time to a decision." (`docs/iteration-log.md:722` `above measures the verification workload and not the wall-clock time to a decision.`).
- The conformance checker has two known false positives from wrapped-literal pairing (`docs/calibration-log.md:121` `Two of the ten findings are false positives from the`), pinned by tests rather than repaired.
- `fmt` is red at HEAD and unattributed: "Keep `fmt` red in all four runs, unattributed to either change and pre-existing at `HEAD`" (`docs/calibration-log.md:170` ``Keep `fmt` red in all four runs, unattributed to either change and pre-existing at `HEAD`.``).
- The sandbox boundary has a stated ceiling: "The sandbox protects nothing if the host is compromised, because the broker runs as a normal user on that host." (`docs/capstone/one-pager.md:44` `The sandbox protects nothing if the host is compromised, because the broker runs as a normal user on that host.`).
- The conformance validator reports pre-existing citation drift in documents the capstone did not touch. Measured with the repository's own validator against the HEAD revision of `docs/governance-policy.md`: 27 `CIT-LINE-DRIFT` citations, 133 of 160 citations resolved at the cited line, plus 1 rule finding. The working-tree copy, which carries an unrelated uncommitted seven-line change, shows the same 27 drifts across 161 checked and 134 resolved. The drift is pre-existing at HEAD and introduced by no capstone edit; it is reported so the conformance gate is not read as a clean bill of health.

---

## Review latency, measured (appended 2026-10-05)

The table above marks review latency "Not directly measured" and offers a proxy. It can be measured
directly for the part of the review the pipeline performs, from the audit rows the gate server already
writes. The section is appended rather than edited into the table above because `docs/reflection-log.md:108`
and `docs/capstone/scoping.md:55` cite lines 3 and 41 of this file, and editing the table would move them.

Source: `.memory/gate-audit.log`, every row carrying `duration_seconds`; window 2026-09-28 to 2026-10-04.

| What is measured | Rows | Median | p90 | Max |
|:--|--:|--:|--:|--:|
| The conformance + policy check, which replaces the manual prose-and-citation review | 49 | 2.22s | 3.63s | 4.75s |
| All gated checks, every role | 254 | 2.62s | 7.44s | 37.62s |
| `tester` calls, the role that runs the gates | 207 | 2.86s | 7.44s | 37.62s |

Against the Module 1 baseline's `Review latency: ≈4m08s` (`docs/clippy-gate/iteration-log.md:49`), the
manual portion of the same review now costs a median 2.22s. That is the machine share only: a human
accept/reject decision is not covered by these rows, and no figure is offered for it. The defect-rate
axis stays unmeasured for the reason given above, and no rate is invented here.

---

## Defect rate, counted from the record (appended 2026-10-05)

The table above marks this axis "No. A rate needs a denominator the record does not carry." The record
does carry one population, and it can be counted. The section is appended rather than edited into the
table for the reason given above: two documents cite lines 3 and 41 of this file.

Defects caught per gated check, from `.memory/gate-audit.log`, 254 rows covering 2026-09-28 to 2026-10-04:

| Population | Caught | Checks | Rate |
|:--|--:|--:|--:|
| Every gated check | 65 | 254 | 25.6% |
| Excluding `fmt` | 10 | 186 | 5.4% |

The second row is the honest one for a comparison. The `fmt` gate is red for a pre-existing reason that
neither change under review caused (`docs/calibration-log.md:170`), and it accounts for 55 of those 65
caught failures, so counting it measures the repository's standing state rather than the pipeline's
ability to catch a defect.

Defects named per recorded run, from the run headings in `docs/iteration-log.md`: 12 named across 16
runs, 0.75 per run. That is a floor and not a rate. Only the headings were counted, so a run that found
a defect without naming it there is not counted here.

## Is that comparable to the Module 1 baseline?

No, and saying so is the finding. The baseline's record carries one run (`docs/clippy-gate/iteration-log.md:12`),
a clippy pass over one workflow. It found no lint violations and left the workspace tests passing at exit
0 with 158 passed and 0 failed (`docs/clippy-gate/iteration-log.md:76`), scoring 19 / 20 at 2m26s and
$0.68125 (`docs/clippy-gate/iteration-log.md:88`).

That is one sample of a different kind of check. Setting 1 run against 254 gated checks would measure
the denominators rather than the defect rates. The axis is now counted on the capstone side, one
recorded run stands on the baseline side, and no rate-to-rate comparison is supported by the record.
