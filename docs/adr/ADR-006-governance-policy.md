# ADR-006: Enforce the governance policy in configuration rather than in prose

Where does the governance policy bind, and what proves each bound rule still holds?

## Status

What is this decision's current status?

**Accepted.** The policy is published as `docs/governance-policy.md` v1.0.0, and each rule it states is carried by an artifact a program reads (`docs/governance-policy.md:4` `Last updated: 2026-09-28`). No human has signed the draft (`docs/governance-policy.md:5` `Reviewed by: pending`), so the Module 4.1 checkpoint owner holds approval.

## Context

Which rules needed enforcement, and what does the record show?

The policy is one section per role, seven in all (`docs/governance-policy.md:27` `## Role 1`). Each entry carries MCP grants and denials, skill scope, a classification ceiling, an autonomy level and a container-mount line (`eval/README.md:10` `MCP grants and denials, skill scope, ceiling, autonomy, container mounts`).

Those values are written more than once, once as prose and once in the artifact that binds the runtime. The routing map holds the machine-readable grant list (`docs/routing-and-tool-grant-map.json:10` `"grants"`). Each server holds a per-role allow-list derived from that map (`mcp/retrieval/allow-list.json:14` `source_of_truth`). The classification ceiling is pinned per role in the map rather than in the allow-list (`docs/routing-and-tool-grant-map.json:62` `"planner": "internal",`).

The policy is not trusted to be right on its own. The suite compares the seven policy entries with the artifacts that enforce them in both directions (`eval/README.md:5` `Compare the seven policy entries with the artifacts that enforce them in both directions`). Every check compares a value, never a file's presence (`eval/test_policy.py:9` `Every check compares values, never file presence`).

The record shows why this discipline was needed. The settings file allows the union of every role's tools, so confinement rested on each definition's denied list (`docs/iteration-log.md:440` `allows the union of every role's tools`). That failure is NM-3 (`docs/calibration-log.md:32` `### NM-3`). Ten patterns follow, each named and each cited to the line that evidences it (`docs/calibration-log.md:18` `Ten patterns follow, each named and each cited to the line that evidences it`).

Two near-misses bound the enforcement at the container layer. NM-5 put a credential inside memory reach (`docs/calibration-log.md:48` `a write path open to any classification can persist a credential into files meant to be committed`). NM-9 showed permission bits a root process ignores (`docs/calibration-log.md:72` `a filesystem permission is not a guardrail when the writing process runs as root`).

The policy and the launcher were reconciled rather than assumed consistent (`docs/policy-reconciliation.md:7` `Four conflicts stood between the map and the seven role definitions`). One conflict was a shared mount sentence that four roles could not satisfy (`docs/policy-reconciliation.md:15` `Four roles the policy grants a memory entry write had no writable path`). The launcher now mounts the memory path read-write for the five roles holding the entry write (`scripts/run-agent.sh:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`).

## Decision

Where does enforcement live, and which artifacts bind it?

Enforcement lives in configuration and in named artifacts. Prose holds no rule a program cannot check.

