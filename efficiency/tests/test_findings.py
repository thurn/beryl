"""The findings rules over small stores built row by row: each rule fires on rows made to
trigger it and stays silent on rows made not to, its saving is the difference from its
reference worked out by hand, and the findings come out grouped by unit and sorted."""

from __future__ import annotations

import json
import sqlite3
from collections.abc import Iterator
from pathlib import Path

import pytest

from beryl_efficiency import facts, findings, store
from beryl_efficiency.finding import total_saving
from beryl_efficiency.findings import Finding
from beryl_efficiency.scope import Range, Scope

HOST = "jarvis"
PROVIDER = "claude"
WEEK = Scope(Range("2026-11-08T00:00:00", "2026-11-15T00:00:00", "7d"))

pytestmark = pytest.mark.usefixtures("utc_days")


def at(day: int, hour: int, minute: int = 0, second: int = 0) -> str:
    return f"2026-11-{day:02d}T{hour:02d}:{minute:02d}:{second:02d}.000Z"


@pytest.fixture
def db(tmp_path: Path) -> Iterator[sqlite3.Connection]:
    found = store.open_store(tmp_path / "store.sqlite")
    yield found
    found.close()


def bead(
    db: sqlite3.Connection,
    id: str,
    *labels: str,
    closed: str | None = at(10, 12),
    issue_type: str = "task",
    enact: dict[str, object] | None = None,
) -> None:
    db.execute(
        "INSERT INTO beads (id, title, issue_type, status, created_at, started_at, "
        "closed_at, enact) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        (
            id,
            f"Bead {id}",
            issue_type,
            "closed" if closed else "open",
            at(10, 9),
            at(10, 10),
            closed,
            json.dumps(enact or {}),
        ),
    )
    db.executemany(
        "INSERT INTO labels (bead, label) VALUES (?, ?)", [(id, l) for l in labels]
    )


def window(
    db: sqlite3.Connection,
    transcript: str,
    bead_id: str | None,
    start: str = at(10, 10),
    kind: str = "bead",
    time: dict[tuple[str, str], float] | None = None,
    dollars: dict[tuple[str, str], float] | None = None,
) -> None:
    db.execute(
        "INSERT INTO windows (host, provider, session, transcript, start, end, bead, kind) "
        "VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        (HOST, PROVIDER, f"s-{transcript}", transcript, start, None, bead_id, kind),
    )
    for (category, what), seconds in (time or {}).items():
        db.execute(
            "INSERT INTO window_time (host, provider, transcript, start, category, kind, "
            "seconds) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (HOST, PROVIDER, transcript, start, category, what, seconds),
        )
    for (split, name), spent in (dollars or {}).items():
        db.execute(
            "INSERT INTO window_dollars (host, provider, transcript, start, split, name, "
            "tokens, dollars) VALUES (?, ?, ?, ?, ?, ?, 0, ?)",
            (HOST, PROVIDER, transcript, start, split, name, spent),
        )


def request(
    db: sqlite3.Connection,
    transcript: str,
    message: str,
    moment: str,
    dollars: float,
    bead_id: str | None,
    output: float = 0.0,
    context: int = 0,
    prefix: int = 0,
) -> None:
    db.execute(
        "INSERT INTO requests (host, provider, transcript, message, time, dollars, "
        "output_dollars, context_tokens, prefix_tokens, bead) "
        "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        (
            HOST,
            PROVIDER,
            transcript,
            message,
            moment,
            dollars,
            output,
            context,
            prefix,
            bead_id,
        ),
    )


def call(
    db: sqlite3.Connection,
    transcript: str,
    id: str,
    start: str,
    end: str,
    kind: str,
    bead_id: str | None,
    added: int | None = None,
    carrying: float | None = None,
    compile_seconds: float | None = None,
    chained: int = 0,
) -> None:
    db.execute(
        "INSERT INTO tool_calls (host, provider, transcript, call, start, end, tool, "
        "kind, command, added_tokens, carrying_dollars, compile_seconds, chained, bead) "
        "VALUES (?, ?, ?, ?, ?, ?, 'Bash', ?, 'just x', ?, ?, ?, ?, ?)",
        (
            HOST,
            PROVIDER,
            transcript,
            id,
            start,
            end,
            kind,
            added,
            carrying,
            compile_seconds,
            chained,
            bead_id,
        ),
    )


