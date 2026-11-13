"""`card brief`: the packet a card is written from, read from a small fake store, vocabulary,
card crate, rulings file and XMage checkout, with no network and no real cache. The last test
reads the real vocabulary, so the scale lane runs this file on a vocabulary change."""

import argparse
import gzip
import json
import subprocess
import sys
import urllib.error
from pathlib import Path

import pytest

from beryl_tools import card, card_store, oracles, scryfall, vocabulary, xmage_classes

BOLT_ID = "bolt-oracle"
COUNTERSPELL_ID = "counterspell-oracle"


def record(name: str, oracle_id: str, *faces: dict[str, str]) -> card_store.Json:
    return {
        "name": name,
        "oracle_id": oracle_id,
        "scryfall_id": name,
        "first_set": "lea",
        "layout": "split" if len(faces) > 1 else "normal",
        "faces": list(faces),
    }


def write(path: Path, text: str) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    return path


VOCABULARY = {
    "effect/mod.rs": """//! Effects.

/// One instruction.
#[derive(Clone, Copy)]
pub enum Effect {
    /// "Counter [spell]"
    /// ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)): "To counter a spell
    /// or ability means to cancel it."
    Counter(Objects),
    /// "[Source] deals [amount] damage to [targets]".
    Damage {
        /// How much damage is dealt.
        amount: u8,
        targets: Objects,
    },
    /// "[Player] gains [amount] life"
    GainLife { amount: u8 },
    Undocumented,
}

impl Effect {
    /// No instruction at all.
    pub const NOTHING: Effect = Effect::Sequence(&[]);

    /// Whether it does nothing: "{" and "}" in a string are not braces.
    pub const fn is_nothing(self) -> bool {
        let _ = "{";
        matches!(self, Effect::Sequence(s) if s.is_empty())
    }
}
""",
    "effect/build.rs": """//! Builders.

/// [`Effect::Counter`]: "counter [spell]".
pub const fn counter(objects: Objects) -> Effect {
    Effect::Counter(objects)
}

/// "Target spell": the first target.
pub const TARGET: Objects = target(0);

fn private_helper() {}
""",
}

CARD_FILES = {
    "cards_1993/lea/counterspell.rs": """pub static CARD: CardDef = CardDef {
    body: Behavior::Data(&counter(TARGET)),
};
pub const SCENARIOS: &[Scenario] = &[Effect::GainLife];
""",
    "cards_1993/lea/mana_leak.rs": """pub static CARD: CardDef = CardDef {
    // Effect::GainLife is only named in a comment.
    body: Behavior::Data(&unless(counter(TARGET))),
};
""",
    "cards_1993/lea/healing_salve.rs": """pub static CARD: CardDef = CardDef {
    body: Behavior::Data(&Effect::GainLife { amount: 3 }),
};
""",
}


@pytest.fixture
def sources(tmp_path: Path) -> card.Sources:
    """A store of five cards, of which three are implemented, a vocabulary of a few entries."""
    cards = [
        record(
            "Counterspell",
            COUNTERSPELL_ID,
            {
                "name": "Counterspell",
                "mana_cost": "{U}{U}",
                "type_line": "Instant",
                "oracle_text": "Counter target spell.",
            },
        ),
        record(
            "Mana Leak",
            "leak-oracle",
            {
                "name": "Mana Leak",
                "mana_cost": "{1}{U}",
                "type_line": "Instant",
                "oracle_text": "Counter target spell unless its controller pays {3}.",
            },
        ),
        record(
            "Healing Salve",
            "salve-oracle",
            {
                "name": "Healing Salve",
                "mana_cost": "{W}",
                "type_line": "Instant",
                "oracle_text": "Target player gains 3 life.",
            },
        ),
        record(
            "Lightning Bolt",
            BOLT_ID,
            {
                "name": "Lightning Bolt",
                "mana_cost": "{R}",
                "type_line": "Instant",
                "oracle_text": "Lightning Bolt deals 3 damage to any target.",
            },
        ),
        record(
            "Fire // Ice",
            "fire-ice-oracle",
            {
                "name": "Fire",
                "mana_cost": "{1}{R}",
                "type_line": "Instant",
                "oracle_text": "Fire deals 2 damage divided as you choose.",
            },
            {
                "name": "Ice",
                "mana_cost": "{1}{U}",
                "type_line": "Instant",
                "oracle_text": "Tap target permanent.\nDraw a card.",
            },
        ),
    ]
    lines = [card_store.line("card", str(c["name"]), c) for c in cards]
    for face in ("Fire", "Ice"):
        alias: card_store.Json = {"alias": "Fire // Ice", "scryfall_id": "Fire // Ice"}
        lines.append(card_store.line("card", face, alias))
    store = tmp_path / "store.tsv"
    card_store.write(store, lines)
    for path, text in VOCABULARY.items():
        write(tmp_path / "vocabulary" / path, text)
    for path, text in CARD_FILES.items():
        write(tmp_path / "cards" / path, text)
    return card.Sources(store, tmp_path / "vocabulary", tmp_path / "cards")


