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

        - Cite the role list (`scripts/run-agent.sh:34` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher"`).
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

        - Cite the launcher's role list (`scripts/run-agent.sh:33` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher"`).
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
        - Cite a drifted line (`scripts/run-agent.sh:34` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher"`).
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


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(pytest.main([__file__, "-v"]))
