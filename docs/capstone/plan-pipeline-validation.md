# Plan — validating the agentic pipeline on Komun (one real, long-running task)

Target codebase: `~/rev` (Komun — Rust/Axum + SvelteKit 5 + WASM + PostgreSQL)
Pipeline under test: `~/agentic-console` (gate/storage/retrieval MCP servers + role definitions + the
ratatui console as the operator surface)
Course context: LaunchCode "Agentic Engineer" (Canvas 269), Module 4.3 acceptance +
the capstone regression. `~/rev` is the Target Codebase for every module — not re-asked.

Goal in one sentence: run the pipeline against Komun on ONE meaningful, long-running **product** task,
with a control arm, so that "the pipeline has value" and "the pipeline is a loop" are each settled by
recorded evidence instead of by impression.

---

## 1. Verified current state (measured 2026-09-30 13:40 CDT, not assumed)

| Fact | Evidence |
|:--|:--|
| Agent sandbox alive, servers up | `docker exec agent-rev-m3 ps -eo pid,etime,args` → storage `:8001` (1d04h), retrieval `:8002` (1d04h), gate `:8003` (3h15m) |
| No run in flight | same ps → **no `claude` process**; broker's last proxied call 17:29 UTC, 70 min before this reading |
| Broker up, provider reachable | `rev-broker` Up 28h; log tail = `200 POST /v1/messages` — **and two `429`s at 17:27 and 17:29 UTC** |
| Console is live against this repo | pid 3460862 `~/rev/console/target/release/agentic-console --repo /home/computing/rev`, in tmux `-L agentic-console` session `ac` (up 52 min) |
| A second console has been up 20 h | pid 3378204 `agentic-console --repo .../scratch/approval-fixture/repo` |
| Gate vocabulary is 8 names | `agentic.config.json` → `toolchain.commands` = test, clippy, fmt, policy, conformance, fmt-fix, webcheck, webtest. **There is no `console` gate.** |
| The console gate is still not landed | run A's brief asks for it; the vocabulary above proves it absent |
| Gate journal holds 190 rows | `.memory/gate-audit.log`: fmt **57**, test **52**, clippy **41**, conformance **21**, policy 14, webcheck 2, webtest 2, fmt-fix 1; exits 0×130, 1×56, 101×3, 2×1; **calling_role: tester 144/190** |
| 168 commits, and they lean process | `git log -200`: docs 29, fix 22, feat 15, log 14, memory 6, chore 6, agent 6, test 3 …; **45 of the last 200** are citation/log/docs churn |
| The run record is huge in both repos | `~/rev/docs/iteration-log.md` 112 KB / 1369 lines; `~/agentic-console/docs/iteration-log.md` 104 KB |
| Storage journal holds 75 entries | `.memory/storage.db`: plan 17, decision 21, test-result 19, review 13, selftest 5 |
| Work in the tree that is the human's | `git status`: `PORTING.md`, `agentic.config.json`, `scripts/agentic_config.py` modified, and `console/` untracked |
| Today's two pipeline runs produced nothing | run A (console gate) not in the vocabulary; run B halted at its **first role** (`run-pipeline-run-b-console-legibility-20260930-095550.log`, EXIT=0) |

### The run B halt, precisely

- Deliverable: console Phase 1 legibility (T1.1–T1.8) inside `~rev/console/`.
- Blocker, verbatim from the planner: `Error executing tool file_read: Path escapes the project root: /root/console-v2-plan.md`, and `mcp__coursetools__file_read` **is rooted at `/workspace`**.
- The file is real and staged: `docker exec agent-rev-m3 ls -la /root/console-v2-plan.md` → 16221 bytes, 14:55 UTC — exactly run B's start time. It sits one directory above the only root any role may read.
- The run refused to paper over it, refused to relay a paraphrase ("a summary replaces the history it describes"), halted, and escalated — and the escalation named both remedies itself. **This is the pipeline behaving correctly and the operator having staged the input one directory wrong.** It is a five-minute fix, not a defect.

---

## 2. The honest diagnosis — what the loop actually is

Three measured patterns, no adjectives:

