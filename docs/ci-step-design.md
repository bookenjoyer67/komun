# CI step design

## Which pipeline does this document describe, and where does its rule set come from?

This document records one design entry per step of `.github/workflows/ci.yml`, the Module 4.2
governed pipeline. The workflow holds seven jobs. Five are the lesson's, keyed exactly
`change-type-check`, `policy-gate`, `eval-gate`, `advisory-review` and `audit-trail`
(`.github/workflows/ci.yml` `needs: [change-type-check, policy-gate, web-gate, console-gate,
eval-gate, advisory-review]`); two are additions this repository makes to the lesson's table, and
they are indexed in their own section below. Each entry covers one workflow step: what it does,
what it consumes, what it produces, whether it can fail the build, its time limit, and the
credentials it receives.

Two style standards meet in this file, and they disagree on one point.

- Follow the lesson's step template for every step entry (`docs/ci-step-design.md` `## Step: <name>`
  with the six `- Does:` … `- Credentials:` lines), because lesson 4.2 block [56] fixes that shape.
- Open every other section with the question it answers, per `docs/DOC-STYLE.md` R1.

Gating and advisory mean one thing each in this document.

- Read "gating" as: a failure of this step stops the run. The lesson defines the behaviour (lesson
  4.2 block [53] `A gating step runs, and if it fails, the pipeline stops and the merge is blocked`).
- Read "advisory" as: the step cannot fail the run, whatever it produces. The lesson defines it
  (lesson 4.2 block [53] `An advisory step runs, produces its output, and lets the pipeline continue
  regardless of the output`).

Three jobs can fail the build: `change-type-check`, `policy-gate` and `eval-gate`. Two cannot, because
each carries `continue-on-error: true` at the job level (`.github/workflows/ci.yml`
`continue-on-error: true`).

## Which steps does each job contain, and how is each one classified?

The table indexes every step entry in this document, in workflow order.

| Job | Step | Classification |
|---|---|---|
| `change-type-check` | Checkout the pull request | gating |
| `change-type-check` | Classify the pull request's changed files | gating, fail-closed |
| `change-type-check` | Upload the change classification | advisory |
| `policy-gate` | Checkout the pull request | gating |
| `policy-gate` | Resolve the portability config | gating |
| `policy-gate` | Build the sandbox image | gating |
| `policy-gate` | Run policy tests | gating |
| `policy-gate` | Upload policy report | advisory |
| `eval-gate` | Checkout the pull request | gating |
| `eval-gate` | Resolve the portability config | gating |
| `eval-gate` | Build the sandbox image | gating |
| `eval-gate` | Restore the cargo caches | advisory |
| `eval-gate` | Verify the pull request's base revision is present | gating |
| `eval-gate` | Run the deterministic gates through the gate server | gating |
| `eval-gate` | Check out the base revision for comparison | gating |
| `eval-gate` | Format check scoped to the changed files | gating |
| `eval-gate` | Run the retrieval ground-truth harness | gating |
| `eval-gate` | Write the retrieval report the audit trail consumes | advisory |
| `eval-gate` | Upload the deterministic gate reports | advisory |
| `eval-gate` | Upload the retrieval harness report | advisory |
| `advisory-review` | Checkout the pull request | advisory |
| `advisory-review` | Run the advisory reviewer | advisory |
| `advisory-review` | Post the review summary as a pull request comment | advisory |
| `advisory-review` | Upload the review report and its audit log | advisory |
| `audit-trail` | Checkout the pull request | advisory |
| `audit-trail` | Download every earlier artifact into one directory | advisory |
| `audit-trail` | Build the audit trail | advisory |
| `audit-trail` | Upload the audit trail | advisory |

## Step: Checkout the pull request (change-type-check)

- Does: Fetch the head revision with full history, so the pull request's base commit is present for
  the diff.
- Input: the repository at the merging commit (`.github/workflows/ci.yml` `fetch-depth: 0`).
- Produces: a working tree holding both revisions; no artifact.
- Classification: gating (a failed checkout stops the job; the step judges no code).
- Time limit: 10 minutes (`.github/workflows/ci.yml` `timeout-minutes: 10`).
- Credentials: none (the default token reads the public repository).

## Step: Classify the pull request's changed files (change-type-check)

- Does: Run the classifier over the diff, which writes the two lesson flags and the classification
  JSON.
- Input: the changed paths from a two-revision diff (`.github/workflows/ci.yml` `git diff --name-only
  "$BASE_SHA" "$HEAD_SHA" > ci-artifacts/changed-files.txt`).
- Produces: `ci-artifacts/change-classification.json` and four job outputs (`.github/workflows/ci.yml`
  `requires-governed-check: ${{ steps.classify.outputs.requires-governed-check }}`).
- Classification: gating, fail-closed (an unclassifiable pull request leaves the eval gate without
  its condition, which stops the run instead of opening it).
- Time limit: 10 minutes (`.github/workflows/ci.yml` `timeout-minutes: 10`).
- Credentials: none (`scripts/classify-change.py` `No model call, no network, no GitHub.`).

