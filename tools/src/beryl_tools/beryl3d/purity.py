"""Beryl 3D's source lints. Each prints one line per finding and exits 1 when there is any."""

from __future__ import annotations

import bisect
import json
import re
import sys
import tomllib
from collections.abc import Iterator
from pathlib import Path
from typing import NamedTuple, TypedDict, cast

from .. import paths

SCENE = "beryl_scene"
SCENE_CRATES: frozenset[str] = frozenset({SCENE, "kit_base", "kit_frame", "kit_text"})
RENDER = "beryl_render"
INSPECT = "beryl_inspect"
# Scene and toolkit code must be deterministic and independent of the host. These `std`
# paths are banned even when they appear in grouped imports (docs/beryl-3d.md, "Determinism").
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
# Dependencies used by the scene or toolkit must preserve frame determinism.
SCENE_DEPENDENCIES: frozenset[str] = frozenset(
    {
        "arrayvec",
        "self_cell",
        "smallbox",  # Inline storage for action messages.
        "beryl_protocol",
        "bytemuck",
        "glam",
        "taffy",
        "parley",
        "kit_base",
        "kit_frame",
        "kit_text",
        "serde",
        "serde_json",
    }
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
# The main workspace's crates that bring the engine, the rules or the cards. Only the app and
# the recorder may depend on them: every other crate, and each of its test binaries, then builds
# without them, so an edit to the engine rebuilds only those two (docs/beryl-3d.md,
# "Edit-to-output time"). Card records come from `beryl_card_records`, which links none of them.
ENGINE_CRATES: tuple[str, ...] = (
    "beryl_session",
    "beryl_cards",
    "beryl_engine",
    "beryl_rules",
)
ENGINE_USERS: frozenset[str] = frozenset({"beryl_3d", RECORD})


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
        found.extend(engine_use(root, crate))
        if crate.name in SCENE_CRATES:
            found.extend(scene_dependencies(root, crate))
        for path in sorted((crate / "src").rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if crate.name in SCENE_CRATES:
                found.extend(
                    f"{rel}:{number}: {crate.name} names {name}; the scene is pure data, with "
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
    """A line for each crate a scene manifest depends on outside `SCENE_DEPENDENCIES`."""
    manifest = crate / "Cargo.toml"
    if not manifest.is_file():
        return []
    named = tomllib.loads(manifest.read_text())
    return [
        f"{manifest.relative_to(root).as_posix()}: {crate.name} depends on {name}, which "
        "SCENE_DEPENDENCIES in tools/src/beryl_tools/beryl3d/purity.py does not list; add it "
        "there once it is known to read no clock, start no thread and touch no file"
        for name in sorted(named.get("dependencies", {}))
        if name not in SCENE_DEPENDENCIES
    ]


def engine_use(root: Path, crate: Path) -> list[str]:
    """A line for each engine crate that `crate`'s manifest depends on, in any table, unless
    `crate` is one of `ENGINE_USERS`."""
    manifest = crate / "Cargo.toml"
    if crate.name in ENGINE_USERS or not manifest.is_file():
        return []
    named = tomllib.loads(manifest.read_text())
    tables = ("dependencies", "dev-dependencies", "build-dependencies")
    return [
        f"{manifest.relative_to(root).as_posix()}: {crate.name} depends on {name}; only "
        "beryl_3d and beryl_record link the engine (card records: beryl_card_records)"
        for name in ENGINE_CRATES
        if any(name in named.get(table, {}) for table in tables)
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


# The folders of beryl_scene's `src` whose code is built from design tokens: a screen, prompt,
# widget, heads-up display, inspector or manual element takes its sizes from the generated
# `tokens` module (rust/beryl3d/crates/kit_base/src/tokens.rs), never from a number of its own
# (docs/beryl-3d.md, "Elements").
TOKEN_FOLDERS: tuple[str, ...] = (
    "screens",
    "prompts",
    "widgets",
    "hud",
    "inspect",
    "manual",
)
TOKEN_FILE = "data/design-tokens.json"
# The float literals that are not sizes: zero, one, a half to centre and two to halve or double.
PLAIN_FLOATS: frozenset[float] = frozenset({0.0, 0.5, 1.0, 2.0})
# A float literal: `12.0`, `1.35`, `1_000.0` or `4f32`, not the `0` of a field `x.0` or a part of
# a name.
FLOAT_LITERAL: re.Pattern[str] = re.compile(
    r"(?<![\w.])(\d[\d_]*\.\d[\d_]*|\d[\d_]*(?=f32))"
)
STRING_LITERAL: re.Pattern[str] = re.compile(r'"(?:[^"\\]|\\.)*"')


def non_test_lines(path: Path) -> list[tuple[int, str]]:
    """The code lines of `path` outside `#[cfg(test)]` items, comments and string contents."""
    kept: list[tuple[int, str]] = []
    depth = 0
    waiting = False
    for number, line in code_lines(path):
        if depth == 0 and not waiting and line.strip() == "#[cfg(test)]":
            waiting = True
            continue
        if waiting or depth > 0:
            depth += line.count("{") - line.count("}")
            if waiting and "{" in line:
                waiting = False
            elif waiting and ";" in line:
                waiting = False
            continue
        kept.append((number, STRING_LITERAL.sub('""', line.split("//")[0])))
    return kept


def scene_literals(root: Path) -> list[str]:
    """Each float literal in beryl_scene's screens, prompts, widgets, heads-up display,
    inspector and manual code, outside tests, that is not 0, 0.5, 1 or 2."""
    found: list[str] = []
    src = crates(root) / SCENE / "src"
    for folder in TOKEN_FOLDERS:
        for path in sorted((src / folder).rglob("*.rs")):
            for number, line in non_test_lines(path):
                for match in FLOAT_LITERAL.finditer(line):
                    if float(match[0].replace("_", "")) not in PLAIN_FLOATS:
                        found.append(
                            f"{path.relative_to(root)}:{number}: the literal {match[0]} is a "
                            f"size, so it is a design token in {TOKEN_FILE}, under `flat`, "
                            "that the code names through `tokens`"
                        )
    return found


LEVELS_FILE = "data/beryl3d/levels.json"
PROTOCOL = "beryl_protocol"
DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


class CrateLevel(TypedDict):
    level: str
    order: int


class CapAllowance(TypedDict):
    lines: int
    reason: str


class Levels(TypedDict):
    crates: dict[str, CrateLevel]
    cap_lines: int
    cap_allowances: dict[str, CapAllowance]
    modules: dict[str, str]
    upward: dict[str, int]
    protocol: dict[str, int]


class RustToken(NamedTuple):
    text: str
    offset: int


RUST_IDENTIFIER: re.Pattern[str] = re.compile(r"(?:r#)?[^\W\d]\w*")
RUST_TOKEN: re.Pattern[str] = re.compile(
    r'(?P<block>/\*)|(?P<comment>//[^\n]*)|(?:br|cr|r)(?P<hashes>\#*)"(?s:.*?)"(?P=hashes)'
    r"""|(?:b|c)?"(?:[^"\\]|\\[\s\S])*"|b?'(?:[^'\\\n]|\\(?:u\{[\da-fA-F_]+\}|x[\da-fA-F]{2}|.))'"""
    r"|::|" + RUST_IDENTIFIER.pattern + r"|\S"
)
BLOCK_DELIMITER: re.Pattern[str] = re.compile(r"/\*|\*/")


def rust_tokens(source: str) -> list[RustToken]:
    """Keep path syntax and identifiers, skipping Rust comments and treating literals as opaque."""
    tokens: list[RustToken] = []
    offset = 0
    while match := RUST_TOKEN.search(source, offset):
        offset = match.end()
        if match.lastgroup == "block":
            depth = 1
            for end in BLOCK_DELIMITER.finditer(source, offset):
                depth += 1 if end[0] == "/*" else -1
                if depth == 0:
                    offset = end.end()
                    break
            else:
                break
        elif match.lastgroup != "comment":
            tokens.append(RustToken(match[0], match.start()))
    return tokens


class RustPath(NamedTuple):
    parts: tuple[str, ...]
    offset: int
    alias: str | None


def path_tree(tokens: list[RustToken], start: int) -> tuple[list[RustPath], int]:
    """Read paths through a nested use tree or through the path part of an expression."""
    parts: list[str] = []
    index = start
    while index < len(tokens):
        word = tokens[index].text
        if word == "{":
            paths: list[RustPath] = []
            index += 1
            while index < len(tokens) and tokens[index].text != "}":
                children, index = path_tree(tokens, index)
                paths.extend(
                    RustPath(
                        (
                            tuple(parts)
                            if child.parts == ("self",)
                            else tuple([*parts, *child.parts])
                        ),
                        child.offset,
                        child.alias,
                    )
                    for child in children
                )
                if index >= len(tokens) or tokens[index].text != ",":
                    break
                index += 1
            return paths, index + 1
        if word != "*" and not RUST_IDENTIFIER.fullmatch(word):
            break
        parts.append(word.removeprefix("r#"))
        index += 1
        if index == len(tokens) or tokens[index].text != "::":
            break
        index += 1
    if not parts:
        return [], index
    alias = None
    if index + 1 < len(tokens) and tokens[index].text == "as":
        alias = tokens[index + 1].text.removeprefix("r#")
        index += 2
    return [RustPath(tuple(parts), tokens[start].offset, alias)], index


def crate_paths(tokens: list[RustToken]) -> Iterator[RustPath]:
    for index in range(len(tokens) - 1):
        if tokens[index].text == "crate" and tokens[index + 1].text == "::":
            paths, _ = path_tree(tokens, index + 2)
            yield from (path for path in paths if path.parts)


def root_reexports(path: Path) -> dict[str, tuple[str, ...]]:
    """Resolve root re-exports such as `crate::Client` to their module in beryl_scene."""
    if not path.is_file():
        return {}
    tokens = rust_tokens(path.read_text())
    exports: dict[str, tuple[str, ...]] = {}
    for index in range(len(tokens) - 1):
        if tokens[index].text != "pub" or tokens[index + 1].text != "use":
            continue
        paths, _ = path_tree(tokens, index + 2)
        for parts, _, alias in paths:
            if parts and parts[-1] != "*":
                exports[alias or parts[-1]] = (
                    parts[1:] if parts[0] == "crate" else parts
                )
    return exports


def module_key(relative: Path, modules: dict[str, str]) -> str | None:
    """Find the most specific file or directory prefix in the division map."""
    for prefix in (relative, relative.with_suffix(""), *relative.parents):
        key = prefix.as_posix()
        if key in modules:
            return key
    return None


def upwards(config: Levels, source: str, target: str) -> bool:
    """Apply the dependency order and the dedicated consumer of the web scaffold."""
    if target == "scaffold_web":
        return source != "app_client"
    if source == "scaffold_web" and target == "app_client":
        return True
    order = config["crates"]
    return (
        source in order
        and target in order
        and order[target]["order"] > order[source]["order"]
    )


def named_lines(tokens: list[RustToken], starts: list[int]) -> set[int]:
    return {
        bisect.bisect_right(starts, token.offset)
        for token in tokens
        if token.text.removeprefix("r#") == PROTOCOL
    }


def line_starts(source: str) -> list[int]:
    return [0, *(match.end() for match in re.finditer("\n", source))]


def legacy_imports(
    root: Path, config: Levels, protocol: dict[str, int], found: list[str]
) -> dict[str, list[str]]:
    """Measure the upward imports left in beryl_scene and its assigned protocol lines."""
    src = crates(root) / SCENE / "src"
    modules = config.get("modules", {})
    assignments: dict[Path, str] = {}
    targets: dict[tuple[str, ...], str] = {}
    for path in sorted(src.rglob("*.rs")):
        relative = path.relative_to(src)
        if relative == Path("lib.rs"):
            continue
        key = module_key(relative, modules)
        if key is None:
            found.append(
                f"{path.relative_to(root)}: assign this module in {LEVELS_FILE}"
            )
            continue
        assignments[path] = key
        parts = (
            relative.parent.parts
            if path.name == "mod.rs"
            else relative.with_suffix("").parts
        )
        targets[parts] = modules[key]
    exports = root_reexports(src / "lib.rs")

    upward: dict[str, list[str]] = {}
    for path, key in assignments.items():
        importer = modules[key]
        source = path.read_text()
        starts = line_starts(source)
        tokens = rust_tokens(source)
        if importer in protocol:
            protocol[importer] += len(named_lines(tokens, starts))
        for parts, offset, _ in crate_paths(tokens):
            resolved = (
                tuple([*exports[parts[0]], *parts[1:]])
                if parts[0] in exports
                else parts
            )
            for end in range(len(resolved), 0, -1):
                if (target := targets.get(resolved[:end])) is not None:
                    if upwards(config, importer, target):
                        number = bisect.bisect_right(starts, offset)
                        upward.setdefault(key, []).append(
                            f"{path.relative_to(root)}:{number}: "
                            f"crate::{'::'.join(parts)} points upward from {importer} to {target}"
                        )
                    break
    return upward


def dependency_names(
    manifest: dict[str, object], inherited: dict[str, object]
) -> set[str]:
    """Find package names behind direct, target-specific and inherited dependencies."""
    sections = [manifest]
    targets = manifest.get("target")
    if isinstance(targets, dict):
        sections.extend(targets.values())
    names: set[str] = set()
    for section in sections:
        for kind in DEPENDENCY_TABLES:
            table = section.get(kind)
            if not isinstance(table, dict):
                continue
            for alias, spec in table.items():
                if isinstance(spec, dict) and spec.get("workspace"):
                    spec = inherited.get(alias)
                names.add(
                    spec.get("package", alias) if isinstance(spec, dict) else alias
                )
    return names


def check_cap(config: Levels, name: str, lines: int, update: bool) -> str | None:
    cap = config["cap_lines"]
    allowances = config["cap_allowances"]
    allowance = allowances.get(name)
    if allowance is None:
        if lines > cap:
            return (
                f"{name} has {lines:,} source lines, over the {cap:,}-line cap. "
                f"Look for a natural seam first. If there is none, add a cap allowance with a "
                f"reason in {LEVELS_FILE}."
            )
        return None
    reason = allowance.get("reason", "")
    if not isinstance(reason, str) or not reason.strip():
        return f"{LEVELS_FILE}: cap_allowances.{name} needs a non-empty reason"
    limit = allowance.get("lines")
    if type(limit) is not int or limit < 0:
        return f"{LEVELS_FILE}: cap_allowances.{name} needs a non-negative line count"
    if lines > cap and lines > limit:
        return f"{name} has {lines:,} source lines, above its {limit:,}-line allowance"
    if lines <= cap or lines < limit:
        if not update:
            return f"{name} has {lines:,} source lines; run `just b3d levels --update` to adjust its cap allowance"
        if lines <= cap:
            del allowances[name]
        else:
            allowance["lines"] = lines
    return None


def crate_levels(
    root: Path, config: Levels, protocol: dict[str, int], found: list[str], update: bool
) -> None:
    """Check manifest edges and source sizes, and add the protocol lines in the new crates."""
    base = crates(root)
    workspace = base.parent / "Cargo.toml"
    inherited = (
        tomllib.loads(workspace.read_text())
        .get("workspace", {})
        .get("dependencies", {})
        if workspace.is_file()
        else {}
    )
    order = config["crates"]
    for name in sorted(config["cap_allowances"].keys() - order.keys()):
        found.append(f"{LEVELS_FILE}: cap_allowances.{name} names an unknown crate")
    for name in order:
        if not (base / name / "Cargo.toml").is_file():
            found.append(
                f"{LEVELS_FILE}: {name} has no Cargo.toml in {base.relative_to(root)}"
            )
    for manifest in sorted(base.glob("*/Cargo.toml")):
        name = manifest.parent.name
        dependencies = dependency_names(tomllib.loads(manifest.read_text()), inherited)
        relative = manifest.relative_to(root)
        for dependency in sorted(dependencies):
            if upwards(config, name, dependency):
                found.append(f"{relative}: {name} depends upward on {dependency}")
        if name not in order:
            continue
        lines = 0
        protocol_lines = 0
        src = manifest.parent / "src"
        sources = manifest.parent if name in protocol else src
        for path in sorted(sources.rglob("*.rs")):
            source = path.read_text()
            if path.is_relative_to(src):
                lines += len(source.splitlines())
            if name in protocol:
                protocol_lines += len(
                    named_lines(rust_tokens(source), line_starts(source))
                )
        if problem := check_cap(config, name, lines, update):
            found.append(f"{src.relative_to(root)}: {problem}")
        if name in protocol:
            protocol[name] += protocol_lines
            if PROTOCOL in dependencies and not (
                protocol_lines and config["protocol"].get(name)
            ):
                found.append(f"{relative}: toolkit crate {name} depends on {PROTOCOL}")


def compare_levels(
    section: str,
    measured: dict[str, int],
    limits: dict[str, int],
    update: bool,
) -> list[str]:
    found: list[str] = []
    for key in sorted(measured.keys() | limits.keys()):
        count, limit = measured.get(key, 0), limits.get(key, 0)
        if count > limit:
            found.append(
                f"{LEVELS_FILE}: {section}.{key} measured {count}, above {limit}"
            )
        elif count < limit:
            if update:
                if section == "upward" and count == 0:
                    del limits[key]
                else:
                    limits[key] = count
            else:
                found.append(
                    f"{LEVELS_FILE}: {section}.{key} measured {count}, below {limit}; "
                    "run `just b3d levels --update`"
                )
    return found


def levels(root: Path, *, update: bool = False, verbose: bool = False) -> list[str]:
    """Hold Beryl 3D's client core to its dependency order and source budgets."""
    path = root / LEVELS_FILE
    config = cast(Levels, json.loads(path.read_text()))
    before = json.dumps(config, indent=2) + "\n"
    found: list[str] = []
    protocol = {
        name: 0
        for name, entry in config["crates"].items()
        if entry["level"] == "toolkit"
    }
    upward = legacy_imports(root, config, protocol, found)
    crate_levels(root, config, protocol, found, update)
    counts = {key: len(lines) for key, lines in upward.items()}
    if verbose:
        for lines in upward.values():
            print("\n".join(lines))
        print(f"{sum(counts.values())} upward imports")
        print("Protocol lines by toolkit crate")
        for name, count in protocol.items():
            print(f"  {name}  {count}")
        for name, allowance in sorted(config["cap_allowances"].items()):
            print(
                f"Cap allowance for {name}: {allowance.get('lines')} lines. {allowance.get('reason', '')}"
            )
    limits = config.get("upward", {})
    found.extend(compare_levels("upward", counts, limits, update))
    found.extend(compare_levels("protocol", protocol, config["protocol"], update))
    if not verbose:
        for key, lines in upward.items():
            if counts[key] > limits.get(key, 0):
                found.extend(lines)
    after = json.dumps(config, indent=2) + "\n"
    if update and after != before:
        path.write_text(after)
        if verbose:
            print(f"Updated {LEVELS_FILE}")
    return found


def levels_step(*args: str) -> int:
    """Print the division's imports and source counts, optionally lowering the recorded counts."""
    return report("levels", levels(paths.REPO, update="--update" in args, verbose=True))


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


def scene_literals_step(*_args: str) -> int:
    """The `scene-literals` check."""
    return report("scene-literals", scene_literals(paths.REPO))


STEPS = {
    "scene-purity": scene_purity_step,
    "levels": levels_step,
    "lints-match": lints_match_step,
    "data-names": data_names_step,
    "scene-literals": scene_literals_step,
}

if __name__ == "__main__":
    sys.exit(STEPS[sys.argv[1]](*sys.argv[2:]))
