"""Beryl 3D's source lints. Each prints one line per finding and exits 1 when there is any."""

from __future__ import annotations

import bisect
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Callable, NamedTuple, NotRequired, TypedDict, cast

from .. import paths

SCENE_CRATES: set[str] = {
    "kit_base",
    "kit_text",
    "kit_frame",
    "kit_ui",
    "kit_space",
    "kit_fx",
    "kit_scene",
    "kit_probe",
    "game_table",
    "game_play",
    "game_hud",
    "game_manual",
    "game_probe",
    "app_screens",
    "app_client",
    "scaffold_web",
}
RENDER = "beryl_render"
INSPECT = "beryl_inspect"
# Scene and toolkit code must be deterministic and independent of the host. These `std`
# paths are banned even when they appear in grouped imports (docs/beryl-3d.md, "Determinism").
SCENE_MODULES: set[str] = {"time", "thread", "fs", "net", "env", "process", "io", "os"}
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
SCENE_DEPENDENCIES: set[str] = SCENE_CRATES | {
    "arrayvec",
    "self_cell",
    "smallbox",  # Inline storage for action messages.
    "beryl_protocol",
    "bytemuck",
    "glam",
    "taffy",
    "parley",
    "serde",
    "serde_json",
    "serde_path_to_error",
}
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


def data_names(root: Path) -> list[str]:
    from .tuning_names import data_names as check

    return check(root)


