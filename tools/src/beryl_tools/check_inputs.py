"""The check-inputs check: a Rust test or build script that reads a repository file must have
that file among the inputs its check family declares.

Ignite reuses a check's cached result while the check's declared inputs are unchanged
(docs/ignite.md, "Input hashes"). A cargo-nextest family declares `crate-closure`, the
directories of the package that holds a test and of the workspace packages it depends on, and
lists any other file its tests read by hand (ignite/checks/rust.toml). A test that reads a file
outside both, such as `include_str!("../../../data/x.json")`, keeps passing from the cache after
that file changes, and only a cache audit, which reruns 0.5% of hits, would notice.

So this check reads the Rust files of each package a `crate-closure` family runs, finds the
string literals that name a tracked repository path in a statement that reads a file or builds
a path (`include_str!`, `include_bytes!`, `fs::read`, `read_to_string`, `read_dir`, `.join(`,
`concat!`, `CARGO_MANIFEST_DIR`), and reports each path that the family's inputs do not cover
for that package, counting the family's `package_inputs` for that package (the extra inputs
that only its checks hash). A literal is tried relative to its file's directory, to its package's
directory, to its workspace's root and to the checkout's root, in that order.

It cannot see a path built from parts it cannot read, such as a name passed in as an argument;
those reads stay the family's to declare. A literal that names a path it does not read is listed
in `data/check-inputs-allowlist.json`, by file, with how many such literals the file may hold;
the counts may only go down.
"""

from __future__ import annotations

import json
import posixpath
import re
import shlex
import subprocess
import sys
import tomllib
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import paths

MANIFESTS = "ignite/checks"
ALLOWLIST: Path = paths.REPO / "data" / "check-inputs-allowlist.json"
# A statement that reads a file or builds a path from a literal.
READS: re.Pattern[str] = re.compile(
    r"include_str!|include_bytes!|fs::read|read_to_string|read_dir|\.join\(|concat!|"
    r"CARGO_MANIFEST_DIR"
)
# A plain string literal on one line.
LITERAL: re.Pattern[str] = re.compile(r'"((?:[^"\\\n]|\\.)*)"')
# How many lines before a literal still belong to its statement.
STATEMENT_LINES = 3
# The files Cargo reads for every package of a workspace, which crate patterns add.
WORKSPACE_FILES = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain")


@dataclass(frozen=True)
class Package:
    name: str
    dir: str
    workspace: str
    deps: tuple[str, ...]


@dataclass(frozen=True)
class Family:
    name: str
    workspace: str
    # Path patterns beside `crate-closure` and `crate:` entries.
    patterns: tuple[str, ...]
    crates: tuple[str, ...]
    # Packages its commands name with `-p`, if any, and those they leave out with `--exclude`.
    only: tuple[str, ...]
    excluded: tuple[str, ...]
    # Its `package_inputs`: by package, the extra path patterns and `crate:` entries that only
    # that package's checks hash.
    package_patterns: dict[str, tuple[str, ...]]
    package_crates: dict[str, tuple[str, ...]]


@dataclass(frozen=True)
class Read:
    file: str
    line: int
    path: str


