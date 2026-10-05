#!/usr/bin/env python3
"""Persistent storage MCP server for the Agentic Engineer Module 3.2 exercise, with the Module 4.1
enforcement layers: a file-based role allow-list and an audit journal that records denials.

Exposes five schema-bound operations over streamable HTTP (FastMCP): ``write_entry``,
``read_entry``, ``list_entries``, ``update_entry`` and ``delete_entry``. There is no raw
SQL operation. Every write-class operation appends one JSON object per line to an audit
journal, and every refused *write classification* is turned away before the journal is
touched, so a refused secret write leaves the log unchanged.

Two enforcement layers sit in front of those operations:

* ``_authorize(calling_role, operation)`` is the first statement of every one of the five
  operations. It reads the role allow-list from ``mcp/storage/allow-list.json`` (override with
  ``STORAGE_ALLOW_LIST_PATH`` or ``--allowlist-path``) and refuses an ungranted pair with an
  ``authorization_denied`` error that names the role, the operation and the roles that ARE
  allowed. An unknown, missing or blank role is refused, never defaulted to an allowed one.
  The refusal is journalled to the same ``storage-audit.log`` that records the successful calls:
  one log, no second file.
* The allow-list file is loaded at startup. A missing or malformed file stops the server with a
  clear error instead of starting without it, because starting without it would let every role
  call every operation.

Run inside the sandbox container:

    python3 mcp/storage/server.py --port 8001 --host 0.0.0.0
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import aiosqlite
from fastmcp import FastMCP
from starlette.middleware import Middleware
from starlette.middleware.cors import CORSMiddleware
_MCP_DIR = str(Path(__file__).resolve().parents[1])
if _MCP_DIR not in sys.path:
    sys.path.insert(0, _MCP_DIR)
import hashchain  # noqa: E402

# --- Runtime paths: container defaults, every one overridable for a local run ---------------
MEMORY_DIR = os.getenv("MEMORY_DIR", "/workspace/.memory")
DB_PATH = os.getenv("STORAGE_DB_PATH", str(Path(MEMORY_DIR) / "storage.db"))
AUDIT_PATH = os.getenv("STORAGE_AUDIT_PATH", str(Path(MEMORY_DIR) / "storage-audit.log"))
# The allow-list sits beside this file, so it resolves the same way whether the server is started
# from the repository root, from the container's /workspace, or from anywhere else.
ALLOW_LIST_PATH = Path(
    os.getenv("STORAGE_ALLOW_LIST_PATH", str(Path(__file__).resolve().parent / "allow-list.json"))
)

# --- Vocabulary -----------------------------------------------------------------------------
ALLOWED_CLASSIFICATIONS = ("public", "internal", "confidential", "secret")
WRITE_CLASSIFICATIONS = ("public", "internal")
# The complete callable surface of this server: what a role can be granted, and the only names the
# allow-list file may use.
OPERATIONS = ("write_entry", "read_entry", "list_entries", "update_entry", "delete_entry")
PROJECT_ID_PATTERN = re.compile(r"^[A-Za-z0-9-]+$")

SCHEMA_STATEMENTS = (
    """
    CREATE TABLE IF NOT EXISTS entries (
        entry_id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        entry_type TEXT NOT NULL,
        title TEXT NOT NULL,
        content TEXT NOT NULL,
        classification TEXT NOT NULL,
        deleted INTEGER NOT NULL DEFAULT 0,
        last_updated TEXT NOT NULL
    )
    """,
    "CREATE INDEX IF NOT EXISTS idx_entries_project_type "
    "ON entries(project_id, entry_type, deleted)",
) + hashchain.STORE_SCHEMA_STATEMENTS

mcp = FastMCP("storage")


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


# --- Validation -----------------------------------------------------------------------------
def validate_project_id(project_id: str) -> None:
    """Reject a project identifier outside ``^[A-Za-z0-9-]+$``."""
    if not PROJECT_ID_PATTERN.match(project_id or ""):
        raise ValueError("project_id must contain only letters, numbers, and hyphens")


def validate_nonempty(value: str, field_name: str) -> None:
    """Reject a missing or blank string field."""
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{field_name} must be a non-empty string")


def validate_classification(classification: str) -> None:
    """Check the vocabulary, then refuse the classifications this server never stores.

    The refusal happens before the audit journal is written, which is what keeps a
    ``secret`` write from adding a line.
    """
    if classification not in ALLOWED_CLASSIFICATIONS:
        raise ValueError(
            f"classification must be one of {list(ALLOWED_CLASSIFICATIONS)}"
        )
    if classification not in WRITE_CLASSIFICATIONS:
        raise ValueError(
            "refused: this server stores only "
            f"{list(WRITE_CLASSIFICATIONS)} entries and never journals a refusal; "
            f"'{classification}' was not written"
        )


# --- The audit journal ----------------------------------------------------------------------
def append_audit_record(record: dict[str, Any]) -> None:
    """Append exactly one JSON object plus newline. The file is opened append-only."""
    ensure_parent(AUDIT_PATH)
    hashchain.append_journal_record(AUDIT_PATH, record)


def audit_event(
    operation: str,
    *,
    allowed: bool,
    project_id: str | None = None,
    entry_id: str | None = None,
    classification: str | None = None,
    calling_role: str | None = None,
    reason: str | None = None,
) -> None:
    """Write one journal record for an allowed state change or a denied access.

    ``project_id`` defaults to ``None`` because the authorization guard refuses a call before any
    argument has been validated, so a denial can arrive with an unusable project identifier.
    """
    append_audit_record(
        {
            "timestamp": utc_now(),
            "operation": operation,
            "project_id": project_id if isinstance(project_id, str) else None,
            "entry_id": entry_id,
            "classification": classification,
            "calling_role": calling_role or "unknown",
            "allowed": allowed,
            "reason": reason,
        }
    )


# --- The allow-list: the whole grant surface, plus the guard every operation calls first ------
class AllowListError(RuntimeError):
    """A missing or malformed allow-list file. Fatal: the server never starts without it."""


class AuthorizationDenied(PermissionError):
    """A role called an operation the allow-list does not grant it. Journalled, then raised."""


def load_allow_list(path: Path) -> dict[str, list[str]]:
    """Read ``role -> permitted operations`` from the allow-list file, or fail loudly.

    There is deliberately no in-code copy of the grants and no permissive fallback: a server that
    cannot read its allow-list must not start, because the alternative is a server that answers
    every role, which is the failure this layer exists to prevent.
    """
    if not path.is_file():
        raise AllowListError(
            f"allow-list file not found at {path}; refusing to start, because a server without its "
            "allow-list would let every role call every operation"
        )
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise AllowListError(f"allow-list file {path} is not valid JSON: {error}") from error

    roles = data.get("roles")
    if not isinstance(roles, dict) or not roles:
        raise AllowListError(f"allow-list file {path} carries no 'roles' object")
    for role, operations in roles.items():
        if not isinstance(operations, list) or not all(
            isinstance(operation, str) for operation in operations
        ):
            raise AllowListError(
                f"allow-list file {path}: the entry for role '{role}' must be a list of operation "
                "names"
            )
        unknown = sorted(set(operations) - set(OPERATIONS))
        if unknown:
            raise AllowListError(
                f"allow-list file {path}: role '{role}' grants unknown operations {unknown}; this "
                f"server exposes {list(OPERATIONS)}"
            )
    return {role: list(operations) for role, operations in roles.items()}


def authorized_roles(operation: str) -> list[str]:
    """Return, sorted, every role the allow-list grants ``operation``."""
    return sorted(role for role, operations in ALLOW_LIST.items() if operation in operations)


# --- Role binding: AGENT_ROLE is the identity, the calling_role argument only corroborates --------
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


def _authorize(calling_role: str | None, operation: str, project_id: str | None = None) -> str:
    """Refuse an ungranted (role, operation) pair, journal the refusal, and return the role.

    This is the first statement of every operation, so nothing else in the operation runs for a
    refused call: no validation, no database connection, no state change. The role is bound to the
    container's ``AGENT_ROLE`` first -- a ``calling_role`` that disagrees with it is refused outright
    -- and the refusal names the role, the operation and the roles that ARE allowed, and it is
    journalled to the same audit log the successful calls use.
    """
    if operation not in OPERATIONS:
        raise ValueError(
            f"'{operation}' is not one of this server's operations {list(OPERATIONS)}"
        )

    allowed = authorized_roles(operation)
    role, mismatch = bind_role(calling_role)
    if mismatch is not None:
        reason = (
            f"authorization_denied: {mismatch}. "
            f"operation={operation!r} role={role or 'unknown'!r} allowed_roles={allowed}"
        )
        audit_event(
            operation,
            allowed=False,
            project_id=project_id,
            calling_role=role or "unknown",
            reason=reason,
        )
        raise AuthorizationDenied(reason)

    if role and role in ALLOW_LIST and operation in ALLOW_LIST[role]:
        return role

    if not role or role == "unknown":
        cause = (
            f"unknown role {calling_role!r}: a missing, blank or unrecognised role is refused and "
            "is never defaulted to an allowed one"
        )
    elif role not in ALLOW_LIST:
        cause = f"unknown role {role!r}: it is not one of the roles {sorted(ALLOW_LIST)}"
    else:
        cause = f"role {role!r} is not granted {operation!r}"

    reason = (
        f"authorization_denied: {cause}. "
        f"operation={operation!r} role={role or 'unknown'!r} allowed_roles={allowed}"
    )
    audit_event(
        operation,
        allowed=False,
        project_id=project_id,
        calling_role=role or "unknown",
        reason=reason,
    )
    raise AuthorizationDenied(reason)


# The allow-list is loaded here, at import time, so a missing or malformed file kills the process
# before uvicorn binds a port. main() loads it again for the CLI flag.
try:
    ALLOW_LIST: dict[str, list[str]] = load_allow_list(ALLOW_LIST_PATH)
except AllowListError as error:
    print(f"ERROR: {error}", file=sys.stderr, flush=True)
    raise SystemExit(2) from error


# --- Database -------------------------------------------------------------------------------
async def open_db() -> aiosqlite.Connection:
    """Open the persistent database, ensure its schema, and return the connection."""
    ensure_parent(DB_PATH)
    conn = await aiosqlite.connect(DB_PATH)
    conn.row_factory = aiosqlite.Row
    await conn.execute("PRAGMA journal_mode=WAL")
    await conn.execute("PRAGMA foreign_keys=ON")
    await conn.execute("PRAGMA busy_timeout=5000")
    for statement in SCHEMA_STATEMENTS:
        await conn.execute(statement)
    await conn.commit()
    return conn


async def fetch_live_entry(
    conn: aiosqlite.Connection, project_id: str, entry_id: str
) -> aiosqlite.Row | None:
    """Return the row for a project-scoped, not soft-deleted entry, or ``None``."""
    cursor = await conn.execute(
        "SELECT * FROM entries WHERE project_id = ? AND entry_id = ? AND deleted = 0",
        (project_id, entry_id),
    )
    return await cursor.fetchone()


# --- Operations -----------------------------------------------------------------------------
# Each write runs between BEGIN IMMEDIATE and its commit together with its chain record. A
# connection closed without that commit discards both, so no row change lands without its record.
@mcp.tool
async def write_entry(
    project_id: str,
    entry_type: str,
    title: str,
    content: str,
    classification: str,
    calling_role: str = "unknown",
) -> dict:
    """Write a new entry, then journal it. Classifications at or above confidential are refused."""
    authorized_role = _authorize(calling_role, "write_entry", project_id=project_id)
    validate_project_id(project_id)
    validate_nonempty(entry_type, "entry_type")
    validate_nonempty(title, "title")
    validate_nonempty(content, "content")
    validate_classification(classification)

    entry_id = str(uuid.uuid4())
    now = utc_now()
    conn = await open_db()
    try:
        await conn.execute("BEGIN IMMEDIATE")
        await conn.execute(
            "INSERT INTO entries (entry_id, project_id, entry_type, title, content, "
            "classification, deleted, last_updated) VALUES (?, ?, ?, ?, ?, ?, 0, ?)",
            (entry_id, project_id, entry_type, title, content, classification, now),
        )
        seeded = await hashchain.append_store_record(conn, "write_entry", entry_id)
        await conn.commit()
    finally:
        await conn.close()

    audit_event(
        "write_entry",
        allowed=True,
        project_id=project_id,
        entry_id=entry_id,
        classification=classification,
        calling_role=authorized_role,
    )
    if seeded is not None:
        return {"entry_id": entry_id, "chain_seeded": seeded}
    return {"entry_id": entry_id}


@mcp.tool
async def read_entry(project_id: str, entry_id: str, calling_role: str = "unknown") -> dict:
    """Read one live entry by identifier. A denied read is journalled, an allowed read is not."""
    authorized_role = _authorize(calling_role, "read_entry", project_id=project_id)
    validate_project_id(project_id)
    validate_nonempty(entry_id, "entry_id")

    conn = await open_db()
    try:
        row = await fetch_live_entry(conn, project_id, entry_id)
    finally:
        await conn.close()

    if row is None:
        audit_event(
            "read_entry",
            allowed=False,
            project_id=project_id,
            entry_id=entry_id,
            calling_role=authorized_role,
            reason="no live entry for that project_id and entry_id",
        )
        raise ValueError("no entry found for that project_id and entry_id")
    return dict(row)


@mcp.tool
async def list_entries(
    project_id: str, entry_type: str | None = None, calling_role: str = "unknown"
) -> list[dict]:
    """List live entries for one project as metadata. Never returns entry content."""
    _authorize(calling_role, "list_entries", project_id=project_id)
    validate_project_id(project_id)

    conn = await open_db()
    try:
        if entry_type is None:
            cursor = await conn.execute(
                "SELECT entry_id, title, entry_type, classification, last_updated "
                "FROM entries WHERE project_id = ? AND deleted = 0 "
                "ORDER BY last_updated DESC, entry_id",
                (project_id,),
            )
        else:
            validate_nonempty(entry_type, "entry_type")
            cursor = await conn.execute(
                "SELECT entry_id, title, entry_type, classification, last_updated "
                "FROM entries WHERE project_id = ? AND entry_type = ? AND deleted = 0 "
                "ORDER BY last_updated DESC, entry_id",
                (project_id, entry_type),
            )
        rows = await cursor.fetchall()
    finally:
        await conn.close()

    return [dict(row) for row in rows]


@mcp.tool
async def update_entry(
    project_id: str,
    entry_id: str,
    content: str,
    title: str | None = None,
    calling_role: str = "unknown",
) -> dict:
    """Replace the content of a live entry, leaving its classification untouched."""
    authorized_role = _authorize(calling_role, "update_entry", project_id=project_id)
    validate_project_id(project_id)
    validate_nonempty(entry_id, "entry_id")
    validate_nonempty(content, "content")
    if title is not None:
        validate_nonempty(title, "title")

    now = utc_now()
    conn = await open_db()
    try:
        row = await fetch_live_entry(conn, project_id, entry_id)
        if row is None:
            audit_event(
                "update_entry",
                allowed=False,
                project_id=project_id,
                entry_id=entry_id,
                calling_role=authorized_role,
                reason="no live entry for that project_id and entry_id",
            )
            raise ValueError("no entry found to update")
        await conn.execute("BEGIN IMMEDIATE")
        if title is None:
            await conn.execute(
                "UPDATE entries SET content = ?, last_updated = ? "
                "WHERE project_id = ? AND entry_id = ? AND deleted = 0",
                (content, now, project_id, entry_id),
            )
        else:
            await conn.execute(
                "UPDATE entries SET content = ?, title = ?, last_updated = ? "
                "WHERE project_id = ? AND entry_id = ? AND deleted = 0",
                (content, title, now, project_id, entry_id),
            )
        seeded = await hashchain.append_store_record(conn, "update_entry", entry_id)
        await conn.commit()
        classification = row["classification"]
    finally:
        await conn.close()

    audit_event(
        "update_entry",
        allowed=True,
        project_id=project_id,
        entry_id=entry_id,
        classification=classification,
        calling_role=authorized_role,
    )
    if seeded is not None:
        return {"success": True, "chain_seeded": seeded}
    return {"success": True}


@mcp.tool
async def delete_entry(project_id: str, entry_id: str, calling_role: str = "unknown") -> dict:
    """Soft-delete a live entry by setting its deleted flag. The row is never removed."""
    authorized_role = _authorize(calling_role, "delete_entry", project_id=project_id)
    validate_project_id(project_id)
    validate_nonempty(entry_id, "entry_id")

    conn = await open_db()
    try:
        row = await fetch_live_entry(conn, project_id, entry_id)
        if row is None:
            audit_event(
                "delete_entry",
                allowed=False,
                project_id=project_id,
                entry_id=entry_id,
                calling_role=authorized_role,
                reason="no live entry for that project_id and entry_id",
            )
            raise ValueError("no entry found to delete")
        await conn.execute("BEGIN IMMEDIATE")
        await conn.execute(
            "UPDATE entries SET deleted = 1, last_updated = ? "
            "WHERE project_id = ? AND entry_id = ? AND deleted = 0",
            (utc_now(), project_id, entry_id),
        )
        seeded = await hashchain.append_store_record(conn, "delete_entry", entry_id)
        await conn.commit()
        classification = row["classification"]
    finally:
        await conn.close()

    audit_event(
        "delete_entry",
        allowed=True,
        project_id=project_id,
        entry_id=entry_id,
        classification=classification,
        calling_role=authorized_role,
    )
    if seeded is not None:
        return {"success": True, "chain_seeded": seeded}
    return {"success": True}


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
    """Parse the CLI flags and serve the storage server on the requested port."""
    global ALLOW_LIST, ALLOW_LIST_PATH, AUDIT_PATH, DB_PATH

    parser = argparse.ArgumentParser(description="Persistent storage MCP server")
    parser.add_argument("--port", type=int, default=8001, help="HTTP port (default 8001)")
    parser.add_argument("--host", default="0.0.0.0", help="bind address (default 0.0.0.0)")
    parser.add_argument("--db-path", default=DB_PATH, help=f"SQLite file (default {DB_PATH})")
    parser.add_argument(
        "--audit-path", default=AUDIT_PATH, help=f"audit journal (default {AUDIT_PATH})"
    )
    parser.add_argument(
        "--allowlist-path",
        default=str(ALLOW_LIST_PATH),
        help=f"role allow-list file (default {ALLOW_LIST_PATH})",
    )
    args = parser.parse_args()

    DB_PATH = args.db_path
    AUDIT_PATH = args.audit_path
    ALLOW_LIST_PATH = Path(args.allowlist_path).expanduser()
    try:
        ALLOW_LIST = load_allow_list(ALLOW_LIST_PATH)
    except AllowListError as error:
        # Fail before the port is bound: a server with no allow-list answers every role.
        print(f"ERROR: {error}", file=sys.stderr, flush=True)
        raise SystemExit(2) from error
    ensure_parent(DB_PATH)
    ensure_parent(AUDIT_PATH)
    print(f"storage database: {DB_PATH}", flush=True)
    print(f"storage audit log: {AUDIT_PATH}", flush=True)
    print(f"storage allow-list: {ALLOW_LIST_PATH}", flush=True)
    print(f"storage grants: {json.dumps(ALLOW_LIST, sort_keys=True)}", flush=True)

    import uvicorn

    uvicorn.run(build_app(), host=args.host, port=args.port)


if __name__ == "__main__":
    main()
