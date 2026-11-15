"""Beryl 3D's source lints (docs/beryl-3d.md, 'Determinism' and 'Checks and isolation'):
`scene-purity`, `lints-match` and `data-names`. Each prints one line per finding and exits 1
when there is any."""

from __future__ import annotations

import bisect
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
# thread, file, socket, lock, global state or iteration-order-dependent map, and no GPU or
# window crate (docs/beryl-3d.md, 'Determinism'). The modules of `std` it may not reach, however
# the path is written (`std::time::Instant`, `use std::{fs, time}` or a group over several
# lines).
SCENE_MODULES: tuple[str, ...] = (
    "time",
    "thread",
    "fs",
    "net",
    "env",
    "process",
    "io",
    "os",
)
# The names it may not use at all, however they were imported: maps seeded per process, clocks,
# locks and global state.
SCENE_NAMES: tuple[str, ...] = (
    "HashMap",
    "HashSet",
    "RandomState",
    "Instant",
    "SystemTime",
    "Mutex",
    "RwLock",
    "Condvar",
    "OnceLock",
    "LazyLock",
    "thread_local",
    "wgpu",
    "winit",
)
# `AtomicBool`, `AtomicU32` and the rest: state shared between threads.
ATOMIC: re.Pattern[str] = re.compile(r"(?<![\w:])Atomic[A-Z]\w*")
# A path into `std`, or a braced group of them: `std::time`, `std::{fs, collections::X}`.
STD_PATH: re.Pattern[str] = re.compile(r"\bstd\s*::\s*(\{|[a-z_]+)")
# The crates beryl_scene may depend on. Each was checked to read no clock, start no thread and
# touch no file inside `Client::handle` and `Client::frame`; a new one is added here once it
# has been checked the same way, and the frame gate runs it twice to hold its counts equal.
SCENE_DEPENDENCIES: frozenset[str] = frozenset(
    {"beryl_protocol", "bytemuck", "glam", "taffy", "parley", "serde", "serde_json"}
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
# The engine runs in one place among Beryl 3D's tools: the `record` program of `beryl_record`,
# which builds the recordings pack that `beryl_inspect::recordings` reads. Only its manifest and
# its recorder file may name the crate that plays it, so that `b3d` links no engine.
RECORDER_CRATE = "beryl_cli"
RECORD = "beryl_record"
RECORDER_FILE = f"rust/beryl3d/crates/{RECORD}/src/recorder.rs"


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
    for crate in sorted(p for p in base.iterdir() if p.is_dir()):
        found.extend(recorder_use(root, crate))
        if crate.name == SCENE:
            found.extend(scene_dependencies(root, crate))
        for path in sorted((crate / "src").rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if crate.name == SCENE:
                found.extend(
                    f"{rel}:{number}: beryl_scene names {name}; the scene is pure data, with "
                    "no clock, thread, file, socket, lock, global state, unordered map or GPU"
                    for number, name in scene_names(path)
                )
                continue
            for number, line in code_lines(path):
                if crate.name != RENDER and GPU_CALL.search(line):
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


def scene_names(path: Path) -> list[tuple[int, str]]:
    """Each line of `path` that names something `SCENE_MODULES` or `SCENE_NAMES` bans, with
    what it names. A braced `use std::{...}` group is followed across lines."""
    lines = code_lines(path)
    numbers: list[int] = [number for number, _ in lines]
    text = "\n".join(line for _, line in lines)
    starts: list[int] = [0]
    for line in text.split("\n")[:-1]:
        starts.append(starts[-1] + len(line) + 1)

    def number_at(offset: int) -> int:
        return numbers[bisect.bisect_right(starts, offset) - 1]

    found: list[tuple[int, str]] = []
    for match in STD_PATH.finditer(text):
        head = match.group(1)
        if head != "{":
            if head in SCENE_MODULES:
                found.append((number_at(match.start()), f"std::{head}"))
            continue
        depth = 0
        end = match.end(1) - 1
        for end in range(match.end(1) - 1, len(text)):
            depth += {"{": 1, "}": -1}.get(text[end], 0)
            if depth == 0:
                break
        group = text[match.end(1) : end]
        # The top-level items of the group: what comes first, and after each comma at depth 0.
        depth = 0
        item_start = 0
        for index, char in enumerate(group + ","):
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
            elif char == "," and depth == 0:
                item = re.match(r"\s*([a-z_]+)", group[item_start:index])
                if item and item.group(1) in SCENE_MODULES:
                    offset = match.end(1) + item_start + item.start(1)
                    found.append((number_at(offset), f"std::{item.group(1)}"))
                item_start = index + 1
    names = [(name, name_pattern(name)) for name in SCENE_NAMES]
    for number, line in lines:
        found.extend((number, name) for name, pattern in names if pattern.search(line))
        found.extend((number, atomic) for atomic in ATOMIC.findall(line))
    return sorted(set(found))


def scene_dependencies(root: Path, crate: Path) -> list[str]:
    """A line for each crate beryl_scene's manifest depends on outside `SCENE_DEPENDENCIES`."""
    manifest = crate / "Cargo.toml"
    if not manifest.is_file():
        return []
    named = tomllib.loads(manifest.read_text())
    return [
        f"{manifest.relative_to(root).as_posix()}: beryl_scene depends on {name}, which "
        "SCENE_DEPENDENCIES in tools/src/beryl_tools/beryl3d/purity.py does not list; add it "
        "there once it is known to read no clock, start no thread and touch no file"
        for name in sorted(named.get("dependencies", {}))
        if name not in SCENE_DEPENDENCIES
    ]


def recorder_use(root: Path, crate: Path) -> list[str]:
    """Each place in `crate` (its manifest, sources, tests and build script) that names the
    recorder's crate, but beryl_record's manifest and its recorder file."""
    found: list[str] = []
    # The crate as code names it, not a path that has it as a directory.
    pattern = re.compile(rf"(?<![\w/]){RECORDER_CRATE}(?![\w/])")
    manifest = crate / "Cargo.toml"
    if crate.name != RECORD and manifest.is_file():
        named = tomllib.loads(manifest.read_text())
        tables = ("dependencies", "dev-dependencies", "build-dependencies")
        if any(RECORDER_CRATE in named.get(table, {}) for table in tables):
            found.append(
                f"{manifest.relative_to(root).as_posix()}: {crate.name} depends on "
                f"{RECORDER_CRATE}; only beryl_record's `record` program plays the engine"
            )
    for path in sorted(crate.rglob("*.rs")):
        rel = path.relative_to(root).as_posix()
        if rel == RECORDER_FILE:
            continue
        for number, line in code_lines(path):
            if pattern.search(line):
                found.append(
                    f"{rel}:{number}: names {RECORDER_CRATE}; recordings come from "
                    "beryl_inspect::recordings, built by beryl_record's `record` program"
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
