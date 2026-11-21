"""Each ticket's estimated share of a subscription plan.

Nobody could say how much of the plan a ticket used. The store prices every Claude request
at API rates with its time, and a Claude dispatcher samples the plan's usage (`plan_usage`,
from `ignite.PlanSamples`): for each window of the plan, such as `5-hour`, `Weekly · all
models` or `Weekly · Fable`, the percent used and when the window resets. This module fits
the two together.

- A period is one window between two resets: the samples of one label with the same
  `resets_at`. A window whose label names its length (`5-hour`, `Weekly`) covers that long
  before its reset; one that does not covers its first sample to its last.
- Each period's factor, percent per dollar, is fitted by least squares through the origin over
  its spans: the rise in percent between consecutive samples against the Claude dollars of
  requests in the same span. An all-models window counts every Claude request; a window named
  for a model family (`Weekly · Fable`) counts only requests whose model names that family.
- The fit is noisy wherever Claude usage happens that the store has no transcript for, such as
  on a phone or on another machine. `Period.spread` is the factor's standard error, which the
  summary prints beside it rather than hiding it.
- Requests are priced by the factor of the period they fall in. While that period has no
  fitted factor yet (fewer than two samples), the latest earlier period's factor is used and
  the estimate is marked `borrowed`. A request before any period is not priced, and an
  estimate that would include one is unknown rather than too low.

Codex supplies its general weekly quota counter in retained rollouts. Fit those
samples to one host's requests, spanning at least two percentage points to reduce
the effect of rounding. Activity outside the host's Beryl logs can add noise.
"""

from __future__ import annotations

import bisect
import math
import re
import sqlite3
import statistics
from collections import defaultdict
from collections.abc import Iterable
from dataclasses import dataclass
from datetime import datetime, timezone

from .scope import local_time
from .timing import seconds

# The separator between a window's length and the models it counts, as in `Weekly · Fable`.
SEPARATOR = "·"
ALL_MODELS = "all models"
# Two samples of one window whose `resets_at` differ by less than this are one period: the
# app may report a reset time that moves by a few seconds between samples.
SAME_RESET_SECONDS = 600.0
# A sample whose percent falls by more than this below the previous one starts a new period
# even when neither reports a reset time.
RESET_DROP = 0.5
# Requests this long before the first sample are loaded, so that a weekly period's first days
# are priced by its factor.
LOOKBACK_SECONDS = 8 * 86400.0
CODEX_WEEKLY = "Weekly"
CODEX_MINIMUM_RISE = 2.0

# A request as the estimates read it: its time in epoch seconds, model and dollars.
Spent = tuple[float, str, float]


def family_of(window: str) -> str | None:
    """The model family a window counts, lowercased, or None when it counts every model."""
    _, separator, rest = window.partition(SEPARATOR)
    family = rest.strip().lower()
    if not separator or not family or family == ALL_MODELS:
        return None
    return family


def counts(family: str | None, model: str) -> bool:
    """Whether a request of `model` counts towards a window of `family`."""
    return family is None or family in model.lower()


def length_of(window: str) -> float | None:
    """How many seconds a window covers before its reset, when its label says."""
    head = window.partition(SEPARATOR)[0].strip().lower()
    hours = re.match(r"(\d+(?:\.\d+)?)\s*-?\s*h(?:ours?|r)?\b", head)
    if hours:
        return float(hours.group(1)) * 3600
    if head.startswith("weekly"):
        return 7 * 86400.0
    if head.startswith("daily"):
        return 86400.0
    return None


@dataclass(frozen=True)
class Sample:
    at: float
    percent: float
    resets: float | None


@dataclass
class Period:
    """One window between two resets, with the factor fitted over its spans."""

    window: str
    resets: float | None
    first: float
    last: float
    samples: int
    spans: int
    factor: float | None
    spread: float | None

    def covers(self, at: float) -> bool:
        length = length_of(self.window)
        resets = self.resets
        if resets is not None and length is not None:
            return resets - length <= at < resets
        return self.first <= at <= self.last

    def start(self) -> float:
        length = length_of(self.window)
        if self.resets is not None and length is not None:
            return self.resets - length
        return self.first


