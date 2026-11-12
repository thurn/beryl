import argparse
import json
import subprocess
from collections.abc import Iterable
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

from beryl_tools import paths, runlog, verdicts
from beryl_tools.rust_tests import slow_only_filter

NOW: datetime = datetime.now(timezone.utc)


def record(
    check: str,
    outcome: str = "pass",
    seconds: float = 1.0,
    script: str = "ci",
    commit: str = "new",
    days_ago: float = 1,
) -> dict[str, object]:
    return {
        "time": (NOW - timedelta(days=days_ago)).isoformat(timespec="seconds"),
        "script": script,
        "lane": check.split(":")[0],
        "check": check,
        "commit": commit,
        "outcome": outcome,
        "seconds": seconds,
    }


def rewrites(check: str, n: int) -> list[dict[str, object]]:
    """Stale updates the backfill logged (runlog.REWRITE)."""
    return [{**record(check, runlog.REWRITE, 0.0, "check"), "backfilled": True}] * n


def verdict(name: str, days_ago: float = 1, **more: object) -> verdicts.Entry:
    return {
        "time": (NOW - timedelta(days=days_ago)).isoformat(timespec="seconds"),
        "verdict": name,
        **more,
    }


class Store(verdicts.Beads):
    def __init__(self, history: dict[str, list[verdicts.Entry]]) -> None:
        self.found = history

    def checks(self) -> list[str]:
        return list(self.found)

    def history(self, checks: Iterable[str]) -> dict[str, list[verdicts.Entry]]:
        return {c: e for c, e in self.found.items() if c in set(checks)}


INTERACTIONS = """
const keptTitle = "kept \\"stale\\"";
// keep: guards the only test of the kept state
await check(
    `${keptTitle} (${viewport.width} wide)`,
    async (page) => {},
);
// keep:
await check("an empty reason keeps nothing", async (page) => {});
"""

LIB_RS = """
// keep: a product comment holds no test
fn product_kept() {}

#[cfg(test)]
mod tests {
    // keep: the one test of the engine's undo
    #[test]
    fn kept_test() {}
}
"""

SHOTS = {
    "shots": [
        {"name": "kept", "keep": "the only shot of the kept state"},
        {"name": "plain"},
    ]
}


@pytest.fixture
def repo(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    monkeypatch.setattr(paths, "REPO", tmp_path)
    subprocess.run(["git", "init", "-q"], cwd=tmp_path, check=True)
    (tmp_path / "ui" / "scripts").mkdir(parents=True)
    (tmp_path / "ui" / "scripts" / "interactions.mjs").write_text(INTERACTIONS)
    (tmp_path / "ui" / "shots.json").write_text(json.dumps(SHOTS))
    lib = tmp_path / "rust" / "crates" / "beryl_rules" / "src" / "lib.rs"
    lib.parent.mkdir(parents=True)
    lib.write_text(LIB_RS)
    return tmp_path


def prune(capsys: pytest.CaptureFixture[str]) -> dict[str, tuple[str, str, str]]:
    """Each acted-on check's (action, row, keep reason), from `ci-report --prune --json`."""
    args = argparse.Namespace(
        days=30, json=True, quarantined=False, pending=False, prune=True
    )
    assert runlog.report(args) == 0
    out = json.loads(capsys.readouterr().out)
    return {a["check"]: (a["action"], a["row"], a["keep"]) for a in out["actions"]}


def test_each_row_of_the_rule_with_keeps_pending_and_gone_checks(
    repo: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    flakes = [verdict("flaky", 1), verdict("flaky", 2)]
    history = {
        "ui-test:stale": [verdict("stale")],
        "ui-test:stale and caught long ago": [verdict("catch", 200)],
        "ui-test:quarantined": flakes,
        "ui-test:quarantined and caught": [verdict("catch", 90), *flakes],
        "ui-test:flaky once": flakes[:1],
        "ui-test:old flakes": [verdict("flaky", 1, backfilled=True)] * 2,
        "rust:beryl_rules::tests::kept_test": flakes,
        "rust:beryl_rules::product_kept": flakes,
    }
    monkeypatch.setattr(verdicts, "STORE", Store(history))
    pending = verdicts.Pending("ui-test:pending", "board", "by-x")
    monkeypatch.setattr(verdicts, "PENDING", (pending,))
    checks = [
        *history,
        'ui-test:kept "stale" (1600 wide)',
        "ui-test:an empty reason keeps nothing",
        "ui-test:pending",
        "shots:kept",
        "shots:plain",
    ]
    log = [record(c) for c in checks]
    log += [record("ui-test:gone", commit="old", days_ago=3)]
    log += rewrites("ui-test:stale", 2)
    log += rewrites("ui-test:stale and caught long ago", 3)
    for c in checks[-5:-1] + ["ui-test:gone"]:
        log += rewrites(c, 3)
    log += rewrites("shots:plain", 2)
    runlog.append(log)
    assert prune(capsys) == {
        "ui-test:stale": ("delete", "three stale updates and no catch", ""),
        "ui-test:an empty reason keeps nothing": (
            "delete",
            "three stale updates and no catch",
            "",
        ),
        'ui-test:kept "stale" (1600 wide)': (
            "keep",
            "three stale updates and no catch",
            "guards the only test of the kept state",
        ),
        "shots:kept": (
            "keep",
            "three stale updates and no catch",
            "the only shot of the kept state",
        ),
        "ui-test:quarantined": ("delete", "quarantined, never caught anything", ""),
        "rust:beryl_rules::product_kept": (
            "delete",
            "quarantined, never caught anything",
            "",
        ),
        "rust:beryl_rules::tests::kept_test": (
            "keep",
            "quarantined, never caught anything",
            "the one test of the engine's undo",
        ),
        "ui-test:quarantined and caught": ("fix", "quarantined, has caught a bug", ""),
    }


def test_the_costliest_tenth_of_the_fast_check_moves_unless_it_failed(
    repo: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    """Twenty fast-check tests and one already moved: the tenth is the moved one and t19. t18
    would be in a tenth of the twenty, as if every prune moved another tenth."""
    monkeypatch.setattr(verdicts, "STORE", Store({}))
    moved = verdicts.SlowOnly("rust:beryl_rules::moved", "2026-11-01", 100.0, 9)
    monkeypatch.setattr(verdicts, "SLOW_ONLY", (moved,))
    tests = [f"rust:beryl_rules::tests::t{i:02}" for i in range(20)]
    log = [record(t, seconds=500.0) for t in [*tests, moved.check]]
    for i, t in enumerate(tests):
        log += [record(t, seconds=float(i), script="check")] * 2
    log += [record("rust:fmt", seconds=999.0, script="check")]
    runlog.append(log)
    assert prune(capsys) == {
        tests[19]: ("slow-only", "never failed, in the costliest tenth", "")
    }
    runlog.append([record(tests[19], "fail", 19.0, script="check")])
    assert prune(capsys) == {}


def test_the_slow_only_filter_names_each_rust_test_by_its_package() -> None:
    assert slow_only_filter(
        [
            "rust:beryl_cli::bin/beryl_cli::positions::tests::a_seed",
            "rust:beryl_search::games",
            "rust:allocations",
            "ui-test:drag",
        ]
    ) == (
        r"(package(beryl_cli) & test(/^(bin\/beryl_cli::)?positions::tests::a_seed$/))"
        r" | (package(beryl_search) & test(/^games$/))"
    )
