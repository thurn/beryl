"""Every Ignite check family declares the Python modules its adapter loads.

A check's input hash covers only what its family declares (docs/ignite.md, "Input hashes"),
so a family whose adapter imports a module it does not declare keeps its cached passes when
that module changes. A family run as `python -m beryl_tools.ignite.<adapter>` loads the
adapter, its parent packages and everything they import when the module is loaded. This test
follows those imports and fails on any module the family's `inputs` do not cover.

Imports inside functions are not followed: the adapters defer orchestration imports that way
on purpose, and following them would make nearly every family read all of `tools/`. A module
read only through such an import is left to the cache audits.
"""

from __future__ import annotations

import ast
import json
import re
import subprocess
import tomllib
from pathlib import Path

from beryl_tools.ignite import python
from beryl_tools.ignite.windows_parity import evidence_gaps, windows_inventory

REPO: Path = Path(__file__).resolve().parents[2]
ADAPTER: re.Pattern[str] = re.compile(r"-m (beryl_tools\.ignite\.\w+)")


def imported_at_load(module: str, files: dict[str, Path]) -> set[str]:
    """The project modules `module` imports outside functions and `TYPE_CHECKING` blocks,
    with their parent packages."""
    path = files[module]
    package = module if path.name == "__init__.py" else module.rpartition(".")[0]
    names: set[str] = set()
    pending: list[ast.stmt] = list(ast.parse(path.read_text()).body)
    while pending:
        node = pending.pop()
        if isinstance(node, ast.If):
            if "TYPE_CHECKING" not in ast.unparse(node.test):
                pending.extend(node.body + node.orelse)
        elif isinstance(node, ast.Try):
            pending.extend(node.body + node.orelse + node.finalbody)
        elif isinstance(node, ast.Import):
            names.update(alias.name for alias in node.names)
        elif isinstance(node, ast.ImportFrom):
            base = node.module or ""
            if node.level:
                parts = package.split(".")
                parts = parts[: len(parts) - (node.level - 1)]
                base = ".".join(parts + ([node.module] if node.module else []))
            names.add(base)
            names.update(f"{base}.{alias.name}" for alias in node.names)
    found: set[str] = set()
    for name in names:
        while name:
            if name in files:
                found.add(name)
            name = name.rpartition(".")[0]
    return found


def closure(adapter: str, files: dict[str, Path]) -> set[str]:
    """Every project module that loading `adapter` loads, `adapter` included."""
    seen: set[str] = set()
    stack = [adapter]
    while stack:
        module = stack.pop()
        if module in seen or module not in files:
            continue
        seen.add(module)
        stack.extend(imported_at_load(module, files))
    return seen


def loaded(adapter: str, files: dict[str, Path]) -> list[str]:
    """The paths, relative to the repository, of the modules in `closure`."""
    return sorted(
        files[m].relative_to(REPO).as_posix() for m in closure(adapter, files)
    )


def pattern(text: str) -> re.Pattern[str]:
    """A manifest path pattern (docs/ignite.md, "Declaring a family") as a regular expression
    that also matches every path below a named directory."""
    out = ""
    i = 0
    while i < len(text):
        if text.startswith("**/", i):
            out, i = out + "(?:.*/)?", i + 3
        elif text.startswith("**", i):
            out, i = out + ".*", i + 2
        elif text[i] == "*":
            out, i = out + "[^/]*", i + 1
        elif text[i] == "?":
            out, i = out + "[^/]", i + 1
        else:
            out, i = out + re.escape(text[i]), i + 1
    return re.compile(out + "(?:/.*)?$")


def undeclared() -> dict[str, list[str]]:
    """For each family with a Python adapter, the modules it loads and does not declare."""
    files = python.project_files(REPO / "tools")
    missing: dict[str, list[str]] = {}
    for manifest in sorted((REPO / "ignite" / "checks").glob("*.toml")):
        families = tomllib.loads(manifest.read_text()).get("family", {})
        for name, family in families.items():
            found = ADAPTER.search(str(family.get("list") or family.get("run") or ""))
            if not found:
                continue
            declared = [
                pattern(p) for p in family["inputs"] if not p.startswith("crate")
            ]
            gaps = [
                path
                for path in loaded(found.group(1), files)
                if not any(d.match(path) for d in declared)
            ]
            if gaps:
                missing[f"{manifest.name}: family.{name}"] = gaps
    return missing


def test_every_family_declares_the_modules_its_adapter_loads() -> None:
    assert undeclared() == {}


def test_every_in_scope_family_can_run_on_cygnus() -> None:
    _, gaps = windows_inventory(REPO)
    assert gaps == []


def test_windows_evidence_requires_every_listed_check_and_local_measurement() -> None:
    def passed(check: str, machine: str = "cygnus-id") -> dict[str, object]:
        return {
            "check": check,
            "outcome": "pass",
            "cached": False,
            "machine": machine,
            "os": "windows",
            "revision": "verified-tip",
        }

    listings = {"counts": ["counts:rollouts"], "rust": ["rust:a", "rust:b"]}
    expected = {*listings, "exhaust"}
    evidence = {
        "listings": listings,
        "results": [passed("counts:rollouts", "other-windows-id"), passed("rust:a")],
    }
    assert evidence_gaps(expected, evidence, "verified-tip", "cygnus-id") == [
        "exhaust: no listing or idle sample",
        "counts:rollouts: verdict is from a different verifier or revision",
        "rust:b: missing verdict",
    ]
    evidence = {
        "listings": {**listings, "exhaust": ["exhaust:sample"]},
        "results": [
            passed(check)
            for check in ["counts:rollouts", "rust:a", "rust:b", "exhaust:sample"]
        ],
    }
    assert evidence_gaps(expected, evidence, "verified-tip", "cygnus-id") == []


def test_the_closure_follows_imports_at_load_and_skips_those_in_functions(
    tmp_path: Path,
) -> None:
    package = tmp_path / "src" / "pkg"
    package.mkdir(parents=True)
    (package / "__init__.py").write_text("")
    (package / "a.py").write_text("from . import b\n\ndef f():\n    from . import c\n")
    (package / "b.py").write_text("import pkg.d\n")
    (package / "c.py").write_text("")
    (package / "d.py").write_text("")
    files = python.project_files(tmp_path)
    assert closure("pkg.a", files) == {"pkg", "pkg.a", "pkg.b", "pkg.d"}


def test_every_crate_input_names_a_workspace_package() -> None:
    """A `crate:` input that names no package leaves every check of its family unresolvable,
    so each would be `incomplete` forever. Cargo package names may use `-` where the
    directory uses `_`."""
    packages: set[str] = set()
    for lock in (
        REPO / "engine" / "Cargo.lock",
        REPO / "client" / "Cargo.lock",
        REPO / "ignite" / "Cargo.lock",
    ):
        if "target" in lock.relative_to(REPO).parts:
            continue
        out = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"]
            + ["--manifest-path", str(lock.with_name("Cargo.toml"))],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        packages.update(p["name"] for p in json.loads(out)["packages"])
    named: set[str] = set()
    for manifest in (REPO / "ignite" / "checks").glob("*.toml"):
        parsed = tomllib.loads(manifest.read_text())
        for table in ("family", "product"):
            for entry in parsed.get(table, {}).values():
                named.update(
                    p.removeprefix("crate:")
                    for p in entry.get("inputs", [])
                    if p.startswith("crate:")
                )
    assert named - packages == set()
