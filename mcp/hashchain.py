#!/usr/bin/env python3
"""One SHA-256 hash chain construction for the storage database and the three MCP audit journals.

Each artifact carries its own chain, and every chain is built the same way:

    head_0 = 64 hex zeros
    head_n = SHA-256(bytes.fromhex(head_{n-1}) || canonical(record_n))

An operator keeps a ``(seq, head)`` pair outside the container (``scripts/chain_anchor.py head``)
and later checks the artifact against it (``scripts/chain_anchor.py verify``). Verification returns
INTACT or FAIL, and a FAIL names the first record that disagrees. The chain stores digests and never
content, so it is never a second source of truth: on any disagreement the chain fails and the data
stays exactly as it is.
"""

from __future__ import annotations

import fcntl
import hashlib
import json
import math
import os
import sqlite3
import sys
from pathlib import Path
from typing import Any, BinaryIO, Iterator, Mapping

GENESIS = "0" * 64
SEED_OPERATION = "seed"
STORE_OPERATIONS = ("write_entry", "update_entry", "delete_entry")
TAIL_BLOCK_BYTES = 65536
HEX_DIGITS = frozenset("0123456789abcdef")

STORE_SCHEMA_STATEMENTS = (
    """
    CREATE TABLE IF NOT EXISTS chain_records (
        seq INTEGER PRIMARY KEY,
        operation TEXT NOT NULL,
        entry_id TEXT,
        record TEXT NOT NULL,
        prev_head TEXT NOT NULL,
        head TEXT NOT NULL
    )
    """,
)


class ChainError(RuntimeError):
    """The chain cannot be extended from what the artifact holds."""


# --- Canonical form -------------------------------------------------------------------------
def _normalise(value: Any) -> Any:
    if value is None or isinstance(value, (bool, str)):
        return value
    if isinstance(value, int):
        return int(value)
    if isinstance(value, float):
        if not math.isfinite(value):
            raise ValueError(f"canonical form refuses a non-finite float ({value!r})")
        return 0.0 if value == 0.0 else float(value)
    if isinstance(value, Mapping):
        normalised: dict[str, Any] = {}
        for key, item in value.items():
            if not isinstance(key, str):
                raise TypeError(f"canonical form needs string keys, not {type(key).__name__}")
            normalised[key] = _normalise(item)
        return normalised
    if isinstance(value, (list, tuple)):
        return [_normalise(item) for item in value]
    raise TypeError(f"canonical form has no encoding for {type(value).__name__}")


def canonical(value: Any) -> bytes:
    """Return the one UTF-8 byte string a JSON value serialises to.

    Keys are sorted, separators carry no whitespace and ``None`` is an explicit ``null``. A float is
    its shortest round-tripping decimal (``repr``), with ``-0.0`` folded into ``0.0``. NaN and the
    infinities raise ``ValueError``, and any non-JSON type raises ``TypeError``.
    """
    return json.dumps(
        _normalise(value),
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    ).encode("utf-8")


def link(prev_head: str, record: Mapping[str, Any]) -> str:
    """Return ``SHA-256(bytes.fromhex(prev_head) || canonical(record))`` as lowercase hex."""
    return hashlib.sha256(bytes.fromhex(prev_head) + canonical(record)).hexdigest()


def is_head(value: Any) -> bool:
    """Say whether ``value`` is a 64-digit lowercase hex head."""
    return isinstance(value, str) and len(value) == 64 and set(value) <= HEX_DIGITS


def _seq(value: Any) -> int | None:
    return value if isinstance(value, int) and not isinstance(value, bool) else None


def _intact(artifact: str, seq: int, head: str, **detail: Any) -> dict[str, Any]:
    return {"status": "INTACT", "artifact": artifact, "seq": seq, "head": head, **detail}


def _fail(artifact: str, seq: int | None, reason: str, **where: Any) -> dict[str, Any]:
    return {"status": "FAIL", "artifact": artifact, "seq": seq, "reason": reason, **where}


def _check_anchor(
    artifact: str,
    heads: dict[int, str],
    tip: int,
    expected_seq: int | None,
    expected_head: str | None,
    where: dict[int, dict[str, Any]],
) -> dict[str, Any] | None:
    if expected_seq is None:
        return None
    if expected_seq == 0:
        if expected_head != GENESIS:
            return _fail(artifact, 0, "an anchor at seq 0 must be the genesis head")
        return None
    if expected_seq not in heads:
        return _fail(
            artifact, expected_seq, f"the anchored seq is missing; the chain ends at seq {tip}"
        )
    if heads[expected_seq] != expected_head:
        return _fail(
            artifact,
            expected_seq,
            "the head at the anchored seq differs from the anchor",
            **where.get(expected_seq, {}),
        )
    return None


