"""`just b3d`'s cargo commands and the text commands it hands to the `b3d` program."""

import os
import subprocess
from collections.abc import Callable, Iterator
from contextlib import AbstractContextManager
from pathlib import Path

import pytest

from beryl_tools import cargo_pool, tokens
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
    envs: list[dict[str, str]] = []

    def cargo(arguments: list[str], env: dict[str, str] | None = None) -> int:
        ran.append(arguments)
        envs.append(env or {})
        return 0

    monkeypatch.setattr(cli, "cargo", cargo)

    def program(
        command: list[str], env: dict[str, str] | None = None, **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        envs.append(env or {})
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(cli.subprocess, "run", program)
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    assert cli.main(["tree", "verbs-zones", "--at", "1500ms", "--help"]) == 0
    build, run = ran
    assert build == ["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"]
    assert run == ["/linked/b3d", "tree", "verbs-zones", "--at", "1500ms", "--help"]
    # The build starts without waiting for tokens, and the one-thread program runs at once
    # inside a lease of one token it never waited for.
    build_env, run_env = envs
    assert build_env[cargo_pool.FLOOR_ENV] == str(cli.QUICK_JOBS)
    assert run_env[tokens.LEASE_ENV] == "1"


def test_interface_tests_run_the_shared_builds_b3d(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The lane's `test` step builds nothing beyond the shared build, which the `cargo-test`
    and `timeline-parity` lanes also start with, so it never holds the build lock they wait
    on (`cli.run_shared_b3d`)."""
    ran: list[list[str]] = []
    monkeypatch.setattr(
        cli, "cargo", lambda arguments, env=None: ran.append(arguments) or 0
    )

    def program(
        command: list[str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(cli.subprocess, "run", program)
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    monkeypatch.setattr(cli, "program", lambda name: Path("/built") / name)
    monkeypatch.setattr(cli.deadline, "until_hard_limit", lambda: None)
    assert cli.main(["test", "verbs-"]) == 0
    assert ran == [cli.SHARED_BUILD, ["/built/b3d", "test", "verbs-"]]

    # Inside a fast check the tests stop starting `TEST_STOP` seconds before the hard limit,
    # which would otherwise kill the step and report none of them.
    ran.clear()
    monkeypatch.setattr(cli.deadline, "until_hard_limit", lambda: 70.0)
    assert cli.main(["test"]) == 0
    stop = f"{70.0 - cli.TEST_STOP:.1f}"
    assert ran == [cli.SHARED_BUILD, ["/built/b3d", "test", "--stop-after", stop]]


def test_a_missing_pack_is_recorded_and_the_command_run_again(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[list[str]] = []
    monkeypatch.setattr(
        cli, "cargo", lambda arguments, env=None: ran.append(arguments) or 0
    )
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


def test_the_listing_names_every_command_once_and_includes_the_phone_commands() -> None:
    names = [command.name for command in cli.COMMANDS]
    assert len(names) == len(set(names))
    listed = cli.listing()
    for name in names:
        assert f"\n  {name} " in listed
    for name in ("ios-smoke", "android-smoke", "phone-search", "ios-device"):
        assert name in names


def test_picture_commands_hand_every_argument_to_the_b3d_render_program(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[list[str]] = []
    monkeypatch.setattr(
        cli, "cargo", lambda arguments, env=None: ran.append(arguments) or 0
    )

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


def test_a_target_another_step_cloned_while_this_one_waited_is_not_cloned_again(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    workspace: Path = tmp_path / "beryl3d"
    workspace.mkdir()
    (tmp_path / "b3d" / "target").mkdir(parents=True)
    monkeypatch.setattr(cli.paths, "WORKSPACE", workspace)
    monkeypatch.setattr(cli.paths, "cache", lambda: tmp_path / "b3d")
    monkeypatch.setattr(cli.paths, "target_lock", lambda: tmp_path / "lock")
    real_lock: Callable[[Path], AbstractContextManager[None, bool | None]] = (
        cli.system.file_lock
    )

    def lock_after_another_clone(
        path: Path,
    ) -> AbstractContextManager[None, bool | None]:
        # The step that held the lock first finished its clone.
        (workspace / "target").mkdir()
        return real_lock(path)

    monkeypatch.setattr(cli.system, "file_lock", lock_after_another_clone)
    clones: list[Path] = []
    monkeypatch.setattr(
        cli.warm, "clone_into", lambda _source, dest: clones.append(dest) or True
    )
    cli.warm_target()
    assert clones == []


def test_a_build_runs_under_the_checkouts_build_lock(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    held: list[bool] = []

    def cargo(arguments: list[str]) -> int:
        # Another build of this checkout would wait here, before its cargo takes tokens.
        try:
            with cli.system.file_lock(cli.paths.build_lock(), timeout=0):
                held.append(False)
        except cli.system.LockHeld:
            held.append(True)
        return 0

    monkeypatch.setattr(cli, "cargo", cargo)
    assert cli.shared_build() == 0
    assert held == [True]