@pytest.fixture
def cached(own_cache: Path) -> Path:
    """A rulings file and an XMage checkout in the test's own cache; Forge's beside it."""
    rulings = [
        {
            "oracle_id": COUNTERSPELL_ID,
            "published_at": "2020-01-02",
            "comment": "Later.",
        },
        {
            "oracle_id": "other",
            "published_at": "2004-10-04",
            "comment": "Not this card.",
        },
        {
            "oracle_id": COUNTERSPELL_ID,
            "published_at": "2004-10-04",
            "comment": "Earlier.",
        },
    ]
    folder = own_cache / "scryfall"
    folder.mkdir(parents=True)
    with gzip.open(folder / "rulings.jsonl.gz", "wt") as out:
        out.writelines(json.dumps(r) + "\n" for r in rulings)
    mage = xmage_classes.checkout() / "Mage.Sets" / "src" / "mage"
    write(mage / "cards" / "c" / "Counterspell.java", "class Counterspell {}\n")
    # A class whose name is not its card's: only the set file says which it is.
    write(mage / "cards" / "l" / "BoltOfLightning.java", "class BoltOfLightning {}\n")
    write(
        mage / "sets" / "Alpha.java",
        'cards.add(new SetCardInfo("Lightning Bolt", 161, Rarity.COMMON, '
        "mage.cards.l.BoltOfLightning.class));\n",
    )
    forge = oracles.checkout(oracles.FORGE)
    write(
        forge / "res" / "cardsfolder" / "c" / "counterspell.txt", "Name:Counterspell\n"
    )
    return own_cache


def sections(text: str) -> list[str]:
    return [line[3:] for line in text.splitlines() if line.startswith("## ")]


def brief_of(name: str, sources: card.Sources) -> str:
    with card_store.Store(sources.store) as store:
        found = card.find(name, store)
    assert not isinstance(found, str), found
    return card.brief(found, sources)


def part(text: str, title: str) -> str:
    return text.split(f"## {title}\n", 1)[1].split("\n## ", 1)[0]


def test_an_implemented_cards_brief_has_all_five_parts(
    sources: card.Sources, cached: Path
) -> None:
    text = brief_of("Counterspell", sources)
    assert sections(text) == [
        "Oracle text",
        "Rulings",
        "Vocabulary",
        "Nearest implemented cards",
        "XMage class",
    ]
    assert "Counterspell {U}{U}\nInstant\nCounter target spell." in part(
        text, "Oracle text"
    )
    assert (
        part(text, "Rulings").split()
        == "- 2004-10-04: Earlier. - 2020-01-02: Later.".split()
    )
    vocabulary_part = part(text, "Vocabulary")
    assert vocabulary_part.index("### Effect::Counter (") < vocabulary_part.index(
        "### build::counter ("
    )
    assert (
        "effect/mod.rs:9)" in vocabulary_part and '"Counter [spell]"' in vocabulary_part
    )
    nearest_part = part(text, "Nearest implemented cards")
    leak = (sources.cards / "cards_1993" / "lea" / "mana_leak.rs").as_posix()
    # Its uses come from its definition: the comment and the scenarios name no entry.
    assert f"- Mana Leak: {leak}\n  uses build::counter, build::TARGET" in nearest_part
    assert "Counterspell:" not in nearest_part
    xmage_part = part(text, "XMage class")
    assert "cards/c/Counterspell.java" in xmage_part
    assert "class Counterspell {}" in xmage_part


def test_an_unimplemented_card_gets_a_brief_from_its_record(
    sources: card.Sources, cached: Path
) -> None:
    text = brief_of("Lightning Bolt", sources)
    assert "Lightning Bolt deals 3 damage to any target." in text
    assert "Scryfall has none." in part(text, "Rulings")
    assert part(text, "Vocabulary").lstrip().startswith("### Effect::Damage (")
    assert "class BoltOfLightning {}" in part(text, "XMage class")


