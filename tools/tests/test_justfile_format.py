import subprocess
from pathlib import Path

import pytest

from beryl_tools import justfile_format
from beryl_tools.reruns import CANNOT_RUN


def test_the_version_is_read_from_just_version() -> None:
    assert justfile_format.version("just 1.59.0\n") == (1, 59)
    assert justfile_format.version("just 1.9.2") == (1, 9)
    assert justfile_format.version("") is None


def test_a_just_too_old_to_format_the_justfile_cannot_run_the_check(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    calls: list[list[str]] = []

    def run(cmd: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        calls.append(cmd)
        said = ["just 1.40.0\n"]
        return subprocess.CompletedProcess(cmd, 0, said[0], "")

    monkeypatch.setattr(justfile_format.subprocess, "run", run)
    assert justfile_format.step() == CANNOT_RUN
    # It never asks the old formatter, whose output would read as a badly formatted file.
    assert calls == [["just", "--version"]]
    assert "cargo install just --locked" in capsys.readouterr().out
