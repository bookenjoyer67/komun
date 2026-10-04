# Ruling — Checkpoint 1, run-2026-10-03-secaudit

**Label: harness-supplied on the human's behalf.** The human's entire instruction was the single
word *"Approve"*. It is reproduced here with no expansion. Where this document answers an item, it
says so; where it does not, it says that too. Do not read a scope into it that is not written.

Source: the Checkpoint 1 plan delivered in session `208bd53c-6619-4b7d-9fb6-507194496fe7`
(72 distinct issues: 1 Critical, 11 High, 21 Medium, 39 Low; 14 groups R1–R14).

## Item 1 — approve the plan, or amend it

**APPROVED AS WRITTEN.** The findings, the 14 fix groups, their grouping and their suggested order
are accepted unchanged. No amendment was requested. The departure note in the plan
("Where the plan departs from your ruling") is noted and is not treated as a defect.

## Item 2 — choose which groups become change runs, and in what order

**NOT ANSWERED by this ruling.** The human did not name a group. Therefore:

- Do **not** start R1, R7 or any other group.
- Do **not** treat "approved" as authorization to begin the first change run.
- R1 and R7 being "ready right away" is an observation about readiness, not an instruction.

## Item 3 — answer whichever decisions those groups wait on

**NOT ANSWERED.** D1–D14 remain open, exactly as the plan lists them. No default is chosen on the
human's behalf. An item on which the plan says no decision is needed (for example R10, R14) needs
nothing from the human.

## What this ruling authorizes, and what it does not

- **Authorized:** recording this approval; continuing to read, plan, rank and report.
- **Not authorized:** any code change, any `migrations/` change, any wasm or auth change, any
  commit or push, any change run, any implementer/tester/reviewer work.
- **Re-approval still required** at the point any group touches auth code or `migrations/`, per the
  plan's own last line.

## What the human owes you next

Item 2 — which groups become change runs and in what order — and then only the D-numbers those
chosen groups wait on. Until item 2 arrives, the correct next action is to acknowledge this ruling
and stop again; a second question to the human that re-asks item 1 is not useful.
