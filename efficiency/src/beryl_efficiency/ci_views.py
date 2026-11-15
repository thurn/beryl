"""The CI view (docs/efficiency-dashboard.md, "CI accounting" and "The page"): the tree
script, check lane, family, check, opened level by level, with CPU by kind of process and the
oracle engines, and the sections it opens: the fast check per push and the cost of slow-CI
failures. Built from the first screen's parts, like `views`."""

from __future__ import annotations

from collections.abc import Mapping
from html import escape

from . import ci, page
from .page import address, bar, figure, integer, line_grid, mapping, number, rows, text
from .views import heading_of, trends

# How many lines of pushes or failures a section shows; the copied command prints them all.
SHOWN_ROWS = 40


def per_catch(node: Mapping[str, object]) -> str:
    """CPU per catch; "never" for a node with no catch, whose first run shows beside it."""
    found = node["core_hours_per_catch"]
    if found is None and not node["catch"]:
        return "never"
    return number(found, 2)


def ci_cells(node: Mapping[str, object], name: str) -> list[str]:
    return [
        name,
        number(node["core_hours"], 2),
        integer(node["runs"]),
        number(node["skipped_percent"], 1),
        integer(node["catch"]),
        integer(node["flaky"]),
        integer(node["stale"]),
        integer(node["miss"]),
        per_catch(node),
        text(node["first_run"]),
    ]


