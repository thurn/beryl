"""Concurrent reports and retries retain one durable owner and all evidence."""

from __future__ import annotations

import copy
import json
from collections.abc import Callable
from dataclasses import replace
from pathlib import Path
from typing import Any

import pytest

from beryl_tools import incidents, tickets


class Store:
    """The service's sequence-guarded batch contract, with injectable faults."""

    def __init__(self) -> None:
        self.seq = 0
        self.rows: dict[str, dict[str, Any]] = {}
        self.before_write: Callable[[], None] | None = None
        self.after_write: Callable[[], None] | None = None
        self.aliases: dict[str, str] = {}

    def snapshot(self) -> tuple[int, list[dict[str, Any]]]:
        return self.seq, copy.deepcopy(list(self.rows.values()))

    def show(self, issue: str) -> dict[str, Any]:
        return copy.deepcopy(self.rows[self.aliases.get(issue, issue)])

    def apply(self, seq: int, operations: list[dict[str, Any]]) -> dict[str, Any]:
        if self.before_write:
            hook, self.before_write = self.before_write, None
            hook()
        if seq != self.seq:
            raise tickets.TicketConflict("sequence changed")
        staged = copy.deepcopy(self.rows)
        results: list[dict[str, str]] = []
        created = ""
        for op in operations:
            if op["op"] == "create":
                created = issue = f"Ttest.{len(staged) + 1}"
                staged[issue] = {
                    "id": issue,
                    "status": "open",
                    "comments": [],
                    **op["fields"],
                    "parent": op.get("parent"),
                }
            else:
                issue = created if op["id"] == "$0" else op["id"]
                row = staged[issue]
                if op["op"] == "comment":
                    row["comments"].append({"text": op["text"]})
                else:
                    row["metadata"].update(op.get("set_meta", {}))
                    row["labels"] = list(
                        set(row["labels"]) | set(op.get("add_labels", []))
                    )
            results.append({"id": issue})
        self.rows = staged
        self.seq += 1
        if self.after_write:
            hook, self.after_write = self.after_write, None
            hook()
        return {"seq": self.seq, "results": results}


def sighting(
    host: str = "jarvis",
    run: str = "nightly:commit-a",
    output: str = "error: expected a legal move",
) -> incidents.Sighting:
    return incidents.Sighting(
        incidents.failure("rules:legal-moves", output),
        run,
        host,
        {
            "title": "Fix a rules failure",
            "body": output,
            "priority": 1,
            "labels": ["nightly-failure", "role:steward"],
            "metadata": {},
        },
    )


def test_two_hosts_racing_keep_one_owner_and_both_outputs() -> None:
    store: Store = Store()
    second: incidents.Sighting = sighting(
        "cygnus", output="error: expected a legal move\ncygnus output"
    )

    # The diagnostic excludes non-diagnostic run output, so both observations
    # describe the same failure while preserving their complete evidence.
    def other_host() -> None:
        incidents.report(store, second)

    store.before_write = other_host
    result = incidents.report(store, sighting())
    [row] = store.rows.values()
    assert result.issue == row["id"]
    assert "cygnus output" in row["body"]
    [comment] = row["comments"]
    assert "jarvis" in comment["text"]
    assert "cygnus" in row["body"]


def test_uncertain_response_and_restart_do_not_duplicate_owner_or_evidence() -> None:
    store = Store()

    def lost() -> None:
        raise tickets.TicketError("response lost after commit")

    store.after_write = lost
    first = incidents.report(store, sighting())
    assert incidents.report(store, sighting()).issue == first.issue
    [row] = store.rows.values()
    assert row["comments"] == []
    # A lost response to a repeat append is safe too.
    store.after_write = lost
    incidents.report(store, sighting("cygnus"))
    assert len(store.rows) == 1
    assert len(store.show(first.issue)["comments"]) == 1


def test_closed_owner_keeps_old_retry_but_new_run_creates_linked_incident() -> None:
    store = Store()
    first = incidents.report(store, sighting())
    store.rows[first.issue]["status"] = "closed"
    store.seq += 1
    assert incidents.report(store, sighting()).issue == first.issue
    new = incidents.report(store, sighting(run="nightly:commit-b"))
    assert new.created and new.issue != first.issue
    assert store.rows[new.issue]["metadata"][incidents.PREVIOUS] == [first.issue]


def test_closed_duplicate_follows_alias_to_open_canonical_owner() -> None:
    store = Store()
    first = incidents.report(store, sighting())
    store.rows["Towner"] = {
        "id": "Towner",
        "status": "open",
        "labels": [],
        "metadata": {},
        "comments": [],
    }
    store.aliases["old-owner-name"] = "Towner"
    duplicate = store.rows[first.issue]
    duplicate["status"] = "closed"
    duplicate["metadata"][incidents.OWNERS] = ["old-owner-name"]
    store.seq += 1
    result = incidents.report(store, sighting("cygnus"))
    assert result.issue == "Towner" and not result.created
    assert len(store.rows) == 2
    assert store.rows["Towner"]["metadata"][incidents.KEYS] == [
        sighting().identity.key()
    ]
    assert "cygnus" in store.rows["Towner"]["comments"][0]["text"]


