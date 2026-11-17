"""The findings rules measured in dollars (docs/efficiency-dashboard.md, "Findings"):
wake-ups, fixed-prefix, carried-logs, small-reads, tier-gap and outlier-beads."""

from __future__ import annotations

import bisect
from collections import defaultdict

from .facts import Facts
from .finding import (
    DOLLARS,
    MIN_COHORT,
    Context,
    Item,
    Rule,
    bounds,
    cohort_of,
    grouped,
    median,
    percentile,
    role_of,
    top,
    total_cost,
    total_saving,
    wanted,
    window_role,
)
from .scope import Scope

# A tool result larger than this many tokens is a carried log.
CARRIED_LOG_TOKENS = 2000
# Consecutive read-only requests from this many on are a run of small reads.
SMALL_READ_RUN = 3
# The kinds of tool call that only read: shell reads and searches, and the read tools.
READ_KINDS: frozenset[str] = frozenset({"read and search", "Read", "Grep", "Glob"})
# Each tier's cohort needs this many tickets before tiers are compared.
TIER_COHORT = 20
# A ticket costing more than this many times its cohort's median is an outlier.
OUTLIER_FACTOR = 5.0


def money(value: float | None) -> str:
    return "unknown" if value is None else f"${value:,.2f}"


def wake_ups(c: Context) -> list[Item]:
    """The dollars of wake-up requests in the range's windows, by role."""
    rows: list[tuple[str, str | None, float, float, float | None]] = []
    for w in c.facts.chosen(c.scope):
        spent = w.dollars.get(("purpose", "wake-up"), (0.0, 0.0))[1]
        if spent > 0:
            rows.append((window_role(c.facts, w), w.bead, spent, 0.0, spent))
    return grouped(rows)


def priced_requests(
    c: Context,
) -> list[tuple[str, str, str, str, str, float, float, int, int, str | None]]:
    """The priced requests in the scope: host, provider, transcript, message, time,
    dollars, input-side dollars, context tokens, prefix tokens and ticket, oldest first.
    """
    clause, given = bounds("time", c.scope)
    keep = wanted(c.facts, c.scope)
    return [
        (
            str(r[0]),
            str(r[1]),
            str(r[2]),
            str(r[3]),
            str(r[4]),
            float(r[5]),
            float(r[5]) - float(r[6] or 0.0),
            int(r[7] or 0),
            int(r[8] or 0),
            r[9],
        )
        for r in c.db.execute(
            f"""SELECT host, provider, transcript, message, time, dollars, output_dollars,
                context_tokens, prefix_tokens, bead
            FROM requests WHERE dollars IS NOT NULL{clause}
            ORDER BY host, provider, transcript, time, message""",
            given,
        )
        if keep(r[9])
    ]


def fixed_prefix(c: Context) -> list[Item]:
    """Each request's dollars for its fixed prefix, by role, against the same request with
    a prefix at the 10th percentile of that role's transcripts in the range."""
    requests = priced_requests(c)
    prefix: dict[tuple[str, str, str], int] = defaultdict(int)
    roles: dict[tuple[str, str, str], str] = {}
    for host, provider, transcript, *_, fixed, bead in requests:
        key = (host, provider, transcript)
        prefix[key] = max(prefix[key], fixed)
        roles.setdefault(key, role_of(c.facts, bead))
    sizes: dict[str, list[float]] = defaultdict(list)
    for key, size in prefix.items():
        if size:
            sizes[roles[key]].append(float(size))
    low = {role: percentile(found, 0.1) for role, found in sizes.items()}
    rows: list[tuple[str, str | None, float, float, float | None]] = []
    for host, provider, transcript, _, _, _, inputs, context, fixed, bead in requests:
        key = (host, provider, transcript)
        size = prefix[key]
        if not context or not fixed or not size:
            continue
        spent = inputs * fixed / context
        reference = spent * min(low[roles[key]] / size, 1.0)
        rows.append((roles[key], bead, spent, reference, spent - reference))
    return grouped(rows)


