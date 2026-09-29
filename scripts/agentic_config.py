#!/usr/bin/env python3
"""Portable configuration for this repository's agentic quality gate.

`agentic.config.json` at the repository root is the single seam table for the gate: every value a
fork must depart from is named there, and the consumers read it with this module. The consumers are
`mcp/gate/server.py`, `scripts/run-agent.sh`, `scripts/classify-change.py`,
`scripts/validate_doc_conformance_deterministic.py` and `eval/test_policy.py`.

Three rules this module exists to hold:

* **Absent is a supported state.** `load()` never raises. A missing file, an unreadable file and a
  file that is not valid JSON all return the embedded `DEFAULT` below, so a fork may delete the
  config and keep working. The embedded defaults are this repository's current values, so a run
  with no config file behaves exactly as the tree did before the config existed.
* **One source of truth per value.** `get()` reads the config, and falls back to the embedded
  default for a key the config leaves out, so no consumer needs a second copy of a literal.
* **The shell side gets a one-line reader.** `get('toolchain.commands.clippy.argv')` prints as
  JSON, so `scripts/run-agent.sh` can read the same values without a JSON parser of its own.

CLI (used by `scripts/port-self-test.sh` and by hand):

    python3 scripts/agentic_config.py --print-config                  the resolved config as JSON
    python3 scripts/agentic_config.py --source                        the config path, or
                                                                      embedded-defaults
    python3 scripts/agentic_config.py --get toolchain.commands.fmt.argv
    python3 scripts/agentic_config.py --get containers.broker.port --default 0

Exit statuses: 0 on success, 2 on a malformed argument, with one line on stderr and no traceback.
"""

from __future__ import annotations

import copy
import json
import os
import sys
from pathlib import Path
from typing import Any

CONFIG_FILENAME = "agentic.config.json"

