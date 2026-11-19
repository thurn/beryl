"""The check-inputs check: a Rust test reads a repository file through the test-files helper.

Ignite hashes a Rust test over the files it was seen to open (docs/ignite.md, "Observed
inputs"). It sees a read only when the test makes it through `beryl_test_files` (the game's
workspaces) or `ignite_core::test_files` (Ignite's own), which record each path in the
directory `IGNITE_READS` names. A test that opens a repository file with `std::fs` keeps its
cached result after that file changes, and only a cache audit, which reruns 0.5% of hits,
would notice.

So this check reads the test code of the `rust/` and `ignite/` workspaces and reports a raw
read (`fs::read`, `fs::read_to_string`, `fs::read_dir`, `File::open`) that opens a repository
path. It tells a repository path from a temporary folder in two ways:

- A file whose test code names `CARGO_MANIFEST_DIR` (or Beryl 3D's `recordings::root()`,
  which is built from it) and also makes a raw read opens the checkout through it. Every line
  that names it is reported. A test that reads only temporary folders never names it.
- A raw read in a statement with a string literal that names a tracked repository path, such
  as `root().join("data/card-store.tsv")`, is reported, except in Ignite's workspace, whose
  tests build fixture repositories in temporary folders under the real repository's file
  names (`ignite.toml`, `.gitignore`, `ignite/checks`).

Test code is a file under a `tests/` directory, a file named `tests.rs` or `*_tests.rs`, and
the part of any other file after its first `#[cfg(test)]`. A build script's reads need no
helper: Ignite observes its `rerun-if-changed` paths. Compile-time reads (`include_str!`,
`include_bytes!`) are observed from Cargo's dep-info files.

It cannot see a path built from parts it cannot read, such as a directory passed in as an
argument. Such a read stays the test's to route through the helper.
"""

from __future__ import annotations

import posixpath
import re
import subprocess
import sys
from collections.abc import Iterable, Sequence
from pathlib import Path

from . import paths

# The workspaces whose test code is read.
SCOPE = ("rust/", "ignite/")
# A raw read of the filesystem. A call through a helper (`test_files::read_to_string`) is
# not one.
RAW_READ: re.Pattern[str] = re.compile(
    r"(?<!test_files::)\b(?:fs::read_dir|fs::read_to_string|fs::read|File::open)\b"
)
# A way to name the checkout other than the helper: the variable Cargo sets to a package's
# directory, and Beryl 3D's `recordings::root()`, which is built from it.
ROOTS: re.Pattern[str] = re.compile(r"CARGO_MANIFEST_DIR|recordings::root\(\)")
# A plain string literal on one line.
LITERAL: re.Pattern[str] = re.compile(r'"((?:[^"\\\n]|\\.)*)"')
# How many lines before a literal still belong to its statement.
STATEMENT_LINES = 3
# Workspaces whose literals this check does not match against repository paths (see the
# module's docs); their `CARGO_MANIFEST_DIR` reads are still reported.
NOT_MATCHED = ("ignite/",)
# The helpers themselves.
HELPERS = (
    "rust/crates/beryl_test_files/",
    "ignite/crates/ignite_core/src/test_files.rs",
)


def in_scope(path: str) -> bool:
    """Whether `path` is a Rust file whose test code this check reads."""
    return (
        path.endswith(".rs")
        and path.startswith(SCOPE)
        and not path.startswith(HELPERS)
        and posixpath.basename(path) != "build.rs"
        and "/examples/" not in path
    )


def test_lines(path: str, lines: Sequence[str]) -> range:
    """The line indexes of `lines` that are test code."""
    name = posixpath.basename(path)
    if "/tests/" in path or name == "tests.rs" or name.endswith("_tests.rs"):
        return range(len(lines))
    for index, line in enumerate(lines):
        if line.strip().startswith("#[cfg(test)]"):
            return range(index, len(lines))
    return range(0)


def package_dir(path: str, known: set[str]) -> str:
    """The directory of the nearest package above `path`."""
    folder = posixpath.dirname(path)
    while folder and f"{folder}/Cargo.toml" not in known:
        folder = posixpath.dirname(folder)
    return folder


def resolve(literal: str, bases: Iterable[str], known: set[str]) -> str | None:
    """The tracked path or directory of tracked files `literal` names from the first of `bases`
    where it names one."""
    if "/" not in literal.strip("/") and "." not in literal:
        return None
    if literal.startswith(("http:", "https:")) or "{" in literal:
        return None
    # A walk up the directories (`"../.."`) finds a root for another literal to join.
    if set(literal.split("/")) <= {"..", ".", ""}:
        return None
    for base in bases:
        joined = posixpath.normpath(posixpath.join(base, literal.lstrip("/")))
        if joined.startswith("..") or joined == ".":
            continue
        if joined in known:
            return joined
    return None


def known_paths(tracked: Sequence[str]) -> set[str]:
    """Every tracked file and every directory that holds one."""
    known: set[str] = set()
    for path in tracked:
        parts = path.split("/")
        for end in range(1, len(parts) + 1):
            known.add("/".join(parts[:end]))
    return known


def reads(path: str, text: str, known: set[str]) -> list[str]:
    """The raw reads of repository files in the test code of one file, as
    `<file>:<line>: <what>`."""
    lines = text.splitlines()
    region = test_lines(path, lines)
    code = [(i, lines[i]) for i in region if not lines[i].lstrip().startswith("//")]
    raw = [i for i, line in code if RAW_READ.search(line)]
    if not raw:
        return []
    found: list[str] = []
    for i, line in code:
        if ROOTS.search(line) and "env_remove" not in line:
            found.append(
                f"{path}:{i + 1}: opens the checkout through {ROOTS.search(line).group()}"
            )
    if not path.startswith(NOT_MATCHED):
        bases = (posixpath.dirname(path), package_dir(path, known), "rust", "")
        code_lines = {i for i, _ in code}
        for i in raw:
            for number in range(max(0, i - STATEMENT_LINES), i + 1):
                if number not in code_lines:
                    continue
                for literal in LITERAL.findall(lines[number]):
                    named = resolve(literal, bases, known)
                    if named is not None:
                        found.append(f"{path}:{number + 1}: reads {named}")
    return found


def check(root: Path, tracked: Sequence[str]) -> list[str]:
    """Every raw read of a repository file in the test code, as
    `<file>:<line>: <what>: read it through the test-files helper`."""
    known = known_paths(tracked)
    found: set[str] = set()
    for path in tracked:
        if in_scope(path) and (root / path).is_file():
            text = (root / path).read_text(errors="replace")
            found.update(reads(path, text, known))
    return [
        f"{line}: read it through beryl_test_files (ignite_core::test_files in ignite/)"
        for line in sorted(found)
    ]


def main(argv: Sequence[str]) -> int:
    tracked = subprocess.run(
        ["git", "-C", str(paths.REPO), "ls-files", "-co", "--exclude-standard"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.splitlines()
    found = check(paths.REPO, tracked)
    for line in found:
        print(f"check-inputs: {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
