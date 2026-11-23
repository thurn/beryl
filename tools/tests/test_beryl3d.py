"""`just b3d` lists its commands, and each command describes itself."""

from __future__ import annotations

import argparse

import pytest

from beryl_tools.beryl3d import cli as beryl3d_cli


def test_b3d_lists_its_commands_and_names_the_nearest_unknown_one(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert beryl3d_cli.main([]) == 0
    listed = capsys.readouterr().out.splitlines()
    assert listed[0] == "just b3d"
    assert any(line.split()[:1] == ["gate"] for line in listed)
    assert beryl3d_cli.main(["gaet"]) == beryl3d_cli.USAGE
    assert "'gate'" in capsys.readouterr().err
    with pytest.raises(SystemExit):
        beryl3d_cli.main(["gate", "--help"])
    shown = capsys.readouterr().out
    assert "examples:" in shown and shown.count("just b3d gate") >= 2


def test_each_command_has_two_examples_or_a_program_that_prints_its_own() -> None:
    for command in beryl3d_cli.COMMANDS:
        if command.passthrough:
            assert not command.examples, command.name
        else:
            assert len(command.examples) == 2, command.name


def test_clippy_builds_in_its_own_target_directory(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    seen: list[dict[str, str] | None] = []
    monkeypatch.setattr(
        beryl3d_cli.cargo, "run", lambda arguments, env=None: seen.append(env) or 0
    )
    assert beryl3d_cli.run_clippy(argparse.Namespace()) == 0
    env = seen[0]
    assert env is not None
    assert env["CARGO_TARGET_DIR"] == str(beryl3d_cli.CLIPPY_TARGET)