# The embedded defaults: the values `agentic.config.json` carries in this repository. Keep the two
# in step -- `python3 -c "import scripts.agentic_config as a, json; print(a.DEFAULT == json.load(
# open('agentic.config.json')))"` is the check, and scripts/port-self-test.sh relies on it.
DEFAULT: dict[str, Any] = {
    "schema_version": 1,
    "_purpose": [
        "The single seam table for this agentic quality gate. Every value here is a place where a"
        " fork",
        "must depart from Komun. The consumers are: mcp/gate/server.py, scripts/run-agent.sh,",
        "scripts/classify-change.py, scripts/validate_doc_conformance_deterministic.py and",
        "eval/test_policy.py. Each consumer reads this file and exposes what it consumed through",
        "`--print-config`, so scripts/port-self-test.sh can prove the wiring instead of trusting it.",
        "Absent or unreadable, every consumer falls back to the values below, which are this"
        " repository's.",
        "See PORTING.md for the fork checklist.",
    ],
    "project": {
        "name": "komun",
        "language": "rust",
        "repo_marker": "Cargo.toml",
    },
    "toolchain": {
        "commands": {
            "test": {
                "argv": ["cargo", "test", "--workspace"],
                "description": "unit-test gate: the workspace test suites",
                "guard": None,
            },
            "clippy": {
                "argv": [
                    "cargo",
                    "clippy",
                    "--release",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
                "description": "lint gate: release clippy over all targets, warnings are errors",
                "guard": {
                    "marker": "Checking komun-server",
                    "marker_regex": "\\bChecking\\b\\s+(?P<marker>komun-server)\\b",
                    "touch_file": "crates/server/src/main.rs",
                    "reason": (
                        "a cached clippy run prints nothing and exits 0, which is indistinguishable"
                        " from a clean lint; the touched file forces a real check"
                    ),
                },
            },
            "fmt": {
                "argv": ["cargo", "fmt", "--check"],
                "description": "formatting gate: check mode, no files are written",
                "guard": None,
            },
            "policy": {
                "argv": [
                    "python3",
                    "-m",
                    "pytest",
                    "eval/test_policy.py",
                    "eval/test_deterministic_step.py",
                    "-q",
                ],
                "description": "policy gate: the governance and deterministic-step eval suites",
                "guard": None,
            },
            "conformance": {
                "argv": ["python3", "scripts/run-conformance-gate.py"],
                "description": (
                    "conformance gate: the prose and citation check, run against base and failing"
                    " on new drift only"
                ),
                "guard": None,
            },
        },
        "cache_dirs": ["target"],
    },
    "containers": {
        "base_image": "agent-sandbox:komun",
        "tools_image": "agent-sandbox:komun-m3",
        "workspace": "/workspace",
        "memory_subpath": ".memory",
        "memory_dir": "/workspace/.memory",
        "registry_volume": "komun-cargo-registry",
        "networks": {
            "internal": "agent-internal",
            "broker": "agent-net",
        },
        "broker": {
            "name": "rev-broker",
            "port": 4000,
        },
        "read_only_overlays": [
            "mcp/storage/allow-list.json",
            "mcp/retrieval/allow-list.json",
            "docs/routing-and-tool-grant-map.json",
            "docs/routing-and-tool-grant-map.md",
        ],
        "journals": [
            ".memory/storage-audit.log",
            ".memory/retrieval-audit.log",
            ".memory/gate-audit.log",
        ],
        "cargo_target_cache": "cargo-target",
        "cargo_registry_cache": "cargo-registry",
    },
    "roles": {
        "valid": [
            "orchestrator",
            "planner",
            "implementer",
            "tester",
            "reviewer",
            "project-manager",
            "researcher",
        ],
        "mounts": {
            "orchestrator": {"workspace": "rw", "memory": "ro", "build_cache": "ro"},
            "planner": {"workspace": "ro", "memory": "rw", "build_cache": "ro"},
            "implementer": {"workspace": "rw", "memory": "rw", "build_cache": "ro"},
            "tester": {"workspace": "ro", "memory": "rw", "build_cache": "rw"},
            "reviewer": {"workspace": "ro", "memory": "rw", "build_cache": "ro"},
            "project-manager": {"workspace": "ro", "memory": "none", "build_cache": "ro"},
            "researcher": {"workspace": "ro", "memory": "rw", "build_cache": "ro"},
        },
    },
    "artifacts": {
        "definitions_dir": ".claude/agents",
        "skills_dir": ".claude/skills",
        "settings": ".claude/settings.json",
        "policy_document": "docs/governance-policy.md",
        "grant_map_md": "docs/routing-and-tool-grant-map.md",
        "grant_map_json": "docs/routing-and-tool-grant-map.json",
        "calibration_log": "docs/calibration-log.md",
        "step_classification": "docs/step-classification.md",
        "style_rules": "docs/DOC-STYLE.md",
        "storage_allow_list": "mcp/storage/allow-list.json",
        "retrieval_allow_list": "mcp/retrieval/allow-list.json",
        "storage_server": "mcp/storage/server.py",
        "retrieval_server": "mcp/retrieval/server.py",
        "gate_server": "mcp/gate/server.py",
        "policy_suite": "eval/test_policy.py",
        "launcher": "scripts/run-agent.sh",
        "pipeline": ".github/workflows/ci.yml",
        "project_key": "proj-komun",
        "coursetools_server": "mcp/coursetools_server.py",
    },
    "classification": {
        "governed_globs": [
            ".claude/agents/*",
            ".claude/agents/**",
            ".claude/skills/*",
            ".claude/skills/**",
            "mcp/*",
            "mcp/**",
            "eval/*",
            "eval/**",
            "docs/governance-policy.md",
            "docs/routing-and-tool-grant-map.md",
            "docs/routing-and-tool-grant-map.json",
            "docs/calibration-log.md",
            "scripts/run-agent.sh",
            ".github/workflows/*",
            ".github/workflows/**",
            "scripts/classify-change.py",
            "scripts/run-reviewer.py",
            "scripts/build-audit-trail.py",
        ],
        "policy_globs": [
            "docs/governance-policy.md",
            "mcp/*/allow-list.json",
            "docs/routing-and-tool-grant-map.json",
            "docs/routing-and-tool-grant-map.md",
            "mcp/*/server.py",
            "scripts/run-agent.sh",
            ".github/workflows/*",
            ".github/workflows/**",
        ],
    },
    "port": {
        "komun_defaults": {
            "project.name": "komun",
            "toolchain.commands.clippy.guard.marker": "Checking komun-server",
            "toolchain.commands.clippy.guard.marker_regex": (
                "\\bChecking\\b\\s+(?P<marker>komun-server)\\b"
            ),
            "toolchain.commands.clippy.guard.touch_file": "crates/server/src/main.rs",
            "containers.base_image": "agent-sandbox:komun",
            "containers.tools_image": "agent-sandbox:komun-m3",
            "containers.registry_volume": "komun-cargo-registry",
            "containers.networks.internal": "agent-internal",
            "containers.networks.broker": "agent-net",
            "containers.broker.name": "rev-broker",
            "artifacts.project_key": "proj-komun",
            "artifacts.style_rules": "docs/DOC-STYLE.md",
        },
        "language_specific_note": [
            "The cargo toolchain commands, the clippy cache guard and the build cache are Rust and"
            " cargo",
            "specific. A Node or Python fork replaces the whole `toolchain` block and drops the"
            " guard, or",
            "writes a guard of its own: the guard exists only because a cached linter prints"
            " nothing.",
        ],
    },
    "gates": {
        "conformance": {
            "files": [
                "AGENTS.md",
                "CLAUDE.md",
                "docs/DOC-STYLE.md",
                "docs/governance-policy.md",
                "docs/routing-and-tool-grant-map.md",
                "docs/policy-reconciliation.md",
                "docs/step-classification.md",
                "docs/iteration-log.md",
                "docs/memory-architecture.md",
                "docs/orchestration-diagram.md",
              "mcp/gate/SCHEMA.md",
      "docs/calibration-log.md"
    ],
        },
    },
    "console": {
        "_purpose": [
            "The runtime facts a terminal console needs that the rest of this file does not carry. A",
            "console is a window onto the artifacts, never a source of them: it reads this block, the",
            "journals and `docker ps`, and it never writes inside the repository. Every value below is",
            "also a Komun default until a fork changes it; a fork that changes one deletes its key from",
            "`komun_defaults` below, which is exactly what the console reads to flag a value as still",
            "Komun's rather than the fork's.",
        ],
        "container": "agent-rev-m3",
        "claude_command": "claude",
        "claude_flags": ["--agent", "orchestrator", "--permission-mode", "acceptEdits"],
        "role_container_prefix": "agent-rev-m4-",
        "ports": {"gate": 8003, "storage": 8001, "retrieval": 8002},
        "evidence_dir": "~/komun-agent-exercise-4-3",
        "briefs_dir": "~/komun-agent-exercise-4-3/briefs",
        "checkpoint_fresh_minutes": 30,
        "ci_jobs": [
            {"name": "change-type-check", "needs": [], "gating": True},
            {"name": "policy-gate", "needs": ["change-type-check"], "gating": True},
            {"name": "eval-gate", "needs": ["change-type-check", "policy-gate"], "gating": True},
            {"name": "advisory-review", "needs": ["change-type-check", "policy-gate"], "gating": False},
            {
                "name": "audit-trail",
                "needs": ["change-type-check", "policy-gate", "eval-gate", "advisory-review"],
                "gating": False,
            },
        ],
        "orchestration_steps": [
            {"label": "project-manager opens the ticket", "kind": "role", "role": "project-manager"},
            {"label": "planner plans", "kind": "role", "role": "planner"},
            {"label": "HUMAN CHECKPOINT 1 (plan approval)", "kind": "human"},
            {"label": "implementer writes", "kind": "role", "role": "implementer"},
            {"label": "tester runs gates", "kind": "role", "role": "tester"},
            {
                "label": "reviewer reads the journal and records a verdict",
                "kind": "role",
                "role": "reviewer",
            },
            {"label": "HUMAN CHECKPOINT 2 (release approval)", "kind": "human"},
            {"label": "project-manager closes", "kind": "role", "role": "project-manager"},
        ],
        "komun_defaults": [
            "container",
            "claude_command",
            "claude_flags",
            "role_container_prefix",
            "ports",
            "evidence_dir",
            "briefs_dir",
            "checkpoint_fresh_minutes",
            "ci_jobs",
            "orchestration_steps",
        ],
    },
}

_CACHE: dict[str, Any] | None = None
_SOURCE: str | None = None


# --- Reading the config ----------------------------------------------------------------------
def _read_json(path: Path | None) -> dict[str, Any] | None:
    """The JSON object at `path`, or None when it is missing, unreadable or not a JSON object."""
    if path is None:
        return None
    try:
        parsed = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError, UnicodeDecodeError):
        return None
    return parsed if isinstance(parsed, dict) else None


