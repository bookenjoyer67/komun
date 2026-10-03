#!/usr/bin/env python3
"""The workflow's spend ledger, and the two ceilings a run is bounded by.

`scripts/run-agent.sh` reads `budgets.per_call_seconds` and `budgets.per_workflow_usd` from
`agentic.config.json` and calls this script twice per call: once with `check` before the call starts,
and once with `record` after it returns. The ceilings live in the seam table so that a fork changes
them in one place, and the ledger lives under `target/`, which is gitignored, because a running spend
total is state rather than source.

Why a refusal rather than a warning: a budget that only warns is a budget nobody meets. `check`
exits 3, which the launcher passes straight through, so a workflow that has spent its ceiling cannot
start another call until a human resets the ledger or raises the ceiling on purpose.

The dollar figure comes from the CLI that spent it -- the same accounting `docs/calibration-log.md`
cites -- and the caller passes it as `BUDGET_CALL_USD`. A call that reports no figure still records
its wall-clock seconds and its exit status, so the ledger is a complete record of the workflow even
when the spend is not attributable.

Usage:
    python3 scripts/budget.py check  --ledger target/budget-ledger.json --ceiling 25 --role tester
    python3 scripts/budget.py record --ledger target/budget-ledger.json --role tester \
                                     --seconds 412 --status 0 --usd 1.83
    python3 scripts/budget.py show   --ledger target/budget-ledger.json
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from datetime import datetime, timezone
from typing import Any

EXIT_OVER_CEILING = 3


def _empty() -> dict[str, Any]:
    return {"total_usd": 0.0, "calls": []}


def load(path: str) -> dict[str, Any]:
    """The ledger, or an empty one. An unreadable ledger is empty rather than fatal.

    A ledger that cannot be read is treated as a workflow that has spent nothing, so a corrupt or
    absent file blocks no work. The alternative -- refusing every call because a state file is
    malformed -- turns a bookkeeping fault into an outage.
    """
    try:
        with open(path, encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, ValueError):
        return _empty()
    if not isinstance(data, dict):
        return _empty()
    total = data.get("total_usd")
    calls = data.get("calls")
    data["total_usd"] = float(total) if isinstance(total, (int, float)) else 0.0
    data["calls"] = calls if isinstance(calls, list) else []
    return data


def save(path: str, data: dict[str, Any]) -> None:
    parent = os.path.dirname(os.path.abspath(path))
    os.makedirs(parent, exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")


def check(ledger: str, ceiling: float, role: str) -> int:
    """Refuse when the workflow has already spent its ceiling. Zero lets the call through."""
    data = load(ledger)
    spent = float(data["total_usd"])
    calls = len(data["calls"])
    if ceiling and spent >= ceiling:
        print(
            f"budget refused: {role} may not start a call. The workflow has spent "
            f"${spent:.4f} of its ${ceiling:.2f} ceiling over {calls} calls.",
            file=sys.stderr,
        )
        print(f"  ledger : {ledger}", file=sys.stderr)
        print(f"  reset  : rm -f {ledger}", file=sys.stderr)
        print(
            "  or raise budgets.per_workflow_usd in agentic.config.json, which is a deliberate act",
            file=sys.stderr,
        )
        return EXIT_OVER_CEILING
    return 0


def record(ledger: str, role: str, seconds: int, status: int, usd: float) -> int:
    """Append one call to the ledger and rewrite the running total."""
    data = load(ledger)
    calls = data["calls"]
    calls.append(
        {
            "role": role,
            "usd": round(float(usd), 6),
            "seconds": int(seconds),
            "status": int(status),
            "at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        }
    )
    data["total_usd"] = round(float(data["total_usd"]) + float(usd), 6)
    save(ledger, data)
    return 0


def show(ledger: str) -> int:
    print(json.dumps(load(ledger), indent=2, sort_keys=True))
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="The workflow's spend ledger.")
    parser.add_argument("action", choices=["check", "record", "show"])
    parser.add_argument("--ledger", required=True, help="path to the ledger file")
    parser.add_argument("--ceiling", type=float, default=0.0, help="per-workflow ceiling in USD")
    parser.add_argument("--role", default="", help="the role this call ran as")
    parser.add_argument("--usd", type=float, default=0.0, help="what this call cost, from the CLI")
    parser.add_argument("--seconds", type=int, default=0, help="this call's wall clock")
    parser.add_argument("--status", type=int, default=0, help="this call's exit status")
    args = parser.parse_args(argv)

    if args.action == "check":
        return check(args.ledger, args.ceiling, args.role)
    if args.action == "record":
        return record(args.ledger, args.role, args.seconds, args.status, args.usd)
    return show(args.ledger)


if __name__ == "__main__":
    raise SystemExit(main())