@dataclass(frozen=True)
class Estimate:
    """A share of one window of the plan: percent of it used, and whether any of it was
    priced by an earlier period's factor."""

    window: str
    percent: float
    borrowed: bool


def fit(rises: list[tuple[float, float]]) -> tuple[float | None, float | None]:
    """The slope through the origin of (dollars, rise) pairs and its standard error."""
    xx = sum(d * d for d, _ in rises)
    if xx <= 0:
        return None, None
    factor = float(sum(d * r for d, r in rises)) / xx
    if len(rises) < 2:
        return factor, None
    residual = sum((r - factor * d) ** 2 for d, r in rises)
    return factor, math.sqrt(float(residual) / (len(rises) - 1) / xx)


class Spending:
    """Requests in time order, with running sums per model family, so that the dollars
    between two times are two lookups."""

    def __init__(self, requests: list[Spent], families: Iterable[str | None]) -> None:
        self.requests: list[Spent] = sorted(requests)
        self.times: list[float] = [r[0] for r in self.requests]
        self.sums: dict[str | None, list[float]] = {}
        for family in set(families):
            running = [0.0]
            for _, model, dollars in self.requests:
                running.append(running[-1] + (dollars if counts(family, model) else 0))
            self.sums[family] = running

    def between(self, low: float, high: float, family: str | None) -> float:
        """The dollars of requests after `low` and at or before `high`."""
        sums = self.sums[family]
        return (
            sums[bisect.bisect_right(self.times, high)]
            - sums[bisect.bisect_right(self.times, low)]
        )


def split(samples: list[Sample], *, rounded: bool = False) -> list[list[Sample]]:
    """One window's samples, in time order, as periods."""
    found: list[list[Sample]] = []
    for sample in samples:
        if found:
            previous = found[-1][-1]
            same_reset = (
                previous.resets is None
                or sample.resets is None
                or abs(previous.resets - sample.resets) < SAME_RESET_SECONDS
            )
            # Concurrent Codex requests can report the previous integer quota tick
            # after a newer one has arrived.
            if rounded and same_reset and 0 < previous.percent - sample.percent <= 1:
                continue
            if same_reset and sample.percent >= previous.percent - RESET_DROP:
                found[-1].append(sample)
                continue
        found.append([sample])
    return found


def sample_pairs(
    group: list[Sample], minimum_rise: float
) -> Iterable[tuple[Sample, Sample]]:
    """Nonoverlapping spans within one quota period."""
    start = group[0]
    for end in group[1:]:
        if minimum_rise == 0 or end.percent - start.percent >= minimum_rise:
            yield start, end
            start = end


