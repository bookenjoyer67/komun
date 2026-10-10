"""Unit tests for scripts/validate_doc_conformance_deterministic.py.

Lesson 4.3, "Implement the Deterministic Replacement", block [74] (VERBATIM):

    Create eval/test_deterministic_step.py. The tests must include:
    - At least three inputs drawn from the holdout set you used to assess stability
    - At least one edge case the agent handled that the script must also handle
    - At least one input that exercises the limitation you just described in the step above

The holdout set is the step's own recorded work: the three calibration cycles whose artifacts the
converted step was measured on -- the documentation-standard run (`docs/iteration-log.md:640`
`Run 001 (workflow 4`), the three citation re-execution cycles (`docs/iteration-log.md:784` `Run 003
(workflow 3`), and the log whose own pointers were repaired by hand (`docs/iteration-log.md:359`
`Every pointer that drifted was repaired`). The tests run the real files, so a rule that stops
finding real violations is visible as a failure.

Run with:  pytest eval/test_deterministic_step.py -v
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[1]
SCRIPT = REPO / "scripts" / "validate_doc_conformance_deterministic.py"
SENTENCE_SPLIT_RE = re.compile(r"(?<=[.!?])\s+")


def run_checker(tmp_path: Path, inputs: list[Path], rules: str | None = None,
                output: Path | None = None) -> tuple[int, dict, Path]:
    """Run the deterministic checker and return (exit status, report, report path)."""
    report_path = output or (tmp_path / "report.json")
    command = [sys.executable, str(SCRIPT)]
    for item in inputs:
        command += ["--input", str(item)]
    command += ["--output", str(report_path), "--repo", str(REPO)]
    if rules:
        command += ["--rules", rules]
    completed = subprocess.run(command, capture_output=True, text=True, cwd=REPO, check=False)
    report = json.loads(report_path.read_text(encoding="utf-8")) if report_path.exists() else {}
    return completed.returncode, report, report_path


def write_fixture(tmp_path: Path, name: str, body: str) -> Path:
    path = tmp_path / name
    path.write_text(textwrap.dedent(body).lstrip("\n"), encoding="utf-8")
    return path


def codes(report: dict) -> list[str]:
    return [finding["code"] for item in report["inputs"] for finding in item["violations"]]


# --- Holdout inputs: the real files the three recorded calibration cycles worked on --------------


def test_holdout_doc_style_rule_set_reports_its_own_opener(tmp_path: Path) -> None:
    """input 1 -- docs/DOC-STYLE.md, the rule set the agent applied.

    The standard is itself a v1 document, so its H1 answers no question. The test re-reads the file
    and asserts the finding's literal is the line the file actually holds, so the finding cannot
    drift away from the artifact.
    """
    status, report, _ = run_checker(tmp_path, [REPO / "docs" / "DOC-STYLE.md"])
    assert status == 1, "the standard's own H1 does not open with a question, so the run must fail"
    openers = [f for f in report["inputs"][0]["violations"] if f["code"] == "R1-NOT-QUESTION"]
    assert openers, "expected an R1-NOT-QUESTION finding on docs/DOC-STYLE.md"
    source = (REPO / "docs" / "DOC-STYLE.md").read_text(encoding="utf-8").splitlines()
    first = openers[0]
    assert first["literal"] == source[first["line"] - 1].strip(), "the literal must be the real line"
    assert not first["literal"].endswith("?"), "the rule fires when the opener is not a question"
    assert first["line"] == 3, "the H1 at line 1 is followed by prose at line 3"


def test_holdout_agents_md_isolates_one_rule(tmp_path: Path) -> None:
    """input 2 -- AGENTS.md, the prose the documentation-standard run edited.

    The rule filter has to hold: with R3 alone enabled no R1, R2 or R4 finding may appear.
    """
    status, report, _ = run_checker(tmp_path, [REPO / "AGENTS.md"], rules="R3")
    assert status in (0, 1)
    assert report["inputs"][0]["rules_enabled"] == ["R3"]
    rule_codes = {f["code"] for f in report["inputs"][0]["violations"] if f["rule"] != "CIT"}
    assert rule_codes <= {"R3-LONG-SENTENCE"}, "only the enabled rule may report"


def test_holdout_routing_map_long_sentences_are_recounted(tmp_path: Path) -> None:
    """input 3 -- docs/routing-and-tool-grant-map.md, a prose file the standard governs.

    Every R3 finding is re-counted by the test from the file, so a wrong word count fails here.
    """
    status, report, _ = run_checker(tmp_path, [REPO / "docs" / "routing-and-tool-grant-map.md"])
    assert status == 1
    findings = [f for f in report["inputs"][0]["violations"] if f["code"] == "R3-LONG-SENTENCE"]
    assert findings, "this file holds sentences over 35 words"
    source = (REPO / "docs" / "routing-and-tool-grant-map.md").read_text(encoding="utf-8").splitlines()
    for finding in findings:
        line = source[finding["line"] - 1]
        recounted = [s.strip() for s in SENTENCE_SPLIT_RE.split(line.replace("|", " "))
                     if s.strip() == finding["literal"]]
        assert recounted, "the reported sentence must be the text on the reported line"
        assert len(finding["literal"].split()) == finding["words"] > 35


def test_holdout_iteration_log_reports_its_own_drift(tmp_path: Path) -> None:
    """input 3 -- docs/iteration-log.md, the file whose pointers were repaired by hand five times.

    Each reported drift is re-verified: the literal must really sit on the line the finding names.
    """
    status, report, _ = run_checker(tmp_path, [REPO / "docs" / "iteration-log.md"])
    assert status == 1, "the log is not a documentation-standard document, so violations exist"
    item = report["inputs"][0]
    assert item["citations_checked"] > 0, "the log cites path:line pairs, so some must be checked"
    assert item["citations_resolved"] > 0, "most of the log's citations resolve"
    for finding in item["violations"]:
        if finding["code"] != "CIT-LINE-DRIFT":
            continue
        target = REPO / finding["citation"].rsplit(":", 1)[0]
        source = target.read_text(encoding="utf-8").splitlines()
        assert finding["literal"] in " ".join(source[finding["found_at"] - 1:finding["found_at"] + 1])


# --- Edge cases the agent handled, which the script must handle too ------------------------------


def test_edge_case_unverified_marker_exempts_the_claim(tmp_path: Path) -> None:
    """The standard's untraceable-claim path: a marked claim keeps its place and gets no authority."""
    marked = write_fixture(tmp_path, "marked.md", """
        # Marked document

        What does a marked claim keep?

        - Keep the claim and mark it (see docs/AGENTS.md).
        - Keep a second claim and mark it `[UNVERIFIED]` (see docs/AGENTS.md).
        """)
    _, report, _ = run_checker(tmp_path, [marked], rules="R2")
    findings = report["inputs"][0]["violations"]
    assert len(findings) == 1, "only the unmarked bare location may be reported"
    assert findings[0]["line"] == 5, "the marked line at line 6 is exempt"
    assert findings[0]["literal"] == "(see docs/AGENTS.md)"