def ci_view(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    """The tree of one script: check lanes open to their families and families to their
    checks, each a level of the same query."""
    total = mapping(data["total"])
    script: str = str(data["script"])
    since: str | None = str(data["range"]) if data.get("range") else None
    level = str(data["level"])
    top = data.get("lane") is None and data.get("family") is None
    # The level a node of each level opens.
    opens: dict[str, str] = {"lane": "lane", "family": "family"}

    def src(name: str, node_level: str) -> str | None:
        key = opens.get(node_level)
        if key is None:
            return None
        return address(
            "/ci", {"script": script, key: name, "since": since, "section": "1"}
        )

    child_level = {"lane": "family", "family": "check"}.get(level, "check")
    lines: list[tuple[list[str], str | None]] = []
    for node in rows(data["rows"]):
        name = str(node["name"])
        lines.append((ci_cells(node, text(name)), src(name, level)))
        for child in rows(node["children"]):
            kid = str(child["name"])
            lines.append(
                (
                    ci_cells(child, f'<span class="kid">{text(kid)}</span>'),
                    src(kid, child_level),
                )
            )
    scripts = ""
    if top:
        scripts = (
            '<nav class="scripts">'
            + "".join(
                f'<a href="{escape(address("/ci", {"script": name, "since": since}))}"'
                + (' class="on"' if name == script else "")
                + f">{escape(title)}</a>"
                for name, title in ci.SCRIPTS.items()
            )
            + "</nav>"
        )
    kinds = [
        {"name": k["kind"], "value": k["core_hours"], "percent": k["percent"]}
        for k in rows(data["kinds"])
    ]
    oracle = [
        (
            [
                text(o["engine"]),
                integer(o["scenarios"]),
                integer(o["played"]),
                integer(o["skipped"]),
                integer(o["remembered"]),
                "",
            ],
            None,
        )
        for o in rows(data["oracle"])
    ]
    others = [
        (["fast check"], address("/pushes", {"since": since, "section": "1"})),
        (["failures"], address("/failures", {"since": since, "section": "1"})),
    ]
    return (
        scripts
        + '<div class="figs">'
        + figure("core h", number(total["core_hours"], 2), fields, "core_hours")
        + figure("runs", integer(data["script_runs"]), fields, "script_runs")
        + figure("catches", integer(total["catch"]), fields, "catch")
        + figure("flaky", integer(total["flaky"]), fields, "flaky")
        + figure("stale", integer(total["stale"]), fields, "stale")
        + figure("misses", integer(total["miss"]), fields, "miss")
        + figure("core h per catch", per_catch(total), fields, "core_hours_per_catch")
        + figure(
            "flake cpu s",
            number(total["flake_cpu_seconds"], 1),
            fields,
            "flake_cpu_seconds",
        )
        + figure("flake $", number(total["flake_dollars"], 2), fields, "flake_dollars")
        + "</div>"
        + (trends(data, fields) if top else "")
        + (bar("core h", "", kinds, 2, fields, "kinds") if kinds else "")
        + line_grid(
            [
                (level, "level"),
                ("core h", "core_hours"),
                ("runs", "runs"),
                ("skip %", "skipped_percent"),
                ("catch", "catch"),
                ("flaky", "flaky"),
                ("stale", "stale"),
                ("miss", "miss"),
                ("h/catch", "core_hours_per_catch"),
                ("first run", "first_run"),
            ],
            lines,
            fields,
            "tree",
        )
        + (
            line_grid(
                [
                    ("oracle", "oracle"),
                    ("scenarios", None),
                    ("played", None),
                    ("skipped", None),
                    ("remembered", None),
                    ("", None),
                ],
                oracle,
                fields,
                "oracle",
            )
            if oracle
            else ""
        )
        + (line_grid([("", None)], others, fields, "others") if top else "")
    )


def more_rows(found: list[Mapping[str, object]]) -> str:
    return '<div class="more">…</div>' if len(found) > SHOWN_ROWS else ""


def pushes(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    found = rows(data["rows"])
    lines = [
        (
            [
                text(p["bead"] or "(no bead)"),
                number(p["seconds"], 0),
                number(p["fast_check_seconds"], 0),
                number(p["cpu_seconds"], 0),
                number(p["queued_seconds"], 0),
                integer(p["repeats"]),
                text(str(p["start"])[:16].replace("T", " ")),
            ],
            address(f"/bead/{p['bead']}", {"section": "1"}) if p["bead"] else None,
        )
        for p in found[:SHOWN_ROWS]
    ]
    failures = mapping(data["failures"])
    return (
        '<div class="figs">'
        + figure("pushes", integer(data["pushes"]), fields, "pushes")
        + figure("wall s", number(data["median_wall_seconds"], 0), fields, "pushes")
        + figure(
            "fast check s",
            number(data["median_fast_check_seconds"], 0),
            fields,
            "pushes",
        )
        + figure("cpu s", number(data["cpu_seconds"], 0), fields, "cpu_seconds")
        + figure(
            "queued s", number(data["queued_seconds"], 0), fields, "queued_seconds"
        )
        + figure("repeats", integer(data["repeats"]), fields, "repeats")
        + "".join(
            figure(str(name), integer(count), fields, "failures")
            for name, count in failures.items()
        )
        + "</div>"
        + trends(data, fields)
        + line_grid(
            [
                ("lane", "lanes"),
                ("s", None),
                ("cpu s", None),
                ("runs", None),
                ("", None),
            ],
            [
                (
                    [
                        text(l["lane"]),
                        number(l["seconds"], 0),
                        number(l["cpu_seconds"], 0),
                        integer(l["runs"]),
                        "",
                    ],
                    None,
                )
                for l in rows(data["lanes"])
            ],
            fields,
            "lanes",
        )
        + line_grid(
            [("left out", "deferred"), ("count", None), ("", None)],
            [
                ([text(d["name"]), integer(d["count"]), ""], None)
                for d in rows(data["deferred"])
            ],
            fields,
            "deferred",
        )
        + line_grid(
            [
                ("push", "rows"),
                ("wall s", None),
                ("fast s", None),
                ("cpu s", None),
                ("queued s", None),
                ("repeats", None),
                ("", None),
            ],
            lines,
            fields,
            "pushes",
        )
        + more_rows(found)
    )


def failures(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    found = rows(data["rows"])
    lines = [
        (
            [
                text(f["steward"]),
                number(f["dollars"], 2),
                number(f["agent_minutes"], 1),
                number(f["core_minutes"], 1),
                number(f["behind_minutes"], 1),
                text(
                    ", ".join(f"{c['check']} {c['verdict']}" for c in rows(f["checks"]))
                    or f["title"]
                ),
            ],
            address(f"/bead/{f['steward']}", {"section": "1"}),
        )
        for f in found[:SHOWN_ROWS]
    ]
    misses = [
        ([text(m["check"]), text(m["steward"])], None) for m in rows(data["misses"])
    ]
    return (
        '<div class="figs">'
        + figure("failures", integer(data["failures"]), fields, "failures")
        + figure("no data", integer(data["no_data"]), fields, "no_data")
        + figure("dollars", number(data["dollars"], 2), fields, "dollars")
        + figure("agent min", number(data["agent_minutes"], 1), fields, "agent_minutes")
        + figure(
            "bisect core min",
            number(data["bisect_core_minutes"], 1),
            fields,
            "bisect_core_minutes",
        )
        + figure(
            "behind min", number(data["behind_minutes"], 1), fields, "behind_minutes"
        )
        + "</div>"
        + trends(data, fields)
        + line_grid(
            [
                ("steward", "steward"),
                ("dollars", "dollars"),
                ("agent min", "agent_minutes"),
                ("core min", "core_minutes"),
                ("behind min", "behind_minutes"),
                ("", "checks"),
            ],
            lines,
            fields,
            "failures",
        )
        + more_rows(found)
        + (
            line_grid([("miss", "misses"), ("", None)], misses, fields, "misses")
            if misses
            else ""
        )
    )


def ci_title(data: Mapping[str, object]) -> str:
    script = str(data["script"])
    within = data.get("family") or data.get("lane")
    return heading_of(
        "CI", ci.SCRIPTS.get(script, script) + (f" {within}" if within else "")
    )


page.RENDERERS.update({"ci": ci_view, "pushes": pushes, "failures": failures})
page.TITLES.update(
    {
        "ci": ci_title,
        "pushes": lambda _: heading_of("Fast check"),
        "failures": lambda _: heading_of("Slow-CI failures"),
    }
)
page.STYLES.append("""
.scripts{display:flex;flex-wrap:wrap;gap:4px;margin:12px 0 0}
.scripts a{padding:2px 8px;border-radius:6px;color:var(--dim)}
.scripts a.on{background:var(--hover);color:var(--fg)}
.kid{padding-left:14px}
.more{color:var(--dim);padding:4px}
.tree{--cols:minmax(0,220px) 72px 80px 56px 52px 52px 52px 52px 64px minmax(0,1fr)}
.oracle{--cols:96px 80px 80px 80px 96px minmax(0,1fr)}
.others{--cols:minmax(0,1fr)}
.lanes{--cols:minmax(0,200px) 72px 72px 56px minmax(0,1fr)}
.deferred{--cols:minmax(0,260px) 56px minmax(0,1fr)}
.pushes{--cols:110px 64px 64px 64px 72px 64px minmax(0,1fr)}
.failures{--cols:96px 72px 80px 72px 88px minmax(0,1fr)}
.misses{--cols:minmax(0,260px) minmax(0,1fr)}
@media (max-width:600px){
.tree{--cols:minmax(0,1fr) 56px 40px 56px}
.tree .cols>span:nth-child(3),.tree .cols>span:nth-child(4),
.tree .cols>span:nth-child(6),.tree .cols>span:nth-child(7),
.tree .cols>span:nth-child(8),.tree .cols>span:nth-child(10){display:none}
.oracle{--cols:minmax(0,1fr) 64px 64px}
.oracle .cols>span:nth-child(2),.oracle .cols>span:nth-child(n+5){display:none}
.lanes{--cols:minmax(0,1fr) 56px 56px}
.lanes .cols>span:nth-child(n+4){display:none}
.deferred{--cols:minmax(0,1fr) 48px}
.deferred .cols>span:nth-child(3){display:none}
.pushes{--cols:minmax(0,1fr) 52px 52px 52px}
.pushes .cols>span:nth-child(n+5){display:none}
.failures{--cols:minmax(0,1fr) 60px 64px 64px}
.failures .cols>span:nth-child(3),.failures .cols>span:nth-child(6){display:none}}
""")
