"""The page's HTML (docs/efficiency-dashboard.md, "The page"): each query's result rendered as a
section, with bars and trend lines drawn as inline SVG or plain boxes, and every number taken
from the result's JSON as it is, formatted to the digits the text shows. Nothing here computes
a figure: a renderer only places numbers the query already rounded.

A section is one query's result. A line that has more detail is a `<details>` whose `data-src`
names the address of the section it opens; the page's one script fetches that section the
first time the line opens."""

from __future__ import annotations

import functools
import zlib
from collections.abc import Callable, Mapping, Sequence
from datetime import datetime
from html import escape
from urllib.parse import urlencode

from . import kinds

# The categories with a colour of their own. A kind of command gets the colour of its place
# in the command kinds table, and any other name one by a hash of the name, so that a
# category has the same colour in every bar and strip.
FIXED: dict[str, str] = {
    "model": "var(--model)",
    "fixed prefix": "var(--prefix)",
    "output": "var(--output)",
    "the rest": "var(--rest)",
    "waiting on a person": "var(--quiet)",
    "idle": "var(--quiet)",
}
# The colours of the kinds of command: Tableau 20 without the fixed colours, the darker of
# each pair first, so that the table's first kinds, the commonest, differ most.
PALETTE: tuple[str, ...] = (
    "#f28e2b",
    "#e15759",
    "#499894",
    "#b6992d",
    "#b07aa1",
    "#d37295",
    "#79706e",
    "#a0cbe8",
    "#ffbe7d",
    "#ff9d9a",
    "#86bcb6",
    "#f1ce63",
    "#d4a6c8",
    "#fabfd2",
    "#bab0ac",
    "#8cd17d",
    "#d7b5a6",
)
# How a session's id shows in a line: its prefix, which `efficiency session` also takes.
SESSION_ID = 14
# The prefixes a name of a dollar source or a kind of background wait carries before the
# kind of command it belongs to.
PREFIXES: tuple[str, ...] = ("carried: ", "new: ", "background: ")


def colour(name: str) -> str:
    """The colour of a category, kind of command or source of dollars: a source or a
    background wait has the colour of the kind of command it belongs to."""
    base = name
    for prefix in PREFIXES:
        if base.startswith(prefix):
            base = base[len(prefix) :]
    if base in FIXED:
        return FIXED[base]
    order = kind_order()
    index = order[base] if base in order else zlib.crc32(base.encode())
    return PALETTE[index % len(PALETTE)]


@functools.cache
def kind_order() -> dict[str, int]:
    """Each kind of command by its first place in the command kinds table."""
    found: dict[str, int] = {}
    for k in kinds.load()[0]:
        found.setdefault(k.kind, len(found))
    return found


def number(value: object, digits: int, lower: bool = False) -> str:
    """A number as the text shows it; unknown is a dash, never zero."""
    if value is None:
        return "-"
    assert isinstance(value, (int, float)) and not isinstance(value, bool), value
    return f"{float(value):.{digits}f}" + ("+" if lower else "")


def integer(value: object) -> str:
    if value is None:
        return "-"
    assert isinstance(value, int), value
    return str(value)


def get(data: Mapping[str, object], key: str) -> object:
    return data.get(key)


def mapping(value: object) -> Mapping[str, object]:
    assert isinstance(value, dict), value
    return value


def rows(value: object) -> list[Mapping[str, object]]:
    assert isinstance(value, list), value
    return [mapping(v) for v in value]


def text(value: object) -> str:
    return escape("" if value is None else str(value))


def address(path: str, params: Mapping[str, str | None]) -> str:
    kept = {k: v for k, v in params.items() if v}
    return path + ("?" + urlencode(kept) if kept else "")


def heading(name: str, fields: Mapping[str, str], field: str | None = None) -> str:
    """A heading whose definition, from the query's field descriptions, shows on hover."""
    found = fields.get(field or "")
    hint = f' title="{escape(found)}"' if found else ""
    return f'<span class="h"{hint}>{escape(name)}</span>'


