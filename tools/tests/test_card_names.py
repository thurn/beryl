import json
from pathlib import Path

from beryl_tools import card_names

NAMES = ["Ponder", "Jace, the Mind Sculptor", "Jace", "Aang's Iceberg", "Island"]
INDEX = card_names.NameIndex(
    [n for n in NAMES if n not in card_names.BASIC_LAND_TYPES], {"PONDER"}
)
ENGINE = "rust/crates/beryl_rules/src"


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text)


def test_names_are_found_whole_and_longest_first() -> None:
    assert INDEX.in_line('x = "Ponder";') == ["Ponder"]
    assert INDEX.in_line("Pondering, Jace, the Mind Sculptor's +2") == [
        "Jace, the Mind Sculptor"
    ]
    assert INDEX.in_line("Aang's Iceberg and Jace") == ["Aang's Iceberg", "Jace"]
    assert INDEX.in_line("names::PONDER, names::OTHER, decks::WU_CONTROL") == [
        "names::PONDER",
        "decks::WU_CONTROL",
    ]
    assert INDEX.in_line("Subtype::Island") == []


def test_a_card_in_code_counts_but_not_in_a_comment_a_doc_or_a_test(
    tmp_path: Path,
) -> None:
    write(
        tmp_path,
        f"{ENGINE}/a.rs",
        '// Ponder here\n/// Ponder too\nconst A: &str = "Ponder";\n'
        '/* Jace\n */ fn f() -> &\'static str { r#"Jace"# }\n'
        '#[cfg(test)]\nmod tests {\n    const T: &str = "Ponder";\n}\n'
        'const B: &str = "Ponder";\n',
    )
    write(tmp_path, f"{ENGINE}/tests/b.rs", 'const T: &str = "Ponder";\n')
    write(tmp_path, f"{ENGINE}/c_tests.rs", 'const T: &str = "Ponder";\n')
    write(tmp_path, f"{ENGINE}/expert/d.rs", 'const T: &str = "Ponder";\n')
    write(
        tmp_path,
        "ui/src/e.tsx",
        "// Ponder\nconst e = <p>don't</p>; const x = 'Ponder';\n",
    )
    write(
        tmp_path,
        "tools/src/beryl_tools/f.py",
        '"""Ponder."""\n# Ponder\nX = "Ponder"\n',
    )
    write(tmp_path, "rust/crates/beryl_cards/src/g.rs", 'const T: &str = "Ponder";\n')
    hits = card_names.scan(tmp_path, INDEX)
    assert {path: [n for _, n in found] for path, found in hits.items()} == {
        f"{ENGINE}/a.rs": ["Ponder", "Jace", "Ponder"],
        "ui/src/e.tsx": ["Ponder"],
        "tools/src/beryl_tools/f.py": ["Ponder"],
    }
    assert hits[f"{ENGINE}/a.rs"][0][0] == 3


def test_the_allowlist_may_only_shrink() -> None:
    hits = {"a.rs": [(1, "Ponder")] * 2, "b.rs": [(1, "Jace")]}
    assert card_names.problems(hits, {"a.rs": 2, "b.rs": 1}, [], []) == []
    rose = card_names.problems(hits, {"a.rs": 1, "b.rs": 1}, [], [])
    assert len(rose) == 1 and "a.rs: 2 card names, up from 1" in rose[0]
    new = card_names.problems(hits, {"a.rs": 2}, [], [])
    assert len(new) == 1 and "b.rs" in new[0] and "not on the allowlist" in new[0]
    stale = card_names.problems(hits, {"a.rs": 3, "b.rs": 1, "c.rs": 1}, [], [])
    assert len(stale) == 2
    assert "lower it to 2" in stale[0]
    assert "c.rs: 0 card names" in stale[1] and "remove the entry" in stale[1]


