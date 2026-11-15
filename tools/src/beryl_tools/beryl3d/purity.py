"""Beryl 3D's source lints (docs/beryl-3d.md, 'Determinism' and 'Checks and isolation'):
`scene-purity`, `lints-match` and `data-names`. Each prints one line per finding and exits 1
when there is any."""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

from .. import paths

SCENE = "beryl_scene"
RENDER = "beryl_render"
INSPECT = "beryl_inspect"
# What beryl_scene's source may not name: it is a pure function of its inputs, so no clock,
# thread, file, socket, lock or iteration-order-dependent map, and no GPU or window crate.
SCENE_BANNED: tuple[str, ...] = (
    "std::time",
    "std::thread",
    "std::fs",
    "std::net",
    "std::sync::Mutex",
    "std::sync::RwLock",
    "std::collections::HashMap",
    "std::collections::HashSet",
    "wgpu",
    "winit",
)
# What beryl_inspect's manifest may not name: the inspector reads a scene with no GPU.
INSPECT_BANNED: tuple[str, ...] = (
    "beryl_render",
    "beryl_host",
    "beryl_3d",
    "wgpu",
    "winit",
)
GPU_CALL: re.Pattern[str] = re.compile(r"\bwgpu::")


def crates(root: Path) -> Path:
    return root / "rust" / "beryl3d" / "crates"


def code_lines(path: Path) -> list[tuple[int, str]]:
    """The lines of `path` with their numbers, but those that are only a `//` comment."""
    return [
        (number, line)
        for number, line in enumerate(path.read_text().splitlines(), 1)
        if not line.lstrip().startswith("//")
    ]


def name_pattern(name: str) -> re.Pattern[str]:
    return re.compile(rf"(?<![\w:]){re.escape(name)}(?!\w)")


def scene_purity(root: Path) -> list[str]:
    """Each place where Beryl 3D's sources break the purity rules."""
    found: list[str] = []
    base = crates(root)
    if not base.is_dir():
        return found
    banned = [(name, name_pattern(name)) for name in SCENE_BANNED]
    for crate in sorted(p for p in base.iterdir() if p.is_dir()):
        for path in sorted((crate / "src").rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            for number, line in code_lines(path):
                if crate.name == SCENE:
                    for name, pattern in banned:
                        if pattern.search(line):
                            found.append(
                                f"{rel}:{number}: beryl_scene names {name}; the scene is "
                                "pure data, with no clock, thread, file, socket, lock, "
                                "unordered map or GPU"
                            )
                elif crate.name != RENDER and GPU_CALL.search(line):
                    found.append(
                        f"{rel}:{number}: {crate.name} calls wgpu; only beryl_render "
                        "talks to the GPU"
                    )
    manifest = base / INSPECT / "Cargo.toml"
    if manifest.is_file():
        text = manifest.read_text()
        named = tomllib.loads(text)
        names: set[str] = set()
        for table in ("dependencies", "dev-dependencies", "build-dependencies"):
            names.update(named.get(table, {}))
        for name in INSPECT_BANNED:
            if name in names:
                found.append(
                    f"{manifest.relative_to(root).as_posix()}: beryl_inspect depends on "
                    f"{name}; the inspector reads a scene with no GPU"
                )
    return found


def lint_tables(manifest: Path) -> object:
    return tomllib.loads(manifest.read_text())["workspace"]["lints"]


def lints_match(root: Path) -> list[str]:
    """A line when the two workspaces' `[workspace.lints]` tables differ."""
    main = lint_tables(root / "rust" / "Cargo.toml")
    own = lint_tables(root / "rust" / "beryl3d" / "Cargo.toml")
    if main == own:
        return []
    return [
        "rust/beryl3d/Cargo.toml: [workspace.lints] differs from rust/Cargo.toml's; "
        "copy the main workspace's table"
    ]


def verb_names(root: Path) -> set[str]:
    """Each verb of data/ui-conventions.json, and each of its cases."""
    data = json.loads((root / "data" / "ui-conventions.json").read_text())
    names: set[str] = set()
    for verb in data["verbs"]:
        names.add(verb["name"])
        names.update(verb.get("cases", {}))
    return names


def data_names(root: Path) -> list[str]:
    """Each key under `verbs` in data/beryl3d/space.json that names no verb or case."""
    space = root / "data" / "beryl3d" / "space.json"
    if not space.is_file():
        return []
    known = verb_names(root)
    return [
        f"data/beryl3d/space.json: verbs.{key} names no verb or case of "
        "data/ui-conventions.json"
        for key in json.loads(space.read_text()).get("verbs", {})
        if key not in known
    ]


def report(name: str, found: list[str]) -> int:
    for line in found:
        print(f"{name}: {line}")
    return 1 if found else 0


def scene_purity_step(*_args: str) -> int:
    """The `scene-purity` check."""
    return report("scene-purity", scene_purity(paths.REPO))


def lints_match_step(*_args: str) -> int:
    """The `lints-match` check."""
    return report("lints-match", lints_match(paths.REPO))


def data_names_step(*_args: str) -> int:
    """The `data-names` check."""
    return report("data-names", data_names(paths.REPO))


STEPS = {
    "scene-purity": scene_purity_step,
    "lints-match": lints_match_step,
    "data-names": data_names_step,
}

if __name__ == "__main__":
    sys.exit(STEPS[sys.argv[1]](*sys.argv[2:]))
