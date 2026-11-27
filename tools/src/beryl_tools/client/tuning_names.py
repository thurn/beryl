"""Checks the names of the client tuning against shared UI conventions."""

from __future__ import annotations

import json
from pathlib import Path


def data_names(root: Path) -> list[str]:
    """Match runtime verb and effect names to the shared UI conventions."""
    directory = root / "data" / "client"
    conventions = json.loads((root / "data" / "ui-conventions.json").read_text())
    verb_names: set[str] = set()
    for verb in conventions["verbs"]:
        verb_names.add(verb["name"])
        verb_names.update(verb.get("cases", {}))
    verbs = json.loads((directory / "verbs.json").read_text())["verbs"]
    found = [
        f"data/client/verbs.json: verbs.{key} names no verb or case of "
        "data/ui-conventions.json"
        for key in verbs
        if key not in verb_names
    ]
    effect_names = {effect["name"] for effect in conventions["effects"]}
    # `effects/palette.json` is the shared color-ramp table loaded via TUNING_FILES.
    effect_files = {file.stem for file in (directory / "effects").glob("*.json")} - {
        "palette"
    }
    extra_effects = effect_files - effect_names
    for name in sorted(extra_effects):
        file = directory / "effects" / f"{name}.json"
        try:
            definition = json.loads(file.read_text())
        except ValueError as error:
            found.append(f"{file.relative_to(root)}: {error}")
            continue
        if not isinstance(definition, dict) or definition.get("preview") is not True:
            found.append(
                f"{file.relative_to(root)}: no such effect in data/ui-conventions.json"
            )
    found.extend(
        f"data/client/effects/{name}.json: missing definition for effect in data/ui-conventions.json"
        for name in sorted(effect_names - effect_files)
    )
    return found