def figure(name: str, shown: str, fields: Mapping[str, str], field: str) -> str:
    return (
        f'<div class="fig">{heading(name, fields, field)}'
        f'<span class="n">{shown}</span></div>'
    )


def copy(command: str) -> str:
    """The control that copies the command printing the same section."""
    return (
        f'<button class="copy" type="button" data-command="{escape(command)}" '
        f'title="{escape(command)}">copy</button>'
    )


def local_moment(moment: str, width: int) -> str:
    """A UTC time as the local day ("2026-11-15", width 10) or hour ("2026-11-15 10:00",
    width 16) that a trend's points are named by."""
    found = datetime.fromisoformat(moment.replace("Z", "+00:00")).astimezone()
    return found.strftime("%Y-%m-%d %H:%M")[:width]


def mark_at(names: list[str], moment: str | None) -> float | None:
    """Where on a trend through points named `names` a mark at `moment` goes: on its own
    point's place, halfway between the two it falls between, or nowhere when it is outside
    the trend."""
    if not moment or not names:
        return None
    at = local_moment(moment, len(names[0]))
    if at < names[0] or at > names[-1]:
        return None
    for i, name in enumerate(names):
        if name == at:
            return float(i)
        if name > at:
            return i - 0.5
    return None


def mark_title(m: Mapping[str, object], digits: int) -> str:
    """A mark's label and the medians either side of it, shown on hover."""
    return (
        f"{text(m['label'])}: {number(m['before'], digits)} ({integer(m['before_count'])})"
        f" to {number(m['after'], digits)} ({integer(m['after_count'])}), "
        f"{text(m['verdict'])}"
    )


def sparkline(
    trend: list[Mapping[str, object]],
    digits: int,
    marks: list[Mapping[str, object]] | None = None,
    key: str = "at",
    kind: str = "spark",
) -> str:
    """A small line through the medians of a figure's trend, oldest first, each point named
    by its `key`; each efficiency change on it is a mark at its commit, with its label and
    medians on hover."""
    shown = [p for p in trend if p.get("median") is not None]
    values = [float(str(p["median"])) for p in shown]
    if len(values) < 2:
        # A small line keeps its place in its figure; a wide one with nothing to draw goes.
        return '<svg class="spark" aria-hidden="true"></svg>' if kind == "spark" else ""
    low, high = min(values), max(values)
    span = (high - low) or 1.0
    step = 100 / (len(values) - 1)
    points = " ".join(
        f"{i * step:.2f},{22 - 20 * (v - low) / span:.2f}" for i, v in enumerate(values)
    )
    # A point is a line of no length with a round cap, so that it stays round however far
    # the line is stretched.
    dots = "".join(
        f'<g class="dot"><title>{text(p[key])}: {number(p["median"], digits)} '
        f'({integer(p["count"])})</title><line x1="{i * step:.2f}" x2="{i * step:.2f}" '
        f'y1="{22 - 20 * (v - low) / span:.2f}" y2="{22 - 20 * (v - low) / span:.2f}" '
        'vector-effect="non-scaling-stroke"></line></g>'
        for i, (v, p) in enumerate(zip(values, shown))
    )
    names = [str(p[key]) for p in shown]
    lines = []
    for m in marks or []:
        place = mark_at(names, str(m["time"]) if m.get("time") else None)
        if place is None:
            continue
        x = f"{place * step:.2f}"
        lines.append(
            f'<g class="mark"><title>{mark_title(m, digits)}</title>'
            f'<line x1="{x}" x2="{x}" y1="-1" y2="25" '
            'vector-effect="non-scaling-stroke"></line>'
            f'<line class="hit" x1="{x}" x2="{x}" y1="-1" y2="25" '
            'vector-effect="non-scaling-stroke"></line></g>'
        )
    return (
        f'<svg class="{kind}" viewBox="-3 -1 106 26" preserveAspectRatio="none">'
        + "".join(lines)
        + f'<polyline points="{points}" vector-effect="non-scaling-stroke"/>{dots}</svg>'
    )