def test_new_signature_platform_and_environment_faults_remain_distinct() -> None:
    store = Store()
    first = incidents.report(store, sighting())
    changed = sighting(output="error: expected an empty stack")
    assert incidents.report(store, changed).issue != first.issue
    environment = replace(
        sighting(), identity=replace(sighting().identity, category="cannot-run")
    )
    assert incidents.report(store, environment).issue not in {first.issue, "Ttest.2"}
    windows = replace(
        sighting(), identity=replace(sighting().identity, platform="win32")
    )
    assert incidents.report(store, windows).created
    assert len(store.rows) == 4


def test_renamed_steps_share_signature_without_merging_assertion_operands(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        incidents,
        "config",
        lambda: {
            "aliases": {"old-step": "stable-workload", "new-step": "stable-workload"}
        },
    )
    a = incidents.failure(
        "old-step",
        "thread 'main' (123) panicked at src/a.rs:12:3:\nassertion failed\nleft: 1\nright: 2",
    )
    b = incidents.failure(
        "new-step",
        "thread 'main' (789) panicked at src/a.rs:34:8:\nassertion failed\nleft: 1\nright: 2",
    )
    assert a.key() == b.key()
    assert (
        a.key()
        != replace(b, diagnostic=b.diagnostic.replace("right: 2", "right: 3")).key()
    )


def test_product_epic_and_global_steward_override_configured_infrastructure_route(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(incidents, "config", lambda: {"parent": "Tinfrastructure"})
    fields = sighting().fields
    routed, parent = incidents.route(fields, "")
    assert parent == "Tinfrastructure" and "role:executor" in routed["labels"]
    assert incidents.route(fields, "Tproduct")[1] == "Tproduct"
    assert incidents.route({**fields, "priority": 0}, "Tproduct")[1] == ""


def test_native_fault_output_keeps_the_workload_and_its_details() -> None:
    output = 'FAULT text-overlap b3d:one/you/1440x900 t=1ms\n  text "A" overlaps "B"\n  show: command\nFAULT off-screen b3d:two/you/1440x900 t=2ms\n  text "C" outside viewport'
    one = incidents.check_output("b3d:one/you/1440x900", output)
    assert '"A" overlaps "B"' in one and "off-screen" not in one
    assert incidents.check_output("unknown", output) == output


def test_legacy_report_requires_matching_diagnostic_before_adopting_owner() -> None:
    store = Store()
    output = "error: expected a legal move"
    store.rows["Tlegacy"] = {
        "id": "Tlegacy",
        "status": "open",
        "labels": ["nightly-failure"],
        "metadata": {"beryl_nightly_checks": "rules:legal-moves"},
        "body": f"Old nightly\nOutput tail:\n```\n{output}\n```",
        "comments": [],
    }
    assert incidents.report(store, sighting()).issue == "Tlegacy"
    assert incidents.report(store, sighting(output="error: new illegal target")).created
    assert len(store.rows) == 2


def test_migration_assigns_only_diagnostics_with_one_reviewed_owner(
    tmp_path: Path,
) -> None:
    from beryl_tools import incident_migration

    store = Store()
    store.rows["Tsource"] = {
        "id": "Tsource",
        "status": "closed",
        "labels": [],
        "comments": [],
        "metadata": {"beryl_nightly_checks": "rules:legal-moves"},
        "body": "Output tail:\n```\nerror: expected a legal move\n```",
    }
    for owner in ("Towner", "Tother"):
        store.rows[owner] = {
            "id": owner,
            "status": "open",
            "labels": [],
            "metadata": {},
            "comments": [],
        }
    file = tmp_path / "transfers.json"
    file.write_text(
        json.dumps(
            {
                "transfers": [
                    {
                        "source": "Tsource",
                        "owners": [
                            {"id": "Towner", "diagnostic": "legal move"},
                            {"id": "Tother", "diagnostic": "new target"},
                        ],
                    }
                ]
            }
        )
    )
    preview = incident_migration.migrate(store, file)
    assert preview["dry_run"] and not store.rows["Towner"]["metadata"]
    incident_migration.migrate(store, file, apply=True)
    assert store.rows["Tsource"]["metadata"][incidents.OWNERS] == ["Towner", "Tother"]
    assert incidents.report(store, sighting()).issue == "Towner"
    assert incidents.report(store, sighting(output="error: new illegal target")).created


def test_alias_added_after_an_incident_was_filed_keeps_its_owner(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    store = Store()
    original = incidents.report(store, sighting())
    monkeypatch.setattr(
        incidents,
        "config",
        lambda: {"aliases": {"rules:legal-moves": "rules:legal-actions"}},
    )
    renamed = replace(
        sighting("cygnus"),
        identity=replace(sighting().identity, workload="rules:legal-actions"),
    )
    assert incidents.report(store, renamed).issue == original.issue
    assert len(store.rows) == 1


def test_native_fault_identity_does_not_include_another_viewport() -> None:
    output = 'FAULT text-overlap b3d:one/you/1440x900 t=1ms\n  text "A" overlaps "B"\nFAULT off-screen b3d:one/you/1440x900@2 t=2ms\n  text "C" outside viewport'
    assert "off-screen" not in incidents.check_output("b3d:one/you/1440x900", output)


def test_different_native_fault_details_are_different_signatures() -> None:
    header = "FAULT text-overlap b3d:one/you/1440x900 t=1ms\n"
    one = incidents.failure(
        "b3d:one/you/1440x900", header + '  text "Combat" overlaps "-3"'
    )
    another = incidents.failure(
        "b3d:one/you/1440x900",
        header + '  text "Your library" overlaps "Your graveyard"',
    )
    assert one.key() != another.key()
