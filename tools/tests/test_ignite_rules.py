import ast
import gzip
import json
import re
import shutil
import subprocess
import tomllib
from pathlib import Path

import pytest

from beryl_tools import (
    citations,
    conventions,
    design_tokens,
    file_length,
    paths,
    scryfall,
)
from beryl_tools.ignite import rules

REPO: Path = Path(__file__).resolve().parents[2]
RELEASE_TEXT = """Contents
601. Casting Spells
Glossary

601. Casting Spells

601.2. Casting a spell.

601.2a Propose the spell.

Glossary
"""
LINK = "https://yawgatog.com/resources/magic-rules/#"
KERNEL = "rust/crates/beryl_kernel/src"
RULES = "rust/crates/beryl_rules/src"


def write(root: Path, rel: str, text: str) -> None:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def repository(tmp_path: Path) -> Path:
    root = tmp_path / "repo"
    root.mkdir()
    subprocess.run(["git", "init", "--quiet", str(root)], check=True)
    return root


def outcome(name: str, root: Path) -> dict[str, object]:
    [check] = [c for c in rules.CHECKS if c.name == name]
    return rules.result(check, root)


def passes(name: str, root: Path) -> bool:
    return outcome(name, root)["outcome"] == "pass"


def fails_at(name: str, root: Path, where: str) -> None:
    result = outcome(name, root)
    assert result["outcome"] == "fail", result
    assert result.get("at") == where, result


def pin_rules(root: Path, cached: bool = True) -> None:
    (root / "data").mkdir(exist_ok=True)
    citations.pin(root, citations.Version("2026-11-06", "http://127.0.0.1:9/rules.txt"))
    if cached:
        write(paths.cache_root(), "rules/2026-11-06.txt", RELEASE_TEXT)


def test_commit_subjects_are_conventional_and_short(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = repository(tmp_path)

    def commit(subject: str, root: Path = root) -> None:
        write(root, "a.txt", subject)
        subprocess.run(["git", "-C", str(root), "add", "."], check=True)
        subprocess.run(
            ["git", "-C", str(root), "-c", "user.name=t", "-c", "user.email=t@t"]
            + ["commit", "--quiet", "-m", subject],
            check=True,
        )

    commit("chore: start")
    base = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "HEAD"], capture_output=True, text=True
    ).stdout.strip()
    monkeypatch.setenv("IGNITE_BASE", base)
    commit("feat: mulligan decisions")
    assert passes("commit-subject", root)
    commit("Fixed the thing")
    commit("fix: " + "x" * 60)
    result = outcome("commit-subject", root)
    assert result["outcome"] == "fail"
    message = str(result["message"])
    assert "not a Conventional Commit" in message and "over 60 characters" in message