def run(db: sqlite3.Connection, scope: Scope = WEEK) -> dict[str, Finding]:
    found = findings.compute(findings.Context(db, facts.load(db), scope))
    return {f.rule: f for f in found}


def measured(db: sqlite3.Connection, rule: str) -> float | None:
    """The rule's saving over `WEEK`, unrounded: a week's saving is the range's."""
    context = findings.Context(db, facts.load(db), WEEK)
    return total_saving(findings.BY_NAME[rule].measure(context))


def weekly(found: dict[str, Finding], rule: str) -> float | None:
    """The finding's saving per week, as the query prints it."""
    return found[rule].saving_per_week[found[rule].unit]


def test_wake_ups_cost_all_their_dollars(db: sqlite3.Connection) -> None:
    bead(db, "by-a", "role:executor")
    window(db, "t1", "by-a", dollars={("purpose", "wake-up"): 3.0})
    window(db, "t2", "by-a", dollars={("purpose", "working"): 5.0})
    found = run(db)
    assert measured(db, "wake-ups") == pytest.approx(3.0)
    assert found["wake-ups"].examples == ["by-a"]


def test_no_wake_ups_no_finding(db: sqlite3.Connection) -> None:
    bead(db, "by-a", "role:executor")
    window(db, "t1", "by-a", dollars={("purpose", "working"): 5.0})
    window(db, "t2", "by-a", at(1, 10), dollars={("purpose", "wake-up"): 3.0})
    assert "wake-ups" not in run(db)


def test_fixed_prefix_is_measured_against_the_role_s_10th_percentile(
    db: sqlite3.Connection,
) -> None:
    bead(db, "by-a", "role:executor")
    # Each request's input-side dollars are 0.8 over 4,000 tokens of context.
    request(db, "t1", "m1", at(10, 10), 1.0, "by-a", 0.2, 4000, 1000)
    request(db, "t2", "m2", at(10, 11), 1.0, "by-a", 0.2, 4000, 2000)
    # The 10th percentile of prefixes 1,000 and 2,000 is 1,100. t2's prefix costs
    # 0.8 * 2000 / 4000 = 0.40, and at 1,100 tokens 0.40 * 1100 / 2000 = 0.22. t1's
    # prefix is below the reference and saves nothing.
    assert measured(db, "fixed-prefix") == pytest.approx(0.40 - 0.22)


def test_one_prefix_per_role_saves_nothing(db: sqlite3.Connection) -> None:
    bead(db, "by-a", "role:executor")
    request(db, "t1", "m1", at(10, 10), 1.0, "by-a", 0.2, 4000, 2000)
    request(db, "t1", "m2", at(10, 11), 1.0, "by-a", 0.2, 4000, 2000)
    assert "fixed-prefix" not in run(db)


def test_carried_logs_are_measured_at_2000_tokens(db: sqlite3.Connection) -> None:
    bead(db, "by-a")
    call(db, "t1", "c1", at(10, 10), at(10, 11), "rust tests", "by-a", 8000, 0.8)
    call(db, "t1", "c2", at(10, 12), at(10, 13), "build", "by-a", 1500, 0.5)
    found = run(db)
    assert measured(db, "carried-logs") == pytest.approx(0.8 - 0.8 * 2000 / 8000)
    assert [i.name for i in found["carried-logs"].items] == ["rust tests"]


def test_small_logs_are_not_carried_logs(db: sqlite3.Connection) -> None:
    bead(db, "by-a")
    call(db, "t1", "c1", at(10, 10), at(10, 11), "rust tests", "by-a", 1999, 0.8)
    assert "carried-logs" not in run(db)


def reads(db: sqlite3.Connection, count: int) -> None:
    """`count` requests whose only tool call reads, then one that runs tests."""
    bead(db, "by-a")
    for n in range(count):
        moment = at(10, 10, n)
        request(db, "t1", f"m{n}", moment, 0.1 * (n + 1), "by-a")
        call(db, "t1", f"c{n}", moment, moment, "read and search", "by-a")
    moment = at(10, 10, count)
    request(db, "t1", "last", moment, 1.0, "by-a")
    call(db, "t1", "tests", moment, moment, "rust tests", "by-a")