def families(root: Path) -> list[Family]:
    """The families whose inputs hold `crate-closure`, each with the workspace it runs in: the
    directory of a `--manifest-path` in its list command, else the directory of the first
    product it needs, else the checkout's root."""
    declared: dict[str, dict[str, Any]] = {}
    products: dict[str, dict[str, Any]] = {}
    for file in sorted((root / MANIFESTS).glob("*.toml")):
        data = tomllib.loads(file.read_text())
        declared.update(data.get("family", {}))
        products.update(data.get("product", {}))
    found: list[Family] = []
    for name, family in sorted(declared.items()):
        inputs = [str(i) for i in family.get("inputs", [])]
        if "crate-closure" not in inputs:
            continue
        words = shlex.split(str(family.get("list", "")))
        workspace = ""
        if "--manifest-path" in words:
            manifest = words[words.index("--manifest-path") + 1]
            workspace = posixpath.dirname(manifest)
        else:
            needs = [str(n) for n in family.get("needs", [])]
            if needs and needs[0] in products:
                workspace = str(products[needs[0]].get("dir", ""))
        per_package: dict[str, list[str]] = {
            str(package): [str(i) for i in listed]
            for package, listed in family.get("package_inputs", {}).items()
        }
        found.append(
            Family(
                name=name,
                workspace=workspace,
                patterns=path_patterns(inputs),
                crates=crate_patterns(inputs),
                only=tuple(w for i, w in enumerate(words[1:]) if words[i] == "-p"),
                excluded=tuple(
                    w for i, w in enumerate(words[1:]) if words[i] == "--exclude"
                ),
                package_patterns={p: path_patterns(i) for p, i in per_package.items()},
                package_crates={p: crate_patterns(i) for p, i in per_package.items()},
            )
        )
    return found


def path_patterns(inputs: Sequence[str]) -> tuple[str, ...]:
    """The path patterns among `inputs`, beside `crate-closure` and `crate:` entries."""
    return tuple(
        i for i in inputs if i != "crate-closure" and not i.startswith("crate:")
    )


def crate_patterns(inputs: Sequence[str]) -> tuple[str, ...]:
    """The packages that `crate:` entries among `inputs` name."""
    return tuple(i[6:] for i in inputs if i.startswith("crate:"))


def packages(root: Path, workspace: str) -> list[Package]:
    """The workspace's packages, from `cargo metadata --no-deps`."""
    manifest = root / workspace / "Cargo.toml"
    done = subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            str(manifest),
        ],
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        raise RuntimeError(f"cargo metadata for {manifest}: {done.stderr.strip()}")
    data = json.loads(done.stdout)
    names = {p["name"] for p in data["packages"]}
    found: list[Package] = []
    for package in data["packages"]:
        folder = Path(package["manifest_path"]).parent.resolve()
        deps = sorted(
            {
                d["name"]
                for d in package["dependencies"]
                if d.get("path") and d["name"] in names
            }
        )
        found.append(
            Package(
                name=package["name"],
                dir=folder.relative_to(root.resolve()).as_posix(),
                workspace=workspace,
                deps=tuple(deps),
            )
        )
    return found


def closure(name: str, graph: dict[str, Package]) -> set[str]:
    seen: set[str] = set()
    stack = [name]
    while stack:
        current = stack.pop()
        if current in seen or current not in graph:
            continue
        seen.add(current)
        stack.extend(graph[current].deps)
    return seen


def within(path: str, folder: str) -> bool:
    return folder in ("", ".") or path == folder or path.startswith(folder + "/")


def glob(pattern: Sequence[str], path: Sequence[str]) -> bool:
    """Ignite's glob (`ignite_core::inputs`): `*` and `?` within a segment, `**` any number of
    segments."""
    if not pattern:
        return not path
    if pattern[0] == "**":
        return any(glob(pattern[1:], path[i:]) for i in range(len(path) + 1))
    if not path:
        return False
    segment = re.escape(pattern[0]).replace(r"\*", "[^/]*").replace(r"\?", "[^/]")
    return re.fullmatch(segment, path[0]) is not None and glob(pattern[1:], path[1:])


def covers(pattern: str, path: str) -> bool:
    """Whether an input pattern names `path` or a directory above it."""
    if pattern.endswith("/**") and not re.search(r"[*?]", pattern[:-3]):
        return within(path, pattern[:-3])
    if not re.search(r"[*?]", pattern):
        return within(path, pattern.rstrip("/"))
    return glob(pattern.split("/"), path.split("/"))


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


