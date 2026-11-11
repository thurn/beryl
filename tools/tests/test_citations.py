import socket
from pathlib import Path

import pytest

from beryl_tools import citation_migrate, citations, paths

# A release's text as Wizards publishes it: a contents section naming the sections, numbered
# rules with further paragraphs, and a glossary whose numbered senses are no rules.
OLD = """﻿Magic: The Gathering Comprehensive Rules

Contents
7. Additional Rules
712. Double-Faced Cards
Glossary

7. Additional Rules

712. Double-Faced Cards

712.8. Each face of a double-faced card has its own set of characteristics.

712.8a While a double-faced card is outside the game, it has only its front face.

712.8e It has “only” the face that’s up, unless a rule says “otherwise”.
Example: A spell—with a dash.

712.11. A player may look at either face of a double-faced card they can see at any time.

Glossary

Vanguard
1. A casual variant. See rule 902.
"""
NEW = """﻿Contents
712. Double-Faced Cards
Glossary

712. Double-Faced Cards

712.6. Each face of a double-faced card has its own set of characteristics.

712.6a While a double-faced card is outside the game, it has only its front face.

712.6d It has “only” the face that’s up, unless a rule says “otherwise”.
Example: A spell—with a dash.

712.8. A player may look at either face of a double-faced card they can see at any time, even an opponent's.

Glossary
"""
LINK = "[CR {n}](https://yawgatog.com/resources/magic-rules/#{a})"


def cite(number: str) -> str:
    return LINK.format(n=number, a=citations.anchor_of(number))


def write(tmp_path: Path, rel: str, text: str) -> Path:
    path = tmp_path / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    return path


def problems(tmp_path: Path, rel: str, text: str) -> list[str]:
    write(tmp_path, rel, text)
    return citations.check(tmp_path, citations.parse_rules(NEW))


def test_the_text_parses_into_rules_by_anchor() -> None:
    rules = citations.parse_rules(NEW)
    assert sorted(rules) == ["R712", "R7126", "R7126a", "R7126d", "R7128"]
    # Curly quotes are straightened, the rule's double quotes become single ones, an em dash
    # is two hyphens and an example joins its rule.
    assert rules["R7126d"] == (
        "It has 'only' the face that's up, unless a rule says 'otherwise'. "
        "Example: A spell--with a dash."
    )
    # The glossary's numbered senses are no rules; the section heading is the body's.
    assert "R1" not in citations.parse_rules(OLD)
    assert citations.parse_rules(OLD)["R7"] == "Additional Rules"


def test_a_true_citation_is_clean(tmp_path: Path) -> None:
    text = (
        f'/// ({cite("712.6")}): "Each face of a double-faced\n'
        '/// card has its own set of characteristics."\n'
        f'/// {cite("712.6d")}: "It has \'only\' the face ... unless a rule says\n'
        "/// 'otherwise'.\"\n"
        f"/// A link with no quotation, {cite('712.6')}, is only checked for its rule,\n"
        '/// as is a bare CR 712.6a, and a bare one in quotes, "CR 712.6".\n'
    )
    assert problems(tmp_path, "rust/crates/a/src/lib.rs", text) == []


def test_a_dead_anchor_wrong_anchor_or_dead_bare_number_is_reported(
    tmp_path: Path,
) -> None:
    text = (
        f"// {cite('712.8e')}\n"
        "// [CR 712.6](https://yawgatog.com/resources/magic-rules/#R712)\n"
        "// (CR 712.11)\n"
    )
    assert problems(tmp_path, "ui/src/a.ts", text) == [
        "ui/src/a.ts:1: CR 712.8e: no such rule",
        "ui/src/a.ts:2: CR 712.6: the anchor is #R712, not #R7126",
        "ui/src/a.ts:3: CR 712.11: no such rule",
    ]


def test_a_quotation_the_rule_lacks_is_reported_per_segment(tmp_path: Path) -> None:
    text = (
        f'/// {cite("712.6")}: "Each face of a double-faced card ... has two sets."\n'
        '/// CR 712.6a: "it has both faces"\n'
    )
    assert problems(tmp_path, "rust/crates/a/src/lib.rs", text) == [
        'rust/crates/a/src/lib.rs:1: CR 712.6: the rule does not say "has two sets."',
        'rust/crates/a/src/lib.rs:2: CR 712.6a: the rule does not say "it has both faces"',
    ]


def test_text_outside_the_roots_and_a_quotation_not_after_the_link_are_ignored(
    tmp_path: Path,
) -> None:
    text = f'// {cite("712.6")} says things "that the page never does".\n'
    assert problems(tmp_path, "rust/crates/a/src/lib.rs", text) == []
    assert problems(tmp_path, "tools/tests/a.py", f"# {cite('1.1')}\n") == []


