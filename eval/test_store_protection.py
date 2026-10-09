#!/usr/bin/env python3
"""Regression tests for the store protection: role boxes cannot write the store directly.

Every mount assertion reads the launchers' own ``--print-mounts`` output as data, so no container
starts and no docker command runs. The cross-check cases build their store and journal only
through the storage server's own write path, in a temporary directory.

    python3 -m pytest eval/test_store_protection.py -q
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[1]
LAUNCHER = REPO / "scripts" / "run-agent.sh"
SIDECAR = REPO / "scripts" / "run-agent-servers.sh"
CROSSCHECK = REPO / "scripts" / "chain_crosscheck.py"
STORAGE_SERVER = REPO / "mcp" / "storage" / "server.py"
POLICY = REPO / "docs" / "governance-policy.md"
GRANT_MAP = REPO / "docs" / "routing-and-tool-grant-map.json"
CONFIG = REPO / "agentic.config.json"

ROLES = (
    "orchestrator",
    "planner",
    "implementer",
    "tester",
    "reviewer",
    "project-manager",
    "researcher",
    "beta-tester",
)
WORKSPACE = "/workspace"
MEMORY = "/workspace/.memory"
PROJECT = "/workspace/.memory/project"

# The inputs a role box's own limits are enforced against. A box that could edit one of them could
# change what binds, authorises or mounts the next box, so each must be overlaid read-only.
ENFORCEMENT_INPUTS = (
    "agentic.config.json",
    ".mcp.json",
    "sandbox/opencode-sandbox.json",
    "scripts/run-agent.sh",
    "scripts/start-mcp-servers.sh",
    "scripts/run-agent-servers.sh",
    ".claude/settings.json",
    ".claude/hooks",
    "scripts/agentic_config.py",
    "scripts/budget.py",
)

# Inherited values for these would change what the scripts print, so every run starts without them.
SCRUBBED_ENV = (
    "ROLE_MEM",
    "ROLE_MEM_PROJECT",
    "REPO",
    "NAME",
    "BOX_NAME",
    "IMAGE",
    "TARGET_VOL",
    "REGISTRY_VOL",
    "NET_INTERNAL",
    "AGENTIC_CONFIG",
    "AGENT_ROLE",
    "BETA_BASE_URL",
)


def _env(**extra: str) -> dict[str, str]:
    env = {key: value for key, value in os.environ.items() if key not in SCRUBBED_ENV}
    env.update(extra)
    return env


def _run(argv: list[str], **extra: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        argv, env=_env(**extra), capture_output=True, text=True, check=False, timeout=300
    )


def _mounts(stdout: str) -> dict[str, tuple[str, str]]:
    """Map each bind destination to (source, mode) from ``-v SRC:DST[:ro]`` lines."""
    mounts: dict[str, tuple[str, str]] = {}
    for line in stdout.splitlines():
        if not line.startswith("-v "):
            continue
        spec = line[len("-v ") :]
        mode = "rw"
        if spec.endswith(":ro"):
            spec, mode = spec[: -len(":ro")], "ro"
        source, destination = spec.rsplit(":", 1)
        assert destination not in mounts, f"two binds on {destination}: {stdout}"
        mounts[destination] = (source, mode)
    return mounts


def _store_writers() -> set[str]:
    grants = json.loads(GRANT_MAP.read_text(encoding="utf-8"))["grants"]
    return {role for role, tools in grants.items() if "mcp__storage__write_entry" in tools}


def _expected(role: str) -> dict[str, str]:
    """The required memory and memory_project modes for a role box."""
    if role == "project-manager":
        return {"memory": "none", "memory_project": "none"}
    if role in _store_writers():
        return {"memory": "ro", "memory_project": "rw"}
    return {"memory": "ro", "memory_project": "none"}


def _fake_docker(tmp_path: Path) -> tuple[dict[str, str], Path]:
    """A PATH whose docker only records that it was called."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    marker = tmp_path / "docker-was-called"
    docker = bin_dir / "docker"
    docker.write_text(f"#!/bin/sh\ntouch '{marker}'\nexit 97\n", encoding="utf-8")
    docker.chmod(0o755)
    return {"PATH": f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '')}"}, marker


