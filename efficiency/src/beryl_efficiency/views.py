"""The page's views beyond the first screen (docs/efficiency-dashboard.md, "The page"): Cards,
Machine, Enact, Changes and Findings, and the sections they open in place (one metric's trend, one
finding's items); `ci_views` holds the CI view. Each renders one query's JSON as it is with
the first screen's parts (`page`), and adds no figure of its own.

A view's trend lines are the metrics its query names under `more`. Each is a line open from
the start, whose section the page's script fetches at once: the metric's medians by day, with
a mark at each efficiency change made to it."""

from __future__ import annotations

import shlex
from collections.abc import Mapping
from html import escape
from urllib.parse import urlencode

from . import page
from .changes import split_metric
from .page import (
    address,
    bar,
    figure,
    integer,
    line_grid,
    mapping,
    number,
    rows,
    sparkline,
    text,
)

# The digits a finding's unit is shown with.
UNIT_DIGITS: dict[str, int] = {"dollars": 2, "agent_hours": 1, "core_hours": 1}
UNIT_NAMES: dict[str, str] = {
    "dollars": "dollars",
    "agent_hours": "agent h",
    "core_hours": "core h",
}


def strings(value: object) -> list[str]:
    assert isinstance(value, list), value
    return [str(v) for v in value]


def plain(value: object) -> str:
    """A number the query rounded to digits this view does not know, as it is."""
    if value is None:
        return "-"
    assert isinstance(value, (int, float)) and not isinstance(value, bool), value
    return f"{float(value):.4f}".rstrip("0").rstrip(".")


def command_address(command: str) -> str | None:
    """The address of the section a query's command prints: `efficiency metrics x --since
    7d` is `/metrics/x?since=7d&section=1`. None for a command that is not such a query.
    """
    try:
        words = shlex.split(command)
    except ValueError:
        return None
    if len(words) < 2 or words[0] != "efficiency" or words[1] in ("sql", "ingest"):
        return None
    path = [words[1]]
    params: list[tuple[str, str]] = []
    rest = words[2:]
    while rest:
        word = rest.pop(0)
        if word.startswith("--"):
            if not rest:
                return None
            params.append((word[2:], rest.pop(0)))
        else:
            path.append(word)
    return "/" + "/".join(path) + "?" + urlencode([*params, ("section", "1")])


