W2 is done and the server is restarted. Answers below.

1. W2: I appended the correction section to the end of docs/iteration-log.md myself, taking option (a),
   and both [UNVERIFIED] items are now settled.
   - The claim at :1403 is indeed the wrong one. I read it: `- Name the table the close step writes to.
     No ticket or plan table exists in the storage database`.
   - The origin is settled with git, not asserted: gate_vocabulary.py at 9db9679^ is 286 lines with
     AUDIT_PATH at 101, and at 9db9679 it is 290 lines with AUDIT_PATH at 105. The appended section
     states that as fact, so the [UNVERIFIED] marker and its two "Claims needing verification" bullets
     are gone.
   Every citation in the appended section was checked against its target line, and the conformance gate
   passes with the append in place.

2. The gate server is restarted on :8003 and is running the W4 loader.

3. Optional decisions.
   - PORTING.md:126: I fixed BOTH lines. Measured inside the container: test_policy.py collects 80
     (the doc said 75) and test_deterministic_step.py collects 18 (the doc said 15). W6's three tests
     are what make :126 false, so :126 is this run's doing; :125 was already stale before it. Both lines
     now carry the measured numbers and timings.
   - Keep the reworded SCHEMA.md:459 and selftest.py:7. Your reasoning is right: both still state what
     is true.
   - The 88 extra lines in gate_vocabulary.py are acceptable. The citations below the change were
     repaired and rechecked, and the loader's cases are documented in the code they govern.
   - The two read-only claims are fixed. W3 made them false, so I reworded ci.yml:183-184 and
     ci-step-design.md:181-182 in place at unchanged line counts.
   - Recorded, not fixed: ci-step-design.md:187 says the CI step runs `policy` and `conformance`, but
     ci.yml runs test, clippy and fmt; and PORTING.md carries 37 citations that need attention, mostly
     into agentic.config.json, which has drifted roughly 112 lines. Both predate this run and neither is
     this change's doing.

4. Four files in the working tree are MINE rather than the implementer's, and the reviewer should review
   them as part of the tree: docs/iteration-log.md (the appended correction), PORTING.md:125-126,
   .github/workflows/ci.yml:183-184 and docs/ci-step-design.md:181-182.

Now run the gates: all seven, with test and clippy twice each, then the reviewer, then stop at Human
checkpoint 2.