def test_a_listed_mechanic_that_is_gone_fails() -> None:
    sources = [
        card_names.lex_c_like(
            "pub enum Rule { OneCard, Other }\n"
            "pub struct Player<L> { pub flag: bool }\n"
            "impl<L: Layout> Player<L> { fn helper(&self) {} }\n"
            "// enum Rule { Gone }\n",
            rust=True,
        )[1]
    ]
    listed = [
        "Rule::OneCard",
        "Rule::Gone",
        "Player::flag",
        "Player::helper",
        "Face::x",
    ]
    assert card_names.missing_mechanics(listed, sources) == ["Rule::Gone", "Face::x"]
    found = card_names.problems({}, {}, listed, sources)
    assert [line.split(":")[0] for line in found] == ["Rule", "Face"]


def test_the_check_over_a_tree(tmp_path: Path) -> None:
    write(tmp_path, f"{ENGINE}/rules.rs", "pub enum Rule { OneCard }\n")
    write(tmp_path, f"{ENGINE}/names.rs", 'const A: &str = "Ponder";\n')
    allowlist = tmp_path / "allowlist.json"
    allowlist.write_text(
        json.dumps({"names": {f"{ENGINE}/names.rs": 1}, "mechanics": ["Rule::OneCard"]})
    )
    assert card_names.check(tmp_path, allowlist, INDEX) == []
    # A card named in another engine file fails.
    write(tmp_path, f"{ENGINE}/cast.rs", 'fn f() { cast("Ponder"); }\n')
    [added] = card_names.check(tmp_path, allowlist, INDEX)
    assert "cast.rs" in added and "not on the allowlist" in added
    (tmp_path / ENGINE / "cast.rs").unlink()
    # A name removed leaves the allowlisted count stale, and the mechanic's removal likewise.
    write(tmp_path, f"{ENGINE}/names.rs", "")
    write(tmp_path, f"{ENGINE}/rules.rs", "pub enum Rule {}\n")
    stale = card_names.check(tmp_path, allowlist, INDEX)
    assert (
        len(stale) == 2
        and "remove the entry" in stale[0]
        and "Rule::OneCard" in stale[1]
    )
    # Shrinking rewrites the file to match, and never raises a count.
    assert card_names.check(tmp_path, allowlist, INDEX, shrink=True) == []
    assert json.loads(allowlist.read_text()) == {"names": {}, "mechanics": []}
    write(tmp_path, f"{ENGINE}/names.rs", 'const A: &str = "Ponder";\n')
    assert len(card_names.check(tmp_path, allowlist, INDEX, shrink=True)) == 1


def test_a_card_file_names_its_card_through_its_definition(tmp_path: Path) -> None:
    src = "rust/crates/beryl_cards/src"
    write(
        tmp_path,
        f"{src}/names.rs",
        'pub const COUNTERSPELL: &str = "Counterspell";\n'
        'pub const JACE_THE_MIND_SCULPTOR: &str = "Jace, the Mind Sculptor";\n',
    )
    definition = "pub static DEFINITION: Card = Card {{\n    name: names::{},\n}};\n"
    write(tmp_path, f"{src}/blue/counterspell.rs", definition.format("COUNTERSPELL"))
    write(
        tmp_path,
        f"{src}/blue/jace_the_mind_sculptor.rs",
        definition.format("JACE_THE_MIND_SCULPTOR")
        + "const X: &[&str] = &[names::COUNTERSPELL];\n",
    )
    write(tmp_path, f"{src}/blue/mod.rs", "pub mod counterspell;\n")
    write(tmp_path, f"{src}/lib.rs", definition.format("COUNTERSPELL"))
    changed = [
        f"{src}/blue/jace_the_mind_sculptor.rs",
        f"{src}/blue/counterspell.rs",
        f"{src}/blue/mod.rs",
        f"{src}/lib.rs",
        f"{src}/blue/deleted.rs",
        "rust/crates/beryl_rules/src/blue/counterspell.rs",
        f"{src}/blue/counterspell.rs",
    ]
    assert card_names.cards_in(changed, tmp_path) == [
        "Jace, the Mind Sculptor",
        "Counterspell",
    ]


def test_every_card_file_names_its_card() -> None:
    files = [
        p.relative_to(card_names.paths.REPO).as_posix()
        for p in (card_names.NAMES_RS.parent).glob("*/*.rs")
        if p.name != "mod.rs" and p.parent.name not in ("bin", "tests")
    ]
    assert len(card_names.cards_in(files)) == len(files)
