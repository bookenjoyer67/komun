# ADR-005: Scope each subagent role by an enumerated tool grant

Which roles run the gate, and what does each role hold?

## Status

What is this decision's current status?

**Accepted.** The routing-and-tool-grant map is the decision of record (`docs/routing-and-tool-grant-map.md:7` `This map is the design decision of record for the gate.`). Its machine-readable twin carries the same grants for a check to read (`docs/routing-and-tool-grant-map.json:10` `"grants"`). The governance policy derives every grant and every denial from the map (`docs/governance-policy.md:11` `Derive every grant from the routing-and-tool-grant map`).

The map records no date and no author for the scoping decision. Not recorded at decision time. [UNVERIFIED] A version line or a date line added to the map would settle it.

## Context

Which roles exist, and what does each one hold?

The workflow runs seven roles, and the launcher names them in one list (`scripts/run-agent.sh:33` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher"`). Six carry the run, and the seventh is the stretch researcher (`docs/routing-and-tool-grant-map.md:21` `(stretch, optional)`).

The map records the scoping in two forms that must agree. The prose table gives the grants and the denial reasons, one row per role (`docs/routing-and-tool-grant-map.md:13` `| Role | Receives from Orchestrator | Produces | Tools granted |`). The JSON twin gives the grant list alone (`docs/routing-and-tool-grant-map.json:10` `"grants"`).

The grants, read from the JSON twin:

- The orchestrator holds no MCP tool at all (`docs/routing-and-tool-grant-map.json:11` `"orchestrator": []`). It sequences roles and holds orchestration documents through the harness tools alone (`docs/routing-and-tool-grant-map.md:15` `a harness capability rather than an MCP tool`).
- The planner holds six tools: two file tools, three storage operations, and retrieval at the `internal` ceiling (`docs/routing-and-tool-grant-map.json:12` `"planner": [`).
- The implementer holds nine tools, and it alone holds the write-mode gate (`docs/routing-and-tool-grant-map.json:29` `"mcp__gate__run_fix"`).
- The tester holds seven tools: one file tool, three storage operations, and the three gate-server tools (`docs/routing-and-tool-grant-map.json:36` `"mcp__gate__run_gate"`).
- The reviewer holds eight tools: two file tools, three storage operations, retrieval, and the two read-only gate tools (`docs/routing-and-tool-grant-map.json:47` `"mcp__gate__list_gates"`).
- The project-manager holds three tools: the ticket tool and two storage reads (`docs/routing-and-tool-grant-map.json:51` `"mcp__coursetools__task_tracker",`; `docs/routing-and-tool-grant-map.json:53` `"mcp__storage__list_entries"`).
- The researcher holds two tools: the workflow's only network tool and one storage write (`docs/routing-and-tool-grant-map.json:56` `"mcp__coursetools__web_search",`).

Five grants sit on one role alone. The check-mode gate is the tester's (`docs/routing-and-tool-grant-map.json:36` `"mcp__gate__run_gate"`). The write-mode gate is the implementer's (`docs/routing-and-tool-grant-map.json:29` `"mcp__gate__run_fix"`). The network tool is the researcher's (`docs/routing-and-tool-grant-map.md:69` `on the Researcher alone, which is the whole reason the stretch role exists`). The ticket tool is the project-manager's (`docs/routing-and-tool-grant-map.md:71` `ticket state has one owner`). The storage update is the implementer's (`docs/routing-and-tool-grant-map.md:72` `the only role that revises a record it wrote`).

The denials are enumerated rather than inferred. Every storage operation is denied to the orchestrator (`docs/routing-and-tool-grant-map.md:41` `Deny the Orchestrator every storage operation`). Storage deletion is granted to no role, and record removal stays outside the gate (`docs/routing-and-tool-grant-map.md:73` `keep record removal outside the gate`). The inert test runner is denied to every role, so the gate server keeps the only execution path (`docs/routing-and-tool-grant-map.md:70` `on the Tester alone, deny mcp__coursetools__test_runner to every role`). Retrieval reaches three roles, the planner, the implementer and the reviewer (`docs/routing-and-tool-grant-map.md:74` `which is safe because the operation is read-only, project-scoped and citation-bearing`). Every ceiling is pinned at `internal` (`docs/routing-and-tool-grant-map.md:75` `Cap every retrieval grant at`).

The launcher turns the grant map into a per-role mount profile. The profile function gives each role a workspace mode, a memory mode and a build-cache mode (`scripts/run-agent.sh:92` `role_profile() {`). Only the roles the map grants a storage write get a writable memory path (`scripts/run-agent.sh:90` `"mcp__storage__write_entry"`). The project-manager is the one role whose memory path is not mounted (`scripts/run-agent.sh:99` `ROLE_MEM=none`).

The denial is enforced twice over. The allow-list refuses a call that reaches the server, and the definition removes the tool before a call is possible (`docs/iteration-log.md:376` `the denial is enforced, and it is enforced twice over`). A grant edited out of a definition narrows the tool set without touching the server (`docs/iteration-log.md:378` `A grant edited out of a definition therefore narrows the tool set without touching the server`).

The researcher is the one role with the network, and it exists to keep open-web text out of every coding context (`docs/routing-and-tool-grant-map.md:88` `one dedicated role keeps that text out of every coding context`). Its definition states that isolation as the role's purpose (`.claude/agents/researcher.md:7` `It isolates the network tool`).

Four near-misses in the calibration log each produced one scoping rule, and each is cited to the iteration-log line that evidences it.

- NM-1, an inert grant: a role held a tool that satisfied no gate (`docs/calibration-log.md:24` `a role granted an inert tool reports blocked gates rather than failed ones`). Every gate in that run came back BLOCKED rather than FAILED, against the tester's inert stub (`docs/iteration-log.md:383-384` `the course's deliberately inert stub`).
- NM-2, a starved role: a planner denied its reads planned from a ten-line brief (`docs/calibration-log.md:30` `a planning role denied its reads plans from a ten-line brief`). The run recorded the plan as resting on the brief alone (`docs/iteration-log.md:412-413` `its plan rested entirely on the ten-line quotation`).
- NM-3, union permissions: confinement was inferred from a union of every role's tools instead of an enumerated denial list (`docs/calibration-log.md:36` `confinement rests on each definition's denied list`). The union let the orchestrator call a tool its own row denies (`docs/iteration-log.md:440` `allows the union of every role's tools`).
- NM-4, self-approved provenance: a checkpoint approval was written by the role whose verdict it approved (`docs/calibration-log.md:42` `a checkpoint record written by the role it judges borrows the appearance of an independent approval`). The entry was the reviewer's own checkpoint approval (`docs/iteration-log.md:453-454` `the same role whose verdict it approves`).

## Decision

Which scoping rule does each role hold, and which artifact owns it?

Scope every role by an enumerated grant list in two artifacts that must agree. The map is the design decision of record (`docs/routing-and-tool-grant-map.md:7` `This map is the design decision of record for the gate.`). The map also fixes the tie-break when a definition drifts from it (`docs/routing-and-tool-grant-map.md:7` `Update an agent definition to match this map whenever the two disagree.`).

Four rules follow from the four near-misses:

- Grant the command runner to the tester alone, and deny the inert stub to every role (`docs/routing-and-tool-grant-map.md:70` `keep the gate server the one path that executes a command`). This answers NM-1.
- Grant the reads a role's work needs, so no role plans from its brief alone (`docs/calibration-log.md:85` `Grant reads to the roles whose work needs them`). This answers NM-2.
- Enumerate the denied MCP tools in every role definition, so confinement is stated positively rather than inferred from the union (`docs/calibration-log.md:86` `Enumerate denials in every definition`). This answers NM-3.
- Give every checkpoint record one author, and route it to the project-manager (`docs/iteration-log.md:464` `assign checkpoint records to the project-manager`). This answers NM-4.

The reservation of execution is the load-bearing clause. The gate server is the one path that runs a command (`docs/routing-and-tool-grant-map.md:70` `keep the gate server the one path that executes a command`). A role holds authorisation per tool rather than per gate name (`docs/routing-and-tool-grant-map.md:70` `Authorisation is held per tool, not per gate name`). The server authorises the bound `AGENT_ROLE`, or the declared role when unbound, against the per-tool grant before the membership check (`mcp/gate/server.py:719` `role = _authorize(calling_role, "run_gate", gate=gate)`).

Each definition states its denials positively. The orchestrator's definition now lists every denied MCP tool (`docs/iteration-log.md:445` `enumerate the denied MCP tools in orchestrator.md`). Its frontmatter carries the denial list the map requires (`.claude/agents/orchestrator.md:11` `disallowedTools: Bash, mcp__coursetools__file_read`).

The gate server refuses a command string as well as a non-allowlisted name, so a call cannot smuggle arguments (`mcp/gate/server.py:125` `it accepts no command string, no extra arguments`). The check surface and the write surface stay on two different roles. The role that writes a fix never runs the gate that judges it (`docs/routing-and-tool-grant-map.md:76` `the role that repairs a file never grades the repair`).

Two recorded conflicts were resolved by fixing the definition to match the map. The orchestrator's file-tool conflict resolved to an empty grant list (`docs/governance-policy.md:41` `"orchestrator": []`). The reviewer's gate-tool conflict kept the check-mode gate denied (`docs/governance-policy.md:222` `keeps mcp__gate__run_gate denied`).

The policy starts every role at no access and adds each grant explicitly (`docs/governance-policy.md:23` `Start every role at no access`). Every denial carries a stated reason (`docs/governance-policy.md:24` `give every denial a one-line reason`).

The seven roles form one chain. The orchestrator briefs the planner, the planner's plan is approved, and the implementer writes it (`docs/routing-and-tool-grant-map.md:13` `| Role | Receives from Orchestrator | Produces | Tools granted |`).

## Alternatives considered

Which grant designs were weighed, and where was each reason recorded?

Recorded at decision time in the map's own alternatives section (`docs/routing-and-tool-grant-map.md:78` `## Alternatives considered`). The section poses its own question (`docs/routing-and-tool-grant-map.md:80` `Which grant designs were weighed and ruled out?`). Each entry carries the reason the map recorded beside it.

- Granting `mcp__storage__delete_entry` to the implementer for scratch cleanup. Ruled out, with the reason recorded (`docs/routing-and-tool-grant-map.md:82` `cleanup does not justify handing a coding role a destructive capability`).
- Granting `mcp__retrieval__retrieve` to the tester. Ruled out, with the reason recorded (`docs/routing-and-tool-grant-map.md:84` `an independent corpus search adds a capability the role does not need`).
- Letting the orchestrator hold `mcp__gate__run_gate` and run the gates itself. Ruled out, with the reason recorded (`docs/routing-and-tool-grant-map.md:86` `evaluates its own execution`).
- Granting `mcp__coursetools__web_search` to every role. Ruled out, with the reason recorded (`docs/routing-and-tool-grant-map.md:88` `the open web returns unbounded text`).
- Raising the reviewer ceiling to `confidential`. Ruled out, with the reason recorded (`docs/routing-and-tool-grant-map.md:90` `no review task in this gate reads confidential data`).

The recorded reasons are the map's own words. This ADR adds no rationale the map does not carry.

## Consequences

What does the scoping change, and what does it leave open?

- The map and the JSON twin carry the same seven grant lists (`docs/routing-and-tool-grant-map.json:10` `"grants"`). The launcher derives its memory mount from the same grant rather than a second list (`scripts/run-agent.sh:90` `"mcp__storage__write_entry"`).
- The mount is the enforcement, not a permission bit, because the container runs as root (`scripts/run-agent.sh:9` `The mount is the enforcement, not a permission bit`).
- NM-2's fix added the planner's reads (`docs/governance-policy.md:77` `where a planner that could not read its inputs planned from the brief alone`).
- NM-3's fix enumerated the denied MCP tools in each definition (`docs/calibration-log.md:86` `Enumerate denials in every definition`).
- Two fixes carry no rerun evidence yet, so their effect is unverified. NM-3's enumeration was recorded as a hypothesis (`docs/iteration-log.md:448` `Rerun evidence: none yet.`), and NM-4's checkpoint reassignment likewise (`docs/iteration-log.md:469` `Rerun evidence: none yet;`). [UNVERIFIED]
- The chain's two checkpoints stay with the human. The planner's plan needs a human approval, and the reviewer's verdict feeds the release decision (`docs/routing-and-tool-grant-map.md:15` `a human owns both checkpoints`).
- The map leaves one thing open: it carries no version line and no date line, so the scoping decision has no recorded date.

## Evidence

Which artifacts settle this decision?

- Provenance: this ADR transcribes a decision already recorded in the map and the policy, and it makes no decision of its own.
- Decision of record, prose: `docs/routing-and-tool-grant-map.md`.
- Decision of record, machine-readable: `docs/routing-and-tool-grant-map.json`.
- Policy that derives every grant and denial: `docs/governance-policy.md`.
- The five grant alternatives, recorded at decision time: `docs/routing-and-tool-grant-map.md` (`docs/routing-and-tool-grant-map.md:78` `## Alternatives considered`).
- The seven role definitions: `.claude/agents/orchestrator.md`, `.claude/agents/planner.md`, `.claude/agents/implementer.md`, `.claude/agents/tester.md`, `.claude/agents/reviewer.md`, `.claude/agents/project-manager.md` and `.claude/agents/researcher.md`.
- The near-misses NM-1 to NM-4: `docs/calibration-log.md`.
- The iteration-log evidence behind each near-miss: `docs/iteration-log.md`.
- Enforcement, the per-role container mounts: `scripts/run-agent.sh` (`scripts/run-agent.sh:92` `role_profile() {`).
- The launcher's per-role mount matrix: `scripts/run-agent.sh` (`scripts/run-agent.sh:80` `| Role | Workspace mount | Memory mount | Network | Reason |`).
- The storage grant table and its denial reasons: `docs/routing-and-tool-grant-map.md` (`docs/routing-and-tool-grant-map.md:39` `Denial reasons:`).
- The gate server's single execution path: `mcp/gate/server.py` (`mcp/gate/server.py:122` `if gate not in GATES:`).
- The gate server's refusal of a command string: `mcp/gate/server.py` (`mcp/gate/server.py:125` `it accepts no command string, no extra arguments`).
- The two recorded definition conflicts and their fixes: `docs/governance-policy.md` (`docs/governance-policy.md:41` `"orchestrator": []`).
- The least-privilege default the policy states: `docs/governance-policy.md` (`docs/governance-policy.md:23` `Start every role at no access`).
- The two enforcement layers behind one denial: `docs/iteration-log.md` (`docs/iteration-log.md:376` `the denial is enforced, and it is enforced twice over`).
- Each role's granted and denied tool list in its definition: `.claude/agents/orchestrator.md` (`docs/governance-policy.md:35` `tools: Task, Read, Write, Edit`).
- The role count and its naming: `scripts/run-agent.sh` (`scripts/run-agent.sh:33` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher"`).

## Open risks

- Two fixes carry no rerun evidence yet, so their effect is unverified. NM-3's enumeration was recorded as a hypothesis (`docs/iteration-log.md:448`).
- The map and the JSON twin must stay equal; nothing generates one from the other, so they can drift until a comparison catches it.
- The scoping is measured against the roles that exist. A new role inherits the default rather than a reasoned list until one is written.
