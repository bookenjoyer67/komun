# Komun Pre-Merge Quality Gate: One-Pager

**What it is:** a governed, multi-agent pre-merge quality gate over the Komun codebase (a Rust/Axum and PostgreSQL server with a SvelteKit 5 front end). Seven roles run behind one launcher, with four MCP servers, seven deterministic gates, five CI jobs, a read-only console, and a human checkpoint system.

## The problem

Before this work, checking a change before merge meant pulling a human or a general-purpose agent through a long review. Both were slow, both were hard to audit, and neither left a clean, reproducible record of what was checked and what each actor was allowed to touch.

Agent-run checks also drifted. The same three defect classes recurred across review cycles even when the agent was told to quote its evidence, so the failures were mechanical rather than motivational.

There was no enforced boundary either. A helper process could read or write almost anything on the host, and an agent mistake could reach the files that record the run's own behaviour, so the audit trail was not trustworthy.

## What was built

- One launcher (`scripts/run-agent.sh`) starts any of the seven roles.
- Four MCP servers (gate, storage, retrieval, coursetools) are the only tools the roles hold.
- Seven deterministic gates check every change: test, clippy, fmt, policy, conformance, webcheck, webtest, plus a write-mode fmt-fix for the implementer alone.
- Each role gets a fixed mount matrix saying what it may write, what is read-only, and which cache it may touch.
- A read-only terminal console (`rev/console/`) watches the run live; Enter approves a checkpoint and `e` opens the ruling chooser.
- Three memory layers (project, knowledge, reference) record decisions behind a pre-commit hard stop.

## Results, measured

Baseline, Komun at HEAD measured 2026-09-25: `cargo test --workspace` 158 passed / 0 failed / 0 ignored (20 komun-core, 138 komun-server); `cargo clippy --release -- -D warnings` exit 0 with zero lints; `npm run check` 0 errors and 0 warnings; `npx vitest run` 82 tests across 7 files.

- Conformance conversion: one agentic pass took 45m13s wall clock (2,713 seconds) and $8.37 of model spend over 80 API requests. The replacement process launches in 0.844s, 0.424s and 0.422s, about 0.42s, at $0.00 and zero model calls. That is 2,713 s divided by 0.42 s, roughly 6,400 times faster, with $8.37 removed per run. Three runs produced byte-identical reports under one SHA-256 digest (15f1c690...).
- Coverage of that check: 299 citations checked, 289 resolved at the cited line, 10 findings, and 72 pointers skipped for want of a literal.
- Earlier test-gate prompt revision: cycle time 4m14s to 2m33s (down 40%); cost $0.4845 to $0.1635 (down 66%); model requests 10 to 5.
- Four-run end-to-end regression (2026-09-29): D1 2323s, D2 1592s, H1 1698s, H2 1438s, a total of 7,051s, about 1.96 hours. Each run reached a reviewer verdict with the conformance gate run by the tester.
- Defects caught: D1's test gate exited 101 (156 passed, 2 failed) and the harness caught it before merge. The conformance script also surfaced 10 citation and rule findings.
- Governance suite: 90 tests in the policy gate, made up of 75 permission tests plus 15 validator tests.

## Risk and governance

Least privilege is enforced, not advisory: an `--internal` Docker network with egress blocked, a credential broker that holds the provider keys (the agent container receives only a dummy token), a per-role mount matrix, and memory layers kept read-only by a hard stop.

Red team: 10 probes (P1 to P10) were run against the enforcement boundaries. 8 were blocked on the first run; P7 and P10 were not. Both failed at the same layer, the container mounts. The fix added seven nested read-only binds over the workspace and memory binds, and the container reuse check now requires them read-only. After the fix all 10 probes are blocked.

## Honest limitations

- The conformance checker has 2 known false positives from wrapped-literal pairing; both are recorded and pinned by tests.
- `fmt` is red at HEAD and is not attributable to either change.
- No cost figure exists for the four-run regression, because the broker reports no usage.
- The sandbox protects nothing if the host is compromised, because the broker runs as a normal user on that host.

## Next steps

- Fix the 2 wrapped-literal false positives so the conformance gate can fail clean.
- Attribute the pre-existing `fmt` failures so the gate can go green.
- Add per-run cost reporting to the broker so the regression can be costed.