def _marker(data: dict[str, Any] | None) -> str:
    """The repository marker file named by a config, or the embedded default."""
    project = (data or {}).get("project")
    if isinstance(project, dict):
        named = project.get("repo_marker")
        if isinstance(named, str) and named:
            return named
    return str(DEFAULT["project"]["repo_marker"])


def repo_root(start: str | os.PathLike[str] | None = None, marker: str | None = None) -> Path:
    """The repository root: the first directory at or above `start` holding `marker`.

    `start` defaults to this file. When no directory holds the marker, the fallback is this file's
    own parent's parent, which is the repository root of a checkout laid out as `<root>/scripts/`.
    """
    here = Path(start).resolve() if start else Path(__file__).resolve()
    directory = here if here.is_dir() else here.parent
    name = marker or str(DEFAULT["project"]["repo_marker"])
    for candidate in (directory, *directory.parents):
        if (candidate / name).is_file():
            return candidate
    return Path(__file__).resolve().parent.parent


def resolve_path(path: str | os.PathLike[str] | None = None) -> Path:
    """The config file this run reads.

    Order: the explicit argument, then `$AGENTIC_CONFIG`, then `<repo root>/agentic.config.json`.
    The repo root is found by walking up from this file for the marker file (default `Cargo.toml`),
    and a config that names a different marker in `project.repo_marker` re-resolves the root with
    it.
    """
    if path is not None:
        return Path(path).expanduser()
    from_env = os.environ.get("AGENTIC_CONFIG")
    if from_env:
        return Path(from_env).expanduser()
    marker = str(DEFAULT["project"]["repo_marker"])
    candidate = repo_root(marker=marker) / CONFIG_FILENAME
    named = _marker(_read_json(candidate))
    if named != marker:
        relocated = repo_root(marker=named) / CONFIG_FILENAME
        if relocated.is_file():
            return relocated
    return candidate