def test_a_run_of_three_reads_costs_all_but_its_last_request(
    db: sqlite3.Connection,
) -> None:
    reads(db, 3)
    assert measured(db, "small-reads") == pytest.approx(0.1 + 0.2 + 0.3 - 0.3)


def test_two_reads_are_not_a_run(db: sqlite3.Connection) -> None:
    reads(db, 2)
    assert "small-reads" not in run(db)


def costed(db: sqlite3.Connection, id: str, dollars: float, *labels: str) -> None:
    bead(db, id, *labels)
    window(db, f"t-{id}", id, dollars={("class", "output"): dollars})


def test_a_dearer_tier_costs_its_beads_at_the_cheaper_mean(
    db: sqlite3.Connection,
) -> None:
    for n in range(20):
        costed(db, f"by-o{n}", 5.0, "role:executor", "tier:opus")
        costed(db, f"by-s{n}", 2.0, "role:executor", "tier:sonnet")
    found = run(db)
    assert measured(db, "tier-gap") == pytest.approx(20 * (5.0 - 2.0))
    assert found["tier-gap"].items[0].name == "executor: tier:opus over tier:sonnet"


def test_tiers_under_twenty_beads_are_not_compared(db: sqlite3.Connection) -> None:
    for n in range(20):
        costed(db, f"by-o{n}", 5.0, "role:executor", "tier:opus")
    for n in range(19):
        costed(db, f"by-s{n}", 2.0, "role:executor", "tier:sonnet")
    assert "tier-gap" not in run(db)


def test_an_outlier_costs_what_it_spent_above_its_cohort_s_median(
    db: sqlite3.Connection,
) -> None:
    for n in range(4):
        costed(db, f"by-{n}", 1.0, "role:executor", "tier:opus")
    costed(db, "by-big", 10.0, "role:executor", "tier:opus")
    found = run(db)
    assert measured(db, "outlier-beads") == pytest.approx(10.0 - 1.0)
    assert found["outlier-beads"].examples == ["by-big"]


def test_five_times_the_median_is_not_an_outlier(db: sqlite3.Connection) -> None:
    for n in range(4):
        costed(db, f"by-{n}", 1.0, "role:executor", "tier:opus")
    costed(db, "by-big", 5.0, "role:executor", "tier:opus")
    assert "outlier-beads" not in run(db)


def test_queued_and_waiting_on_a_person_count_all_their_hours(
    db: sqlite3.Connection,
) -> None:
    bead(db, "by-a")
    window(
        db,
        "t1",
        "by-a",
        time={("queued", "rust tests"): 1800.0, ("person", ""): 3600.0},
    )
    found = run(db)
    assert measured(db, "queued") == pytest.approx(0.5)
    assert measured(db, "waiting-on-person") == pytest.approx(1.0)


def test_no_queued_time_no_finding(db: sqlite3.Connection) -> None:
    bead(db, "by-a")
    window(db, "t1", "by-a", time={("tool", "rust tests"): 1800.0})
    found = run(db)
    assert "queued" not in found and "waiting-on-person" not in found


def test_compile_time_above_the_kind_s_10th_percentile(db: sqlite3.Connection) -> None:
    bead(db, "by-a")
    for n, spent in enumerate([100.0, 200.0, 1000.0]):
        call(db, "t1", f"c{n}", at(10, 10 + n), at(10, 10 + n, 30), "rust tests", None)
        db.execute(
            "UPDATE tool_calls SET compile_seconds = ? WHERE call = ?", (spent, f"c{n}")
        )
    # The 10th percentile of 100, 200 and 1,000 seconds is 120.
    assert measured(db, "compile-share") == pytest.approx((80 + 880) / 3600)


def test_equal_compile_times_save_nothing(db: sqlite3.Connection) -> None:
    for n in range(3):
        call(db, "t1", f"c{n}", at(10, 10 + n), at(10, 10 + n, 30), "build", None)
    db.execute("UPDATE tool_calls SET compile_seconds = 60")
    assert "compile-share" not in run(db)


