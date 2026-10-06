#!/usr/bin/env python3
"""Self-test for the browser MCP server.

Starts the server against a page this file serves, drives it over streamable HTTP, and asserts what
the eight tools claim. The page carries ``<img src=x onerror="document.title='PWNED'">``: a browser
that runs page JavaScript sets the title, and the positive control reads it back.

The negative control is ``--negative-control``: the same run with page JavaScript disabled, so the
inline handler never runs, the title stays ``before``, and the title assertion fails. A smoke test
that cannot fail proves nothing; this one exits non-zero when the browser does not run the page's own
script.

Run inside the sandbox container:

    python3 mcp/browser/selftest.py                    # positive control, exits 0
    python3 mcp/browser/selftest.py --negative-control # negative control, exits non-zero

Every check prints PASS or FAIL with its evidence, and the last line is machine-readable.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from fastmcp import Client
from fastmcp.exceptions import ToolError

REPO = Path(__file__).resolve().parents[2]
SERVER = REPO / "mcp" / "browser" / "server.py"
DEFAULT_PORT = int(os.environ.get("BROWSER_SELFTEST_PORT", "8004"))
BROWSER = os.environ.get("BROWSER_BINARY", "/usr/bin/chromium")
TOOL_NAMES = {
    "browser_open",
    "browser_snapshot",
    "browser_click",
    "browser_type",
    "browser_press",
    "browser_diagnostics",
    "browser_screenshot",
    "browser_close",
}
REFUSED_ORIGIN = "http://example.com/evil"

# The page the positive control reads: the inline onerror sets the title, and the missing image also
# produces the 404 console entry the diagnostics check expects.
PWNED_PAGE = b"""<!doctype html>
<html><head><title>before</title></head>
<body>
<h1>Beta surface</h1>
<a id="next" href="/next">Next page</a>
<form><input id="q" name="q" type="text"><button type="submit">Go</button></form>
<img id="pwn" src="/missing.png" onerror="document.title='PWNED'">
<script>console.log('beta page loaded');</script>
</body></html>"""
NEXT_PAGE = b"<html><head><title>next</title></head><body><p>arrived</p></body></html>"

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


# --- The origin the base URL points at ------------------------------------------------------
class OriginHandler(BaseHTTPRequestHandler):
    """Serve the control page, one second page, and a 404 for everything else."""

    def log_message(self, format: str, *args: Any) -> None:  # noqa: A002 - the base signature
        pass

    def do_GET(self) -> None:  # noqa: N802 - the interface BaseHTTPRequestHandler defines
        if self.path == "/pwned":
            body, status = PWNED_PAGE, 200
        elif self.path == "/next":
            body, status = NEXT_PAGE, 200
        else:
            body, status = b"not found", 404
        self.send_response(status)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def start_origin() -> tuple[ThreadingHTTPServer, str]:
    """Start the control-page origin on an ephemeral port and return it with its base URL."""
    httpd = ThreadingHTTPServer(("127.0.0.1", 0), OriginHandler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return httpd, f"http://127.0.0.1:{httpd.server_address[1]}/"


# --- The hermetic configuration the server is started with ----------------------------------
def write_config(directory: Path) -> None:
    """Write a self-contained allow-list and routing map so the test needs no repository config."""
    grants = {
        "beta-tester": sorted(f"mcp__browser__{tool}" for tool in TOOL_NAMES)
        + ["mcp__coursetools__file_read", "mcp__storage__read_entry"],
        "tester": ["mcp__coursetools__file_read", "mcp__gate__run_gate"],
    }
    (directory / "routing-map.json").write_text(
        json.dumps(
            {"project": "proj-komun", "servers": ["browser", "storage"], "grants": grants},
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    (directory / "allow-list.json").write_text(
        json.dumps(
            {
                "server": "browser",
                "operations": sorted(TOOL_NAMES),
                "roles": {"beta-tester": sorted(TOOL_NAMES), "tester": []},
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


def port_open(port: int, host: str = "127.0.0.1", timeout: float = 1.0) -> bool:
    with socket.socket() as probe:
        probe.settimeout(timeout)
        return probe.connect_ex((host, port)) == 0


def wait_for_port(port: int, timeout: float = 90.0) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if port_open(port):
            return True
        time.sleep(0.25)
    return False


def read_journal(path: Path) -> list[dict[str, Any]]:
    """Read the audit journal as parsed records, one per non-empty line."""
    if not path.is_file():
        return []
    records: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.strip():
            records.append(json.loads(line))
    return records


def start_server(directory: Path, port: int, base_url: str, negative: bool, log_path: Path) -> subprocess.Popen:
    """Launch the browser server against the hermetic config and return the process."""
    argv = [
        sys.executable,
        str(SERVER),
        "--port",
        str(port),
        "--host",
        "127.0.0.1",
        "--base-url",
        base_url,
        "--binary",
        BROWSER,
        "--evidence-dir",
        str(directory / "evidence"),
        "--audit-path",
        str(directory / "browser-audit.log"),
        "--allowlist-path",
        str(directory / "allow-list.json"),
        "--routing-map-path",
        str(directory / "routing-map.json"),
        "--timeout-seconds",
        "30",
    ]
    if negative:
        argv.append("--disable-javascript")
    environment = {**os.environ, "AGENT_ROLE": "beta-tester"}
    log = open(log_path, "w", encoding="utf-8")  # noqa: SIM115 - held for the process's lifetime
    return subprocess.Popen(  # noqa: S603 - a fixed argv naming the repository's own server
        argv, cwd=str(REPO), env=environment, stdout=log, stderr=subprocess.STDOUT
    )


# --- The checks -----------------------------------------------------------------------------
async def run_checks(url: str, journal_path: Path, evidence_dir: Path) -> None:
    """Drive every tool over streamable HTTP and assert the behaviours the server claims."""
    calls = 0

    async def call(client: Client, name: str, arguments: dict) -> Any:
        nonlocal calls
        calls += 1
        return await client.call_tool(name, arguments)

    async with Client(url, timeout=120) as client:
        tools = await client.list_tools()
        names = sorted(tool.name for tool in tools)
        check(
            "tool_surface_is_exactly_the_eight",
            set(names) == TOOL_NAMES and len(names) == 8,
            f"tools={names}",
        )

        opened = payload(await call(client, "browser_open", {"url": "/pwned", "calling_role": "beta-tester"}))
        check(
            "positive_control_page_javascript_ran",
            opened.get("title") == "PWNED",
            f"title={opened.get('title')!r} (the inline onerror sets 'PWNED')",
        )
        check(
            "browser_open_reports_url_status_and_ok",
            opened.get("ok") is True
            and opened.get("status") == 200
            and str(opened.get("url", "")).endswith("/pwned"),
            f"ok={opened.get('ok')} status={opened.get('status')} url={opened.get('url')}",
        )

        state = payload(await call(client, "browser_snapshot", {"calling_role": "beta-tester"}))
        check(
            "browser_snapshot_is_text_first_and_structured",
            set(state) >= {"url", "title", "text", "links", "fields", "buttons"}
            and "Beta surface" in state.get("text", "")
            and any(link.get("href", "").endswith("/next") for link in state.get("links", []))
            and any(field.get("name") == "q" for field in state.get("fields", []))
            and any("Go" in button.get("text", "") for button in state.get("buttons", []))
            and b"PNG" not in json.dumps(state).encode(),
            f"links={len(state.get('links', []))} fields={len(state.get('fields', []))} "
            f"buttons={len(state.get('buttons', []))} text_has_Beta={'Beta surface' in state.get('text', '')}",
        )

        drained = payload(await call(client, "browser_diagnostics", {"calling_role": "beta-tester"}))
        console_text = " ".join(entry.get("text", "") for entry in drained.get("console", []))
        missing = [r for r in drained.get("requests", []) if str(r.get("url", "")).endswith("/missing.png")]
        check(
            "browser_diagnostics_reports_console_and_requests",
            set(drained) == {"console", "page_errors", "requests"}
            and bool(drained.get("console"))
            and "404" in console_text
            and bool(missing)
            and missing[0].get("status") == 404,
            f"console={len(drained.get('console', []))} requests={len(drained.get('requests', []))} "
            f"missing.png={missing}",
        )

        empty = payload(await call(client, "browser_diagnostics", {"calling_role": "beta-tester"}))
        check(
            "browser_diagnostics_drains_on_read",
            empty == {"console": [], "page_errors": [], "requests": []},
            f"second drain={json.dumps(empty)}",
        )

        typed = payload(await call(client, "browser_type", {"selector": "#q", "text": "hello", "calling_role": "beta-tester"}))
        await call(client, "browser_press", {"key": "Enter", "calling_role": "beta-tester"})
        after_type = payload(await call(client, "browser_snapshot", {"calling_role": "beta-tester"}))
        field_value = next(
            (f.get("value") for f in after_type.get("fields", []) if f.get("name") == "q"), None
        )
        check(
            "browser_type_sets_the_field_and_browser_press_returns",
            typed.get("typed") is True and typed.get("selector") == "#q" and field_value == "hello",
            f"typed={typed.get('typed')} field_value={field_value!r}",
        )

        clicked = payload(await call(client, "browser_click", {"selector": "#next", "calling_role": "beta-tester"}))
        check(
            "browser_click_returns_clicked_selector_and_url_after",
            clicked.get("clicked") is True
            and clicked.get("selector") == "#next"
            and str(clicked.get("url_after", "")).endswith("/next"),
            f"clicked={clicked.get('clicked')} url_after={clicked.get('url_after')}",
        )

        try:
            await call(client, "browser_open", {"url": REFUSED_ORIGIN, "calling_role": "beta-tester"})
            refused, refusal_text = False, ""
        except ToolError as error:
            refused, refusal_text = True, str(error)
        check(
            "browser_open_refuses_a_non_base_origin",
            refused and "url_refused" in refusal_text and "example.com" in refusal_text,
            f"refused={refused} text={refusal_text.splitlines()[0][:200] if refusal_text else ''}",
        )

        try:
            await call(client, "browser_open", {"url": "/pwned", "calling_role": "tester"})
            denied, denied_text = False, ""
        except ToolError as error:
            denied, denied_text = True, str(error)
        check(
            "role_binding_refuses_a_disagreeing_calling_role",
            denied and "authorization_denied" in denied_text and "disagrees" in denied_text,
            f"denied={denied} text={denied_text.splitlines()[0][:200] if denied_text else ''}",
        )

        shot = payload(await call(client, "browser_screenshot", {"name": "control", "calling_role": "beta-tester"}))
        shot_path = Path(shot.get("path", ""))
        check(
            "browser_screenshot_writes_a_png",
            shot_path.is_file() and shot_path.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n"
            and shot_path.parent == evidence_dir,
            f"path={shot_path} exists={shot_path.is_file()}",
        )

        closed = payload(await call(client, "browser_close", {"calling_role": "beta-tester"}))
        check("browser_close_returns_closed", closed == {"closed": True}, f"closed={json.dumps(closed)}")

    rows = read_journal(journal_path)
    required = {"timestamp", "tool", "calling_role", "target", "allowed", "ok", "reason"}
    check(
        "journal_has_one_line_per_call",
        len(rows) == calls,
        f"journal lines={len(rows)} tool calls={calls}",
    )
    check(
        "journal_rows_carry_tool_role_target_and_ok",
        bool(rows) and all(required <= set(record) for record in rows)
        and all("T" in str(record.get("timestamp")) for record in rows),
        f"missing_keys={sorted(required - set(rows[0])) if rows else sorted(required)} rows={len(rows)}",
    )
    refusals = [
        record
        for record in rows
        if record.get("tool") == "browser_open"
        and record.get("ok") is False
        and "url_refused" in (record.get("reason") or "")
    ]
    check(
        "the_url_refusal_is_journalled_with_the_role_and_reason",
        bool(refusals)
        and refusals[-1].get("calling_role") == "beta-tester"
        and REFUSED_ORIGIN.split("/evil")[0] in (refusals[-1].get("reason") or ""),
        json.dumps(refusals[-1], sort_keys=True)[:300] if refusals else "no refusal row",
    )
    check(
        "the_role_binding_refusal_is_journalled",
        any(
            record.get("tool") == "browser_open"
            and record.get("allowed") is False
            and "authorization_denied" in (record.get("reason") or "")
            for record in rows
        ),
        f"allowed_false_rows={sum(1 for r in rows if r.get('allowed') is False)}",
    )


def tail(path: Path, count: int = 20) -> str:
    """Return the last ``count`` lines of a file, or a note that it is missing."""
    if not path.is_file():
        return f"{path} was not created"
    return "\n".join(path.read_text(encoding="utf-8", errors="replace").splitlines()[-count:])


def main() -> int:
    """Start the control origin and the server, run the checks, and return an exit code."""
    parser = argparse.ArgumentParser(description="Self-test for the browser MCP server")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument(
        "--negative-control",
        action="store_true",
        help="disable page JavaScript; the positive control must then fail and this exits non-zero",
    )
    args = parser.parse_args()

    print(f"browser self-test {'NEGATIVE control (page JavaScript disabled)' if args.negative_control else 'positive control'}")
    directory = Path(tempfile.mkdtemp(prefix="browser-selftest-"))
    write_config(directory)
    origin, base_url = start_origin()
    print(f"origin={base_url} server_port={args.port} work={directory}", flush=True)

    log_path = directory / "server.log"
    process = start_server(directory, args.port, base_url, args.negative_control, log_path)
    exit_code = 1
    try:
        if not wait_for_port(args.port):
            check(
                "server_binds_its_port",
                False,
                f"nothing listening on {args.port} after 90s; server log:\n{tail(log_path)}",
            )
        else:
            check("server_binds_its_port", True, f"listening on 127.0.0.1:{args.port}")
            asyncio.run(run_checks(f"http://127.0.0.1:{args.port}/mcp", directory / "browser-audit.log", directory / "evidence"))
    finally:
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
        origin.shutdown()

    passed = sum(1 for _, ok, _ in RESULTS if ok)
    total = len(RESULTS)
    print(f"SELFTEST_RESULT passed={passed} total={total} mode={'negative' if args.negative_control else 'positive'}")
    for name, ok, detail in RESULTS:
        if not ok:
            print(f"SELFTEST_FAILURE {name} :: {detail}")
    if passed == total:
        exit_code = 0
    elif args.negative_control:
        # The negative control is expected to fail on the title check; exit non-zero either way.
        exit_code = 1
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