def carried_logs(c: Context) -> list[Item]:
    """The carrying cost of tool results over 2,000 tokens, by command kind, against the
    same results at 2,000 tokens each."""
    clause, given = bounds("start", c.scope)
    keep = wanted(c.facts, c.scope)
    rows: list[tuple[str, str | None, float, float, float | None]] = []
    for kind, added, carrying, bead in c.db.execute(
        f"""SELECT coalesce(kind, 'other'), added_tokens, carrying_dollars, bead
        FROM tool_calls WHERE added_tokens > ? AND carrying_dollars > 0{clause}""",
        (CARRIED_LOG_TOKENS, *given),
    ):
        if keep(bead):
            spent = float(carrying)
            reference = spent * CARRIED_LOG_TOKENS / float(added)
            rows.append((str(kind), bead, spent, reference, spent - reference))
    return grouped(rows)


def read_only(c: Context, keys: set[tuple[str, str, str]]) -> dict[str, bool]:
    """Whether each request (by host, provider, transcript and message) of `keys`'
    transcripts made tool calls, all of them reads. A call belongs to the first request at
    or after it, as token accounting assigns them."""
    found: dict[str, bool] = {}
    for host, provider, transcript in sorted(keys):
        given = (host, provider, transcript)
        requests = [
            (str(r[0]), str(r[1]))
            for r in c.db.execute(
                "SELECT time, message FROM requests WHERE host = ? AND provider = ? "
                "AND transcript = ? ORDER BY time, message",
                given,
            )
        ]
        times = [t for t, _ in requests]
        kinds: dict[int, list[str]] = defaultdict(list)
        for start, kind in c.db.execute(
            "SELECT start, coalesce(kind, 'other') FROM tool_calls WHERE host = ? "
            "AND provider = ? AND transcript = ?",
            given,
        ):
            owner = bisect.bisect_left(times, str(start))
            if owner < len(requests):
                kinds[owner].append(str(kind))
        for index, (_, message) in enumerate(requests):
            calls = kinds.get(index, [])
            found["\t".join((*given, message))] = bool(calls) and all(
                k in READ_KINDS for k in calls
            )
    return found


def small_reads(c: Context) -> list[Item]:
    """Runs of three or more consecutive requests whose only tool calls read, by role,
    against the cost of each run's last request: one request could have asked for it
    all."""
    requests = priced_requests(c)
    reads = read_only(c, {(r[0], r[1], r[2]) for r in requests})
    rows: list[tuple[str, str | None, float, float, float | None]] = []
    run: list[tuple[float, str | None]] = []
    last: tuple[str, str, str] | None = None

    def close() -> None:
        if len(run) >= SMALL_READ_RUN:
            spent = sum(d for d, _ in run)
            bead = run[-1][1]
            final = run[-1][0]
            rows.append((role_of(c.facts, bead), bead, spent, final, spent - final))
        run.clear()

    for host, provider, transcript, message, _, dollars, *_, bead in requests:
        key = (host, provider, transcript)
        if key != last:
            close()
            last = key
        if reads.get("\t".join((*key, message))):
            run.append((dollars, bead))
        else:
            close()
    close()
    return grouped(rows)


def bead_costs(facts: Facts, scope: Scope) -> dict[str, float]:
    """Each ticket closed in the scope with windows in the store: its direct dollars plus its
    rework."""
    windows = facts.bead_windows(scope)
    found = facts.rework()
    return {
        b.id: sum(w.total_dollars() for w in windows[b.id]) + found.dollars(b.id)
        for b in facts.closed(scope)
        if windows.get(b.id)
    }


def tier_gap(c: Context) -> list[Item]:
    """For each role, every tier dearer per ticket than the role's cheapest tier, against the
    same number of tickets at the cheapest tier's mean cost; only tiers with twenty tickets or
    more are compared."""
    costs = bead_costs(c.facts, c.scope)
    tiers: dict[str, dict[str, dict[str, float]]] = {}
    for bead, cost in costs.items():
        tier = c.facts.beads[bead].label("tier")
        if tier is not None:
            by_tier = tiers.setdefault(role_of(c.facts, bead), {})
            by_tier.setdefault(tier, {})[bead] = cost
    items: list[Item] = []
    for role, by_tier in sorted(tiers.items()):
        big = {t: beads for t, beads in by_tier.items() if len(beads) >= TIER_COHORT}
        if len(big) < 2:
            continue
        mean = {t: sum(b.values()) / len(b) for t, b in big.items()}
        cheap = min(mean, key=lambda t: (mean[t], t))
        for tier, beads in sorted(big.items()):
            if tier == cheap:
                continue
            spent = sum(beads.values())
            reference = len(beads) * mean[cheap]
            items.append(
                Item(
                    f"{role}: tier:{tier} over tier:{cheap}",
                    len(beads),
                    spent,
                    reference,
                    spent - reference,
                    {b: v - mean[cheap] for b, v in beads.items()},
                )
            )
    return sorted(items, key=lambda i: (-(i.saving or 0.0), i.name))


