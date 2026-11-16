"""Cirrus's components: functions that return HTML strings using the classes in
`static/cirrus.css`. Every piece of text passes through `esc`; a function's `body` arguments
are HTML that another component already built.

Names follow what a component is (`chip`, `section`, `card`), and each one's docstring says
which principle of status/AGENTS.md it serves where that is not obvious."""

from __future__ import annotations

from collections.abc import Iterable, Sequence
from html import escape

from .assets import Assets

# The tones a chip, dot or bar can take. Each is a colour token in cirrus.css.
TONES: frozenset[str] = frozenset(
    {"neutral", "accent", "red", "orange", "amber", "green", "blue", "violet", "muted"}
)


def esc(value: object) -> str:
    return escape("" if value is None else str(value), quote=True)


def attrs(**values: str | None) -> str:
    """HTML attributes from keyword arguments, `data_tip` as `data-tip`, leaving out None."""
    return "".join(
        f' {k.rstrip("_").replace("_", "-")}="{esc(v)}"'
        for k, v in values.items()
        if v is not None
    )


def tone(name: str) -> str:
    assert name in TONES, name
    return name


def page(
    *,
    title: str,
    body: str,
    root: str,
    assets: Assets,
    crumbs: Sequence[tuple[str, str | None]] = (),
    brand: str = "",
    description: str = "",
    base: str | None = None,
) -> str:
    """A whole document. `root` is the relative path from this page to the site's root ("" or
    "../"), so that every link works both at a server's root and under a subfolder such as a
    GitHub Pages project site. `crumbs` are (label, href) pairs; the last is the current page.

    The header and `<main>` carry ids that cirrus.js swaps on navigation, so moving between
    pages replaces only what changed, inside a view transition.

    `base`, when given, is an absolute address for the page's links, for a page such as a 404
    that the server answers at any address; `root` is then "".
    """
    trail = "".join(
        (
            f'<a class="c-crumb" href="{esc(href)}">{esc(label)}</a>'
            if href is not None
            else f'<span class="c-crumb" aria-current="page">{esc(label)}</span>'
        )
        for label, href in crumbs
    )
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
{f'<base href="{esc(base)}">' if base else ""}
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>{esc(title)}</title>
<meta name="description" content="{esc(description or title)}">
<meta name="cirrus-root" content="{esc(root)}">
<link rel="stylesheet" href="{esc(assets.url("cirrus.css", root))}">
<script src="{esc(assets.url("cirrus.js", root))}" defer></script>
</head>
<body>
<div class="c-progress-bar" aria-hidden="true"></div>
<header class="c-header" id="c-header">
<nav class="c-crumbs" aria-label="Breadcrumbs"><a class="c-brand" href="{esc(root or "./")}">{esc(brand)}</a>{trail}</nav>
</header>
<main class="c-main" id="c-main">
{body}
</main>
</body>
</html>
"""


def title(text: str, eyebrow: str = "", sub: str = "") -> str:
    """The page's one focal point: a large title, an optional small line above it (an id or
    a category) and an optional quiet line below."""
    above = f'<div class="c-eyebrow">{eyebrow}</div>' if eyebrow else ""
    below = f'<p class="c-subtitle">{sub}</p>' if sub else ""
    return f'<div class="c-titleblock">{above}<h1 class="c-title">{esc(text)}</h1>{below}</div>'


def chip(text: str, tone_name: str = "neutral", tip: str | None = None) -> str:
    return (
        f'<span class="c-chip c-tone-{tone(tone_name)}"'
        f'{attrs(data_tip=tip, tabindex="0" if tip else None)}>{esc(text)}</span>'
    )


def dot(tone_name: str, label: str, tip: str | None = None) -> str:
    """A small coloured dot followed by its label: a status that reads at a glance without a
    box around it."""
    return (
        f'<span class="c-dot c-tone-{tone(tone_name)}"'
        f'{attrs(data_tip=tip, tabindex="0" if tip else None)}>{esc(label)}</span>'
    )


def mono(text: str) -> str:
    return f'<span class="c-mono">{esc(text)}</span>'


def muted(body: str) -> str:
    return f'<span class="c-muted">{body}</span>'


def tip(body: str, text: str) -> str:
    """`body` with a tooltip, shown on hover, keyboard focus or a tap."""
    return (
        f'<span class="c-tip-target" tabindex="0"{attrs(data_tip=text)}>{body}</span>'
    )


def time(iso: str | None, fallback: str = "") -> str:
    """A timestamp. cirrus.js rewrites the text as a relative time ("3h ago") in the viewer's
    clock and shows the full date in a tooltip; the date alone is the text without script.
    """
    if not iso:
        return esc(fallback)
    return (
        f'<time class="c-time" datetime="{esc(iso)}" tabindex="0" '
        f'data-tip="{esc(iso)}">{esc(iso[:10])}</time>'
    )


def link(href: str, body: str, hovercard: str | None = None, cls: str = "") -> str:
    """A link. `hovercard` names a `<template>` id on the page whose content shows in a card
    beside the link on hover or focus."""
    extra = f' class="{esc(cls)}"' if cls else ""
    return f'<a href="{esc(href)}"{extra}{attrs(data_hovercard=hovercard)}>{body}</a>'


def template(id_: str, body: str) -> str:
    return f'<template id="{esc(id_)}">{body}</template>'


def section(
    heading: str, body: str, count: int | None = None, id_: str | None = None
) -> str:
    """A titled group. Space and a small heading separate it from the next; there is no rule
    or box (principle 3)."""
    number = f'<span class="c-count">{count}</span>' if count is not None else ""
    title_html = (
        f'<h2 class="c-section-title">{esc(heading)}{number}</h2>' if heading else ""
    )
    return f'<section class="c-section"{attrs(id=id_)}>{title_html}{body}</section>'


def subsection(heading: str, body: str, count: int | None = None) -> str:
    """A group inside a section, under a quieter heading, separated by space."""
    number = f'<span class="c-count">{count}</span>' if count is not None else ""
    return (
        f'<div class="c-subsection"><h3 class="c-subsection-title">{esc(heading)}'
        f"{number}</h3>{body}</div>"
    )


def grid(cards: Iterable[str]) -> str:
    return f'<div class="c-grid">{"".join(cards)}</div>'


def card(href: str, body: str, hovercard: str | None = None) -> str:
    """A whole-card link. cirrus.js prefetches cards as they scroll into view, so opening
    one is instant."""
    return (
        f'<a class="c-card" href="{esc(href)}" data-prefetch'
        f"{attrs(data_hovercard=hovercard)}>{body}</a>"
    )


def rows(items: Iterable[str]) -> str:
    """A list of rows separated by space, not lines."""
    return f'<ul class="c-rows">{"".join(f"<li>{i}</li>" for i in items)}</ul>'


def row(main: str, trailing: str = "", href: str | None = None) -> str:
    inner = f'<span class="c-row-main">{main}</span><span class="c-row-trailing">{trailing}</span>'
    if href is None:
        return f'<div class="c-row">{inner}</div>'
    return f'<a class="c-row" href="{esc(href)}" data-prefetch>{inner}</a>'


def meta(items: Sequence[tuple[str, str]]) -> str:
    """Label and value pairs, labels quiet and values in ink: a property list."""
    body = "".join(
        f'<div class="c-meta-item"><dt>{esc(label)}</dt><dd>{value}</dd></div>'
        for label, value in items
    )
    return f'<dl class="c-meta">{body}</dl>'


def progress(done: int, total: int, tip_text: str | None = None) -> str:
    """A thin bar with its count, for how many of a group are finished."""
    share = 0 if total == 0 else round(100 * done / total)
    return (
        f'<span class="c-progress"{attrs(data_tip=tip_text, tabindex="0" if tip_text else None)}>'
        f'<span class="c-progress-track"><span class="c-progress-fill" style="width:{share}%">'
        f'</span></span><span class="c-progress-label">{done}/{total}</span></span>'
    )


def disclosure(summary: str, body: str, open_: bool = False) -> str:
    """Secondary content one click away (principle 2)."""
    opened = " open" if open_ else ""
    return (
        f'<details class="c-disclosure"{opened}><summary>{summary}</summary>'
        f'<div class="c-disclosure-body">{body}</div></details>'
    )


def skeleton(lines: int = 3) -> str:
    """Placeholder lines shaped like the content that will replace them, so the page does not
    move when it arrives."""
    widths = (92, 78, 85, 64, 88, 70)
    bars = "".join(
        f'<span class="c-skeleton-line" style="width:{widths[i % len(widths)]}%"></span>'
        for i in range(lines)
    )
    return f'<div class="c-skeleton" aria-busy="true" aria-label="Loading">{bars}</div>'


def include(url: str, missing: str, lines: int = 3) -> str:
    """A fragment fetched after the page shows: a skeleton until the HTML at `url` arrives,
    then that HTML, faded in; `missing` if the address answers with an error."""
    return (
        f'<div class="c-include"{attrs(data_include=url, data_missing=missing)}>'
        f"{skeleton(lines)}</div>"
    )


def empty(text: str) -> str:
    return f'<p class="c-empty">{esc(text)}</p>'


def prose(html: str) -> str:
    return f'<div class="c-prose">{html}</div>'


def layout(main: str, aside: str) -> str:
    """A reading column with a side rail of properties, which moves above the column, in a
    compact grid, on a narrow screen. The page's title goes before it, across both."""
    return (
        f'<div class="c-layout"><div class="c-layout-main">{main}</div>'
        f'<aside class="c-layout-aside">{aside}</aside></div>'
    )


