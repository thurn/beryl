"""`just b3d`'s cargo commands and the text commands it hands to the `b3d` program."""

import os
import subprocess
from collections.abc import Iterator
from pathlib import Path

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
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    assert cli.main(["tree", "verbs-zones", "--at", "1500ms", "--help"]) == 0
    build, run = ran
    assert build == ["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"]
    assert run == ["/linked/b3d", "tree", "verbs-zones", "--at", "1500ms", "--help"]


def test_a_missing_pack_is_recorded_and_the_command_run_again(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[list[str]] = []
    monkeypatch.setattr(cli, "cargo", lambda arguments: ran.append(arguments) or 0)
    codes: Iterator[int] = iter([cli.MISSING_PACK, 0, 0])

    def program(
        command: list[str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        return subprocess.CompletedProcess(command, next(codes))

    monkeypatch.setattr(cli.subprocess, "run", program)
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    monkeypatch.setattr(cli, "program", lambda name: Path("/built") / name)
    assert cli.main(["list"]) == 0
    assert ran == [
        ["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"],
        ["/linked/b3d", "list"],
        cli.SHARED_BUILD,
        ["/built/record"],
        ["/linked/b3d", "list"],
    ]


def test_the_linkers_newest_binary_is_run(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(cli.paths, "WORKSPACE", tmp_path)
    debug = tmp_path / "target" / "debug"
    deps = debug / "deps"
    deps.mkdir(parents=True)
    assert cli.linked("b3d") == debug / "b3d"
    old, new = deps / "b3d-0000000000000000", deps / "b3d-1111111111111111"
    for path in (old, new, deps / "b3d-1111111111111111.d"):
        path.write_text("")
    os.utime(old, (1, 1))
    os.utime(new, (2, 2))
    assert cli.linked("b3d") == new
    render = deps / "b3d_render-2222222222222222"
    render.write_text("")
    assert cli.linked("b3d-render") == render


def test_a_newer_test_harness_of_the_crate_is_not_run_as_the_program(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(cli.paths, "WORKSPACE", tmp_path)
    debug = tmp_path / "target" / "debug"
    deps = debug / "deps"
    deps.mkdir(parents=True)
    program, harness = deps / "b3d-0000000000000000", deps / "b3d-1111111111111111"
    for path in (program, harness):
        path.write_text("")
    os.utime(program, (1, 1))
    os.utime(harness, (2, 2))
    for hash_, kind in (("0000000000000000", "bin"), ("1111111111111111", "test-bin")):
        (debug / ".fingerprint" / f"beryl_inspect-{hash_}").mkdir(parents=True)
        (debug / ".fingerprint" / f"beryl_inspect-{hash_}" / f"{kind}-b3d").write_text(
            ""
        )
    assert cli.linked("b3d") == program


def test_the_listing_names_every_text_command() -> None:
    listed = cli.listing()
    for name in ("list", "info", "tree", "diff", "check", "test", "png", "film"):
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
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    assert cli.main(["png", "verbs-zones", "--at", "1500ms", "--tree"]) == 0
    build, run = ran
    assert build == ["build", "-q", "-p", "beryl_pictures", "--bin", "b3d-render"]
    assert run == [
        "/linked/b3d-render",
        "png",
        "verbs-zones",
        "--at",
        "1500ms",
        "--tree",
    ]
