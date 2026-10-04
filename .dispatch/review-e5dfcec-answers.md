Human answers to your five questions. Two instructions in my last brief were wrong; the corrections
below are mine to own, and you were right to stop.

1. OQ1, W2: add a CORRECTION SECTION at the end of docs/iteration-log.md. Do not edit the matchstatus
section in place. My brief said to correct it in place, and that would break docs/iteration-log.md:4
`Entries are never deleted or rewritten, and the commits that add them are never squashed.` Your
precedent is the right shape: docs/iteration-log.md:905 `Correction to Run 002 (M5) — found by
re-executing its citations after the fact:`. The new section carries the same question-first shape
and citations, states the correct mechanism (the ticket tool is a simulation with no store), and
supersedes the old section's quote of gate_vocabulary.py:263 by quoting the line as it now reads.

2. OQ4, F3: your reading is right and the planner's is wrong. Keep the server running. Check that the
pattern is a non-empty string and that it compiles; on any failure fall back to the built-in default;
and when there is no usable default, fail only that gate's guard, with a message that names the
config key. A startup failure is one of the three behaviours I asked to remove, so it cannot be the
fix. Send that back to the planner as a defect before the implementer starts.

3. OQ2: yes, fix all ten off-by-four citations in this run, and record their origin correctly. Your
premise is wrong and I have the evidence. The +4 did not come from e5dfcec: gate_vocabulary.py is
290 lines at both e5dfcec^ and e5dfcec, with AUDIT_PATH at line 105 in both. It came from 9db9679,
the test-gate guard commit, which took that file from 286 lines with AUDIT_PATH at 101 to 290 lines
with AUDIT_PATH at 105. The ten citations were falsified by 9db9679, not by the commit under review.

4. OQ3: yes, add the other places that still name only clippy to W5 — ci.yml:210, server.py:77,
SCHEMA.md:438, selftest.py:7 and the config's language_specific_note. The point of W5 is to make the
commit's claim true, so the list should be exhaustive rather than the four I named.

5. OQ5: yes, add a unit test for the F3 cases to eval/test_deterministic_step.py, so the policy gate
checks the behaviour rather than a reviewer's reading of it. Cover an empty pattern, a pattern that
will not compile, and a guarded command with no usable default. Pin the non-fatal behaviour you chose
in OQ4, and name the config key in the message the guard reports.

6. One correction to your line estimate. You reported that the planner found no citations below
gate_vocabulary.py:263, but mcp/gate/SCHEMA.md cites gate_vocabulary.py:269, and that citation is
already four low (the real line is 273). Adding lines from :267 down moves it again, so the
implementer must repair that citation with both the old drift and the new shift accounted for. Derive
the citing-file list by searching the tree for the filename rather than from a list I supply, and run
the check over every file that cites either changed file, not just the ones a gate reads.

Then: implementer, then stop so I can restart the gate server, then the tester, then the reviewer,
then Human checkpoint 2.
