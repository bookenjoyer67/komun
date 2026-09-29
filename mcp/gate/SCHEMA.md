# Quality-Gate MCP Server Schema

Which server, endpoint, and runtime files does this document describe?

The server is named `gate` (`mcp/gate/server.py:86` `mcp = FastMCP("gate")`), and it serves streamable
HTTP at `http://localhost:8003/mcp` (a live `initialize` returned `HTTP/1.1 200 OK` with
`"serverInfo":{"name":"gate","version":"3.4.7"}`).

It exposes three named operations and no shell, no argv, and no working-directory control:
`list_gates`, `run_gate`, `read_audit_log`
(`mcp/gate/server.py:338`, `:359` and `:379`, one `@mcp.tool` before each of the three definitions).

Runtime file inside the sandbox container:

| File | Default path | Authority |
| --- | --- | --- |
| Audit journal | `/workspace/.memory/gate-audit.log` | `mcp/gate/gate_vocabulary.py:60` `str(Path(MEMORY_DIR) / "gate-audit.log")` |

The path follows `MEMORY_DIR`, which defaults to `/workspace/.memory`, the config's `containers.memory_dir`
(`mcp/gate/gate_vocabulary.py:59` `MEMORY_DIR = os.getenv("MEMORY_DIR", str(agentic_config.get("containers.memory_dir")))`).
Override `GATE_AUDIT_PATH` and `GATE_WORKSPACE` for a local run
(`mcp/gate/gate_vocabulary.py:58` `WORKSPACE = os.getenv("GATE_WORKSPACE", str(agentic_config.get("containers.workspace")))`).

Which command starts the server, and how is it registered?

```bash
python3 mcp/gate/server.py --port 8003 --host 0.0.0.0
```

```bash
claude mcp add --transport http gate http://localhost:8003/mcp
```

`--port` defaults to `8003` (`mcp/gate/server.py:407` `parser.add_argument("--port", type=int, default=8003`),
and `--host` defaults to `0.0.0.0` (`mcp/gate/server.py:408` `default="0.0.0.0"`). The help output
confirms both (`python3 mcp/gate/server.py --help` -> `--port PORT           HTTP port (default 8003)`).

## Which three gates exist, and which command does each one run?

Three gates exist, and the argv of each is a module constant in the `GATES` table
(`mcp/gate/gate_vocabulary.py:94` `GATES: dict[str, dict[str, Any]] = {`).

| Gate | Exact argv | Authority |
| --- | --- | --- |
| `test` | `cargo test --workspace` | `agentic.config.json:20-24` `"argv": [ "cargo", "test", "--workspace" ],` |
| `clippy` | `cargo clippy --release --all-targets -- -D warnings` | `agentic.config.json:29-37` `"argv": [ "cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings" ],` |
| `fmt` | `cargo fmt --check` | `agentic.config.json:47-51` `"argv": [ "cargo", "fmt", "--check" ],` |

These are the project's documented gates (`AGENTS.md:196` `cargo test --workspace`,
`AGENTS.md:197` `cargo clippy --release -- -D warnings`, `.memory/knowledge/coding-standards.md:10`
`` `cargo clippy --release -- -D warnings` must exit clean on every change. ``, and rule 1's cache-hit
note `.memory/knowledge/coding-standards.md:11` `a silent second run is a cache hit, not a clean lint`).

The clippy argv is deliberately stricter than the documented form: it adds `--all-targets`, so test
and example targets are linted too.

`list_gates` publishes the same three tuples (`mcp/gate/server.py:344` `"argv": list(definition["argv"]),`).
A live call returned them verbatim, so the allowlist a caller sees is the allowlist that runs:

```json
{"clippy": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"], "fmt": ["cargo", "fmt", "--check"], "test": ["cargo", "test", "--workspace"]}
```

## Why can a caller not run an arbitrary command?

Because the only thing `run_gate` accepts is a key into `GATES`, and the argv it executes is that
table's tuple (`mcp/gate/server.py:120` `if gate not in GATES:` and
`mcp/gate/server.py:273` `argv = list(definition["argv"])`).

