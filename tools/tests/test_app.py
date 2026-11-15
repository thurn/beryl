"""The desktop launcher parses table-pair arguments and refuses agents before building."""

from pathlib import Path

import pytest

from beryl_tools import app, cli


def test_table_pair_routes_through_the_cli(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(app, "table_pair", lambda _: 42)
    assert cli.main(["table-pair"]) == 42
    with pytest.raises(SystemExit, match="2"):
        cli.main(["table-pair", "--window"])


def test_table_pair_refuses_agents(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setenv("CODEX_THREAD_ID", "test")
    monkeypatch.setattr(
        app, "launch_pair", lambda *_: pytest.fail("launched two windows")
    )
    assert cli.main(["table-pair"]) == 2
    assert "agents never open a window" in capsys.readouterr().err


def test_table_pair_builds_once_and_launches_distinct_profiles(
    monkeypatch: pytest.MonkeyPatch, own_cache: Path
) -> None:
    for marker in app.paths.AGENT_ENV:
        monkeypatch.delenv(marker, raising=False)
    monkeypatch.setenv("BERYL_SMOKE", "60")
    mailbox = own_cache.resolve() / "table-pair"
    mailbox.mkdir()
    (mailbox / "invite").write_text("stale invite")
    (mailbox / "guest-ready").touch()
    calls: list[tuple[str, ...]] = []

    def build(*args: str) -> int:
        calls.append(args)
        return 0

    class Process:
        def __init__(self, command: list[str], *, env: dict[str, str], stdin: int):
            assert command == [str(app.BINARY)]
            assert "BERYL_SMOKE" not in env
            assert env["BERYL_CACHE"] == str(own_cache.resolve())
            assert not (mailbox / "invite").exists()
            assert not (mailbox / "guest-ready").exists()
            calls.append((env["BERYL_PROFILE"], env["BERYL_TABLE_PAIR"]))

        def poll(self) -> int:
            return 0

        def wait(self) -> int:
            return 0

    monkeypatch.setattr(app, "tauri", build)
    monkeypatch.setattr(app.subprocess, "Popen", Process)
    assert cli.main(["table-pair"]) == 0
    assert calls == [
        ("build", "--debug", "--no-bundle", "--features", "iroh"),
        ("host", "host"),
        ("guest", "guest"),
    ]