def _workspace_modes() -> dict[str, str]:
    """Each role's effective workspace mode, as the launcher resolves it."""
    completed = _run(["bash", str(LAUNCHER), "--print-config"])
    assert completed.returncode == 0, completed.stderr
    mounts = json.loads(completed.stdout)["roles.mounts"]
    return {role: mounts[role]["workspace"] for role in ROLES}


def _assert_refused(completed: subprocess.CompletedProcess[str], marker: Path, reason: str) -> None:
    assert completed.returncode == 2, (completed.stdout, completed.stderr)
    assert not completed.stdout.strip(), completed.stdout
    assert "refused" in completed.stderr and reason in completed.stderr, completed.stderr
    assert not marker.exists(), "docker was called before the refusal"


# --- role boxes -------------------------------------------------------------------------------
def test_store_writers_are_the_six_roles() -> None:
    assert _store_writers() == {
        "planner",
        "implementer",
        "tester",
        "reviewer",
        "researcher",
        "beta-tester",
    }


@pytest.mark.parametrize("role", ROLES)
def test_role_box_mounts_memory_read_only_and_only_project_writable(role: str) -> None:
    completed = _run(["bash", str(LAUNCHER), "--print-mounts", role])
    assert completed.returncode == 0, completed.stderr
    mounts = _mounts(completed.stdout)
    expected = _expected(role)

    if expected["memory"] == "none":
        assert MEMORY not in mounts, completed.stdout
    else:
        assert MEMORY in mounts, completed.stdout
        source, mode = mounts[MEMORY]
        assert source.endswith("/.memory") and mode == expected["memory"], completed.stdout

    if expected["memory_project"] == "rw":
        assert PROJECT in mounts, completed.stdout
        source, mode = mounts[PROJECT]
        assert source.endswith("/.memory/project") and mode == "rw", completed.stdout
    else:
        assert PROJECT not in mounts, completed.stdout

    writable_under_memory = sorted(
        destination
        for destination, (_, mode) in mounts.items()
        if mode == "rw" and (destination == MEMORY or destination.startswith(MEMORY + "/"))
    )
    allowed = [PROJECT] if expected["memory_project"] == "rw" else []
    assert writable_under_memory == allowed, completed.stdout


@pytest.mark.parametrize("role", ROLES)
def test_role_box_overlays_the_enforcement_inputs_read_only(role: str) -> None:
    expected = _expected(role)
    completed = _run(
        ["bash", str(LAUNCHER), "--print-mounts", role],
        ROLE_MEM=expected["memory"],
        ROLE_MEM_PROJECT=expected["memory_project"],
    )
    assert completed.returncode == 0, completed.stderr
    mounts = _mounts(completed.stdout)
    not_read_only = [
        path
        for path in ENFORCEMENT_INPUTS
        if f"{WORKSPACE}/{path}" not in mounts
        or mounts[f"{WORKSPACE}/{path}"][1] != "ro"
        or not mounts[f"{WORKSPACE}/{path}"][0].endswith(f"/{path}")
    ]
    assert not_read_only == [], completed.stdout


# The launcher skips an overlay whose source is absent, so a missing path would drop silently.
def test_enforcement_overlays_exist_in_the_checkout() -> None:
    missing = [path for path in ENFORCEMENT_INPUTS if not (REPO / path).exists()]
    assert missing == [], missing


def test_environment_override_wins_over_the_config() -> None:
    completed = _run(
        ["bash", str(LAUNCHER), "--print-mounts", "implementer"],
        ROLE_MEM="ro",
        ROLE_MEM_PROJECT="rw",
    )
    assert completed.returncode == 0, completed.stderr
    mounts = _mounts(completed.stdout)
    assert mounts[MEMORY][1] == "ro" and mounts[PROJECT][1] == "rw", completed.stdout

    completed = _run(
        ["bash", str(LAUNCHER), "--print-mounts", "planner"],
        ROLE_MEM="ro",
        ROLE_MEM_PROJECT="none",
    )
    assert completed.returncode == 0, completed.stderr
    assert PROJECT not in _mounts(completed.stdout), completed.stdout