1. **The pipeline has only ever been pointed at itself.** Every run's "system under test", in the
   iteration log's own words, is a pipeline artifact: gate vocabulary, gate summaries, selftests,
   conformance scripts, `routings`/`SCHEMA.md`, memory system, role definitions. Across 8 runs and 75
   storage entries, the only product-code tickets that exist are `KOMUN-3101` (a clippy
   `assertions_on_constants` fix in `crates/server`) and `KOMUN-2026-09-29-UNTESTED-INVARIANT` (one
   core test pinning the `db_enum!` agreement). Two small ones.
2. **The headline run's own deliverable was a formatter.** Run 008's change, per its log: two gate
   names plus one `run_fix` call whose argv is `["cargo","fmt","--all"]`. Its stated value is that the
   gate found 1101 added / 2475 removed hunks across 35 files of pre-existing drift. That is a
   pre-commit hook's job, and its measured cost was 0.242 s once dispatched.
3. **The record generates the next run's work.** Entries are PREPENDED, so every entry shifts every
   `path:line` citation below it, in a repo whose docs cite by line. 21 conformance runs and a cluster
   of "repair citations shifted by …" commits are the fingerprint. The loop is not the agent wandering;
   it is a documentation format that bills every run for the last run.

Why it reads as "no proof of value": the pipeline's own thesis — **a step an agent repeats with a
stable outcome should stop being an agent step** — was proven once (ADR-001: 45 min and $8.37 →
0.9 s and $0, byte-identical output) and then not applied again. A loop dressed as a pipeline is
exactly what a step that should have been converted looks like.

What is missing is not more runs. It is **a task the pipeline did not write, and a measurement the
cheap path could not have produced.**

---

## 3. What "proof of value" has to mean here (the experiment)

### 3.1 The claim under test

> The orchestrated pipeline produces, on a product task in Komun, verified artifacts that a
> deterministic path (the repo's own gates plus a short script) does not, at a cost worth paying.

This is falsifiable, and it must be tested against a **control arm** — otherwise a green run proves
only that the pipeline can do a thing a shell script also does, which is the ADR-001 result and not a
proof of value.

### 3.2 Metrics, per run — every one obtainable, none invented

| metric | how it is read |
|:--|:--|
| invocation and exit code | the `docker exec … claude -p` line and `$?` |
| wall clock, per phase | gate-journal timestamps + session JSONL mtimes |
| model calls | `docker logs rev-broker \| grep -c 'POST /v1/messages'` — a before/after delta |
| tokens | **unmeasurable** — the broker reports status and duration, not usage. State that; do not estimate |
| artifacts written | `git -C ~/rev diff --stat` before/after, plus `git status --porcelain` |
| gate evidence appended | the journal's new rows: gate, exit_code, calling_role, duration |
| product vs process | lines of product code (`crates/`, `web/`) against lines of docs/pipeline in the same diff |
| defects the control missed | the control arm's own output, run over the same tree |
| honest limitations | the run's own carried-findings list, quoted |

### 3.3 The control arm (this is the part that makes it an experiment)

For EACH task, before the orchestrated run:

1. Run the cheap path on the same tree: the repo's documented gates, exactly —
   `cargo test --workspace`, `cargo clippy --release --all-targets -- -D warnings` after touching a
   source file, `cargo fmt --check`, plus the task's own scripted check.
2. Record what it finds, in a file, with the command that produced each figure.
3. After the run, diff the two findings sets. **Value = the run's findings minus the control's.**

The control is not a strawman: it is what the repo already does in CI, and it is the bar the pipeline
must clear.

### 3.4 The falsification criterion — agreed in advance

The pipeline is assessed as **not paying for itself** if, on these tasks, any of these hold, recorded
rather than argued:

- the run's diff is ≥50% process/citation/docs churn;
- the control catches the same defect set, with no finding unique to the run;
- a criterion is recorded UNVERIFIED or settled by operator measurement because no gate name exposes it;
- the run halts on an environment precondition (as run B did) after tokens are spent;
- wall clock per verified artifact exceeds the ADR-001 baseline for that class of step.

Write the verdict down whichever way it falls. A recorded negative is the most valuable artifact this
exercise can produce.

---

## 4. Task slate — the decision to make

Module 4.3's acceptance is explicit: **at least two DEVELOPMENT tasks (the class the pipeline was
tuned on) and two HOLDOUT tasks (a surface no earlier run touched)**, driven through the FULL
orchestration, with per-run evidence. The slate below is chosen so each task maps to gate NAMES, and
so the four together satisfy that requirement.