def migrate(tmp_path: Path, text: str) -> tuple[str, list[str], list[str]]:
    path = write(tmp_path, "rust/crates/a/src/lib.rs", text)
    moved, notes = citation_migrate.migrate(
        tmp_path, citations.parse_rules(OLD), citations.parse_rules(NEW), write=True
    )
    return path.read_text(), moved, notes


def test_migration_renumbers_by_quotation_and_by_text_in_one_pass(
    tmp_path: Path,
) -> None:
    # 712.11 becomes 712.8 while 712.8 becomes 712.6: neither may apply twice.
    text = (
        f'/// {cite("712.8e")}: "the face that\'s up"\n'
        "/// CR 712.8a, CR 712.8 and CR 712.11 on one line.\n"
        f"/// {cite('712.8')} links too.\n"
    )
    after, moved, notes = migrate(tmp_path, text)
    assert after == (
        f'/// {cite("712.6d")}: "the face that\'s up"\n'
        "/// CR 712.6a, CR 712.6 and CR 712.8 on one line.\n"
        f"/// {cite('712.6')} links too.\n"
    )
    assert len(moved) == 5
    # 712.11 kept its meaning but gained words: one note for the rule.
    assert notes == [
        "CR 712.11 (now CR 712.8) was reworded; check its 1 citations still hold: "
        '"A player may look at either face of a double-faced card they can see at any '
        "time, even an opponent's.\""
    ]
    assert citations.check(tmp_path, citations.parse_rules(NEW)) == []


def test_migration_leaves_what_it_cannot_place_and_reports_it(tmp_path: Path) -> None:
    text = (
        '/// CR 712.8e: "the face that is down"\n'
        "/// CR 712.9 is no rule in either release.\n"
    )
    after, moved, notes = migrate(tmp_path, text)
    assert moved == [
        "rust/crates/a/src/lib.rs:1: CR 712.8e -> CR 712.6d",
    ]
    assert notes[0].startswith(
        "rust/crates/a/src/lib.rs:1: CR 712.8e: the quotation was not the old rule's "
        "text either; requote from CR 712.6d:"
    )
    assert (
        notes[1]
        == "rust/crates/a/src/lib.rs:2: CR 712.9: no such rule in either release"
    )
    assert "CR 712.9 is no rule" in after


def test_a_version_pins_and_reads_back(tmp_path: Path) -> None:
    (tmp_path / "data").mkdir()
    version = citations.Version.for_date("2026-11-06")
    assert version.url.endswith("/2026/downloads/MagicCompRules%2020261106.txt")
    citations.pin(tmp_path, version)
    assert citations.pinned(tmp_path) == version


def refused_url() -> str:
    """A URL on this machine that nothing listens at, so a fetch fails at once."""
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    return f"http://127.0.0.1:{port}/MagicCompRules.txt"


def step_tree(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, cached: bool
) -> list[str]:
    """A tree pinned to a release at an unreachable URL, with one wrong citation and one
    right one, and the step's rules cache, holding that release's text when `cached`."""
    root, cache = tmp_path / "repo", tmp_path / "cache"
    (root / "data").mkdir(parents=True)
    citations.pin(root, citations.Version("2026-11-06", refused_url()))
    write(root, "docs/a.md", f'{cite("712.6a")}: "not the rule\'s text"\n')
    write(root, "docs/b.md", f'{cite("712.6a")}: "it has only its front face"\n')
    monkeypatch.setattr(paths, "REPO", root)
    monkeypatch.setenv("BERYL_CACHE", str(cache))
    if cached:
        write(cache, "rules/2026-11-06.txt", NEW)
    return ["docs/a.md", "docs/b.md"]


def test_the_step_holds_the_tree_or_the_files_named_to_the_cached_text(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    wrong, right = step_tree(tmp_path, monkeypatch, cached=True)
    assert citations.step("--fetch") == 1
    assert capsys.readouterr().out == (
        f'citations: {wrong}:1: CR 712.6a: the rule does not say "not the rule\'s text"\n'
    )
    assert citations.step(wrong) == 1
    capsys.readouterr()
    # A file the branch deleted, or one that cites no rules, is passed over.
    assert citations.step(right, "docs/gone.md", "rust/crates/a/target/b.rs") == 0
    assert capsys.readouterr().out == ""


def test_the_step_skips_and_passes_with_no_text_to_hold_the_tree_to(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    wrong, _ = step_tree(tmp_path, monkeypatch, cached=False)
    # The fast check never fetches.
    assert citations.step(wrong) == 0
    assert capsys.readouterr().out == (
        "citations: skipped: the 2026-11-06 rules are not cached\n"
    )
    # Slow CI fetches, and a failed fetch is no failure.
    assert citations.step("--fetch") == 0
    assert capsys.readouterr().out.startswith(
        "citations: skipped: the 2026-11-06 rules could not be fetched: "
    )
    assert not citations.cached_text(citations.pinned(paths.REPO)).exists()
