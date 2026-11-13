"""`card new` and `card check`, on a small fake store and card tree, with every command they
would run replaced: nothing is built and no oracle runs."""

import argparse
import json
import subprocess
from pathlib import Path

import pytest

from beryl_tools import card, card_check, card_new, card_store


def record(name: str, *faces: dict[str, str]) -> card_store.Json:
    return {
        "name": name,
        "oracle_id": name,
        "scryfall_id": name,
        "first_set": "lea",
        "layout": "split" if len(faces) > 1 else "normal",
        "faces": list(faces),
    }


def instant(name: str, text: str) -> dict[str, str]:
    return {
        "name": name,
        "mana_cost": "{U}",
        "type_line": "Instant",
        "oracle_text": text,
    }


COUNTERSPELL = "pub static CARD: CardDef = card(names::COUNTERSPELL, &[]);\n"


@pytest.fixture
def sources(tmp_path: Path) -> card.Sources:
    """A store of five cards in Alpha, whose set crate has one card file, Counterspell's."""
    cards = [
        record("Counterspell", instant("Counterspell", "Counter target spell.")),
        record(
            "Mana Leak", instant("Mana Leak", "Counter target spell unless it pays.")
        ),
        record("Ancestral Recall", instant("Ancestral Recall", "Copy target spell.")),
        record(
            "Grizzly Bears",
            {
                "name": "Grizzly Bears",
                "mana_cost": "{1}{G}",
                "type_line": "Creature — Bear",
                "power": "2",
                "toughness": "2",
            },
        ),
        record(
            "Fire // Ice",
            instant("Fire", "Fire deals 2 damage divided as you choose."),
            instant("Ice", "Tap target permanent.\nDraw a card."),
        ),
    ]
    store = tmp_path / "store.tsv"
    card_store.write(store, [card_store.line("card", str(c["name"]), c) for c in cards])
    crate = tmp_path / "cards" / "cards_1993"
    (crate / "lea").mkdir(parents=True)
    (crate / "Cargo.toml").write_text("[package]\n")
    (crate / "lea" / "counterspell.rs").write_text(COUNTERSPELL)
    return card.Sources(store, tmp_path / "vocabulary", tmp_path / "cards")


@pytest.fixture
def no_rustfmt(monkeypatch: pytest.MonkeyPatch) -> list[list[str]]:
    ran: list[list[str]] = []

    def fake(args: list[str], **_: object) -> subprocess.CompletedProcess[str]:
        ran.append(args)
        return subprocess.CompletedProcess[str](args, 0, "", "")

    monkeypatch.setattr(card_new.subprocess, "run", fake)
    return ran


def new(name: str, sources: card.Sources) -> int:
    return card_new.run(argparse.Namespace(name=name), sources)