# Elements take their sizes from kit_base design tokens (docs/beryl-3d.md, "Elements").
TOKEN_FOLDERS: tuple[tuple[str, str], ...] = (
    ("kit_ui", "widgets"),
    ("game_play", "hud"),
    ("game_hud", "hud"),
    ("game_hud", "inspect"),
    ("game_hud", "prompts"),
    ("game_hud", "screens"),
    ("game_manual", "manual"),
    ("game_manual", "screens"),
    ("app_screens", "screens"),
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
    """Each nontrivial float literal outside tests in code that lays out and styles elements."""
    found: list[str] = []
    for crate, folder in TOKEN_FOLDERS:
        directory = crates(root) / crate / "src" / folder
        for path in sorted(directory.rglob("*.rs")):
            for number, line in non_test_lines(path):
                for match in FLOAT_LITERAL.finditer(line):
                    if float(match[0].replace("_", "")) not in PLAIN_FLOATS:
                        found.append(
                            f"{path.relative_to(root)}:{number}: the literal {match[0]} belongs "
                            f"in {TOKEN_FILE} under `flat` for a design token, or in "
                            "the corresponding data/beryl3d/ tuning file for a layout setting"
                        )
    return found


LEVELS_FILE = "data/beryl3d/levels.json"
PROTOCOL = "beryl_protocol"
# These Magic-specific names belong at the game level. Match words and identifier parts
# so the rule covers types, fields, labels, test data and comments.
MAGIC_WORDS = {
    "battlefield",
    "graveyard",
    "library",
    "mana",
    "creature",
    "planeswalker",
}
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
    protocol_literals: dict[str, int]
    profile_words: NotRequired[dict[str, int]]
    web_citations: NotRequired[dict[str, int]]


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


def rust_tokens(source: str, *, comments: bool = False) -> list[RustToken]:
    """Keep path syntax and identifiers, treating literals and optional comments as opaque."""
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
            if comments:
                tokens.append(RustToken(source[match.start() : offset], match.start()))
        elif comments or match.lastgroup != "comment":
            tokens.append(RustToken(match[0], match.start()))
    return tokens


def upwards(config: Levels, source: str, target: str) -> bool:
    """Apply the dependency order and the dedicated consumer of the web scaffold."""
    order = config["crates"]
    if source in order and order[source]["level"] == "toolkit":
        if target.startswith(("game_", "app_")) or (
            target in order and order[target]["level"] in {"game", "app"}
        ):
            return True
    if source in {"beryl_assets", "beryl_render"} and target in order:
        return order[target]["level"] != "toolkit"
    if target == "scaffold_web":
        return source != "app_client"
    if source == "scaffold_web" and target == "app_client":
        return True
    return (
        source in order
        and target in order
        and order[target]["order"] > order[source]["order"]
    )


def line_starts(source: str) -> list[int]:
    return [0, *(match.end() for match in re.finditer("\n", source))]


def protocol_literal_lines(source: str) -> list[tuple[int, str]]:
    """Find direct protocol struct literals, including ones split across lines."""
    tokens = rust_tokens(source)
    names = [token.text.removeprefix("r#") for token in tokens]
    starts = line_starts(source)
    found: list[tuple[int, str]] = []
    for index, name in enumerate(names[:-2]):
        if name not in {"ObjectView", "PlayerView", "Step"} or names[index + 1] != "{":
            continue
        start = index
        while start >= 2 and names[start - 1] == "::":
            if RUST_IDENTIFIER.fullmatch(names[start - 2]) is None:
                break
            start -= 2
        if start and names[start - 1] == "::":
            start -= 1
        if start and names[start - 1] in {">", "struct", "enum", "impl", "for", "let"}:
            continue
        if name == "Step":
            # A bare Step may also be an enum variant, such as TablePress::Step.
            if index == start and names[index + 2] not in {"act", "cause", "."}:
                continue
            if index != start and names[index - 2] != "script":
                continue
        found.append((bisect.bisect_right(starts, tokens[index].offset), name))
    return found


def protocol_literals(root: Path) -> list[str]:
    """Hold Beryl 3D's direct protocol literals to the legacy stories' counts."""
    config = cast(Levels, json.loads((root / LEVELS_FILE).read_text()))
    limits = config["protocol_literals"]
    measured: dict[str, list[tuple[int, str]]] = {}
    for path in sorted(crates(root).rglob("*.rs")):
        if locations := protocol_literal_lines(path.read_text()):
            measured[path.relative_to(root).as_posix()] = locations

    found: list[str] = []
    for path in sorted(measured.keys() | limits.keys()):
        locations, limit = measured.get(path, []), limits.get(path, 0)
        if len(locations) > limit:
            found.extend(
                f"{path}:{line}: {name} literal raises the count to {len(locations)}, "
                f"above {limit}. Use a builder from beryl_protocol."
                for line, name in locations[limit:]
            )
        elif len(locations) < limit:
            found.append(
                f"{LEVELS_FILE}: protocol_literals count for {path} dropped to "
                f"{len(locations)} from {limit}. Lower or remove the allowance."
            )
    return found


IDENTIFIER_PART: re.Pattern[str] = re.compile(
    r"[A-Z]+(?=[A-Z][a-z]|[^A-Za-z]|$)|[A-Z]?[a-z]+"
)


def profile_word_counts(root: Path) -> dict[str, int]:
    """Count layout names in Rust identifiers outside profile modules, by crate."""
    counts: dict[str, int] = {}
    for crate in sorted(crates(root).iterdir()):
        count = 0
        for path in crate.rglob("*.rs"):
            if "profiles" in path.relative_to(crate).parts[:-1]:
                continue
            for token in rust_tokens(path.read_text()):
                if RUST_IDENTIFIER.fullmatch(token.text) and any(
                    part.lower() in {"phone", "desktop"}
                    for part in IDENTIFIER_PART.findall(token.text.removeprefix("r#"))
                ):
                    count += 1
        if count:
            counts[crate.name] = count
    return counts


def profile_words(root: Path) -> list[str]:
    """Keep knowledge of particular layout names inside profile modules."""
    config = cast(Levels, json.loads((root / LEVELS_FILE).read_text()))
    return compare_levels(
        "profile_words",
        profile_word_counts(root),
        config.get("profile_words", {}),
        False,
    )


def web_citation_counts(root: Path) -> dict[str, int]:
    """Count web source references in comments and strings outside the scaffold."""
    counts: dict[str, int] = {}
    for crate in sorted(crates(root).iterdir()):
        if crate.name == "scaffold_web":
            continue
        count = 0
        for path in crate.rglob("*.rs"):
            source = path.read_text()
            if "ui/src" in source:
                count += sum(
                    len(re.findall(r"(?<![\w.-])ui/src(?![\w.-])", token.text))
                    for token in rust_tokens(source, comments=True)
                )
        if count:
            counts[crate.name] = count
    return counts


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


def toolkit_words(root: Path, path: Path, source: str) -> list[str]:
    """Report names the toolkit source or story obtains from game concepts."""
    found: list[str] = []
    for number, line in enumerate(source.splitlines(), 1):
        # This is the standard macOS location for supplemental system fonts.
        line = line.replace("/System/Library/Fonts/Supplemental", "")
        words = {word.lower() for word in IDENTIFIER_PART.findall(line)}
        for word in sorted(words & MAGIC_WORDS):
            found.append(
                f"{path.relative_to(root)}:{number}: toolkit source names {word}"
            )
        if re.search(r"\bberyl_protocol\b", line):
            found.append(
                f"{path.relative_to(root)}:{number}: toolkit source names {PROTOCOL}"
            )
    return found


def crate_levels(root: Path, config: Levels, found: list[str], update: bool) -> None:
    """Check dependency edges, source budgets and toolkit vocabulary."""
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
            if name.startswith("kit_"):
                found.append(f"{relative}: register {name} as toolkit in {LEVELS_FILE}")
            continue
        toolkit = order[name]["level"] == "toolkit"
        if name.startswith("kit_") and not toolkit:
            found.append(f"{relative}: register {name} as toolkit in {LEVELS_FILE}")
        if toolkit and PROTOCOL in dependencies:
            found.append(f"{relative}: toolkit crate {name} depends on {PROTOCOL}")
        lines = 0
        src = manifest.parent / "src"
        sources = manifest.parent if toolkit else src
        for path in sorted(sources.rglob("*.rs")):
            source = path.read_text()
            if path.is_relative_to(src):
                lines += len(source.splitlines())
            if toolkit:
                found.extend(toolkit_words(root, path, source))
        if toolkit:
            for path in sorted((manifest.parent / "stories").rglob("*.story")):
                found.extend(toolkit_words(root, path, path.read_text()))
        if problem := check_cap(config, name, lines, update):
            found.append(f"{src.relative_to(root)}: {problem}")


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
    if "protocol" in config:
        found.append(f"{LEVELS_FILE}: remove the toolkit protocol allowance")
    crate_levels(root, config, found, update)
    if verbose:
        for name, allowance in sorted(config["cap_allowances"].items()):
            print(
                f"Cap allowance for {name}: {allowance.get('lines')} lines. {allowance.get('reason', '')}"
            )
    if "profile_words" in config:
        found.extend(
            compare_levels(
                "profile_words",
                profile_word_counts(root),
                config["profile_words"],
                update,
            )
        )
    found.extend(
        compare_levels(
            "web_citations",
            web_citation_counts(root),
            config.get("web_citations", {}),
            update,
        )
    )
    after = json.dumps(config, indent=2) + "\n"
    if update and after != before:
        path.write_text(after)
        if verbose:
            print(f"Updated {LEVELS_FILE}")
    return found


def levels_step(*args: str) -> int:
    """Print the dependency order violations and source counts, optionally lowering the recorded counts."""
    return report("levels", levels(paths.REPO, update="--update" in args, verbose=True))


def guides(root: Path) -> list[str]:
    from .guides import guides as check

    return check(root)


def report(name: str, found: list[str]) -> int:
    for line in found:
        print(f"{name}: {line}")
    return 1 if found else 0


SOURCE_RULES: dict[str, Callable[[Path], list[str]]] = {
    "scene-purity": scene_purity,
    "levels": levels,
    "lints-match": lints_match,
    "data-names": data_names,
    "scene-literals": scene_literals,
    "protocol-literals": protocol_literals,
    "profile-words": profile_words,
    "guides": guides,
}
SOURCE_INPUTS: dict[str, list[str]] = {
    "data-names": ["tools/src/beryl_tools/beryl3d/tuning_names.py"],
    "guides": [
        "tools/src/beryl_tools/beryl3d/guides.py",
        "tools/src/beryl_tools/beryl3d/new.py",
        "tools/src/beryl_tools/beryl3d/new_files.py",
        "tools/src/beryl_tools/beryl3d/command.py",
        "tools/src/beryl_tools/system.py",
    ],
}

if __name__ == "__main__":
    name: str = sys.argv[1]
    found: int = (
        levels_step(*sys.argv[2:])
        if name == "levels"
        else report(name, SOURCE_RULES[name](paths.REPO))
    )
    sys.exit(found)
