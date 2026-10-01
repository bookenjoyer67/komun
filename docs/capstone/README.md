# Capstone package index

This directory holds the capstone submission package for the Komun pre-merge quality gate. The
repository is the graded artifact; this directory is the part of it written to be read by a person
rather than by a gate.

Each artifact below states what it is, who it is for, and which capstone criterion it answers. Where a
number appears, it is reproduced from the record with a `path:line` citation in the artifact itself.

## The artifacts

| File | What it is | Criterion |
|:--|:--|:--|
| `one-pager.md` | 751 words. The problem, the workflow, the result, for a reader who will give it two minutes | Stakeholder communication |
| `deck.md` | Marp Markdown source, 20 slides, 1,339 words. Renders to HTML and PDF with one command | Deck clarity and flow; deck design |
| `deck.html` | Rendered deck, 130,718 bytes | Same, in a browser |
| `deck.pdf` | Rendered deck, 156,417 bytes, 20 pages at 960x540 | Same, as a file |
| `impact-report.md` | 1,302 words. Before and after across five axes, two confirmed improvement cycles, measured separated from projected | Iteration narrative and impact |
| `scoping.md` | 1,289 words. Chosen workflow, quantified baseline pain, why a pipeline rather than simpler automation, Module 1 to 4 artifact gaps | Workflow scoping |
| `plan-pipeline-validation.md` | 307 lines. The capstone plan of record, copied verbatim from the gitignored `.hermes/plans/` so that it travels with the repository. The provenance note at its end records what was and was not edited | Workflow scoping |
| `video/script.md` | 3,115 words. Shot-by-shot script for a 5 to 10 minute walkthrough | Stakeholder communication |
| `video/demo-runbook.md` | 3,691 words. Every command the demo runs, with expected output and the checks between segments | Evidence for the demo |
| `video/capture.md` | 2,474 words. The captured output each segment displays | Evidence for the demo |
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
| Evaluation and calibration | `eval/`, `docs/calibration-log.md`, `eval/red-team-results.md` and `eval/red-team/` |
| Governance, security and CI-CD | `docs/governance-policy.md`, `.github/workflows/ci.yml`, `eval/test_policy.py` |
| Right-tool decisions | `docs/adr/`, `docs/step-classification.md` |

## Rendering the deck

Verified on this host, Marp CLI 4.5.1, about 7 seconds:

    npx -y @marp-team/marp-cli@4.5.1 --pdf --allow-local-files docs/capstone/deck.md -o docs/capstone/deck.pdf
    npx -y @marp-team/marp-cli@4.5.1 --allow-local-files docs/capstone/deck.md -o docs/capstone/deck.html

The Markdown is the source of truth and is diffable in the repository. The PDF is reproducible: a
regenerated copy holds the same 20 pages and the same 156,417 bytes, and the extracted text is
identical. The bytes differ only in the embedded creation timestamp, so no digest is claimed for it.

## Reproducing the gate results

The policy and validator suites run inside the sandbox container, not on the host:

    docker exec -w /workspace agent-rev-m3 python3 -m pytest eval/test_policy.py -q
    docker exec -w /workspace agent-rev-m3 python3 -m pytest eval/test_deterministic_step.py -q

The citation checker runs on the host and takes one input and one output path:

    python3 scripts/validate_doc_conformance_deterministic.py --input docs/adr/ADR-006-governance-policy.md --output /tmp/adr006.json

## Citation integrity of the ADR set

Measured by running the repository's own checker over each file. The five ADRs added for this
submission are `ADR-002` to `ADR-006`, all under `docs/adr/`.

| File | Lines | Citations | Resolved at the cited line |
|:--|--:|--:|--:|
| `ADR-001-doc-conformance-deterministic-conversion.md` | 172 | 35 | 26 |
| `ADR-002-rubric-design.md` | 172 | 67 | 67 |
| `ADR-003-memory-layout.md` | 147 | 65 | 65 |
| `ADR-004-mcp-tool-boundaries.md` | 122 | 112 | 112 |
| `ADR-005-subagent-scoping-and-routing.md` | 120 | 85 | 85 |
| `ADR-006-governance-policy.md` | 121 | 91 | 91 |

The five ADRs added for this submission resolve 420 of 420 citations. `ADR-001`, which predates this
submission, resolves 26 of 35: nine of its citations point at lines that moved after it was written.
That drift is pre-existing and is left as found rather than silently repaired, because an accepted ADR
is a record of a decision at a point in time.

## Limits of this package

* Review latency and defect rate are not directly measured. The record carries no denominator over
  time, so no defect rate is computed, and the proxies used instead are named where they appear.
* The four-run end-to-end regression carries no cost figure, because the credential broker reports no
  usage.
* `fmt` is red at `HEAD` and is unattributed to either change; it is pre-existing.
* The conformance checker has a known false positive: a quoted literal that wraps to the next line can
  pair with a neighbouring literal, so a correct citation can read as drift.
* Rollback is documented in `ADR-001` and was never exercised.
* The Module 1 plans under `.hermes/plans/` are gitignored and do not travel with the repository, so
  the citations to them in `scoping.md` resolve only on the machine that holds the directory. The
  capstone plan is not affected: it is tracked here as `plan-pipeline-validation.md`.