def test_edge_case_wrapped_literal_resolves(tmp_path: Path) -> None:
    """Two of this repository's citations wrap their literal over two lines."""
    fixture = write_fixture(tmp_path, "wrapped.md", """
        # Wrapped document

        Which citation wraps its literal?

        - Cite the standard's unwritten claim (`docs/DOC-STYLE.md:72` `Do not delete the claim, and do
          not invent authority for it.`).
        """)
    status, report, _ = run_checker(tmp_path, [fixture])
    item = report["inputs"][0]
    assert status == 0, "the document is clean, so the wrapped literal must resolve"
    assert item["violations"] == []
    assert item["citations_checked"] == 1
    assert item["citations_resolved"] == 1, "the wrapped literal resolves at docs/DOC-STYLE.md:72"
    assert "CIT-LITERAL-MISSING" not in codes(report)


def test_edge_case_off_by_one_citation_is_reported(tmp_path: Path) -> None:
    """The agent's recorded M6 class: the quoted text is right and the pointer is one line off."""
    fixture = write_fixture(tmp_path, "drift.md", """
        # Drift document

        Which pointer lands one line away?

        - Cite the role list (`scripts/run-agent.sh:34` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher beta-tester"`).
        """)
    _, report, _ = run_checker(tmp_path, [fixture])
    drift = [f for f in report["inputs"][0]["violations"] if f["code"] == "CIT-LINE-DRIFT"]
    assert len(drift) == 1
    assert drift[0]["citation"] == "scripts/run-agent.sh:34"
    assert drift[0]["found_at"] == 33, "VALID_ROLES sits on line 33 of the launcher"


# --- The limitation the step classification names ------------------------------------------------


def test_limitation_r4_verb_initial_clause_is_not_checked(tmp_path: Path) -> None:
    """The standard defines no verb lexicon, so only its four named hedge words are checked."""
    fixture = write_fixture(tmp_path, "r4.md", """
        # R4 document

        Which part of R4 is not decidable?

        - Nested bullets are allowed where they show real hierarchy.
        - Should keep the other rule, because the standard names that hedge.
        """)
    _, report, _ = run_checker(tmp_path, [fixture], rules="R4")
    findings = report["inputs"][0]["violations"]
    assert len(findings) == 1, "the non-verb, non-hedging bullet is not flagged"
    assert findings[0]["hedge"] == "should"
    assert findings[0]["line"] == 6
    assert any("verb-initial" in item for item in report["limitations"])


def test_limitation_command_authority_is_counted_not_executed(tmp_path: Path) -> None:
    """A command authority needs the command run, so the script counts it and leaves it unverified."""
    fixture = write_fixture(tmp_path, "command.md", """
        # Command document

        Which authority needs a command run?

        - Cite the workspace gate as a command (`cargo test --workspace` -> `158 passed, 0 failed`).
        """)
    _, report, _ = run_checker(tmp_path, [fixture])
    item = report["inputs"][0]
    assert item["citations_checked"] == 0, "a command authority carries no path:line pair"
    assert item["command_authorities_unverified"] == 1
    assert "CIT-LITERAL-MISSING" not in codes(report)


def test_limitation_wrapped_literal_pairs_with_a_neighbour(tmp_path: Path) -> None:
    """The checker's known false positive, pinned on a fixture rather than argued.

    Pairing is structural: a pointer takes the literal inside its parentheses or beside it on the
    same line, so a pointer whose literal wraps to the next line takes a neighbouring literal and
    reports `CIT-LINE-DRIFT` on a correct citation. Two live examples are left unrepaired in
    `AGENTS.md` (`AGENTS.md:29` and `AGENTS.md:45`, both citing `web/package.json`); this test
    reproduces the shape and proves the misread citation really is correct by reading the target
    file, so the limitation cannot be argued away without failing here.
    """
    fixture = write_fixture(tmp_path, "wrapped-pointer.md", """
        # Pointer document

        Which pointer takes a neighbouring literal?

        - Cite two manifest entries (`web/package.json:13` `"@sveltejs/kit": "^2.0.0",`; `web/package.json:12`
          `"@sveltejs/adapter-static": "^3.0.0",`).
        """)
    _, report, _ = run_checker(tmp_path, [fixture])
    item = report["inputs"][0]
    drift = [f for f in item["violations"] if f["code"] == "CIT-LINE-DRIFT"]
    assert len(drift) == 1, "the wrapped literal makes the checker report exactly one drift"
    finding = drift[0]
    assert finding["citation"] == "web/package.json:12"
    assert finding["literal"] == '"@sveltejs/kit": "^2.0.0",', "the neighbour is the paired literal"
    assert finding["found_at"] == 13, "the neighbouring entrant sits one line below the cited one"
    manifest = (REPO / "web" / "package.json").read_text(encoding="utf-8").splitlines()
    assert manifest[11].strip() == '"@sveltejs/adapter-static": "^3.0.0",', "the citation is correct"
    assert manifest[12].strip() == '"@sveltejs/kit": "^2.0.0",', "the paired literal is line 13"
    assert item["citations_checked"] == 2
    assert item["citations_resolved"] == 1, "only the unwrapped pointer resolves"


# --- The checker can pass, the checker can fail, and two runs over one input agree ----------------


def test_clean_input_passes_and_corrupt_input_fails(tmp_path: Path) -> None:
    """A checker that cannot fail is evidence of nothing, so both directions are pinned here."""
    clean = write_fixture(tmp_path, "clean.md", """
        # Probe document

        What does this probe document prove?

        - Cite the launcher's role list (`scripts/run-agent.sh:33` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher beta-tester"`).
        - Cite the role table (`scripts/README.md:31` `Policy grants workspace writes and no memory write`).
        """)
    status, report, _ = run_checker(tmp_path, [clean], output=tmp_path / "clean.json")
    assert status == 0, "the clean document holds no violation"
    assert report["verdict"] == "PASS"
    assert report["inputs"][0]["citations_resolved"] == 2

    corrupt = write_fixture(tmp_path, "corrupt.md", """
        # Probe document

        This section describes the probe document.

        - Cite a bare location with no literal (docs/DOC-STYLE.md:40).
        - Should keep the memory layer plain files, because that is the decision of record.
        - Cite a drifted line (`scripts/run-agent.sh:34` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher beta-tester"`).
        - Cite a literal that is absent (`docs/DOC-STYLE.md:21` `this literal appears in no file at all`).
        - Cite a missing file (`docs/NO-SUCH-FILE.md:1` `absent`).
        """)
    status, report, _ = run_checker(tmp_path, [corrupt], output=tmp_path / "corrupt.json")
    assert status == 1
    assert report["verdict"] == "FAIL"
    assert set(codes(report)) == {
        "R1-THIS-SECTION", "R2-BARE-LOCATION", "R4-HEDGE",
        "CIT-LINE-DRIFT", "CIT-LITERAL-MISSING", "CIT-FILE-MISSING",
    }


