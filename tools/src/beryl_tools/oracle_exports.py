"""Prepare each scenario once, before selection, and identify the inputs the oracles run."""

from __future__ import annotations

import hashlib
import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

from . import card_facts, card_store, forge, xmage_export, xmage_java


def label(entry: Mapping[str, object]) -> str:
    """The scenario name shared by the dump and both oracle reports."""
    return f"{entry['card']} / {entry['name']}"


@dataclass
class Exports:
    """The prepared inputs, including exporter refusals, shared by selection and execution."""

    xmage: list[xmage_java.Export]
    forge: list[dict[str, object]]
    skipped: list[dict[str, object]]

    @staticmethod
    def prepare(entries: list[dict[str, object]], checkout: Path) -> Exports:
        faces = xmage_java.Faces.load(card_store.STORE, card_facts.named_cards(entries))
        java = xmage_export.export_all(entries, faces, xmage_java.xmage_cards(checkout))
        exported, skipped = forge.export(entries, card_store.STORE)
        return Exports(java, exported, skipped)

    def keys(self) -> dict[str, str]:
        """Hash each method's Java class context and Forge input, or its refusal reason.

        Rendering one method at a time includes imports and the superclass without making
        another scenario in the same card invalidate this one. Method names are retained
        when execution narrows the exports, so the remembered input is what actually runs.
        """
        forge_inputs = {label(e): {"input": e} for e in self.forge}
        forge_inputs.update({label(e): {"unsupported": e} for e in self.skipped})
        keys = {}
        for export in self.xmage:
            name = f"{export.card} / {export.scenario}"
            payload = {
                "xmage": {
                    "java": xmage_java.render_class(export.card, [export]),
                    "unsupported": export.unsupported,
                    "skipped": export.skipped,
                },
                "forge": forge_inputs[name],
            }
            keys[name] = hashlib.sha256(
                json.dumps(payload, sort_keys=True).encode()
            ).hexdigest()
        return keys

    def select(self, entries: Sequence[Mapping[str, object]]) -> Exports:
        """Keep the chosen scenarios without exporting them again."""
        names = {label(e) for e in entries}
        return Exports(
            [e for e in self.xmage if f"{e.card} / {e.scenario}" in names],
            [e for e in self.forge if label(e) in names],
            [e for e in self.skipped if label(e) in names],
        )
