"""Import reviewed CI ticket transfers without inferring causes from check names."""

from __future__ import annotations

import argparse
from dataclasses import replace
import json
import re
import sys
from pathlib import Path
from typing import Any

from . import incidents, tickets


def identities(source: dict[str, Any]) -> list[incidents.Identity]:
    meta = incidents.metadata(source)
    if meta.get("beryl_budget_script"):
        return [
            incidents.Identity(
                f"budget:{meta['beryl_budget_script']}",
                "budget",
                "load-adjusted budget exceeded",
            )
        ]
    if meta.get("beryl_cut_lane"):
        return [
            incidents.Identity(
                f"budget:ci:{meta['beryl_cut_lane']}", "budget", "CPU budget exceeded"
            )
        ]
    body = str(source.get("body", source.get("description", "")))
    tail = re.search(r"Output tail:\s*```\n(.*?)\n```", body, re.DOTALL)
    if not tail and meta.get("beryl_flaky_check"):
        blocks = re.findall(r"```\n(.*?)\n```", body, re.DOTALL)
        output = blocks[-1] if blocks else ""
    else:
        output = tail[1] if tail else ""
    if not output:
        return []  # A missing diagnostic cannot prove an incident identity.
    host = str(meta.get("enact_ci_host", ""))
    checks = str(
        meta.get("beryl_nightly_checks", meta.get("beryl_flaky_check", ""))
    ).splitlines()
    platform = str(meta.get("enact_ci_os", ""))
    shown_platform = re.search(r" on (darwin|win32|windows|linux)\b", body)
    if shown_platform:
        platform = shown_platform[1]
    return [
        replace(
            incidents.failure(
                check,
                incidents.check_output(check, output),
                host,
                category="flaky" if meta.get("beryl_flaky_check") else "failure",
            ),
            platform=platform,
        )
        for check in checks
    ]


def operations(
    backend: incidents.Backend, transfers: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    patches: dict[str, dict[str, Any]] = {}
    for transfer in transfers:
        source = backend.show(str(transfer["source"]))
        owners = [backend.show(str(owner["id"])) for owner in transfer["owners"]]
        # Multi-cause aggregates retain all references. Only a diagnostic
        # explicitly assigned to exactly one owner can adopt its signature.
        existing_keys = incidents.metadata(source).get(incidents.KEYS, [])
        if existing_keys and len(owners) != 1:
            raise tickets.TicketError(
                f"{source['id']} already owns signatures; split their transfers explicitly"
            )
        source_patch = patches.setdefault(str(source["id"]), {})
        source_patch[incidents.OWNERS] = [row["id"] for row in owners]
        source_patch[incidents.KEYS] = []
        source_patch["ci_consolidation_epic"] = transfer.get("epic", "")
        if existing_keys:
            owner = owners[0]
            patch = patches.setdefault(str(owner["id"]), {})
            patch[incidents.KEYS] = list(
                dict.fromkeys(
                    [
                        *patch.get(
                            incidents.KEYS,
                            incidents.metadata(owner).get(incidents.KEYS, []),
                        ),
                        *existing_keys,
                    ]
                )
            )
        for identity in identities(source):
            selected = [
                row
                for row, spec in zip(owners, transfer["owners"], strict=True)
                if re.search(str(spec.get("diagnostic", ".*")), identity.diagnostic)
            ]
            if len(selected) != 1:
                continue
            owner = selected[0]
            patch = patches.setdefault(str(owner["id"]), {})
            keys = patch.setdefault(
                incidents.KEYS, list(incidents.metadata(owner).get(incidents.KEYS, []))
            )
            if identity.key() not in keys:
                keys.append(identity.key())
    return [
        {
            "op": "update",
            "id": issue,
            "add_labels": [incidents.LABEL],
            "set_meta": patch,
        }
        for issue, patch in patches.items()
    ]


def migrate(
    backend: incidents.Backend, file: Path, apply: bool = False
) -> dict[str, Any]:
    transfers = json.loads(file.read_text())["transfers"]
    for _ in range(incidents.MAX_ATTEMPTS):
        seq, _ = backend.snapshot()
        ops = operations(backend, transfers)
        if not apply:
            return {"dry_run": True, "operations": ops}
        try:
            return backend.apply(seq, ops)
        except tickets.TicketConflict:
            continue
    raise tickets.TicketError(
        "Incident migration lost repeated conditional-write races"
    )


def command(args: argparse.Namespace) -> int:
    try:
        result = migrate(incidents.Native(tickets.client()), args.file, args.apply)
        print(json.dumps(result))
    except (OSError, RuntimeError, ValueError) as error:
        print(f"incident-migrate: {error}", file=sys.stderr)
        return 1
    return 0