def test_two_runs_over_one_input_are_byte_identical(tmp_path: Path) -> None:
    """Determinism is the conversion's whole point: no timestamp, no ordering by chance."""
    first = tmp_path / "first.json"
    second = tmp_path / "second.json"
    inputs = [REPO / "docs" / "DOC-STYLE.md", REPO / "scripts" / "README.md"]
    run_checker(tmp_path, inputs, output=first)
    run_checker(tmp_path, inputs, output=second)
    assert first.read_bytes() == second.read_bytes()


# --- The named-file interface the lesson requires ------------------------------------------------


def test_interface_requires_named_input_and_output(tmp_path: Path) -> None:
    """The lesson fixes the interface: a named input file argument and a named output file."""
    no_output = subprocess.run(
        [sys.executable, str(SCRIPT), "--input", str(REPO / "AGENTS.md")],
        capture_output=True, text=True, check=False)
    assert no_output.returncode == 2
    assert "--output" in no_output.stderr

    no_input = subprocess.run(
        [sys.executable, str(SCRIPT), "--output", str(tmp_path / "x.json")],
        capture_output=True, text=True, check=False)
    assert no_input.returncode == 2
    assert "--input" in no_input.stderr

    missing = subprocess.run(
        [sys.executable, str(SCRIPT), "--input", "docs/NO-SUCH-FILE.md",
         "--output", str(tmp_path / "x.json")],
        capture_output=True, text=True, check=False)
    assert missing.returncode == 2
    assert "no such input file" in missing.stderr


def test_interface_rejects_an_unknown_rule(tmp_path: Path) -> None:
    """A rule name the standard does not define is refused rather than ignored."""
    completed = subprocess.run(
        [sys.executable, str(SCRIPT), "--input", str(REPO / "AGENTS.md"),
         "--output", str(tmp_path / "x.json"), "--rules", "R9"],
        capture_output=True, text=True, check=False)
    assert completed.returncode == 2
    assert "unknown rule" in completed.stderr


def test_report_names_its_rule_source(tmp_path: Path) -> None:
    """The report states the authority for its rules, so a reader can check the rule set."""
    _, report, _ = run_checker(tmp_path, [REPO / "docs" / "DOC-STYLE.md"], rules="R1")
    assert report["rule_source"] == "docs/DOC-STYLE.md v2 (Rules (v2) table, and the Authority forms (v2) section)"
    assert report["max_sentence_words"] == 35
    assert report["hedge_words"] == ["should", "might", "probably", "may want to"]
    assert report["inputs"][0]["rules_enabled"] == ["R1"]


# --- The gate vocabulary's guard-pattern fallback ------------------------------------------------
# These three tests cover mcp/gate/gate_vocabulary.py, not the conformance checker above. Run
# run-2026-10-03-gate-guard-per-command added them to pin three things. An unusable cache-hit guard
# pattern never stops the vocabulary, or the server entry point, at import. It falls back to the
# built-in default when that default is usable. Otherwise only that command's guard fails, with a
# message naming the config key. Each case runs in a child process against a temporary config
# chosen through AGENTIC_CONFIG, so the loader's module-level cache cannot leak between cases.

GATE_DIR = REPO / "mcp" / "gate"
GATE_SERVER = GATE_DIR / "server.py"
GUARD_SAMPLE = "   Compiling komun-core v0.1.0 (/workspace/crates/core)"
GUARD_PROBE = textwrap.dedent("""\
    import json
    import sys

    sys.path.insert(0, sys.argv[1])
    import gate_vocabulary as vocabulary

    searches = {}
    for name, definition in vocabulary.COMMANDS.items():
        if definition["guard"]:
            for label, text in (("sample", sys.argv[2]), ("empty", "")):
                match, message = vocabulary.guard_marker_search(name, text)
                searches[name + ":" + label] = [match is not None, message]
    defaults = vocabulary.agentic_config.DEFAULT["toolchain"]["commands"]
    print(json.dumps({
        "patterns": {name: p.pattern for name, p in vocabulary.GUARD_MARKER_PATTERNS.items()},
        "fallbacks": vocabulary.GUARD_PATTERN_FALLBACKS,
        "errors": vocabulary.GUARD_PATTERN_ERRORS,
        "searches": searches,
        "default_test": defaults["test"]["guard"]["marker_regex"],
    }))
    """)


def guarded_test_command(**guard_fields: object) -> dict:
    """The `test` command as a config entry, with its guard's fields replaced or added."""
    guard = {"marker": "Compiling komun-core", "touch_file": "crates/core/src/tests.rs", "reason": "r"}
    guard.update(guard_fields)
    return {"argv": ["cargo", "test", "--workspace"], "guard": guard, "summary": None, "writes": False}


def probe_guards(tmp_path: Path, commands: dict) -> dict:
    """Load the vocabulary and the server entry point against a temporary config; return the probe.

    Both children must exit 0, which is the proof that neither one raises at import, whatever the
    configured guard patterns are.
    """
    import os  # imported here so the top-level imports, and every line cited into this file, stay put

    config = tmp_path / "agentic.config.json"
    config.write_text(json.dumps({"schema_version": 1, "toolchain": {"commands": commands}}),
                      encoding="utf-8")
    env = {**os.environ, "AGENTIC_CONFIG": str(config)}
    probe = subprocess.run([sys.executable, "-c", GUARD_PROBE, str(GATE_DIR), GUARD_SAMPLE],
                           capture_output=True, text=True, cwd=REPO, env=env, check=False)
    assert probe.returncode == 0, f"gate_vocabulary must load, not raise: {probe.stderr}"
    server = subprocess.run([sys.executable, str(GATE_SERVER), "--print-config"],
                            capture_output=True, text=True, cwd=REPO, env=env, check=False)
    assert server.returncode == 0, f"the server entry point must get past the import: {server.stderr}"
    assert "toolchain.commands.test.guard.marker_regex" in json.loads(server.stdout)
    return json.loads(probe.stdout)


def assert_test_guard_falls_back(report: dict) -> None:
    """The `test` guard runs on the built-in default, with the fallback recorded and no error."""
    assert report["patterns"]["test"] == report["default_test"], "the built-in default answers"
    assert "toolchain.commands.test.guard.marker_regex" in report["fallbacks"]["test"]
    assert "test" not in report["errors"]
    assert report["searches"]["test:sample"] == [True, None], "the default matches the marker line"
    assert report["searches"]["test:empty"] == [False, None], "nothing matches empty output"