The refusal is raised before anything is executed
(`mcp/gate/server.py:122` `f"refused: '{gate}' is not an allowlisted gate. This server runs only "`). A caller asking for a
free-form command gets this text, captured from a live call:

```
Error calling tool 'run_gate': refused: 'cargo test --workspace' is not an allowlisted gate. This server runs only ['clippy', 'fmt', 'test'] by name; it accepts no command string, no extra arguments and no shell.
```

An argument-passthrough attempt and a shell-injection attempt were refused with the same message:
`gate='test -- --nocapture'` and `gate='test; touch /tmp/gate-selftest-pwned'`, and the injection
target did not exist afterwards (`/tmp/gate-selftest-pwned exists=False`).

Which properties make that airtight?

- `shell` is never enabled and no caller string reaches the command line
  (`mcp/gate/server.py:279` `subprocess.run(  # noqa: S603 - a fixed argv, never a caller-supplied string`
  with `shell` absent, and `mcp/gate/server.py:280` `argv,` as the whole argument list).
- The same tuple is what the result reports and what the journal records, so a reviewer can compare
  the executed argv against the allowlist rather than trusting the tool
  (`mcp/gate/server.py:312` `"argv": argv,`).
- The working directory is the server's own workspace, not a caller value
  (`mcp/gate/server.py:281` `cwd=WORKSPACE,`).
- The gates cannot be widened by a second element: there is no per-call argument list in the tool
  signature at all (`mcp/gate/server.py:360` `def run_gate(gate: str, calling_role: str = "unknown", timeout_seconds: int | None = None) -> dict:`).

## What does `run_gate` accept, and what does it return?

The tool signature is the parameter authority (`mcp/gate/server.py:360` `def run_gate(`).

- Pass `gate` (`str`, required) as one of `test`, `clippy`, `fmt` (`mcp/gate/server.py:360` `gate: str,`).
- Pass `calling_role` (`str`, optional, default `unknown`) so the journal names the caller (`mcp/gate/server.py:360` `calling_role: str = "unknown"`).
- Pass `timeout_seconds` (`int`, optional, default `null` -> 900) as the per-run cap, clamped to 60..3600 (`mcp/gate/server.py:360` `timeout_seconds: int | None = None`).

The returned object carries one key per required fact (`mcp/gate/server.py:310` `return {`):

| Field | Type | Meaning |
| --- | --- | --- |
| `gate` | `str` | The allowlisted name that ran (`mcp/gate/server.py:311` `"gate": gate,`) |
| `argv` | `list[str]` | The exact argv executed (`mcp/gate/server.py:312` `"argv": argv,`) |
| `exit_code` | `int` | Process exit status, `-1` when the run was killed (`mcp/gate/server.py:313` `"exit_code": exit_code,`) |
| `passed` | `bool` | Verdict: exit 0 **and** guard satisfied (`mcp/gate/server.py:298` `passed = exit_code == 0 and bool(guard["satisfied"])`) |
| `verdict` | `str` | `pass`, `fail: exit code N`, or the guard/timeout cause (`mcp/gate/server.py:300` `f"fail: killed after {timeout_seconds}s without finishing"`) |
| `timed_out` | `bool` | Whether the timeout killed the process (`mcp/gate/server.py:291` `timed_out = True`) |
| `timeout_seconds` | `int` | The effective (clamped) timeout (`mcp/gate/server.py:317` `"timeout_seconds": timeout_seconds,`) |
| `duration_seconds` | `float` | Wall-clock seconds, rounded to milliseconds (`mcp/gate/server.py:295` `duration_seconds = round(time.monotonic() - started, 3)`) |
| `guard` | `dict` | The cache-hit guard outcome, see below (`mcp/gate/server.py:319` `"guard": guard,`) |
| `stdout` / `stderr` | `str` | Captured output, ANSI escapes stripped (`mcp/gate/server.py:308` `clamp_output(strip_ansi(raw_stdout))`) |
| `stdout_truncated` / `stderr_truncated` | `bool` | Whether the 200 000-character cap cut the stream (`mcp/gate/server.py:84` `MAX_OUTPUT_CHARS = 200_000`) |
| `output_ansi_stripped` | `bool` | Always `true`, so a caller knows colour codes were removed (`mcp/gate/server.py:324` `"output_ansi_stripped": True,`) |

