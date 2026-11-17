"""An in-memory ticket transport shared by reporter behavior tests."""

from __future__ import annotations

import copy
from typing import Any

from beryl_tools import incidents, tickets, verdicts


class ReporterTickets(verdicts.Tickets):
    def incident_enabled(self) -> bool:
        return True

    def incident_backend(self) -> incidents.Backend:
        return ReporterBackend(self)


class ReporterBackend:
    def __init__(self, store: ReporterTickets) -> None:
        self.store = store

    def rows(self) -> list[dict[str, Any]]:
        rows = getattr(self.store, "issues", [])
        rows = list(rows.values()) if isinstance(rows, dict) else list(rows)
        rows.extend(getattr(self.store, "filed", {}).values())
        return rows

    def snapshot(self) -> tuple[int, list[dict[str, Any]]]:
        rows = self.rows()
        return int(incidents.digest(rows)[:12], 16), copy.deepcopy(rows)

    def show(self, issue: str) -> dict[str, Any]:
        row = copy.deepcopy(next(row for row in self.rows() if row["id"] == issue))
        row["comments"] = [
            {"text": c} if isinstance(c, str) else c for c in row.get("comments", [])
        ]
        return row

    def apply(self, seq: int, operations: list[dict[str, Any]]) -> dict[str, Any]:
        if self.snapshot()[0] != seq:
            raise tickets.TicketConflict("changed")
        results: list[dict[str, str]] = []
        created = ""
        for op in operations:
            if op["op"] == "create":
                f = op["fields"]
                code, created = self.store.create_issue(
                    title=f["title"],
                    body=f["body"],
                    priority=str(f["priority"]),
                    acceptance=f.get("acceptance", ""),
                    labels=",".join(f["labels"]),
                    metadata=f.get("metadata"),
                    parent=op.get("parent", ""),
                )
                assert code == 0
                issue = created
            else:
                issue = created if op["id"] == "$0" else op["id"]
                row = next(row for row in self.rows() if row["id"] == issue)
                if op["op"] == "comment":
                    row.setdefault("comments", []).append(op["text"])
                else:
                    row["metadata"].update(op.get("set_meta", {}))
                    row["labels"] = list(
                        set(row["labels"]) | set(op.get("add_labels", []))
                    )
            results.append({"id": issue})
        return {"results": results}
