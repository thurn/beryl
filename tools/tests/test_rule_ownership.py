import json
from pathlib import Path

from beryl_tools import citations, rule_ownership

KERNEL: str = rule_ownership.SOURCE_ROOTS[0]
RULES: str = rule_ownership.SOURCE_ROOTS[1]
RELEASE: dict[str, str] = citations.parse_rules("""601. Casting Spells
601.2. Casting a spell.
601.2a Propose the spell.
601.20. A distinct rule from 601.2.
602. Activating Abilities
602.1. Activate an ability.
""")


def write(root: Path, rel: str, text: str) -> None:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def audit(root: Path, *allow: str) -> list[str]:
    write(root, rule_ownership.ALLOWLIST, json.dumps(allow))
    return rule_ownership.check(root, RELEASE)


def test_a_child_can_cite_its_own_and_its_enclosing_modules_rules(
    tmp_path: Path,
) -> None:
    write(tmp_path, f"{RULES}/casting/mod.rs", "//! Implements: CR 601.2\n")
    write(
        tmp_path,
        f"{RULES}/casting/legal.rs",
        "//! Implements: CR 602.1\n"
        "/// Implements [CR 601.2a] and CR 602.1.\n"
        "/// See [CR 601.20], and see CR 602.\n"
        "/// For the other action see\n"
        "/// [CR 602].\n"
        "/// [CR 601.20]: https://yawgatog.com/resources/magic-rules/#R60120\n"
        "/// [CR 601.2a]: https://yawgatog.com/resources/magic-rules/#R6012a\n"
        "/// [CR 602]: https://yawgatog.com/resources/magic-rules/#R602\n",
    )
    assert audit(tmp_path) == []


def test_an_unowned_citation_after_a_test_item_still_fails(tmp_path: Path) -> None:
    write(
        tmp_path,
        f"{KERNEL}/cast.rs",
        "//! Implements: CR 601.2\n"
        "/// CR 601.2\n"
        "fn cast() {}\n"
        "#[cfg(test)]\n"
        "mod sample {\n"
        "    // CR 602.1\n"
        "}\n"
        "// CR 601.20\n",
    )
    assert audit(tmp_path) == [
        f"{KERNEL}/cast.rs:8: CR 601.20 is outside the module's "
        "Implements declaration; use 'see' for a cross-reference"
    ]


def test_a_header_and_cross_reference_do_not_count_as_implementation(
    tmp_path: Path,
) -> None:
    write(
        tmp_path,
        f"{KERNEL}/cast.rs",
        "//! Implements: CR 601.2\n"
        "//! See [CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a).\n",
    )
    assert audit(tmp_path) == [
        f"{KERNEL}/cast.rs:1: declares CR 601.2 without an implementation citation"
    ]


def test_a_subrule_conflicts_with_the_same_rule_claimed_by_another_crate(
    tmp_path: Path,
) -> None:
    write(tmp_path, f"{KERNEL}/cast.rs", "//! Implements: CR 601.2\n// CR 601.2\n")
    write(tmp_path, f"{RULES}/cast.rs", "//! Implements: CR 601.2a\n// CR 601.2a\n")
    assert audit(tmp_path) == [
        f"{RULES}/cast.rs:1: CR 601.2a overlaps the declaration in {KERNEL}/cast.rs"
    ]


def test_only_files_waiting_on_a_declaration_belong_on_the_allowlist(
    tmp_path: Path,
) -> None:
    new = f"{KERNEL}/new.rs"
    old = f"{KERNEL}/old.rs"
    done = f"{RULES}/scratch.rs"
    write(tmp_path, new, "")
    write(tmp_path, old, "// CR 601")
    write(tmp_path, done, "//! Implements: none. Holds scratch buffers.\n")
    write(tmp_path, f"{KERNEL}/tests.rs", "// CR 601")
    write(tmp_path, f"{RULES}/tests/nested/example.rs", "// CR 602")
    write(tmp_path, "engine/cards/set/example.rs", "// CR 602")
    assert audit(tmp_path, old, done) == [
        f"{new}: missing an Implements declaration",
        f"{rule_ownership.ALLOWLIST}: remove {done}",
    ]


def test_module_file_ownership_and_coverage_include_only_its_subtree(
    tmp_path: Path,
) -> None:
    write(tmp_path, f"{RULES}/casting.rs", "//! Implements: CR 601.2\n")
    write(tmp_path, f"{RULES}/casting/steps.rs", "// CR 601.2a\n")
    assert audit(tmp_path) == []
    assert rule_ownership.coverage(tmp_path, RELEASE) == (
        {"601": ["601", "601.20"], "602": ["602", "602.1"]},
        [],
    )


def test_a_claim_requires_a_valid_rule_or_an_explanation(tmp_path: Path) -> None:
    write(tmp_path, f"{KERNEL}/bad.rs", "//! Implements: CR 999\n")
    write(tmp_path, f"{KERNEL}/buffers.rs", "//! Implements: none\n")
    assert audit(tmp_path) == [
        f"{KERNEL}/bad.rs:1: no such rule CR 999",
        f"{KERNEL}/buffers.rs:1: expected CR number(s) or 'none. <reason>.'",
    ]


def test_an_allowlisted_child_can_be_adopted_after_its_parent(tmp_path: Path) -> None:
    child = f"{RULES}/casting/steps.rs"
    write(
        tmp_path,
        f"{RULES}/casting/mod.rs",
        "//! Implements: CR 601.2\n// CR 601.2\n",
    )
    write(tmp_path, child, "// CR 602.1\n")
    assert audit(tmp_path, child) == []
    assert audit(tmp_path) == [
        f"{child}:1: CR 602.1 is outside the module's "
        "Implements declaration; use 'see' for a cross-reference"
    ]
