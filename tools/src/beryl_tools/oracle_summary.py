"""The summary of an oracle's results: one status per scenario, with the known divergences
where the oracle is wrong (`data/oracle-divergences.json`) taken out of the failures."""

from __future__ import annotations

import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import cast

JsonObject = dict[str, object]


@dataclass(frozen=True)
class Divergence:
    """A known disagreement where the oracle is wrong, with the rule that says so."""

    oracle: str
    card: str
    scenario: str
    cr: str
    reason: str


def divergences(path: Path, oracle: str) -> dict[str, Divergence]:
    """The allowlist's entries for `oracle`, keyed by "card / scenario"."""
    entries = cast(list[dict[str, str]], json.loads(path.read_text()))
    found = [Divergence(**e) for e in entries]
    return {f"{d.card} / {d.scenario}": d for d in found if d.oracle == oracle}


def summarize(
    results: Sequence[JsonObject],
    skipped: Sequence[JsonObject],
    allowed: Mapping[str, Divergence],
) -> JsonObject:
    """One status per scenario (pass, fail, allowlisted, unsupported) and their counts. A
    scenario on the allowlist that passes is reported too: its entry is stale."""
    scenarios: list[JsonObject] = []
    for r in results:
        key = f"{r['card']} / {r['name']}"
        entry: JsonObject = {"card": r["card"], "name": r["name"]}
        divergence = allowed.get(key)
        if r["status"] == "unsupported":
            entry["status"] = "unsupported"
            entry["reason"] = r["reason"]
        elif r["status"] == "pass":
            entry["status"] = "pass"
            if divergence is not None:
                entry["stale_allowlist"] = True
        elif divergence is not None:
            entry["status"] = "allowlisted"
            entry["cr"] = divergence.cr
            entry["reason"] = divergence.reason
        else:
            entry["status"] = "fail"
            entry["failures"] = r["failures"]
        scenarios.append(entry)
    for u in skipped:
        scenarios.append(
            {
                "card": u["card"],
                "name": u["name"],
                "status": "unsupported",
                "reason": u["reason"],
            }
        )
    counts = {
        status: sum(1 for e in scenarios if e["status"] == status)
        for status in ("pass", "fail", "allowlisted", "unsupported")
    }
    stale = sum(1 for e in scenarios if e.get("stale_allowlist"))
    return {**counts, "stale_allowlist": stale, "scenarios": scenarios}


def attention(summary: JsonObject) -> list[JsonObject]:
    """The scenarios of a summary that need a look: all but the clean passes."""
    scenarios = cast(list[JsonObject], summary["scenarios"])
    return [s for s in scenarios if s["status"] != "pass" or s.get("stale_allowlist")]
