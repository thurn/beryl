"""Browser checks wait long enough for a starved page when they run at low priority.

The browser sweep and the nightly run their checks at low priority (`system.utility`), where a
check gets only the CPU that agents' fast checks leave. At a load of 50 to 70, shots that take
2 s at normal priority ran past Playwright's default 30 s wait, passed on their rerun and were
filed as flaky checks to delete. `system.utility` sets `BERYL_LOW_PRIORITY=1`, and
`hangContext` in ui/scripts/hang.mjs gives every wait in a context opened that way 3 minutes.
These tests hold both halves: the flag reaches the command, and no script opens a browser
context around the helper, where its waits would fall back to Playwright's 30 s.
"""

import re
from pathlib import Path

from beryl_tools import paths, system

SCRIPTS: Path = paths.UI / "scripts"


def test_utility_marks_low_priority() -> None:
    command = system.utility(["true"])
    if system.WINDOWS:
        assert system.LOW_ENV["BERYL_LOW_PRIORITY"] == "1"
    else:
        assert "BERYL_LOW_PRIORITY=1" in command


def test_every_context_comes_from_hang_context() -> None:
    direct = re.compile(r"\.newContext\(")
    offenders = [
        f"{path.name}:{n}"
        for path in sorted(SCRIPTS.glob("*.mjs"))
        if path.name != "hang.mjs"
        for n, line in enumerate(path.read_text().splitlines(), 1)
        if direct.search(line)
    ]
    assert not offenders, (
        "open browser contexts with hangContext (ui/scripts/hang.mjs), so their waits "
        f"tolerate a starved low-priority run: {offenders}"
    )