# --- Journals -------------------------------------------------------------------------------
def _hashed_line(line: Mapping[str, Any], seq: int, seeded: Any) -> dict[str, Any]:
    hashed = {key: value for key, value in line.items() if key != "chain"}
    hashed["chain"] = {"seq": seq, "seeded": seeded}
    return hashed


def _lines_from_end(handle: BinaryIO) -> Iterator[bytes]:
    handle.seek(0, os.SEEK_END)
    position = handle.tell()
    remainder = b""
    while position > 0:
        step = min(TAIL_BLOCK_BYTES, position)
        position -= step
        handle.seek(position)
        pieces = (handle.read(step) + remainder).split(b"\n")
        remainder = pieces[0]
        yield from reversed(pieces[1:])
    yield remainder


def _usable_block(raw: bytes) -> dict[str, Any] | None:
    try:
        line = json.loads(raw)
    except ValueError:
        return None
    block = line.get("chain") if isinstance(line, dict) else None
    if not isinstance(block, dict):
        return None
    seq = _seq(block.get("seq"))
    if seq is None or seq < 1 or not is_head(block.get("head")):
        return None
    return block


def _journal_tip(handle: BinaryIO) -> tuple[int, str] | None:
    for raw in _lines_from_end(handle):
        if raw.strip():
            block = _usable_block(raw)
            if block is not None:
                return block["seq"], block["head"]
    return None


def _prefix_seed(handle: BinaryIO, size: int, add_newline: bool) -> dict[str, Any]:
    digest = hashlib.sha256()
    lines = 0
    handle.seek(0)
    remaining = size
    while remaining > 0:
        block = handle.read(min(TAIL_BLOCK_BYTES, remaining))
        if not block:
            raise ChainError("the journal shrank while its prefix was being read")
        remaining -= len(block)
        digest.update(block)
        lines += block.count(b"\n")
    if add_newline:
        digest.update(b"\n")
        lines += 1
    return {"prior_lines": lines, "prior_sha256": digest.hexdigest()}


def _next_journal_block(
    handle: BinaryIO, record: Mapping[str, Any]
) -> tuple[dict[str, Any], bool]:
    """Return the chain block for ``record`` and whether the file needs a newline before it.

    The caller holds the exclusive lock. A journal with no chained line yet is seeded: the first
    chained line records how many lines came before it and the digest of their exact bytes, and
    no earlier line is rewritten.
    """
    if "chain" in record:
        raise ChainError("the record already carries a 'chain' key")
    handle.seek(0, os.SEEK_END)
    size = handle.tell()
    add_newline = False
    if size:
        handle.seek(size - 1)
        add_newline = handle.read(1) != b"\n"
    tip = _journal_tip(handle)
    seeded = None
    if tip is None:
        prev_seq, prev_head = 0, GENESIS
        if size:
            seeded = _prefix_seed(handle, size, add_newline)
    else:
        prev_seq, prev_head = tip
    seq = prev_seq + 1
    head = link(prev_head, _hashed_line(record, seq, seeded))
    return {"seq": seq, "prev": prev_head, "head": head, "seeded": seeded}, add_newline


def _chain_failure(path: str | os.PathLike[str], reason: str) -> dict[str, Any]:
    print(
        f"ERROR: hash chain for {path} could not extend ({reason}); the record is written with "
        "chain.error and no head, and every verify of this journal fails at that line",
        file=sys.stderr,
        flush=True,
    )
    return {"seq": None, "prev": None, "head": None, "seeded": None, "error": reason}


def append_journal_record(
    path: str | os.PathLike[str], record: Mapping[str, Any]
) -> dict[str, Any]:
    """Append ``record`` as one JSON line carrying its chain block, and return the block.

    A chain that cannot extend never suppresses or alters the record and never raises: the line is
    written with ``chain.error`` in place of a head, the failure goes to stderr, and every later
    verify fails at that line. The caller's decision therefore stands exactly as it would without
    a chain. Writing the line itself still raises on an I/O error, as the journal always did.
    """
    with open(path, "a+b") as handle:
        add_newline = False
        try:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
            block, add_newline = _next_journal_block(handle, record)
        except Exception as error:  # noqa: BLE001
            block = _chain_failure(path, f"{type(error).__name__}: {error}")
        line = json.dumps({**record, "chain": block}, sort_keys=True) + "\n"
        handle.write((b"\n" if add_newline else b"") + line.encode("utf-8"))
        handle.flush()
        os.fsync(handle.fileno())
    return block


