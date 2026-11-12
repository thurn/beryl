"""A check run over its budget files one bead per script, and fails nothing (budget.py)."""

from __future__ import annotations

import json

from beryl_tools import budget, verdicts


class FakeBd(verdicts.Beads):
    """bd as budget.py uses it, in memory."""

    def __init__(self) -> None:
        self.issues: list[dict[str, object]] = []

    def bd(self, *args: str) -> tuple[int, str]:
        if args[:3] == ("list", "--label", budget.LABEL):
            return 0, json.dumps(self.issues)
        if args[0] == "create":
            flag = lambda name: args[args.index(name) + 1]  # noqa: E731
            bead = f"by-{len(self.issues) + 1}"
            self.issues.append(
                {
                    "id": bead,
                    "status": "open",
                    "title": flag("--title"),
                    "description": flag("--description"),
                    "labels": flag("--labels").split(","),
                    "metadata": json.loads(flag("--metadata")),
                }
            )
            return 0, bead + "\n"
        return 1, ""


LANES = ["rust 120 s (fmt 1 s, clippy 1 s, tests 118 s)", "repo 2 s (just 0 s)"]


def test_an_overrun_files_a_bead_naming_its_lanes() -> None:
    store = FakeBd()
    line = budget.report("check", 60, "191 s", LANES, "load average 6", store)
    assert line == "check: over budget: filed by-1 to audit it"
    [issue] = store.issues
    assert issue["metadata"] == {budget.SCRIPT_KEY: "check"}
    assert budget.LABEL in issue["labels"]  # pyre-ignore[58]
    assert "tests 118 s" in str(issue["description"])


def test_an_open_bead_takes_later_overruns_of_its_script() -> None:
    store = FakeBd()
    budget.report("check", 60, "191 s", LANES, "", store)
    again = budget.report("check", 60, "200 s", LANES, "", store)
    assert again == "check: over budget: by-1 is open to audit it"
    budget.report("ci", 300, "400 s", LANES, "", store)
    assert [i["id"] for i in store.issues] == ["by-1", "by-2"]


def test_a_closed_bead_lets_the_next_overrun_file_again() -> None:
    store = FakeBd()
    budget.report("check", 60, "191 s", LANES, "", store)
    store.issues[0]["status"] = "closed"
    budget.report("check", 60, "191 s", LANES, "", store)
    assert len(store.issues) == 2