def outlier_beads(c: Context) -> list[Item]:
    """Tickets costing more than five times their cohort's median (same role and tier),
    against that median, by cohort."""
    costs = bead_costs(c.facts, c.scope)
    cohorts: dict[str, dict[str, float]] = defaultdict(dict)
    for bead, cost in costs.items():
        cohorts[cohort_of(c.facts, bead)][bead] = cost
    rows: list[tuple[str, str | None, float, float, float | None]] = []
    for name, beads in cohorts.items():
        if len(beads) < MIN_COHORT:
            continue
        middle = median(list(beads.values()))
        for bead, cost in beads.items():
            if cost > OUTLIER_FACTOR * middle:
                rows.append((name, bead, cost, middle, cost - middle))
    return grouped(rows)


def say_wake_ups(items: list[Item]) -> str:
    return (
        f"Wake-up requests, which find nothing to do and wait again, cost "
        f"{money(total_cost(items))} a week, most of it in {top(items)} sessions"
    )


def say_fixed_prefix(items: list[Item]) -> str:
    return (
        f"Fixed prefixes cost {money(total_cost(items))} a week; prefixes at each role's "
        f"10th percentile would save {money(total_saving(items))}, most in {top(items)}"
    )


def say_carried_logs(items: list[Item]) -> str:
    count = sum(i.count for i in items)
    return (
        f"{count} tool results over {CARRIED_LOG_TOKENS:,} tokens cost "
        f"{money(total_cost(items))} a week to read again; cut to {CARRIED_LOG_TOKENS:,} "
        f"tokens they would save {money(total_saving(items))}, most from {top(items)}"
    )


def say_small_reads(items: list[Item]) -> str:
    count = sum(i.count for i in items)
    return (
        f"{count} runs of three or more read-only requests cost "
        f"{money(total_cost(items))} a week; one request per run would save "
        f"{money(total_saving(items))}"
    )


def say_tier_gap(items: list[Item]) -> str:
    return (
        f"Tickets at dearer tiers cost {money(total_saving(items))} a week more than the "
        f"same tickets at their role's cheapest tier, direct cost plus rework; the largest "
        f"gap is {top(items)}, {money(items[0].saving if items else 0.0)}"
    )


def say_outlier_beads(items: list[Item]) -> str:
    count = sum(i.count for i in items)
    return (
        f"{count} tickets cost over {OUTLIER_FACTOR:g} times their cohort's median, "
        f"{money(total_saving(items))} a week above it"
    )


RULES: tuple[Rule, ...] = (
    Rule(
        "wake-ups",
        DOLLARS,
        "zero: no request wakes only to wait again",
        wake_ups,
        say_wake_ups,
        "efficiency dollars --by purpose",
    ),
    Rule(
        "fixed-prefix",
        DOLLARS,
        "the same requests with a fixed prefix at the 10th percentile of that role's "
        "transcripts in the range",
        fixed_prefix,
        say_fixed_prefix,
        "efficiency dollars --by role",
    ),
    Rule(
        "carried-logs",
        DOLLARS,
        f"the same tool results at {CARRIED_LOG_TOKENS:,} tokens each",
        carried_logs,
        say_carried_logs,
        "efficiency dollars --by source",
    ),
    Rule(
        "small-reads",
        DOLLARS,
        "the cost of the last request of each run of three or more consecutive requests "
        "whose only tool calls read (shell reads and searches, Read, Grep and Glob)",
        small_reads,
        say_small_reads,
        "efficiency time --by kind",
    ),
    Rule(
        "tier-gap",
        DOLLARS,
        "the dearer tier's tickets at the cheapest tier's mean direct cost plus rework, for "
        f"tiers of one role with at least {TIER_COHORT} closed tickets each",
        tier_gap,
        say_tier_gap,
        "efficiency cards --by tier",
    ),
    Rule(
        "outlier-beads",
        DOLLARS,
        f"the median direct cost plus rework of the ticket's cohort (role and tier, at least "
        f"{MIN_COHORT} tickets), for tickets over {OUTLIER_FACTOR:g} times it",
        outlier_beads,
        say_outlier_beads,
        "efficiency summary",
    ),
)
