---
classification: internal
project: proj-komun
doc_type: standard
---

# Standard: reproduce a count before reporting it

What must be re-run before a number or a denominator is reported as fact?

A count is a claim, and its authority is the search that produced it (`docs/DOC-STYLE.md:38` `a search and its count`). A number copied from another document is a second document, not evidence, so re-run the command in this tree before reporting it. The worked example in the documentation standard does not reproduce here: the recorded search returns 47 occurrences across 18 files (`docs/memory-architecture.md:39` `does not reproduce — 47`).

## Why is an exit code not a measurement?

A cached tool run prints nothing, and a silent second run is a cache hit rather than a clean result (`docs/DEVELOPMENT.md:256` `A second run with no source change prints nothing at all`). Compare numbers, not just exit codes, and re-measure after each change (`docs/DEVELOPMENT.md:263` `Compare numbers, not just exit codes`). A build never shows lint output, so a warning claim must come from the command that prints it (`docs/DEVELOPMENT.md:259` `never shows clippy lints`).

## How is a reported total checked?

Tie every reported total to the runner's own count lines, including per-crate counts (`` `docs/rubric.md:59` `Do the reported per-crate passed/failed counts match the runner's own `test result:` lines?` ``). Distinguish tests that ran from filtered-out or ignored ones, and record the state the measurement came from (`AGENTS.md:201` `Measured 2026-09-25 in the agent sandbox`). A score or denominator is assigned against the intended behaviour, never adjusted to fit a run (`docs/rubric.md:90` `Scores are assigned against the PRD's intended behavior, not adjusted to fit a run.`).