- Bind each rule to an artifact a program reads, and name every artifact in one place (`eval/README.md:6` `Read the policy, the routing map, both allow-lists, the seven definitions and the launcher`).
- Test the binding in both directions, so an artifact grant the policy never made fails as loudly as a policy grant the artifact omits (`eval/test_policy.py:670` `def test_storage_allowlist_matches_policy`).
- Assert equality rather than presence, so the policy and the storage allow-list cannot drift apart (`eval/test_policy.py:678` `assert allow_ops == policy_storage`).
- Check the launcher's own mount matrix against its case block and against the policy line it cites (`eval/test_policy.py:1040` `def test_launcher_matrix_matches_its_case_block`).
- Require every near-miss the policy cites to exist in the calibration log (`eval/test_policy.py:1132` `def test_near_miss_citations_exist`).
- Keep one mount profile per role in one case block, so the launcher is the mount authority (`scripts/run-agent.sh:95` `ROLE_WS=ro; ROLE_MEM=rw`).
- Mount the workspace read-only for the five roles that write no repository file (`scripts/run-agent.sh:165` `MOUNTS+=(-v "$REPO:/workspace:ro")`).
- Grant deletion to no role, and keep record removal outside the gate (`docs/routing-and-tool-grant-map.md:73` `Grant mcp__storage__delete_entry to no role`).
- Keep the only network lookup on one role, so open web text never enters a coding context (`docs/routing-and-tool-grant-map.md:69` `Hold mcp__coursetools__web_search on the Researcher alone`).
- Cap every retrieval grant at internal, because nothing above internal may be stored (`docs/routing-and-tool-grant-map.md:75` `Cap every retrieval grant at internal`).
- Refuse a missing, blank or unknown role rather than defaulting it to a grant (`mcp/retrieval/allow-list.json:9` `nothing is defaulted to an allowed role`).
- Keep the check surface on the tester, so the role that repairs never grades the repair (`docs/routing-and-tool-grant-map.md:76` `keep the check surface on the Tester`).
- Name the boundary each prompt targets before the run, so a block is judged against a stated layer (`eval/red-team-prompts.md:12` `the read-only /workspace bind for the reviewer`).
- Set autonomy in the definition and read it back from the policy (`docs/governance-policy.md:63` `Hold high autonomy`).
- Run the suite to a recorded count (`eval/README.md:18` `75 passed`).
- Prove the suite bites by injecting drift into a live artifact, one defect per run (`eval/README.md:27` `Withdraw a policy storage grant, drift one`).
- Prove the boundaries with an adversarial run, not with an assertion (`eval/red-team-prompts.md:7` `Ten prompts attack the six Module 4.1 boundaries.`).
- Fix the layer that failed and re-run the same prompt (`eval/red-team-results.md:7` `Eight prompts were blocked on their first run.`).
- Gate a governed change with five CI jobs named for the lesson's rows (`/.github/workflows/ci.yml:4` `change-type-check, policy-gate, eval-gate, advisory-review`).
- Keep the deterministic gates binding and the agentic ones advisory (`/.github/workflows/ci.yml:17` `policy-gate carries no`).
- Mark the four advisory or report jobs continue-on-error, so a review cannot fail a merge (`/.github/workflows/ci.yml:486` `continue-on-error: true`).
- Run the policy suite in the sandbox with the workspace read-only (`/.github/workflows/ci.yml:137` `python -m pytest eval/test_policy.py -v`).
- Run the eval gate only on a governed change (`/.github/workflows/ci.yml:160` `if: needs.change-type-check.outputs.requires-governed-check == 'true'`).
- Decide which changes are governed from two glob tables in one config file (`scripts/classify-change.py:129` `classification.governed_globs`).
- Treat a policy file as agent-affecting whatever the globs say (`scripts/classify-change.py:180` `governed = governed or policy`).
- Keep the policy document inside the governed set, so a policy edit triggers the gate (`agentic.config.json:219` `"docs/governance-policy.md"`).

**Provenance:** every rejection in the next section is recorded in this repository at the cited line, except the cases where this record writes exactly "Not recorded at decision time."

## Alternatives considered

Which alternatives were rejected, and why?