def load(path: str | os.PathLike[str] | None = None) -> dict[str, Any]:
    """The config as a dict. Never raises: bad input yields the embedded defaults.

    A `path` argument is read fresh; no argument reads the resolved path once and caches it.
    """
    global _CACHE, _SOURCE
    if path is None and _CACHE is not None:
        return copy.deepcopy(_CACHE)
    resolved = resolve_path(path)
    data = _read_json(resolved)
    source: str | None = None
    if data is None:
        data = DEFAULT
    else:
        try:
            source = str(Path(resolved).expanduser().resolve())
        except OSError:
            source = str(resolved)
    if path is None:
        _CACHE, _SOURCE = data, source
    return copy.deepcopy(data)


def source() -> str:
    """The absolute path of the config in use, or `embedded-defaults` after a fallback."""
    load()
    return _SOURCE if _SOURCE else "embedded-defaults"


def _dig(data: Any, dotted_key: str) -> Any:
    node: Any = data
    for part in dotted_key.split("."):
        if not isinstance(node, dict) or part not in node:
            return None
        node = node[part]
    return node


def get(dotted_key: str, default: Any = None) -> Any:
    """One value by dotted key, e.g. `toolchain.commands.clippy.guard.marker`.

    The config wins; a key the config leaves out falls back to the embedded default, and only a key
    in neither returns `default`. `scripts/run-agent.sh` reads this over the CLI.
    """
    if not isinstance(dotted_key, str):
        return default
    value = _dig(load(), dotted_key)
    if value is None:
        value = _dig(DEFAULT, dotted_key)
    return default if value is None else value


# --- CLI -------------------------------------------------------------------------------------
USAGE = "usage: agentic_config.py (--print-config | --source | --get <dotted.key> [--default <v>])"


def _render(value: Any) -> str:
    """A value as the CLI prints it: a list as a JSON array, a bool as true/false, None as null."""
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (list, dict)):
        return json.dumps(value, sort_keys=isinstance(value, dict))
    return str(value)


def _fail(message: str) -> int:
    print(f"agentic_config: {message}", file=sys.stderr)
    return 2


def main(argv: list[str] | None = None) -> int:
    """Parse the CLI flags and print what was asked for."""
    args = list(sys.argv[1:] if argv is None else argv)
    if not args:
        return _fail(f"no mode given; {USAGE}")
    mode = args[0]
    if mode in ("-h", "--help"):
        print(USAGE)
        print("  --print-config         the resolved config as JSON, sorted keys")
        print("  --source               the absolute config path, or embedded-defaults")
        print("  --get <dotted.key> [--default <value>]   one value")
        return 0
    if mode in ("--print-config", "--source"):
        if len(args) != 1:
            return _fail(f"{mode} takes no further argument; {USAGE}")
        if mode == "--print-config":
            print(json.dumps(load(), indent=2, sort_keys=True))
        else:
            print(source())
        return 0
    if mode == "--get":
        if len(args) < 2 or not args[1] or args[1].startswith("--"):
            return _fail(f"--get needs a dotted key; {USAGE}")
        key = args[1]
        rest = args[2:]
        fallback: Any = None
        if rest:
            if len(rest) != 2 or rest[0] != "--default":
                return _fail(f"unexpected argument {rest[0]!r} after --get; {USAGE}")
            fallback = rest[1]
        print(_render(get(key, fallback)))
        return 0
    return _fail(f"unknown argument {mode!r}; {USAGE}")


if __name__ == "__main__":
    sys.exit(main())