@pytest.mark.parametrize("variable", ["ROLE_MEM", "ROLE_MEM_PROJECT"])
def test_environment_override_rejects_an_unknown_mode(variable: str) -> None:
    completed = _run(["bash", str(LAUNCHER), "--print-mounts", "implementer"], **{variable: "rwx"})
    assert completed.returncode == 2
    assert not completed.stdout.strip()
    assert variable in completed.stderr


# --- refusals: no resolved profile leaves .memory writable in a role box ----------------------
@pytest.mark.parametrize("role", ROLES)
def test_launcher_refuses_memory_rw_for_every_role(role: str, tmp_path: Path) -> None:
    path_env, marker = _fake_docker(tmp_path)
    completed = _run(
        ["bash", str(LAUNCHER), "--print-mounts", role],
        ROLE_MEM="rw",
        ROLE_MEM_PROJECT="none",
        **path_env,
    )
    _assert_refused(completed, marker, "memory=rw")


def test_launcher_refuses_memory_none_under_a_read_write_workspace(tmp_path: Path) -> None:
    path_env, marker = _fake_docker(tmp_path)
    writable = sorted(role for role, mode in _workspace_modes().items() if mode == "rw")
    assert {"orchestrator", "implementer"} <= set(writable), writable
    for role in writable:
        completed = _run(
            ["bash", str(LAUNCHER), "--print-mounts", role],
            ROLE_MEM="none",
            ROLE_MEM_PROJECT="none",
            **path_env,
        )
        _assert_refused(completed, marker, "memory=none with workspace=rw")


def test_launcher_accepts_memory_none_under_a_read_only_workspace() -> None:
    assert _workspace_modes()["project-manager"] == "ro"
    completed = _run(
        ["bash", str(LAUNCHER), "--print-mounts", "project-manager"],
        ROLE_MEM="none",
        ROLE_MEM_PROJECT="none",
    )
    assert completed.returncode == 0, completed.stderr
    mounts = _mounts(completed.stdout)
    assert MEMORY not in mounts and PROJECT not in mounts, completed.stdout


def test_launcher_refuses_a_project_bind_without_a_read_only_memory(tmp_path: Path) -> None:
    path_env, marker = _fake_docker(tmp_path)
    read_only = sorted(role for role, mode in _workspace_modes().items() if mode == "ro")
    assert "project-manager" in read_only, read_only
    for role in read_only:
        completed = _run(
            ["bash", str(LAUNCHER), "--print-mounts", role],
            ROLE_MEM="none",
            ROLE_MEM_PROJECT="rw",
            **path_env,
        )
        _assert_refused(completed, marker, "memory_project=rw needs memory=ro")


def test_launch_refuses_before_any_docker_call(tmp_path: Path) -> None:
    path_env, marker = _fake_docker(tmp_path)
    completed = _run(["bash", str(LAUNCHER), "implementer", "bash"], ROLE_MEM="rw", **path_env)
    _assert_refused(completed, marker, "memory=rw")


# A config mode that matches no build_mounts arm adds no .memory bind, which leaves .memory writable
# through a read-write workspace bind.
def test_launcher_refuses_a_non_canonical_config_memory_mode(tmp_path: Path) -> None:
    config = json.loads(CONFIG.read_text(encoding="utf-8"))
    config["roles"]["mounts"]["implementer"]["memory"] = "RO"
    config_copy = tmp_path / "agentic.config.json"
    config_copy.write_text(json.dumps(config), encoding="utf-8")
    path_env, marker = _fake_docker(tmp_path)
    for argv in (["--print-mounts", "implementer"], ["implementer", "bash"]):
        completed = _run(
            ["bash", str(LAUNCHER), *argv], AGENTIC_CONFIG=str(config_copy), **path_env
        )
        _assert_refused(completed, marker, "memory=RO is not ro or none")