- **Rely on the role definitions' denied lists alone.** Rejected, and the reason is recorded at the near-miss that settled it (`docs/iteration-log.md:440` `allows the union of every role's tools`).
- **Publish the policy with no executable enforcement.** Rejected. Not recorded at decision time. [UNVERIFIED] A design note in the policy's revision history or the Module 4.1 brief would settle it.
- **Mark every pipeline job gating.** Rejected, and the reason is recorded in the workflow itself (`/.github/workflows/ci.yml:12` `gate immediately because they produce the same result for the same input`).
- **Adopt the lesson's repository paths verbatim.** Rejected, and the reason is recorded in the policy's own basis (`docs/governance-policy.md:15` `Use this repo's paths rather than the lesson's`).
- **Keep the map and the role definitions as separate authorities.** Rejected, and the reconciliation pass records why (`docs/policy-reconciliation.md:7` `Four conflicts stood between the map and the seven role definitions`).
- **Settle a conflict by editing both sides.** Rejected, and the reason is recorded in the suite's fix note (`eval/README.md:23` `Fix the side that is wrong, never both`).
- **Check only that each artifact exists.** Rejected, and the reason is recorded in the suite's own docstring (`eval/test_policy.py:9` `Every check compares values, never file presence`).

## Consequences

What does this decision change, and what does it leave open?

- A policy change cannot land without the artifacts changing with it, because each comparison asserts equality (`eval/test_policy.py:678` `assert allow_ops == policy_storage`).
- Every denial now traces to an observed event rather than to a preference (`docs/calibration-log.md:84` `Bind gate execution to the tester alone`).
- The controls are enumerated one per near-miss, so each denial has a written owner (`docs/calibration-log.md:87` `Give every checkpoint record one author`).
- The narrower permission set is separated from the wider evidence set by a second flag (`scripts/classify-change.py:52` `eval/ is agent-affecting but grants nothing`).
- The pipeline governs itself, because the workflow and the classifier are themselves governed globs (`agentic.config.json:210` `"governed_globs": [`).
- Autonomy is not uniform across the roles, so the level is a per-role fact rather than a default (`docs/governance-policy.md:200` `Hold low autonomy`).
- Every role's container line names its launcher mount, and the policy carries seven such lines (`docs/policy-reconciliation.md:27` `Show the per-role mount statement in every policy entry`).
- The two failed red-team prompts were fixed at the container layer, not at the prompt layer (`eval/red-team-results.md:249` `The fix adds seven nested read-only binds`).
- The fix is seven nested read-only binds over the grant files and the journals (`scripts/run-agent.sh:187` `declare -a OVERLAY_FILES=(`), each mounted read-only over its parent bind (`scripts/run-agent.sh:199` `MOUNTS+=(-v "$REPO/$overlay:$WORKSPACE/$overlay:ro")`).
- The reuse check now also demands those mounts be read-only, so a stale container is recreated (`scripts/run-agent.sh:216` `ws_rw=""; mem_present=""; mem_rw=""; overlays_ro=yes`).
- Each fixed boundary returns a refusal rather than a warning (`eval/red-team-results.md:151` `each authority file returned`), and P7 and P10 flip from not blocked to blocked (`eval/red-team-results.md:18` `| P7 | NOT blocked | blocked |`).
- The write-mode gate is one named command, and the journal records it (`/.memory/gate-audit.log:181` `"tool": "run_fix"`).
- The enforcement is demonstrated by the local suites and the workflow definition, not by a green CI run. This repository's workflow has no recorded run history (`gh run list` -> `no runs listed`, on 2026-10-01). That is a limitation of the evidence, not a claim that the workflow is broken.
- One boundary stays unprobed: no prompt tested a direct write to the SQLite memory database (`eval/red-team-results.md:286` `no prompt here tested`).
- The policy's basis still names the range NM-1 to NM-9 although the log defines ten patterns (`docs/governance-policy.md:13` `patterns NM-1 to NM-9`). The policy cites seven of them (`grep -o 'NM-[0-9]*' docs/governance-policy.md | sort -u` -> `NM-1 NM-2 NM-3 NM-4 NM-5 NM-7 NM-9`). The suite passes because it checks that a cited pattern exists, not that every pattern is cited (`eval/test_policy.py:1132` `def test_near_miss_citations_exist`).
- The policy is an unsigned v1.0.0 draft, and the human checkpoint still owns approval (`docs/governance-policy.md:5` `Reviewed by: pending`).
- The reviewer no longer runs the prose check by hand, because that step is converted (`docs/routing-and-tool-grant-map.md:96` `Run the prose and citation conformance check as`).

## Evidence

Which artifacts settle this decision?

- Policy of record: `docs/governance-policy.md`, seven role entries, each with the four enforcement dimensions and a container-mount line (`eval/README.md:10` `MCP grants and denials, skill scope, ceiling, autonomy, container mounts`).
- Policy basis, cited pattern range: `docs/governance-policy.md:13` `patterns NM-1 to NM-9`.
- Reconciliation of policy and launcher: `docs/policy-reconciliation.md:32` `## Citation maintenance`.
- Enforcement suite: `eval/test_policy.py`, run to `75 passed` (`eval/README.md:18` `75 passed`).
- Drift injections that prove the suite bites: `eval/README.md:27` `Withdraw a policy storage grant, drift one`.
- Storage allow-list, deletion refused everywhere: `mcp/storage/allow-list.json:29` `refused to every role`.
- Retrieval allow-list, the project-manager holds no retrieve: `mcp/retrieval/allow-list.json:41` `"project-manager": [],`.
- Per-role mount statements in the policy, one per role (`grep -c 'the launcher mounts this repository' docs/governance-policy.md` -> `7`).
- Red-team prompt set: `eval/red-team-prompts.md:7` `Ten prompts attack the six Module 4.1 boundaries.`
- Red-team outcomes: `eval/red-team-results.md:7` `Eight prompts were blocked on their first run.`, with P7 and P10 flipped after the fix (`eval/red-team-results.md:21` `| P10 | NOT blocked | blocked |`).
- Red-team refusal recorded in a journal: `eval/red-team-results.md:56` `authorization_denied: role 'project-manager' is not granted 'write_entry'`.
- Refused gate call journals nothing: `eval/red-team-results.md:113` `refused call runs nothing and journals nothing`.
- Reviewer mount state at refusal: `eval/red-team-results.md:36` `/workspace RW=false`.
- Red-team fix: `eval/red-team-results.md:252` `to be read-only, so a stale container is recreated instead of reused`, implemented at `scripts/run-agent.sh:187` and `scripts/run-agent.sh:199`.
- Eval-gated change control: five jobs at `.github/workflows/ci.yml:4` `change-type-check, policy-gate, eval-gate, advisory-review`, with the gating decision at `.github/workflows/ci.yml:17` `policy-gate carries no`.
- Classification globs: `agentic.config.json:210` `"governed_globs": [`, consumed at `scripts/classify-change.py:129` `classification.governed_globs`.
- Calibration produced the policy from named patterns rather than from a template (`docs/calibration-log.md:18` `Ten patterns follow, each named and each cited to the line that evidences it`).
- NM-8 records a self-report that does not reproduce (`docs/calibration-log.md:66` `a role's self-reported count can drift while the artifact stays correct`).
- NM-9 records permission bits a root process ignores (`docs/calibration-log.md:72` `a filesystem permission is not a guardrail when the writing process runs as root`).
- Gate journal that records the local runs rather than a CI run: `.memory/gate-audit.log:165` `"gate": "conformance"`.
- Gate journal showing the CI-role gate runs: `.memory/gate-audit.log:26` `"calling_role": "ci"`.
- CI history gap: `gh run list` -> `no runs listed`, checked on 2026-10-01.
