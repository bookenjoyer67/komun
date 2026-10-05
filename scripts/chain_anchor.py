#!/usr/bin/env python3
"""Read-only operator command for the hash chains over the store and the three MCP journals.

    python3 scripts/chain_anchor.py head [--memory-dir DIR]
    python3 scripts/chain_anchor.py verify --artifact NAME --expected SEQ:HEAD [--memory-dir DIR]

``head`` prints, for every artifact, the current ``seq``, ``head`` and the ``anchor`` string
``SEQ:HEAD``. Keep that string outside the container. ``verify`` recomputes one artifact's chain and
checks it against an anchor kept earlier. It prints the verdict as JSON and exits 0 when the chain
is INTACT and 1 on FAIL, naming the first record that disagrees. It exits 2 on a usage or I/O error.

There is no write mode. Journals are read as bytes, and the store is opened with SQLite
``mode=ro`` and never reopened read-write, so a run leaves every artifact exactly as it found it.
No agent role is granted this command; the operator runs it on the host.
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from pathlib import Path
from typing import Any, Callable

REPO = Path(__file__).resolve().parents[1]
if str(REPO / "mcp") not in sys.path:
    sys.path.insert(0, str(REPO / "mcp"))
import hashchain  # noqa: E402

DEFAULT_MEMORY_DIR = REPO / ".memory"
ARTIFACTS: dict[str, tuple[str, str]] = {
    "store": ("storage.db", "store"),
    "storage-journal": ("storage-audit.log", "journal"),
    "retrieval-journal": ("retrieval-audit.log", "journal"),
    "gate-journal": ("gate-audit.log", "journal"),
}
EXIT_INTACT, EXIT_FAIL, EXIT_ERROR = 0, 1, 2


def artifact_path(memory_dir: Path, name: str) -> Path:
    return memory_dir / ARTIFACTS[name][0]


def head_of(memory_dir: Path, name: str) -> dict[str, Any]:
    path = artifact_path(memory_dir, name)
    reader = hashchain.store_head if ARTIFACTS[name][1] == "store" else hashchain.journal_head
    head = reader(path)
    return {**head, "path": str(path), "anchor": f"{head['seq']}:{head['head']}"}


def parse_anchor(text: str) -> tuple[int, str]:
    seq_text, separator, head = text.partition(":")
    if not separator or not seq_text.isdigit() or not hashchain.is_head(head):
        raise ValueError(
            f"--expected must be SEQ:HEAD as printed by `head` (a decimal seq, a colon and 64 "
            f"lowercase hex digits), not {text!r}"
        )
    return int(seq_text), head


def verify_one(memory_dir: Path, name: str, seq: int, head: str) -> dict[str, Any]:
    path = artifact_path(memory_dir, name)
    verifier: Callable[..., dict[str, Any]] = (
        hashchain.verify_store if ARTIFACTS[name][1] == "store" else hashchain.verify_journal
    )
    return verifier(path, expected_seq=seq, expected_head=head, artifact=name)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="chain_anchor.py",
        description="Read the current hash-chain heads, or verify one artifact against an anchor. "
        "Read-only: there is no write mode.",
    )
    parser.add_argument(
        "--memory-dir",
        type=Path,
        default=DEFAULT_MEMORY_DIR,
        help=f"directory holding the store and the journals (default {DEFAULT_MEMORY_DIR})",
    )
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("head", help="print seq, head and anchor for every artifact")
    verify = commands.add_parser("verify", help="verify one artifact against an anchor")
    verify.add_argument("--artifact", required=True, choices=sorted(ARTIFACTS))
    verify.add_argument("--expected", required=True, help="the anchor, SEQ:HEAD, printed by `head`")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    memory_dir: Path = args.memory_dir
    try:
        if args.command == "head":
            heads = {name: head_of(memory_dir, name) for name in sorted(ARTIFACTS)}
            print(json.dumps(heads, indent=2, sort_keys=True))
            return EXIT_INTACT
        seq, head = parse_anchor(args.expected)
        result = verify_one(memory_dir, args.artifact, seq, head)
    except ValueError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return EXIT_ERROR
    except (OSError, sqlite3.Error) as error:
        print(f"ERROR: cannot read {memory_dir} read-only: {type(error).__name__}: {error}",
              file=sys.stderr)
        return EXIT_ERROR

    print(json.dumps(result, indent=2, sort_keys=True))
    if result["status"] == "INTACT":
        return EXIT_INTACT
    where = ", ".join(
        f"{key} {result[key]}" for key in ("seq", "line", "entry_id") if result.get(key) is not None
    )
    print(f"FAIL: {args.artifact}: {result['reason']} ({where or 'no record named'})",
          file=sys.stderr)
    return EXIT_FAIL


if __name__ == "__main__":
    raise SystemExit(main())
