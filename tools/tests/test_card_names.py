import json
from pathlib import Path

from beryl_tools import card_files, card_names, card_store, paths, set_groups

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


def test_a_card_file_names_its_card_by_its_path(tmp_path: Path) -> None:
    lea, wwk = "rust/cards/cards_1993_1994/lea", "rust/cards/cards_2009_2011/wwk"
    for path in (
        f"{lea}/counterspell.rs",
        f"{wwk}/jace_the_mind_sculptor.rs",
        # A file in a set its card was not first printed in, and a crate's own files.
        f"{wwk}/counterspell.rs",
        "rust/cards/cards_1993_1994/lib.rs",
        "rust/crates/beryl_rules/src/blue/counterspell.rs",
    ):
        write(tmp_path, path, "")
    changed = [
        f"{wwk}/jace_the_mind_sculptor.rs",
        f"{lea}/counterspell.rs",
        f"{wwk}/counterspell.rs",
        "rust/cards/cards_1993_1994/lib.rs",
        f"{lea}/deleted.rs",
        "rust/crates/beryl_rules/src/blue/counterspell.rs",
        f"{lea}/counterspell.rs",
    ]
    assert card_files.cards_in(changed, tmp_path) == [
        "Jace, the Mind Sculptor",
        "Counterspell",
    ]


def test_every_card_file_is_where_its_first_printing_puts_it() -> None:
    files = card_files.files()
    assert len(files) >= 47
    groups = set_groups.SetGroups()
    with card_store.Store() as store:
        for path in files:
            name = card_files.name_of(path, store)
            assert name is not None, f"{path} is no card of its set in the store"
            record = store.card(name)
            assert record is not None and groups.card_path(record) == path
    rels = [p.relative_to(paths.REPO).as_posix() for p in files]
    assert len(card_files.cards_in(rels)) == len(files)


def test_names_has_a_constant_for_every_deck_card_and_card_file() -> None:
    constants = card_files.constants()
    assert constants["JACE_VRYNS_PRODIGY"] == "Jace, Vryn's Prodigy"
    assert constants["FOREST"] == "Forest"
    assert set(card_files.implemented()) <= set(constants.values())