def trends(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    """The trend lines of the metrics a result names under `more`, open from the start."""
    found = []
    for command in strings(data.get("more") or []):
        words = str(command).split()
        src = command_address(str(command))
        if len(words) < 3 or words[1] != "metrics" or src is None:
            continue
        found.append(([text(words[2])], src))
    if not found:
        return ""
    grid = line_grid([("trend", "series")], found, fields, "trends")
    return grid.replace('<details class="line"', '<details class="line" open')


def series(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    """One metric by day with its marks, or, with no metric named, the list of metrics."""
    if "metric" not in data:
        return line_grid(
            [("metric", None), ("unit", None), ("", None)],
            [
                (
                    [
                        f'<span title="{text(m["description"])}">{text(m["name"])}</span>',
                        text(m["unit"]),
                        "",
                    ],
                    f"/metrics/{m['name']}?section=1",
                )
                for m in rows(data["metrics"])
            ],
            fields,
            "metric-list",
            frozenset({1}),
        )
    days = rows(data["series"])
    return (
        '<div class="figs">'
        + figure("count", integer(data["count"]), fields, "series")
        + figure("median", plain(data["median"]), fields, "series")
        + figure("p90", plain(data["p90"]), fields, "series")
        + "</div>"
        + sparkline(days, 4, rows(data["marks"]), key="day", kind="trend")
    )


def cards(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    total = mapping(data["total"])
    capacity = mapping(data["capacity"])
    lines = [
        (
            [
                text(g["name"]),
                integer(g["beads"]),
                integer(g["cards"]),
                number(g["direct"], 2, bool(g["lower_bound"])),
                number(g["rework"], 2),
                number(g["agent_minutes"], 1),
                number(g["fixed_later"], 0),
                integer(g["no_data"]),
                "",
            ],
            None,
        )
        for g in [*rows(data["groups"]), total]
    ]
    return (
        '<div class="figs">'
        + figure("cards", integer(total["cards"]), fields, "cards")
        + figure(
            "direct",
            number(total["direct"], 2, bool(total["lower_bound"])),
            fields,
            "per_card",
        )
        + figure("rework", number(total["rework"], 2), fields, "rework")
        + figure("agent min", number(total["agent_minutes"], 1), fields, "per_card")
        + figure("sessions", integer(capacity["sessions"]), fields, "capacity")
        + figure("cores", number(capacity["mean_cores"], 1), fields, "capacity")
        + figure("cores p90", number(capacity["p90_cores"], 1), fields, "capacity")
        + "</div>"
        + trends(data, fields)
        + line_grid(
            [
                (str(data["by"]), None),
                ("beads", None),
                ("cards", "cards"),
                ("direct", "per_card"),
                ("rework", "rework"),
                ("agent min", "per_card"),
                ("fixed later", "fixed_later"),
                ("no data", "no_data"),
                ("", None),
            ],
            lines,
            fields,
            "cards",
        )
    )


def machine(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    own = mapping(data["own"])
    capacity = mapping(data["capacity"])
    queued = [
        (
            [
                text(q["command"]),
                integer(q["leases"]),
                number(q["waited_hours"], 2),
                number(q["core_hours"], 2),
                text(q["klass"]),
            ],
            None,
        )
        for q in rows(data["queued"])
    ]
    narrow = [
        (
            [
                text(n["command"]),
                integer(n["leases"]),
                integer(n["narrower"]),
                number(n["mean_cores"], 1),
                number(n["mean_wanted"], 1),
                text(n["klass"]),
            ],
            None,
        )
        for n in rows(data["narrow"])
    ]
    return (
        '<div class="figs">'
        + figure("cores", integer(data["cores"]), fields, "cores")
        + figure("in use", number(data["cores_in_use"], 1), fields, "cores_in_use")
        + figure("peak", number(data["peak_cores"], 1), fields, "peak_cores")
        + figure("saturation", number(data["saturation"], 1), fields, "saturation")
        + figure("queued h", number(data["queued_hours"], 2), fields, "queued_hours")
        + figure(
            "queued core h",
            number(data["queued_core_hours"], 2),
            fields,
            "queued_core_hours",
        )
        + figure("sessions", integer(capacity["sessions"]), fields, "capacity")
        + "</div>"
        + trends(data, fields)
        + bar(
            "shared",
            number(data["shared_core_hours"], 2),
            rows(data["shared"]),
            2,
            fields,
            "shared",
        )
        + '<div class="figs">'
        + figure("core h", number(data["core_hours"], 2), fields, "core_hours")
        + figure(
            "in tickets", number(data["bead_core_hours"], 2), fields, "bead_core_hours"
        )
        + figure(
            "shared", number(data["shared_core_hours"], 2), fields, "shared_core_hours"
        )
        + figure("per day", number(data["shared_per_day"], 2), fields, "shared")
        + figure("per ticket", number(data["shared_per_bead"], 2), fields, "shared")
        + figure("sampler cores", number(own["sampler_cores"], 3), fields, "own")
        + figure("ingest cpu s", number(own["ingest_cpu_seconds"], 1), fields, "own")
        + "</div>"
        + line_grid(
            [
                ("queued", "queued"),
                ("leases", None),
                ("waited h", None),
                ("core h", None),
                ("", None),
            ],
            queued,
            fields,
            "queued",
        )
        + line_grid(
            [
                ("narrow", "narrow"),
                ("leases", None),
                ("narrower", None),
                ("cores", None),
                ("wanted", None),
                ("", None),
            ],
            narrow,
            fields,
            "narrow",
        )
    )


def enact(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    """Time waiting for Enact's ownership lock and syncing under it, by subcommand, then
    the commands that held the lock while others waited."""
    subcommands = [
        (
            [
                text(s["subcommand"]),
                integer(s["lines"]),
                integer(s["gave_up"]),
                number(s["wait_minutes"], 1),
                number(s["sync_minutes"], 1),
                number(s["held_minutes"], 1),
                integer(s["calls"]),
                number(s["call_minutes"], 1),
                "",
            ],
            None,
        )
        for s in rows(data["subcommands"])
    ]
    holders = [
        (
            [text(h["holder"]), integer(h["waits"]), number(h["wait_minutes"], 1), ""],
            None,
        )
        for h in rows(data["holders"])
    ]
    return (
        '<div class="figs">'
        + figure(
            "lock wait min",
            number(data["lock_wait_minutes"], 1),
            fields,
            "lock_wait_minutes",
        )
        + figure("sync min", number(data["sync_minutes"], 1), fields, "sync_minutes")
        + figure("lines", integer(data["lines"]), fields, "lock_lines")
        + figure("joined", integer(data["joined"]), fields, "lock_lines")
        + figure("no call", integer(data["unjoined"]), fields, "lock_lines")
        + "</div>"
        + line_grid(
            [
                ("subcommand", "lock_lines"),
                ("lines", None),
                ("gave up", None),
                ("wait min", None),
                ("sync min", None),
                ("held min", None),
                ("calls", None),
                ("call min", None),
                ("", None),
            ],
            subcommands,
            fields,
            "subcommands",
        )
        + line_grid(
            [("holder", "lock_lines"), ("waits", None), ("wait min", None), ("", None)],
            holders,
            fields,
            "holders",
        )
    )


def metric_address(spec: str) -> str | None:
    """The address of a change's metric over all time, in the cohort its filter names."""
    try:
        name, scope = split_metric(spec)
    except ValueError:
        return None
    return command_address(
        f"efficiency metrics {name} --since all {scope.describe()}".strip()
    )


def changes(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    lines = [
        (
            [
                text(str(c["time"])[:10] if c["time"] else "-"),
                text(c["metric"]),
                plain(c["before"]),
                plain(c["after"]),
                text(c["verdict"]),
                f'<span title="{text(c["label"])}">{text(c["label"])}</span>',
            ],
            metric_address(str(c["metric"])),
        )
        for c in rows(data["changes"])
    ]
    return line_grid(
        [
            ("reached staging", "changes"),
            ("metric", None),
            ("before", "changes"),
            ("after", "changes"),
            ("", None),
            ("", None),
        ],
        lines,
        fields,
        "changes",
        frozenset({1, 4}),
    )


def findings(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    """The groups of findings, each line opening its rule's items; for one rule, its items."""
    found = [f for g in rows(data["groups"]) for f in rows(g["findings"])]
    detail = [f for f in found if "items" in f]
    if detail:
        return "".join(items(f, fields) for f in detail)
    groups = []
    for g in rows(data["groups"]):
        unit = str(g["unit"])
        digits = UNIT_DIGITS.get(unit, 1)
        lines = [
            (
                [
                    text(f["rule"]),
                    number(mapping(f["saving_per_week"]).get(unit), digits),
                    number(f["cost_per_week"], digits),
                    f'<span title="{text(f["summary"])}">{text(f["summary"])}</span>',
                ],
                command_address(str(f["evidence"])),
            )
            for f in rows(g["findings"])
        ]
        groups.append(
            line_grid(
                [
                    (UNIT_NAMES.get(unit, unit), "unit"),
                    ("saving", "saving_per_week"),
                    ("cost", "cost_per_week"),
                    ("", "summary"),
                ],
                lines,
                fields,
                "findings",
            )
        )
    return "".join(groups)


def items(finding: Mapping[str, object], fields: Mapping[str, str]) -> str:
    """One finding's items, such as one role or one kind of command, per week."""
    unit = str(finding["unit"])
    digits = UNIT_DIGITS.get(unit, 1)
    lines = [
        (
            [
                text(i["name"]),
                integer(i["count"]),
                number(i["cost_per_week"], digits),
                number(i["reference_per_week"], digits),
                number(i["saving_per_week"], digits),
                text(", ".join(strings(i["examples"]))),
            ],
            None,
        )
        for i in rows(finding["items"])
    ]
    # The rule's own reference is the definition its reference column shows on hover.
    named = {**fields, "reference": str(finding["reference"])}
    return (
        '<div class="figs">'
        + figure(
            "saving",
            number(mapping(finding["saving_per_week"]).get(unit), digits),
            fields,
            "saving_per_week",
        )
        + figure(
            "cost", number(finding["cost_per_week"], digits), fields, "cost_per_week"
        )
        + "</div>"
        + items_grid(unit, lines, named)
    )


def items_grid(
    unit: str, lines: list[tuple[list[str], None]], fields: Mapping[str, str]
) -> str:
    return line_grid(
        [
            (UNIT_NAMES.get(unit, unit), "items"),
            ("count", None),
            ("cost", "cost_per_week"),
            ("reference", "reference"),
            ("saving", "saving_per_week"),
            ("", "examples"),
        ],
        lines,
        fields,
        "items",
    )


def heading_of(name: str, title: object = None) -> str:
    return (
        f'<h2><span class="id">{escape(name)}</span>'
        + (f' <span class="t">{text(title)}</span>' if title else "")
        + "</h2>"
    )


page.RENDERERS.update(
    {
        "metrics": series,
        "cards": cards,
        "machine": machine,
        "enact": enact,
        "changes": changes,
        "findings": findings,
    }
)
page.TITLES.update(
    {
        "metrics": lambda d: heading_of(
            str(d.get("metric") or "metrics"), d.get("description")
        ),
        "cards": lambda d: heading_of("Cards", f"by {d['by']}"),
        "machine": lambda _: heading_of("Machine"),
        "enact": lambda _: heading_of("Enact"),
        "changes": lambda _: heading_of("Changes"),
        "findings": lambda d: heading_of("Findings", None),
    }
)
page.STYLES.append("""
.trend{display:block;width:100%;height:64px;overflow:visible;margin:4px 0 8px}
.w{color:var(--dim)}
.trends{--cols:minmax(0,1fr)}
.trends .open{padding-bottom:4px}
.q-metrics .figs{margin:4px 0;gap:4px 24px}
.q-metrics .n{font-size:15px}
.trends{margin-bottom:12px}
.metric-list{--cols:260px 100px minmax(0,1fr)}
.cards{--cols:96px 56px 56px 72px 72px 72px 80px 64px minmax(0,1fr)}
.queued{--cols:minmax(0,260px) 64px 72px 72px minmax(0,1fr)}
.narrow{--cols:minmax(0,260px) 64px 72px 64px 64px minmax(0,1fr)}
.subcommands{--cols:minmax(0,200px) 56px 56px 64px 64px 64px 56px 64px minmax(0,1fr)}
.holders{--cols:minmax(0,260px) 56px 72px minmax(0,1fr)}
.changes{--cols:104px minmax(0,220px) 64px 64px minmax(0,190px) minmax(0,1fr)}
.findings{--cols:150px 80px 80px minmax(0,1fr)}
.items{--cols:minmax(0,260px) 56px 72px 80px 72px minmax(0,1fr)}
@media (max-width:600px){
.metric-list{--cols:minmax(0,1fr)}
.metric-list .cols>span:nth-child(n+2){display:none}
.cards{--cols:minmax(0,1fr) 40px 56px 52px 56px}
.cards .cols>span:nth-child(2),.cards .cols>span:nth-child(n+7){display:none}
.queued{--cols:minmax(0,1fr) 64px 56px}
.queued .cols>span:nth-child(2),.queued .cols>span:nth-child(5){display:none}
.narrow{--cols:minmax(0,1fr) 44px 52px}
.subcommands{--cols:minmax(0,1fr) 52px 52px 52px}
.subcommands .cols>span:nth-child(2),.subcommands .cols>span:nth-child(3),
.subcommands .cols>span:nth-child(n+6){display:none}
.holders{--cols:minmax(0,1fr) 48px 64px}
.holders .cols>span:nth-child(4){display:none}
.narrow .cols>span:nth-child(2),.narrow .cols>span:nth-child(3),
.narrow .cols>span:nth-child(6){display:none}
.changes{--cols:52px 52px minmax(0,96px) minmax(0,1fr)}
.changes .cols>span:nth-child(1),.changes .cols>span:nth-child(2){display:none}
.findings{--cols:minmax(0,1fr) 72px 72px}
.findings .cols>span:nth-child(4){display:none}
.items{--cols:minmax(0,1fr) 64px}
.items .cols>span:nth-child(n+2):not(:nth-child(5)){display:none}}
""")
