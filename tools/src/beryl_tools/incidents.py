"""Durable CI incidents using the tracker's atomic sequence precondition.

A report is a signature and a sighting. Signatures describe failures; sightings
retain host, run and output. Both the owner and its evidence commit in one batch.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import sys
from dataclasses import dataclass
from typing import Any, Protocol

from . import paths, tickets

LABEL = "ci-incident"
KEYS = "beryl_incident_keys"
OWNERS = "ci_canonical_tickets"
PREVIOUS = "beryl_incident_previous"
MARKER = "beryl-incident-evidence:"
MAX_ATTEMPTS = 8


def digest(value: object) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def config() -> dict[str, Any]:
    file = paths.REPO / "data" / "ci-incidents.json"
    return json.loads(file.read_text()) if file.is_file() else {}


def canonical(workload: str) -> str:
    aliases = config().get("aliases", {})
    seen: set[str] = set()
    while workload in aliases:
        if workload in seen:
            raise tickets.TicketError("Cycle in CI incident workload aliases")
        seen.add(workload)
        workload = str(aliases[workload])
    return workload


def diagnostic(output: str, host: str = "") -> str:
    """Keep diagnostic content, including assertion operands, but omit run noise.

    Unknown formats use their whole output. That errs towards separate incidents
    rather than merging unrelated failures of the same check.
    """
    lines = re.sub(r"\x1b\[[0-9;]*m", "", output).splitlines()
    selected: list[str] = []
    for i, line in enumerate(lines):
        if line.startswith("FAULT "):
            selected.append(line)
            for detail in lines[i + 1 :]:
                if detail.startswith(("FAULT ", "OVER ", "  show:", "...")):
                    break
                selected.append(detail)
            continue
        if re.search(
            r"panicked at|assertion .*failed|^error(?:\[|:)|^OVER |Error:|AssertionError",
            line,
        ):
            selected.extend(
                lines[i : i + 4]
                if "panicked at" in line or "assertion" in line
                else [line]
            )
    text = "\n".join(dict.fromkeys(selected or lines)).strip()
    if host:
        text = text.replace(host, "<host>")
    text = re.sub(r"thread '[^']*' \(\d+\)", "thread", text)
    text = re.sub(r":\d+:\d+(?=[:\s]|$)", ":<location>", text)
    text = re.sub(r"\b0x[0-9a-fA-F]+\b", "<address>", text)
    # Runner summaries describe the run, not the fault. Preserve numerical
    # assertion operands and board/view identities; they distinguish regressions.
    text = "\n".join(
        line
        for line in text.splitlines()
        if not re.search(
            r"^note: run with|^(?:b3d-sweep|check|ci|sweep|nightly) .*FAILED|^just |^warm:",
            line,
        )
    )
    return text or "(no diagnostic output)"


@dataclass(frozen=True)
class Identity:
    workload: str
    category: str
    diagnostic: str
    platform: str = ""

    def key(self) -> str:
        platform = self.platform or sys.platform
        platform = {"win32": "windows", "Windows": "windows", "Darwin": "darwin"}.get(
            platform, platform
        )
        return digest(
            [1, canonical(self.workload), self.category, platform, self.diagnostic]
        )


def failure(
    check: str, output: str, host: str = "", *, category: str = "failure"
) -> Identity:
    if category == "failure" and re.search(
        r"(?:^|\n)(?:CANNOT_RUN|cannot-run):", output
    ):
        category = "cannot-run"
    return Identity(check, category, diagnostic(output, host))


@dataclass(frozen=True)
class Sighting:
    identity: Identity
    run: str
    host: str
    fields: dict[str, Any]
    parent: str = ""

    def evidence(self) -> str:
        return digest([self.identity.key(), self.run, self.host, self.fields["body"]])


@dataclass(frozen=True)
class Result:
    issue: str
    created: bool


class Backend(Protocol):
    def snapshot(self) -> tuple[int, list[dict[str, Any]]]: ...
    def show(self, issue: str) -> dict[str, Any]: ...
    def apply(self, seq: int, operations: list[dict[str, Any]]) -> dict[str, Any]: ...


class Native:
    def __init__(self, executable: str) -> None:
        self.executable = executable

    def snapshot(self) -> tuple[int, list[dict[str, Any]]]:
        # Read the cursor first. Any concurrent change during the following
        # filtered read makes the eventual write conflict, including creation.
        graph = json.loads(tickets.invoke(self.executable, "graph"))
        rows = tickets.list_issues(self.executable, "--label", LABEL, "--all")
        # The live graph also supplies legacy reports which have not yet been
        # migrated. Only a matching diagnostic may adopt one of these owners.
        by_id = {str(row["id"]): row for row in [*graph["tickets"], *rows]}
        return int(graph["seq"]), list(by_id.values())

    def show(self, issue: str) -> dict[str, Any]:
        return json.loads(tickets.invoke(self.executable, "show", issue, "--comments"))

    def apply(self, seq: int, operations: list[dict[str, Any]]) -> dict[str, Any]:
        return json.loads(
            tickets.invoke(
                self.executable,
                "batch",
                input_text=json.dumps(
                    {
                        "if": [{"seq": seq}],
                        "ops": operations,
                    }
                ),
            )
        )


def metadata(row: dict[str, Any]) -> dict[str, Any]:
    return row.get("metadata") or {}


def resolve(
    backend: Backend, row: dict[str, Any], seen: frozenset[str] = frozenset()
) -> list[dict[str, Any]]:
    issue = str(row["id"])
    if issue in seen:
        raise tickets.TicketError(f"Cycle in incident owners at {issue}")
    owners = metadata(row).get(OWNERS, [])
    if not owners:
        return [row]
    result: list[dict[str, Any]] = []
    for owner in owners:
        result.extend(resolve(backend, backend.show(owner), seen | {issue}))
    return result


def route(fields: dict[str, Any], parent: str) -> tuple[dict[str, Any], str]:
    fields = dict(fields)
    labels = list(fields.get("labels", []))
    # Global P0 Stewards belong to Enact, never the configured programme.
    if fields.get("priority") == 0 and "role:steward" in labels:
        return fields, ""
    destination = os.environ.get("BERYL_CI_INCIDENT_PARENT", config().get("parent", ""))
    if not parent and destination:
        parent = str(destination)
        labels = [label for label in labels if label != "role:steward"]
        if "role:executor" not in labels:
            labels.append("role:executor")
    fields["labels"] = labels
    return fields, parent


def has_evidence(row: dict[str, Any], marker: str) -> bool:
    return marker in str(row.get("body", row.get("description", ""))) or any(
        marker in comment.get("text", "") for comment in row.get("comments", [])
    )


def legacy_candidate(row: dict[str, Any], identity: Identity) -> bool:
    meta = metadata(row)
    if KEYS in meta:
        return False
    checks = str(meta.get("beryl_nightly_checks", "")).splitlines()
    if meta.get("beryl_flaky_check"):
        checks = (
            [str(meta["beryl_flaky_check"])] if identity.category == "flaky" else []
        )
    if meta.get("beryl_budget_script"):
        checks = [f"budget:{meta['beryl_budget_script']}"]
    if meta.get("beryl_cut_lane"):
        checks = [f"budget:ci:{meta['beryl_cut_lane']}"]
    return canonical(identity.workload) in {canonical(check) for check in checks}


def matches(row: dict[str, Any], key: str) -> bool:
    meta = metadata(row)
    if key in meta.get(KEYS, []):
        return True
    # Alias additions must also work for incidents filed before the rename.
    saved = meta.get("beryl_incident_identity")
    return isinstance(saved, dict) and Identity(**saved).key() == key


def report(backend: Backend, sighting: Sighting) -> Result:
    key, evidence = sighting.identity.key(), sighting.evidence()
    marker = MARKER + evidence
    text = f"{sighting.fields['body']}\n\nRun: {sighting.run}; host: {sighting.host}.\n<!-- {marker} -->"
    last_error = ""
    for _ in range(MAX_ATTEMPTS):
        seq, rows = backend.snapshot()
        matching: list[dict[str, Any]] = []
        for row in rows:
            if matches(row, key):
                matching.append(row)
            elif legacy_candidate(row, sighting.identity):
                from .incident_migration import identities

                detail = backend.show(str(row["id"]))
                if any(key == old.key() for old in identities(detail)):
                    matching.append(row)
        owners: dict[str, dict[str, Any]] = {}
        previous: list[str] = []
        for row in matching:
            # An uncertain create response remains idempotent even if the
            # owner closed before retry. Creation embeds the first sighting.
            detail = backend.show(str(row["id"]))
            resolved = resolve(backend, row)
            if has_evidence(detail, marker):
                live = [owner for owner in resolved if owner.get("status") != "closed"]
                return Result(
                    str(live[0]["id"]) if len(live) == 1 else str(row["id"]), False
                )
            for owner in resolved:
                if owner.get("status") != "closed":
                    owners[str(owner["id"])] = owner
                else:
                    previous.append(str(owner["id"]))
        if len(owners) > 1:
            raise tickets.TicketError(
                f"Ambiguous incident owners for {key}: {', '.join(owners)}"
            )
        operations: list[dict[str, Any]] = []
        if owners:
            issue, owner = next(iter(owners.items()))
            detail = backend.show(issue)
            if has_evidence(detail, marker):
                return Result(issue, False)
            keys = list(metadata(owner).get(KEYS, []))
            if key not in keys:
                operations.append(
                    {
                        "op": "update",
                        "id": issue,
                        "add_labels": [LABEL],
                        "set_meta": {KEYS: keys + [key]},
                    }
                )
        else:
            fields, parent = route(sighting.fields, sighting.parent)
            fields["labels"] = list(dict.fromkeys([*fields.get("labels", []), LABEL]))
            fields["metadata"] = {
                **fields.get("metadata", {}),
                KEYS: [key],
                "beryl_incident_identity": {
                    "workload": canonical(sighting.identity.workload),
                    "category": sighting.identity.category,
                    "platform": sighting.identity.platform or sys.platform,
                    "diagnostic": sighting.identity.diagnostic,
                },
                PREVIOUS: sorted(set(previous)),
            }
            # Global seq fences this create; a random request key added by tk
            # need not outlive its one-day transport retry window.
            fields["body"] = text
            operations.append(
                {
                    "op": "create",
                    "fields": fields,
                    **({"parent": parent} if parent else {}),
                }
            )
            issue = "$0"
        if owners:
            operations.append({"op": "comment", "id": issue, "text": text})
        try:
            reply = backend.apply(seq, operations)
        except tickets.TicketConflict as error:
            last_error = str(error)
            continue
        except tickets.TicketError as error:
            # A transport failure may have committed. Re-read before any
            # retry; the durable marker resolves the uncertain response.
            last_error = str(error)
            continue
        return Result(str(reply["results"][-1]["id"]), not bool(owners))
    raise tickets.TicketError(
        f"CI incident reporting did not commit after concurrent changes or transport errors: {last_error}"
    )


class Preview:
    """Read the same ownership decision as a write and print its atomic batch."""

    def __init__(self, real: Backend, command: str) -> None:
        self.real: Backend = real
        self.command: str = command

    def snapshot(self) -> tuple[int, list[dict[str, Any]]]:
        return self.real.snapshot()

    def show(self, issue: str) -> dict[str, Any]:
        return self.real.show(issue)

    def apply(self, seq: int, operations: list[dict[str, Any]]) -> dict[str, Any]:
        for operation in operations:
            if operation["op"] == "create":
                fields = operation["fields"]
                host = fields.get("metadata", {}).get("enact_ci_host", "")
                print(
                    f"{self.command}: would file under {operation.get('parent', '')}, "
                    f"labelled {','.join(fields['labels'])}, P{fields['priority']}, "
                    f"from host {host}: {fields['title']}\n{fields['body']}\nAcceptance: {fields['acceptance']}"
                )
            elif operation["op"] == "comment" and operation["id"] != "$0":
                print(
                    f"{self.command}: would add to {operation['id']} the comment:\n{operation['text']}"
                )
        return {
            "results": [
                {"id": "(dry run)" if op.get("id", "$0") == "$0" else op["id"]}
                for op in operations
            ]
        }


def check_output(check: str, output: str) -> str:
    """Take a native fault's own details when an aggregate step names them.

    A full output with no separable record remains evidence. It must not be
    silently replaced by the check name, which would hide new diagnostics.
    """
    lines = output.splitlines()
    blocks: list[str] = []
    for i, line in enumerate(lines):
        if re.search(rf"(?<!\S){re.escape(check)}(?=\s|$)", line) and line.startswith(
            ("FAULT ", "OVER ", "---- ")
        ):
            block = [line]
            if line.startswith(("FAULT ", "---- ")):
                for following in lines[i + 1 :]:
                    if following.startswith(
                        ("FAULT ", "OVER ", "---- ", "  show:", "...")
                    ):
                        break
                    block.append(following)
            blocks.append("\n".join(block))
    return "\n".join(blocks) if blocks else output
