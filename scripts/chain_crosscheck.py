#!/usr/bin/env python3
"""Read-only operator cross-check of the store's chain records against the storage journal.

    python3 scripts/chain_crosscheck.py [--db PATH] [--journal PATH]

Every allowed write_entry, update_entry and delete_entry leaves one chain record in the store and
one line in the journal. This command pairs the two sides by (operation, entry_id), counting
repeats and never relying on position, and prints whatever does not pair. It reports facts only.
The storage server appends the journal line after the store commit, so a crash between the two
leaves an "unjournalled chain record" exactly as a write that bypassed the server does. The
operator decides what a mismatch means.

Two kinds of record are never paired, and are printed as counts:
  chain records outside the write vocabulary  the seed record that opens a store chain over rows
                                              written before the chain existed
  pre-chain journal writes                    lines with no "chain" block that precede the
                                              journal's first chained line

Exit 0: both mismatch lists are empty. Exit 1: either list is non-empty. Exit 2: the store or the
journal is missing or unreadable, the store has no chain_records table, or a journal line is not a
JSON object (named by line number).

There is no write mode: the store is opened with SQLite mode=ro, the journal is read as text, and
nothing is hashed. No agent role is granted this command; the operator runs it on the host.
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from collections import Counter
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[1]
if str(REPO / "mcp") not in sys.path:
    sys.path.insert(0, str(REPO / "mcp"))
import hashchain  # noqa: E402

DEFAULT_DB = REPO / ".memory" / "storage.db"
DEFAULT_JOURNAL = REPO / ".memory" / "storage-audit.log"
EXIT_MATCH, EXIT_MISMATCH, EXIT_ERROR = 0, 1, 2
WRITES = frozenset(hashchain.STORE_OPERATIONS)

UNJOURNALLED = "unjournalled chain record"
UNCHAINED = "journal write with no chain record"
OUTSIDE_VOCABULARY = "chain records outside the write vocabulary (not matched)"
PRE_CHAIN = "pre-chain journal writes (not matched)"


class CrossCheckError(RuntimeError):
    """The inputs cannot be read as a store and a storage journal."""


def read_chain(db: Path) -> list[dict[str, Any]]:
    if not db.is_file():
        raise CrossCheckError(f"no store at {db}")
    conn = hashchain.open_store_read_only(db)
    try:
        table = conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'chain_records'"
        ).fetchone()
        if table is None:
            raise CrossCheckError(f"{db} has no chain_records table")
        rows = conn.execute(
            "SELECT seq, operation, entry_id FROM chain_records ORDER BY seq"
        ).fetchall()
    finally:
        conn.close()
    return [
        {"seq": row["seq"], "operation": row["operation"], "entry_id": row["entry_id"]}
        for row in rows
    ]


def read_journal(journal: Path) -> tuple[list[dict[str, Any]], int]:
    """Return the allowed write lines from the first chained line on, and the pre-chain count.

    Every line the chained appender writes carries a "chain" key, including a line whose chain
    could not extend, so the first line holding one is where the chain begins.
    """
    if not journal.is_file():
        raise CrossCheckError(f"no journal at {journal}")
    lines = journal.read_bytes().decode("utf-8").split("\n")
    writes: list[dict[str, Any]] = []
    pre_chain = 0
    chain_begun = False
    for number, raw in enumerate(lines, start=1):
        if not raw.strip():
            continue
        try:
            record = json.loads(raw)
        except ValueError as error:
            raise CrossCheckError(f"{journal} line {number} is not JSON: {error}") from error
        if not isinstance(record, dict):
            raise CrossCheckError(f"{journal} line {number} is not a JSON object")
        chain_begun = chain_begun or "chain" in record
        if record.get("allowed") is not True or record.get("operation") not in WRITES:
            continue
        if not chain_begun:
            pre_chain += 1
            continue
        writes.append(
            {"line": number, "operation": record["operation"], "entry_id": record.get("entry_id")}
        )
    return writes, pre_chain


def unpaired(
    items: list[dict[str, Any]], others: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    """Return the items left after each (operation, entry_id) consumes one matching other."""
    remaining = Counter((other["operation"], other["entry_id"]) for other in others)
    left = []
    for item in items:
        key = (item["operation"], item["entry_id"])
        if remaining[key]:
            remaining[key] -= 1
        else:
            left.append(item)
    return left


def cross_check(db: Path, journal: Path) -> dict[str, Any]:
    chain = read_chain(db)
    writes, pre_chain = read_journal(journal)
    in_vocabulary = [record for record in chain if record["operation"] in WRITES]
    outside = [record for record in chain if record["operation"] not in WRITES]
    store = hashchain.store_head(db)
    journal_tip = hashchain.journal_head(journal)
    return {
        "store": {"path": str(db), "seq": store["seq"], "head": store["head"]},
        "journal": {"path": str(journal), "seq": journal_tip["seq"], "head": journal_tip["head"]},
        "n (chain records in the write vocabulary)": len(in_vocabulary),
        "m (journal writes from the first chained line on)": len(writes),
        "n - m": len(in_vocabulary) - len(writes),
        OUTSIDE_VOCABULARY: {"count": len(outside), "records": outside},
        PRE_CHAIN: {"count": pre_chain},
        UNJOURNALLED: unpaired(in_vocabulary, writes),
        UNCHAINED: unpaired(writes, in_vocabulary),
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="chain_crosscheck.py",
        description="Pair the store's chain records with the storage journal's write lines and "
        "print what does not pair. Read-only. The journal line is appended after the store "
        "commit, so a crash between the two also produces an unjournalled chain record; the "
        "operator decides what a mismatch means. Exit 0: no mismatch, 1: any mismatch, 2: error.",
    )
    parser.add_argument(
        "--db", type=Path, default=DEFAULT_DB, help=f"the store (default {DEFAULT_DB})"
    )
    parser.add_argument(
        "--journal",
        type=Path,
        default=DEFAULT_JOURNAL,
        help=f"the storage journal (default {DEFAULT_JOURNAL})",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        report = cross_check(args.db, args.journal)
    except CrossCheckError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return EXIT_ERROR
    except (OSError, ValueError, sqlite3.Error) as error:
        print(f"ERROR: cannot read the store or the journal read-only: "
              f"{type(error).__name__}: {error}", file=sys.stderr)
        return EXIT_ERROR

    print(json.dumps(report, indent=2, sort_keys=True))
    unjournalled, unchained = len(report[UNJOURNALLED]), len(report[UNCHAINED])
    if unjournalled or unchained:
        print(f"MISMATCH: {unjournalled} {UNJOURNALLED}(s), {unchained} {UNCHAINED}(s)",
              file=sys.stderr)
        return EXIT_MISMATCH
    return EXIT_MATCH


if __name__ == "__main__":
    raise SystemExit(main())
