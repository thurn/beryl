"""The tools select the ticket backend for the Git checkout family."""

import subprocess
from pathlib import Path

import pytest

from beryl_tools import tickets


def test_detect_uses_main_enrollment_from_an_old_worktree(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    main: Path = tmp_path / "main"
    main.mkdir()
    old = tmp_path / "old"
    config = main / ".enact.toml"
    config.write_text('# [tickets]\n# project = "by"\n')

    def git(*args: str) -> None:
        subprocess.run(["git", *args], cwd=main, check=True, capture_output=True)

    git("init")
    git("add", ".enact.toml")
    git(
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        "Dormant",
    )
    git("worktree", "add", "--detach", str(old))

    helper = tmp_path / "enact" / "scripts" / "enact.py"
    helper.parent.mkdir(parents=True)
    helper.write_text(
        "import os\n"
        "import sys\n"
        f"assert os.path.samefile(os.getcwd(), {str(old)!r})\n"
        "assert sys.argv[1:] == ['backend']\n"
        'print(\'{"backend": "server", "tk": "/fixture/tk"}\')\n'
    )
    monkeypatch.setattr(Path, "home", lambda: tmp_path)
    with pytest.raises(tickets.TicketError):
        tickets.detect(old)
    config.write_text('[tickets]\nproject = "by"\nurl = "http://localhost:8888"\n')
    assert tickets.detect(old) == "/fixture/tk"