def test_guard_pattern_empty_blank_or_not_a_string_falls_back_to_the_default(tmp_path: Path) -> None:
    """Cases C4 and C5: an empty, blank or non-string marker_regex is replaced by the default."""
    for marker_regex in ("", "   ", 5):
        report = probe_guards(tmp_path, {"test": guarded_test_command(marker_regex=marker_regex)})
        assert_test_guard_falls_back(report)


def test_guard_pattern_uncompilable_or_matching_empty_falls_back_to_the_default(tmp_path: Path) -> None:
    """Cases C6 and C7: a pattern that will not compile, or that matches empty text, is replaced."""
    for marker_regex in ("(", ".*"):
        report = probe_guards(tmp_path, {"test": guarded_test_command(marker_regex=marker_regex)})
        assert_test_guard_falls_back(report)


def test_guard_with_no_usable_default_fails_only_its_own_gate(tmp_path: Path) -> None:
    """Case C8 on a fork-added command: its guard fails naming the key, and `test` is untouched.

    `test` leaves its marker_regex out, so the loader hands back the default (case C2). `lint` is
    absent from the built-in defaults and carries a pattern that will not compile.
    """
    commands = {
        "test": guarded_test_command(),
        "lint": {
            "argv": ["true"],
            "guard": {"marker": "Linting x", "marker_regex": "(", "touch_file": "x", "reason": "r"},
            "summary": None,
            "writes": False,
        },
    }
    report = probe_guards(tmp_path, commands)
    key = "toolchain.commands.lint.guard.marker_regex"
    assert "lint" not in report["patterns"], "no pattern, so nothing can match for this gate"
    assert key in report["errors"]["lint"]
    matched, message = report["searches"]["lint:sample"]
    assert matched is False, "the guard of a command with no usable pattern is never satisfied"
    assert key in message, "the detail names the config key to fix"
    assert report["patterns"]["test"] == report["default_test"]
    assert "test" not in report["fallbacks"] and "test" not in report["errors"]
    assert report["searches"]["test:sample"] == [True, None], "only the lint gate's guard fails"


# --- The store and journal hash chain: mcp/hashchain.py and scripts/chain_anchor.py --------------
# Run run-2026-10-04-b3b5 added these tests. Every artifact is a temporary file under tmp_path and
# nothing reads or writes .memory/. A test that changes a fixture asserts that verify FAILS and
# names the record; a missing server dependency fails the test rather than skipping it.

CHAIN_OPERATOR = "chain-test-operator"
CHAIN_PROJECT = "proj-chain"
CHAIN_ANCHOR = REPO / "scripts" / "chain_anchor.py"
CHAIN_ARTIFACTS = {"store", "storage-journal", "retrieval-journal", "gate-journal", "browser-journal"}
STORAGE_JOURNAL_KEYS = {
    "timestamp", "operation", "project_id", "entry_id", "classification", "calling_role",
    "allowed", "reason",
}


def chain_module():
    """Import mcp/hashchain.py under the name the servers use, so a patch here reaches them."""
    mcp_dir = str(REPO / "mcp")
    if mcp_dir not in sys.path:
        sys.path.insert(0, mcp_dir)
    import hashchain  # noqa: PLC0415 - kept local so every line cited into this file stays put

    return hashchain


def load_chain_server(name: str, relative: str):
    import importlib.util  # noqa: PLC0415

    spec = importlib.util.spec_from_file_location(name, REPO / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def chain_storage(tmp_path: Path, monkeypatch, *, operator: bool = True):
    """The storage server on a temporary store and journal, with AGENT_ROLE unset."""
    monkeypatch.delenv("AGENT_ROLE", raising=False)
    storage = load_chain_server("chain_storage_server", "mcp/storage/server.py")
    monkeypatch.setattr(storage, "DB_PATH", str(tmp_path / "storage.db"))
    monkeypatch.setattr(storage, "AUDIT_PATH", str(tmp_path / "storage-audit.log"))
    if operator:
        grants = {**storage.ALLOW_LIST, CHAIN_OPERATOR: list(storage.OPERATIONS)}
        monkeypatch.setattr(storage, "ALLOW_LIST", grants)
    return storage


def chain_gate(tmp_path: Path, monkeypatch):
    gate = load_chain_server("chain_gate_server", "mcp/gate/server.py")
    monkeypatch.setattr(gate, "AUDIT_PATH", str(tmp_path / "gate-audit.log"))
    return gate


def run_storage(coroutine):
    import asyncio  # noqa: PLC0415

    return asyncio.run(coroutine)


def write_chain_entry(storage, title: str, calling_role: str = CHAIN_OPERATOR) -> dict:
    return run_storage(storage.write_entry(CHAIN_PROJECT, "decision", title, f"content of {title}",
                                           "internal", calling_role=calling_role))


def chained_journal(path: Path, count: int) -> dict:
    chain = chain_module()
    for index in range(count):
        chain.append_journal_record(path, {"operation": "write_entry", "allowed": True,
                                           "index": index, "duration_seconds": 0.25 * index})
    return chain.journal_head(path)


def journal_lines(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]


def run_anchor(memory_dir: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, str(CHAIN_ANCHOR), "--memory-dir", str(memory_dir), *args],
                          capture_output=True, text=True, cwd=REPO, check=False)


def test_hashchain_canonical_is_stable_and_explicit() -> None:
    """Keys sort, separators carry no space, nulls stay, and key order never changes the bytes."""
    chain = chain_module()
    assert chain.canonical({"b": 1, "a": None, "c": [True, "x"]}) == b'{"a":null,"b":1,"c":[true,"x"]}'
    assert chain.canonical({"a": 1, "b": 2}) == chain.canonical({"b": 2, "a": 1})
    assert chain.canonical(1) == b"1" and chain.canonical(1.0) == b"1.0"
    with pytest.raises(TypeError):
        chain.canonical({"when": object()})
    with pytest.raises(TypeError):
        chain.canonical({1: "a non-string key"})


def test_hashchain_canonical_floats_round_trip_fold_negative_zero_and_refuse_non_finite() -> None:
    """Amendment A3: repr round-trip, -0.0 as 0.0, and NaN or an infinity raises."""
    import math  # noqa: PLC0415

    chain = chain_module()
    for value in (0.1, 1 / 3, 2.533, 1e-7, 1e16, 2.5e300, -123456.789):
        text = chain.canonical(value).decode("ascii")
        assert text == repr(value), "a float is its shortest round-tripping decimal"
        assert float(text) == value
    assert chain.canonical(-0.0) == chain.canonical(0.0) == b"0.0"
    assert chain.canonical({"d": -0.0}) == b'{"d":0.0}'
    for value in (math.nan, math.inf, -math.inf):
        with pytest.raises(ValueError):
            chain.canonical({"duration_seconds": value})


