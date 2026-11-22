"""`workspace-hack`: writes `beryl_workspace_hack`'s manifest, which makes a build of any one
workspace package resolve every shared dependency with the whole workspace's features.

Cargo resolves features for the packages a command names, not for the whole workspace. So
`cargo test -p beryl_cards -p cards_1993` built serde without `rc`, serde_json without
`raw_value` and syn without `full`, which the whole workspace's build turns on for the Tauri
crates. Those units hash differently from the whole workspace's, so the subset compiled them
and every crate above them again. Ignite compares the two `cargo tree -e features` outputs
before it builds a subset of packages (`feature_differences` in
ignite/crates/ignite_daemon/src/build/cargo.rs), and when they differ it builds the whole
workspace instead: after a card edit, 35 crates' test binaries in 33-77 s.

`beryl_workspace_hack` depends on every external package whose features a package's own build
resolves differently, with the union of the features the whole workspace and the packages give
it. It lists them twice: as `[dependencies]` for the target side of the build and as
`[build-dependencies]` for the host side, which build scripts and proc macros use and which
Cargo resolves separately. Every workspace package names it as a dev-dependency, never a
normal one: the kernel, rules and card crates have no runtime dependencies, and the Beryl 3D
workspace (rust/beryl3d) depends on beryl_cli, whose normal dependencies it resolves with its
own versions. A plain build of the whole workspace still turns on what its tests do
(tools/tests/test_cli_build.py), because the hack crate is a workspace member. The reference
includes the iroh transport so its tests can run separately and share the other artifacts.
When a package is used on both sides, its features are shared too, so Cargo can reuse a
single artifact when both sides use the same compiler settings.
Cargo 1.98's `resolver.feature-unification = "workspace"` does the same, but needs nightly.

Each list sits under a `cfg(target_os = "macos")` table: `cargo tree` resolves for the host
platform, and a dependency such as objc2 does not build elsewhere. A build on Windows does without
the unified features, and Ignite there builds the whole workspace for a subset, as before.

Features of the workspace's own packages are not this crate's to fix: a package that the whole
workspace builds with a feature its own build lacks names itself or that package with the
feature among its dev-dependencies (the set crates turn on `scenarios`, for example).
tools/tests/test_workspace_hack.py fails when any package's features differ.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tomllib
from collections.abc import Mapping
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from . import paths

HACK = "beryl_workspace_hack"
MANIFEST: Path = paths.RUST / "crates" / HACK / "Cargo.toml"

HEADER = """# Written by `just workspace-hack` (tools/src/beryl_tools/workspace_hack.py), which says why
# this crate exists. Do not edit.

[package]
name = "beryl_workspace_hack"
version = "0.1.0"
description = "Every shared dependency with the whole workspace's features, for builds of a subset"
edition.workspace = true
license.workspace = true
publish = false
build = "build.rs"

[lib]
path = "lib.rs"
test = false
doctest = false