class Plan:
    """The fitted periods of every window of the plan."""

    def __init__(
        self,
        samples: dict[str, list[Sample]],
        requests: list[Spent],
        *,
        rounded_percent: bool = False,
        unpriced: list[float] | None = None,
    ) -> None:
        missing = sorted(unpriced or [])
        minimum_rise = CODEX_MINIMUM_RISE if rounded_percent else 0.0
        self.spending = Spending(requests, [family_of(w) for w in samples] + [None])
        self.periods: dict[str, list[Period]] = {}
        for window, found in samples.items():
            family = family_of(window)
            periods: list[Period] = []
            for group in split(
                sorted(found, key=lambda s: s.at), rounded=rounded_percent
            ):
                rises = [
                    (self.spending.between(a.at, b.at, family), b.percent - a.percent)
                    for a, b in sample_pairs(group, minimum_rise)
                    if bisect.bisect_right(missing, a.at)
                    == bisect.bisect_right(missing, b.at)
                ]
                rises = [(d, r) for d, r in rises if d > 0 or r != 0]
                factor, spread = fit(rises)
                resets = [s.resets for s in group if s.resets is not None]
                periods.append(
                    Period(
                        window,
                        resets[-1] if resets else None,
                        group[0].at,
                        group[-1].at,
                        len(group),
                        len(rises),
                        factor,
                        spread,
                    )
                )
            self.periods[window] = periods

    def primary(self) -> str | None:
        """The window an estimate is given against when only one is shown: the longest
        window that counts every model, which is the weekly limit."""
        found = [w for w in self.periods if family_of(w) is None]
        return max(found, key=lambda w: (length_of(w) or 0, w), default=None)

    def factor(self, window: str, at: float) -> tuple[float, bool] | None:
        """The factor for a request at `at`, and whether it was borrowed from an earlier
        period; None before any period was fitted."""
        periods = self.periods.get(window, [])
        own = next((p for p in periods if p.covers(at)), None)
        if own is not None and own.factor is not None:
            return own.factor, False
        earlier = [
            p
            for p in periods
            if p.factor is not None and p.start() <= at and p is not own
        ]
        if not earlier:
            return None
        factor = earlier[-1].factor
        assert factor is not None
        return factor, True

    def estimate(self, window: str, requests: Iterable[Spent]) -> Estimate | None:
        """The percent of `window` that `requests` used; None when any request that counts
        towards it falls before every fitted period."""
        family = family_of(window)
        percent = 0.0
        borrowed = False
        for at, model, dollars in requests:
            if not dollars or not counts(family, model):
                continue
            found = self.factor(window, at)
            if found is None:
                return None
            percent += dollars * found[0]
            borrowed = borrowed or found[1]
        return Estimate(window, percent, borrowed)

    def estimates(self, requests: list[Spent]) -> list[Estimate]:
        """The estimate against every window that has one."""
        found = (self.estimate(w, requests) for w in sorted(self.periods))
        return [e for e in found if e is not None]

    def unsettled_since(self) -> str | None:
        """The start of the earliest period whose factor may still change: the latest period
        of each window. Windows that ended after it are estimated again at each run."""
        starts = [p[-1].start() for p in self.periods.values() if p]
        if not starts:
            return None
        moment = datetime.fromtimestamp(min(starts), timezone.utc)
        return moment.isoformat(timespec="milliseconds").replace("+00:00", "Z")


def requests_of(
    db: sqlite3.Connection,
    where: str = "",
    params: tuple[object, ...] = (),
    *,
    provider: str = "claude",
) -> list[Spent]:
    """The provider's priced requests, as (time, model, dollars), filtered by `where`."""
    return [
        (seconds(str(t)), str(m or ""), float(d))
        for t, m, d in db.execute(
            "SELECT time, model, dollars FROM requests "
            f"WHERE provider = ? AND dollars > 0 {where}",
            (provider, *params),
        )
    ]


def load(db: sqlite3.Connection) -> Plan:
    """Every sample in the store, fitted to the Claude requests around them."""
    samples: dict[str, list[Sample]] = defaultdict(list)
    for at, window, percent, resets in db.execute(
        "SELECT at, window, percent_used, resets_at FROM plan_usage ORDER BY at"
    ):
        try:
            reset = seconds(str(resets)) if resets else None
        except ValueError:
            reset = None
        samples[str(window)].append(Sample(seconds(str(at)), float(percent), reset))
    if not samples:
        return Plan({}, [])
    first = min(s.at for found in samples.values() for s in found) - LOOKBACK_SECONDS
    since = datetime.fromtimestamp(first, timezone.utc).isoformat(timespec="seconds")
    return Plan(dict(samples), requests_of(db, "AND time >= ?", (since[:19],)))


def load_codex(db: sqlite3.Connection, host: str) -> Plan:
    """Fit the weekly Codex quota samples to the requests collected on this host."""
    rows = db.execute(
        "SELECT at, percent_used, resets_at FROM codex_plan_usage WHERE host = ? ORDER BY at",
        (host,),
    ).fetchall()
    if not rows:
        return Plan({}, [])
    requests: list[Spent] = []
    unpriced: list[float] = []
    for at, model, dollars in db.execute(
        "SELECT time, model, dollars FROM requests "
        "WHERE host = ? AND provider = 'codex' AND time > ?",
        (host, rows[0][0]),
    ):
        if dollars is None:
            unpriced.append(seconds(str(at)))
        elif dollars > 0:
            requests.append((seconds(str(at)), str(model or ""), float(dollars)))
    return Plan(
        {CODEX_WEEKLY: [Sample(seconds(str(at)), p, reset) for at, p, reset in rows]},
        requests,
        rounded_percent=True,
        unpriced=unpriced,
    )


