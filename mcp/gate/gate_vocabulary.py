#!/usr/bin/env python3
"""The gate vocabulary this repository's gate server exposes, read from ``agentic.config.json``.

``mcp/gate/server.py`` is the execution surface; everything a fork must change about it -- the gate
names, the argv tuple of each, the clippy cache-hit guard, the container paths the gates run in --
is read here, from the config's ``toolchain.commands`` and ``containers`` blocks, instead of being
written into the server. The command names under ``toolchain.commands`` ARE the vocabulary: a fork
adds a gate by adding one entry there and changing no Python. Nothing in the vocabulary is
caller-supplied: it is composed once, at import time, from the config alone.

The loader is stdlib only and falls back to its own embedded defaults, which are this repository's
values, so the server behaves exactly as it did before the config existed when the config file is
absent, unreadable or not valid JSON.

``print_config_if_requested()`` answers the server's ``--print-config`` flag: a JSON object of
exactly the config keys this vocabulary consumes, sorted, with no gate run and no journal read.
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path
from typing import Any

# --- The loader, found from this file --------------------------------------------------------
_SCRIPTS = next(
    (
        parent / "scripts"
        for parent in Path(__file__).resolve().parents
        if (parent / "scripts" / "agentic_config.py").is_file()
    ),
    Path(__file__).resolve().parents[2] / "scripts",
)
if str(_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS))
import agentic_config  # noqa: E402 - the loader sits in the path inserted above

# What this vocabulary consumes. `--print-config` prints exactly these keys, sorted.
CONFIG_KEYS: tuple[str, ...] = (
    "toolchain.commands.test.argv",
    "toolchain.commands.test.description",
    "toolchain.commands.clippy.argv",
    "toolchain.commands.clippy.description",
    "toolchain.commands.clippy.guard.marker",
    "toolchain.commands.clippy.guard.marker_regex",
    "toolchain.commands.clippy.guard.touch_file",
    "toolchain.commands.clippy.guard.reason",
    "toolchain.commands.fmt.argv",
    "toolchain.commands.fmt.description",
    "toolchain.commands.policy.argv",
    "toolchain.commands.policy.description",
    "toolchain.commands.conformance.argv",
    "toolchain.commands.conformance.description",
    "containers.workspace",
    "containers.memory_dir",
    "containers.tools_image",
)

# --- Runtime paths: container defaults, every one overridable for a local run ----------------
WORKSPACE = os.getenv("GATE_WORKSPACE", str(agentic_config.get("containers.workspace")))
MEMORY_DIR = os.getenv("MEMORY_DIR", str(agentic_config.get("containers.memory_dir")))
AUDIT_PATH = os.getenv("GATE_AUDIT_PATH", str(Path(MEMORY_DIR) / "gate-audit.log"))
TOOLS_IMAGE = str(agentic_config.get("containers.tools_image"))


def _gate(name: str) -> dict[str, Any]:
    """One gate's argv, description and guard, from ``toolchain.commands.<name>``.

    The embedded defaults answer for anything the config leaves out, so a missing config, a missing
    key and a complete config all yield a usable gate. A gate the config names but gives no argv to
    is reported as the config error it is: an empty argv would be a gate that runs nothing, which is
    the failure this table exists to make unrepresentable. ``argv`` is a tuple of strings and
    ``guard`` is None or exactly the three fields the server's cache-hit guard reads, whatever else
    the config carries beside them.
    """
    embedded = agentic_config.DEFAULT["toolchain"]["commands"].get(name, {})
    command = agentic_config.get(f"toolchain.commands.{name}")
    if not isinstance(command, dict):
        command = embedded
    argv = command.get("argv")
    if not isinstance(argv, (list, tuple)) or not argv:
        argv = embedded.get("argv")
    if not isinstance(argv, (list, tuple)) or not argv:
        raise ValueError(
            f"gate '{name}' names no argv: give it one under "
            f"toolchain.commands.{name}.argv in agentic.config.json"
        )
    guard = command.get("guard")
    if isinstance(guard, dict) and guard.get("marker"):
        guard = {field: str(guard.get(field, "")) for field in ("marker", "touch_file", "reason")}
    else:
        guard = None
    return {
        "argv": tuple(str(element) for element in argv),
        "description": str(command.get("description") or embedded.get("description", "")),
        "guard": guard,
    }


def _command_names() -> tuple[str, ...]:
    """The gate names, in the order the config lists them under ``toolchain.commands``.

    The command names are the vocabulary, so a fork adds or removes a gate by editing the config and
    nothing else. A config that names no command falls back to the embedded defaults, which are this
    repository's five names.
    """
    commands = agentic_config.get("toolchain.commands")
    if not isinstance(commands, dict) or not commands:
        commands = agentic_config.DEFAULT["toolchain"]["commands"]
    return tuple(str(name) for name in commands)


# --- Gate vocabulary: the whole execution surface of the gate server -------------------------
# The argv tuples below are the only commands the server can ever run. Nothing else is composed,
# interpolated or appended at call time, and the caller contributes no element of any argv. The
# names are the config's `toolchain.commands` keys, so the table below is built rather than written.
GATE_NAMES: tuple[str, ...] = _command_names()
GATES: dict[str, dict[str, Any]] = {name: _gate(name) for name in GATE_NAMES}

# The clippy cache-hit guard's marker pattern. cargo honours CARGO_TERM_COLOR=always in the sandbox
# image even when stderr is a pipe, so the server matches this pattern against output with the SGR
# escapes stripped; a fork that renames the crate names its own crate here.
try:
    GUARD_MARKER_PATTERN = re.compile(
        str(agentic_config.get("toolchain.commands.clippy.guard.marker_regex"))
    )
except re.error:  # a fork's pattern that will not compile: the embedded default answers
    GUARD_MARKER_PATTERN = re.compile(
        str(agentic_config.DEFAULT["toolchain"]["commands"]["clippy"]["guard"]["marker_regex"])
    )


def print_config_if_requested(argv: list[str]) -> bool:
    """Print what this vocabulary consumes when ``argv`` asks for it, and say whether it did."""
    if "--print-config" not in list(argv):
        return False
    print(
        json.dumps({key: agentic_config.get(key) for key in CONFIG_KEYS}, indent=2, sort_keys=True)
    )
    return True
