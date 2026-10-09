#!/usr/bin/env python3
"""Browser MCP server for the beta-tester role: drives a real headless Chromium over the DevTools
Protocol and exposes exactly eight tools.

The beta tester uses the running app instead of reading the repository, so this is the only MCP
server that reaches the application surface. It exposes eight schema-bound operations over
streamable HTTP (FastMCP): ``browser_open``, ``browser_snapshot``, ``browser_click``,
``browser_type``, ``browser_press``, ``browser_diagnostics``, ``browser_screenshot`` and
``browser_close``. The browser is the container's own ``chromium`` (``--headless=new``); see
``mcp/browser/driver.py`` for the wire.

Two enforcement layers sit in front of those operations, mirroring the storage server:

* ``_authorize(calling_role, tool)`` is the first statement of every one of the eight tools. It
  reads the role allow-list from ``mcp/browser/allow-list.json`` (override with
  ``BROWSER_ALLOW_LIST_PATH`` or ``--allowlist-path``) and refuses an ungranted pair with an
  ``authorization_denied`` error that names the role, the tool and the roles that ARE allowed. An
  unknown, missing or blank role is refused, never defaulted to an allowed one.
* The role is bound server-side to the container's own ``AGENT_ROLE``: a ``calling_role`` argument
  that disagrees with it is refused outright. ``bind_role`` is the storage server's guard, unchanged.
* ``browser_open`` resolves a path or an absolute URL against ``BETA_BASE_URL`` and refuses any
  origin but the base origin's. The refusal is journalled like an authorization refusal.
* Every call appends one JSON object per line to ``.memory/browser-audit.log`` -- the tool, the
  resolved calling role, the target, whether it succeeded, the refusal reason, and ``bound``.

The allow-list is cross-checked at startup against ``docs/routing-and-tool-grant-map.json``: the
projection of ``grants[role]`` filtered to ``mcp__browser__*`` must equal this file's entry for the
role, or the server refuses to start. While the routing map carries no browser grant yet, the server
starts with a warning and the allow-list governs, so a checkout that predates the beta-tester row
still serves.

Run inside the sandbox container:

    python3 mcp/browser/server.py --port 8004 --host 0.0.0.0
"""

from __future__ import annotations

import argparse
import atexit
import asyncio
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.parse import urljoin, urlparse

from fastmcp import FastMCP
from starlette.middleware import Middleware
from starlette.middleware.cors import CORSMiddleware

_HERE = Path(__file__).resolve().parent
_MCP_DIR = str(_HERE.parent)
for _path in (str(_HERE), _MCP_DIR):
    if _path not in sys.path:
        sys.path.insert(0, _path)
import hashchain  # noqa: E402
from driver import Chrome  # noqa: E402

# --- Runtime paths: container defaults, every one overridable for a local run ---------------
MEMORY_DIR = os.getenv("MEMORY_DIR", "/workspace/.memory")
BASE_URL = os.getenv("BETA_BASE_URL", "http://rt-app-web/")
BROWSER_BINARY = os.getenv("BROWSER_BINARY", "/usr/bin/chromium")
EVIDENCE_DIR = os.getenv(
    "BROWSER_EVIDENCE_DIR", str(Path(MEMORY_DIR) / "beta-evidence")
)
AUDIT_PATH = os.getenv("BROWSER_AUDIT_PATH", str(Path(MEMORY_DIR) / "browser-audit.log"))
BROWSER_TIMEOUT_SECONDS = float(os.getenv("BROWSER_TIMEOUT_SECONDS", "30"))
DISABLE_JAVASCRIPT = os.getenv("BROWSER_DISABLE_JAVASCRIPT", "").lower() in ("1", "true", "yes")
# The two configuration files sit beside this one and in docs/, resolved relative to this file so
# the server reads the same pair whether it is started from /workspace, from the repository root, or
# from anywhere else.
ALLOW_LIST_PATH = Path(
    os.getenv("BROWSER_ALLOW_LIST_PATH", str(Path(__file__).resolve().parent / "allow-list.json"))
)
ROUTING_MAP_PATH = Path(
    os.getenv(
        "BROWSER_ROUTING_MAP_PATH",
        str(Path(__file__).resolve().parents[2] / "docs" / "routing-and-tool-grant-map.json"),
    )
)