def test_hashchain_gate_duration_float_is_journalled_unchanged(tmp_path: Path, monkeypatch) -> None:
    """The gate's float duration_seconds stays the same JSON number, and the chain covers it."""
    chain = chain_module()
    gate = chain_gate(tmp_path, monkeypatch)
    gate.audit_invocation(tool="run_gate", gate="clippy", argv=["cargo", "clippy"], exit_code=0,
                          duration_seconds=2.533, passed=True, timed_out=False, guard_applied=True,
                          guard_satisfied=True, writes=False, calling_role="tester", summary=None)
    journal = tmp_path / "gate-audit.log"
    assert '"duration_seconds": 2.533' in journal.read_text(encoding="utf-8")
    assert journal_lines(journal)[0]["duration_seconds"] == 2.533
    assert chain.verify_journal(journal)["status"] == "INTACT"


def test_hashchain_store_clean_append_head_verify_intact(tmp_path: Path, monkeypatch) -> None:
    """Every write, update and delete extends the store chain; a read or a list never does."""
    chain = chain_module()
    storage = chain_storage(tmp_path, monkeypatch)
    db = tmp_path / "storage.db"
    first = write_chain_entry(storage, "first")
    second = write_chain_entry(storage, "second")
    before = chain.store_head(db)
    run_storage(storage.read_entry(CHAIN_PROJECT, first["entry_id"], calling_role=CHAIN_OPERATOR))
    run_storage(storage.list_entries(CHAIN_PROJECT, calling_role=CHAIN_OPERATOR))
    assert chain.store_head(db) == before, "reads never extend the chain"
    assert before["seq"] == 2

    run_storage(storage.update_entry(CHAIN_PROJECT, first["entry_id"], "revised", title="first b",
                                     calling_role=CHAIN_OPERATOR))
    run_storage(storage.delete_entry(CHAIN_PROJECT, second["entry_id"], calling_role=CHAIN_OPERATOR))
    head = chain.store_head(db)
    assert head["seq"] == 4
    result = chain.verify_store(db, head["seq"], head["head"])
    assert result["status"] == "INTACT", result
    assert (result["records"], result["rows"]) == (4, 2)
    assert chain.verify_store(db, before["seq"], before["head"])["status"] == "INTACT"
    assert chain.verify_journal(tmp_path / "storage-audit.log")["status"] == "INTACT"


def test_hashchain_store_write_without_its_chain_record_does_not_commit(tmp_path: Path,
                                                                        monkeypatch) -> None:
    """A store write whose chain record cannot be written rolls back, so the row never lands."""
    import sqlite3  # noqa: PLC0415

    chain = chain_module()
    storage = chain_storage(tmp_path, monkeypatch)
    db = tmp_path / "storage.db"
    write_chain_entry(storage, "kept")
    head = chain.store_head(db)
    journal_before = (tmp_path / "storage-audit.log").read_bytes()

    def refuse(prev_head, record):
        raise chain.ChainError("the store chain is made unable to extend for this test")

    original = chain.store_link
    monkeypatch.setattr(chain, "store_link", refuse)
    with pytest.raises(chain.ChainError):
        write_chain_entry(storage, "never lands")
    monkeypatch.setattr(chain, "store_link", original)

    conn = sqlite3.connect(db)
    try:
        assert conn.execute("SELECT COUNT(*) FROM entries").fetchone()[0] == 1
    finally:
        conn.close()
    assert chain.store_head(db) == head
    assert chain.verify_store(db, head["seq"], head["head"])["status"] == "INTACT"
    assert (tmp_path / "storage-audit.log").read_bytes() == journal_before


def test_hashchain_store_row_changed_outside_server_fails_verify_and_names_entry(
        tmp_path: Path, monkeypatch) -> None:
    """A row edited directly in the temporary store fails verify, naming its entry_id and seq."""
    import sqlite3  # noqa: PLC0415

    chain = chain_module()
    storage = chain_storage(tmp_path, monkeypatch)
    db = tmp_path / "storage.db"
    first = write_chain_entry(storage, "first")
    write_chain_entry(storage, "second")
    head = chain.store_head(db)

    conn = sqlite3.connect(db)
    try:
        conn.execute("UPDATE entries SET content = ? WHERE entry_id = ?",
                     ("changed outside the server", first["entry_id"]))
        conn.commit()
    finally:
        conn.close()

    result = chain.verify_store(db, head["seq"], head["head"])
    assert result["status"] == "FAIL"
    assert (result["seq"], result["entry_id"]) == (1, first["entry_id"])
    completed = run_anchor(tmp_path, "verify", "--artifact", "store",
                           "--expected", f"{head['seq']}:{head['head']}")
    assert completed.returncode == 1, "a changed row is a hard failure, never a warning"
    assert first["entry_id"] in completed.stderr


def test_hashchain_journal_line_changed_fails_verify_and_names_seq(tmp_path: Path) -> None:
    """A journal line edited in the temporary fixture fails verify at that line."""
    chain = chain_module()
    journal = tmp_path / "storage-audit.log"
    head = chained_journal(journal, 4)
    lines = journal.read_text(encoding="utf-8").splitlines()
    record = json.loads(lines[1])
    record["allowed"] = False
    lines[1] = json.dumps(record, sort_keys=True)
    journal.write_text("\n".join(lines) + "\n", encoding="utf-8")

    result = chain.verify_journal(journal, head["seq"], head["head"])
    assert result["status"] == "FAIL"
    assert (result["seq"], result["line"]) == (2, 2)


def test_hashchain_journal_line_removed_fails_verify_and_names_seq(tmp_path: Path) -> None:
    """A journal line removed from the temporary fixture fails verify where the gap opens."""
    chain = chain_module()
    journal = tmp_path / "retrieval-audit.log"
    head = chained_journal(journal, 4)
    lines = journal.read_text(encoding="utf-8").splitlines()
    del lines[1]
    journal.write_text("\n".join(lines) + "\n", encoding="utf-8")

    result = chain.verify_journal(journal, head["seq"], head["head"])
    assert result["status"] == "FAIL"
    assert (result["seq"], result["line"]) == (2, 2)


def test_hashchain_journal_tail_removed_fails_against_the_anchor(tmp_path: Path) -> None:
    """A journal cut back past the anchored record fails, which only the anchor can show."""
    chain = chain_module()
    journal = tmp_path / "gate-audit.log"
    head = chained_journal(journal, 3)
    lines = journal.read_text(encoding="utf-8").splitlines()
    journal.write_text("\n".join(lines[:-1]) + "\n", encoding="utf-8")

    result = chain.verify_journal(journal, head["seq"], head["head"])
    assert result["status"] == "FAIL"
    assert result["seq"] == 3
    assert "anchored seq is missing" in result["reason"]