## How does the clippy cache-hit guard work?

The gate touches a file under test, then requires cargo's status line for that crate in the output.

1. Touch `crates/server/src/main.rs` before running cargo
   (`agentic.config.json:42` `"touch_file": "crates/server/src/main.rs",`,
   `mcp/gate/server.py:215` `os.utime(touch_file, None)`). A missing file is reported as an
   unsatisfied guard rather than ignored (`mcp/gate/server.py:222` `f"cache-hit guard could not touch {touch_file}: {error}"`).
2. Require the `Checking komun-server` marker in the combined output
   (`agentic.config.json:40` `"marker": "Checking komun-server",`,
   `agentic.config.json:41` `"marker_regex": "\\bChecking\\b\\s+(?P<marker>komun-server)\\b",`).
3. Fail the gate when the marker is absent, even at exit 0 (`mcp/gate/server.py:303` `elif not guard["satisfied"]:` with `mcp/gate/server.py:240` `f"cache-hit guard not satisfied: no '{guard['marker']}' line in the output, so a clean "`).

Why does the guard check the stripped output rather than the raw output?

Because this image runs cargo with colour forced on (`CARGO_TERM_COLOR=always` in the container
environment), so a status line is written as `\033[1m\033[92m    Checking\033[0m komun-server`. Byte
evidence from the captured stderr: `Checking` appears 3 times while the literal
`Checking komun-server` appears 0 times, because the escape sequence sits between the two words. The
guard strips SGR sequences first (`mcp/gate/server.py:238` `match = GUARD_MARKER_PATTERN.search(strip_ansi(combined_output))`,
`mcp/gate/server.py:79` `ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")`), and the returned
stdout and stderr are stripped for the same reason.

A live clippy call through the server reported the guard satisfied and pasted its own evidence:

```json
{"gate": "clippy", "argv": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"], "exit_code": 0, "passed": true, "verdict": "pass", "duration_seconds": 2.533, "timed_out": false, "guard": {"applied": true, "satisfied": true, "marker": "Checking komun-server", "touched": "/workspace/crates/server/src/main.rs", "detail": "found 'Checking komun-server' in the cargo output"}}
```

and the returned stderr begins with that line:

```
    Checking komun-server v0.1.0 (/workspace/crates/server)
    Finished `release` profile [optimized] target(s) in 2.67s
```

What does the guard protect against, measured rather than assumed? Two clippy runs with no touch in
between, both over the warm target directory, exited `0` with no `Checking komun-server` line at all
(`run1: exit=0 checking_komun_server=0`, `run2: exit=0 checking_komun_server=0`). A gate that read
exit codes alone would call that a clean lint; the guard makes it unrepresentable.

## What does the per-run timeout do?

Default 900 s (`mcp/gate/server.py:81` `DEFAULT_TIMEOUT_SECONDS = int(os.getenv("GATE_TIMEOUT_SECONDS", "900"))`),
clamped into 60..3600 s (`mcp/gate/server.py:135` `return max(MIN_TIMEOUT_SECONDS, min(MAX_TIMEOUT_SECONDS, timeout_seconds))`).

The expiry kills the process and is reported rather than raised: `exit_code` becomes `-1`,
`timed_out` becomes `true`, and `passed` becomes `false`
(`mcp/gate/server.py:290` `except subprocess.TimeoutExpired as expired:`).

A live call asked for `timeout_seconds=1` and got the floor reported back
(`requested=1s reported=60s timed_out=False exit=1`), which is the clamp working. The kill path
itself is not exercised by the self-test: no gate in this repository runs longer than the 60 s floor,
so there is no honest way to reach it without changing the gates.