[lints]
workspace = true
"""

# A `cargo tree --format '{p}|{f}'` line's package: a name and a version, and a source in
# parentheses for a workspace or git package.
PACKAGE: re.Pattern[str] = re.compile(r"^(\S+) v(\S+)(?: \((.*)\))?$")

# Each package can resolve independently for the host (build scripts and proc macros)
# and the target (libraries, tests and binaries).
PackageSide = tuple[str, str]
Features = dict[PackageSide, str]


def feature_tree(selection: list[str], *, tests: bool = True) -> Features:
    """Read Cargo's resolved features for each package on each side of a native build."""
    edges = "normal,build,dev" if tests else "normal,build"
    # Explicitly select the native target so Cargo does not fold identical host
    # and target nodes together when it deduplicates the tree.
    out = subprocess.run(
        [
            "cargo",
            "tree",
            "--locked",
            "--target",
            "host-tuple",
            "-e",
            edges,
            "--charset",
            "ascii",
            "--format",
            "{p}|{f}",
            *selection,
        ],
        cwd=paths.RUST,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return parse_feature_tree(out)


# The ASCII tree prints four characters per level, including the branch to each
# package. A dependency heading is indented to the depth of its parent package.
INDENT: re.Pattern[str] = re.compile(r"^(?:[|]   |    )*(?:[|`]-- )?")


def parse_feature_tree(tree: str) -> Features:
    found: Features = {}
    host: dict[int, bool] = {-1: False}
    build_edges: dict[int, bool] = {-1: False}
    for line in tree.splitlines():
        indent = INDENT.match(line)
        assert indent is not None
        depth = indent.end() // 4
        node = line[indent.end() :]
        if "|" not in node:
            build_edges[depth] = node == "[build-dependencies]"
            continue
        package, features = node.split("|", 1)
        proc_macro = package.endswith(" (proc-macro)")
        package = package.removesuffix(" (proc-macro)")
        host[depth] = proc_macro or host[depth - 1] or build_edges[depth - 1]
        build_edges[depth] = False
        side = "host" if host[depth] else "target"
        features = features.removesuffix(" (*)").strip()
        assert found.setdefault((side, package), features) == features, (side, package)
    return found


def members() -> list[str]:
    """The names of the workspace's packages, read from their manifests. The reads
    are recorded in data/pytest-reads.json so an edit to any manifest reruns the
    feature checks."""
    workspace = tomllib.loads((paths.RUST / "Cargo.toml").read_text())["workspace"]
    names: list[str] = []
    for pattern in workspace["members"]:
        for directory in sorted(paths.RUST.glob(pattern)):
            manifest_path = directory / "Cargo.toml"
            if manifest_path.is_file():
                names.append(
                    tomllib.loads(manifest_path.read_text())["package"]["name"]
                )
    return sorted(names)


# Each shared package's features in a build and in the whole workspace, by side.
Difference = dict[PackageSide, tuple[str, str]]


def compare(build: Features, whole: Features) -> Difference:
    return {
        key: (features, whole[key])
        for key, features in build.items()
        if key in whole and features != whole[key]
    }


def differences() -> dict[str, Difference]:
    """Compare the resolved features of the workspace, the check's scope, and single-package
    test builds. Include iroh in the reference without making it a regular build root.
    """
    whole: Features = feature_tree(["--workspace"])

    def differ(package: str) -> tuple[str, Difference]:
        return package, compare(feature_tree(["-p", package]), whole)

    threads = int(os.environ.get("IGNITE_CORES") or os.cpu_count() or 1)
    with ThreadPoolExecutor(max(1, threads)) as pool:
        found = dict(pool.map(differ, members()))
    found["workspace host vs. target"] = {
        (side, package): (host, target)
        for (side, package), host in whole.items()
        if side == "host"
        and (target := whole.get(("target", package))) is not None
        and host != target
    }
    found["workspace without iroh"] = compare(feature_tree(paths.BUILD_SCOPE), whole)
    found["workspace libraries without iroh"] = compare(
        feature_tree(paths.BUILD_SCOPE, tests=False), whole
    )
    return {package: diff for package, diff in found.items() if diff}


def external(package: str) -> tuple[str, str] | None:
    """A crates.io package's name and version, from its `{p}` text; None for a workspace or git
    package."""
    found = PACKAGE.match(package)
    if not found or found[3]:
        return None
    return found[1], found[2]


def manifest(unions: Mapping[tuple[str, str], set[str]]) -> str:
    """The hack crate's manifest listing each package with its features, by name and version.
    Multiple versions of a package get keys that include their version.
    """
    names: dict[str, int] = {}
    for name, _ in unions:
        names[name] = names.get(name, 0) + 1
    lines: list[str] = []
    for (name, version), features in sorted(unions.items()):
        requirement = version.split("+", 1)[0]
        suffix = re.sub(r"[^a-zA-Z0-9]", "_", requirement)
        key = name if names[name] == 1 else f"{name}_{suffix}"
        rename = f'package = "{name}", ' if key != name else ""
        listed = ", ".join(f'"{f}"' for f in sorted(features))
        lines.append(
            f'{key} = {{ {rename}version = "{requirement}", default-features = false,'
            f" features = [{listed}] }}"
        )
    body = "".join(f"{line}\n" for line in lines)
    return (
        HEADER
        + "\n[target.'cfg(target_os = \"macos\")'.dependencies]\n"
        + body
        + "\n[target.'cfg(target_os = \"macos\")'.build-dependencies]\n"
        + body
    )


def write() -> dict[str, Difference]:
    """Writes the manifest from nothing until no package's own tree resolves a crates.io
    package differently from the whole workspace's, and returns the differences left, which
    are in workspace packages' own features. Each round adds the packages that still differ,
    with the union of their features, which can turn on features further down. Features only
    grow, so the rounds end. Starting from nothing drops a package or version that no longer
    differs."""
    unions: dict[tuple[str, str], set[str]] = {}
    MANIFEST.write_text(manifest(unions))
    while True:
        # Records the manifest's dependencies in Cargo.lock, at the versions it already holds.
        subprocess.run(
            ["cargo", "metadata", "--offline", "--format-version", "1"],
            cwd=paths.RUST,
            capture_output=True,
            check=True,
        )
        found = differences()
        grown = False
        for diff in found.values():
            for (_, package), (alone, whole) in diff.items():
                key = external(package)
                if key is None:
                    continue
                features = {
                    f for selection in (alone, whole) for f in selection.split(",") if f
                }
                if key not in unions or not features <= unions[key]:
                    unions.setdefault(key, set()).update(features)
                    grown = True
        if not grown:
            return found
        MANIFEST.write_text(manifest(unions))


def describe(found: Mapping[str, Difference]) -> list[str]:
    """A line for each package that differs, for a person to read."""
    return [
        f"{package}: {dependency} has {side} features [{alone}], [{whole}] in the reference"
        for package, diff in sorted(found.items())
        for (side, dependency), (alone, whole) in sorted(diff.items())
    ]


def run(args: argparse.Namespace) -> int:
    del args
    left = write()
    for line in describe(left):
        print(line)
    if left:
        print(
            "a package that differs in a crates.io package lacks beryl_workspace_hack among its"
            " dev-dependencies; for a workspace package's own feature, name that package with"
            " the feature among the dev-dependencies of the package built alone",
            file=sys.stderr,
        )
    return 1 if left else 0


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    c = commands.add_parser(
        "workspace-hack",
        help="write beryl_workspace_hack's manifest from the workspace's feature trees",
    )
    c.set_defaults(func=run)
