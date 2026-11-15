"""The `beryl_cli` the checks run shares the tests step's build (`paths.CLI_BUILD`).

`CLI_BUILD` builds the workspace's libraries and the CLI in the same profile and over the same
packages as the tests step's `cargo test`, so whichever runs second compiles nothing new. That
holds only while both commands resolve the same features for every crate. `cargo test` turns
on what the members' dev-dependencies ask for, and a plain `cargo build` does not. A feature
that only a dev-dependency turns on, such as the old `coverage` feature of `beryl_engine`, or
`getrandom`'s `std` that the tests' `proptest` asks for, makes the two commands compile that
crate and every crate above it twice.
"""

import re
import subprocess

from beryl_tools import paths


def features(edges: str) -> dict[str, set[str]]:
    """Each package's resolved features, as `cargo tree` resolves them over the tests step's
    packages following `edges`."""
    tree = subprocess.run(
        ["cargo", "tree", "--locked", *paths.BUILD_SCOPE, "-e", edges]
        + ["-f", "{p} [{f}]", "--prefix", "none", "--no-dedupe"],
        cwd=paths.RUST,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    found: dict[str, set[str]] = {}
    for line in tree.splitlines():
        if m := re.match(r"(\S+ v\S+)(?: \(.*\))? \[(.*)\]$", line.strip()):
            found.setdefault(m[1], set()).update(f for f in m[2].split(",") if f)
    return found


def test_no_feature_is_turned_on_only_by_a_dev_dependency() -> None:
    plain = features("normal,build")
    tests = features("normal,build,dev")
    only_dev = {p: sorted(tests[p] - f) for p, f in plain.items() if tests[p] != f}
    assert only_dev == {}, (
        "these features are turned on only by dev-dependencies, so the tests step and"
        " CLI_BUILD would compile these crates and every crate above them twice; turn them"
        f" on in a normal dependency too: {only_dev}"
    )


def test_the_cli_builds_over_the_tests_steps_packages_in_its_profile() -> None:
    build = paths.CLI_BUILD
    scope = paths.BUILD_SCOPE
    assert any(build[i : i + len(scope)] == scope for i in range(len(build)))
    assert build[build.index("--profile") + 1] == "test"
    assert "--lib" in build
