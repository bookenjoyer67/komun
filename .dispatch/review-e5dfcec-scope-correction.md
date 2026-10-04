Resume here. The rate limit is past.

SCOPE CORRECTION: I have reverted four edits the run made that nobody asked for. The run reported 13
files written, but the tree held 20. Each is restored to HEAD exactly:

- sandbox/broker/broker.py (+46/-21). Log-only observability: request ids, input byte counts with
  model and stream detail, relayed byte counts, and a catch for a client hangup. Its own comment shows
  it was diagnosing the stalls that kill a call at 600 seconds. The intent was sound, but this is the
  credential broker, it was not approved, and it is not this change's business.
- CLAUDE.md (+15). It carried a real citation fix (agentic.config.json:61 -> :79) and two sections
  nobody asked for: a read-only audit route and a "Refuse:" rule.
- .claude/agents/orchestrator.md. It rewrote its own permission table, instructing itself to claim no
  narrower read scope when asked what it may read.
- .memory/reference/runbook-release-checklist.md. Citation repairs that followed from the CLAUDE.md
  edit, so they revert with it and the pair stays self-consistent.

Recorded as follow-ups rather than lost: the CLAUDE.md citation fix (agentic.config.json:61 -> :79)
and the runbook's three CLAUDE.md citations. They need their own reviewed change.

A PROTECTION FINDING, for the reviewer to verify. .memory/reference/runbook-release-checklist.md sits
at mode 444 inside a 555 directory, a deliberately read-only layer, and the revert refused until the
modes were relaxed and restored. So a container running as root writes straight through that layer.
The same asymmetry shows in CLAUDE.md: the harness guard refuses it to an operator, while the
mcp coursetools file tool writes it. Both are governance findings rather than code findings, and
neither is for this run to fix.

THE TREE CHANGED, SO THE EARLIER GATE EVIDENCE IS STALE. Re-run all seven gates, with test and clippy
twice each. CLAUDE.md is one of the twelve conformance files, so reverting it invalidates that gate's
report in particular.

Do not edit any file outside the approved scope. Those four were written because a tool allowed it,
not because the work needed it.

Then the reviewer reviews the working tree against e5dfcec, and the run stops at Human checkpoint 2.
