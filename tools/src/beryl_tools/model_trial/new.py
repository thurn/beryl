"""`model-trial new`: file a new trial from a spec file, as one `ignite ticket batch`. The batch
creates the trial epic (parked at allocation zero, pending approval, pinned to this machine,
with the spec in its notes), one block ticket per `block_size` tickets of the list, and the
report ticket, each depending on the one before. The tickets' text comes from the templates in
`templates/`, and the steps they run are in the `model-trial` skill, not in the tickets.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any

from . import io
from .spec import HEADING, Spec, SpecError, from_file

TEMPLATES: Path = Path(__file__).with_name("templates")
ACCEPTANCE = "=== acceptance ==="
TIER = "tier:opus"
BLOCK_MINUTES = 45
REPORT_MINUTES = 60


def template(name: str, **fields: object) -> tuple[str, str]:
    """The body and acceptance of `templates/<name>.txt`, with `fields` filled in."""
    text = (TEMPLATES / f"{name}.txt").read_text(encoding="utf-8")
    body, _, acceptance = text.partition(f"\n{ACCEPTANCE}\n")
    return body.format(**fields).strip(), acceptance.format(**fields).strip()


def block_count(spec: Spec) -> int:
    """One block per `block_size` tickets of the list, never more than the pair limit allows."""
    return math.ceil(min(len(spec.tickets), spec.pair_limit) / spec.block_size)


def block_text(title: str, spec: Spec, block: int) -> tuple[str, str]:
    return template("block", title=title, block=block, blocks=block_count(spec))


def report_text(title: str) -> tuple[str, str]:
    return template("report", title=title)


def notes(raw: object) -> str:
    return f"{HEADING}\n\n```json\n{json.dumps(raw, indent=2)}\n```\n"


def build(title: str, spec: Spec, raw: object, machine: str) -> dict[str, Any]:
    """The batch that files the trial. `raw` is the spec as the user wrote it, kept in the
    epic's notes so that later fields and their defaults read the same way."""
    body, acceptance = template(
        "epic",
        source_labels=", ".join(f"`{label}`" for label in spec.source.labels()),
        copy_labels=", ".join(f"`{label}`" for label in spec.copy.labels()),
        block_size=spec.block_size,
    )
    ops: list[dict[str, Any]] = [
        {
            "op": "create",
            "fields": {
                "title": title,
                "type": "epic",
                "status": "pending_approval",
                "allocation": 0,
                "body": body,
                "acceptance": acceptance,
                "notes": notes(raw),
                "labels": [f"machine:{machine}"],
            },
        }
    ]

    def child(name: str, text: tuple[str, str], minutes: int) -> None:
        ops.append(
            {
                "op": "create",
                "parent": "$0",
                "deps": [{"on": f"${len(ops) - 1}"}] if len(ops) > 1 else [],
                "fields": {
                    "title": name,
                    "type": "task",
                    "status": "open",
                    "body": text[0],
                    "acceptance": text[1],
                    "labels": [TIER],
                    "estimate_minutes": minutes,
                },
            }
        )

    blocks = block_count(spec)
    for block in range(1, blocks + 1):
        child(
            f"{title}, block {block} of {blocks}",
            block_text(title, spec, block),
            BLOCK_MINUTES,
        )
    child(f"{title}, the final reading", report_text(title), REPORT_MINUTES)
    return {"ops": ops}


def cmd_new(args: argparse.Namespace) -> int:
    path = Path(args.spec)
    try:
        spec = from_file(path)
        raw: object = json.loads(path.read_text(encoding="utf-8"))
    except SpecError as error:
        print(f"model-trial: {error}", file=sys.stderr)
        return 2
    batch = build(args.title, spec, raw, io.this_machine())
    if args.dry_run:
        print(json.dumps(batch, indent=2))
        return 0
    reply = io.batch(batch)
    results = reply.get("results", []) if isinstance(reply, dict) else []
    ids = [str(r.get("id")) for r in results]
    print(f"Filed trial {ids[0]} (parked, pending approval): {', '.join(ids[1:])}")
    print(
        f"Next: `just model-trial pair add --trial {ids[0]} {' '.join(spec.tickets)}` "
        "parks the listed tickets."
    )
    return 0


def register(verbs: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    new = verbs.add_parser("new", help="file a new trial's epic, blocks and report")
    new.add_argument("--title", required=True, help="the trial epic's title")
    new.add_argument("--spec", required=True, help="a JSON file holding the trial spec")
    new.add_argument(
        "--dry-run", action="store_true", help="print the batch and file nothing"
    )
    new.set_defaults(func=cmd_new)