# --- Vocabulary -----------------------------------------------------------------------------
# The complete callable surface of this server: what a role can be granted, and the only names the
# allow-list file may use. The grant identifier is ``mcp__browser__<tool>``.
OPERATIONS = (
    "browser_open",
    "browser_snapshot",
    "browser_click",
    "browser_type",
    "browser_press",
    "browser_diagnostics",
    "browser_screenshot",
    "browser_close",
)
GRANT_PREFIX = "mcp__browser__"

mcp = FastMCP("browser")


# --- Time and paths -------------------------------------------------------------------------
def utc_now() -> str:
    """Return the current UTC instant as an ISO-8601 string."""
    return datetime.now(timezone.utc).isoformat()


def ensure_parent(path: str) -> None:
    """Create the parent directory of a runtime file and fail loudly if it is unwritable."""
    parent = Path(path).expanduser().parent
    parent.mkdir(parents=True, exist_ok=True)
    if not os.access(parent, os.W_OK):
        raise PermissionError(f"{parent} is not writable; the server cannot use {path}")


# --- The audit journal ----------------------------------------------------------------------
def append_audit_record(record: dict[str, Any]) -> None:
    """Append exactly one JSON object plus newline. The file is opened append-only."""
    ensure_parent(AUDIT_PATH)
    hashchain.append_journal_record(AUDIT_PATH, record)


def audit_event(
    tool: str,
    *,
    allowed: bool,
    ok: bool,
    calling_role: str | None = None,
    target: str | None = None,
    reason: str | None = None,
) -> None:
    """Write one journal record for an allowed call or a refusal.

    ``target`` is the resolved URL, the selector or the key the call named; a refused
    authorization carries ``None`` because the guard refuses before any argument is read.
    """
    append_audit_record(
        {
            "timestamp": utc_now(),
            "tool": tool,
            "calling_role": calling_role or "unknown",
            "target": target if isinstance(target, str) else None,
            "allowed": allowed,
            "ok": ok,
            "reason": reason,
            "bound": bool(environment_role()),
        })


# --- The allow-list: the whole grant surface, plus the guard every tool calls first ----------
class AllowListError(RuntimeError):
    """A missing, malformed or self-contradictory configuration. Fatal: the server never starts."""


class AuthorizationDenied(PermissionError):
    """A role called a tool the allow-list does not grant it. Journalled, then raised."""


def load_allow_list(path: Path) -> dict[str, list[str]]:
    """Read ``role -> permitted tools`` from the allow-list file, or fail loudly.

    There is deliberately no in-code copy of the grants and no permissive fallback: a server that
    cannot read its allow-list must not start, because the alternative is a server that answers
    every role, which is the failure this layer exists to prevent.
    """
    if not path.is_file():
        raise AllowListError(
            f"allow-list file not found at {path}; refusing to start, because a server without its "
            "allow-list would let every role call every tool"
        )
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise AllowListError(f"allow-list file {path} is not valid JSON: {error}") from error

    roles = data.get("roles")
    if not isinstance(roles, dict) or not roles:
        raise AllowListError(f"allow-list file {path} carries no 'roles' object")
    for role, tools in roles.items():
        if not isinstance(tools, list) or not all(isinstance(tool, str) for tool in tools):
            raise AllowListError(
                f"allow-list file {path}: the entry for role '{role}' must be a list of tool names"
            )
        unknown = sorted(set(tools) - set(OPERATIONS))
        if unknown:
            raise AllowListError(
                f"allow-list file {path}: role '{role}' grants unknown tools {unknown}; this server "
                f"exposes {list(OPERATIONS)}"
            )
    return {role: list(tools) for role, tools in roles.items()}


def load_routing_browser_grants(path: Path) -> tuple[dict[str, list[str]], bool]:
    """Project ``grants[role]`` from the routing map to browser tool names.

    Returns the per-role projection and whether the map mentions the browser server at all. A map
    that carries no ``mcp__browser__`` grant and does not list ``browser`` predates this server, and
    that is the one case where the startup cross-check cannot run.
    """
    if not path.is_file():
        raise AllowListError(
            f"routing map not found at {path}; refusing to start, because without it this server "
            "cannot check that its allow-list is the map's own projection"
        )
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise AllowListError(f"routing map {path} is not valid JSON: {error}") from error

    grants = data.get("grants")
    if not isinstance(grants, dict) or not grants:
        raise AllowListError(f"routing map {path} carries no 'grants' object")
    mentions_browser = "browser" in (data.get("servers") or [])
    projected: dict[str, list[str]] = {}
    for role, tools in grants.items():
        if not isinstance(tools, list) or not all(isinstance(tool, str) for tool in tools):
            raise AllowListError(
                f"routing map {path}: grants for role '{role}' must be a list of tool identifiers"
            )
        short = sorted(tool[len(GRANT_PREFIX) :] for tool in tools if tool.startswith(GRANT_PREFIX))
        projected[role] = short
        if short:
            mentions_browser = True
    return projected, mentions_browser


