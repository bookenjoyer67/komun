# Plan — Agentic Engineer M1 lab: quality-control system for one agent workflow

Target Codebase: `~/rev` (Komun — Rust/Axum + SvelteKit 5 + WASM + PostgreSQL, AGPL-3.0)
Course: LaunchCode "Hire Human / Agentic Engineer" (Canvas 269)
Lesson: "Sandbox Your Agents and Set Your Quality Bar" (mod-1_rise-lesson-94dc19558c063baa)
Deliverables uploaded to the LMS: `docs/prd.md`, `docs/rubric.md`, `docs/iteration-log.md` + 3 written answers.

---

## 1. Goal

Build the three quality-control artifacts for ONE narrow single-agent workflow in Komun, then run
that workflow twice inside the module-1 sandbox (`agent-sandbox:komun`), score both runs against the
rubric, and record cycle time / review latency / cost / token counts in a versioned iteration log.

No multi-agent work. No fixes to the repo. The artifact is evidence, not improvement.

## 2. Verified current state (checked 2026-09-24 09:13 CDT, not assumed)

| Fact | Evidence |
|:--|:--|
| Docker daemon up | `docker info` → server 29.8.1 |
| Sandbox image present | `agent-sandbox:komun` (3.27 GB, built 41 h ago); `claude --version` → 2.1.280, `opencode --version` → 1.18.32 |
| Broker image present | `komun-sandbox-broker:local`; launcher `~/rev/sandbox/run-agent.sh` builds/starts it on demand |
| Credentials staged | `~/.config/komun-sandbox/deepseek.key` (600), `~/.claude/.credentials.json` (refreshed today 09:10) |
| Agent container from module 1 | `rev-agent` exists but is **Exited (255), 22 h ago** — must be recreated by `run-agent.sh` |
| Network model | `agent-net` is `--internal`; agent has **no egress**, broker holds the real keys |
| Cargo registry volume | `komun-cargo-registry` warm (575 MB, has axum 0.8.9 / sqlx-core / tokio cache + index + src) |
| Cargo target volume | `rev-cargo-target` is **EMPTY (0 B)** → first in-container build is COLD |
| Host frontend deps | `web/node_modules` absent, `crates/wasm/pkg` absent → any npm/vitest/wasm-pack path would need network. **Excluded from the task choice.** |
| Server/core tests need no DB | `crates/server/src/tests/mod.rs` is pure unit tests; the 2 tokio tests use `connect_lazy` to an unreachable URL |
| Repo git state | 3 module-1 files still untracked: `Dockerfile`, `setup.md`, `sandbox/`, plus `docker-entrypoint.sh`, `settings.json`, `statusline.sh`, `agent-summary.md`; `session_tasks.md` modified |
| `docs/` | tracked (6 docs); no prd/rubric/iteration-log yet |
| `.hermes/` | gitignored in this repo — this plan lives there and stays out of history |

## 3. The workflow to measure (decision)

Name: **Workspace Test Gate**.

Chosen task: *run Komun's documented workspace test command inside the sandbox and summarize the result.*

Why this one:
- It is one command, one agent, no decomposition, no writes — exactly the shape the assignment asks for.
- `cargo test --workspace` is Komun's own documented gate (`AGENTS.md` → "Tests"), so "the right
  command" is a fact in the repo, not a judgement call.
- It needs no database, no node_modules and no network (registry volume is already warm), so a
  failure in the run is the agent's behaviour, not the environment.
- Its output is countable (per-crate `test result:` lines), which makes the rubric falsifiable.

Rejected: coverage estimation (needs cargo-llvm-cov → network), clippy `--release` (cold release
build of the whole workspace), build-command reporting (wasm-pack + npm need network/deps),
vitest/svelte-check (no node_modules, no registry).

Fallbacks if the offline build turns out to be impossible: `cargo check --workspace`, then
`cargo test -p komun-core`.

## 4. Agent and invocation (decision)

Agent: **Claude Code 2.1.280**, model `opus`, inside `agent-rev`, headless, fresh session per run.

```
# host, from ~/rev  (fish)
time docker exec -w /workspace agent-rev claude -p "<PROMPT>" --model opus --output-format json | tee /tmp/run001.json
```

- `time` is the lesson's "time prefix" and yields cycle time (`real Xm Ys`).
- `-p` guarantees a NEW session (the lesson forbids `--continue` / `--resume` on the baseline).
- `--output-format json` prints `total_cost_usd` + `usage.input_tokens` / `output_tokens` — the cost
  and token fields the log demands, captured as data instead of read off `/status` by hand.
- Lesson-literal alternative if preferred: `docker exec -it agent-rev bash`, then inside
  `time claude "..."`, then `/status` → usage menu. Same run, manual capture.