def bar(
    name: str,
    total: str,
    parts: Sequence[Mapping[str, object]],
    digits: int,
    fields: Mapping[str, str],
    field: str,
) -> str:
    """One horizontal bar of a split's parts. A part is labelled with its name only when the
    label fits (the page's script hides the others); its value and share show on hover.
    """
    # Widths as shares of the drawn parts: CSS fills only part of a box whose flex-grow
    # values sum to less than one.
    whole = sum(max(float(str(p["value"])), 0.0) for p in parts) or 1.0
    segments = "".join(
        f'<span class="seg" style="flex-grow:{1000 * float(str(p["value"])) / whole:.3f};'
        f'background:{colour(str(p["name"]))}" '
        f'title="{text(p["name"])} {number(p["value"], digits)} '
        f'({number(p["percent"], 0)}%)"><i>{text(p["name"])}</i></span>'
        for p in parts
        if float(str(p["value"])) > 0
    )
    return (
        f'<div class="bar-row">{heading(name, fields, field)}'
        f'<span class="n">{total}</span>'
        f'<div class="bar">{segments}</div></div>'
    )


def line_grid(
    headers: Sequence[tuple[str, str | None]],
    lines: Sequence[tuple[list[str], str | None]],
    fields: Mapping[str, str],
    kind: str,
    words: frozenset[int] = frozenset(),
) -> str:
    """Lines in aligned columns: the first is a name, the last a title, the ones between
    numbers, except the columns `words` names, which hold words. A line with an address
    opens the section it names in place."""

    def kind_of(i: int, last: int, cell: bool) -> str:
        if 0 < i < last:
            return "w" if i in words else "num"
        return ("t" if i else "id") if cell else ""

    head = "".join(
        f'<span class="{kind_of(i, len(headers) - 1, False)}">'
        f"{heading(h, fields, f)}</span>"
        for i, (h, f) in enumerate(headers)
    )
    body: list[str] = []
    for cells, src in lines:
        shown = "".join(
            f'<span class="{kind_of(i, len(cells) - 1, True)}">{c}</span>'
            for i, c in enumerate(cells)
        )
        if src:
            body.append(
                f'<details class="line" data-src="{escape(src)}">'
                f'<summary class="cols">{shown}</summary><div class="open"></div>'
                "</details>"
            )
        else:
            body.append(f'<div class="line"><div class="cols">{shown}</div></div>')
    return (
        f'<div class="grid {kind}"><div class="cols head">{head}</div>'
        + "".join(body)
        + "</div>"
    )


def scope_params(data: Mapping[str, object]) -> dict[str, str | None]:
    """The range a roll-up's lines open their sections in."""
    since = data.get("range")
    return {"since": str(since) if since else None}