def check_configuration_agrees(
    allow_list: dict[str, list[str]],
    map_grants: dict[str, list[str]],
    mentions_browser: bool,
    allow_list_path: Path,
    routing_map_path: Path,
) -> None:
    """Fail loudly when the allow-list and the routing map contradict each other.

    Every role's browser grants in the file must be exactly the map's ``mcp__browser__*`` projection.
    A role granted a tool here but not there would be an invented grant, and one granted there but
    not here would be a silently dropped grant; neither is resolved by guessing. A map that predates
    the browser server has no projection to disagree with, so it is a warning and the allow-list
    governs.
    """
    if not mentions_browser:
        print(
            f"WARNING: {routing_map_path} lists no 'browser' server and no '{GRANT_PREFIX}' grant, "
            f"so the browser grant map could not be cross-checked; {allow_list_path} governs",
            flush=True,
        )
        return
    for role in sorted(set(allow_list) | set(map_grants)):
        expected = map_grants.get(role, [])
        actual = sorted(allow_list.get(role, []))
        if actual != expected:
            raise AllowListError(
                f"{allow_list_path} lists {actual} for role '{role}' while {routing_map_path} "
                f"projects {expected}; the two files must agree"
            )


def authorized_roles(tool: str) -> list[str]:
    """Return, sorted, every role the allow-list grants ``tool``."""
    return sorted(role for role, tools in ALLOW_LIST.items() if tool in tools)


# --- Role binding: AGENT_ROLE is the identity, the calling_role argument only corroborates -----
def environment_role() -> str:
    """Return the role this process is bound to, from ``AGENT_ROLE``, or an empty string.

    The harness sets ``AGENT_ROLE`` in the container it launches (``scripts/run-agent.sh:259``
    ``-e AGENT_ROLE="$ROLE"``). A blank or whitespace-only value counts as unset. There is
    deliberately no flag, no config key and no "trusted client" escape hatch: this environment
    variable is the only switch, and the harness is what sets it.
    """
    value = os.environ.get("AGENT_ROLE")
    return value.strip() if isinstance(value, str) else ""


def bind_role(calling_role: str | None) -> tuple[str, str | None]:
    """Bind the caller's role to this process's ``AGENT_ROLE`` and return ``(role, mismatch)``.

    With ``AGENT_ROLE`` unset the argument is used exactly as before, so a local run, pytest or a
    self-test is unchanged. With it set, the environment is the effective role: an omitted, blank
    or ``unknown`` argument yields it, and an argument naming a *different* role yields a mismatch
    description instead of a role -- a caller cannot escalate by typing another role's name, and
    the environment is never silently overridden by the argument.
    """
    argument = calling_role.strip() if isinstance(calling_role, str) else ""
    bound = environment_role()
    if not bound:
        return argument, None
    if argument and argument != "unknown" and argument != bound:
        return bound, (
            f"calling_role {argument!r} disagrees with the bound AGENT_ROLE {bound!r}: the role is "
            "bound to this container by its environment, so a disagreeing argument is refused, "
            "never overridden"
        )
    return bound, None


