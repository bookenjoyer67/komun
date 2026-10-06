# ADR-007: Add a `beta-tester` role and the browser MCP server it drives

Which role exercises the running application, and what does the harness add for it?

## Status

What is this decision's current status?

**Accepted.** The human answered the placement question with the literal `2`, which selects a Komun harness role. The harness gains the `beta-tester` role and the `browser` MCP server. The change awaits one operator commit; this session commits and pushes nothing.

## Context

What gap does the pre-merge gate leave, and what does the harness hold today?

The gate verifies a change by reading the repository and running named checks. The planner reads the repository and orders the work (`.claude/agents/planner.md:23` `autonomy: medium`). The tester runs the fixed gates by name and reports (`docs/governance-policy.md:203` `decides when a gate has passed against the baseline`). The reviewer weighs the change against the repository's standards (`.claude/agents/reviewer.md:48` `v2 is the current rule set`). No role drives the application the change ships.

The harness ran a fixed MCP server set, and none of them drove a browser (`docs/routing-and-tool-grant-map.json:4` `"servers": [`). Every role starts at no access (`docs/governance-policy.md:23` `Start every role at no access`), so a browser capability has to arrive as a new enumerated grant. Record every grant beside the artifact that states it, and give every denial a reason (`docs/governance-policy.md:24` `Record every grant beside the artifact that states it`).

The network stays isolated to one role. The researcher holds the workflow's only network lookup (`docs/governance-policy.md:308` `The workflow's only network lookup`), so the new check cannot ride on an existing role's tool.

## Decision

What does the harness add, and what does the new role hold?

Add a governed role named `beta-tester` and an MCP server named `browser`, whose tools are written `mcp__browser__<tool>`. The role uses the running app instead of reading the repository.

- Expose exactly eight browser tools on the `browser` server at port 8004 (`docs/AGENT-HARNESS.md:131` `holding the eight` `mcp__browser__*` `tools`).
- Grant the role all eight `mcp__browser__*` tools, `mcp__coursetools__file_read`, and the three storage reads plus one write (`docs/routing-and-tool-grant-map.md:7` `This map is the design decision of record for the gate.`).
- Deny `codebase_search`, `shell`, the gate tools and every other storage operation, so the role reports findings and repairs nothing.
- Write one `test-result` entry at `public` or `internal`, and edit no repository file.
- Hold no retrieval ceiling and `low` autonomy, because its evidence is the app's behaviour, not the corpus.
- Mount workspace `ro`, memory `rw` and build_cache `ro`, so it writes its entry and its screenshots only.
- Start the server only when a beta target is named (`scripts/start-mcp-servers.sh:94` `if [ -n "${BETA_BASE_URL:-}" ]; then`), so no other role's start changes.
- Journal every call to `.memory/browser-audit.log`, and join the hash chain like the other three journals.
- Refuse a non-base origin with a refusal row that names the role and the reason.

## Alternatives considered

Which placements were weighed, and why was each rejected?

The room question offered three placements, and the human chose the second. Only that option is quoted verbatim from the room.

- **A Hermes-side, dogfood-only tester.** Rejected by the answer `2`. It would drive a browser from the agent host and hold no governed role, so its run leaves no role-bound journal row and no recorded result.
- **Both placements.** Rejected by the answer `2`. It doubles the browser surface, and the ungoverned half adds no evidence the governed role does not already produce.
- **A Komun harness role.** Chosen, and quoted verbatim in the Authority section below.
- **Put the browser tools on the existing `tester` role.** Rejected. The tester verifies recorded gate evidence and reads no open-web text (`docs/governance-policy.md:178` `the tester reads no open-web text`), so a browser pass would widen the verifying role rather than add a separate one.
- **Reuse the gate's name-only surface with a browser script.** Rejected. The gate binds each name to one argv and takes no caller argument (`mcp/gate/server.py:125` `it accepts no command string, no extra arguments`), so a browsing step that needs a URL does not fit that surface.

## Consequences

What does the addition change, and what does it leave open?

- The gate gains an optional pre-release step that no gate's verdict depends on (`docs/orchestration-diagram.md:78` `Treat the beta-tester pass as an optional pre-release step rather than a gate`).
- The step is recorded as agentic, on request, rather than as a conversion (`docs/step-classification.md:182` `Step: beta-test pass against the running app`).
- The role introduces no conflict between the map and the definitions (`docs/policy-reconciliation.md:44` `How does the new`).
- The change is recorded as the run's change under gate (`docs/iteration-log.md:1446` `- Change under gate: ticket`), the entry appended rather than prepended so existing citations do not drift.
- The browser server joins the hash chain, so its journal is anchored like the other three.
- The `CLAUDE.md` role list still reads seven roles. The human authorized the edit, but this session's
  guardrail refused the write when its own approval prompt went unanswered, so the line stays pending and a
  later session may land it with that prompt answered
  (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md:113` `One line stays pending`).
- The operator committed the change on `harness/beta-tester-role` and pushed it, and the release decision
  stays with the human.

## Authority

Who authorized this, and what did they say?

The human was asked, in the Komun room, which of three placements the beta tester should take, and answered, verbatim, `2`. Option `2` reads, verbatim:

> **2. A Komun harness role** — `.claude/agents/beta-tester.md` plus a routing-map row, but it needs a browser MCP built for the harness first (the `rt-browser:komun` image could host one)

The answer authorizes one new governed role and the browser MCP server it needs. It authorizes no change to another role's grants, no touch to the app under test, no commit to `main` and no push; the operator commits and pushes.

## Evidence

Which artifacts settle this decision?

- The role definition and its grants: `.claude/agents/beta-tester.md`, and the map that is the design of record (`docs/routing-and-tool-grant-map.md:7` `This map is the design decision of record for the gate.`).
- The browser server and its allow-list: `mcp/browser/server.py` and `mcp/browser/allow-list.json`.
- The optional start: `scripts/start-mcp-servers.sh:94` `if [ -n "${BETA_BASE_URL:-}" ]; then`.
- The harness mention: `docs/AGENT-HARNESS.md:125` `Is there a role that tests the running application?`.
- The diagram branch: `docs/orchestration-diagram.md:64` `In the optional pre-release flow`.
- The classification row: `docs/step-classification.md:42` `Beta-test pass against the running app`.
- The run entry: `docs/iteration-log.md:1442` `run-2026-10-05-beta-tester-role`.