def journal_head(path: str | os.PathLike[str]) -> dict[str, Any]:
    """Return the journal's current ``seq`` and ``head``: seq 0 and genesis when unchained."""
    journal = Path(path)
    if not journal.exists():
        return {"present": False, "seq": 0, "head": GENESIS}
    with open(journal, "rb") as handle:
        tip = _journal_tip(handle)
    seq, head = tip if tip is not None else (0, GENESIS)
    return {"present": True, "seq": seq, "head": head}


def verify_journal(
    path: str | os.PathLike[str],
    expected_seq: int | None = None,
    expected_head: str | None = None,
    artifact: str = "journal",
) -> dict[str, Any]:
    """Check every chained line, the seeded prefix and the anchor; return INTACT or the first FAIL.

    Each chained line must be the exact bytes the appender writes for its own content, carry the
    next seq, link to the previous head, and recompute to its stored head. A line after the first
    chained line that carries no usable chain block is a failure, never a skip.
    """
    journal = Path(path)
    data = journal.read_bytes() if journal.exists() else b""
    lines = data.split(b"\n")
    if lines[-1] == b"":
        lines.pop()

    heads: dict[int, str] = {}
    where: dict[int, dict[str, Any]] = {}
    prev_head, prev_seq = GENESIS, 0
    prefix_end: int | None = None
    prefix_lines = 0
    offset = 0
    for number, raw in enumerate(lines, start=1):
        try:
            line = json.loads(raw)
        except ValueError:
            line = None
        chained = isinstance(line, dict) and "chain" in line
        if prefix_end is None:
            if not chained:
                offset += len(raw) + 1
                continue
            prefix_end, prefix_lines = offset, number - 1

        seq = prev_seq + 1
        if not chained:
            return _fail(artifact, seq, "the line carries no chain block", line=number)
        block = line["chain"]
        if not isinstance(block, dict):
            return _fail(artifact, seq, "the chain block is not an object", line=number)
        if block.get("error") is not None:
            reason = f"the chain could not extend at this line: {block['error']}"
            return _fail(artifact, seq, reason, line=number)
        if _seq(block.get("seq")) != seq:
            reason = f"expected seq {seq}, found {block.get('seq')!r}"
            return _fail(artifact, seq, reason, line=number)
        if block.get("prev") != prev_head:
            return _fail(artifact, seq, "prev is not the previous line's head", line=number)
        if raw != json.dumps(line, sort_keys=True).encode("utf-8"):
            reason = "the line bytes are not the serialisation of its content"
            return _fail(artifact, seq, reason, line=number)

        seeded = block.get("seeded")
        if seq == 1:
            expected_seed = None
            if prefix_lines:
                expected_seed = {
                    "prior_lines": prefix_lines,
                    "prior_sha256": hashlib.sha256(data[:prefix_end]).hexdigest(),
                }
            if seeded != expected_seed:
                reason = "the seed does not match the lines before the chain"
                return _fail(artifact, seq, reason, line=number)
        elif seeded is not None:
            return _fail(artifact, seq, "only the first chained line may carry a seed", line=number)

        try:
            head = link(prev_head, _hashed_line(line, seq, seeded))
        except (TypeError, ValueError) as error:
            return _fail(artifact, seq, f"the line has no canonical form: {error}", line=number)
        if block.get("head") != head:
            reason = "the recomputed head differs from the stored head"
            return _fail(artifact, seq, reason, line=number)

        heads[seq] = head
        where[seq] = {"line": number}
        prev_head, prev_seq = head, seq
        offset += len(raw) + 1

    if prefix_end is None and lines:
        reason = f"{len(lines)} line(s) and no chained line: the chain covers nothing"
        return _fail(artifact, 1, reason, line=1)
    anchor = _check_anchor(artifact, heads, prev_seq, expected_seq, expected_head, where)
    if anchor is not None:
        return anchor
    return _intact(artifact, prev_seq, prev_head, lines=len(lines), prior_lines=prefix_lines)


