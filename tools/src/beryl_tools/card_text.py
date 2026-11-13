"""The card-text check: every card file's rendered rules text matches its Oracle text.

`card render` (render_diff.py) turns each card's definition back into English and compares it
with the card's Oracle text in the card store. A definition that leaves out a clause renders
without it, so this check is what catches a card that does less than it says: Thieving Skydiver
once rendered without "If that artifact is an Equipment, attach it to this creature", and its
scenarios, which never stole an Equipment, passed in Beryl, XMage and Forge alike.

A mismatch fails the check unless `data/card-text-allowlist.json` lists the card with exactly
the paragraphs that differ (the word diff `card render` prints). Each entry has a `reason`:

- `wording`: the definition does what the card says, and the renderer has no template for
  Oracle's words ("it" where Oracle names the card, one sentence split in two). The `note` says
  why the two texts mean the same thing.
- `escalated`: the definition leaves out a clause because the engine lacks the mechanic. The
  card must have an entry in `data/card-escalations.json` with `"partial": true` naming it, so
  the gap is recorded and `just card brief` shows it.

The list may only shrink. An entry whose card now matches, or whose diff changed, fails the
check, so the list always says exactly what is pinned.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import cast

from . import card_store, paths, render_diff

ALLOWLIST: Path = paths.REPO / "data" / "card-text-allowlist.json"
ESCALATIONS: Path = paths.REPO / "data" / "card-escalations.json"
REASONS = ("wording", "escalated")

Json = card_store.Json


def problems(
    diffs: dict[str, render_diff.Diff], allowlist: Json, escalations: Json
) -> list[str]:
    """What is wrong with the rendered texts `diffs` against the allowlist: a mismatch it doesn't
    pin, an entry for a card that matches or has no file, and an entry that is malformed.
    """
    found: list[str] = []
    for name, diff in sorted(diffs.items()):
        entry = cast(Json | None, allowlist.get(name))
        if diff.matches:
            if entry is not None:
                found.append(f"{name} now matches its Oracle text: drop its entry")
            continue
        if entry is None:
            found.append(
                f"{name} differs from its Oracle text:\n"
                + "\n".join(f"  {line}" for line in diff.lines)
            )
            continue
        pinned = cast(list[str], entry.get("diff", []))
        if pinned != diff.lines:
            found.append(
                f"{name} differs from its Oracle text other than its entry pins:\n"
                + "\n".join(f"  {line}" for line in diff.lines)
            )
    for name, value in sorted(allowlist.items()):
        entry = cast(Json, value)
        if name not in diffs:
            found.append(f"{name} has an entry but no card file: drop its entry")
        reason = entry.get("reason")
        if reason not in REASONS:
            found.append(f"{name}: reason must be one of {', '.join(REASONS)}")
        if not str(entry.get("note", "")).strip():
            found.append(f"{name}: say in `note` why the texts differ")
        escalation = cast(Json | None, escalations.get(name))
        partial = escalation is not None and escalation.get("partial") is True
        if reason == "escalated" and not partial:
            found.append(
                f"{name} is escalated but data/card-escalations.json has no partial entry"
            )
    return found


def load(path: Path) -> Json:
    return cast(Json, json.loads(path.read_text())) if path.exists() else {}


def step(*args: str) -> int:
    """Renders every card file and holds each mismatch to the allowlist."""
    texts = render_diff.rendered([])
    if isinstance(texts, str):
        print(f"card-text: {texts}")
        return 1
    diffs: dict[str, render_diff.Diff] = {}
    with card_store.Store(card_store.STORE) as store:
        for name, faces in texts:
            record = cast(Json, store.card(name))
            canonical = str(record["name"])
            diffs[canonical] = render_diff.compare(
                canonical, render_diff.oracle_faces(record), faces
            )
    found = problems(diffs, load(ALLOWLIST), load(ESCALATIONS))
    for line in found:
        print(f"card-text: {line}")
    if found:
        print(
            "card-text: fix the definition (see the card-writing skill), or pin a wording "
            f"difference in {ALLOWLIST.relative_to(paths.REPO)}"
        )
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