def test_hashchain_preexisting_store_seeds_once_and_reports_it(tmp_path: Path, monkeypatch) -> None:
    """The first write on an unchained store seeds the chain over its rows and says so, once."""
    import sqlite3  # noqa: PLC0415

    chain = chain_module()
    storage = chain_storage(tmp_path, monkeypatch)
    db = tmp_path / "storage.db"
    conn = sqlite3.connect(db)
    try:
        for statement in storage.SCHEMA_STATEMENTS:
            if statement not in chain.STORE_SCHEMA_STATEMENTS:
                conn.execute(statement)
        for entry_id in ("pre-existing-a", "pre-existing-b"):
            conn.execute(
                "INSERT INTO entries (entry_id, project_id, entry_type, title, content, "
                "classification, deleted, last_updated) VALUES (?, ?, 'note', 't', 'c', "
                "'internal', 0, '2026-10-01T00:00:00+00:00')",
                (entry_id, CHAIN_PROJECT),
            )
        conn.commit()
    finally:
        conn.close()
    unchained = chain.verify_store(db)
    assert unchained["status"] == "FAIL", "rows the chain does not cover yet never pass"

    first = write_chain_entry(storage, "first after seeding")
    seeded = first["chain_seeded"]
    assert (seeded["seq"], seeded["rows"]) == (1, 2)
    second = write_chain_entry(storage, "second after seeding")
    assert set(second) == {"entry_id"}, "only the write that seeds reports it"
    head = chain.store_head(db)
    assert head["seq"] == 3
    assert chain.verify_store(db, 1, seeded["head"])["status"] == "INTACT"
    assert chain.verify_store(db, head["seq"], head["head"])["status"] == "INTACT"

    conn = sqlite3.connect(db)
    try:
        conn.execute("UPDATE entries SET title = 'changed' WHERE entry_id = 'pre-existing-b'")
        conn.commit()
    finally:
        conn.close()
    result = chain.verify_store(db, head["seq"], head["head"])
    assert (result["status"], result["seq"], result["entry_id"]) == ("FAIL", 1, "pre-existing-b")


def test_hashchain_preexisting_journal_seeds_without_adding_a_line(tmp_path: Path) -> None:
    """The first chained line covers the unchained lines before it and rewrites none of them."""
    chain = chain_module()
    journal = tmp_path / "gate-audit.log"
    prior = [json.dumps({"gate": "fmt", "exit_code": 1}, sort_keys=True),
             json.dumps({"gate": "test", "exit_code": 0}, sort_keys=True)]
    journal.write_text("\n".join(prior) + "\n", encoding="utf-8")

    block = chain.append_journal_record(journal, {"gate": "clippy", "exit_code": 0})
    lines = journal.read_text(encoding="utf-8").splitlines()
    assert len(lines) == 3, "seeding adds no line of its own"
    assert lines[:2] == prior
    assert (block["seq"], block["seeded"]["prior_lines"]) == (1, 2)
    result = chain.verify_journal(journal, 1, block["head"])
    assert (result["status"], result["prior_lines"]) == ("INTACT", 2)

    lines[0] = json.dumps({"gate": "fmt", "exit_code": 0}, sort_keys=True)
    journal.write_text("\n".join(lines) + "\n", encoding="utf-8")
    result = chain.verify_journal(journal, 1, block["head"])
    assert (result["status"], result["seq"], result["line"]) == ("FAIL", 1, 3)


def test_hashchain_journals_clean_append_head_verify_intact(tmp_path: Path, monkeypatch) -> None:
    """The storage, retrieval and gate sinks each extend their own journal chain."""
    chain = chain_module()
    storage = chain_storage(tmp_path, monkeypatch)
    gate = chain_gate(tmp_path, monkeypatch)
    retrieval = load_chain_server("chain_retrieval_server", "mcp/retrieval/server.py")
    monkeypatch.setattr(retrieval, "AUDIT_PATH", str(tmp_path / "retrieval-audit.log"))

    for index in range(3):
        storage.audit_event("read_entry", allowed=False, project_id=CHAIN_PROJECT,
                            calling_role="implementer", reason=f"probe {index}")
        retrieval.audit_call(operation="retrieve", calling_role="implementer", decision="allowed",
                             project_id=CHAIN_PROJECT, result_count=index)
        gate.audit_denial(tool="run_gate", gate="test", calling_role="implementer",
                          reason=f"probe {index}")

    for name in ("storage-audit.log", "retrieval-audit.log", "gate-audit.log"):
        journal = tmp_path / name
        head = chain.journal_head(journal)
        assert head["seq"] == 3, name
        result = chain.verify_journal(journal, head["seq"], head["head"])
        assert (result["status"], result["lines"]) == ("INTACT", 3), (name, result)


def test_hashchain_no_anchor_results_unchanged(tmp_path: Path, monkeypatch) -> None:
    """Without an anchor the tools return what they always returned; a line only gains "chain"."""
    storage = chain_storage(tmp_path, monkeypatch)
    written = write_chain_entry(storage, "plain")
    assert set(written) == {"entry_id"}
    entry_id = written["entry_id"]
    row = run_storage(storage.read_entry(CHAIN_PROJECT, entry_id, calling_role=CHAIN_OPERATOR))
    assert set(row) == {"entry_id", "project_id", "entry_type", "title", "content",
                        "classification", "deleted", "last_updated"}
    listed = run_storage(storage.list_entries(CHAIN_PROJECT, calling_role=CHAIN_OPERATOR))
    assert [set(item) for item in listed] == [
        {"entry_id", "title", "entry_type", "classification", "last_updated"}]
    assert run_storage(storage.update_entry(CHAIN_PROJECT, entry_id, "again",
                                            calling_role=CHAIN_OPERATOR)) == {"success": True}
    assert run_storage(storage.delete_entry(CHAIN_PROJECT, entry_id,
                                            calling_role=CHAIN_OPERATOR)) == {"success": True}
    for line in journal_lines(tmp_path / "storage-audit.log"):
        assert set(line) - {"chain"} == STORAGE_JOURNAL_KEYS


