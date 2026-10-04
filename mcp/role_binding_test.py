#!/usr/bin/env python3
"""Role-binding test for the storage, retrieval and gate MCP servers (the AGENT_ROLE guard).

The three servers bind every caller to the container's own ``AGENT_ROLE``: a ``calling_role``
argument that disagrees with it is refused outright, an omitted or ``unknown`` argument yields the
environment role, and with ``AGENT_ROLE`` unset the server behaves exactly as it did before the
guard existed. This test drives the servers **in process**, because the environment variable being
tested is process state: the same interpreter must set it before each call.

Run inside the tools image, where the MCP dependencies are real:

    docker run --rm -v <repo>:/w -w /w -e AGENT_ROLE=project-manager \
        agent-sandbox:komun-m3 python3 mcp/role_binding_test.py

It covers, per server:
  (a) AGENT_ROLE set, the argument matches it  -> the call is authorised
  (b) AGENT_ROLE set, the argument disagrees    -> refused, and the refusal is journalled
  (c) AGENT_ROLE set, the argument is omitted    -> the environment role is used
  (d) AGENT_ROLE unset                           -> today's behaviour is unchanged

Plain ``python3`` on purpose: no pytest is required inside the image. Exit code 0 when every
check passes, 1 otherwise; the last line is machine-readable.
"""

from __future__ import annotations

import asyncio
import importlib.util
import json
import os
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
async def test_storage(storage: Any) -> None:
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
def test_retrieval(retrieval: Any) -> None:
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
def test_gate(gate: Any) -> None:
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

    # (d) AGENT_ROLE unset: today's behaviour -- no authorisation runs at all.
    set_role(None)
    for tool, call, needle in (
        ("run_gate", lambda: gate.run_gate("definitely-not-a-gate", calling_role="anon"),
         "not an allowlisted gate"),
        ("run_fix", lambda: gate.run_fix("fmt", calling_role="anon"),
         "not an allowlisted write-mode command"),
    ):
        try:
            call()
            check(f"gate (d) AGENT_ROLE unset + {tool} -> no authorisation, today's refusal",
                  False, "the call was allowed")
        except Exception as error:  # noqa: BLE001
            check(f"gate (d) AGENT_ROLE unset + {tool} -> no authorisation, today's refusal",
                  not is_denied(error) and needle in str(error), str(error)[:200])


async def main() -> int:
    print(f"role-binding test at {SCRATCH}  primary role={PRIMARY!r}")
    print(f"bound AGENT_ROLE from the container at start: {os.environ.get('AGENT_ROLE')!r}")
    storage = load_module("rb_storage", REPO / "mcp" / "storage" / "server.py")
    retrieval = load_module("rb_retrieval", REPO / "mcp" / "retrieval" / "server.py")
    gate = load_module("rb_gate", REPO / "mcp" / "gate" / "server.py")

    await test_storage(storage)
    test_retrieval(retrieval)
    test_gate(gate)

    passed = sum(1 for _, ok, _ in RESULTS if ok)
    total = len(RESULTS)
    print()
    for name, ok, detail in RESULTS:
        if not ok:
            print(f"FAILED: {name} -- {detail}")
    print(f"ROLE_BINDING_RESULT passed={passed} total={total}")
    return 0 if passed == total else 1


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