def _authorize(calling_role: str | None, tool: str) -> str:
    """Refuse an ungranted (role, tool) pair, journal the refusal, and return the role.

    This is the first statement of every tool, so nothing else in the tool runs for a refused call:
    no navigation, no browser launch, no state change. The refusal names the role, the tool and the
    roles that ARE allowed, and it is journalled to the same audit log the allowed calls use.
    """
    if tool not in OPERATIONS:
        raise ValueError(f"'{tool}' is not one of this server's tools {list(OPERATIONS)}")

    allowed = authorized_roles(tool)
    role, mismatch = bind_role(calling_role)
    if mismatch is not None:
        reason = (
            f"authorization_denied: {mismatch}. "
            f"tool={tool!r} role={role or 'unknown'!r} allowed_roles={allowed}"
        )
        audit_event(tool, allowed=False, ok=False, calling_role=role or "unknown", reason=reason)
        raise AuthorizationDenied(reason)

    if role and role in ALLOW_LIST and tool in ALLOW_LIST[role]:
        return role

    if not role or role == "unknown":
        cause = (
            f"unknown role {calling_role!r}: a missing, blank or unrecognised role is refused and "
            "is never defaulted to an allowed one"
        )
    elif role not in ALLOW_LIST:
        cause = f"unknown role {role!r}: it is not one of the roles {sorted(ALLOW_LIST)}"
    else:
        cause = f"role {role!r} is not granted {tool!r}"

    reason = (
        f"authorization_denied: {cause}. "
        f"tool={tool!r} role={role or 'unknown'!r} allowed_roles={allowed}"
    )
    audit_event(tool, allowed=False, ok=False, calling_role=role or "unknown", reason=reason)
    raise AuthorizationDenied(reason)


# The configuration is loaded here, at import time, so a missing or contradictory pair kills the
# process before uvicorn binds a port. main() loads it again for the CLI flags.
try:
    ALLOW_LIST: dict[str, list[str]] = load_allow_list(ALLOW_LIST_PATH)
    MAP_GRANTS, MAP_MENTIONS_BROWSER = load_routing_browser_grants(ROUTING_MAP_PATH)
    check_configuration_agrees(
        ALLOW_LIST, MAP_GRANTS, MAP_MENTIONS_BROWSER, ALLOW_LIST_PATH, ROUTING_MAP_PATH
    )
except AllowListError as error:
    print(f"ERROR: {error}", file=sys.stderr, flush=True)
    raise SystemExit(2) from error


# --- The URL boundary -----------------------------------------------------------------------
def resolve_target(url: str) -> tuple[str | None, str | None]:
    """Resolve a path or URL against ``BETA_BASE_URL``, refusing any other origin.

    Returns ``(resolved_url, refusal_reason)``: exactly one is ``None``. A path is resolved against
    the base; an absolute URL is accepted only when its scheme, host and port equal the base's.
    """
    if not isinstance(url, str) or not url.strip():
        return None, (
            "url_refused: the target is empty; pass a path such as '/aid' or an absolute URL whose "
            f"origin is {BASE_URL!r}"
        )
    candidate = url.strip()
    parsed = urlparse(candidate)
    resolved = urljoin(BASE_URL, candidate) if not parsed.scheme and not parsed.netloc else candidate
    base = urlparse(BASE_URL)
    target = urlparse(resolved)
    if (target.scheme, target.hostname, target.port) != (base.scheme, base.hostname, base.port):
        return None, (
            f"url_refused: {resolved!r} is outside the base origin "
            f"{base.scheme}://{base.netloc}; only the base origin is reachable"
        )
    return resolved, None


def safe_evidence_name(name: str) -> str:
    """Return a bare ``*.png`` file name for the evidence dir, refusing anything else."""
    bare = Path(name.strip()).name if isinstance(name, str) else ""
    if not bare or bare in (".", "..") or bare != (name.strip() if isinstance(name, str) else ""):
        raise ValueError("name must be a bare file name with no directory separators")
    if not bare.lower().endswith(".png"):
        bare += ".png"
    return bare


# --- The browser singleton ------------------------------------------------------------------
_BROWSER: Chrome | None = None
_BROWSER_LOCK = asyncio.Lock()


async def get_browser() -> Chrome:
    """Return the live browser, launching it on first use."""
    global _BROWSER
    if _BROWSER is None:
        browser = Chrome(
            BROWSER_BINARY, timeout=BROWSER_TIMEOUT_SECONDS, disable_javascript=DISABLE_JAVASCRIPT
        )
        await browser.start()
        _BROWSER = browser
    return _BROWSER


async def close_browser() -> None:
    """Close and forget the browser, so the next call starts a fresh one."""
    global _BROWSER
    if _BROWSER is not None:
        await _BROWSER.close()
        _BROWSER = None


@atexit.register
def _kill_browser_on_exit() -> None:
    """Kill a browser the process is still holding when the server exits without ``browser_close``."""
    if _BROWSER is not None:
        _BROWSER.kill_sync()