def commands(db: sqlite3.Connection, repeats: int) -> None:
    """Five beads of one cohort; by-0 runs the tests `repeats` times, the others once,
    each run a minute long."""
    for b in range(5):
        bead(db, f"by-{b}", "role:executor", "tier:opus")
        for n in range(repeats if b == 0 else 1):
            start, end = at(10, 10 + b, n), at(10, 10 + b, n + 1)
            call(db, f"t{b}", f"c{b}-{n}", start, end, "rust tests", f"by-{b}")


def test_a_bead_that_repeats_a_command_is_measured_at_the_median_count(
    db: sqlite3.Connection,
) -> None:
    commands(db, 10)
    found = run(db)
    # Counts 10, 1, 1, 1, 1: the 90th percentile is 6.4 and the median 1. by-0 took ten
    # minutes, and one run at its mean of a minute would take one.
    assert measured(db, "repeated-command") == pytest.approx(9 * 60 / 3600)
    assert found["repeated-command"].examples == ["by-0"]


def test_equal_counts_are_not_repeats(db: sqlite3.Connection) -> None:
    commands(db, 1)
    assert "repeated-command" not in run(db)


def test_chained_commands_have_an_unknown_saving(db: sqlite3.Connection) -> None:
    call(db, "t1", "c1", at(10, 10), at(10, 11), "build", None, chained=1)
    found = run(db)
    assert measured(db, "chained-commands") is None
    assert found["chained-commands"].cost_per_week == 1.0


def test_no_chained_commands_no_finding(db: sqlite3.Connection) -> None:
    call(db, "t1", "c1", at(10, 10), at(10, 11), "build", None)
    assert "chained-commands" not in run(db)


def lease(db: sqlite3.Connection, cores: int, wanted: int, held: float) -> None:
    db.execute(
        "INSERT INTO leases (host, time, class, worktree, command, cores, wanted, "
        "waited_seconds, held_seconds, wait_start, wait_end) "
        "VALUES (?, ?, 'push', 'by-a', 'test-rust', ?, ?, 0, ?, ?, ?)",
        (HOST, at(10, 11), cores, wanted, held, at(10, 10), at(10, 10)),
    )


def test_a_narrow_lease_misses_its_missing_cores_while_held(
    db: sqlite3.Connection,
) -> None:
    bead(db, "by-a", enact={"enact_worktree": "/work/beryl/.worktrees/by-a"})
    lease(db, 2, 4, 1800.0)
    found = run(db)
    assert measured(db, "narrow-leases") == pytest.approx((4 - 2) * 1800 / 3600)
    assert found["narrow-leases"].examples == ["by-a"]


def test_a_full_lease_is_not_narrow(db: sqlite3.Connection) -> None:
    lease(db, 4, 4, 1800.0)
    assert "narrow-leases" not in run(db)


def checks_row(db: sqlite3.Connection, day: str, runs: int, cpu: float) -> None:
    db.execute(
        'INSERT INTO checks (day, host, script, lane, "check", first, runs, passes, '
        "fails, skips, seconds, cpu_seconds, measured_seconds) "
        "VALUES (?, ?, 'ci', 'lint', 'lint:names', ?, ?, ?, 0, 0, ?, ?, ?)",
        (day, HOST, f"{day}T10:00:00.000Z", runs, runs, cpu, cpu, cpu),
    )


def verdict(db: sqlite3.Connection, name: str, kind: str, steward: str | None) -> None:
    db.execute(
        'INSERT INTO verdicts (comment, "check", time, host, "commit", script, lane, '
        "verdict, steward) VALUES (?, ?, ?, ?, 'b2', 'ci', 'lint', ?, ?)",
        (f"v-{name}-{kind}", name, at(10, 11), HOST, kind, steward),
    )


def test_a_family_never_caught_in_100_runs_wastes_its_cpu(
    db: sqlite3.Connection,
) -> None:
    checks_row(db, "2026-11-01", 60, 3600.0)
    checks_row(db, "2026-11-10", 40, 7200.0)
    found = run(db)
    assert measured(db, "never-caught") == pytest.approx(7200 / 3600)
    assert "flake-cost" not in found