def test_launcher_print_mounts_makes_no_docker_call_and_no_directory(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    (repo / ".memory").mkdir(parents=True)
    path_env, marker = _fake_docker(tmp_path)
    completed = _run(
        ["bash", str(LAUNCHER), "--print-mounts", "implementer"],
        REPO=str(repo),
        ROLE_MEM="ro",
        ROLE_MEM_PROJECT="rw",
        **path_env,
    )
    assert completed.returncode == 0, completed.stderr
    assert not marker.exists()
    assert not (repo / ".memory" / "project").exists()
    assert not (repo / "target").exists()


def test_launcher_print_config_carries_memory_project() -> None:
    completed = _run(["bash", str(LAUNCHER), "--print-config"])
    assert completed.returncode == 0, completed.stderr
    mounts = json.loads(completed.stdout)["roles.mounts"]
    for role in ROLES:
        assert mounts[role]["memory_project"] in ("rw", "none"), mounts[role]


@pytest.mark.parametrize("role", sorted(set(ROLES) - {"orchestrator", "project-manager"}))
def test_matrix_memory_cell_reads_as_read_only(role: str) -> None:
    completed = _run(["bash", str(LAUNCHER), "--matrix"])
    assert completed.returncode == 0, completed.stderr
    rows = [line for line in completed.stdout.splitlines() if line.startswith(f"| `{role}` |")]
    assert len(rows) == 1, completed.stdout
    memory_cell = rows[0].strip().strip("|").split("|")[2].strip()
    assert "read-only" in memory_cell and "read-write" not in memory_cell, memory_cell
    assert "/workspace/.memory/project" in memory_cell, memory_cell


# --- configuration ----------------------------------------------------------------------------
@pytest.mark.parametrize("role", ROLES)
def test_config_mounts_match_the_store_protection(role: str) -> None:
    mounts = json.loads(CONFIG.read_text(encoding="utf-8"))["roles"]["mounts"]
    assert role in mounts, sorted(mounts)
    expected = _expected(role)
    assert mounts[role].get("memory") == expected["memory"], mounts[role]
    assert mounts[role].get("memory_project") == expected["memory_project"], mounts[role]


def test_loader_defaults_match_the_config_mounts() -> None:
    sys.path.insert(0, str(REPO / "scripts"))
    try:
        import agentic_config  # noqa: PLC0415
    finally:
        sys.path.remove(str(REPO / "scripts"))
    config = json.loads(CONFIG.read_text(encoding="utf-8"))
    assert agentic_config.DEFAULT["roles"]["valid"] == config["roles"]["valid"]
    assert agentic_config.DEFAULT["roles"]["mounts"] == config["roles"]["mounts"]


def test_policy_gate_runs_this_file() -> None:
    argv = json.loads(CONFIG.read_text(encoding="utf-8"))["toolchain"]["commands"]["policy"]["argv"]
    assert "eval/test_store_protection.py" in argv, argv


# --- the policy text --------------------------------------------------------------------------
def _container_paragraph(role: str) -> str:
    lines = POLICY.read_text(encoding="utf-8").splitlines()
    heading = next(i for i, line in enumerate(lines) if line.startswith("## Role ") and f"`{role}`" in line)
    for line in lines[heading + 1 :]:
        if line.startswith("## "):
            break
        if line.startswith("**Container permissions**"):
            return line
    raise AssertionError(f"no Container permissions paragraph for {role}")


@pytest.mark.parametrize("role", ROLES)
def test_policy_states_the_project_bind_and_the_sidecar_identity(role: str) -> None:
    paragraph = _container_paragraph(role)
    names_project_bind = "binds `/workspace/.memory/project` read-write" in paragraph
    assert names_project_bind == (_expected(role)["memory_project"] == "rw"), paragraph
    assert "mounts `/workspace/.memory` read-write" not in paragraph, paragraph
    assert "binds `AGENT_ROLE` to this role" in paragraph, paragraph
    if role == "orchestrator":
        assert "may run unbound, and only with `--unbound`" in paragraph, paragraph
        assert "single documented unbound" not in paragraph, paragraph


# --- the sidecar ------------------------------------------------------------------------------
@pytest.mark.parametrize("role", ROLES)
def test_sidecar_mounts_memory_read_write_and_binds_its_role_by_default(role: str) -> None:
    completed = _run(["bash", str(SIDECAR), "--print-mounts", role])
    assert completed.returncode == 0, completed.stderr
    mounts = _mounts(completed.stdout)
    assert mounts[WORKSPACE][1] == "rw", completed.stdout
    assert mounts[MEMORY][0].endswith("/.memory") and mounts[MEMORY][1] == "rw", completed.stdout
    assert all(mode == "rw" for _, mode in mounts.values()), completed.stdout
    assert f"-e AGENT_ROLE={role}" in completed.stdout.splitlines(), completed.stdout


def test_sidecar_stays_bound_whatever_the_environment_says() -> None:
    completed = _run(
        ["bash", str(SIDECAR), "--print-mounts", "orchestrator"], AGENT_ROLE="", UNBOUND="1"
    )
    assert completed.returncode == 0, completed.stderr
    assert "-e AGENT_ROLE=orchestrator" in completed.stdout.splitlines(), completed.stdout


def test_sidecar_unbound_needs_the_flag_and_the_orchestrator() -> None:
    completed = _run(["bash", str(SIDECAR), "--print-mounts", "orchestrator", "--unbound"])
    assert completed.returncode == 0, completed.stderr
    assert "-e AGENT_ROLE=" in completed.stdout.splitlines(), completed.stdout

    for role in sorted(set(ROLES) - {"orchestrator"}):
        refused = _run(["bash", str(SIDECAR), "--print-mounts", role, "--unbound"])
        assert refused.returncode == 2, (role, refused.stdout)
        assert "AGENT_ROLE" not in refused.stdout, (role, refused.stdout)


# No orchestrated role holds a browser grant, so an unbound sidecar never needs the browser server.
def test_sidecar_refuses_unbound_with_a_browser_target() -> None:
    completed = _run(
        ["bash", str(SIDECAR), "--print-mounts", "orchestrator", "--unbound"],
        BETA_BASE_URL="http://example.invalid/",
    )
    assert completed.returncode == 2, (completed.stdout, completed.stderr)
    assert not completed.stdout.strip(), completed.stdout
    assert "BETA_BASE_URL" in completed.stderr, completed.stderr


def test_sidecar_help_names_the_unbound_exposure() -> None:
    completed = _run(["bash", str(SIDECAR), "--help"])
    assert completed.returncode == 0, completed.stderr
    assert "any caller on the network" in completed.stdout
    assert "run_fix, which writes this tree" in completed.stdout
    assert "mcp/gate/server.py:13" in completed.stdout
    assert "granted role" in completed.stdout
    assert '"bound": false' in completed.stdout


def test_sidecar_print_mounts_makes_no_docker_call(tmp_path: Path) -> None:
    path_env, marker = _fake_docker(tmp_path)
    completed = _run(["bash", str(SIDECAR), "--print-mounts", "tester"], **path_env)
    assert completed.returncode == 0, completed.stderr
    assert not marker.exists()


# --- the cross-check --------------------------------------------------------------------------
# Drives write_entry and update_entry exactly as a bound sidecar would serve them. No row and no
# chain record is written any other way.
WRITER = """
import asyncio, importlib.util, json, sys
spec = importlib.util.spec_from_file_location("protection_storage", sys.argv[1])
storage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(storage)

async def main():
    first = await storage.write_entry(
        "proj-komun", "note", "first", "one", "internal", calling_role="implementer")
    await storage.update_entry("proj-komun", first["entry_id"], "two", calling_role="implementer")
    await storage.update_entry("proj-komun", first["entry_id"], "three", calling_role="implementer")
    second = await storage.write_entry(
        "proj-komun", "note", "second", "one", "internal", calling_role="implementer")
    print(json.dumps({"first": first["entry_id"], "second": second["entry_id"]}))

asyncio.run(main())
"""


@pytest.fixture(scope="module")
def served_store(tmp_path_factory: pytest.TempPathFactory) -> dict[str, object]:
    root = tmp_path_factory.mktemp("store-protection")
    db, journal = root / "storage.db", root / "storage-audit.log"
    completed = subprocess.run(
        [sys.executable, "-c", WRITER, str(STORAGE_SERVER)],
        env=_env(
            MEMORY_DIR=str(root),
            STORAGE_DB_PATH=str(db),
            STORAGE_AUDIT_PATH=str(journal),
            AGENT_ROLE="implementer",
        ),
        capture_output=True,
        text=True,
        check=False,
        timeout=300,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    ids = json.loads(completed.stdout.strip().splitlines()[-1])
    return {"root": root, "db": db, "journal": journal, **ids}


def _crosscheck(db: Path, journal: Path) -> tuple[int, dict | None, str]:
    completed = _run([sys.executable, str(CROSSCHECK), "--db", str(db), "--journal", str(journal)])
    report = json.loads(completed.stdout) if completed.stdout.strip() else None
    return completed.returncode, report, completed.stderr


def _journal_copy(served: dict[str, object], tmp_path: Path) -> tuple[Path, list[str]]:
    lines = [
        line
        for line in Path(served["journal"]).read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    return tmp_path / "storage-audit.log", lines


def test_crosscheck_exits_0_when_every_write_pairs(served_store: dict[str, object]) -> None:
    code, report, stderr = _crosscheck(served_store["db"], served_store["journal"])
    assert code == 0, stderr
    assert report["unjournalled chain record"] == []
    assert report["journal write with no chain record"] == []
    assert report["n - m"] == 0


def test_crosscheck_exits_1_on_a_missing_journal_line(
    served_store: dict[str, object], tmp_path: Path
) -> None:
    copy, lines = _journal_copy(served_store, tmp_path)
    assert served_store["second"] in lines[-1]
    copy.write_text("\n".join(lines[:-1]) + "\n", encoding="utf-8")
    code, report, _ = _crosscheck(served_store["db"], copy)
    assert code == 1
    assert [(r["operation"], r["entry_id"]) for r in report["unjournalled chain record"]] == [
        ("write_entry", served_store["second"])
    ]
    assert report["journal write with no chain record"] == []


def test_crosscheck_pairs_repeats_by_count_not_position(
    served_store: dict[str, object], tmp_path: Path
) -> None:
    copy, lines = _journal_copy(served_store, tmp_path)
    updates = [i for i, line in enumerate(lines) if '"update_entry"' in line]
    assert len(updates) == 2
    del lines[updates[-1]]
    copy.write_text("\n".join(lines) + "\n", encoding="utf-8")
    code, report, _ = _crosscheck(served_store["db"], copy)
    assert code == 1
    assert [(r["operation"], r["entry_id"]) for r in report["unjournalled chain record"]] == [
        ("update_entry", served_store["first"])
    ]


def test_crosscheck_exits_1_on_a_journal_write_with_no_chain_record(
    served_store: dict[str, object], tmp_path: Path
) -> None:
    copy, lines = _journal_copy(served_store, tmp_path)
    copy.write_text("\n".join(lines + [lines[-1]]) + "\n", encoding="utf-8")
    code, report, _ = _crosscheck(served_store["db"], copy)
    assert code == 1
    assert report["unjournalled chain record"] == []
    assert [(r["operation"], r["entry_id"]) for r in report["journal write with no chain record"]] == [
        ("write_entry", served_store["second"])
    ]


def test_crosscheck_exits_2_on_a_missing_store(
    served_store: dict[str, object], tmp_path: Path
) -> None:
    code, report, stderr = _crosscheck(tmp_path / "absent.db", served_store["journal"])
    assert code == 2 and report is None, stderr


def test_crosscheck_exits_2_on_a_malformed_journal_line(
    served_store: dict[str, object], tmp_path: Path
) -> None:
    copy, lines = _journal_copy(served_store, tmp_path)
    copy.write_text("\n".join(lines + ["not json"]) + "\n", encoding="utf-8")
    code, report, stderr = _crosscheck(served_store["db"], copy)
    assert code == 2 and report is None, stderr
    assert f"line {len(lines) + 1}" in stderr


def test_crosscheck_reads_a_copy_without_changing_it(
    served_store: dict[str, object], tmp_path: Path
) -> None:
    db_copy = tmp_path / "storage.db"
    shutil.copyfile(served_store["db"], db_copy)
    before = db_copy.read_bytes()
    code, _, stderr = _crosscheck(db_copy, served_store["journal"])
    assert code == 0, stderr
    assert db_copy.read_bytes() == before