# --- The store ------------------------------------------------------------------------------
def row_digest(row: Mapping[str, Any]) -> str:
    """Return the SHA-256 of the canonical form of one full ``entries`` row."""
    return hashlib.sha256(canonical(dict(row))).hexdigest()


def store_record(
    seq: int,
    operation: str,
    entry_id: str | None,
    *,
    row_sha256: str | None = None,
    rows: list[list[str]] | None = None,
) -> dict[str, Any]:
    """Build one store chain record. It holds digests only, never a row's content."""
    return {
        "seq": seq,
        "operation": operation,
        "entry_id": entry_id,
        "row_sha256": row_sha256,
        "rows": rows,
        "seeded": operation == SEED_OPERATION,
    }


def store_link(prev_head: str, record: Mapping[str, Any]) -> tuple[str, str]:
    """Return the record's stored text and its head."""
    return canonical(record).decode("utf-8"), link(prev_head, record)


def _row_dict(cursor: Any, row: Any) -> dict[str, Any]:
    return {column[0]: row[index] for index, column in enumerate(cursor.description)}


async def _insert_store_record(conn: Any, record: Mapping[str, Any], prev_head: str) -> str:
    text, head = store_link(prev_head, record)
    await conn.execute(
        "INSERT INTO chain_records (seq, operation, entry_id, record, prev_head, head) "
        "VALUES (?, ?, ?, ?, ?, ?)",
        (record["seq"], record["operation"], record["entry_id"], text, prev_head, head),
    )
    return head


async def append_store_record(
    conn: Any, operation: str, entry_id: str
) -> dict[str, Any] | None:
    """Extend the store chain for one write, inside the caller's open write transaction.

    ``conn`` is the storage server's aiosqlite connection after ``BEGIN IMMEDIATE`` and the write
    itself. The record holds the digest of the row as the write left it. When the chain is empty
    and other rows already exist, a seed record covering those rows is written first and its report
    is returned; every other call returns None. Any exception leaves the transaction uncommitted, so
    a write and its chain record commit together or not at all.
    """
    if operation not in STORE_OPERATIONS:
        raise ChainError(f"{operation!r} is not a store write operation")
    cursor = await conn.execute("SELECT seq, head FROM chain_records ORDER BY seq DESC LIMIT 1")
    tip = await cursor.fetchone()
    prev_seq, prev_head = (tip[0], tip[1]) if tip is not None else (0, GENESIS)

    seeded = None
    if tip is None:
        cursor = await conn.execute(
            "SELECT * FROM entries WHERE entry_id != ? ORDER BY entry_id", (entry_id,)
        )
        prior = [_row_dict(cursor, row) for row in await cursor.fetchall()]
        if prior:
            pairs = [[row["entry_id"], row_digest(row)] for row in prior]
            seed = store_record(1, SEED_OPERATION, None, rows=pairs)
            prev_seq, prev_head = 1, await _insert_store_record(conn, seed, prev_head)
            seeded = {"seq": 1, "rows": len(pairs), "head": prev_head}

    cursor = await conn.execute("SELECT * FROM entries WHERE entry_id = ?", (entry_id,))
    row = await cursor.fetchone()
    if row is None:
        raise ChainError(f"no row for entry {entry_id} after its {operation}")
    digest = row_digest(_row_dict(cursor, row))
    record = store_record(prev_seq + 1, operation, entry_id, row_sha256=digest)
    await _insert_store_record(conn, record, prev_head)
    return seeded


def open_store_read_only(path: str | os.PathLike[str]) -> sqlite3.Connection:
    """Open the store with SQLite ``mode=ro``. There is no read-write fallback."""
    conn = sqlite3.connect(Path(path).resolve().as_uri() + "?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    return conn


def _has_table(conn: sqlite3.Connection, name: str) -> bool:
    found = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", (name,)
    ).fetchone()
    return found is not None


def store_head(path: str | os.PathLike[str]) -> dict[str, Any]:
    """Return the store's current ``seq`` and ``head``: seq 0 and genesis when unchained."""
    if not Path(path).exists():
        return {"present": False, "seq": 0, "head": GENESIS}
    conn = open_store_read_only(path)
    try:
        tip = None
        if _has_table(conn, "chain_records"):
            tip = conn.execute(
                "SELECT seq, head FROM chain_records ORDER BY seq DESC LIMIT 1"
            ).fetchone()
    finally:
        conn.close()
    if tip is None:
        return {"present": True, "seq": 0, "head": GENESIS}
    return {"present": True, "seq": tip["seq"], "head": tip["head"]}