def reads(root: Path, file: str, package: Package, known: set[str]) -> list[Read]:
    """The repository paths `file` reads through a literal (see the module's docs)."""
    lines = (root / file).read_text(errors="replace").splitlines()
    bases = (posixpath.dirname(file), package.dir, package.workspace, "")
    found: list[Read] = []
    for number, line in enumerate(lines, 1):
        if line.lstrip().startswith("//"):
            continue
        statement = "\n".join(lines[max(0, number - STATEMENT_LINES) : number])
        if not READS.search(statement):
            continue
        for literal in LITERAL.findall(line):
            path = resolve(literal, bases, known)
            if path is not None:
                found.append(Read(file, number, path))
    return found


def known_paths(tracked: Sequence[str]) -> set[str]:
    """Every tracked file and every directory that holds one."""
    known: set[str] = set()
    for path in tracked:
        parts = path.split("/")
        for end in range(1, len(parts) + 1):
            known.add("/".join(parts[:end]))
    return known


def uncovered(root: Path, tracked: Sequence[str]) -> list[str]:
    """Each read of a `crate-closure` family's package that its inputs do not cover, as
    `<file>:<line>: <path> (<family>, <package>)`."""
    known = known_paths(tracked)
    rust_files = [p for p in tracked if p.endswith(".rs")]
    graphs: dict[str, dict[str, Package]] = {}
    found: list[str] = []
    for family in families(root):
        if family.workspace not in graphs:
            graphs[family.workspace] = {
                p.name: p for p in packages(root, family.workspace)
            }
        graph = graphs[family.workspace]
        package_dirs = sorted((p.dir for p in graph.values()), key=len, reverse=True)
        for package in sorted(graph.values(), key=lambda p: p.name):
            if family.only and package.name not in family.only:
                continue
            if package.name in family.excluded:
                continue
            crates = closure(package.name, graph)
            extras = [*family.crates, *family.package_crates.get(package.name, ())]
            for extra in extras:
                crates |= closure(extra, graph)
            patterns = [
                *family.patterns,
                *family.package_patterns.get(package.name, ()),
                *(graph[c].dir for c in crates),
                *(posixpath.join(family.workspace, f) for f in WORKSPACE_FILES),
            ]
            # A file belongs to the innermost package whose directory holds it.
            own = [
                f
                for f in rust_files
                if within(f, package.dir)
                and next(d for d in package_dirs if within(f, d)) == package.dir
            ]
            for file in own:
                for read in reads(root, file, package, known):
                    if not any(covers(p, read.path) for p in patterns):
                        found.append(
                            f"{read.file}:{read.line}: {read.path} "
                            f"({family.name}, {package.name})"
                        )
    return sorted(set(found))


def problems(found: Sequence[str], allowed: dict[str, int]) -> list[str]:
    """The reads beyond each file's allowance, and allowances the files no longer use."""
    by_file: dict[str, list[str]] = {}
    for line in found:
        by_file.setdefault(line.split(":", 1)[0], []).append(line)
    out: list[str] = []
    for file, lines in sorted(by_file.items()):
        if len(lines) > allowed.get(file, 0):
            out += [
                f"{line}: the family does not declare this path; add it to the family's "
                "inputs in ignite/checks"
                for line in lines
            ]
    for file, count in sorted(allowed.items()):
        have = len(by_file.get(file, []))
        if have < count:
            out.append(
                f"{file}: allowed {count} undeclared reads but has {have}; lower its count "
                f"in {ALLOWLIST.name}"
            )
    return out


def check(root: Path, allowlist: Path, tracked: Sequence[str]) -> list[str]:
    allowed: dict[str, int] = json.loads(allowlist.read_text())
    return problems(uncovered(root, tracked), allowed)


def main(argv: Sequence[str]) -> int:
    tracked = subprocess.run(
        ["git", "-C", str(paths.REPO), "ls-files", "-co", "--exclude-standard"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.splitlines()
    found = check(paths.REPO, ALLOWLIST, tracked)
    for line in found:
        print(f"check-inputs: {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