## Step: Upload the change classification (change-type-check)

- Does: Upload the classification JSON and the changed-file list as one artifact.
- Input: `ci-artifacts/change-classification.json` and `ci-artifacts/changed-files.txt`.
- Produces: the `change-classification` artifact for 90 days, under the name the audit trail reads
  (`scripts/build-audit-trail.py:86` `CLASSIFICATION = "change-classification.json"`).
- Classification: advisory (`if: always()` with `if-no-files-found: ignore`).
- Time limit: inside the job's 10 minutes (`.github/workflows/ci.yml` `timeout-minutes: 10`).
- Credentials: none.

## Step: Checkout the pull request (policy-gate)

- Does: Fetch the head revision the policy suite reads.
- Input: the repository at the merging commit (`.github/workflows/ci.yml` `- uses: actions/checkout@v4`).
- Produces: a working tree; no artifact.
- Classification: gating (a failed checkout stops the job).
- Time limit: 30 minutes (`.github/workflows/ci.yml` `timeout-minutes: 30`).
- Credentials: none.

## Step: Build the sandbox image (policy-gate)

- Does: Build the two images the gate runs in, the Rust base first and the Module 3 image second.
- Input: the checked-out repository and the base image tag (`sandbox/Dockerfile.m3:25`
  `ARG BASE=agent-sandbox:komun`).
- Produces: the local image `agent-sandbox:komun-m3` (`sandbox/Dockerfile.m3`
  `docker build -f sandbox/Dockerfile.m3 -t agent-sandbox:komun-m3 .`).
- Classification: gating (a failed build stops the gate before any check runs).
- Time limit: 30 minutes (`.github/workflows/ci.yml` `timeout-minutes: 30`).
- Credentials: none (the build reads public registries).

This step adapts the lesson's build line: lesson 4.2 block [93] names
`docker build -t agentic_engineer_4 .`, and this repository's equivalent image is the sandbox pair
(`docker build -t agent-sandbox:komun .` then the Module 3 file). The adaptation is named here and in
the entry for the same step under `eval-gate`.

## Step: Run policy tests (policy-gate)

- Does: Run the two policy suites the gate table's `policy` gate names (`eval/test_policy.py` and
  `eval/test_deterministic_step.py`) inside the sandbox image, with the workspace read-only and a
  report directory mounted writable.
- Input: the repository mounted read-only at `/workspace` (`.github/workflows/ci.yml` `-v
  "$GITHUB_WORKSPACE":/workspace:ro`).
- Produces: `ci-artifacts/policy-report.json` (`.github/workflows/ci.yml`
  `--json-report-file=/reports/policy-report.json`).
- Classification: gating, and never advisory (lesson 4.2 block [89] `Policy gates should run before
  agent steps and should never be advisory or demotable.`).
- Time limit: 30 minutes (`.github/workflows/ci.yml` `timeout-minutes: 30`); the two suites together
  finished in `19.56s` against this tree on 2026-10-05.
- Credentials: none (`eval/test_policy.py` reads repository files only).

The step keeps the lesson's YAML verbatim (lesson 4.2 block [93] `-v "$GITHUB_WORKSPACE":/workspace:ro`,
`-w /workspace`, `python -m pytest eval/test_policy.py -v`, `--json-report`) with one added line: the
JSON report plugin is installed inside the container when it is absent, because the image's
requirements do not list it (`sandbox/requirements-m3.txt` names `pytest` and no reporter plugin). The
guarded install is what makes the verbatim pytest line run at all here.

The report flag comes from a pinned dependency rather than a run-time install:
`sandbox/requirements-m3.txt` carries `pytest-json-report==1.5.0`, so this step runs the lesson's
command with the second suite added. Two measurements, both in `agent-sandbox:komun-m3`:
`eval/test_policy.py` alone reported `80 passed, 1 warning in 0.48s` in the workflow's first run,
and the two suites together reported `119 passed in 19.56s` on 2026-10-05. An earlier revision of
this document recorded `75 passed`, which was the single suite before the validator suite was added
to it; the number moves with the tests, so quote the command beside it.

## Step: Upload policy report (policy-gate)

- Does: Upload the policy report whether the suite passed or failed.
- Input: `ci-artifacts/policy-report.json`.
- Produces: the `policy-report` artifact for 90 days (`.github/workflows/ci.yml` `retention-days: 90`).
- Classification: advisory (`if: always()` with `if-no-files-found: ignore`, per lesson 4.2 block
  [93]).
- Time limit: inside the job's 30 minutes (`.github/workflows/ci.yml` `timeout-minutes: 30`).
- Credentials: none.

## Step: Checkout the pull request (eval-gate)

- Does: Fetch the head revision the gates and the harness read.
- Input: the repository at the merging commit (`.github/workflows/ci.yml` `- uses: actions/checkout@v4`).
- Produces: a working tree; no artifact.
- Classification: gating (a failed checkout stops the job).
- Time limit: 60 minutes (`.github/workflows/ci.yml` `timeout-minutes: 60`).
- Credentials: none.