def test_missing_rulings_and_checkout_are_said_and_the_brief_goes_on(
    sources: card.Sources, monkeypatch: pytest.MonkeyPatch
) -> None:
    def offline(url: str) -> scryfall.Json:
        raise urllib.error.URLError("offline")

    monkeypatch.setattr(scryfall, "http_get", offline)
    text = brief_of("Counterspell", sources)
    assert "not cached and could not be fetched" in part(text, "Rulings")
    assert "Mana Leak" in part(text, "Nearest implemented cards")
    assert "no XMage checkout" in part(text, "XMage class")
    assert "oracle-setup" in part(text, "XMage class")


def test_a_card_is_found_by_slug_face_or_case(sources: card.Sources) -> None:
    with card_store.Store(sources.store) as store:
        for name in ("counterspell", "COUNTERSPELL", "mana-leak", "Ice", "fire-ice"):
            found = card.find(name, store)
            assert not isinstance(found, str), name
        assert card.find("ice", store) != card.find("Counterspell", store)


def test_an_unknown_name_exits_nonzero_naming_the_closest(
    sources: card.Sources, capsys: pytest.CaptureFixture[str]
) -> None:
    code = card.run_brief(argparse.Namespace(name="Counterspel"), sources)
    assert code == 1
    captured = capsys.readouterr()
    assert captured.out == ""
    assert "no card named Counterspel" in captured.err
    assert "closest: Counterspell" in captured.err


def test_path_prints_a_cards_file_or_says_it_has_none(
    sources: card.Sources, capsys: pytest.CaptureFixture[str]
) -> None:
    assert card.run_path(argparse.Namespace(name="mana leak"), sources) == 0
    printed = capsys.readouterr().out.strip()
    assert printed == (sources.cards / "cards_1993/lea/mana_leak.rs").as_posix()
    assert card.run_path(argparse.Namespace(name="Fire // Ice"), sources) == 1
    captured = capsys.readouterr()
    assert captured.out == ""
    assert "Fire // Ice is a record in the card store with no file" in captured.err
    assert "cards_1993/lea/fire_ice.rs" in captured.err


READS: list[str] = []
WATCHING: list[bool] = []


def watch(event: str, args: tuple[object, ...]) -> None:
    if WATCHING and event in ("open", "os.listdir", "os.scandir") and args:
        READS.append(str(args[0]))


def test_a_brief_reads_nothing_of_forges_checkout(
    sources: card.Sources, cached: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    # K16: no brief is derived from a Forge script. An audit hook sees every file the process
    # opens and every folder it lists, however it does so; hooks cannot be removed, so this one
    # records only while the brief runs.
    sys.addaudithook(watch)
    WATCHING.append(True)
    try:
        assert card.run_brief(argparse.Namespace(name="Counterspell"), sources) == 0
    finally:
        WATCHING.clear()
    assert "Counterspell.java" in capsys.readouterr().out
    forge = str(oracles.checkout(oracles.FORGE).resolve())
    assert any("Counterspell.java" in r for r in READS)
    assert [r for r in READS if str(Path(r).resolve()).startswith(forge)] == []


def test_the_module_does_not_import_forge() -> None:
    loaded = subprocess.run(
        [
            sys.executable,
            "-c",
            "import sys, beryl_tools.card; print('beryl_tools.forge' in sys.modules)",
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    assert loaded.stdout.strip() == "False"


def test_the_xmage_checkout_is_the_oracles() -> None:
    assert xmage_classes.checkout() == oracles.checkout(oracles.XMAGE)


def test_entries_are_documented_variants_functions_and_constants(
    tmp_path: Path,
) -> None:
    for path, text in VOCABULARY.items():
        write(tmp_path / path, text)
    found = {e.name: e for e in vocabulary.entries(tmp_path)}
    # Not the undocumented variant, a struct variant's field, the enum itself or a private
    # function.
    assert sorted(found) == [
        "Effect::Counter",
        "Effect::Damage",
        "Effect::GainLife",
        "Effect::NOTHING",
        "Effect::is_nothing",
        "build::TARGET",
        "build::counter",
    ]
    counter = found["Effect::Counter"]
    assert counter.path == tmp_path / "effect" / "mod.rs" and counter.line == 9
    assert counter.doc.startswith('"Counter [spell]"\n([CR 701.6a]')
    # What the entry says a card says, not the rule it quotes.
    assert "cancel" not in counter.words() and "counter" in counter.words()


def test_the_real_vocabulary_reads() -> None:
    found = {e.name: e for e in vocabulary.entries()}
    for name in ("Effect::Counter", "build::counter", "TargetKind::Spell"):
        entry = found[name]
        assert entry.doc
        lines = entry.path.read_text().splitlines()
        assert (
            vocabulary.VARIANT.match(lines[entry.line - 1])
            or "fn " in lines[entry.line - 1]
        )