### Development tasks (Rust backend + the gates — the tuned class)

- **D1 — the stale wasm artifact (a false green the pipeline itself named).** Run 008's own carried
  findings say it: "No gate rebuilds the wasm package the frontend consumes. The reformat rewrote
  `crates/wasm/src/lib.rs` while `crates/wasm/pkg/` held a build from ten hours earlier, so those
  frontend tests ran against a stale artifact." Fixing this means a new gate name (a vocabulary
  change) plus the staleness check — and the value is exactly the class of defect the pipeline exists
  to catch: a green suite that measured the wrong bytes.
- **D2 — the frontend API layer.** 47 `fetch(` calls to `/api/` across 18 files, 18 of them in
  `web/src/lib/stores/auth.ts` alone, while `web/src/lib/api/**` holds only ~6. Consolidating them onto
  the typed client is a genuine multi-file refactor with real regression risk, and every criterion maps
  to existing gate names (`webcheck`, `webtest`, plus `test`/`clippy` as regression cover).

### Holdout tasks (surfaces no earlier run touched)

- **H1 — a live runtime behaviour.** `crates/server/src/rate_limit.rs` is a per-IP limiter that no run
  has ever exercised. Its behaviour is proven only on the live artifact: boot the server, make two
  requests, watch the second get refused. A unit test validates the logic and not the wiring.
- **H2 — a client-side crypto boundary.** `crates/wasm/src/lib.rs` (x25519 + XChaCha20Poly1305) is
  cited by the docs and never gated. A criterion here must be measurable offline, without network.

Alternative/additional holdouts, if either above reads as too far from the app's centre:
`crates/server/src/api/` route-level behaviour; an additive `migrations/00N_*.sql` plus a boot that
proves it applied; `web/src/service-worker.ts` (PWA cache correctness — a false-green surface by
nature).

### Tasks explicitly NOT in this slate

- Anything whose deliverable is a document, a citation repair, or a pipeline self-description. That is
  the loop, and re-entering it here would invalidate the experiment.
- The console legibility work (run B) as a *measured* task: it is worth finishing, but it is console
  work, and the console is a tool beside the product. It belongs in its own small run after the
  unblock in §6 — and the console crate's ungated status (no `console` gate) is itself the most
  defensible thing the pipeline could fix about the console.

---

## 5. Run mechanics (evidence-preserving, from hard-won failures)

1. **Stage the brief as a FILE inside the container's readable root** — `/workspace/.dispatch/<CARD>.md`
   — and invoke `claude -p "$(cat .dispatch/<CARD>.md)"`. Never as an argv string, never from `read_file`
   output (its `N|` prefixes ship as payload).
2. **Everything a role must READ goes under `/workspace`.** The only file-reading path a role holds is
   `mcp__coursetools__file_read`, rooted at `/workspace`. A design doc at `/root` is unreachable to
   every role, whatever the container. This is run B's blocker, and the run diagnosed it correctly.
3. **Checkpoint rulings: one file per ruling, item by item.** Stage it, `sha256sum` both sides,
   `claude --resume <session-id> -p "$(cat /workspace/<ruling>.md)"`. A blanket "approved, proceed" is
   not a ruling and the runs say so. Take a liveness read first (`ps -eo args | grep -c '[c]laude'` must
   be 0) — two concurrent resumers on one session jsonl is a real, already-observed failure.
4. **Capture the session JSONL out after EVERY invocation** — `/root/.claude/projects/<slug>/<session>.jsonl`.
   `claude -p` buffers stdout, so a killed phase leaves an empty transcript file.
5. **Cap wall clock, and terminate INSIDE the container**: `docker exec agent-rev-m3 pkill -f 'claude -p'`,
   then SIGKILL, then assert the count is 0. A host-side `timeout` around `docker exec` kills the client
   and leaves the workload running.