@dataclass
class Figure:
    """The summary's plan figures against the weekly limit (`window`): each closed ticket's
    percent, as its median and 95th percentile over the `count` tickets with an estimate
    (`borrowed` of them priced in part by an earlier period's factor); the factor of the
    latest fitted period with its standard error; and the pool's percent per hour on its
    busiest local day in the range."""

    window: str
    median: float | None
    p95: float | None
    count: int
    borrowed: int
    factor: float | None
    spread: float | None
    busiest_day: str | None
    per_hour: float | None


def rounded(value: float | None, digits: int) -> float | None:
    return None if value is None else round(value, digits)


def percentile(ordered: list[float], share: float) -> float:
    """The `share` quantile of sorted values, interpolated between neighbours."""
    position = share * (len(ordered) - 1)
    low = math.floor(position)
    high = min(low + 1, len(ordered) - 1)
    return ordered[low] + (ordered[high] - ordered[low]) * (position - low)


def ticket_requests(
    db: sqlite3.Connection, tickets: list[str]
) -> dict[str, list[Spent]]:
    """Each ticket's Claude requests on every host."""
    found: dict[str, list[Spent]] = defaultdict(list)
    for index in range(0, len(tickets), 500):
        chunk = tickets[index : index + 500]
        marks = ", ".join("?" for _ in chunk)
        for bead, t, m, d in db.execute(
            "SELECT bead, time, model, dollars FROM requests WHERE provider = 'claude' "
            f"AND dollars > 0 AND bead IN ({marks})",
            chunk,
        ):
            found[str(bead)].append((seconds(str(t)), str(m or ""), float(d)))
    return found


def figure(
    db: sqlite3.Connection,
    plan: Plan,
    tickets: list[str],
    since: str | None,
    until: str | None,
) -> Figure | None:
    """The plan figures of the tickets `tickets` and of every Claude request from `since` to
    `until`; None when the store holds no samples."""
    window = plan.primary()
    if window is None:
        return None
    by_ticket = ticket_requests(db, tickets)
    found = [plan.estimate(window, by_ticket.get(t, [])) for t in tickets]
    known = [e for e in found if e is not None and e.percent > 0]
    ordered = sorted(e.percent for e in known)
    fitted = [p for p in plan.periods[window] if p.factor is not None]
    by_day: dict[str, float] = defaultdict(float)
    for at, model, dollars in plan.spending.requests:
        moment = datetime.fromtimestamp(at, timezone.utc)
        text = moment.isoformat()[:19]
        if (since and text < since[:19]) or (until and text >= until[:19]):
            continue
        factor = plan.factor(window, at)
        if factor is not None:
            day = local_time(moment).strftime("%Y-%m-%d")
            by_day[day] += dollars * factor[0]
    busiest = max(by_day.items(), key=lambda item: item[1], default=None)
    return Figure(
        window,
        round(statistics.median(ordered), 3) if ordered else None,
        round(percentile(ordered, 0.95), 3) if ordered else None,
        len(known),
        sum(e.borrowed for e in known),
        rounded(fitted[-1].factor if fitted else None, 5),
        rounded(fitted[-1].spread if fitted else None, 5),
        busiest[0] if busiest else None,
        round(busiest[1] / 24, 3) if busiest else None,
    )


def text(f: Figure | None) -> list[str]:
    """The summary's line for the plan figures."""
    if f is None:
        return []

    def shown(value: float | None, digits: int) -> str:
        return "-" if value is None else f"{value:.{digits}f}"

    spread = f" ± {f.spread:.5f}" if f.spread is not None else ""
    return [
        f"plan      {f.window}: per ticket median {shown(f.median, 3)}%"
        f"   p95 {shown(f.p95, 3)}%   ({f.count} tickets, {f.borrowed} borrowed)"
        f"   {shown(f.factor, 5)}{spread} % per dollar"
        f"   busiest day {f.busiest_day or '-'} {shown(f.per_hour, 3)}% per hour"
    ]