def test_new_writes_the_header_and_the_smallest_definition(
    sources: card.Sources,
    no_rustfmt: list[list[str]],
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert new("ancestral recall", sources) == 0
    path = sources.cards / "cards_1993" / "lea" / "ancestral_recall.rs"
    text = path.read_text()
    assert text.startswith("// Generated from the card store")
    assert "//! Copy target spell.\n// End of the generated card header.\n" in text
    assert "pub static CARD: CardDef = Printed::AncestralRecall.card();" in text
    assert "pub const SCENARIOS: &[Scenario] = &[];" in text
    assert no_rustfmt == [["rustfmt", "--edition", "2024", str(path)]]
    out = capsys.readouterr().out
    assert "ancestral_recall.rs" in out
    # The nearest instant with a file, to read as a model.
    assert "The nearest Instant implemented: Counterspell" in out
    assert '`just card brief "Ancestral Recall"`, then `just card check' in out


def test_new_gives_each_face_its_printed_constant(
    sources: card.Sources, no_rustfmt: list[list[str]]
) -> None:
    assert new("Fire // Ice", sources) == 0
    text = (sources.cards / "cards_1993" / "lea" / "fire_ice.rs").read_text()
    assert "card(\n    Card::FireIce,\n    &[Printed::Fire, Printed::Ice],\n)" in text


@pytest.mark.parametrize(
    ("name", "said"),
    [
        ("Counterspell", "Counterspell already has a file"),
        ("Grizzly Bears", "Grizzly Bears is complete from the store"),
        ("Counterspel", "no card named Counterspel in the card store; closest"),
    ],
)
def test_new_refuses_a_card_with_a_file_one_complete_from_the_store_and_a_stranger(
    sources: card.Sources,
    no_rustfmt: list[list[str]],
    capsys: pytest.CaptureFixture[str],
    name: str,
    said: str,
) -> None:
    assert new(name, sources) == 1
    assert said in capsys.readouterr().err
    assert (sources.cards / "cards_1993" / "lea" / "counterspell.rs").read_text() == (
        COUNTERSPELL
    )
    assert sorted(p.name for p in (sources.cards / "cards_1993" / "lea").iterdir()) == [
        "counterspell.rs"
    ]
    assert no_rustfmt == []


ORACLE_SUMMARY: dict[str, object] = {
    "xmage": {
        "pass": 2,
        "fail": 0,
        "allowlisted": 0,
        "unsupported": 1,
        "scenarios": [{"status": "unsupported", "reason": "no exporter for X"}],
    },
    "forge": {
        "pass": 3,
        "fail": 0,
        "allowlisted": 0,
        "unsupported": 0,
        "scenarios": [],
    },
    "report": "oracle.json",
}


class Commands:
    """Stands in for `card_check.command`: records each command and answers it, failing the
    one whose words include `fail_on`. It renders each card as `rendered` says, else as its
    Oracle text in the fake store."""

    def __init__(
        self, fail_on: str | None = None, rendered: dict[str, str] | None = None
    ) -> None:
        self.ran: list[list[str]] = []
        self.fail_on = fail_on
        self.rendered = {"Counterspell": "Counter target spell.", "Grizzly Bears": ""}
        self.rendered.update(rendered or {})

    def __call__(
        self, args: list[str], cwd: Path = Path(".")
    ) -> subprocess.CompletedProcess[str]:
        self.ran.append(args)
        failing = self.fail_on is not None and self.fail_on in args
        if "render" in args:
            name = args[-1]
            face = {"name": name, "text": self.rendered[name]}
            out = json.dumps({"name": name, "faces": [face]}) + "\n"
        elif "beryl_scenario" in args:
            out = "pass hand-written: one\n1 hand-written, 2 generated, 0 failed\n"
            if failing:
                out = "fail hand-written: one: action 2: wrong\n1 hand-written, 0 failed\n"
        elif "oracle" in args:
            out = json.dumps(ORACLE_SUMMARY, indent=2) + "\n"
        elif "test" in args:
            out = "test result: ok. 2 passed; 0 failed\n"
        else:
            out = ""
        err = "error[E0425]: cannot find value `X`\nmore\n" if failing else ""
        return subprocess.CompletedProcess(args, 1 if failing else 0, out, err)

    def steps(self) -> list[str]:
        """Which step each command was: its cargo subcommand, `oracle` or `render`."""
        return [
            next((w for w in ("oracle", "render") if w in a), a[1]) for a in self.ran
        ]


def check(
    name: str,
    sources: card.Sources,
    monkeypatch: pytest.MonkeyPatch,
    commands: Commands,
    no_oracles: bool = False,
) -> int:
    monkeypatch.setattr(card_check, "command", commands)
    return card_check.run(argparse.Namespace(name=name, no_oracles=no_oracles), sources)


def test_check_runs_each_step_in_order_on_the_cards_crate(
    sources: card.Sources,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    commands = Commands()
    assert check("Counterspell", sources, monkeypatch, commands) == 0
    assert commands.steps() == ["check", "test", "run", "oracle", "render"]
    build, tests, beryl, oracle, render = commands.ran
    assert build[build.index("-p") + 1] == "cards_1993"
    assert tests[-2:] == ["--", "lea::counterspell::"]
    assert beryl[-3:] == ["run", "--card", "Counterspell"]
    assert oracle[-3:] == ["oracle", "Counterspell", "--no-sim"]
    assert render[-3:] == ["render", "--json", "Counterspell"]
    assert capsys.readouterr().out.splitlines() == [
        "build: pass: cargo check -p cards_1993",
        "tests: pass: 2 passed",
        "beryl: pass: 1 hand-written, 2 generated, 0 failed",
        "oracles: pass: xmage: 2 pass, 1 unsupported (no exporter for X); forge: 3 pass",
        "render: pass: matches its Oracle text",
        "card check: Counterspell passed",
    ]


@pytest.mark.parametrize(
    ("fail_on", "ran", "said"),
    [
        ("check", ["check"], "build: fail: error[E0425]: cannot find value `X`"),
        (
            "run",
            ["check", "test", "run"],
            "beryl: fail: fail hand-written: one: action 2",
        ),
        ("oracle", ["check", "test", "run", "oracle"], "oracles: fail: xmage: 2 pass"),
    ],
)
def test_check_stops_at_the_first_failure_and_exits_nonzero(
    sources: card.Sources,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    fail_on: str,
    ran: list[str],
    said: str,
) -> None:
    commands = Commands(fail_on)
    assert check("Counterspell", sources, monkeypatch, commands) == 1
    assert commands.steps() == ran
    out = capsys.readouterr().out
    assert said in out
    assert "render:" not in out
    assert out.rstrip().endswith(f"failed at {said.split(':')[0]}")


def test_no_oracles_skips_xmage_and_forge(
    sources: card.Sources,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    commands = Commands()
    assert check("Counterspell", sources, monkeypatch, commands, no_oracles=True) == 0
    assert commands.steps() == ["check", "test", "run", "render"]
    assert "oracles: skip: --no-oracles" in capsys.readouterr().out


def test_a_card_complete_from_the_store_has_no_tests_of_its_own(
    sources: card.Sources,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    commands = Commands()
    assert check("Grizzly Bears", sources, monkeypatch, commands, no_oracles=True) == 0
    assert commands.steps() == ["check", "run", "render"]
    assert "tests: skip: no file" in capsys.readouterr().out


def test_a_card_file_with_no_scenario_of_its_own_fails(
    sources: card.Sources,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    class Generated(Commands):
        def __call__(
            self, args: list[str], cwd: Path = Path(".")
        ) -> subprocess.CompletedProcess[str]:
            if "beryl_scenario" not in args:
                return super().__call__(args, cwd)
            self.ran.append(args)
            out = "pass generated: one\n0 hand-written, 1 generated, 0 failed\n"
            return subprocess.CompletedProcess(args, 0, out, "")

    assert check("Counterspell", sources, monkeypatch, Generated(), True) == 1
    assert "beryl: fail: its file has no scenario" in capsys.readouterr().out


def test_a_rendering_that_differs_from_the_oracle_text_fails_with_the_diff(
    sources: card.Sources,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    commands = Commands(rendered={"Counterspell": "Counter target creature spell."})
    assert check("Counterspell", sources, monkeypatch, commands, no_oracles=True) == 1
    out = capsys.readouterr().out.splitlines()
    assert out[-3:] == [
        "render: fail: 1 paragraph differs from its Oracle text",
        "Counterspell: counter target {+creature+} spell .",
        "card check: Counterspell failed at render",
    ]


def test_check_refuses_a_card_with_no_file_and_a_stranger(
    sources: card.Sources,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    commands = Commands()
    assert check("Mana Leak", sources, monkeypatch, commands) == 1
    assert '`just card new "Mana Leak"`' in capsys.readouterr().err
    assert check("Mana Lek", sources, monkeypatch, commands) == 1
    assert "no card named Mana Lek" in capsys.readouterr().err
    assert commands.ran == []


def test_the_commands_are_registered() -> None:
    parser = argparse.ArgumentParser()
    card.register(parser.add_subparsers())
    args = parser.parse_args(["card", "check", "Counterspell", "--no-oracles"])
    assert (args.func, args.name, args.no_oracles) == (
        card_check.run,
        "Counterspell",
        True,
    )
    assert parser.parse_args(["card", "new", "Ancestral Recall"]).func == card_new.run