## What shape does one audit-journal record have?

Write one JSON object per line, with keys in sorted order
(`mcp/gate/server.py:142` `line = json.dumps(record, sort_keys=True) + "\n"`).

Flush and `fsync` each record so a following `tail -n 1` sees it immediately
(`mcp/gate/server.py:146` `os.fsync(handle.fileno())`).

| Key | Meaning |
| --- | --- |
| `timestamp` | ISO-8601 UTC instant (`mcp/gate/server.py:164` `"timestamp": utc_now(),`) |
| `tool` | The tool that ran it, always `run_gate` (`mcp/gate/server.py:165` `"tool": "run_gate",`) |
| `gate` | The allowlisted gate name (`mcp/gate/server.py:166` `"gate": gate,`) |
| `argv` | The exact argv executed (`mcp/gate/server.py:167` `"argv": argv,`) |
| `exit_code` | The process exit status (`mcp/gate/server.py:168` `"exit_code": exit_code,`) |
| `duration_seconds` | Wall-clock seconds (`mcp/gate/server.py:169` `"duration_seconds": duration_seconds,`) |
| `passed` | The verdict (`mcp/gate/server.py:170` `"passed": passed,`) |
| `timed_out` | Whether the timeout killed it (`mcp/gate/server.py:171` `"timed_out": timed_out,`) |
| `guard_applied` / `guard_satisfied` | The cache-hit guard outcome (`mcp/gate/server.py:172` `"guard_applied": guard_applied,`) |
| `calling_role` | The caller's role, defaulting to `unknown` (`mcp/gate/server.py:174` `"calling_role": calling_role or "unknown",`) |

A real line written by this server (`run_gate` on `clippy` as `tester`):

```json
{"argv": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"], "calling_role": "tester", "duration_seconds": 2.533, "exit_code": 0, "gate": "clippy", "guard_applied": true, "guard_satisfied": true, "passed": true, "timed_out": false, "timestamp": "2026-09-28T17:54:14.719689+00:00", "tool": "run_gate"}
```

Why does a refused call add no line?

Because the refusal is raised in validation, before `execute_gate` and before `audit_invocation`
(`mcp/gate/server.py:362` `validate_gate(gate)` on the line above `mcp/gate/server.py:365` `audit_invocation(`),
so the journal stays a record of executed commands only — the same reason the storage server never
journals a refused write (`mcp/storage/server.py:121` `if classification not in WRITE_CLASSIFICATIONS:`).

Measured: three refusals in one self-test run left the journal at 10 lines, unchanged
(`refusals_journal_nothing :: journal lines before=10 after=10`).

No tool edits or erases the journal; it is opened append-only
(`mcp/gate/server.py:143` `with open(AUDIT_PATH, "a", encoding="utf-8") as handle:`), and the only
read is bounded (`mcp/gate/server.py:382` `if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1 or limit > 200:`
with `mcp/gate/server.py:383` `raise ValueError("limit must be an integer between 1 and 200")`).

## Which exit codes did the three gates produce?

Captured through the server on 2026-09-28 in the agent sandbox container
(`cargo 1.95.0 (f2d3ce0bd 2026-03-21)`, `rustfmt 1.9.0-stable (59807616e1 2026-04-14)`):

| Gate | Exit code | `passed` | Guard | Duration | Evidence |
| --- | --- | --- | --- | --- | --- |
| `test` | `0` | `true` | not applicable | 5.7 s | `test result: ok. 20 passed; 0 failed` and `ok. 138 passed; 0 failed` — 158 passed, 0 failed, the documented baseline |
| `clippy` | `0` | `true` | applied, satisfied | 2.5 s | `Checking komun-server v0.1.0 (/workspace/crates/server)`, then `Finished` with no lint |
| `fmt` | `1` | `false` | not applicable | 0.1 s | 213 diff hunks across 35 files |

