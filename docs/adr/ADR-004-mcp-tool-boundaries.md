# ADR-004: Bound every MCP server by its own allow-list, and keep the gate name-only

Which tool boundaries separate the four MCP servers, and what contract does each one enforce?

## Status

What is this decision's current status?

**Accepted.** The orchestration runs four MCP servers (`docs/routing-and-tool-grant-map.json:4` `"servers": [`), named coursetools, storage, retrieval and gate. Each server enforces its own allow-list (`mcp/storage/server.py:13` `is the first statement of every one of the five`). The gate server executes no caller-supplied string (`mcp/gate/server.py:7` `A caller names a command, never a command line`). The grant map is the design decision of record for the gate (`docs/routing-and-tool-grant-map.md:7` `This map is the design decision of record for the gate.`).

The gate's own selftest asserts that surface and those refusals (`mcp/gate/selftest.py:63` `EXPECTED_TOOLS = {"list_gates", "run_gate", "run_fix", "read_audit_log"}`; `mcp/gate/selftest.py:5` `refusal of a free-form command,`).

## Context

Which tool surfaces does the role set need, and which artifact records each grant?

The gate server exists because the first orchestrated run held no command runner at all (`docs/iteration-log.md:396` `a fourth MCP server`). The tester had been granted the course's inert stub (`docs/iteration-log.md:383` `the course's deliberately`; `docs/iteration-log.md:384` `inert stub`), so every gate came back blocked rather than failed.

Coursetools is a safe stand-in rather than a live provider (`mcp/coursetools_server.py:3` `This server is a safe stand-in for real course tools.`). Its one real behaviour is the role allow-list check (`mcp/coursetools_server.py:74` `if role not in allowed_roles:`).

The routing map holds every grant per role (`docs/routing-and-tool-grant-map.md:7` `This map is the design decision of record for the gate.`), and its operative form is machine-readable (`docs/routing-and-tool-grant-map.json:10` `"grants": {`). The policy derives every grant from that map (`docs/governance-policy.md:11` `Derive every grant from the routing-and-tool-grant map`).

The policy draws each denial from a recorded near miss (`docs/governance-policy.md:13` `Derive every denial from the near-miss patterns observed in calibration`). Record every grant beside its stating artifact (`docs/governance-policy.md:24` `Record every grant beside the artifact that states it, and give every denial a one-line reason.`).

Name the inert grant as near miss NM-1 (`docs/calibration-log.md:24` `a role granted an inert tool reports blocked gates rather than failed ones`). Draw those near-miss patterns from the log's own top five entries (`docs/calibration-log.md:11` `Draw the patterns below from the log's top five entries`).

Every role starts at no access (`docs/governance-policy.md:23` `Start every role at no access: no MCP operation, no skill, no retrieval ceiling, no autonomy.`). The map grants the gate path to the tester alone (`docs/iteration-log.md:398` `was granted to the tester alone.`), and keeps the gate the only executor (`docs/routing-and-tool-grant-map.md:18` `the gate server is the only execution path`).

## Decision

What contract does each server enforce at its boundary?

- Read each role's permit set from a file beside the server (`mcp/storage/allow-list.json:14` `Derived from docs/routing-and-tool-grant-map.json`).
- Check that permit set as the first statement of every operation (`mcp/storage/server.py:13` `is the first statement of every one of the five`; `mcp/retrieval/server.py:762` `The guard is the first statement of the operation`).
- Refuse an unknown, blank or ungranted role, and default nothing (`mcp/retrieval/allow-list.json:9` `nothing is defaulted to an allowed role.`).
- Treat an empty permit set as a known role holding nothing (`mcp/storage/allow-list.json:13` `An empty list means the role is known and holds nothing.`).
- Name the role, the operation and the permitted roles in each refusal (`mcp/storage/server.py:16` `error that names the role, the operation and the roles that ARE`).
- Mount both allow-lists and the routing map read-only into the sandbox (`agentic.config.json:127` `"mcp/storage/allow-list.json",`; `agentic.config.json:128` `"mcp/retrieval/allow-list.json",`).
- Take a gate name, and never a command line (`mcp/gate/server.py:7` `A caller names a command, never a command line`).
- Resolve that name against an allow-list before anything runs (`mcp/gate/server.py:120` `if gate not in GATES:`; `mcp/gate/SCHEMA.md:120` `The refusal is raised before anything is executed`).
- Read every argv from the config table alone (`mcp/gate/server.py:69` `The argv tuples are the only commands this process can ever run`).
- Keep the shell off and append nothing caller-supplied (`mcp/gate/server.py:400` `a fixed argv, never a caller-supplied string`; `mcp/gate/server.py:402` `cwd=WORKSPACE,`).
- Refuse a command string, an extra argument and a shell with one message (`mcp/gate/server.py:123` `it accepts no command string, no extra arguments and no`).
- Split the vocabulary by declared mode, and give each half one tool (`mcp/gate/server.py:146` `if command not in FIX_COMMANDS:`).
- Expose four operations and no shell on the gate (`mcp/gate/SCHEMA.md:9` `It exposes four named operations and no shell, no argv, and no working-directory control:`).
- Journal every executed call, and journal nothing for a refusal (`mcp/gate/server.py:12` `refused call runs nothing and journals nothing`).
- Record the tool and the outcome on every gate journal row (`mcp/gate/server.py:10` `Each invocation returns the exit code, the captured stdout and stderr`).
- Stamp each gate journal row with the caller (`mcp/gate/server.py:222` `"calling_role": calling_role or "unknown",`).
- Keep each journal append-only (`mcp/storage/server.py:131` `The file is opened append-only.`; `mcp/retrieval/server.py:25` `A denial and a withholding land in the same log as a success.`).
- Write three journals, one per server that journals (`agentic.config.json:133` `".memory/storage-audit.log",`; `agentic.config.json:134` `".memory/retrieval-audit.log",`; `agentic.config.json:135` `".memory/gate-audit.log"`).
- Refuse a storage write above the internal ceiling before the journal row (`mcp/storage/server.py:121` `if classification not in WRITE_CLASSIFICATIONS:`; `mcp/storage/server.py:124` `entries and never journals a refusal;`).
- Refuse a write above `internal` at the storage layer (`mcp/storage/server.py:307` `Classifications at or above confidential are refused.`).
- Pin a retrieval ceiling per role, and take the stricter of the role's cap and the request (`mcp/retrieval/server.py:807` `withheld = effective_ceiling != classification_ceiling`; `docs/routing-and-tool-grant-map.json:62` `"planner": "internal",`).
- Refuse a retrieval call for a role with no ceiling (`mcp/retrieval/server.py:494` `elif ceiling == CEILING_NONE:`; `mcp/retrieval/server.py:88` `which means the role holds no retrieval grant at all.`).
- Journal each retrieval decision as allowed or withheld (`mcp/retrieval/server.py:831` `decision="withheld_ceiling" if withheld else "allowed",`).
- Keep the ceiling in one file rather than two (`mcp/retrieval/allow-list.json:11` `The classification ceiling is NOT duplicated in this file.`).
- Refuse to start when the allow-list and the routing map disagree (`mcp/retrieval/server.py:429` `if granted and ceiling == CEILING_NONE:`).
- Deny shell to every role on the coursetools server (`mcp/roles.allowlist.json:5` `"shell": [],`).
- Grant `delete_entry` to no role (`mcp/storage/allow-list.json:29` `refused to every role, so record removal stays outside the gate and audit evidence survives`).

## Alternatives considered

Which alternatives were rejected, and which artifact records each rejection?

The map records five grant designs in its own `Alternatives considered` section (`docs/routing-and-tool-grant-map.md:78` `## Alternatives considered`). Record those rulings in the map itself, not in a separate decision log (`docs/routing-and-tool-grant-map.md:78` `## Alternatives considered`).

- Reject `delete_entry` to the Implementer, because cleanup does not justify a destructive capability (`docs/routing-and-tool-grant-map.md:82` `Ruled out, because cleanup does not justify handing a coding role a destructive capability.`).
- Reject retrieval to the Tester, because the brief already carries the criteria (`docs/routing-and-tool-grant-map.md:84` `an independent corpus search adds a capability the role does not need.`).
- Reject `run_gate` to the Orchestrator, because a self-graded run is no check (`docs/routing-and-tool-grant-map.md:86` `evaluates its own execution, which removes the independent check the Tester provides.`).
- Reject web search to every role, because one role holds the open-web text (`docs/routing-and-tool-grant-map.md:88` `the open web returns unbounded text, and one dedicated role keeps that text out of every coding context.`).
- Reject a higher reviewer ceiling, because no review task reads confidential data (`docs/routing-and-tool-grant-map.md:90` `no review task in this gate reads confidential data, and the ceiling stays at`).

Weigh a dedicated cleanup role only under explicit review (`docs/routing-and-tool-grant-map.md:82` `Introduce a dedicated maintenance role under explicit review if cleanup ever becomes necessary.`). Bound every ceiling by the memory classification levels (`docs/governance-policy.md:17` `Bound every ceiling by the memory classification levels`).

[UNVERIFIED] One server versus four: Not recorded at decision time. No artifact here weighs one MCP server against four (`docs/routing-and-tool-grant-map.json:4` `"servers": [`). A record that weighs the two designs would settle it.

[UNVERIFIED] A command-string gate: Not recorded at decision time. The name-only rule is recorded as a design (`docs/iteration-log.md:397` `with no command string, no argument passthrough and no shell`). Its refusal is measured (`docs/iteration-log.md:404` `it accepts no command string, no extra arguments and`). No artifact weighs a command-string gate as the rejected alternative.

Provenance: the five grant alternatives stand as written in `docs/routing-and-tool-grant-map.md`; the two server-design alternatives carry `[UNVERIFIED]` because no artifact in this repository records them as of 2026-10-01.

## Consequences

What does this boundary change, and what does it leave open?

- Make the only command-executing path non-arbitrary (`docs/routing-and-tool-grant-map.md:70` `keep the gate server the one path that executes a command.`).
- Refuse a command string, measured (`docs/iteration-log.md:404` `it accepts no command string, no extra arguments and`).
- Refuse an argument passthrough identically (`docs/iteration-log.md:405` `is refused identically`).
- Refuse a shell injection and leave no file (`docs/iteration-log.md:406` `is refused and left no file`).
- Refuse the same attempt with the same message in both directions (`mcp/gate/SCHEMA.md:128` `An argument-passthrough attempt and a shell-injection attempt were refused with the same message:`).
- Leave the injection target absent afterwards (`mcp/gate/SCHEMA.md:130` `target did not exist afterwards`).
- Add no journal line for a refusal (`mcp/gate/server.py:12` `refused call runs nothing and journals nothing`).
- Add no journal line and create no file for a mode refusal (`mcp/gate/SCHEMA.md:524` `The mode refusals added no journal line and created no file`).
- Count the journal's line count as an audit fact (`mcp/gate/SCHEMA.md:440` `Measured: three refusals in one self-test run left the journal at 10 lines, unchanged`).
- Leave a refused secret write with no new audit row (`docs/iteration-log.md:501` `is refused by classification enforcement with **no** new audit`).
- Keep record removal outside the gate (`mcp/storage/allow-list.json:29` `no role holds a destructive capability here`).
- Widen the surface by tool rather than by gate name (`docs/routing-and-tool-grant-map.md:70` `Authorisation is held per tool, not per gate name.`).
- Extend every holder's surface when a name is added to the config (`docs/routing-and-tool-grant-map.md:70` `adding a name to the config extends the surface of every holder of that tool.`).
- Validate the caller no further than a recorded stamp (`mcp/gate/server.py:222` `"calling_role": calling_role or "unknown",`).
- Keep `fmt-fix` unexercised, so its checks are refusals alone (`mcp/gate/selftest.py:13` `through the refusals that prove it is unreachable from the check surface.`).
- Keep the retrieval corpus read-only and bounded (`docs/routing-and-tool-grant-map.md:74` `the operation is read-only, project-scoped and citation-bearing.`).
- Carry four allow-lists, so the files must stay in step (`mcp/retrieval/allow-list.json:10` `Derived from docs/routing-and-tool-grant-map.json`).
- Keep the two server-design alternatives open, above.

## Evidence

Which artifacts settle this decision?

- Server set and its members: (`docs/routing-and-tool-grant-map.json:4` `"servers": [`; `docs/routing-and-tool-grant-map.json:8` `"gate"`).
- Gate name-only rule: (`mcp/gate/server.py:7` `A caller names a command, never a command line`).
- Measured refusals: (`docs/iteration-log.md:404` `it accepts no command string, no extra arguments and`; `docs/iteration-log.md:405` `is refused identically`; `docs/iteration-log.md:406` `is refused and left no file`).
- Gate selftest surface: (`mcp/gate/selftest.py:63` `EXPECTED_TOOLS = {"list_gates", "run_gate", "run_fix", "read_audit_log"}`; `mcp/gate/selftest.py:5` `refusal of a free-form command,`).
- Per-role allow-lists: (`mcp/storage/allow-list.json:14` `Derived from docs/routing-and-tool-grant-map.json`; `mcp/retrieval/allow-list.json:10` `Derived from docs/routing-and-tool-grant-map.json`).
- Coursetools deny-list for shell: (`mcp/roles.allowlist.json:5` `"shell": [],`).
- Coursetools denial path: (`mcp/coursetools_server.py:76` `Authorization error: role`).
- Storage classification ceiling: (`mcp/storage/server.py:121` `if classification not in WRITE_CLASSIFICATIONS:`; `docs/iteration-log.md:501` `is refused by classification enforcement with **no** new audit`).
- Retrieval ceiling source and use: (`docs/routing-and-tool-grant-map.json:60` `"retrieval_ceiling": {`; `mcp/retrieval/server.py:807` `withheld = effective_ceiling != classification_ceiling`).
- Retrieval journal records a withholding: (`mcp/retrieval/server.py:23` `A withholding is journalled.`).
- Gate journal opened append-only: (`mcp/gate/server.py:173` `The file is opened append-only.`).
- Journals: (`agentic.config.json:133` `".memory/storage-audit.log",`; `agentic.config.json:134` `".memory/retrieval-audit.log",`; `agentic.config.json:135` `".memory/gate-audit.log"`).
- Read-only inputs: (`agentic.config.json:129` `"docs/routing-and-tool-grant-map.json",`).
- Classification levels that bound the ceilings: (`docs/memory-architecture.md:149` `Do not store in agent memory.`; `docs/memory-architecture.md:151` `Must never appear in any memory file.`).
- Grant alternatives of record: (`docs/routing-and-tool-grant-map.md:78` `## Alternatives considered`).
