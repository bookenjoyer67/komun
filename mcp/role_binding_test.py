#!/usr/bin/env python3
"""Role-binding test for the storage, retrieval, gate and browser MCP servers (the AGENT_ROLE guard).

The four servers bind every caller to the container's own ``AGENT_ROLE``: a ``calling_role``
argument that disagrees with it is refused outright, an omitted or ``unknown`` argument yields the
environment role, and with ``AGENT_ROLE`` unset each server checks the declared role against its own
grants. This test drives the servers **in process**, because the environment variable being tested
is process state: the same interpreter must set it before each call.

Run inside the tools image, where the MCP dependencies are real:

    docker run --rm -v <repo>:/w -w /w -e AGENT_ROLE=project-manager \
        agent-sandbox:komun-m3 python3 mcp/role_binding_test.py

It covers, per server:
  (a) AGENT_ROLE set, the argument matches it  -> the call is authorised
  (b) AGENT_ROLE set, the argument disagrees    -> refused, and the refusal is journalled
  (c) AGENT_ROLE set, the argument is omitted    -> the environment role is used
  (d) AGENT_ROLE unset                           -> the declared role is checked against the grants

and, for the gate and the browser, T-G1..T-G9 and T-B1..T-B5: unbound refusals by declared role, and
the resolved role plus the ``bound`` key on every journal row.

Plain ``python3`` on purpose: no pytest is required inside the image. Exit code 0 when every
check passes, 1 otherwise; the last line is machine-readable.
"""

from __future__ import annotations

import asyncio
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[1]
SCRATCH = Path(os.environ.get("ROLE_BINDING_TEST_DIR") or tempfile.mkdtemp(prefix="role-binding-"))
MEMORY = SCRATCH / "memory"
MEMORY.mkdir(parents=True, exist_ok=True)

# Runtime paths, set before the modules are imported because each reads them at import time. The
# database and the three journals land in a throwaway directory, never in the repository.
os.environ["MEMORY_DIR"] = str(MEMORY)
os.environ["STORAGE_DB_PATH"] = str(SCRATCH / "storage.db")
os.environ["STORAGE_AUDIT_PATH"] = str(SCRATCH / "storage-audit.log")
os.environ["RETRIEVAL_AUDIT_PATH"] = str(SCRATCH / "retrieval-audit.log")
os.environ["GATE_AUDIT_PATH"] = str(SCRATCH / "gate-audit.log")
os.environ["BROWSER_AUDIT_PATH"] = str(SCRATCH / "browser-audit.log")
# The primary role the escalation cases are bound to. The caller passes it as ``-e AGENT_ROLE``;
# the test sets it explicitly before each call anyway, so the guard is exercised deliberately.
PRIMARY = (os.environ.get("AGENT_ROLE") or "project-manager").strip() or "project-manager"

PROJECT = "proj-komun"

RESULTS: list[tuple[str, bool, str]] = []