def test_hashchain_chain_failure_never_changes_an_authorisation_outcome(tmp_path: Path, monkeypatch,
                                                                        capsys) -> None:
    """Amendment A1: with the journal chain unable to extend, every outcome and reason is the same.

    The failure is loud twice over: stderr says the chain could not extend, and the journal line,
    still written in full, carries chain.error so every later verify fails at it.
    """
    chain = chain_module()
    storage = chain_storage(tmp_path, monkeypatch, operator=False)
    gate = chain_gate(tmp_path, monkeypatch)

    def attempt(call) -> tuple[str, str]:
        try:
            result = call()
        except Exception as error:  # noqa: BLE001 - the outcome is the assertion
            return type(error).__name__, str(error)
        return "allowed", json.dumps(sorted(result))

    def outcomes() -> list[tuple[str, str]]:
        seen = []
        monkeypatch.setenv("AGENT_ROLE", "project-manager")
        seen.append(attempt(lambda: gate.run_gate("not-a-gate", calling_role="tester")))
        seen.append(attempt(lambda: gate.run_fix("not-a-command")))
        monkeypatch.setenv("AGENT_ROLE", "implementer")
        seen.append(attempt(lambda: run_storage(storage.read_entry(
            CHAIN_PROJECT, "0f6b7b1e-0000-4000-8000-000000000000", calling_role="planner"))))
        monkeypatch.delenv("AGENT_ROLE", raising=False)
        seen.append(attempt(lambda: write_chain_entry(storage, "denied", "project-manager")))
        seen.append(attempt(lambda: write_chain_entry(storage, "allowed", "implementer")))
        seen.append(attempt(lambda: gate.run_gate("not-a-gate", calling_role="anon")))
        return seen

    intact = outcomes()
    assert [kind for kind, _ in intact[:4]] == ["AuthorizationDenied"] * 4
    assert intact[4] == ("allowed", '["entry_id"]')
    gate_lines = len(journal_lines(tmp_path / "gate-audit.log"))
    capsys.readouterr()

    def unable(handle, record):
        raise chain.ChainError("the journal chain is made unable to extend for this test")

    monkeypatch.setattr(chain, "_next_journal_block", unable)
    assert outcomes() == intact, "a chain failure changed an authorisation outcome or reason"
    assert "could not extend" in capsys.readouterr().err

    gate_journal = tmp_path / "gate-audit.log"
    failed = journal_lines(gate_journal)[gate_lines]
    assert failed["allowed"] is False and failed["reason"] == intact[0][1]
    assert failed["chain"]["head"] is None and "unable to extend" in failed["chain"]["error"]
    result = chain.verify_journal(gate_journal)
    assert (result["status"], result["line"]) == ("FAIL", gate_lines + 1)
    assert "could not extend" in result["reason"]


def test_hashchain_operator_head_and_verify_have_no_write_mode(tmp_path: Path, monkeypatch) -> None:
    """The operator command reads heads and verifies anchors, and leaves every artifact as it was."""
    chain = chain_module()
    storage = chain_storage(tmp_path, monkeypatch)
    write_chain_entry(storage, "anchored")
    chained_journal(tmp_path / "gate-audit.log", 2)
    artifacts = ("storage.db", "storage-audit.log", "gate-audit.log")
    before = {name: (tmp_path / name).read_bytes() for name in artifacts}

    usage = run_anchor(tmp_path, "--help")
    assert usage.returncode == 0 and "{head,verify}" in usage.stdout, "head and verify only"

    printed = run_anchor(tmp_path, "head")
    assert printed.returncode == 0, printed.stderr
    heads = json.loads(printed.stdout)
    assert set(heads) == CHAIN_ARTIFACTS
    assert heads["retrieval-journal"]["present"] is False
    assert heads["store"]["anchor"] == f"1:{chain.store_head(tmp_path / 'storage.db')['head']}"

    for name in ("store", "storage-journal", "gate-journal"):
        verified = run_anchor(tmp_path, "verify", "--artifact", name,
                              "--expected", heads[name]["anchor"])
        assert verified.returncode == 0, (name, verified.stdout, verified.stderr)
        assert json.loads(verified.stdout)["status"] == "INTACT"

    seq = heads["gate-journal"]["seq"]
    differing = run_anchor(tmp_path, "verify", "--artifact", "gate-journal",
                           "--expected", f"{seq}:{'0' * 64}")
    assert differing.returncode == 1 and "FAIL" in differing.stderr
    assert json.loads(differing.stdout)["seq"] == seq
    malformed = run_anchor(tmp_path, "verify", "--artifact", "store", "--expected", "no-anchor")
    assert malformed.returncode == 2

    assert {name: (tmp_path / name).read_bytes() for name in artifacts} == before


def test_role_binding_suite_still_passes(tmp_path: Path) -> None:
    """mcp/role_binding_test.py, unchanged, passes with its three journals chained and intact."""
    import os  # noqa: PLC0415

    env = {**os.environ, "ROLE_BINDING_TEST_DIR": str(tmp_path), "AGENT_ROLE": "project-manager"}
    completed = subprocess.run([sys.executable, str(REPO / "mcp" / "role_binding_test.py")],
                               capture_output=True, text=True, cwd=REPO, env=env, check=False,
                               timeout=600)
    result = re.search(r"^ROLE_BINDING_RESULT passed=(\d+) total=(\d+)$", completed.stdout, re.M)
    assert completed.returncode == 0, completed.stdout[-4000:] + completed.stderr[-4000:]
    assert result and result.group(1) == result.group(2), completed.stdout[-4000:]

    chain = chain_module()
    for name in ("storage-audit.log", "retrieval-audit.log", "gate-audit.log"):
        journal = tmp_path / name
        head = chain.journal_head(journal)
        assert head["seq"] >= 1, name
        assert chain.verify_journal(journal, head["seq"], head["head"])["status"] == "INTACT", name


GATE = REPO / "scripts" / "run-conformance-gate.py"
CONFORMANCE_ENV = "CONFORMANCE_BASE_REF"


def run_conformance_gate(env_extra: dict[str, str] | None = None) -> subprocess.CompletedProcess:
    """Run the conformance wrapper with the environment a base-revision case needs."""
    import os  # noqa: PLC0415

    env = {**os.environ}
    env.pop(CONFORMANCE_ENV, None)
    env.update(env_extra or {})
    return subprocess.run(
        [sys.executable, str(GATE)], capture_output=True, text=True, cwd=REPO, env=env, check=False,
        timeout=600,
    )


def test_conformance_gate_defaults_to_head_and_passes_on_this_tree() -> None:
    """With no base revision named, the wrapper compares against HEAD and this tree is clean."""
    completed = run_conformance_gate()
    assert completed.returncode == 0, completed.stdout[-4000:] + completed.stderr[-4000:]
    report = json.loads(completed.stdout)
    assert report["base_revision"] == "HEAD", report["base_revision"]
    assert report["verdict"] == "pass", report["reason"]


def test_conformance_gate_refuses_a_base_ref_that_is_not_a_hex_sha() -> None:
    """A ref-shaped value never reaches git: the wrapper refuses it as a bad invocation."""
    completed = run_conformance_gate({CONFORMANCE_ENV: "--upload-pack=evil"})
    assert completed.returncode == 2, completed.stdout[-4000:] + completed.stderr[-4000:]
    assert "must be a hex commit SHA" in completed.stderr, completed.stderr


