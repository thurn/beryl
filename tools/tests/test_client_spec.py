"""`just client spec`: a card's spec moments from each seat, with the `just client` programs stubbed."""

import json
import subprocess

import pytest

from beryl_tools import specs
from beryl_tools.client import spec

RECORDINGS = [
    {"name": "counterspell-targets", "seat": "you"},
    {"name": "counterspell/a", "seat": "you"},
    {"name": "counterspell/a", "seat": "opponent"},
    {"name": "counterspell/b", "seat": "you"},
]


def stub(monkeypatch: pytest.MonkeyPatch) -> list[list[str]]:
    ran: list[list[str]] = []

    def run(command: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        arguments = command[command.index("client") + 1 :]
        ran.append(arguments)
        text = {
            "list": json.dumps({"recordings": RECORDINGS}),
            "info": 'x: 1 views\nprompt:1  5ms  button "Next"\nview:1  5ms',
            "tree": "\n".join(f"line {n}" for n in range(60)),
            "check": "1 test, 0 faults in 0.1s",
            "film": "wrote /tmp/film.png (12 frames)",
        }[arguments[0]]
        return subprocess.CompletedProcess(command, 0, stdout=text)

    monkeypatch.setattr(spec.subprocess, "run", run)
    monkeypatch.setattr(specs, "resolve", lambda names: [n.lower() for n in names])
    return ran


def test_each_moment_and_seat_runs_info_tree_and_check(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    ran = stub(monkeypatch)
    assert spec.main_for(["Counterspell"]) == 0
    verbs = [r[0] for r in ran]
    assert verbs.count("info") == verbs.count("tree") == verbs.count("check") == 3
    assert ["tree", "counterspell/a", "--seat", "opponent", "--at", "prompt:1"] in ran
    out = capsys.readouterr().out
    assert "... 20 more lines" in out and "counterspell-targets" not in out


def test_a_moment_and_seat_narrow_the_recordings_and_film_draws_them(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    ran = stub(monkeypatch)
    assert spec.main_for(["counterspell", "b", "--seat", "you", "--film"]) == 0
    assert [r for r in ran if r[0] == "film"] == [
        ["film", "counterspell/b", "--seat", "you", "--frames", "12"]
    ]
    assert "wrote /tmp/film.png" in capsys.readouterr().out


def test_a_misspelt_card_exits_2_naming_the_nearest(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    stub(monkeypatch)
    monkeypatch.undo()
    monkeypatch.setattr(specs, "implemented", lambda: {"counterspell"})
    monkeypatch.setattr(specs, "nearest", lambda name, specced: ["Counterspell"])
    assert spec.main_for(["countrspell"]) == 2
    assert "nearest specced: Counterspell" in capsys.readouterr().err
