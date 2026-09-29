---
classification: internal
project: proj-komun
doc_type: runbook
---

# Runbook: the quality gate and its recorded baselines

Which commands are the gate, and what numbers is a run compared against?

Run all four from the repository root, with cargo on `PATH` inside a container (`docs/DEVELOPMENT.md:166` `export PATH=/usr/local/cargo/bin:$PATH`):

```bash
cargo build --workspace --all-targets   # compile every crate, tests included
cargo test --workspace                  # unit + integration tests
cargo clippy --release -- -D warnings   # the zero-warning standard
cd web && npm run check && npm run build && npx vitest run
```

## What were the numbers on the recorded baseline?

Measured 2026-09-25 in the agent sandbox on `rustc 1.95.0` and `node v22.23.2`, on the commit that dropped the plaintext column (`AGENTS.md:201` `Measured 2026-09-25 in the agent sandbox`):

- `cargo test --workspace` gave 158 passed, 0 failed, 0 ignored, split 20 in `komun-core` and 138 in `komun-server` (`AGENTS.md:202` `158 passed, 0 failed, 0 ignored`).
- `cargo clippy --release -- -D warnings` exited 0 with no lints, and the only line cargo printed was a future-incompat note about `sqlx-postgres` (`AGENTS.md:204` `exit 0, no lints`).
- `npm run check` reported 0 errors and 0 warnings, and `npx vitest run` ran 82 tests in 7 files, all passing (`AGENTS.md:205` `0 errors, 0 warnings`).
- The frontend gates need `crates/wasm/pkg/` to exist first (`AGENTS.md:206` `The frontend gates need`).

## Which mistakes make a measurement wrong?

- Re-measure after touching a source file, because clippy caches and a silent second run is a cache hit (`docs/DEVELOPMENT.md:256` `A second run with no source change prints nothing at all`).
- Never silence a lint with `#[allow]`; fix the code or record the diagnostic as a finding (`docs/CONVENTIONS.md:124` `Silencing a lint with `#[allow(...)]` is forbidden`).