def test_the_commit_check_lists_a_digest_of_the_subjects_it_reads(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root: Path = repository(tmp_path)

    def commit(subject: str) -> str:
        write(root, "a.txt", subject)
        subprocess.run(["git", "-C", str(root), "add", "."], check=True)
        subprocess.run(
            ["git", "-C", str(root), "-c", "user.name=t", "-c", "user.email=t@t"]
            + ["commit", "--quiet", "-m", subject],
            check=True,
        )
        return subprocess.run(
            ["git", "-C", str(root), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
        ).stdout.strip()

    def digest() -> object:
        return rules.listing("commit", root)[0]["digest"]

    monkeypatch.setenv("IGNITE_BASE", commit("chore: start"))
    commit("feat: one")
    first = digest()
    assert digest() == first, "the same subjects give the same digest"
    commit("Fixed the thing")
    assert digest() != first, "a new subject is a new input"
    monkeypatch.delenv("IGNITE_BASE")
    assert digest() != digest(), "with no base, no listing shares a digest"


def test_a_commit_check_with_no_base_is_incomplete(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delenv("IGNITE_BASE", raising=False)
    assert outcome("commit-subject", repository(tmp_path))["outcome"] == "incomplete"


def test_conflict_markers_are_found_in_source_and_in_markdown_separately(
    tmp_path: Path,
) -> None:
    root = repository(tmp_path)
    write(root, "a.rs", "fn main() {}\n")
    write(root, "docs/a.md", "# Title\n")
    assert passes("conflict-markers", root) and passes(
        "markdown-conflict-markers", root
    )
    write(root, "src/b.rs", "fn a() {}\n<<<<<<< HEAD\nfn b() {}\n")
    fails_at("conflict-markers", root, "src/b.rs:2")
    assert passes("markdown-conflict-markers", root)
    write(root, "docs/c.md", "text\n>>>>>>> main\n")
    fails_at("markdown-conflict-markers", root, "docs/c.md:2")


def test_markdown_must_be_formatted_as_mdformat_writes_it(tmp_path: Path) -> None:
    root = repository(tmp_path)
    write(root, "docs/a.md", "# Title\n\nText.\n")
    assert passes("markdown-format", root)
    write(root, "docs/b.md", "#   Title\n\n\n\n* item\n")
    assert outcome("markdown-format", root)["outcome"] == "fail"


def test_file_length_holds_a_tree_to_its_allowlist(tmp_path: Path) -> None:
    root = repository(tmp_path)
    write(root, "data/file-length-allowlist.json", "{}")
    write(root, "ui/src/a.ts", "x\n" * 800)
    assert passes("file-length", root)
    write(root, "ui/src/b.ts", "x\n" * 801)
    assert outcome("file-length", root)["outcome"] == "fail"


def test_card_names_stay_out_of_generic_code(tmp_path: Path) -> None:
    root = repository(tmp_path)
    write(root, "data/card-names-allowlist.json", '{"names": {}, "mechanics": []}')
    write(root, f"{KERNEL}/a.rs", "// Counts permanents.\n")
    assert passes("card-names", root)
    write(root, f"{KERNEL}/b.rs", 'fn f() { let _ = "Thieving Skydiver"; }\n')
    assert outcome("card-names", root)["outcome"] == "fail"


def test_rules_code_keeps_to_two_players_the_board_and_pushes(tmp_path: Path) -> None:
    root = repository(tmp_path)
    write(root, f"{RULES}/a.rs", "fn a(s: &State) { s.opponents_of(p); }\n")
    for name in ("two-player", "board-door", "discarded-push"):
        assert passes(name, root), name
    write(root, f"{RULES}/b.rs", "use k::two_player;\n")
    fails_at("two-player", root, f"{RULES}/b.rs:1")
    write(root, f"{RULES}/c.rs", "fn c(s: &State) { s.board(); }\n")
    assert outcome("board-door", root)["outcome"] == "fail"
    write(root, f"{RULES}/d.rs", "fn d(q: &mut Queue) {\n    let _ = q.push(1);\n}\n")
    fails_at("discarded-push", root, f"{RULES}/d.rs:2")


def test_code_does_not_cite_a_planning_document(tmp_path: Path) -> None:
    root = repository(tmp_path)
    write(root, "tools/src/a.py", "# Nothing here.\n")
    assert passes("plan-references", root)
    write(root, "tools/src/b.py", "# See plan K16.\n")
    fails_at("plan-references", root, "tools/src/b.py:1")


def test_citations_are_held_to_the_cached_release_in_code_and_in_docs(
    tmp_path: Path,
) -> None:
    root = repository(tmp_path)
    pin_rules(root)
    good = f'[CR 601.2a]({LINK}R6012a): "Propose the spell"\n'
    bad = f'[CR 601.2a]({LINK}R6012a): "not the rule\'s text"\n'
    write(root, "tools/src/a.py", "# " + good)
    write(root, "docs/a.md", good)
    assert passes("citations", root) and passes("citations-docs", root)
    write(root, "docs/b.md", bad)
    assert passes("citations", root)
    fails_at("citations-docs", root, "docs/b.md:1")
    write(root, "tools/src/b.py", "# " + bad)
    fails_at("citations", root, "tools/src/b.py:1")


def test_checks_that_need_the_rules_or_rulings_are_incomplete_without_them(
    tmp_path: Path,
) -> None:
    root = repository(tmp_path)
    pin_rules(root, cached=False)
    write(root, "data/oracle-divergences.json", "[]")
    write(root, "data/rule-ownership-allowlist.json", "[]")
    write(root, "tools/src/a.py", "# Nothing.\n")
    write(root, "docs/a.md", "Nothing.\n")
    for name in (
        "citations",
        "citations-docs",
        "rule-ownership",
        "divergences",
    ):
        result = outcome(name, root)
        assert result["outcome"] == "incomplete", (name, result)
        assert "not cached" in str(result["message"])


def test_rulings_check_refills_a_missing_cache_and_blames_the_machine_if_it_cannot(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = repository(tmp_path)
    cached: Path = scryfall.cache_dir() / "rulings.jsonl.gz"
    cached.unlink(missing_ok=True)
    calls: list[str] = []

    def refuse(kind: str, refresh: bool = False) -> Path:
        calls.append(kind)
        raise OSError("offline")

    monkeypatch.setattr(scryfall, "bulk_file", refuse)
    result = outcome("rulings", root)
    assert result["outcome"] == "incomplete", result
    assert "machine fault" in str(result["message"]) and "offline" in str(
        result["message"]
    )

    def fill(kind: str, refresh: bool = False) -> Path:
        calls.append(kind)
        cached.write_bytes(gzip.compress(b""))
        return cached

    monkeypatch.setattr(scryfall, "bulk_file", fill)
    assert outcome("rulings", root)["outcome"] != "incomplete"
    assert calls == ["rulings", "rulings"]


def test_divergences_and_rule_ownership_run_on_the_cached_release(
    tmp_path: Path,
) -> None:
    root = repository(tmp_path)
    pin_rules(root)
    write(root, "data/oracle-divergences.json", "[]")
    write(root, "data/rule-ownership-allowlist.json", "[]")
    write(root, f"{RULES}/mod.rs", "//! Implements: CR 601.2\n// CR 601.2\n")
    assert passes("divergences", root) and passes("rule-ownership", root)
    write(root, "data/oracle-divergences.json", "{}")
    assert outcome("divergences", root)["outcome"] == "fail"
    write(root, f"{RULES}/other.rs", "//! Implements: CR 999.9\n")
    assert outcome("rule-ownership", root)["outcome"] == "fail"


HAVE_JUST: bool = shutil.which("just") is not None


@pytest.mark.skipif(not HAVE_JUST, reason="just is not installed")
def test_the_justfile_must_be_formatted_as_just_writes_it(tmp_path: Path) -> None:
    root = repository(tmp_path)
    write(root, "justfile", "default:\n    echo hi\n")
    if outcome("justfile-format", root)["outcome"] == "incomplete":
        pytest.skip("just is older than the justfile's format needs")
    assert passes("justfile-format", root)
    write(root, "justfile", "default :\n        echo   hi\n\n\n")
    assert outcome("justfile-format", root)["outcome"] == "fail"


def test_generated_pages_are_held_to_their_data(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = repository(tmp_path)
    assert passes("conventions", root) and passes("design-tokens", root)
    write(tmp_path, "page.md", "stale\n")
    write(tmp_path, "tokens.css", "stale\n")
    monkeypatch.setattr(conventions, "PAGE", tmp_path / "page.md")
    monkeypatch.setattr(design_tokens, "STYLESHEET", tmp_path / "tokens.css")
    result = outcome("conventions", root)
    assert result["outcome"] == "fail" and "is stale" in str(result["message"])
    result = outcome("design-tokens", root)
    assert result["outcome"] == "fail" and "is stale" in str(result["message"])


def test_run_writes_one_line_per_check_to_the_results_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = repository(tmp_path)
    write(root, "data/file-length-allowlist.json", "{}")
    monkeypatch.setattr(paths, "REPO", root)
    monkeypatch.setattr(
        file_length, "ALLOWLIST", root / "data/file-length-allowlist.json"
    )
    results = tmp_path / "results.jsonl"
    monkeypatch.setenv("IGNITE_RESULTS", str(results))
    assert rules.main(["run", "source", "source:file-length", "plan-references"]) == 0
    lines = [json.loads(line) for line in results.read_text().splitlines()]
    assert [(r["check"], r["outcome"]) for r in lines] == [
        ("source:file-length", "pass"),
        ("source:plan-references", "pass"),
    ]
    assert rules.main(["run", "source", "nope"]) == 2


def test_every_family_lists_its_checks_with_their_own_inputs(
    capsys: pytest.CaptureFixture[str],
) -> None:
    names = []
    for family in ("commit", "source", "markdown", "repository"):
        assert rules.main(["list", family]) == 0
        for line in capsys.readouterr().out.splitlines():
            entry = json.loads(line)
            assert entry["check"].startswith(f"{family}:")
            assert all(isinstance(i, str) for i in entry["inputs"])
            names.append(entry["check"])
    assert len(names) == len(set(names)) == len(rules.CHECKS)
    # The always set of docs/ignite.md.
    wanted = {
        "commit-subject", "conflict-markers", "file-length", "markdown-format",
        "justfile-format", "conventions", "design-tokens", "card-names", "two-player",
        "board-door", "discarded-push", "plan-references", "citations", "rulings",
        "rule-ownership", "divergences",
    }  # fmt: skip
    assert wanted <= {c.name for c in rules.CHECKS}


def declared() -> dict[str, dict[str, object]]:
    text = (REPO / "ignite/checks/rules.toml").read_text()
    return tomllib.loads(text)["family"]


def matches(pattern: str, path: str) -> bool:
    """The glob of docs/ignite.md: `*` and `?` within a segment, `**` across segments."""
    out = ""
    i = 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out, i = out + "(?:.*/)?", i + 3
        elif pattern.startswith("**", i):
            out, i = out + ".*", i + 2
        elif pattern[i] == "*":
            out, i = out + "[^/]*", i + 1
        elif pattern[i] == "?":
            out, i = out + "[^/]", i + 1
        else:
            out, i = out + re.escape(pattern[i]), i + 1
    return re.fullmatch(out, path) is not None


def reading(path: str) -> set[str]:
    """The checks whose input patterns, with their family's, match `path`."""
    families = declared()
    return {
        c.name
        for c in rules.CHECKS
        if any(
            matches(str(p), path)
            for p in [*families[c.family]["inputs"], *c.inputs]  # type: ignore[union-attr]
        )
    }


def test_the_manifest_declares_each_family_the_adapter_lists() -> None:
    families = declared()
    assert set(families) == {c.family for c in rules.CHECKS}
    for name, family in families.items():
        assert family["kind"] == "regression" and family["always"] is True
        assert family["adapter"] == "jsonl" and family["platforms"] == "any"
        assert f"rules list {name}" in str(family["list"])
        assert f"rules run {name} {{checks}}" in str(family["run"])


def test_a_documentation_change_reruns_only_the_markdown_checks() -> None:
    assert reading("docs/ignite.md") == {
        "markdown-format",
        "markdown-conflict-markers",
        "citations-docs",
    }
    # And a change to rules code reads the rules checks, not the Markdown ones.
    code = reading("rust/crates/beryl_rules/src/combat/mod.rs")
    assert {"two-player", "board-door", "file-length", "rule-ownership"} <= code
    assert not code & {"markdown-format", "citations-docs"}


FORBIDDEN = {
    "tokens", "lanes", "step_runner", "selection", "rust_fit", "python_fit", "runlog",
    "runstore", "verdicts", "budget", "hang", "flaky", "bisect", "slow_ci", "sweep",
    "nightly", "warm", "caches", "incidents", "deadline", "clock", "check_value", "checks",
}  # fmt: skip


def test_the_adapter_imports_no_orchestration() -> None:
    source = (REPO / "tools/src/beryl_tools/ignite/rules.py").read_text()
    imported: set[str] = set()
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.ImportFrom) and node.level == 2:
            imported.update(a.name for a in node.names)
            if node.module:
                imported.add(node.module.split(".")[0])
    assert imported and not imported & FORBIDDEN


def test_no_listed_input_matches_nothing() -> None:
    """Ignite refuses a check whose input pattern matches no tracked file: `source:citations`
    named `docs/*.json` and every landing failed with "invalid: docs/*.json: matches no tracked
    file". Each listed wildcard pattern must match something."""
    for family in sorted({c.family for c in rules.CHECKS}):
        for line in rules.listing(family, REPO):
            for pattern in line["inputs"]:
                if "*" in pattern and not pattern.endswith("/**"):
                    found = rules.git(
                        REPO,
                        "ls-files",
                        "-co",
                        "--exclude-standard",
                        "--",
                        f":(glob){pattern}",
                    )
                    assert found, f"{line['check']}: {pattern} matches no tracked file"
