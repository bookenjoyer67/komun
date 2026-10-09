# Capstone package index

This directory holds the capstone submission package for the Komun pre-merge quality gate. The
repository is the graded artifact; this directory is the part of it written to be read by a person
rather than by a gate.

Each artifact below states what it is, who it is for, and which capstone criterion it answers. Where a
number appears, it is reproduced from the record with a `path:line` citation in the artifact itself.

## The artifacts

| File | What it is | Criterion |
|:--|:--|:--|
| `one-pager.md` | 795 words. The problem, the workflow, the result, for a reader who will give it two minutes | Stakeholder communication |
| `deck.md` | Marp Markdown source, 20 slides, 1,378 words. Renders to HTML and PDF with one command | Deck clarity and flow; deck design |
| `deck.html` | Rendered deck, 130,945 bytes | Same, in a browser |
| `deck.pdf` | Rendered deck, 157,054 bytes, 20 pages at 960x540 | Same, as a file |
| `impact-report.md` | 1,849 words. Before and after across five axes, two confirmed improvement cycles, measured separated from projected | Iteration narrative and impact |
| `scoping.md` | 1,751 words. Chosen workflow, quantified baseline pain, why a pipeline rather than simpler automation, Module 1 to 4 artifact gaps | Workflow scoping |
| `plan-pipeline-validation.md` | 311 lines. The capstone plan of record, copied verbatim from the gitignored `.hermes/plans/` so that it travels with the repository. The provenance note at its end records what was and was not edited | Workflow scoping |
| `video/script.md` | 3,115 words. Shot-by-shot script for a 5 to 10 minute walkthrough | Stakeholder communication |
| `video/demo-runbook.md` | 3,880 words. Every command the demo runs, with expected output and the checks between segments | Evidence for the demo |
| `video/capture.md` | 2,474 words. The captured output each segment displays | Evidence for the demo |
| `evidence/` | The four Module 2.3 drill files, copied verbatim from outside the repository so the citation in `iteration-log.md` resolves for a reader | Production integration and tool-evolution drill |
| `assets/architecture.svg` | The seven roles, the gates, and where each sits | Deck design |
| `assets/governance-matrix.svg` | Role against tool grant, as the policy states it | Deck design |
| `assets/before-after.svg` | The measured cost and latency change, on a real axis | Deck design |

## Where the evidence for the other criteria lives

The package above answers the communication and narrative criteria. The build criteria are answered by
the repository itself, and these are the files a reader should open.

| Criterion | Primary evidence |
|:--|:--|
| Sandboxed environment | `setup.md` sections 4 to 6, `sandbox/README-m3.md`, `Dockerfile`, `scripts/run-agent.sh` |
| Quality specification and baseline | `docs/rubric.md`, `docs/agent-rubric.md`, the measured baseline block in `AGENTS.md` |
| Agents, skills and memory | `.claude/agents/`, `.claude/skills/`, `docs/memory-architecture.md` |
| Orchestration and MCP tools | `docs/orchestration-diagram.md`, `docs/routing-and-tool-grant-map.md` and its `.json`, `scripts/start-mcp-servers.sh` |
| Evaluation and calibration | `eval/`, `docs/calibration-log.md`, `eval/red-team-prompts.md` and `eval/red-team/` (the raw run report is kept outside this repository) |
| Governance, security and CI-CD | `docs/governance-policy.md`, `.github/workflows/ci.yml`, `eval/test_policy.py` |
| Right-tool decisions | `docs/adr/`, `docs/step-classification.md` |
| Production integration and tool-evolution drill | `docs/iteration-log.md` Run 003, `docs/capstone/evidence/`, `docs/adr/ADR-001-doc-conformance-deterministic-conversion.md` |

## Rendering the deck

Verified on this host, Marp CLI 4.5.1, about 7 seconds:

    npx -y @marp-team/marp-cli@4.5.1 --pdf --allow-local-files docs/capstone/deck.md -o docs/capstone/deck.pdf
    npx -y @marp-team/marp-cli@4.5.1 --allow-local-files docs/capstone/deck.md -o docs/capstone/deck.html

The Markdown is the source of truth and is diffable in the repository. The PDF is reproducible: a
regenerated copy holds the same 20 pages and the same 157,054 bytes, and the extracted text is
identical. The bytes differ only in the embedded creation timestamp, so no digest is claimed for it.

## Reproducing the gate results

The policy and validator suites run inside the sandbox container, not on the host:

    docker exec -w /workspace agent-rev-m3 python3 -m pytest eval/test_policy.py -q
    docker exec -w /workspace agent-rev-m3 python3 -m pytest eval/test_deterministic_step.py -q

The citation checker runs on the host and takes one input and one output path:

    python3 scripts/validate_doc_conformance_deterministic.py --input docs/adr/ADR-006-governance-policy.md --output /tmp/adr006.json

## Citation integrity of the ADR set

Measured by running the repository's own checker over each file. All eight ADRs sit under `docs/adr/`;
`ADR-002` to `ADR-006` were added for this submission, and `ADR-007` and `ADR-008` came after it.

| File | Lines | Citations | Resolved at the cited line |
|:--|--:|--:|--:|
| `ADR-001-doc-conformance-deterministic-conversion.md` | 211 | 39 | 30 |
| `ADR-002-rubric-design.md` | 178 | 68 | 68 |
| `ADR-003-memory-layout.md` | 153 | 66 | 66 |
| `ADR-004-mcp-tool-boundaries.md` | 127 | 112 | 78 |
| `ADR-005-subagent-scoping-and-routing.md` | 126 | 85 | 42 |
| `ADR-006-governance-policy.md` | 127 | 91 | 62 |
| `ADR-007-beta-tester-and-browser-mcp.md` | 91 | 23 | 23 |
| `ADR-008-marketplace-parity-non-goals.md` | 88 | 13 | 2 |

The five ADRs added for this submission resolve 316 of 422 citations; `ADR-001`, which predates it,
resolves 30 of 39. The unresolved remainder is line drift, and it is left as found rather than
silently repaired, because an accepted ADR records a decision at a point in time. The same counts
appear at `0457339`, the revision before the last merge, so the drift predates it.

## Limits of this package

* Review latency and defect rate are measured only in the two appended sections of `impact-report.md`,
  from the audit rows. The human accept-or-reject share of review is not covered, and no rate is offered for it.
* The four-run end-to-end regression carries no cost figure, because the credential broker reports no
  usage.
* `fmt` is red at `HEAD` and is unattributed to either change; it is pre-existing.
* The conformance checker has a known false positive: a quoted literal that wraps to the next line can
  pair with a neighbouring literal, so a correct citation can read as drift.
* Rollback is documented in `ADR-001` and exercised on 2026-10-03. It stops on 4 conflicts; the conformance gate then exits 2, against exit 0 at HEAD.
* Both Module 1 plans travel with the repository as verbatim copies, `plan-pipeline-validation.md` and
  `mod1-pre-work-plan.md`. The gitignored `.hermes/plans/` originals are named only in each copy's
  provenance note, so no citation in the package depends on that directory.