def timeline(entries: Iterable[str]) -> str:
    return f'<ol class="c-timeline">{"".join(entries)}</ol>'


def timeline_entry(body: str, when: str, who: str = "", quiet: bool = False) -> str:
    cls = "c-timeline-entry c-quiet" if quiet else "c-timeline-entry"
    actor = f'<span class="c-timeline-actor">{who}</span>' if who else ""
    return (
        f'<li class="{cls}"><div class="c-timeline-body">{actor}{body}</div>'
        f'<div class="c-timeline-when">{when}</div></li>'
    )


def comment(author: str, shown: str, when: str, body: str) -> str:
    """A comment: `author` is the full name, whose first letter is the avatar, and `shown`
    the name as the page shows it (HTML)."""
    initial = esc((author.strip() or "?")[0].upper())
    return (
        f'<article class="c-comment"><div class="c-avatar" aria-hidden="true">{initial}</div>'
        f'<div class="c-comment-main"><header class="c-comment-head">'
        f'<span class="c-comment-author">{shown}</span>{when}</header>'
        f'<div class="c-prose">{body}</div></div></article>'
    )


def stats(items: Sequence[tuple[str, str, str | None]]) -> str:
    """A row of large numbers with small labels, for a summary under a page's title. Each is
    (label, value, href); a stat with an href links to what it counts, such as a section's
    anchor."""
    body = "".join(
        f'<{"a" if href else "div"} class="c-stat"{attrs(href=href)}>'
        f'<span class="c-stat-value">{esc(value)}</span>'
        f'<span class="c-stat-label">{esc(label)}</span></{"a" if href else "div"}>'
        for label, value, href in items
    )
    return f'<div class="c-stats">{body}</div>'