def _seed_pairs(record: Mapping[str, Any]) -> list[list[str]] | None:
    pairs = record.get("rows")
    if not isinstance(pairs, list):
        return None
    for pair in pairs:
        if not (isinstance(pair, list) and len(pair) == 2 and all(isinstance(p, str) for p in pair)):
            return None
    return pairs


def verify_store(
    path: str | os.PathLike[str],
    expected_seq: int | None = None,
    expected_head: str | None = None,
    artifact: str = "store",
) -> dict[str, Any]:
    """Check the chain records, the anchor and every row's digest; return INTACT or the first FAIL.

    A row whose digest differs from its latest chain record, a row no record covers, and a covered
    row that is missing all fail and name the ``entry_id``.
    """
    chain: list[sqlite3.Row] = []
    rows: list[sqlite3.Row] = []
    if Path(path).exists():
        conn = open_store_read_only(path)
        try:
            if _has_table(conn, "chain_records"):
                chain = conn.execute(
                    "SELECT seq, operation, entry_id, record, prev_head, head "
                    "FROM chain_records ORDER BY seq"
                ).fetchall()
            if _has_table(conn, "entries"):
                rows = conn.execute("SELECT * FROM entries ORDER BY entry_id").fetchall()
        finally:
            conn.close()

    heads: dict[int, str] = {}
    where: dict[int, dict[str, Any]] = {}
    covered: dict[str, tuple[int, str]] = {}
    prev_head = GENESIS
    for seq, stored in enumerate(chain, start=1):
        entry_id = stored["entry_id"]
        if stored["seq"] != seq:
            reason = f"expected seq {seq}, found {stored['seq']}"
            return _fail(artifact, seq, reason, entry_id=entry_id)
        if stored["prev_head"] != prev_head:
            return _fail(artifact, seq, "prev_head is not the previous head", entry_id=entry_id)
        try:
            record = json.loads(stored["record"])
            in_canonical_form = canonical(record) == stored["record"].encode("utf-8")
        except (TypeError, ValueError):
            record, in_canonical_form = None, False
        if not isinstance(record, dict) or not in_canonical_form:
            return _fail(artifact, seq, "the record is not in canonical form", entry_id=entry_id)
        if (
            record.get("seq") != seq
            or record.get("operation") != stored["operation"]
            or record.get("entry_id") != entry_id
        ):
            reason = "the record disagrees with its columns"
            return _fail(artifact, seq, reason, entry_id=entry_id)
        head = link(prev_head, record)
        if head != stored["head"]:
            reason = "the recomputed head differs from the stored head"
            return _fail(artifact, seq, reason, entry_id=entry_id)

        operation = record["operation"]
        if operation == SEED_OPERATION:
            pairs = _seed_pairs(record)
            if seq != 1 or pairs is None:
                return _fail(artifact, seq, "a seed record may only open the chain, as id pairs")
            for seeded_id, digest in pairs:
                covered[seeded_id] = (seq, digest)
        elif (
            operation in STORE_OPERATIONS
            and isinstance(entry_id, str)
            and isinstance(record.get("row_sha256"), str)
        ):
            covered[entry_id] = (seq, record["row_sha256"])
        else:
            reason = f"unknown or incomplete operation {operation!r}"
            return _fail(artifact, seq, reason, entry_id=entry_id)

        heads[seq] = head
        where[seq] = {"entry_id": entry_id}
        prev_head = head

    tip = len(chain)
    anchor = _check_anchor(artifact, heads, tip, expected_seq, expected_head, where)
    if anchor is not None:
        return anchor

    problems: list[tuple[float, str, str]] = []
    present = set()
    for row in rows:
        entry_id = row["entry_id"]
        present.add(entry_id)
        cover = covered.get(entry_id)
        if cover is None:
            problems.append((math.inf, entry_id, "the row has no chain record"))
        elif row_digest(dict(row)) != cover[1]:
            problems.append((cover[0], entry_id, f"the row differs from chain record seq {cover[0]}"))
    for entry_id, (seq, _digest) in covered.items():
        if entry_id not in present:
            problems.append((seq, entry_id, "the chain covers a row that is missing"))
    if problems:
        seq, entry_id, reason = min(problems)
        return _fail(artifact, None if seq == math.inf else int(seq), reason, entry_id=entry_id)
    return _intact(artifact, tip, prev_head, records=tip, rows=len(rows))
