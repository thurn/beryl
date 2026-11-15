"""`just b3d`'s cargo commands and the text commands it hands to the `b3d` program."""

import subprocess

import pytest

from beryl_tools.beryl3d import cli


def test_tests_run_the_workspace_or_the_named_crates() -> None:
    assert cli.test_command([], [], nextest=False) == ["test", "-q", "--workspace"]
    plain = cli.test_command(["a", "b"], ["pose"], nextest=False)
    assert plain == ["test", "-q", "-p", "a", "-p", "b", "pose"]
    fast = cli.test_command(["a"], [], nextest=True)
    assert fast[:2] == ["nextest", "run"] and "--no-tests=pass" in fast
    assert fast[-2:] == ["-p", "a"]


def test_text_commands_hand_every_argument_to_the_b3d_program(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[list[str]] = []
    monkeypatch.setattr(cli, "cargo", lambda arguments: ran.append(arguments) or 0)

    def program(
        command: list[str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(cli.subprocess, "run", program)
    assert cli.main(["tree", "verbs-zones", "--at", "1500ms", "--help"]) == 0
    build, run = ran
    assert build == ["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"]
    assert run[0].endswith("target/debug/b3d")
    assert run[1:] == ["tree", "verbs-zones", "--at", "1500ms", "--help"]


def test_the_listing_names_every_text_command() -> None:
    listed = cli.listing()
    for name in ("list", "info", "tree", "diff", "check", "png", "film"):
        assert f"\n  {name} " in listed


def test_picture_commands_hand_every_argument_to_the_b3d_render_program(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[list[str]] = []
    monkeypatch.setattr(cli, "cargo", lambda arguments: ran.append(arguments) or 0)

    def program(
        command: list[str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(cli.subprocess, "run", program)
    assert cli.main(["png", "verbs-zones", "--at", "1500ms", "--tree"]) == 0
    build, run = ran
    assert build[:5] == ["build", "-q", "-p", "beryl_3d", "--features"]
    assert build[-2:] == ["--bin", "b3d-render"]
    assert run[0].endswith("target/debug/b3d-render")
    assert run[1:] == ["png", "verbs-zones", "--at", "1500ms", "--tree"]
