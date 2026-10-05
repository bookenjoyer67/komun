Human checkpoint 1: the plan a4b4de35-d382-4edb-bb50-16361a5ac33b is APPROVED as written. Run all
seven gates, with test and clippy twice each for the warm-cache case. The run's scope is widened to
cover the three findings your planner raised, plus one wording item. Keep the plan's structure; add
these as work items.

WHAT TO FIX, IN THIS RUN

1. F1, a false citation. mcp/gate/SCHEMA.md:246 quotes mcp/gate/server.py:286 as
   `match = GUARD_MARKER_PATTERN.search(strip_ansi(combined_output))`. That line now reads
   `match = GUARD_MARKER_PATTERNS[command].search(strip_ansi(combined_output))`. The commit kept
   every line number and every line count, so the citation still resolves as a location; the quoted
   text is what went stale. Update the quoted literal to the text the line now carries.

   Then sweep the whole class, not just this instance. Check every file that cites a line that
   e5dfcec changed: the module docstring around server.py:15-18, the import at :56, the guard
   function at :282-286, the call at :418, and the comment block and helper at
   gate_vocabulary.py:145 and :258-268. A citation whose quoted literal no longer matches the line
   it names is false, whatever its line number. Report what the sweep found, including files whose
   citations are still correct.

2. F1b, a wrong finding in the log. docs/iteration-log.md carries a section titled
   `Run run-2026-10-03-matchstatus-is-resolved`, and it says the ticket close step cannot run
   because the storage layer has no ticket or plan table. That mechanism is wrong. The ticket tool
   is a simulation with no store at all:
   `mcp/coursetools_server.py:183` `def task_tracker(role: str, ticket_id: str, status: str = "done", note: str = "") -> str:`
   and `:184` `"""Simulate updating a shared work ticket."""`. It reads nothing and writes nothing.
   Correct the section to say what is actually true, and say what that means: a close step can
   report success while persisting nothing, and no record corroborates a ticket action. Keep the
   section's question-first shape and its citations; the conformance gate must still pass.

3. F2, the CI path. .github/workflows/ci.yml:198 mounts the workspace read-only and :199 makes only
   crates/server/src/main.rs writable. The test gate's guard touches crates/core/src/tests.rs, so in
   CI a green suite still reports passed=False. Make the mount set able to satisfy the test gate's
   guard too, without making the whole workspace writable.

4. F3, the pattern loader. gate_vocabulary.py's `_guard_pattern` validates less than its neighbour
   `_summary` does. An empty pattern matches everything, so the guard silently always passes. A
   guarded command with no pattern compiles the literal text "None", so the guard silently always
   fails. A broken pattern with no usable default stops the server from starting. Validate the way
   `_summary` does and record which case each branch handles.

5. Wording. Four places still name only clippy where the guard is now per-command:
   gate_vocabulary.py:5, server.py:42, server.py:244, SCHEMA.md:225. Generalise them, so the
   commit's claim that the naming was generalised is true.

CONSTRAINTS

- These files are cited by line across the repository. mcp/gate/server.py has 83 cited lines and
  gate_vocabulary.py has 9. Change line counts only where a fix genuinely needs it, and when a cited
  line does move, update every citation that moves with it. Run the conformance gate before you
  call the work done; it compares your tree against HEAD and will report drift.
- AGENTS.md is a protected agent-instruction file. Do not edit it.
- The change is governed: mcp/** is in classification.governed_globs, and so is
  .github/workflows/ci.yml if the classifier says so. Run the gate the classifier requires.
- Do not run git commands; no role here has a shell. The human will run the git checks you ask for.

AFTER THE FIXES

Send the tester through the gates as the plan describes, then have the reviewer review the working
tree against e5dfcec, then stop at Human checkpoint 2.

ONE THING TO EXPECT AT THE CLOSE

The project manager's task_tracker tool is the simulation described in F1b, so closing the ticket
persists nothing. Record the close as a no-op with that reason rather than reporting it as a
completed action, and say so in the summary.