## Step: Build the sandbox image (eval-gate)

- Does: Build the same two images `policy-gate` builds, because each job starts on a fresh runner.
- Input: the checked-out repository and the base image tag (`sandbox/Dockerfile.m3:25`
  `ARG BASE=agent-sandbox:komun`).
- Produces: the local image `agent-sandbox:komun-m3`.
- Classification: gating (a failed build stops the gate before any check runs).
- Time limit: 60 minutes (`.github/workflows/ci.yml` `timeout-minutes: 60`).
- Credentials: none (the build reads public registries).

## Step: Restore the cargo caches (eval-gate)

- Does: Restore the compiler caches into `$RUNNER_TEMP`, where the container binds them writable.
- Input: the cache key (`.github/workflows/ci.yml` `key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}`).
- Produces: `${{ runner.temp }}/cargo-target` and `${{ runner.temp }}/cargo-registry`.
- Classification: advisory (a cache miss costs minutes and changes no verdict).
- Time limit: inside the job's 60 minutes (`.github/workflows/ci.yml` `timeout-minutes: 60`).
- Credentials: none (the Actions cache reads no secret).

The caches live outside the checkout because the checkout is mounted read-only apart from the two
files the guards touch, and the image's own default target path is not usable there (`Dockerfile:90`
`ENV CARGO_TARGET_DIR=/workspace/target`).

## Step: Run the deterministic gates through the gate server (eval-gate)

- Does: Load `mcp/gate/server.py` inside the container and call its `run_gate` tool for `test`,
  `clippy`, `fmt` and `conformance`.
