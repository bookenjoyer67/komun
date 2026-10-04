A second governed change needs its own review. It is already written and committed as e5dfcec, so
your plan is expected to be verification and review rather than new code.

WHAT THE COMMIT FIXES

The gate server compiled a single cache-hit guard marker pattern, from
toolchain.commands.clippy.guard.marker_regex, and matched every guarded gate's output against it.
A gate's verdict is passed = (exit_code == 0 and guard.satisfied) for every gate, so the test gate,
whose own marker is "Compiling komun-core", was searched for clippy's "Checking komun-server" and
could never be satisfied. A green test suite therefore reported passed=False.

WHAT THE COMMIT CHANGES

- mcp/gate/gate_vocabulary.py builds GUARD_MARKER_PATTERNS, one compiled pattern per guarded
  command, beside the existing SUMMARY_PATTERNS and by the same rule, with a per-command fallback to
  the embedded default when a fork's pattern does not compile.
- mcp/gate/server.py guard_satisfied takes the calling command and matches with that command's own
  pattern instead of a module-level one.
- The module docstring and two comments that named clippy were generalised, because they described
  the single pattern that is gone.
- Both files keep their exact line counts, server.py 602 lines and gate_vocabulary.py 290 lines,
  because 83 lines of server.py and 9 of gate_vocabulary.py are cited elsewhere in the repository.

EVIDENCE ALREADY IN HAND

A scratch server instance on port 8013, with its own audit path so the matchstatus run's journal was
untouched, returned for the test gate: exit=0, passed=True, verdict=pass, guard applied=True,
satisfied=True, touched=/workspace/crates/core/src/tests.rs, marker="Compiling komun-core", detail
"found 'Compiling komun-core' in the cargo output". For clippy it returned exit=0, passed=True,
satisfied=True. The gate journal row at 2026-10-03T18:57:21 records the same for the test gate in a
real run, which is the row committed as a91ecb9.

WHY THIS IS GOVERNED

mcp/** is in classification.governed_globs, so this change needs its own governed check. It is not
part of run run-2026-10-03-matchstatus-is-resolved, which is complete and delivered as 8e9aa2f.

YOUR JOB

Produce the plan, then stop at Human checkpoint 1 for approval before going further. Expect the plan
to cover: which gates to run and why each one is relevant to a change in mcp/**, what evidence the
reviewer needs to judge the commit's diff, and what could still be wrong with the fix. The two
questions worth putting to the reviewer are whether the per-command patterns can go missing for a
guarded command, and whether any caller of guard_satisfied or apply_cache_hit_guard was missed.

CONSTRAINTS

AGENTS.md is a protected agent-instruction file. Do not edit it. Do not run git commands; you have no
shell. The repository is at /workspace and the working tree is clean apart from two untracked files
under .dispatch/.