The `fmt` gate genuinely fails, and the failure is not this server's doing and not new: rustfmt over
the committed `HEAD` revision of an untouched file also fails (`git show HEAD:crates/server/src/api/admin.rs`
piped into `rustfmt --check --edition 2021` -> exit `1`, `9` hunks). The drift spans 35 files, far
wider than the two test files the working-tree fix touches, so it is repo-wide formatting drift
against this rustfmt version rather than a change under test. Neither `cargo fmt --check` nor the
gate rewrites any file; check mode only reports.

## How is this server's quality validated?

`mcp/gate/selftest.py` calls the running server over streamable HTTP and asserts every behaviour this
document claims: the tool surface, the three-gate allowlist, three kinds of refusal, the clippy
guard, the timeout clamp, real exit codes from the three gates, the ANSI strip, and one journal line
per executed invocation.

Run it against a listening server:

```bash
python3 mcp/gate/server.py --port 8003 &
python3 mcp/gate/selftest.py --url http://localhost:8003/mcp
```

Which line does it print, and which exit code does it use?

Print one `SELFTEST_RESULT` line and exit `0` only when every check passed
(`mcp/gate/selftest.py:268` `print(f"SELFTEST_RESULT passed={passed} total={total}", flush=True)`).

A live run against the server on port 8003 printed `SELFTEST_RESULT passed=21 total=21` and exited
`0`, with these checks:

| Check | What it proves |
| --- | --- |
| `tool_surface`, `run_gate_parameters` | Exactly three tools, and `run_gate` takes `gate`, `calling_role`, `timeout_seconds` |
| `allowlist_is_the_three_documented_gates` | The published argv equals the expected argv, held independently in the test (`mcp/gate/selftest.py:34` `EXPECTED_GATES = {`) |
| `refuses_free_form_command`, `refuses_argument_passthrough`, `refuses_shell_injection` | A command string, an extra argument, and an injected `touch` are all refused |
| `shell_injection_ran_nothing`, `refusals_journal_nothing` | Nothing ran and nothing was journalled |
| `timeout_is_clamped_and_reported` | `timeout_seconds=1` comes back as `60` |
| `fmt_gate_executes_and_reports`, `fmt_gate_writes_nothing` | The real `fmt` exit code and its diff report |
| `clippy_guard_applied`, `clippy_guard_satisfied`, `clippy_passed_requires_guard`, `clippy_marker_in_captured_output` | The guard touched the file, found the marker, and gates `passed` |
| `test_gate_executes_and_reports`, `test_gate_totals` | The real `test` exit code and 158 passed / 0 failed |
| `captured_output_has_no_ansi_escapes` | No ESC byte survives into the returned output |
| `journal_grows_one_line_per_invocation`, `journal_records_argv_exit_code_and_timestamp`, `read_audit_log_matches_the_journal_file` | One line per executed call, with the exact argv, exit code and timestamp |

The test holds its own expectation of the three argv tuples rather than asking the server twice, so a
server that published a wrong allowlist would fail the check instead of validating itself.

## Design notes

Why is there no `run_command`, no `--` passthrough, and no cwd argument?

- Keep the execution surface a fixed vocabulary, so a call cannot widen its own access: the
  `GATES` table is the only argv source (`mcp/gate/gate_vocabulary.py:94` `GATES: dict[str, dict[str, Any]] = {`).
- Keep `shell` off and the argv a list, so a metacharacter in a caller value can never become a
  command (`mcp/gate/server.py:279` `subprocess.run(`).
- Journal only executed commands, so the journal's line count is itself an audit fact
  (`mcp/gate/server.py:362` `validate_gate(gate)` runs before the journal call).
- Touch one declared file and require its status line, so a cached clippy run cannot be mistaken for
  a clean lint (`mcp/gate/server.py:298` `passed = exit_code == 0 and bool(guard["satisfied"])`).
- Strip ANSI before matching and before returning, so cargo's forced colour cannot break a guard or a
  caller's grep (`mcp/gate/server.py:79` `ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")`).
