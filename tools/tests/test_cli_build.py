"""The CLI and scenario products use the same workspace build as the Rust tests."""

import shlex
import subprocess
import sys
import tomllib

import pytest

from beryl_tools import paths, workspace_hack


def package_selection(command: list[str]) -> list[str]:
    """The build arguments that also select packages and features in cargo tree."""
    selection: list[str] = []
    values = {"-p", "--package", "--exclude", "--features", "-F"}
    switches = {"--workspace", "--all-features", "--no-default-features"}
    words = iter(command)
    for word in words:
        flag = word.split("=", 1)[0]
        if flag in values:
            selection.append(word)
            if flag == word:
                selection.append(next(words))
        elif word in switches:
            selection.append(word)
    return selection


def cli_commands() -> dict[str, list[str]]:
    """The Cargo invocations that build the shared CLI and scenario binaries."""
    manifest = paths.REPO / "ignite/checks/rust.toml"
    products = tomllib.loads(manifest.read_text())["product"]
    review = subprocess.run(
        ["just", "--dry-run", "review", "replay.json"],
        cwd=paths.REPO,
        capture_output=True,
        text=True,
        check=True,
    )
    review_build = next(
        part.strip()
        for line in review.stderr.splitlines()
        for part in line.split(";")
        if part.strip().startswith("cargo build ")
    )
    return {
        "cli": shlex.split(products["cli"]["build"]),
        "scenario-dump": shlex.split(products["scenario-dump"]["build"]),
        "CLI_BUILD": paths.CLI_BUILD,
        "just review": shlex.split(review_build),
    }


def test_the_cli_builds_over_the_tests_steps_packages_in_its_profile() -> None:
    product = tomllib.loads((paths.REPO / "ignite/checks/rust.toml").read_text())[
        "product"
    ]["rust-tests"]
    scope = [
        "--workspace",
        *[flag for name in product["exclude"] for flag in ("--exclude", name)],
    ]
    assert scope == paths.BUILD_SCOPE
    profile = tomllib.loads((paths.ENGINE / "Cargo.toml").read_text())["profile"][
        "test"
    ]
    for setting in ("opt-level", "debug"):
        assert profile["build-override"][setting] == profile[setting], setting
    for label, command in cli_commands().items():
        assert package_selection(command) == paths.BUILD_SCOPE, label
        assert command[command.index("--profile") + 1] == "test", label
        assert "--lib" in command, label
        binary = "beryl_scenario" if label == "scenario-dump" else "beryl_cli"
        assert binary in command, label
    # The oracle lane's dump also uses CLI_BUILD's scenario binary.
    assert "beryl_scenario" in paths.CLI_BUILD


def test_cli_products_match_the_rust_test_features_on_both_sides() -> None:
    if sys.platform not in workspace_hack.NATIVE_TARGETS:
        pytest.skip("the hack crate lists macOS and Windows dependencies")
    whole: workspace_hack.Features = workspace_hack.feature_tree(["--workspace"])
    found = {
        label: diff
        for label, command in cli_commands().items()
        if (
            diff := workspace_hack.compare(
                workspace_hack.feature_tree(package_selection(command), tests=False),
                whole,
            )
        )
    }
    assert not found, "\n".join(workspace_hack.describe(found))