# --- Operations -----------------------------------------------------------------------------
@mcp.tool
async def browser_open(url: str, calling_role: str = "unknown") -> dict:
    """Open a base-origin path or URL and return its url, title, HTTP status and ok flag."""
    role = _authorize(calling_role, "browser_open")
    resolved, refusal = resolve_target(url)
    if refusal is not None:
        audit_event(
            "browser_open", allowed=True, ok=False, calling_role=role, target=url, reason=refusal
        )
        raise ValueError(refusal)
    assert resolved is not None  # the refusal and the URL are mutually exclusive
    async with _BROWSER_LOCK:
        browser = await get_browser()
        navigation = await browser.navigate(resolved)
    audit_event(
        "browser_open",
        allowed=True,
        ok=navigation["ok"],
        calling_role=role,
        target=resolved,
        reason=navigation["error"],
    )
    return {
        "url": resolved,
        "title": navigation["title"],
        "status": navigation["status"],
        "ok": navigation["ok"],
    }


@mcp.tool
async def browser_snapshot(calling_role: str = "unknown") -> dict:
    """Return a text-first view of the current page: url, title, text, links, fields and buttons."""
    role = _authorize(calling_role, "browser_snapshot")
    async with _BROWSER_LOCK:
        browser = await get_browser()
        state = await browser.page_state()
    audit_event(
        "browser_snapshot", allowed=True, ok=True, calling_role=role, target=state.get("url")
    )
    return state


@mcp.tool
async def browser_click(selector: str, calling_role: str = "unknown") -> dict:
    """Click the first element matching ``selector`` and report the url afterwards."""
    role = _authorize(calling_role, "browser_click")
    async with _BROWSER_LOCK:
        browser = await get_browser()
        clicked = await browser.click(selector)
        url_after = await browser.evaluate("location.href")
    audit_event(
        "browser_click",
        allowed=True,
        ok=clicked,
        calling_role=role,
        target=selector,
        reason=None if clicked else "no element matched the selector",
    )
    return {"clicked": bool(clicked), "selector": selector, "url_after": url_after}


@mcp.tool
async def browser_type(selector: str, text: str, calling_role: str = "unknown") -> dict:
    """Type ``text`` into the form field matching ``selector``."""
    role = _authorize(calling_role, "browser_type")
    async with _BROWSER_LOCK:
        browser = await get_browser()
        typed = await browser.type_text(selector, text)
    audit_event(
        "browser_type",
        allowed=True,
        ok=typed,
        calling_role=role,
        target=selector,
        reason=None if typed else "no element matched the selector",
    )
    return {"typed": bool(typed), "selector": selector}


@mcp.tool
async def browser_press(key: str, calling_role: str = "unknown") -> dict:
    """Press ``key`` on the focused element, for example ``Enter`` or ``Tab``."""
    role = _authorize(calling_role, "browser_press")
    async with _BROWSER_LOCK:
        browser = await get_browser()
        await browser.press(key)
    audit_event("browser_press", allowed=True, ok=True, calling_role=role, target=key)
    return {"pressed": key}


@mcp.tool
async def browser_diagnostics(calling_role: str = "unknown") -> dict:
    """Return, and drain, the console, page-error and request evidence the page produced."""
    role = _authorize(calling_role, "browser_diagnostics")
    async with _BROWSER_LOCK:
        browser = await get_browser()
        drained = browser.diagnostics()
    audit_event(
        "browser_diagnostics",
        allowed=True,
        ok=True,
        calling_role=role,
        reason=None,
    )
    return drained


@mcp.tool
async def browser_screenshot(name: str, calling_role: str = "unknown") -> dict:
    """Write a PNG of the current viewport under the evidence dir and return its path."""
    role = _authorize(calling_role, "browser_screenshot")
    try:
        file_name = safe_evidence_name(name)
    except ValueError as error:
        audit_event(
            "browser_screenshot",
            allowed=True,
            ok=False,
            calling_role=role,
            target=name,
            reason=str(error),
        )
        raise
    async with _BROWSER_LOCK:
        browser = await get_browser()
        png = await browser.screenshot()
    ensure_parent(str(Path(EVIDENCE_DIR) / file_name))
    path = Path(EVIDENCE_DIR) / file_name
    path.write_bytes(png)
    audit_event(
        "browser_screenshot", allowed=True, ok=True, calling_role=role, target=str(path)
    )
    return {"path": str(path)}