6. **Record the journal watermark before and after** (`wc -l .memory/gate-audit.log`) and pin the base
   revision (`git rev-parse HEAD`) — a base-versus-current gate re-reads HEAD on every invocation, so a
   commit landing mid-run moves the base silently.
7. **No mid-flight steering.** To widen scope, kill the run while it has produced nothing, fold the new
   deliverable into the brief, relaunch fresh.
8. **Prefix re-runs** (`run2-<id>-<phase>.txt`) and keep the failed attempt's files.
9. **Sweep for parked pollers** after any long-blocked run before declaring the work stopped.
10. **Provider posture:** two 429s are already in the broker log. On a 429, park with ONE bounded timer
    and resume on the clock; never retry into the window. Report the run as blocked-by-provider, never
    as a pass or a failure of the change.

---

## 6. Immediate unblock, before any long run (cheap, and it is one file deep)

| # | item | the decision |
|:--|:--|:--|
| U1 | run B's design doc is at `/root/console-v2-plan.md` in `agent-rev-m3`, outside every role's readable root | move/copy it under `/workspace` (accepting that a prose file there falls under the `conformance` gate), or widen the coursetools read root. The run named both; pick one |
| U2 | the `console` gate does not exist, so `~rev/console/` is ungated | finish run A's brief: add the `console` command to `toolchain.commands` with `writes: false` and `CARGO_TARGET_DIR=/tmp/...`, plus the host-side `vendor-console-deps.sh` priming script |
| U3 | two console processes are live (one 52 min, one 20 h against a scratch fixture) | confirm both are yours; leave them alone. Do not let a new run share a container with a live one |
| U4 | the human's uncommitted work (`PORTING.md`, `agentic.config.json`, `scripts/agentic_config.py`, untracked `console/`) | decide before the first run: commit boundaried paths, or state that the run's diff must exclude them. A gated run measured over an unsplit tree attributes someone else's diff to itself — run 008 already recorded exactly that mis-attribution |

---

## 7. The standing workflow this plan proposes for Komun

Not just this experiment, but how the three parties divide work from here:

| who | what they own |
|:--|:--|
| the pipeline (roles + gates) | work whose acceptance criterion is a **gate name**, on a **product** surface, where a second pair of eyes matters: multi-file refactors, behaviour with a live-assertion harness, anything where the cheap path has historically shipped a false green |
| a deterministic script + a gate name | **any step that repeats with a stable outcome**. The conversion loop is the pipeline's own thesis and the fix for the loop: a run that has produced the same shaped check twice becomes a script, then a gate, then a CI step |
| I (this session) | brief authoring, the control arm, journal-watermark and diff measurement, the verification of every claim the run makes, and the honest write-up |
| you | checkpoint rulings, the commits and pushes, and the scope decisions |

And the one structural change that stops the loop from re-billing every run: **the iteration log's
entries shift every citation below them.** Either append entries at the end, or cite by anchor rather
than by line, or accept line-citations and stop gating prose. Until one of those is chosen, every run's
own record manufactures the next run's work — by construction.

---

## 8. Decisions locked (2026-09-30)

| # | decision |
|:--|:--|
| Slate | **D1 + D2 + H1 + H2** — Module 4.3's shape (two development, two holdout) |
| Control arm | **Yes, for every task** — the cheap path runs first, then the findings sets are diffed |
| U4 | **Committed before the first run**, so every run diffs against a clean tree |

Landed on the way: `1abb67b` (each canned ruling bound to the checkpoint it answers — `agentic.config.json`
plus `scripts/agentic_config.py`; the embedded DEFAULT verified JSON-equal to the config, and `halt`'s empty
`checkpoints` list verified as the deliberate unscoped case, not a defect) and `0a2d312` (the `PORTING.md`
console section). Tree is clean apart from `.memory/retrieval-audit.log` (run output, tracked by accident) and
the untracked `console/`. **Nothing pushed.**

### First measured finding, recorded before the first run

