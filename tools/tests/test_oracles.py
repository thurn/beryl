import argparse
import json
import subprocess
from pathlib import Path

import pytest

from beryl_tools import oracles


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=True
    ).stdout.strip()


def upstream(path: Path) -> str:
    """A repository with two commits; returns the first, which the oracle pins."""
    path.mkdir()
    git(path, "init", "--quiet")
    for text in ("pinned", "newer"):
        (path / "README").write_text(text)
        git(path, "add", "README")
        git(path, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", text)
    return git(path, "rev-parse", "HEAD~1")


def test_setup_fetches_and_builds_once(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    pinned = upstream(tmp_path / "upstream")
    # The build stands in for Maven: it counts its runs in the checkout.
    oracle = oracles.Oracle(
        name="fake",
        repo=(tmp_path / "upstream").as_uri(),
        commit=pinned,
        build=("sh", "-c", "echo built >> builds"),
    )
    monkeypatch.setattr(oracles, "ORACLES", (oracle,))
    monkeypatch.setattr(oracles, "mvn", lambda *args: list(args))
    monkeypatch.setattr(oracles, "brew_prefix", lambda formula: tmp_path / formula)
    monkeypatch.setattr(oracles, "configure_forge", lambda: None)

    def summary() -> dict[str, object]:
        assert oracles.setup(argparse.Namespace()) == 0
        return json.loads(capsys.readouterr().out)["fake"]

    first = summary()
    assert (first["fetched"], first["built"]) == (True, True)
    checkout = oracles.checkout(oracle)
    assert git(checkout, "rev-parse", "HEAD") == pinned
    assert (checkout / "README").read_text() == "pinned"

    second = summary()
    assert (second["fetched"], second["built"]) == (False, False)
    assert (checkout / "builds").read_text() == "built\n"