## 5. Deliverable 1 — `docs/prd.md` (full draft, ready to commit)

```markdown
# PRD — Workspace Test Gate

## Workflow description
This workflow runs Komun's documented workspace test command inside the sandbox and reports
whether the workspace's tests pass, naming every failure exactly as the test runner printed it.

## Trigger
A developer working from the repo root on the host, with the agent sandbox running, manually
invokes Claude Code inside the container and asks it to run the project's tests. The workflow does
not fire on a schedule, on a git event, or from another agent. This is a manual, single-run trigger.

## Decision events
1. If the test command exits 0, the workflow reports the workspace as green, gives the per-crate
   pass counts, and recommends proceeding.
2. If the test command exits non-zero, the workflow reports the workspace as not green, names every
   failing test with the assertion text the runner printed, and names the crate to inspect first.
   It does not attempt a repair.
3. If the test command cannot run at all (compile error, missing toolchain, no network), the
   workflow reports the blocker verbatim and stops. It does not install anything, does not disable
   tests, and does not work around the blocker.
4. If the runner's output is longer than can be quoted, the workflow summarizes and states the exact
   command that reproduces the full output. It never drops a failing test's name.

## Actions (ordered, observable)
1. Reads `/workspace/AGENTS.md` to find the documented test command.
2. Runs that command in `/workspace`, exactly as documented, with no extra arguments.
3. Captures the command's complete stdout and stderr, including every `test result:` line.
4. Determines the command's exit status.
5. Summarizes: tests passed and failed per crate, and each failing test by full path with its
   assertion message.
6. States one recommendation — proceed, not ready, or blocked — with a one-line rationale.
7. Writes nothing to the filesystem.

## Acceptance criteria
- AC1 (falsifiable) The summary states the command it ran, and that command is character-for-character
  the test command documented in the repo.
- AC2 (falsifiable) The overall verdict matches the real exit status of that command.
- AC3 (falsifiable) Every failing test named in the summary appears in the real runner output, and
  the summary omits no failing test that appears there.
- AC4 (falsifiable) The per-crate passed/failed counts in the summary equal the counts on the real
  `test result:` lines.
- AC5 (binary gate) The run created, modified, deleted, or moved no file under `/workspace`, and
  performed no commit, push, install, or deploy.
- AC6 (falsifiable) The closing recommendation is consistent with the verdict: green → proceed,
  not green → not ready, blocked → escalate.

## Out of scope
Fixing failures, editing tests, installing tooling or crates, touching the frontend build, and any
multi-step planning beyond running the one command and reporting it.
```

Commit: `git add docs/prd.md && git commit -m "docs: add initial PRD for workspace test gate"`

## 6. Deliverable 2 — `docs/rubric.md` (full draft, ready to commit)

Five dimensions, one per graduable acceptance criterion. AC5 is binary and stays a gate.

Pass threshold: **all binary gates pass, total ≥ 17 / 20, and no dimension scored 1.**

| # | Dimension | Definition |
|:--|:--|:--|
| D1 | Command Fidelity | Did the run use the repo's documented test command, exactly, with no invented flags or substitutions? |
| D2 | Verdict Accuracy | Does the stated pass/fail verdict match the command's real exit status? |
| D3 | Failure-Naming Completeness | Is every failing test named by full path with its assertion text — no omissions, no invented failures? |
| D4 | Count Fidelity | Do the per-crate passed/failed counts match the runner's own `test result:` lines? |
| D5 | Recommendation Consistency | Does the closing recommendation follow from the verdict, and is it specific enough to act on? |

Scoring levels and concrete example cases (each case is what a scorer should be able to point at):

**D1 Command Fidelity**
1 — Ran something else entirely (`cargo test` in one crate, `cargo nextest`, `cargo check`) or never ran a command.
2 — Ran the right command but added arguments the docs do not specify, or ran it from the wrong directory.
3 — Ran `cargo test --workspace` from `/workspace` and said so accurately.
4 — Ran the documented command and quoted the AGENTS.md line it took it from, so a reviewer can confirm the mapping.

**D2 Verdict Accuracy**
1 — Verdict contradicts the exit status (calls a failing run green).
2 — No explicit verdict, or a hedged one ("tests seem mostly fine").
3 — Verdict matches the exit status.
4 — Verdict matches the exit status and the summary says which line of output it was derived from (exit code / `test result:` lines).

**D3 Failure-Naming Completeness**
1 — Failures not mentioned at all, or paraphrased so loosely they cannot be located.
2 — Some failures named, at least one omitted from the real output.
3 — Every failure named by full test path with its assertion message.
4 — Every failure named with assertion message, plus the first place in the failure's file the next reader should look.

