#!/usr/bin/env python3
"""Self-test for the quality-gate MCP server.

Calls the running server over streamable HTTP and asserts the behaviours the server claims: the tool
surface, the three-gate allowlist, refusal of a free-form command, refusal of a shell-injection
string, the clippy cache-hit guard, real exit codes from the three gates, and one audit-journal line
per executed invocation. Every check prints PASS or FAIL with the evidence it used; any FAIL makes
the process exit non-zero. Printed output is the record of a real run, not a restatement of intent.

Run against a server already listening inside the sandbox container:

    python3 mcp/gate/server.py --port 8003 &
    python3 mcp/gate/selftest.py --url http://localhost:8003/mcp
"""

from __future__ import annotations

import argparse
import asyncio
import json
import re
import sys
from pathlib import Path
from typing import Any

from fastmcp import Client
from fastmcp.exceptions import ToolError

DEFAULT_URL = "http://localhost:8003/mcp"
DEFAULT_AUDIT_PATH = "/workspace/.memory/gate-audit.log"

# The allowlist the server is supposed to publish. Kept here as the independent expectation: a check
# that read both sides from the server would prove nothing.
EXPECTED_GATES = {
    "test": ["cargo", "test", "--workspace"],
    "clippy": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"],
    "fmt": ["cargo", "fmt", "--check"],
}
EXPECTED_TOOLS = {"list_gates", "run_gate", "read_audit_log"}
EXPECTED_RUN_GATE_PARAMS = {"gate", "calling_role", "timeout_seconds"}
GUARD_MARKER = "Checking komun-server"
INJECTION_TARGET = "/tmp/gate-selftest-pwned"
TEST_SUMMARY = re.compile(r"^test result: \w+\. (?P<passed>\d+) passed; (?P<failed>\d+) failed")

RESULTS: list[tuple[str, bool, str]] = []


def check(name: str, passed: bool, detail: str) -> bool:
    """Record and print one behaviour's verdict with the evidence behind it."""
    RESULTS.append((name, passed, detail))
    print(f"CHECK {'PASS' if passed else 'FAIL'} {name} :: {detail}", flush=True)
    return passed


def payload(result: Any) -> Any:
    """Return a tool result as Python data, whatever shape the client hands back."""
    for attribute in ("data", "structured_content"):
        value = getattr(result, attribute, None)
        if isinstance(value, (dict, list)):
            return value
    chunks = [
        getattr(block, "text", "")
        for block in getattr(result, "content", []) or []
        if getattr(block, "type", "") == "text"
    ]
    return json.loads("".join(chunks))


def audit_lines(path: str) -> list[dict[str, Any]]:
    """Read the audit journal as parsed records, one per non-empty line."""
    journal = Path(path)
    if not journal.exists():
        return []
    return [
        json.loads(line)
        for line in journal.read_text(encoding="utf-8", errors="replace").splitlines()
        if line.strip()
    ]


async def refusal(client: Client, name: str, gate: str, what: str) -> str | None:
    """Call run_gate with a value that must be refused and return the error text, or ``None``."""
    try:
        result = await client.call_tool("run_gate", {"gate": gate})
    except ToolError as error:
        text = str(error)
        check(name, "refused" in text and "allowlisted" in text, f"{what} -> {text.splitlines()[0]}")
        return text
    else:
        check(name, False, f"{what} -> NOT REFUSED, returned {json.dumps(payload(result))[:200]}")
        return None