def test_a_family_that_caught_a_bug_is_not_never_caught(
    db: sqlite3.Connection,
) -> None:
    checks_row(db, "2026-11-01", 60, 3600.0)
    checks_row(db, "2026-11-10", 40, 7200.0)
    verdict(db, "lint:names", "catch", None)
    assert "never-caught" not in run(db)


def test_a_flaky_failure_costs_one_rerun(db: sqlite3.Connection) -> None:
    checks_row(db, "2026-11-10", 40, 7200.0)
    verdict(db, "lint:names", "flaky", None)
    # One rerun at the check's mean of 7,200 / 40 CPU seconds a run.
    assert measured(db, "flake-cost") == pytest.approx(180 / 3600)


def lane_run(db: sqlite3.Connection, script: str, fails: int, cpu: float) -> None:
    db.execute(
        'INSERT INTO lane_runs (host, script, worktree, "commit", tree, lane, start, '
        "end, checks, fails, seconds, cpu_seconds, measured_seconds) "
        "VALUES (?, ?, '', 'b2', 't', 'lint', ?, ?, 1, ?, ?, ?, ?)",
        (HOST, script, at(10, 11), at(10, 12), fails, cpu, cpu, cpu),
    )


def failed_range(db: sqlite3.Connection, kind: str) -> None:
    """A Steward bead for the range a1..b2, whose slow-CI run failed at a check with
    verdict `kind`, with an hour of failed slow CI and half an hour of bisection."""
    bead(db, "by-st", "role:steward", enact={"enact_ci_range": "a1..b2"})
    db.execute(
        "INSERT INTO commits (sha, time, parents) VALUES ('a1', ?, '[]')", (at(10, 9),)
    )
    db.execute(
        "INSERT INTO commits (sha, time, parents) VALUES ('b2', ?, '[\"a1\"]')",
        (at(10, 10),),
    )
    lane_run(db, "ci", 1, 3600.0)
    lane_run(db, "bisect", 0, 1800.0)
    verdict(db, "lint:names", kind, "by-st")


def test_a_miss_costs_its_bisection_and_failed_runs(db: sqlite3.Connection) -> None:
    failed_range(db, "miss")
    found = run(db)
    assert measured(db, "misses") == pytest.approx(1800 / 3600 + 3600 / 3600)
    assert found["misses"].examples == ["by-st"]


def test_a_failure_that_was_no_miss_is_not_counted(db: sqlite3.Connection) -> None:
    failed_range(db, "catch")
    assert "misses" not in run(db)


def test_findings_are_grouped_by_unit_and_sorted_by_saving(
    db: sqlite3.Connection,
) -> None:
    bead(db, "by-a", "role:executor")
    window(
        db,
        "t1",
        "by-a",
        time={("queued", "build"): 1800.0, ("person", ""): 7200.0},
        dollars={("purpose", "wake-up"): 3.0},
    )
    call(db, "t1", "c1", at(10, 10), at(10, 11), "rust tests", "by-a", 8000, 0.8)
    call(db, "t1", "c2", at(10, 12), at(10, 13), "build", "by-a", chained=1)
    lease(db, 2, 4, 1800.0)
    result = findings.report(db, facts.load(db), WEEK)
    groups = result.data["groups"]
    assert isinstance(groups, list)
    assert [g["unit"] for g in groups] == ["dollars", "agent_hours", "core_hours"]
    rules = [[f["rule"] for f in g["findings"]] for g in groups]
    assert rules == [
        ["wake-ups", "carried-logs"],
        ["waiting-on-person", "queued", "chained-commands"],
        ["narrow-leases"],
    ]
    evidence = "evidence: efficiency findings --rule wake-ups --since 7d"
    assert evidence in [line.strip() for line in result.lines]
    detail = findings.report(db, facts.load(db), WEEK, "carried-logs")
    assert any(line.startswith("rust tests") for line in detail.lines)


def test_figures_are_per_week(db: sqlite3.Connection) -> None:
    bead(db, "by-a", "role:executor")
    window(db, "t1", "by-a", dollars={("purpose", "wake-up"): 3.0})
    fortnight = Scope(Range("2026-11-01T00:00:00", "2026-11-15T00:00:00", "14d"))
    assert weekly(run(db, fortnight), "wake-ups") == 1.5