`PORTING.md`'s new console block makes eight `path:line` citations. **Four were stale** — `console/open.sh`
`:32`→`:37` and `:26`→`:27`, `console/README.md` `:84`→`:101`, `agentic.config.json` `:355`→`:389` — and are
now corrected. The conformance gate returned **PASS throughout (181 findings, base == current)**, because
`PORTING.md` **is not in `gates.conformance.files`**: that set is the twelve governed docs, and the portability
document is not one of them.

So a freshly written block carried four wrong line numbers and no gate in the surface could see it. This is
the exercise's first finding, it is a false-green of exactly the class D1 targets, and it arrived without a
single agent token being spent.

### Immediate next actions, in order

1. **U1** — stage run B's design doc under `/workspace` (its blocker is one directory deep).
2. ~~D1's control arm~~ — **done, and it dissolved D1's premise. See below.**
3. **Author the D1 brief as a file**, with every acceptance criterion mapped to a gate NAME and every figure
   re-measured in that session.
4. **Launch D1** in `agent-rev-m3`; copy the session JSONL out, record the journal watermark and the base
   revision, cap the wall clock.

### D1's control arm: run, and the premise it killed

Script: `~/.hermes/profiles/dev/cache/scratch/control-d1-wasm-staleness.sh` (the cheap path, kept out of the repo).

| measurement | result |
|:--|:--|
| artifact vs source | `crates/wasm/pkg/komun_wasm.js` **2026-09-25 00:09:45**; `crates/wasm/src/lib.rs` **2026-09-30 10:29:19** |
| gap | 469,174 s = **130 hours** |
| what the frontend actually imports | `web/node_modules/komun-wasm/` holds the same **2026-09-25** build (`web/package.json:33` `"komun-wasm": "file:../crates/wasm/pkg"`) |
| control cost | **8 ms**, exit 1, output byte-identical over 3 runs (sha `c93333218fff9101`), **zero tokens** |
| the repo's own gates | `cargo test/clippy` compile sources and never `pkg/`; `fmt` is formatting only; `webtest` imports the artifact |

**Then the premise fell.** The only source change to `lib.rs` since that build is `e6bf44d` (run 008). Run 008
recorded it as a reformat and the diff looks like a massacre — **224 insertions, 2062 deletions**, the file going
2359 → 521 lines. Measured:

- functions: **13 before, 13 after, zero names removed**;
- comment lines: **40 before, 40 after**;
- token multiset: **2366 distinct tokens on both sides, zero count differences**; the 2048-entry recovery
  wordlist is identical entry for entry.

So the change is a **pure rustfmt reflow** — reordered `use` lists, joined signatures, reflowed method chains,
and the one-word-per-line wordlist repacked. The "stale" artifact is **behaviourally current**, and run 008's
carried finding is true of mtime and false of behaviour.

**Therefore D1 is re-scoped.** There is no false green to catch: the artifact corresponds to the code. And an
mtime-based staleness gate is a **permanent false positive** on this repo — precisely the "red forever, so
everyone learns to ignore it" failure the skill warns about. D1's real content is now a design problem with a
falsifiable criterion: *a staleness check whose signal is content, not mtime, and which does not fire on a
reformat* — proven by feeding it the pre- and post-`e6bf44d` sources and requiring the same verdict.

That is a materially different, and much more honest, card than the one §4 proposed. It is also now small
enough that the conversion loop may be the right answer for it, not a full orchestrated run — which is itself a
result the experiment should record.

Nothing else has been changed. Every figure above was measured in this session; the commands that produced
them are in §1's evidence column.

## Provenance of this copy

Copied verbatim from `.hermes/plans/2026-09-30_134155-komun-pipeline-validation.md` on 2026-10-01 for
the capstone package. `.hermes/` is gitignored (`.gitignore:14`), so the original cannot travel with the
repository. This copy holds the same 299 lines at the same numbers, and nothing above this heading was
edited, em dashes included.

An earlier version of this note claimed that `docs/DOC-STYLE.md` forbids em dashes in repository prose.
That claim was false and has been removed. The rule set in that file is R1 to R5 and none of them
concerns dashes, and the repository's own documents use them freely: `docs/iteration-log.md` 183 times,
`docs/governance-policy.md` 49 times, `AGENTS.md` 14 times.