@mcp.tool
async def browser_close(calling_role: str = "unknown") -> dict:
    """Close the browser and release its profile directory."""
    role = _authorize(calling_role, "browser_close")
    async with _BROWSER_LOCK:
        await close_browser()
    audit_event("browser_close", allowed=True, ok=True, calling_role=role)
    return {"closed": True}


def build_app():
    """Build the Starlette application that serves the MCP endpoint at ``/mcp``."""
    return mcp.http_app(
        stateless_http=True,
        middleware=[
            Middleware(
                CORSMiddleware,
                allow_origins=["*"],
                allow_methods=["*"],
                allow_headers=["*"],
            )
        ],
    )


def main() -> None:
    """Parse the CLI flags and serve the browser server on the requested port."""
    global ALLOW_LIST, ALLOW_LIST_PATH, AUDIT_PATH, ROUTING_MAP_PATH, BASE_URL
    global BROWSER_BINARY, EVIDENCE_DIR, BROWSER_TIMEOUT_SECONDS, DISABLE_JAVASCRIPT

    parser = argparse.ArgumentParser(description="Browser MCP server for the beta tester")
    parser.add_argument("--port", type=int, default=8004, help="HTTP port (default 8004)")
    parser.add_argument("--host", default="0.0.0.0", help="bind address (default 0.0.0.0)")
    parser.add_argument("--base-url", default=BASE_URL, help=f"base origin (default {BASE_URL})")
    parser.add_argument("--binary", default=BROWSER_BINARY, help="Chromium binary")
    parser.add_argument("--evidence-dir", default=EVIDENCE_DIR, help="screenshot directory")
    parser.add_argument("--audit-path", default=AUDIT_PATH, help=f"audit journal (default {AUDIT_PATH})")
    parser.add_argument("--timeout-seconds", type=float, default=BROWSER_TIMEOUT_SECONDS)
    parser.add_argument(
        "--disable-javascript",
        action="store_true",
        default=DISABLE_JAVASCRIPT,
        help="start Chromium with page JavaScript disabled (the self-test's negative control)",
    )
    parser.add_argument(
        "--allowlist-path",
        default=str(ALLOW_LIST_PATH),
        help=f"role allow-list file (default {ALLOW_LIST_PATH})",
    )
    parser.add_argument(
        "--routing-map-path",
        default=str(ROUTING_MAP_PATH),
        help=f"routing map for the startup cross-check (default {ROUTING_MAP_PATH})",
    )
    args = parser.parse_args()

    BASE_URL = args.base_url
    BROWSER_BINARY = args.binary
    EVIDENCE_DIR = args.evidence_dir
    AUDIT_PATH = args.audit_path
    BROWSER_TIMEOUT_SECONDS = args.timeout_seconds
    DISABLE_JAVASCRIPT = bool(args.disable_javascript)
    ALLOW_LIST_PATH = Path(args.allowlist_path).expanduser()
    ROUTING_MAP_PATH = Path(args.routing_map_path).expanduser()
    try:
        ALLOW_LIST = load_allow_list(ALLOW_LIST_PATH)
        MAP_GRANTS, MAP_MENTIONS_BROWSER = load_routing_browser_grants(ROUTING_MAP_PATH)
        check_configuration_agrees(
            ALLOW_LIST, MAP_GRANTS, MAP_MENTIONS_BROWSER, ALLOW_LIST_PATH, ROUTING_MAP_PATH
        )
    except AllowListError as error:
        # Fail before the port is bound: a server with no allow-list answers every role.
        print(f"ERROR: {error}", file=sys.stderr, flush=True)
        raise SystemExit(2) from error
    ensure_parent(AUDIT_PATH)
    ensure_parent(str(Path(EVIDENCE_DIR) / "x"))
    print(f"browser base url: {BASE_URL}", flush=True)
    print(f"browser binary: {BROWSER_BINARY}", flush=True)
    print(f"browser evidence dir: {EVIDENCE_DIR}", flush=True)
    print(f"browser audit log: {AUDIT_PATH}", flush=True)
    print(f"browser allow-list: {ALLOW_LIST_PATH}", flush=True)
    print(f"browser routing map: {ROUTING_MAP_PATH}", flush=True)
    print(f"browser grants: {json.dumps(ALLOW_LIST, sort_keys=True)}", flush=True)

    import uvicorn

    uvicorn.run(build_app(), host=args.host, port=args.port)


if __name__ == "__main__":
    main()
