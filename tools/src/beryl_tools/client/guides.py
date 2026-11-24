"""Standards for The client's guides and scaffolding recipes."""

from __future__ import annotations

import json
import re
from pathlib import Path

from . import new
from .purity import LEVELS_FILE, crates

MAP_SOURCE = "client/crates/beryl_inspect/src/cli/map.rs"
MAP_RECIPE: re.Pattern[str] = re.compile(r'Kind::scaffold\(\s*"([^"]+)",\s*"([^"]+)"')


def guides(root: Path) -> list[str]:
    """Hold guide sizes and check the recipes printed by the scaffold parser."""
    config = json.loads((root / LEVELS_FILE).read_text())
    allowances = config.get("guide_allowances", {})
    top = crates(root).parent / "AGENTS.md"
    base = crates(root)
    expected = {
        base / name / "AGENTS.md"
        for name, crate in config.get("crates", {}).items()
        if crate["level"] != "scaffold"
    }
    crate_guides = expected | set(base.glob("*/AGENTS.md"))
    limits = {top: 150, **dict.fromkeys(crate_guides, 60)}
    found: list[str] = []
    remaining = set(allowances)
    for path, cap in sorted(limits.items()):
        relative = path.relative_to(root).as_posix()
        allowance = allowances.get(relative, cap)
        remaining.discard(relative)
        if not path.is_file():
            if allowance is not None:
                found.append(f"{relative}: missing guide")
            continue
        count = len(path.read_text().splitlines())
        if allowance is None:
            found.append(
                f"{LEVELS_FILE}: {relative} now has a guide; remove its allowance"
            )
            allowance = cap
        if type(allowance) is not int or allowance < cap:
            found.append(f"{LEVELS_FILE}: invalid guide allowance for {relative}")
        elif count > allowance:
            found.append(
                f"{relative}: {count} lines exceeds the {allowance}-line limit"
            )
        elif allowance > cap and count < allowance:
            found.append(
                f"{LEVELS_FILE}: lower or remove the guide allowance for {relative} ({count} lines)"
            )
        if path != top:
            companion = path.with_name("CLAUDE.md")
            if not companion.is_file() or companion.read_text().splitlines() != [
                "@AGENTS.md"
            ]:
                found.append(
                    f"{companion.relative_to(root)}: add a single @AGENTS.md line"
                )
    for path in sorted(remaining):
        found.append(f"{LEVELS_FILE}: remove the guide allowance for missing {path}")

    recipes = new.help_by_kind()
    for kind, recipe in recipes.items():
        count = len(recipe.splitlines())
        if count > 30:
            found.append(
                f"just client new {kind} --help: {count} lines exceeds the 30-line limit"
            )
        if not recipe.strip():
            found.append(f"just client new {kind}: missing recipe")
    for inventory, kind in MAP_RECIPE.findall((root / MAP_SOURCE).read_text()):
        if kind not in recipes:
            found.append(
                f"just client map {inventory}: missing scaffold recipe {kind!r}"
            )
    return found
