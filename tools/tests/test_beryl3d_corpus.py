"""A corpus sweep reports the recordings it cannot cover and resumes completed parts."""

import json
import subprocess
from pathlib import Path

import pytest

from beryl_tools.beryl3d import corpus, pack
from beryl_tools.ignite import beryl3d as adapter
from .test_beryl3d_pack import encode


def test_sweep_fails_when_a_pack_has_an_unrepresented_signature(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    baseline_pack = tmp_path / "baseline.bin"
    baseline_pack.write_bytes(encode([("opening", 0, "[]")]))
    card_pack = tmp_path / "part.bin"
    card_pack.write_bytes(encode([("card/cast", 0, '[{"shape":"cast"}]')]))
    part = corpus.Part("cards_test", "test-inputs")
    inspected: list[Path] = []

    def inspect(
        _binary: Path, selected: Path, operation: str, baseline: str
    ) -> subprocess.CompletedProcess[str]:
        inspected.append(selected)
        entries = pack.read(selected.read_bytes())
        if operation == "--snapshot":
            Path(baseline).write_text(entries[0].messages())
            return subprocess.CompletedProcess[str]([], 0, "", "")
        signatures = Path(baseline).read_text()
        missing = [entry.name for entry in entries if entry.messages() != signatures]
        return subprocess.CompletedProcess[str](
            [],
            int(bool(missing)),
            json.dumps({"missing": missing, "faults": [], "tests": 2}),
            "",
        )

    monkeypatch.setattr(corpus.b3d_paths, "cache", lambda: tmp_path)
    monkeypatch.setattr(corpus, "parts", lambda _names: [part])
    monkeypatch.setattr(corpus.card_files, "implemented", lambda: {})
    monkeypatch.setattr(corpus, "inspect", inspect)
    monkeypatch.setattr(
        pack,
        "record",
        lambda _binary, *selection: card_pack if selection else baseline_pack,
    )
    args = ["--sweep", "--crate", part.crate]
    assert corpus.run(args, Path("b3d"), Path("record")) == 1
    printed = capsys.readouterr()
    assert "cards_test: 2 tests" in printed.out
    assert "card/cast" in printed.err
    assert "just b3d corpus --update" in printed.err
    assert corpus.run(args, Path("b3d"), Path("record")) == 1
    assert inspected == [baseline_pack, card_pack]


def test_exploration_lists_one_check_per_crate(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(corpus, "crates", lambda: ["cards_a", "cards_b"])
    monkeypatch.setattr(
        corpus, "parts", lambda names: [corpus.Part(n, f"hash-{n}") for n in names]
    )
    assert adapter.main(["b3d-corpus", "list"]) == 0
    assert [json.loads(line) for line in capsys.readouterr().out.splitlines()] == [
        {"check": "b3d-corpus:cards_a"},
        {"check": "b3d-corpus:cards_b"},
    ]
    assert adapter.main(["b3d-corpus", "next"]) == 0
    assert [json.loads(line) for line in capsys.readouterr().out.splitlines()] == [
        {"check": "b3d-corpus:cards_a", "digest": "hash-cards_a"},
        {"check": "b3d-corpus:cards_b", "digest": "hash-cards_b"},
    ]