**D4 Count Fidelity**
1 — Counts invented or contradicting the output.
2 — Per-crate counts missing, or a total given that does not reconcile with the runner's lines.
3 — Counts match the `test result:` lines per crate.
4 — Counts match and the summary distinguishes "passed" from filtered-out/ignored tests.

**D5 Recommendation Consistency**
1 — No recommendation, or one that contradicts the verdict.
2 — Recommendation present but generic ("review the failures").
3 — One clear recommendation consistent with the verdict.
4 — Recommendation consistent, names the crate or module to inspect first, and states the reason in one line.

Commits (mirrors the lesson's own example history):
```
git add docs/rubric.md && git commit -m "docs: add rubric dimensions and definitions"
# then add scoring levels, example cases and pass threshold
git add docs/rubric.md && git commit -m "docs: add scoring guide, example cases, and pass threshold to rubric"
```

## 7. Deliverable 3 — `docs/iteration-log.md`

Header `# Iteration Log`, newest entry on top, this exact entry shape (from the lesson template):

```markdown
## Run 001 — 2026-09-24 — Baseline
Task: ...
Full prompt: ...
Rubric Scores:
| Dimension | Score (1-4) | Notes |
|---|---|---|
| D1 Command Fidelity | | |
...
| Total | /20 | Pass threshold: gates pass, ≥17/20, no dimension 1 |
Measurements:
- Cycle time: Xm Ys
- Review latency: Xm Ys
- Cost per run: $X.XX (N in / N out)
Pass/Fail:
Observations:
Changes made: None. This is the baseline run.
```

Commit: `git add docs/iteration-log.md && git commit -m "log: run 001 baseline -- cycle time Xm Ys, cost $X.XX, rubric N/20"`

Rules the lesson is explicit about: never delete or rewrite an entry, never squash these commits,
never adjust the PRD or threshold after seeing a run, and fill every field (mark approximations).

## 8. Execution sequence

### Phase 0 — pre-flight (must happen before any graded run)
1. `~/rev/sandbox/run-agent.sh` → creates/reuses `rev-broker`, creates `agent-rev`.
2. Confirm the container and the mount:
   `docker exec -w /workspace agent-rev bash -c 'ls /workspace | head -5; git -C /workspace status -sb | head -3'`
3. Confirm the agent can actually reach the model through the broker with a throwaway call:
   `docker exec -w /workspace agent-rev claude -p "reply with the single word ok"`.
   (Not a graded run — record it in Observations as a pre-flight check if it costs anything.)
4. Warm the compile cache OUTSIDE the graded runs, because `rev-cargo-target` is empty and a cold
   build would otherwise dominate Run 001's cycle time and wreck the run-1 vs run-2 comparison:
   `docker exec -w /workspace agent-rev bash -c 'export PATH=/usr/local/cargo/bin:$PATH; time cargo test --workspace --no-run'`
   If it fails on a missing registry entry: `docker network connect bridge agent-rev`, run
   `cargo fetch`, then `docker network disconnect bridge agent-rev`, and note it in Observations.
5. Capture GROUND TRUTH: run `cargo test --workspace` once myself and save the full output to
   `/tmp/truth.txt`. Scoring D2/D3/D4 means comparing the agent's summary to this file, not to the
   agent's own words.

### Phase 1 — artifacts (before any run; the rubric must be committed first, per the lesson)
- Write and commit `docs/prd.md`, `docs/rubric.md` (two commits), `docs/iteration-log.md` (empty
  header only; Run 001 is added after the run). **Every commit needs your explicit OK first.**

### Phase 2 — Run 001 (baseline, thin prompt — deliberately not over-specified)
Prompt, verbatim, thin so the run reveals Claude's default behaviour:
```
Run the project's tests and summarize the results for me.
```
Then, on the host:
```
time docker exec -w /workspace agent-rev claude -p "Run the project's tests and summarize the results for me." --model opus --output-format json | tee /tmp/run001.json
```
- Do not intervene while it runs (lesson rule). A stall or timeout is still a data point.
- Cycle time = `real` from `time`. Cost + tokens = `/tmp/run001.json`.
- Review latency: start a stopwatch the moment the command returns, stop it when you have finished
  scoring. That number is yours, not mine.
- Then score D1–D5 against `/tmp/truth.txt`, write the Run 001 entry, commit.
- Tear the container down after the run (the lesson wants a bounded, repeatable session):
  `docker rm -f agent-rev` (keep `rev-broker` and the named volumes).

### Phase 3 — one change, Run 002
Pick exactly ONE change, targeting the lowest-scoring dimension from Run 001. Menu (pick one):

- C1 (D3/D4 low — output contract):
  append `Report exactly: the command run; the passed/failed count per crate; one line per failure as "FAIL: <full test path> — <assertion line>"; VERDICT: green|not-green|blocked.`
- C2 (D2 low — verdict source):
  append `Base the verdict only on the command's exit status. Do not infer it from warnings or from the absence of failures in the output you happened to read.`
- C3 (D5 low — recommendation shape):
  append `End with exactly one sentence naming the crate to inspect first and the reason, or "proceed" if the run is green.`
- C4 (AC5 gate failed — scope):
  append `Do not create, modify or delete any file, and do not attempt repairs.`

Then: same command, run 002, score against the same truth file (re-run `cargo test --workspace`
first and regenerate `/tmp/truth.txt` if any test outcome moved), log Run 002 on top of Run 001,
commit with the run-002 message shape.

### Phase 4 — the three written answers
Written last, from the log, in plain student voice, each citing real numbers from the two runs:
1. What Run 001 revealed about default behaviour with the thin prompt (be specific: did it pick the
   right command by itself? did it read AGENTS.md? did it report counts? did it hedge the verdict?).
2. The single change made and the corresponding change in output — dimension by dimension, with the
   score shift.
3. What stayed consistent, what changed, and whether the workflow is stable enough to repeat —
   including the honest confound (compile cache warming between runs; container recreated per run).

### Optional stretch
A third and fourth run with the same prompt to test stability, logged the same way. Cheap now that
the compile cache is warm. Recommended if Run 001 vs Run 002 differ by less than one point.

## 9. Files that change

| Path | Change |
|:--|:--|
| `docs/prd.md` | new |
| `docs/rubric.md` | new |
| `docs/iteration-log.md` | new |
| `~/.hermes/plans/…` (this file) | gitignored, not committed |
| `/tmp/run001.json`, `/tmp/run002.json`, `/tmp/truth.txt` | scratch, outside the repo |

Nothing else. No source file, no test file, no config, no Dockerfile in this exercise.

## 10. Risks and how each is handled

| Risk | Handling |
|:--|:--|
| Cold compile dominates Run 001 and makes run-1 vs run-2 meaningless | Phase 0 step 4 pre-warms the target volume; note in Observations that warm-up happened outside the runs |
| Offline build fails on a missing crate | temporary `docker network connect bridge` + `cargo fetch`, disconnect immediately, record it as an environment change |
| Cargo registry/`cargo` not on PATH in `docker exec` | use `bash -c` with `export PATH=/usr/local/cargo/bin:$PATH` (documented module-1 pitfall) |
| Claude auth/broker failure makes a "0-minute run" | Phase 0 step 3 proves the path with a throwaway call before the graded runs |
| Agent writes files → root-owned junk in the repo | AC5 gate detects it; fix with `docker exec -u 0 agent-rev chown $(id -u):$(id -g) <file>`; note it in the log |
| Run 001 scores low and tempts a PRD/threshold edit | lesson rule: the PRD is the intended bar. Fix the prompt, not the standard |
| Two graded runs differ only in prompt wording | that IS the experiment; keep the change to exactly one clause |

## 11. Open questions — need your call before I touch anything

1. **Task**: `cargo test --workspace` as planned (recommended), or the narrower `cargo test -p komun-core`, or `cargo check --workspace`?
2. **Who drives the two runs**: you at the terminal with me handing you the exact command line (recommended — review latency is a human measurement), or me running them via my terminal while you time the review?
3. **Headless `-p --output-format json`** (recommended, machine-readable cost/tokens) or the lesson-literal interactive form with `/status`?
4. **Commit the module-1 artifacts first?** `Dockerfile`, `setup.md`, `sandbox/`, `docker-entrypoint.sh`, `settings.json`, `statusline.sh` are still untracked, and `agent-summary.md` is the module-1 smoke-test evidence. Committing them makes the repo history match the story the lab tells. Include or leave alone?
5. **Stretch runs**: do 4 runs total for the stability check, or stop at 2?

---

## Provenance

This plan is a verbatim copy of `.hermes/plans/2026-09-24_0913-agentic-engineer-mod1-qc-system.md`,
committed here because `.hermes/` is gitignored (`.gitignore:14`) and a citation into an ignored path is
dead for any reader who clones the repository. Nothing above this line was edited; the file was copied
whole, and its sha256 prefix is `644e5c86e4b213e3`. It is dated 2026-09-24, before the capstone work began, and its
section 2 ("Verified current state", checked 2026-09-24 09:13 CDT) is the pre-work gap list: what existed,
what was absent, and what was therefore not yet assumed.