async def run_selftest(url: str, audit_path: str) -> int:
    """Drive every check against the live server and return a process exit code."""
    before = audit_lines(audit_path)

    async with Client(url, timeout=1800) as client:
        tools = await client.list_tools()
        names = {tool.name for tool in tools}
        check(
            "tool_surface",
            names == EXPECTED_TOOLS,
            f"tools={sorted(names)} expected={sorted(EXPECTED_TOOLS)}",
        )

        run_gate_tool = next((tool for tool in tools if tool.name == "run_gate"), None)
        params = set((run_gate_tool.inputSchema.get("properties") or {})) if run_gate_tool else set()
        check(
            "run_gate_parameters",
            params == EXPECTED_RUN_GATE_PARAMS,
            f"properties={sorted(params)} expected={sorted(EXPECTED_RUN_GATE_PARAMS)}",
        )

        gates = payload(await client.call_tool("list_gates", {}))
        published = {entry["gate"]: entry["argv"] for entry in gates}
        check(
            "allowlist_is_the_three_documented_gates",
            published == EXPECTED_GATES,
            f"published={json.dumps(published, sort_keys=True)}",
        )

        await refusal(client, "refuses_free_form_command", "cargo test --workspace", "gate='cargo test --workspace'")
        await refusal(client, "refuses_argument_passthrough", "test -- --nocapture", "gate='test -- --nocapture'")
        await refusal(
            client,
            "refuses_shell_injection",
            f"test; touch {INJECTION_TARGET}",
            f"gate='test; touch {INJECTION_TARGET}'",
        )
        check(
            "shell_injection_ran_nothing",
            not Path(INJECTION_TARGET).exists(),
            f"{INJECTION_TARGET} exists={Path(INJECTION_TARGET).exists()}",
        )
        check(
            "refusals_journal_nothing",
            len(audit_lines(audit_path)) == len(before),
            f"journal lines before={len(before)} after={len(audit_lines(audit_path))}",
        )

        # --- the three real gates, in the order that keeps the run short -----------------------
        # One extra invocation first: it proves the per-run timeout is clamped and reported rather
        # than passed through, and it is counted by the journal assertions below.
        clamped = payload(await client.call_tool("run_gate", {"gate": "fmt", "timeout_seconds": 1}))
        check(
            "timeout_is_clamped_and_reported",
            clamped["timeout_seconds"] == 60 and clamped["timed_out"] is False,
            f"requested=1s reported={clamped['timeout_seconds']}s timed_out={clamped['timed_out']} "
            f"exit={clamped['exit_code']}",
        )
        executed: list[dict[str, Any]] = [clamped]

        fmt = payload(await client.call_tool("run_gate", {"gate": "fmt", "calling_role": "tester"}))
        executed.append(fmt)
        check(
            "fmt_gate_executes_and_reports",
            fmt["argv"] == EXPECTED_GATES["fmt"]
            and isinstance(fmt["exit_code"], int)
            and fmt["passed"] == (fmt["exit_code"] == 0)
            and fmt["duration_seconds"] > 0
            and fmt["timeout_seconds"] == 900,
            f"exit={fmt['exit_code']} passed={fmt['passed']} verdict={fmt['verdict']} "
            f"duration={fmt['duration_seconds']}s argv={fmt['argv']} "
            f"default_timeout={fmt['timeout_seconds']}s "
            f"diffs={len(re.findall(r'^Diff in ', fmt['stdout'], re.M))}",
        )
        check(
            "fmt_gate_writes_nothing",
            "Diff in" in fmt["stdout"] or fmt["exit_code"] == 0,
            f"stdout is a rustfmt diff report, exit={fmt['exit_code']}",
        )

        clippy = payload(
            await client.call_tool("run_gate", {"gate": "clippy", "calling_role": "tester"})
        )
        executed.append(clippy)
        guard = clippy["guard"]
        check(
            "clippy_guard_applied",
            guard["applied"] is True and guard["touched"] == "/workspace/crates/server/src/main.rs",
            f"applied={guard['applied']} touched={guard['touched']} marker='{guard['marker']}'",
        )
        check(
            "clippy_guard_satisfied",
            guard["satisfied"] is True,
            f"satisfied={guard['satisfied']} detail={guard['detail']}",
        )
        check(
            "clippy_passed_requires_guard",
            clippy["passed"] == (clippy["exit_code"] == 0 and guard["satisfied"] is True),
            f"exit={clippy['exit_code']} guard_satisfied={guard['satisfied']} "
            f"passed={clippy['passed']} verdict={clippy['verdict']} "
            f"duration={clippy['duration_seconds']}s",
        )
        check(
            "clippy_marker_in_captured_output",
            GUARD_MARKER in clippy["stderr"],
            f"'{GUARD_MARKER}' occurrences in stderr="
            f"{clippy['stderr'].count(GUARD_MARKER)}, stdout={clippy['stdout'].count(GUARD_MARKER)}",
        )

        test = payload(await client.call_tool("run_gate", {"gate": "test", "calling_role": "tester"}))
        executed.append(test)
        summaries = [TEST_SUMMARY.match(line) for line in test["stdout"].splitlines()]
        totals = [
            (int(match.group("passed")), int(match.group("failed")))
            for match in summaries
            if match
        ]
        check(
            "test_gate_executes_and_reports",
            test["argv"] == EXPECTED_GATES["test"] and isinstance(test["exit_code"], int),
            f"exit={test['exit_code']} passed={test['passed']} verdict={test['verdict']} "
            f"duration={test['duration_seconds']}s argv={test['argv']}",
        )
        check(
            "test_gate_totals",
            len(totals) > 0
            and sum(passed for passed, _ in totals) >= 158
            and (sum(failed for _, failed in totals) == 0) == (test["exit_code"] == 0),
            f"suites={len(totals)} passed={sum(p for p, _ in totals)} "
            f"failed={sum(f for _, f in totals)} baseline=158",
        )
        check(
            "captured_output_has_no_ansi_escapes",
            "\x1b" not in fmt["stdout"] + clippy["stderr"] + clippy["stdout"] + test["stdout"]
            and clippy["output_ansi_stripped"] is True,
            "no ESC byte in the returned fmt, clippy and test output; cargo runs with "
            "CARGO_TERM_COLOR=always in this image, so a status line arrives as "
            "'\\x1b[1m\\x1b[92m    Checking\\x1b[0m komun-server' and the server must strip the "
            "escapes before the guard or any caller-side grep can match it",
        )

        journal = payload(await client.call_tool("read_audit_log", {"limit": 200}))
        after = audit_lines(audit_path)
        check(
            "journal_grows_one_line_per_invocation",
            len(after) == len(before) + len(executed),
            f"lines before={len(before)} after={len(after)} expected={len(before) + len(executed)} "
            f"for {len(executed)} executed invocations",
        )
        required = {"timestamp", "tool", "gate", "argv", "exit_code", "duration_seconds", "passed"}
        tail = after[-len(executed):]
        expected_argv = [EXPECTED_GATES[result["gate"]] for result in executed]
        check(
            "journal_records_argv_exit_code_and_timestamp",
            len(tail) == len(executed)
            and all(required <= set(record) for record in tail)
            and [record["gate"] for record in tail] == [result["gate"] for result in executed]
            and [record["argv"] for record in tail] == expected_argv
            and [record["exit_code"] for record in tail]
            == [result["exit_code"] for result in executed]
            and all("T" in record["timestamp"] for record in tail),
            f"gates={[r['gate'] for r in tail]} exit_codes={[r['exit_code'] for r in tail]} "
            f"timestamps={[r['timestamp'] for r in tail]} argv_match="
            f"{[r['argv'] for r in tail] == expected_argv}",
        )
        check(
            "read_audit_log_matches_the_journal_file",
            [record["timestamp"] for record in journal[-len(executed):]]
            == [record["timestamp"] for record in tail],
            f"tool returned {len(journal)} records, newest={journal[-1]['gate'] if journal else None}",
        )

    passed = sum(1 for _, ok, _ in RESULTS if ok)
    total = len(RESULTS)
    print(f"SELFTEST_RESULT passed={passed} total={total}", flush=True)
    for name, ok, detail in RESULTS:
        if not ok:
            print(f"SELFTEST_FAILURE {name} :: {detail}", flush=True)
    return 0 if passed == total else 1


def main() -> int:
    """Parse the CLI flags and run the self-test against the live server."""
    parser = argparse.ArgumentParser(description="Self-test for the quality-gate MCP server")
    parser.add_argument("--url", default=DEFAULT_URL, help=f"MCP endpoint (default {DEFAULT_URL})")
    parser.add_argument(
        "--audit-path",
        default=DEFAULT_AUDIT_PATH,
        help=f"audit journal (default {DEFAULT_AUDIT_PATH})",
    )
    args = parser.parse_args()
    return asyncio.run(run_selftest(args.url, args.audit_path))


if __name__ == "__main__":
    sys.exit(main())