def summary(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    total = mapping(data["total"])
    figures = []
    for name, key, digits in (
        ("agent min", "agent_minutes", 1),
        ("core min", "core_minutes", 1),
        ("dollars", "dollars", 2),
        ("rework", "rework", 2),
    ):
        found = mapping(data[key])
        figures.append(
            f'<div class="fig">{heading(name, fields, key)}'
            f'<span class="n">{number(found["median"], digits, bool(found["lower_bound"]))}'
            f"</span>{sparkline(rows(found['trend']), digits, rows(found['marks']))}"
            "</div>"
        )
    params = scope_params(data)
    lines = []
    for l in rows(data["lines"]):
        name = str(l["name"])
        opens = name.startswith("by-")
        lines.append(
            (
                [
                    text(name),
                    integer(l["beads_closed"]),
                    number(l["agent_minutes"], 1),
                    number(l["dollars"], 2, bool(l["lower_bound"])),
                    text(l["title"]),
                ],
                address(f"/epic/{name}", {**params, "section": "1"}) if opens else None,
            )
        )
    return (
        '<div class="figs">'
        + figure(
            "tickets closed", integer(data["beads_closed"]), fields, "beads_closed"
        )
        + "".join(figures)
        + "</div>"
        + bar(
            "agent min",
            number(total["agent_minutes"], 1),
            rows(data["time"]),
            1,
            fields,
            "time",
        )
        + bar(
            "dollars",
            number(total["dollars"], 2, bool(total["lower_bound"])),
            rows(data["dollars_by_source"]),
            2,
            fields,
            "dollars_by_source",
        )
        + line_grid(
            [
                ("epic", "lines"),
                ("closed", "beads_closed"),
                ("agent min", "agent_minutes"),
                ("dollars", "dollars"),
                ("", None),
            ],
            lines,
            fields,
            "epics",
        )
    )


def epic(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    total = mapping(data["total"])
    lines = []
    for l in rows(data["lines"]):
        name = str(l["name"])
        opens = name.startswith("by-")
        lines.append(
            (
                [
                    text(name),
                    integer(l["beads_closed"]),
                    number(l["agent_minutes"], 1),
                    number(l["dollars"], 2, bool(l["lower_bound"])),
                    text(l["title"]),
                ],
                address(f"/bead/{name}", {"section": "1"}) if opens else None,
            )
        )
    return (
        '<div class="figs">'
        + figure("closed", integer(data["beads_closed"]), fields, "beads_closed")
        + figure("rework", number(total["rework"], 2), fields, "rework")
        + "</div>"
        + bar(
            "agent min",
            number(total["agent_minutes"], 1),
            rows(data["time"]),
            1,
            fields,
            "time",
        )
        + bar(
            "dollars",
            number(total["dollars"], 2, bool(total["lower_bound"])),
            rows(data["dollars_by_source"]),
            2,
            fields,
            "dollars_by_source",
        )
        + line_grid(
            [
                ("bead", "lines"),
                ("closed", "beads_closed"),
                ("agent min", "agent_minutes"),
                ("dollars", "dollars"),
                ("", None),
            ],
            lines,
            fields,
            "beads",
        )
    )


def strip(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    """The ticket's timeline: one bar as long as its wall minutes, coloured by what each
    stretch went to, and under it the context of each request."""
    stretches = rows(data["strip"])
    wall = data.get("wall_minutes")
    if not stretches or not isinstance(wall, (int, float)) or wall <= 0:
        return ""
    rects = "".join(
        f'<rect x="{float(str(s["at"])):.2f}" width="{float(str(s["minutes"])):.2f}" '
        f'height="1" fill="{colour(str(s["name"]))}"><title>{text(s["name"])} '
        f'{number(s["minutes"], 2)}</title></rect>'
        for s in stretches
    )
    contexts = rows(data["contexts"])
    peak = max((int(str(c["tokens"])) for c in contexts), default=0)
    line = ""
    if peak:
        points = " ".join(
            f'{float(str(c["at"])):.2f},{peak - int(str(c["tokens"]))}'
            for c in contexts
        )
        line = (
            f'<svg class="context" viewBox="0 0 {float(wall):.2f} {peak}" '
            f'preserveAspectRatio="none"><polyline points="{points}" '
            'vector-effect="non-scaling-stroke"/></svg>'
        )
    return (
        f'<div class="strip-row">{heading("wall min", fields, "strip")}'
        f'<span class="n">{number(wall, 1)}</span><div class="strip">'
        f'<svg viewBox="0 0 {float(wall):.2f} 1" preserveAspectRatio="none">{rects}</svg>'
        f"{line}</div></div>"
    )


def bead(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    if data.get("no_data"):
        return (
            '<div class="figs">'
            + figure("no data", text(data.get("ran_on")), fields, "no_data")
            + "</div>"
        )
    lower = bool(data["lower_bound"])
    by = mapping(data["dollars_by"])
    sessions: list[tuple[list[str], str | None]] = [
        (
            [
                f'<span title="{text(s["session"])}">'
                f'{text(str(s["session"])[:SESSION_ID])}</span>',
                text(s["kind"]),
                number(s["minutes"], 1),
                text(s["title"]),
            ],
            address(
                f"/session/{s['session']}", {"bead": str(data["bead"]), "section": "1"}
            ),
        )
        for s in rows(data["sessions"])
    ]
    return (
        '<div class="figs">'
        + figure("agent min", number(data["agent_minutes"], 1), fields, "agent_minutes")
        + figure("core min", number(data["core_minutes"], 1), fields, "core_minutes")
        + figure("dollars", number(data["dollars"], 2, lower), fields, "dollars")
        + figure("rework", number(data["rework"], 2), fields, "rework")
        + figure("requests", integer(data["requests"]), fields, "requests")
        + figure(
            "median context",
            number(data["median_context"], 0),
            fields,
            "context_tokens",
        )
        + "</div>"
        + bar(
            "agent min",
            number(data["agent_minutes"], 1),
            rows(data["time"]),
            1,
            fields,
            "time",
        )
        + bar(
            "dollars",
            number(data["dollars"], 2, lower),
            rows(by.get("source", [])),
            2,
            fields,
            "dollars",
        )
        + strip(data, fields)
        + line_grid(
            [("session", None), ("kind", None), ("min", None), ("", None)],
            sessions,
            fields,
            "sessions",
        )
    )


def session(data: Mapping[str, object], fields: Mapping[str, str]) -> str:
    found = rows(data["rows"])
    hidden = int(str(data["hidden"]))
    shown = found[: len(found) - hidden] if hidden else found
    body = "".join(
        f'<tr class="{"sub" if r["subagent"] else ""}">'
        f"<td>{text(str(r['time'])[11:19])}</td><td>{text(r['what'])}</td>"
        f'<td class="num">{integer(r["context_tokens"])}</td>'
        f'<td class="num">{integer(r["output_tokens"])}</td>'
        f'<td class="num">{number(r["dollars"], 2)}</td>'
        f'<td class="num">{number(r["seconds"], 1)}</td>'
        f'<td class="t">{text(r["detail"])}</td></tr>'
        for r in shown
    )
    more = f'<tr><td colspan="7" class="hidden">+{hidden}</td></tr>' if hidden else ""
    return (
        '<div class="figs">'
        + figure("requests", integer(data["requests"]), fields, "requests")
        + figure("tool calls", integer(data["tool_calls"]), fields, "rows")
        + figure(
            "dollars",
            number(data["dollars"], 2, bool(data["lower_bound"])),
            fields,
            "dollars",
        )
        + "</div>"
        '<div class="rows"><table><thead><tr>'
        f'<th>{heading("time", fields, "rows")}</th><th></th>'
        f'<th class="num">{heading("context", fields, "context_tokens")}</th>'
        '<th class="num">output</th>'
        f'<th class="num">{heading("dollars", fields, "dollars")}</th>'
        '<th class="num">seconds</th><th></th></tr></thead>'
        f"<tbody>{body}{more}</tbody></table></div>"
    )


Renderer = Callable[[Mapping[str, object], Mapping[str, str]], str]
# The queries the page renders, by name; `views` adds the other views.
RENDERERS: dict[str, Renderer] = {
    "summary": summary,
    "epic": epic,
    "bead": bead,
    "session": session,
}
# The headings of the sections whose heading is not an id and title, by query.
TITLES: dict[str, Callable[[Mapping[str, object]], str]] = {}
# The views' own rules, added to the stylesheet.
STYLES: list[str] = []
# The views the page's header leads to.
VIEWS: tuple[tuple[str, str], ...] = (
    ("cards", "Cards"),
    ("machine", "Machine"),
    ("enact", "Enact"),
    ("ci", "CI"),
    ("changes", "Changes"),
    ("findings", "Findings"),
)


def title_of(data: Mapping[str, object], query: str) -> str:
    """The section's heading: the id and title of what it shows."""
    if query == "summary":
        return ""
    if query in TITLES:
        return TITLES[query](data)
    name = data.get(query)
    found = data.get("title")
    return (
        f'<h2><span class="id">{text(name)}</span>'
        + (f' <span class="t">{text(found)}</span>' if found else "")
        + "</h2>"
    )


def section(
    query: str,
    data: Mapping[str, object],
    fields: Mapping[str, str],
    command: str | None,
) -> str:
    """One query's result as a section and its content. A section opened in place under its
    line (`command` given) has the control that copies its command, and no heading, since
    the line names it; a whole page's section has its heading, and the page's header holds
    the control."""
    top = copy(command) if command else title_of(data, query)
    return (
        f'<section class="q-{query}">'
        + (f'<div class="top">{top}</div>' if top else "")
        + f"{RENDERERS[query](data, fields)}</section>"
    )


RANGES: tuple[tuple[str, str], ...] = (
    ("24h", "24 hours"),
    ("7d", "7 days"),
    ("all", "all"),
)


def page(
    path: str,
    since: str | None,
    body: str,
    title: str,
    command: str,
    version: int | None = None,
) -> str:
    """A whole page around a section: the range control, the control that copies the
    section's command, the store's status, and the stylesheet and script. `version` is the
    store's last ingest when the page was made; the script compares it with the server's
    `/status.json` to offer a reload once newer data is in."""
    choices = "".join(
        f'<a href="{escape(address(path, {"since": value}))}"'
        + (' class="on"' if value == since else "")
        + f">{label}</a>"
        for value, label in RANGES
    )
    here = path.strip("/").split("/")[0]
    views = "".join(
        f'<a href="{escape(address("/" + name, {"since": since}))}"'
        + (' class="on"' if name == here else "")
        + f">{label}</a>"
        for name, label in VIEWS
    )
    return (
        '<!doctype html><html lang="en"><head><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width, initial-scale=1">'
        f"<title>{escape(title)}</title><style>{STYLE}{''.join(STYLES)}</style></head>"
        f'<body><header><a class="home" href="/">efficiency</a>'
        f'<nav class="views">{views}</nav>'
        f'<nav class="range">{choices}</nav>'
        f'<span class="status" data-version="{"" if version is None else version}"></span>'
        f"{copy(command)}</header>"
        f"<main>{body}</main><script>{SCRIPT}</script></body></html>"
    )


STYLE = """
:root{color-scheme:light dark;--bg:#fbfbfa;--fg:#1d1d1f;--dim:#6e6e73;--line:#e6e6e3;
--hover:#f1f1ee;--model:#4e79a7;--prefix:#9c755f;--output:#59a14f;--rest:#c7c7c2;
--quiet:#d9d9d4;
}
@media (prefers-color-scheme:dark){:root{--bg:#161617;--fg:#ececec;--dim:#98989d;
--line:#2c2c2e;--hover:#232325;--model:#6f9fd8;--prefix:#c19a82;--output:#7cc36f;
--rest:#8e8e93;--quiet:#636366;
}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);
font:14px/1.4 -apple-system,BlinkMacSystemFont,"Helvetica Neue",sans-serif;
font-variant-numeric:tabular-nums}
header{display:flex;flex-wrap:wrap;align-items:baseline;gap:8px 24px;max-width:1080px;margin:0 auto;
padding:20px 16px 4px}
a{color:inherit;text-decoration:none}
.home{font-weight:600}
.range,.views{display:flex;flex-wrap:wrap;gap:4px}
.range a,.views a{padding:2px 8px;border-radius:6px;color:var(--dim)}
.range a.on,.views a.on{background:var(--hover);color:var(--fg)}
.views a:hover,.range a:hover{color:var(--fg)}
main{max-width:1080px;margin:0 auto;padding:8px 16px 48px}
section{min-width:0}
.top{display:flex;align-items:baseline;gap:12px;min-height:8px}
.top h2{margin:0;font-size:15px;font-weight:600;min-width:0;flex:1;
overflow-wrap:anywhere}
.top h2 .t{font-weight:400;color:var(--dim)}
.status{margin-left:auto;font-size:12px;color:var(--dim);white-space:nowrap}
.status a{color:var(--fg);text-decoration:underline}
.status+.copy{margin-left:0}
.loading{color:var(--dim);font-size:12px;padding:4px 0}
.loading::after{content:"";display:inline-block;width:10px;height:10px;margin-left:6px;
border:2px solid var(--line);border-top-color:var(--dim);border-radius:50%;
animation:spin .8s linear infinite;vertical-align:-1px}
@keyframes spin{to{transform:rotate(360deg)}}
body::before{content:"";position:fixed;top:0;left:0;height:2px;width:0;
background:var(--model);z-index:1}
body.busy::before{animation:busy 8s cubic-bezier(.1,.7,.3,1) forwards}
body.busy main{opacity:.55;transition:opacity .2s .15s}
@keyframes busy{to{width:92%}}
@media (prefers-reduced-motion:reduce){.loading::after,body.busy::before{animation:none}
body.busy::before{width:100%}}
.copy{margin-left:auto;font:inherit;font-size:12px;color:var(--dim);background:none;
border:1px solid var(--line);border-radius:6px;padding:1px 8px;cursor:pointer}
.copy:hover{color:var(--fg)}
.h{color:var(--dim);font-size:12px;cursor:help;white-space:nowrap}
.n{font-size:22px;font-weight:500;white-space:nowrap}
.figs{display:flex;flex-wrap:wrap;gap:12px 32px;margin:12px 0 20px}
.fig{display:flex;flex-direction:column;min-width:96px}
.spark{width:96px;height:24px;overflow:visible}
.spark polyline,.trend polyline{fill:none;stroke:var(--model);stroke-width:1.5}
.dot line{stroke:transparent;stroke-width:9;stroke-linecap:round}
.dot:hover line{stroke:var(--model)}
.mark line{stroke:var(--dim);stroke-width:1;stroke-dasharray:2 2}
.mark line.hit{stroke:transparent;stroke-width:9;stroke-dasharray:none}
.mark:hover line{stroke:var(--fg)}
.mark:hover line.hit{stroke:transparent}
.bar-row,.strip-row{display:grid;grid-template-columns:72px 88px minmax(0,1fr);
align-items:center;gap:8px;margin:6px 0}
.bar-row .n,.strip-row .n{font-size:14px;text-align:right}
.bar{display:flex;height:22px;border-radius:4px;overflow:hidden;min-width:0}
.seg{flex-basis:0;min-width:1px;display:flex;align-items:center;overflow:hidden;
border-right:1px solid var(--bg)}
.seg i{font-style:normal;font-size:11px;padding:0 5px;white-space:nowrap;color:#1d1d1f}
.seg i.hide{visibility:hidden}
.strip-row{margin-top:14px;align-items:start}
.strip svg{display:block;width:100%;height:14px;border-radius:3px}
.strip .context{height:28px;margin-top:2px}
.context polyline{fill:none;stroke:var(--dim);stroke-width:1}
.grid{margin-top:20px}
.cols{display:grid;grid-template-columns:var(--cols);gap:12px;align-items:baseline;
padding:5px 4px}
.epics,.beads{--cols:96px 56px 80px 80px minmax(0,1fr)}
.sessions{--cols:120px 96px 56px minmax(0,1fr)}
.head{border-bottom:1px solid var(--line)}
.line{border-bottom:1px solid var(--line)}
.cols span{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.num{text-align:right}
.t{color:var(--dim)}
summary{list-style:none;cursor:pointer}
summary::-webkit-details-marker{display:none}
summary:hover{background:var(--hover)}
details[open]>summary{background:var(--hover)}
.open{padding:4px 0 16px 16px;border-left:2px solid var(--line);margin-left:4px}
.open:empty{display:none}
.rows{overflow-x:auto}
table{border-collapse:collapse;width:100%;font-size:12px}
th{text-align:left;font-weight:400;border-bottom:1px solid var(--line);padding:4px 6px}
td{padding:2px 6px;white-space:nowrap;border-bottom:1px solid var(--line)}
td.t{white-space:normal;overflow-wrap:anywhere;min-width:160px}
tr.sub td:nth-child(2){padding-left:18px}
.hidden{color:var(--dim)}
@media (max-width:600px){
.epics,.beads{--cols:100px 40px 72px 72px}
.epics .cols>span:last-child,.beads .cols>span:last-child{display:none}
.sessions{--cols:120px 64px 44px}
.sessions .cols>span:last-child{display:none}
.bar-row,.strip-row{grid-template-columns:64px 72px minmax(0,1fr)}
.figs{gap:12px 20px}
.rows th:nth-child(4),.rows td:nth-child(4),.rows th:nth-child(7),
.rows td:nth-child(7){display:none}
td,th{padding-left:4px;padding-right:4px}
.rows td:nth-child(2){white-space:normal;overflow-wrap:anywhere}}
"""

# Opens a section's content the first time its line opens (a line open from the start, such
# as a trend, at once), showing that it is loading meanwhile; dims the page and runs a bar
# along its top while a link's page loads; shows how old the store is and offers a reload
# once an ingest has landed; copies a section's command; and hides a bar part's label where
# it does not fit.
SCRIPT = """
function fit(root){for(const i of root.querySelectorAll('.seg i')){
i.classList.remove('hide');if(i.scrollWidth>i.parentElement.clientWidth)
i.classList.add('hide');}}
async function load(d){if(!d.dataset||!d.dataset.src||d.dataset.loaded)return;
d.dataset.loaded='1';const box=d.querySelector('.open');
box.innerHTML='<div class="loading">loading</div>';
let shown='-';try{const r=await fetch(d.dataset.src);
shown=r.ok?await r.text():'-';}catch(e){d.dataset.loaded='';}
box.innerHTML=shown;fit(box);opened(box);}
document.addEventListener('click',e=>{const a=e.target.closest&&e.target.closest('a[href]');
if(!a||e.defaultPrevented||e.button||e.metaKey||e.ctrlKey||e.shiftKey||e.altKey)return;
if(a.origin===location.origin&&!a.closest('.status'))document.body.classList.add('busy');});
addEventListener('pageshow',()=>document.body.classList.remove('busy'));
function ago(t){const m=Math.round((Date.now()-Date.parse(t))/60000);
return m<1?'just now':m<60?m+' min ago':Math.round(m/60)+' h ago';}
async function status(){const s=document.querySelector('.status');if(!s)return;
try{const r=await fetch('/status.json');const j=await r.json();
const mine=s.dataset.version;const newer=j.version!==null&&String(j.version)!==mine;
if(newer&&mine===''){location.reload();return;}
s.innerHTML=newer?'<a href="">new data, reload</a>':j.updating?'updating':
j.updated?'updated '+ago(j.updated):'no data yet';
if(!j.updated&&!newer&&j.updating)s.innerHTML='reading logs, first time';}catch(e){}}
status();setInterval(status,5000);
function opened(root){for(const d of root.querySelectorAll('details[open][data-src]'))
load(d);}
document.addEventListener('toggle',e=>{if(e.target.open)load(e.target);},true);
document.addEventListener('click',e=>{const b=e.target.closest&&e.target.closest('.copy');
if(!b)return;navigator.clipboard.writeText(b.dataset.command);b.textContent='copied';
setTimeout(()=>{b.textContent='copy'},1200);});
addEventListener('resize',()=>fit(document));fit(document);opened(document);
"""
