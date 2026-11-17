"""`just efficiency findings`: rules that point at waste, ranked by what removing it would
save (docs/efficiency-dashboard.md, "Findings").

Each rule measures its cost and its reference, the level the cost would fall to without the
waste, over the scope's range, and the saving is the difference scaled to a week. Findings
are listed in three groups, one per unit (dollars, agent hours, core hours), each sorted by
saving; no unit is converted into another. A finding's `evidence` is the command that prints
the numbers behind it: `findings --rule <rule>`, which lists the rule's items with their
cost, reference and saving. Findings never file tickets.
"""

from __future__ import annotations

import sqlite3
import textwrap
from dataclasses import asdict, dataclass, field

from . import cpu_rules, dollar_rules, time_rules
from .facts import Facts
from .finding import (
    AGENT_HOURS,
    CORE_HOURS,
    DOLLARS,
    UNITS,
    Context,
    Item,
    Rule,
    examples,
    total_cost,
    total_saving,
)
from .render import WIDTH, Result, cell, describe, table
from .rollups import days_in
from .scope import Scope

RULES: tuple[Rule, ...] = dollar_rules.RULES + time_rules.RULES + cpu_rules.RULES
BY_NAME: dict[str, Rule] = {r.name: r for r in RULES}
# How each unit is named in the text.
UNIT_NAMES: dict[str, str] = {
    DOLLARS: "dollars",
    AGENT_HOURS: "agent hours",
    CORE_HOURS: "core hours",
}
# How many items a rule's detail lists.
DETAIL_ROWS = 30
# What each field of the result means, which the page shows on hover.
FIELDS: dict[str, str] = {
    "weeks": "the range's length in weeks; every figure is divided by it, so that it is "
    "per week",
    "groups": "the findings in three groups, one per unit, each sorted by saving",
    "rule": "the rule's name",
    "unit": "dollars, agent_hours or core_hours; no unit is converted into another",
    "summary": "what the rule found, in one sentence, with per-week figures",
    "saving_per_week": "the cost per week above the rule's reference, in its unit; null "
    "when the rule cannot estimate it",
    "cost_per_week": "the cost per week the rule measured, in its unit",
    "reference": "the level the cost would fall to if the waste were removed",
    "evidence": "the command that prints the numbers behind the finding",
    "see": "the query with the wider breakdown the rule draws on",
    "examples": "tickets with the largest share of the saving",
    "items": "the rule's lines, such as one role or one kind of command: their count, "
    "cost, reference and saving per week",
}


@dataclass
class Line:
    name: str
    count: int
    cost_per_week: float
    reference_per_week: float
    saving_per_week: float | None
    examples: list[str]


@dataclass
class Finding:
    rule: str
    unit: str
    summary: str
    saving_per_week: dict[str, float | None]
    cost_per_week: float
    reference: str
    evidence: str
    see: str
    examples: list[str]
    items: list[Line] = field(default_factory=list)


@dataclass
class Group:
    unit: str
    findings: list[Finding]


@dataclass
class Findings:
    since: str | None
    until: str | None
    range: str
    filters: str
    weeks: float | None
    groups: list[Group]


def digits(unit: str) -> int:
    return 2 if unit == DOLLARS else 1


def finding(rule: Rule, items: list[Item], weeks: float, flags: str) -> Finding:
    """The rule's finding from its items over the range, scaled to a week."""
    weekly = [i.scaled(1 / weeks) for i in items]
    places = digits(rule.unit)
    saving = total_saving(weekly)
    return Finding(
        rule.name,
        rule.unit,
        rule.summary(weekly),
        {rule.unit: None if saving is None else round(saving, places)},
        round(total_cost(weekly), places),
        rule.reference,
        f"efficiency findings --rule {rule.name} {flags}".strip(),
        f"{rule.see} {flags}".strip(),
        examples(i.beads for i in weekly),
        [
            Line(
                i.name,
                i.count,
                round(i.cost, places),
                round(i.reference, places),
                None if i.saving is None else round(i.saving, places),
                examples([i.beads]),
            )
            for i in weekly
        ],
    )