- Input: the repository read-only at `/workspace`, plus one writable bind per file a cache-hit guard
  touches (`.github/workflows/ci.yml:199` `-v "$GITHUB_WORKSPACE/crates/server/src/main.rs":${{ env.AGENT_WORKSPACE }}/crates/server/src/main.rs \`,
  `.github/workflows/ci.yml:200` `-v "$GITHUB_WORKSPACE/crates/core/src/tests.rs":${{ env.AGENT_WORKSPACE }}/crates/core/src/tests.rs \`).
- Produces: `ci-artifacts/deterministic-report.json` and `ci-artifacts/gate-audit.log`, one journal
  line per executed gate (`mcp/gate/server.py:710` `def run_gate(`).
- Classification: gating (deterministic; lesson 4.2 block [53] `Deterministic checks can gate
  immediately because they produce the same result for the same input.`).
- Time limit: 1800 seconds per gate (`.github/workflows/ci.yml` `gate.run_gate(name, "ci", 1800)`),
  inside the job's 60 minutes (`.github/workflows/ci.yml` `timeout-minutes: 60`).
- Credentials: none.

The gate argv is never re-typed here: the server reads the three tuples from the config
(`agentic.config.json:20-24` `"argv": [ "cargo", "test", "--workspace" ],`,
`agentic.config.json:31-39` `"argv": [ "cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings" ],`, `agentic.config.json:51-55` `"argv": [ "cargo", "fmt", "--check" ],`).
The conformance gate compares the working tree against the revision named in `CONFORMANCE_BASE_REF`,
which this step sets to the pull request's base commit (`.github/workflows/ci.yml` `-e
CONFORMANCE_BASE_REF=${{ github.event.pull_request.base.sha }}`). Without it the gate compares the
checkout against itself: a CI working tree *is* its `HEAD`, so the gate would pass on any change and
would be a check that cannot fail. The step before it refuses to run when that base commit is not in
the checkout, and the gate itself refuses a base that names no commit rather than reporting every file
as having no baseline.

`policy` is not called here: it is the `policy-gate` job's whole purpose, and calling it twice would
run the same suite twice for no verdict. Two writable binds exist because each cache-hit guard writes an mtime on its own file
(`mcp/gate/server.py:459` `os.utime(touch_file, None)`) and then requires its own status line: `Compiling komun-core` for
`test` (`agentic.config.json:26` `"marker": "Compiling komun-core"`) and `Checking komun-server` for `clippy` (`agentic.config.json:51` `"marker": "Checking komun-server",`).

Repo-wide formatting is recorded but never tallied. This tree carries rustfmt drift that the
change under test did not cause, and the iteration log measures it (`docs/iteration-log.md` `213 hunks
across 35 files, pre-existing at HEAD and unrelated to this change`). A repo-wide fmt verdict would
therefore fail every pull request, so the scoped step below carries the fmt gate and the repo-wide
result stays in `ci-artifacts/deterministic-report.json` under `gates.fmt`.

## Step: Check out the base revision for comparison (eval-gate)

- Does: Check out the pull request's base commit into a second directory, so the formatting check can
  read each changed file as it stood before the change.
- Input: `${{ github.event.pull_request.base.sha }}` (`.github/workflows/ci.yml` `ref:
  ${{ github.event.pull_request.base.sha }}`).
- Produces: `$GITHUB_WORKSPACE/base`, mounted read-only into the next step.
- Classification: gating (a failed checkout stops the job, and the step judges no code).
- Time limit: inside the job's 60 minutes (`.github/workflows/ci.yml` `timeout-minutes: 60`).
- Credentials: none.

## Step: Format check scoped to the changed files (eval-gate)

- Does: Compare each changed Rust file's formatting now against its formatting at the base revision.
  It fails in exactly two cases. One case is a file formatted at base and unformatted now. The other is
  a file new in this change that is unformatted.
- Input: the classifier's `changed-files` output, the repository read-only at `/workspace`, and the
  base revision read-only at `/base`.
- Produces: `ci-artifacts/fmt-scope.txt`, one verdict line per file, and a printed summary count.
- Classification: gating (deterministic; lesson 4.2 block [53] `Deterministic checks can gate
  immediately because they produce the same result for the same input.`).
- Time limit: 300 seconds per file (`.github/workflows/ci.yml` `timeout=300`).
- Credentials: none.

The comparison exists because a repo-wide fmt gate is unusable here and a naive scoped gate would be
unfair. Measured on this tree, the two files the Module 3 change touches both report
`pass-drift-carried-from-base`, so the change passes. A control run with the base file formatted and the
current file unformatted reports `fail-new-drift`, so the check can fail. A change touching no Rust file
prints `SCOPED_FMT result=skip reason=no-rust-file-in-this-change` and passes.

## Step: Run the retrieval ground-truth harness (eval-gate)

- Does: Start the retrieval server inside the container, wait for its index line, then run the
  ground-truth harness against it.
- Input: `docs/retrieval-ground-truth.md` and the corpus under `.memory/reference`
  (`.github/workflows/ci.yml` `-e RETRIEVAL_EMBEDDING_MODEL=BAAI/bge-small-en-v1.5`).
- Produces: `ci-artifacts/retrieval-ground-truth.log`; measured locally as `pass rate: 8/8 (100.0%)
  against the 80% floor`.
- Classification: gating (the harness exit status is the floor check, `mcp/retrieval/run_ground_truth.py:29`
  `PASS_FLOOR = 0.80`).
- Time limit: inside the job's 60 minutes; the readiness wait is bounded at 150 seconds
  (`.github/workflows/ci.yml` `for attempt in $(seq 1 150); do`).
- Credentials: none (the embedding model is baked into the image, `sandbox/Dockerfile.m3`
  `baked in:`).

The two model variables are not optional. The same harness over the same corpus reports
`5/8 (62.5%)` without them and `8/8 (100.0%)` with them, which is the comparison the retrieval quality
report records (`docs/retrieval-quality-report.md` `HARNESS_RESULT passed=8 total=8 rate=100.0
floor=80.0`).

## Step: Write the retrieval report the audit trail consumes (eval-gate)

- Does: Convert the harness's result line into the JSON report shape the audit trail reads.
- Input: `ci-artifacts/retrieval-ground-truth.log` (`mcp/retrieval/run_ground_truth.py` prints
  `HARNESS_RESULT passed=8 total=8 rate=100.0 floor=80.0`).
- Produces: `ci-artifacts/retrieval-report.json` with a nested summary object
  (`scripts/build-audit-trail.py` `summary = report.get("summary")`).
- Classification: advisory (the floor is enforced by the harness step, so this step formats evidence
  only).
- Time limit: seconds, inside the job's 60 minutes (`.github/workflows/ci.yml` `timeout-minutes: 60`).
- Credentials: none.

## Step: Upload the deterministic gate reports (eval-gate)

- Does: Upload the three gate verdicts and the gate journal whether the gates passed or failed.
- Input: `ci-artifacts/deterministic-report.json` and `ci-artifacts/gate-audit.log`.
- Produces: the `deterministic-report` artifact for 90 days (`.github/workflows/ci.yml`
  `retention-days: 90`).
- Classification: advisory (`if: always()`, lesson 4.2 block [134] `Deterministic and rubric reports
  upload with if: always() and if-no-files-found: ignore`).
- Time limit: inside the job's 60 minutes (`.github/workflows/ci.yml` `timeout-minutes: 60`).
- Credentials: none.

## Step: Upload the retrieval harness report (eval-gate)

- Does: Upload the harness log, its JSON report, the retrieval audit journal and the server log.
- Input: `ci-artifacts/retrieval-report.json`, `ci-artifacts/retrieval-ground-truth.log`,
  `ci-artifacts/retrieval-audit.log` and `ci-artifacts/retrieval-server.log`.
- Produces: the `retrieval-report` artifact for 90 days (`.github/workflows/ci.yml`
  `retention-days: 90`).
- Classification: advisory (`if: always()` with `if-no-files-found: ignore`).
- Time limit: inside the job's 60 minutes (`.github/workflows/ci.yml` `timeout-minutes: 60`).
- Credentials: none.

## Step: Checkout the pull request (advisory-review)

- Does: Fetch the revision the reviewer reads its changed files from.
- Input: the repository at the merging commit (`.github/workflows/ci.yml` `- uses: actions/checkout@v4`).
- Produces: a working tree; no artifact.
- Classification: advisory (the whole job is marked `continue-on-error: true`).
- Time limit: 15 minutes (`.github/workflows/ci.yml` `timeout-minutes: 15`).
- Credentials: none (the reviewer reads its one key on a later step).

## Step: Run the advisory reviewer (advisory-review)

- Does: Run `scripts/run-reviewer.py` over the classifier's changed-file list and capture its output.
- Input: the classifier's file list passed through the environment (`.github/workflows/ci.yml`
  `CHANGED_FILES: ${{ needs.change-type-check.outputs.changed-files }}`).
- Produces: `ci-artifacts/review-report.json`, `ci-artifacts/review-summary.md` and
  `ci-artifacts/audit-review.log` (`scripts/run-reviewer.py:237` `audit_path = out_dir /
  "audit-review.log"`).
- Classification: advisory (the step captures the exit status and exits 0 itself,
  `.github/workflows/ci.yml` `exit 0`).
- Time limit: 15 minutes (`.github/workflows/ci.yml` `timeout-minutes: 15`); the script's own model
  call times out after 120 seconds (`scripts/run-reviewer.py` `DEFAULT_TIMEOUT = 120`).
- Credentials: `OPENROUTER_API_KEY`, scoped to this step only (`.github/workflows/ci.yml`
  `OPENROUTER_API_KEY: ${{ secrets.OPENROUTER_API_KEY }}`).

Two mechanisms keep this step out of the merge decision. The job carries
`continue-on-error: true` (lesson 4.2 block [150] `continue-on-error: true is set on the advisory-review
job`), and the step itself reports the script's status and exits 0. The second mechanism is what
survives a non-zero exit from the script: the script is designed to exit 0 always (lesson 4.2 block
[150] `scripts/run-reviewer.py exits 0 regardless of review severity`), and the step enforces the same
outcome even when it does not.

## Step: Post the review summary as a pull request comment (advisory-review)

- Does: Post `review-summary.md` as a pull request comment, and report rather than fail when it
  cannot.
- Input: `ci-artifacts/review-summary.md` and the pull request number (`.github/workflows/ci.yml`
  `PULL_REQUEST_NUMBER: ${{ github.event.pull_request.number }}`).
- Produces: the pull request comment lesson 4.2 block [56] names as the step's output (`Produces: a
  structured review comment.`).
- Classification: advisory (`if: always()`, with an explicit ignore on the `gh` call,
  `.github/workflows/ci.yml` `|| echo "the review comment could not be posted (advisory,
  ignored)"`).
- Time limit: seconds, inside the job's 15 minutes (`.github/workflows/ci.yml` `timeout-minutes: 15`).
- Credentials: `GITHUB_TOKEN`, scoped to this step only (`.github/workflows/ci.yml` `GH_TOKEN: ${{
  secrets.GITHUB_TOKEN }}`).

`pull-requests: write` is granted to this job for this one step, which is the only consumer of that
permission (`.github/workflows/ci.yml` `permissions:` with `pull-requests: write`). A fork pull
request receives a read-only token, so the comment fails there and the step reports it; the artifact
and not the comment is the durable output.

## Step: Upload the review report and its audit log (advisory-review)

- Does: Upload the review report, the summary, the step's audit journal and the captured output.
- Input: `ci-artifacts/review-report.json`, `ci-artifacts/review-summary.md`,
  `ci-artifacts/audit-review.log` and `ci-artifacts/review-output.txt`.
- Produces: the `advisory-review-report` artifact for 90 days (`.github/workflows/ci.yml`
  `retention-days: 90`).
- Classification: advisory (`if: always()` with `if-no-files-found: ignore`, lesson 4.2 block [150]
  `The review job's MCP audit log was uploaded as an artifact`).
- Time limit: inside the job's 15 minutes (`.github/workflows/ci.yml` `timeout-minutes: 15`).
- Credentials: none.

## Step: Checkout the pull request (audit-trail)

- Does: Fetch the revision the trail names in its metadata.
- Input: the repository at the merging commit (`.github/workflows/ci.yml` `- uses: actions/checkout@v4`).
- Produces: a working tree; no artifact.
- Classification: advisory (the job is marked `continue-on-error: true`, because the trail decides
  whether code can merge not at all).
- Time limit: 10 minutes (`.github/workflows/ci.yml` `timeout-minutes: 10`).
- Credentials: none.

## Step: Download every earlier artifact into one directory (audit-trail)

- Does: Download every artifact of the run into one flat directory.
- Input: the artifacts the four jobs uploaded.
- Produces: `ci-artifacts/` holding `policy-report.json`, `change-classification.json`, the two harness
  reports and the journals side by side (`.github/workflows/ci.yml` `merge-multiple: true`).
- Classification: advisory (`continue-on-error: true` on the job, so a run with no artifacts at all
  still reaches the assembly step).
- Time limit: inside the job's 10 minutes (`.github/workflows/ci.yml` `timeout-minutes: 10`).
- Credentials: none (the run's own artifacts are read with the default token).

This step runs before the assembly step, because the flattening is what the assembly reads (lesson
4.2 `merge-multiple: true flattens them into one directory (ci-artifacts)` so the script finds
`policy-report.json, the harness reports, and the audit-*.log files side by side`).

## Step: Build the audit trail (audit-trail)

- Does: Assemble the run's trail from the downloaded artifacts and the four job results.
- Input: `ci-artifacts/` and the job results (`.github/workflows/ci.yml` `--governed-file-gate
  "$EVAL_GATE_RESULT"`).
- Produces: `ci-artifacts/audit-trail.json` in the lesson's shape (`scripts/build-audit-trail.py`
  `out_path = Path(args.out) if args.out else root / "audit-trail.json"`).
- Classification: advisory (the script exits 0 with a trail even when artifacts are missing,
  `scripts/build-audit-trail.py` `A trail with a missing artifact is still a trail`).
- Time limit: inside the job's 10 minutes (`.github/workflows/ci.yml` `timeout-minutes: 10`).
- Credentials: none (the script redacts secret-shaped values out of the trail,
  `scripts/build-audit-trail.py` `text.replace(secret, "[REDACTED]")`).

## Step: Upload the audit trail (audit-trail)

- Does: Upload the assembled trail as the run's combined artifact.
- Input: `ci-artifacts/audit-trail.json`.
- Produces: the `audit-trail` artifact for 90 days (`.github/workflows/ci.yml` `retention-days: 90`).
- Classification: advisory (`if: always()` with `if-no-files-found: ignore`).
- Time limit: inside the job's 10 minutes (`.github/workflows/ci.yml` `timeout-minutes: 10`).
- Credentials: none.

## Which jobs does this repository add to the lesson's table, and what does each step do?

Three jobs are additions rather than rows of the lesson's table, and their per-step design lives in
the workflow's own comments. They are recorded here in one place because the table above indexes
the lesson's five jobs step by step, and duplicating that shape for every added step would bury
the mapping it exists to show.

`web-gate` (added for the two frontend gates the seam table already names,
`agentic.config.json` `toolchain.commands.webcheck`): checkout, resolve the portability config,
build the wasm package `web/package.json` imports, set up Node 22.23.2, `npm ci`, assert the two
commands still match the gate table, `npm --prefix web run check`, `npm --prefix web run test`.
Every step gates: a deterministic check gates immediately, and a broken frontend is exactly the
failure the job exists to catch. It receives no credential and no secret. Its wasm step is not
optional: the artifact is gitignored, and measured without it `svelte-check found 2 errors`
("Cannot find module 'komun-wasm'").

`console-gate` (added because `console/.github/workflows/ci.yml` cannot run in this repository:
GitHub reads workflows only from the repository root, and the crate is its own workspace root, so
`cargo test --workspace` never sees it): checkout, install the Rust toolchain, restore the Rust
cache, `cargo fmt --check`, `cargo clippy --release --all-targets -- -D warnings`, `cargo test
--release`. The last three gate; the toolchain and cache steps cannot fail a build on their own.
No credential is involved.

`database-gate` (added for the ignored set, which needs a live Postgres the sandbox image cannot
reach): checkout, install the Rust toolchain, restore the Rust cache, then `cargo test -p
komun-server -- --ignored --test-threads=1` with `KOMUN_TEST_DATABASE_URL` built into the step's
environment, against a disposable `postgres:16-alpine` service container with a readiness check.
The test step gates. The password is a literal because the database is disposable and reachable
only on the runner's loopback, and the URL is never echoed.

## How does the workflow classify a change as deterministic work or agentic work?

The classifier separates a change that alters what an agent does or may do from a change that alters
neither, and the lesson's two flags carry that answer.

- Set `requires-governed-check` when any changed path matches the governed globs
  (`scripts/classify-change.py` `GOVERNED_GLOBS: tuple[str, ...] = (`).
- Set `touches-policy` when any changed path matches the narrower permission set
  (`scripts/classify-change.py` `POLICY_GLOBS: tuple[str, ...] = (`).
- Read the lesson's criterion for the governed set (lesson 4.2 block [123] `directly change what
  agents do or what they are allowed to do`).
- Read the lesson's criterion for the second flag (lesson 4.2 block [123] `Some files are especially
  sensitive because they control permissions and access boundaries.`).

The lesson names the course repository's paths and this repository carries the same classes at
different paths, so the mapping is a path translation rather than a new rule.

| Lesson path | Path here | Flag |
|---|---|---|
| `.agents/` | `.claude/agents/**` (`scripts/classify-change.py` `".claude/agents/**"`) | `requires-governed-check` |
| `.skills/` | `.claude/skills/**` (`scripts/classify-change.py` `".claude/skills/**"`) | `requires-governed-check` |
| `mcp-servers/` | `mcp/**` (`scripts/classify-change.py` `"mcp/**"`) | `requires-governed-check` |
| `eval/` | `eval/**` (`scripts/classify-change.py` `"eval/**"`) | `requires-governed-check` |
| `docs/governance-policy.md` | `docs/governance-policy.md` (`scripts/classify-change.py` `"docs/governance-policy.md"`) | both flags |
| — (this repo's enforcement layer) | `scripts/run-agent.sh` (`scripts/classify-change.py` `"scripts/run-agent.sh"`) | both flags |
| — (this repo's gates) | `.github/workflows/**` (`scripts/classify-change.py` `".github/workflows/**"`) | both flags |

The gating consequences follow the lesson's rule that deterministic checks may gate at once (lesson
4.2 block [53] `Deterministic checks can gate immediately because they produce the same result for the
same input.`).

- Run `eval-gate` only when `requires-governed-check` is true (`.github/workflows/ci.yml` `if:
  needs.change-type-check.outputs.requires-governed-check == 'true'`).
- Run `policy-gate` on every pull request, because the lesson's table row says it runs every run
  (lesson 4.2 block [16] `Policy Test Suite` with `Every run`).
- Keep `advisory-review` off the flags, because the lesson's own worked pull request changes a
  non-governed file and still expects a review (lesson 4.2 block [167] `"requires_governed_check":
  false` with `"advisory_review": "success"`).

Two facts about the four outputs are worth stating plainly.

- Consume `requires-governed-check` in the `eval-gate` condition, as the lesson's acceptance check
  requires (lesson 4.2 block [134] `You confirmed the eval gate runs on an agent-affecting change—and
  skips on a docs-only change.`).
- Consume `touches-policy` in the audit trail rather than in a job condition, because every
  policy-control file is already agent-affecting (`scripts/classify-change.py` `governed = governed
  or policy`).

## Which gate fails against the current tree, and why is the repo-wide result not tallied?

No deterministic gate is red against the tree the workflow was last measured on, and the earlier
version of this section was wrong to say one was. Re-measured on 2026-10-05, at the revision this
document is committed with:

- `cargo fmt --check` exits 0 (`cargo fmt --check | grep -c "^Diff in"` -> `0`), and the gate's own row
  in the workflow's first run reads `GATE fmt argv=['cargo', 'fmt', '--check'] exit=0 passed=True
  guard_applied=False guard_satisfied=True verdict=pass` with `hunks 0`. The formatting drift this
  section used to describe (`213 hunks`, `DETERMINISTIC_RESULT passed=2 failed=1 total=3`) was repaired
  by the comment-trim commits that followed it, and the section outlived its truth.
- `cargo test --workspace` passes; the first run reported `184 passed; 0 failed; 36 ignored` in the
  server crate. The ignored count is the subject of its own question below.
- `cargo clippy --release --all-targets -- -D warnings` passes with both guards satisfied
  (`GATE clippy argv=['cargo', 'clippy', '--release', '--all-targets', '--', '-D', 'warnings'] exit=0
  passed=True guard_applied=True guard_satisfied=True`).
- The repo-wide `fmt` result stays recorded and never tallied, because it would judge the whole
  repository on every pull request; the scoped step below carries the formatting gate, and it judges
  each changed file against its own revision at the base commit.
- The three tallied gates are `test`, `clippy` and `conformance`
  (`DETERMINISTIC_RESULT passed=3 failed=0 total=3` once this branch lands), with `fmt` recorded
  separately as `fmt_recorded_only`. Keep the format gate gating on the scoped comparison, because the
  lesson gates deterministic checks on first sight (lesson 4.2 block [53] `Deterministic checks can
  gate immediately`).

## Which repository settings does the pipeline depend on, and which of them live outside this file?

Four settings live in the repository rather than in the workflow, and the pipeline is incomplete
without them.

- Mark the `Policy Test Suite` check required for `main`, because an unreported check is advisory in
  effect (lesson 4.2 block [102] `Until a check is required, it is advisory in effect no matter what
  your YAML says.`).
- Mark the `Change Classifier` check required as well, because `eval-gate` keys off its output and a
  skipped classifier would leave the eval gate unanswered.
- Register `OPENROUTER_API_KEY` in the repository secrets store, because the reviewer step reads it
  by that name and no other source (`scripts/run-reviewer.py:72` `API_KEY_ENV = "OPENROUTER_API_KEY"`).
- Rotate any Module 3 MCP credential through the secrets store rather than through this file
  (lesson 4.2 block [72] `Rotatable without pipeline changes`).

One interaction between required checks and the skip rule is unresolved here. A skipped job reports
neither success nor failure, and how branch protection treats a skipped required check is a GitHub
setting rather than a workflow property.

## How does the workflow hold the lesson's four secrets requirements?

One secret reaches one step, in two places in the whole workflow, and each of the four requirements
names the literal that holds it.

1. "Injected at runtime: The value lives only in the secret store." The workflow references the
   secret store and writes no key into any file (`.github/workflows/ci.yml` `OPENROUTER_API_KEY: ${{
   secrets.OPENROUTER_API_KEY }}`).
2. "Never logged: ... Treat masking as a backstop rather than a guarantee." No step runs `printenv`,
   `env` or any command that dumps the environment, and no step echoes a key
   (`.github/workflows/ci.yml` `echo "run-reviewer.py exit status: $status`).
3. "Scoped to the step: Inject a secret only into the steps that actually use it, not onto the whole
   job." Each secret sits in the `env:` block of its one consuming step, never on a job
   (`.github/workflows/ci.yml` `GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}` on the comment step alone).
4. "Rotatable without pipeline changes: ... you must be able to replace it by updating the secrets
   store on your own." Both secrets are referenced by name only, so a rotation edits no file here
   (`scripts/run-reviewer.py:72` `API_KEY_ENV = "OPENROUTER_API_KEY"`).

The reviewer step receives one credential and the comment step receives the other, so no step holds
both. The reviewer script writes no key and no key length into its report, its summary or its audit
journal (`scripts/run-reviewer.py` `_redact()` scrubs secret-shaped values from everything it writes).

## Which tests does the workspace gate run, and which does it never run?

Two different sets, run in two different jobs, and the distinction is worth keeping straight.

The `test` gate inside the sandbox image runs every test that needs no database, and reports the rest
as ignored rather than passing them. The first run's own line reads `test result: ok. 184 passed; 0
failed; 36 ignored; 0 measured; 0 filtered out` for the server crate. Its argv is unchanged
(`cargo test --workspace`), so that line stays what it is.

The `Database Suite` job -- added for this -- runs exactly the ignored set, in the place the tests can
actually reach a database. They cannot run in the sandbox image: they need a live Postgres, and the
image's route to one is what an earlier probe failed to find. So the job runs them on the runner
against a disposable `postgres:16-alpine` service container, with the command the suites' own
documentation gives:

    KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server -- --ignored --test-threads=1

Three files carry `#[ignore]` attributes at the revision this document is committed with --
`key_coherence.rs` (27), `key_change.rs` (10) and `outbound_routes.rs` (2), 39 attributes selecting 36
ignored tests (measured locally against `postgres:16-alpine`: `36 passed; 0 failed; 0 ignored; 184
filtered out`, 19.77s). They are the suites closest to the security audit: stored-key coherence, key
replacement and outbound fetch. The count is a property of the revision, not of the pipeline: a branch
that adds a database-backed suite changes it, which is why each figure above is stated with the
revision or the command that produced it.

Two consequences are worth stating plainly, because a green `Evaluation Harness` does not state them:

- The evaluation harness still does not prove the database behaviour. The `Database Suite` job does,
  and it gates: a red there blocks the merge the same way a red `test` gate does.
- The one-pager's `0 ignored` figure (it quotes the workspace gate at `158 passed, 0 failed, 0
  ignored`) is a measurement from a revision that predates these suites, not a property of the
  pipeline.

What is still an operator step is any filter narrower than the whole ignored set, and any run against a
database the workflow does not start. The job starts its own, and the sandbox remains unable to reach
one.

## What remains unverified after the workflow's first real run, and what did that run settle?

The pipeline has run on GitHub Actions. Run `37259644200` (pull request, `security/consolidated-audit-stack`,
conclusion `success`, 6m23s) settles most of what this section used to list as unverified, and the artifacts
it uploaded are the evidence:

- GitHub's parser accepts the workflow, and every `uses:` resolves: the run's five jobs each reached
  `completed`.
- The artifact round trip is real, not emulated: `Audit Trail` ran `merge-multiple: true` and assembled
  a trail from nine artifacts (`artifacts_consumed` in `audit-trail.json`).
- The sandbox image builds on a hosted runner: `Policy Test Suite` built both images and finished in
  86s.
- The cargo cache binds work under Actions: `Evaluation Harness` restored the caches, ran the gates with
  `guard_applied=True, guard_satisfied=True` for both guarded gates, and finished in 238s.
- `cargo fmt --check` is clean on a hosted runner, and the scoped formatting gate judged 45 changed
  Rust files with zero regressions.
- The retrieval floor holds in CI with the two model variables set: `HARNESS_RESULT passed=8 total=8
  rate=100.0 floor=80.0`.

Four things stay unverified, and each has an owner:

- The reviewer's model call, because no `OPENROUTER_API_KEY` is in the repository secrets: the artifact
  records `"status": "not_run"` and `"api_key_present": false`. Register the secret and re-run one pull
  request to settle it.
- The pull request comment path on a fork, because a fork's token is read-only. The same run shows the
  non-fork path works: `github-actions[bot]` posted the summary comment.
- Whether a skipped required check satisfies branch protection, because `main` carries no protection
  rule at all (`Branch not protected`). The skip path exists to serve the lesson's docs-only case
  (lesson 4.2 block [134] `skips on a docs-only change`).
- Whether the added gates are green, because none of them existed when the run above was taken:
  `web-gate`, `console-gate` and `database-gate` each settle themselves on their first green run, and
  the `Database Suite` job also settles whether this repository's database suites pass outside an
  operator's shell.