def test_conformance_gate_refuses_a_hex_base_ref_this_checkout_does_not_have() -> None:
    """A hex SHA naming no commit fails closed, instead of reporting every file as incomparable.

    Without this the gate prints `verdict pass` with `files_without_a_baseline` equal to the file
    count: a typo in the CI wiring would turn the gate green, which is the failure it exists to catch.
    """
    completed = run_conformance_gate({CONFORMANCE_ENV: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"})
    assert completed.returncode == 2, completed.stdout[-4000:] + completed.stderr[-4000:]
    assert "names no commit this checkout has" in completed.stderr, completed.stderr


def test_classify_change_refuses_an_empty_change_set() -> None:
    """An empty diff is refused, because it classifies the same as "nothing agent-affecting"."""
    completed = subprocess.run(
        [sys.executable, str(REPO / "scripts" / "classify-change.py")],
        capture_output=True, text=True, cwd=REPO, input="", check=False, timeout=120,
    )
    assert completed.returncode == 2, completed.stdout[-2000:] + completed.stderr[-2000:]
    assert "refusing to classify an empty change set" in completed.stderr, completed.stderr


def test_classify_change_still_answers_a_real_change_set() -> None:
    """The refusal is about an empty list, not about a list with no agent-affecting file."""
    completed = subprocess.run(
        [sys.executable, str(REPO / "scripts" / "classify-change.py"), "--quiet"],
        capture_output=True, text=True, cwd=REPO, input="web/src/lib/api/client.ts\n", check=False,
        timeout=120,
    )
    assert completed.returncode == 0, completed.stdout[-2000:] + completed.stderr[-2000:]


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(pytest.main([__file__, "-v"]))


# Every cargo-shaped line in the fixtures below is synthetic: no real suite or test is named.
B7_SYNTHETIC_TEST_RULE = {
    "reason": "synthetic rule for the collect_group_values unit tests",
    "streams": ["stdout", "stderr"],
    "strip_ansi": True,
    "counts": {
        "suites": {"mode": "count_matching_lines", "pattern": "^test result: "},
        "suite_results": {"mode": "collect_group_values", "group": "value", "limit": 50,
                          "pattern": r"^test result: (?P<value>.+?)(?:; finished in .*)?$"},
        "failed_tests": {"mode": "collect_group_values", "group": "value", "limit": 20,
                         "pattern": r"^test (?P<value>\S+) \.\.\. FAILED$"},
    },
}


def b7_summary(monkeypatch, stdout: str, stderr: str = "") -> dict:
    gate = load_chain_server("b7_summary_gate_server", "mcp/gate/server.py")
    vocabulary = sys.modules["gate_vocabulary"]
    rule = vocabulary._summary({"summary": B7_SYNTHETIC_TEST_RULE}, {})
    assert rule is not None
    monkeypatch.setitem(gate.COMMANDS, "b7-synthetic", {"summary": rule})
    monkeypatch.setitem(gate.SUMMARY_PATTERNS, "b7-synthetic",
                        {label: re.compile(r["pattern"]) for label, r in rule["counts"].items()})
    return gate.compute_output_summary("b7-synthetic", stdout, stderr)


def test_b7_collect_zero_failed(monkeypatch) -> None:
    out = ("running 2 tests\ntest synthetic::alpha ... ok\ntest synthetic::beta ... ok\n\n"
           "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n")
    summary = b7_summary(monkeypatch, out)
    assert summary["counts"] == {"suites": 1, "suite_results": 1, "failed_tests": 0}
    assert summary["values"]["failed_tests"] == {"items": [], "truncated": False}
    assert summary["values"]["suite_results"]["items"] == [
        "ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"]


def test_b7_collect_two_failed_names_both(monkeypatch) -> None:
    out = ("test synthetic::gamma ... \x1b[31mFAILED\x1b[0m\ntest synthetic::delta ... FAILED\n\n"
           "failures:\n    synthetic::gamma\n    synthetic::delta\n\n"
           "test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n")
    summary = b7_summary(monkeypatch, out)
    assert summary["counts"]["suites"] == 1
    assert summary["counts"]["failed_tests"] == 2
    assert summary["values"]["failed_tests"] == {
        "items": ["synthetic::gamma", "synthetic::delta"], "truncated": False}


def test_b7_collect_compile_error_reports_no_suite(monkeypatch) -> None:
    summary = b7_summary(monkeypatch, "", "error[E0308]: mismatched types\nerror: could not compile `synthetic`\n")
    assert summary["counts"] == {"suites": 0, "suite_results": 0, "failed_tests": 0}


def test_b7_collect_caps_items_and_keeps_the_total(monkeypatch) -> None:
    names = [f"synthetic::case_{index:02d}" for index in range(60)]
    out = "".join(f"test {name} ... FAILED\n" for name in names)
    out += "test synthetic::" + "x" * 300 + " ... FAILED\n"
    summary = b7_summary(monkeypatch, out)
    assert summary["counts"]["failed_tests"] == 61
    assert summary["values"]["failed_tests"] == {"items": names[:20], "truncated": True}


def test_b7_collect_value_is_cut_to_the_char_cap(monkeypatch) -> None:
    summary = b7_summary(monkeypatch, "test synthetic::" + "y" * 300 + " ... FAILED\n")
    assert len(summary["values"]["failed_tests"]["items"][0]) == 200


def test_b7_limit_is_validated_whole_rule_or_nothing() -> None:
    load_chain_server("b7_limit_gate_server", "mcp/gate/server.py")
    vocabulary = sys.modules["gate_vocabulary"]
    base = {"mode": "collect_group_values", "pattern": r"^(?P<value>\S+)$", "group": "value"}
    for bad in (0, 51, True, "20", 2.0):
        assert vocabulary._summary({"summary": {"counts": {"c": {**base, "limit": bad}}}}, {}) is None
    assert vocabulary._summary({"summary": {"counts": {"c": {**base, "group": "absent"}}}}, {}) is None
    assert vocabulary._summary({"summary": {"counts": {"c": base}}}, {})["counts"]["c"]["limit"] == 20


def test_b7_fmt_and_no_rule_summaries_carry_no_values_key() -> None:
    gate = load_chain_server("b7_fmt_gate_server", "mcp/gate/server.py")
    fmt = gate.compute_output_summary("fmt", "Diff in /tmp/synthetic.rs:1:\n+a\n-b\n", "")
    assert "values" not in fmt
    assert list(fmt) == ["applied", "counts", "reason", "detail", "streams", "input_chars",
                         "input_truncated_by_the_clamp"]
    assert gate.compute_output_summary("policy", "", "") == {
        "applied": False, "counts": None, "reason": None,
        "detail": "no output summary rule applies to this gate"}