def compute(c: Context, rules: tuple[Rule, ...] = RULES) -> list[Finding]:
    """Every rule in `rules` that found something, each with all its items."""
    weeks = (days_in(c.facts, c.scope) or 7.0) / 7
    flags = c.scope.flags()
    found: list[Finding] = []
    for rule in rules:
        items = rule.measure(c)
        if not items:
            continue
        made = finding(rule, items, weeks, flags)
        # A rule whose saving rounds to nothing has found nothing; an unknown saving is
        # still a finding, such as a gap in measurement.
        saving = saving_of(made)
        if saving is None or saving > 0:
            found.append(made)
    return found


def saving_of(f: Finding) -> float | None:
    return f.saving_per_week[f.unit]


def ranked(found: list[Finding]) -> list[Group]:
    """The findings in one group per unit, largest saving first and unknown savings last."""
    return [
        Group(
            unit,
            sorted(
                (f for f in found if f.unit == unit),
                key=lambda f: (
                    saving_of(f) is None,
                    -(saving_of(f) or 0.0),
                    f.rule,
                ),
            ),
        )
        for unit in UNITS
    ]


def report(
    db: sqlite3.Connection, facts: Facts, scope: Scope, rule: str | None = None
) -> Result:
    c = Context(db, facts, scope)
    if rule is not None and rule not in BY_NAME:
        raise KeyError(f"no rule {rule!r}; the rules are {', '.join(BY_NAME)}")
    chosen = (BY_NAME[rule],) if rule is not None else RULES
    found = compute(c, chosen)
    days = days_in(c.facts, scope)
    result = Findings(
        scope.range.since,
        scope.range.until,
        scope.range.name,
        scope.describe(),
        None if days is None else round(days / 7, 2),
        ranked(found),
    )
    data = asdict(result)
    if rule is None:
        for group in data["groups"]:
            for f in group["findings"]:
                del f["items"]
    more = [f.evidence for g in result.groups for f in g.findings[:1]]
    if rule is not None and found:
        more = [found[0].see]
    return Result(
        "findings",
        data,
        detail_lines(result, BY_NAME[rule]) if rule is not None else lines(result),
        more,
        {**describe("since"), **FIELDS},
    )


def amount(value: float | None, unit: str) -> object:
    """A per-week figure as a table shows it: unknown is a dash."""
    return None if value is None else round(value, digits(unit))


def lines(f: Findings) -> list[str]:
    head = f"findings  since {f.range}, per week"
    if f.filters:
        head += f"  ({f.filters})"
    out = [head]
    for group in f.groups:
        out.append("")
        out.append(UNIT_NAMES[group.unit])
        if not group.findings:
            out.append("  nothing found")
            continue
        out.extend(
            "  " + row
            for row in table(
                ["rule", "saving", "cost"],
                [
                    [x.rule, amount(saving_of(x), x.unit), x.cost_per_week]
                    for x in group.findings
                ],
            )
        )
        for x in group.findings:
            out.extend(
                textwrap.wrap(
                    f"{x.rule}: {x.summary}",
                    WIDTH,
                    initial_indent="  ",
                    subsequent_indent="    ",
                )
            )
            out.append(f"    evidence: {x.evidence}")
            if x.examples:
                out.append(f"    examples: {', '.join(x.examples)}")
    return out


def detail_lines(f: Findings, rule: Rule) -> list[str]:
    found = next((x for g in f.groups for x in g.findings), None)
    head = f"findings --rule {rule.name}  since {f.range}, per week, in "
    head += UNIT_NAMES[rule.unit]
    out = [head, f"reference: {rule.reference}"]
    if found is None:
        out.append("nothing found")
        return out
    out.append(
        f"saving {cell(amount(saving_of(found), found.unit))}   "
        f"cost {cell(found.cost_per_week)}"
    )
    out.extend(textwrap.wrap(found.summary, WIDTH))
    out.append("")
    rows = found.items[:DETAIL_ROWS]
    out.extend(
        table(
            ["item", "count", "cost", "reference", "saving", "examples"],
            [
                [
                    i.name,
                    i.count,
                    i.cost_per_week,
                    i.reference_per_week,
                    amount(i.saving_per_week, found.unit),
                    ", ".join(i.examples),
                ]
                for i in rows
            ],
        )
    )
    if len(found.items) > DETAIL_ROWS:
        out.append(f"and {len(found.items) - DETAIL_ROWS} more items in --json")
    return out