def check(name: str, passed: bool, detail: str = "") -> bool:
    RESULTS.append((name, passed, detail))
    print(f"[{'PASS' if passed else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""), flush=True)
    return passed


def set_role(role: str | None) -> None:
    """Bind this process to ``role``, or unset the switch when ``role`` is None."""
    if role is None:
        os.environ.pop("AGENT_ROLE", None)
    else:
        os.environ["AGENT_ROLE"] = role


def load_module(name: str, path: Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def journal(path: Path) -> list[dict]:
    if not path.is_file():
        return []
    records = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return records


def denial_in(path: Path, *, role: str, needle: str = "authorization_denied") -> dict | None:
    for record in reversed(journal(path)):
        if record.get("calling_role") == role and record.get("allowed") is False and needle in (
            record.get("reason") or ""
        ):
            return record
    return None


def is_denied(error: Exception) -> bool:
    return type(error).__name__ == "AuthorizationDenied"


# --- storage ----------------------------------------------------------------------------------
async def check_storage(storage: Any) -> None:
    # (a) AGENT_ROLE set and the argument matches it: authorised.
    set_role("implementer")
    try:
        rows = await storage.list_entries(PROJECT, calling_role="implementer")
        check("storage (a) AGENT_ROLE=implementer + calling_role=implementer -> authorised",
              isinstance(rows, list), f"rows={len(rows)}")
    except Exception as error:  # noqa: BLE001 - the assertion is the check
        check("storage (a) AGENT_ROLE=implementer + calling_role=implementer -> authorised",
              False, f"{type(error).__name__}: {error}")

    # (b) AGENT_ROLE set and the argument disagrees: refused outright, and journalled.
    set_role(PRIMARY)
    task = storage.read_entry(PROJECT, "0f6b7b1e-0000-4000-8000-000000000000", calling_role="planner")
    try:
        await task
        check(f"storage (b) AGENT_ROLE={PRIMARY} + calling_role=planner -> REFUSED", False,
              "the call was allowed")
    except Exception as error:  # noqa: BLE001
        text = f"{type(error).__name__}: {error}"
        check(f"storage (b) AGENT_ROLE={PRIMARY} + calling_role=planner -> REFUSED, names the mismatch",
              is_denied(error) and "disagrees" in str(error) and "planner" in str(error)
              and PRIMARY in str(error), text[:240])
    record = denial_in(Path(os.environ["STORAGE_AUDIT_PATH"]), role=PRIMARY)
    check("storage (b) the refusal is journalled, naming the bound role and the mismatch",
          bool(record) and "disagrees" in (record["reason"] if record else ""),
          json.dumps(record, sort_keys=True)[:240] if record else "no denial record")

    # (c) AGENT_ROLE set and the argument omitted: the environment role is used.
    set_role("project-manager")
    try:
        rows = await storage.list_entries(PROJECT)  # no calling_role
        check("storage (c) AGENT_ROLE=project-manager + no argument -> the environment role is used",
              isinstance(rows, list), f"rows={len(rows)}")
    except Exception as error:  # noqa: BLE001
        check("storage (c) AGENT_ROLE=project-manager + no argument -> the environment role is used",
              False, f"{type(error).__name__}: {error}")

    # (d) AGENT_ROLE unset: today's behaviour, enforced by the argument alone.
    set_role(None)
    try:
        rows = await storage.list_entries(PROJECT, calling_role="implementer")
        allowed_ok = isinstance(rows, list)
        detail = f"rows={len(rows)}"
    except Exception as error:  # noqa: BLE001
        allowed_ok, detail = False, f"{type(error).__name__}: {error}"
    check("storage (d) AGENT_ROLE unset + calling_role=implementer -> today's allow-list still governs",
          allowed_ok, detail)
    try:
        await storage.read_entry(PROJECT, "0f6b7b1e-0000-4000-8000-000000000000")
        check("storage (d) AGENT_ROLE unset + no argument -> refused as 'unknown', as before",
              False, "the call was allowed")
    except Exception as error:  # noqa: BLE001
        check("storage (d) AGENT_ROLE unset + no argument -> refused as 'unknown', as before",
              is_denied(error) and "unknown role" in str(error), str(error)[:200])


# --- retrieval --------------------------------------------------------------------------------
def check_retrieval(retrieval: Any) -> None:
    kwargs = {"project_id": PROJECT, "requested_ceiling": "internal", "query": "cost of hosting"}

    # (a) AGENT_ROLE set and the argument matches it: authorised (the guard returns the role).
    set_role("implementer")
    try:
        role = retrieval._authorize("implementer", "retrieve", **kwargs)
        check("retrieval (a) AGENT_ROLE=implementer + calling_role=implementer -> authorised",
              role == "implementer", f"role={role!r}")
    except Exception as error:  # noqa: BLE001
        check("retrieval (a) AGENT_ROLE=implementer + calling_role=implementer -> authorised",
              False, f"{type(error).__name__}: {error}")

    # (b) AGENT_ROLE set and the argument disagrees: refused, and journalled.
    set_role(PRIMARY)
    try:
        retrieval.retrieve("cost of hosting", PROJECT, top_k=3,
                           classification_ceiling="confidential", calling_role="planner")
        check(f"retrieval (b) AGENT_ROLE={PRIMARY} + calling_role=planner (ceiling=confidential) "
              "-> REFUSED", False, "the call was allowed")
    except Exception as error:  # noqa: BLE001
        text = f"{type(error).__name__}: {error}"
        check(f"retrieval (b) AGENT_ROLE={PRIMARY} + calling_role=planner (ceiling=confidential) "
              "-> REFUSED, names the mismatch",
              is_denied(error) and "disagrees" in str(error) and "planner" in str(error)
              and PRIMARY in str(error), text[:240])
    # the ceiling is read for the bound role: the journalled denial carries the bound role, never planner
    records = [r for r in journal(Path(os.environ["RETRIEVAL_AUDIT_PATH"]))
               if r.get("calling_role") == PRIMARY and r.get("decision") == "denied"]
    check("retrieval (b) the denial is journalled under the BOUND role, not the argument",
          bool(records) and all(r.get("calling_role") != "planner" for r in records),
          json.dumps(records[-1], sort_keys=True)[:240] if records else "no denied record")

    # (c) AGENT_ROLE set and the argument omitted: the environment role is used.
    set_role("implementer")
    try:
        role = retrieval._authorize(None, "retrieve", **kwargs)
        check("retrieval (c) AGENT_ROLE=implementer + no argument -> the environment role is used",
              role == "implementer", f"role={role!r}")
    except Exception as error:  # noqa: BLE001
        check("retrieval (c) AGENT_ROLE=implementer + no argument -> the environment role is used",
              False, f"{type(error).__name__}: {error}")

    # (d) AGENT_ROLE unset: today's behaviour, enforced by the argument alone.
    set_role(None)
    try:
        role = retrieval._authorize("implementer", "retrieve", **kwargs)
        allowed_ok, detail = role == "implementer", f"role={role!r}"
    except Exception as error:  # noqa: BLE001
        allowed_ok, detail = False, f"{type(error).__name__}: {error}"
    check("retrieval (d) AGENT_ROLE unset + calling_role=implementer -> allow-list still governs",
          allowed_ok, detail)
    try:
        retrieval._authorize("unknown", "retrieve", **kwargs)
        check("retrieval (d) AGENT_ROLE unset + unknown role -> refused, as before",
              False, "the call was allowed")
    except Exception as error:  # noqa: BLE001
        check("retrieval (d) AGENT_ROLE unset + unknown role -> refused, as before",
              is_denied(error) and "unknown role" in str(error), str(error)[:200])


# --- gate -------------------------------------------------------------------------------------
def check_gate(gate: Any) -> None:
    # (a) AGENT_ROLE set and the argument matches it: authorised, so the name check is what refuses.
    set_role("tester")
    try:
        gate.run_gate("definitely-not-a-gate", calling_role="tester")
        check("gate (a) AGENT_ROLE=tester + calling_role=tester -> authorised (name check refuses)",
              False, "an unallowlisted name was accepted")
    except Exception as error:  # noqa: BLE001
        check("gate (a) AGENT_ROLE=tester + calling_role=tester -> authorised, then name refused",
              not is_denied(error) and "not an allowlisted gate" in str(error), str(error)[:200])
    set_role("implementer")
    try:
        gate.run_fix("test", calling_role="implementer")
        check("gate (a) AGENT_ROLE=implementer + calling_role=implementer -> authorised on run_fix",
              False, "a check-mode name was accepted")
    except Exception as error:  # noqa: BLE001
        check("gate (a) AGENT_ROLE=implementer + calling_role=implementer -> authorised, mode refused",
              not is_denied(error) and "not an allowlisted write-mode command" in str(error),
              str(error)[:200])

    # (b) AGENT_ROLE set and the argument disagrees: refused outright, and journalled.
    set_role(PRIMARY)
    try:
        gate.run_gate("test", calling_role="tester")
        check(f"gate (b) AGENT_ROLE={PRIMARY} + run_gate(calling_role=tester) -> REFUSED", False,
              "the call was allowed")
    except Exception as error:  # noqa: BLE001
        check(f"gate (b) AGENT_ROLE={PRIMARY} + run_gate(calling_role=tester) -> REFUSED, names the mismatch",
              is_denied(error) and "disagrees" in str(error) and "tester" in str(error)
              and PRIMARY in str(error), str(error)[:240])
    record = denial_in(Path(os.environ["GATE_AUDIT_PATH"]), role=PRIMARY)
    check("gate (b) the refusal is journalled with allowed=false and the bound role",
          bool(record) and record.get("allowed") is False and record.get("tool") == "run_gate",
          json.dumps(record, sort_keys=True)[:240] if record else "no denial record")

    # (c) AGENT_ROLE set and the argument omitted: the environment role is used.
    set_role("tester")
    try:
        gate.run_gate("definitely-not-a-gate")  # no calling_role
        check("gate (c) AGENT_ROLE=tester + no argument -> the environment role is used",
              False, "an unallowlisted name was accepted")
    except Exception as error:  # noqa: BLE001
        check("gate (c) AGENT_ROLE=tester + no argument -> the environment role is used (name refused)",
              not is_denied(error) and "not an allowlisted gate" in str(error), str(error)[:200])

    # (d) AGENT_ROLE unset: the declared role meets the same grants, so an unknown one is refused.
    # Neither name is runnable by its tool, so a guard that let "anon" through still runs nothing.
    set_role(None)
    for tool, call in (
        ("run_gate", lambda: gate.run_gate("definitely-not-a-gate", calling_role="anon")),
        ("run_fix", lambda: gate.run_fix("fmt", calling_role="anon")),
    ):
        name = f"gate (d) AGENT_ROLE unset + {tool}(calling_role='anon') -> REFUSED, 'unknown role'"
        try:
            call()
            check(name, False, "the call was allowed")
        except Exception as error:  # noqa: BLE001
            check(name, is_denied(error) and "unknown role" in str(error), str(error)[:200])


# Every name here fails validation, so code that skips authorisation refuses by name and runs nothing.
GATE_REFUSED_NAME = {"run_gate": "definitely-not-a-gate", "run_fix": "definitely-not-a-command"}
GATE_NAME_REFUSAL = {"run_gate": "not an allowlisted gate",
                     "run_fix": "not an allowlisted write-mode command"}


def check_gate_unbound(gate: Any) -> None:
    path = Path(os.environ["GATE_AUDIT_PATH"])
    set_role(None)

    # T-G1, T-G2: a declared role holding the grant still reaches the name check unbound.
    for case, tool, declared in (("T-G1", "run_gate", "tester"), ("T-G2", "run_fix", "implementer")):
        name = f"gate {case} AGENT_ROLE unset + {tool}(calling_role={declared!r}) -> authorised, name refused"
        try:
            getattr(gate, tool)(GATE_REFUSED_NAME[tool], calling_role=declared)
            check(name, False, "an unallowlisted name was accepted")
        except Exception as error:  # noqa: BLE001
            check(name, not is_denied(error) and GATE_NAME_REFUSAL[tool] in str(error),
                  f"{type(error).__name__}: {error}"[:240])

    # T-G3..T-G6 refuse by authorisation; T-G7 requires exactly one denial row for each.
    for case, tool, declared, journalled, needle in (
        ("T-G3", "run_fix", "janitor", "janitor", "unknown role"),
        ("T-G4", "run_fix", None, "unknown", "unknown role"),
        ("T-G5", "run_fix", "tester", "tester", "is not granted 'run_fix'"),
        ("T-G6", "run_gate", "reviewer", "reviewer", "is not granted 'run_gate'"),
    ):
        name = f"gate {case} AGENT_ROLE unset + {tool}(calling_role={declared!r}) -> REFUSED, {needle!r}"
        before = len(journal(path))
        try:
            getattr(gate, tool)(GATE_REFUSED_NAME[tool], calling_role=declared)
            check(name, False, "the call was allowed")
        except Exception as error:  # noqa: BLE001
            check(name, is_denied(error) and needle in str(error),
                  f"{type(error).__name__}: {error}"[:240])
        added = journal(path)[before:]
        row = added[0] if len(added) == 1 else None
        check(f"gate T-G7 ({case}) one denial row: allowed=false, bound=false, "
              f"calling_role={journalled!r}",
              row is not None and row.get("allowed") is False and row.get("bound") is False
              and row.get("calling_role") == journalled and row.get("tool") == tool,
              json.dumps(added, sort_keys=True)[:240] if added else "no row written")


class StubExecutor:
    """Replaces ``execute_gate`` so the attribution cases journal a row while no command runs."""

    def __init__(self) -> None:
        self.tables: list[Any] = []

    def __call__(self, command: str, table: Any, timeout_seconds: int) -> dict[str, Any]:
        self.tables.append(table)
        return {
            "gate": command,
            "argv": ["stub"],
            "exit_code": 0,
            "passed": True,
            "verdict": "pass",
            "timed_out": False,
            "timeout_seconds": timeout_seconds,
            "duration_seconds": 0.0,
            "guard": {"applied": False, "satisfied": True},
            "summary": None,
        }


def last_row_after(path: Path, before: int) -> dict | None:
    added = journal(path)[before:]
    return added[-1] if added else None


def check_gate_attribution(gate: Any) -> None:
    path = Path(os.environ["GATE_AUDIT_PATH"])
    stub = StubExecutor()
    real = gate.execute_gate
    gate.execute_gate = stub
    try:
        # T-G8a: bound, no argument -> the row names the bound role.
        set_role("tester")
        before = len(journal(path))
        try:
            gate.run_gate("fmt")
            row, detail = last_row_after(path, before), ""
        except Exception as error:  # noqa: BLE001
            row, detail = None, f"{type(error).__name__}: {error}"
        check("gate T-G8a AGENT_ROLE=tester + run_gate(fmt), no argument -> row calling_role=tester, "
              "bound=true",
              row is not None and row.get("tool") == "run_gate" and row.get("calling_role") == "tester"
              and row.get("bound") is True,
              detail or json.dumps(row, sort_keys=True)[:240])

        # T-G8b: bound, no argument, write mode -> the stub got the write table, the row the role.
        set_role("implementer")
        before = len(journal(path))
        try:
            gate.run_fix("fmt-fix")
            row, detail = last_row_after(path, before), ""
        except Exception as error:  # noqa: BLE001
            row, detail = None, f"{type(error).__name__}: {error}"
        check("gate T-G8b AGENT_ROLE=implementer + run_fix(fmt-fix), no argument -> FIX_COMMANDS, "
              "row calling_role=implementer, writes=true, bound=true",
              bool(stub.tables) and stub.tables[-1] is gate.FIX_COMMANDS and row is not None
              and row.get("tool") == "run_fix" and row.get("calling_role") == "implementer"
              and row.get("writes") is True and row.get("bound") is True,
              detail or json.dumps(row, sort_keys=True)[:240])

        # T-G8c: unbound, declared and granted -> the row is marked unbound.
        set_role(None)
        before = len(journal(path))
        try:
            gate.run_gate("fmt", calling_role="tester")
            row, detail = last_row_after(path, before), ""
        except Exception as error:  # noqa: BLE001
            row, detail = None, f"{type(error).__name__}: {error}"
        check("gate T-G8c AGENT_ROLE unset + run_gate(fmt, calling_role=tester) -> row "
              "calling_role=tester, bound=false",
              row is not None and row.get("calling_role") == "tester" and row.get("bound") is False,
              detail or json.dumps(row, sort_keys=True)[:240])

        # T-G9: rows written before "bound" existed and rows carrying it verify as one chain.
        hashchain = load_module("rb_hashchain", REPO / "mcp" / "hashchain.py")
        compat = SCRATCH / "gate-chain-compat.log"
        hashchain.append_journal_record(compat, {
            "timestamp": "2026-10-01T00:00:00+00:00", "tool": "run_gate", "gate": "fmt",
            "argv": ["stub"], "exit_code": 0, "duration_seconds": 0.0, "passed": True,
            "timed_out": False, "guard_applied": False, "guard_satisfied": True, "summary": None,
            "writes": False, "calling_role": "tester",
        })
        saved = gate.AUDIT_PATH
        gate.AUDIT_PATH = str(compat)
        try:
            set_role(None)
            try:
                gate.run_gate("fmt", calling_role="tester")
                detail = ""
            except Exception as error:  # noqa: BLE001
                detail = f"{type(error).__name__}: {error}"
        finally:
            gate.AUDIT_PATH = saved
        rows = journal(compat)
        verdict = hashchain.verify_journal(compat)
        check("gate T-G9 a row without 'bound' then a row with bound=false -> chain INTACT",
              verdict.get("status") == "INTACT" and len(rows) == 2 and "bound" not in rows[0]
              and rows[1].get("bound") is False,
              detail or json.dumps({"verify": verdict, "rows": rows}, sort_keys=True)[:240])
    finally:
        gate.execute_gate = real


# --- browser ----------------------------------------------------------------------------------
async def check_browser(browser: Any) -> None:
    # (a) AGENT_ROLE set and the argument matches it: authorised (the guard returns the role).
    set_role("beta-tester")
    try:
        role = browser._authorize("beta-tester", "browser_open")
        check("browser (a) AGENT_ROLE=beta-tester + calling_role=beta-tester -> authorised",
              role == "beta-tester", f"role={role!r}")
    except Exception as error:  # noqa: BLE001
        check("browser (a) AGENT_ROLE=beta-tester + calling_role=beta-tester -> authorised",
              False, f"{type(error).__name__}: {error}")

    # (b) AGENT_ROLE set and the argument disagrees: refused, and journalled.
    set_role(PRIMARY)
    try:
        browser._authorize("beta-tester", "browser_open")
        check(f"browser (b) AGENT_ROLE={PRIMARY} + calling_role=beta-tester -> REFUSED", False,
              "the call was allowed")
    except Exception as error:  # noqa: BLE001
        text = f"{type(error).__name__}: {error}"
        check(f"browser (b) AGENT_ROLE={PRIMARY} + calling_role=beta-tester -> REFUSED, names the mismatch",
              is_denied(error) and "disagrees" in str(error) and "beta-tester" in str(error)
              and PRIMARY in str(error), text[:240])
    record = denial_in(Path(os.environ["BROWSER_AUDIT_PATH"]), role=PRIMARY)
    check("browser (b) the refusal is journalled with allowed=false and the bound role",
          bool(record) and record.get("allowed") is False and record.get("tool") == "browser_open",
          json.dumps(record, sort_keys=True)[:240] if record else "no denial record")

    # (c) AGENT_ROLE set and the argument omitted: the environment role is used.
    set_role("beta-tester")
    try:
        role = browser._authorize(None, "browser_open")
        check("browser (c) AGENT_ROLE=beta-tester + no argument -> the environment role is used",
              role == "beta-tester", f"role={role!r}")
    except Exception as error:  # noqa: BLE001
        check("browser (c) AGENT_ROLE=beta-tester + no argument -> the environment role is used",
              False, f"{type(error).__name__}: {error}")

    # (d) AGENT_ROLE unset: today's behaviour, enforced by the allow-list alone.
    set_role(None)
    try:
        role = browser._authorize("beta-tester", "browser_open")
        allowed_ok, detail = role == "beta-tester", f"role={role!r}"
    except Exception as error:  # noqa: BLE001
        allowed_ok, detail = False, f"{type(error).__name__}: {error}"
    check("browser (d) AGENT_ROLE unset + calling_role=beta-tester -> the allow-list still governs",
          allowed_ok, detail)
    try:
        browser._authorize("unknown", "browser_open")
        check("browser (d) AGENT_ROLE unset + unknown role -> refused, as before",
              False, "the call was allowed")
    except Exception as error:  # noqa: BLE001
        check("browser (d) AGENT_ROLE unset + unknown role -> refused, as before",
              is_denied(error) and "unknown role" in str(error), str(error)[:200])

    path = Path(os.environ["BROWSER_AUDIT_PATH"])

    # T-B1, T-B2 are refused unbound; T-B3 requires their denial rows to read bound=false.
    set_role(None)
    for case, declared, needle in (("T-B1", "tester", "is not granted"),
                                   ("T-B2", "janitor", "unknown role")):
        name = f"browser {case} AGENT_ROLE unset + calling_role={declared} -> REFUSED, {needle!r}"
        before = len(journal(path))
        try:
            browser._authorize(declared, "browser_open")
            check(name, False, "the call was allowed")
        except Exception as error:  # noqa: BLE001
            check(name, is_denied(error) and needle in str(error),
                  f"{type(error).__name__}: {error}"[:240])
        row = last_row_after(path, before)
        check(f"browser T-B3 ({case}) the denial row reads bound=false",
              row is not None and row.get("allowed") is False and row.get("calling_role") == declared
              and row.get("bound") is False,
              json.dumps(row, sort_keys=True)[:240] if row else "no denial row")

    # browser_close needs no Chromium while no browser is open. A fastmcp release that wraps its
    # tools keeps the coroutine function on ``.fn``.
    close = getattr(browser.browser_close, "fn", browser.browser_close)
    for case, bound_role, argument, bound in (("T-B4", "beta-tester", None, True),
                                              ("T-B5", None, "beta-tester", False)):
        set_role(bound_role)
        before = len(journal(path))
        try:
            await (close() if argument is None else close(calling_role=argument))
            row, detail = last_row_after(path, before), ""
        except Exception as error:  # noqa: BLE001
            row, detail = None, f"{type(error).__name__}: {error}"
        check(f"browser {case} AGENT_ROLE={bound_role} + browser_close(calling_role={argument!r}) "
              f"-> row calling_role=beta-tester, bound={str(bound).lower()}",
              row is not None and row.get("allowed") is True and row.get("tool") == "browser_close"
              and row.get("calling_role") == "beta-tester" and row.get("bound") is bound,
              detail or json.dumps(row, sort_keys=True)[:240])


async def main() -> int:
    print(f"role-binding test at {SCRATCH}  primary role={PRIMARY!r}")
    print(f"bound AGENT_ROLE from the container at start: {os.environ.get('AGENT_ROLE')!r}")
    storage = load_module("rb_storage", REPO / "mcp" / "storage" / "server.py")
    retrieval = load_module("rb_retrieval", REPO / "mcp" / "retrieval" / "server.py")
    gate = load_module("rb_gate", REPO / "mcp" / "gate" / "server.py")
    browser = load_module("rb_browser", REPO / "mcp" / "browser" / "server.py")

    await check_storage(storage)
    check_retrieval(retrieval)
    check_gate(gate)
    check_gate_unbound(gate)
    check_gate_attribution(gate)
    await check_browser(browser)

    passed = sum(1 for _, ok, _ in RESULTS if ok)
    total = len(RESULTS)
    print()
    for name, ok, detail in RESULTS:
        if not ok:
            print(f"FAILED: {name} -- {detail}")
    print(f"ROLE_BINDING_RESULT passed={passed} total={total}")
    return 0 if passed == total else 1


def test_role_binding_script_runs() -> None:
    """Run the whole check as a script, so `pytest mcp/role_binding_test.py` exercises it."""
    completed = subprocess.run(
        [sys.executable, str(Path(__file__).resolve())],
        capture_output=True, text=True, check=False, timeout=600,
    )
    assert completed.returncode == 0, completed.stdout[-4000:] + completed.stderr[-4000:]


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
