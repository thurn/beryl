"""The site's pages, built from cirrus components: the home page of top-level beads and one
page per bead. Links are relative (`beads/by-x` from the home page, `by-x` between beads),
so the same files work at a server's root and under a GitHub Pages project folder."""

from __future__ import annotations

import functools
import re
from cirrus import components as c
from cirrus import markdown
from cirrus.assets import Assets

from .beads import Bead, Index
from .changelog import Changelog, Event

BRAND = "Beryl"
STATUS_TONES: dict[str, str] = {
    "in_progress": "blue",
    "blocked": "amber",
    "open": "neutral",
    "deferred": "muted",
    "closed": "green",
}
STATUS_LABELS: dict[str, str] = {
    "in_progress": "In progress",
    "blocked": "Blocked",
    "open": "Open",
    "deferred": "Deferred",
    "closed": "Closed",
}
PRIORITY_TONES: dict[int, str] = {0: "red", 1: "orange", 2: "neutral"}
PRIORITY_NAMES: dict[int, str] = {
    0: "Critical",
    1: "High",
    2: "Medium",
    3: "Low",
    4: "Backlog",
}
BEAD_ID: re.Pattern[str] = re.compile(r"\bby-[a-z0-9]+(?:\.[0-9]+)*\b")
SESSION: re.Pattern[str] = re.compile(r"^local_([0-9a-f]{6})[0-9a-f-]*$")
VIZIER: re.Pattern[str] = re.compile(r"^vizier:([^:]+):")
UUID: re.Pattern[str] = re.compile(
    r"^([0-9a-f]{8})-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
)
# Changed fields an event names in words, and what it says about any other.
FIELD_WORDS: dict[str, str] = {
    "description": "edited the description",
    "acceptance_criteria": "edited the acceptance criteria",
    "design": "edited the design",
    "notes": "edited the notes",
    "close_reason": "set the close reason",
    "issue_type": "changed the type",
}


def status_label(status: str) -> str:
    return STATUS_LABELS.get(status, status.replace("_", " ").capitalize())


def status_dot(status: str, label: bool = True) -> str:
    tone = STATUS_TONES.get(status, "neutral")
    text = status_label(status)
    return c.dot(tone, text) if label else c.dot(tone, "", tip=text)


def priority_chip(priority: int) -> str:
    return c.chip(
        f"P{priority}",
        PRIORITY_TONES.get(priority, "muted"),
        tip=PRIORITY_NAMES.get(priority, ""),
    )


def person(name: str) -> str:
    """A person or agent, short: an Enact session id as `session ff7437`, a vizier as
    `vizier on jarvis`; the full name shows in a tooltip whenever it was shortened."""
    if not name:
        return c.muted("Nobody")
    short = name
    if m := SESSION.match(name):
        short = f"session {m.group(1)}"
    elif m := VIZIER.match(name):
        short = f"vizier on {m.group(1)}"
    elif m := UUID.match(name):
        short = m.group(1)
    if short == name:
        return c.esc(name)
    return c.tip(c.esc(short), name)


class Linker:
    """Links between pages, relative to the page being rendered, and the hovercards of the
    beads the page mentions, which it collects as it goes."""

    def __init__(self, index: Index, prefix: str) -> None:
        self.index = index
        self.prefix = prefix
        self.cards: dict[str, str] = {}

    def href(self, bead_id: str) -> str:
        return f"{self.prefix}{bead_id}"

    def card_id(self, bead_id: str) -> str:
        """The id of the bead's hovercard template, made the first time it is asked for."""
        key = "hc-" + bead_id.replace(".", "-")
        if bead_id not in self.cards:
            bead = self.index.beads[bead_id]
            self.cards[bead_id] = c.template(key, hovercard(bead, self.index))
        return key

    def mention(self, escaped: str) -> str:
        """Bead ids in already-escaped text, linked, with a hovercard each."""

        def one(m: re.Match[str]) -> str:
            bead_id = m.group(0)
            if bead_id not in self.index.beads:
                return bead_id
            return c.link(self.href(bead_id), c.mono(bead_id), self.card_id(bead_id))

        return BEAD_ID.sub(one, escaped)

    def bead(self, bead_id: str, body: str | None = None) -> str:
        return c.link(
            self.href(bead_id),
            body if body is not None else c.esc(bead_id),
            self.card_id(bead_id),
        )

    def templates(self) -> str:
        return "".join(self.cards.values())


def hovercard(bead: Bead, index: Index) -> str:
    count = index.progress(bead.id)
    extra = c.progress(count.closed, count.total) if count.total else ""
    return (
        f'<div class="c-card-top">{c.mono(bead.id)}{priority_chip(bead.priority)}</div>'
        f'<div class="c-hovercard-title">{c.esc(bead.title)}</div>'
        f'<div class="c-hovercard-meta">{status_dot(bead.status)}'
        f"<span>{c.esc(bead.issue_type)}</span>{extra}</div>"
    )


def children_progress(bead: Bead, index: Index) -> str:
    count = index.progress(bead.id)
    if not count.total:
        return ""
    to_do = count.total - count.closed - count.working
    return c.progress(
        count.closed,
        count.total,
        f"{count.closed} closed, {count.working} in progress, {to_do} to do",
    )


def references(bead: Bead, index: Index, log: Changelog | None) -> set[str]:
    """Every bead the bead's page links to: its epics, children, blockers and related beads,
    and the beads its text and changelog mention."""
    found: set[str] = set()
    current = index.parent.get(bead.id)
    while current is not None and current not in found:
        found.add(current)
        current = index.parent.get(current)
    for table in (index.children, index.blocked_by, index.blocking, index.related):
        found.update(table.get(bead.id, ()))
    found.update(i for i in mentions(bead, log) if i in index.beads)
    return found


@functools.lru_cache(maxsize=16384)
def mentions(bead: Bead, log: Changelog | None) -> frozenset[str]:
    """The bead ids the bead's text and changelog mention. Cached, because each rebuild asks
    for every bead's and the texts rarely change."""
    texts = [
        bead.description,
        bead.acceptance_criteria,
        bead.design,
        bead.notes,
        bead.close_reason,
        *(x.text for x in bead.comments),
        *(e.text for e in (log.events if log is not None else ())),
    ]
    return frozenset(i for text in texts for i in BEAD_ID.findall(text))


# ---- The home page --------------------------------------------------------------


def home_card(
    bead: Bead, index: Index, kind_shown: bool = True, state_shown: bool = True
) -> str:
    """A bead's card. `kind_shown` False leaves out the Epic chip, in the Epics section,
    and `state_shown` False the status, in Active, where every card would carry it."""
    epic = bead.issue_type == "epic" and kind_shown
    kind = c.chip("Epic", "violet") if epic else ""
    # Open is the usual state, so only the other states earn a dot.
    state = status_dot(bead.status) if bead.status != "open" and state_shown else ""
    foot = [
        state,
        kind,
        children_progress(bead, index),
        person(bead.assignee) if bead.assignee else "",
        f'<span class="c-card-time">{c.time(bead.updated_at)}</span>',
    ]
    return c.card(
        f"beads/{bead.id}",
        f'<div class="c-card-top">{c.mono(bead.id)}{priority_chip(bead.priority)}</div>'
        f'<h3 class="c-card-title">{c.esc(bead.title)}</h3>'
        f'<div class="c-card-foot">{"".join(f for f in foot if f)}</div>',
    )


def cards(
    beads: list[Bead], index: Index, kind_shown: bool = True, state_shown: bool = True
) -> str:
    return c.grid(home_card(b, index, kind_shown, state_shown) for b in beads)


def home(index: Index, assets: Assets) -> str:
    """Two parts: Active, every bead in progress, and All, the open beads by category. A
    bead in progress shows in both. A sticky table of contents beside them leads to each
    part and category."""
    found = index.home()
    automatic = sum(len(beads) for _, beads in found.automatic)
    # (heading, anchor, count, body) for each category of All, in the page's order.
    parts: list[tuple[str, str, int, str]] = [
        ("Epics", "epics", len(found.epics), cards(found.epics, index, False)),
        ("Beads", "beads", len(found.beads), cards(found.beads, index)),
        (
            "Filed automatically",
            "automatic",
            automatic,
            "".join(
                c.subsection(name, cards(beads, index), count=len(beads), level=4)
                for name, beads in found.automatic
            ),
        ),
        (
            "Bug reports",
            "bug-reports",
            len(found.bug_reports),
            cards(found.bug_reports, index),
        ),
        ("Deferred", "deferred", len(found.deferred), cards(found.deferred, index)),
    ]
    shown = [p for p in parts if p[2]]
    total = sum(count for _, _, count, _ in shown)
    active = c.part(
        "Active",
        (
            cards(found.active, index, state_shown=False)
            if found.active
            else c.empty("Nothing is in progress.")
        ),
        "active",
        count=len(found.active),
    )
    everything = c.part(
        "All",
        "".join(
            c.section(heading, body, count=count, id_=anchor, level=3)
            for heading, anchor, count, body in shown
        )
        or c.empty("No open beads."),
        "all",
        count=total,
    )
    nav = c.toc(
        [
            ("Active", "active", len(found.active), 0),
            ("All", "all", total, 0),
            *((heading, anchor, count, 1) for heading, anchor, count, _ in shown),
        ]
    )
    head = c.title("Beads", sub="What is in progress, and everything open.")
    return c.page(
        title="Beads · Beryl",
        body=head + c.with_toc(nav, active + everything),
        root="",
        assets=assets,
        crumbs=[("Beads", None)],
        brand=BRAND,
        description="Project Beryl's open beads",
    )


# ---- A bead's page --------------------------------------------------------------


def bead_row(bead: Bead, index: Index, link: Linker) -> str:
    main = (
        f"{status_dot(bead.status, label=False)}"
        f'<span class="c-row-id c-mono c-hide-narrow">{c.esc(bead.id)}</span>'
        f'<span class="c-row-text">{c.esc(bead.title)}</span>'
    )
    trailing = children_progress(bead, index) + priority_chip(bead.priority)
    return c.row(main, trailing, href=link.href(bead.id))


def related_section(
    heading: str,
    ids: list[str],
    index: Index,
    link: Linker,
    show_done: bool = True,
    id_: str | None = None,
) -> str:
    if not ids:
        return ""
    beads = index.get(ids)
    open_beads = [b for b in beads if not b.closed]
    closed = [b for b in beads if b.closed]
    body = c.rows(bead_row(b, index, link) for b in open_beads) if open_beads else ""
    if closed and show_done:
        rows = c.rows(bead_row(b, index, link) for b in closed)
        if open_beads:
            body += c.disclosure(f"{len(closed)} closed", rows)
        else:
            body += rows
    return c.section(heading, body, count=len(beads), id_=id_)


def describe(event: Event, link: Linker) -> tuple[str, str]:
    """What an event did, as HTML, and a detail line under it (or "")."""
    fields = dict(event.fields)
    detail = ""
    if event.kind == "created":
        what = "created this"
    elif event.kind == "started":
        what = "started work"
    elif event.kind == "closed":
        what = "closed this"
        detail = event.text
    elif event.kind == "reopened":
        what = "reopened this"
    elif event.kind in ("label_added", "label_removed"):
        label = event.text.split(":", 1)[-1].strip() if event.text else ""
        verb = "added" if event.kind == "label_added" else "removed"
        what = f"{verb} label {c.chip(label, 'muted')}"
    elif event.kind in ("updated", "claimed"):
        parts = ["claimed this"] if event.kind == "claimed" else []
        for name, value in event.fields:
            if name == "status":
                if event.kind != "claimed":
                    parts.append(f"changed status to {status_dot(value)}")
            elif name == "priority":
                parts.append(f"set priority to {priority_chip(int(value))}")
            elif name == "assignee":
                if event.kind != "claimed":
                    parts.append(
                        f"assigned {person(value)}" if value else "unassigned it"
                    )
            elif name == "title":
                parts.append(f"renamed it to “{c.esc(value)}”")
            else:
                parts.append(FIELD_WORDS.get(name, f"changed {c.esc(name)}"))
        if event.kind == "claimed" and fields.get("assignee"):
            parts[0] = f"claimed this for {person(fields['assignee'])}"
        what = ", ".join(parts) or "updated Enact's records"
    else:
        what = c.esc(event.kind.replace("_", " "))
        detail = event.text
    if detail:
        detail = link.mention(c.esc(detail))
    return what, detail


def timeline(log: Changelog, link: Linker) -> str:
    """A changelog as a timeline, newest first. A run of events that changed only Enact's
    bookkeeping folds into one quiet line."""
    entries: list[str] = []
    events = list(log.events)
    i = 0
    while i < len(events):
        event = events[i]
        if event.quiet:
            j = i
            while j < len(events) and events[j].quiet:
                j += 1
            count = j - i
            noun = "update" if count == 1 else "updates"
            entries.append(
                c.timeline_entry(
                    f"{count} bookkeeping {noun} by Enact",
                    c.time(event.at),
                    quiet=True,
                )
            )
            i = j
            continue
        what, detail = describe(event, link)
        # The same change by the same person, again and again (a description edited in
        # several passes), shows once with a count.
        j = i + 1
        while (
            j < len(events)
            and not detail
            and not events[j].quiet
            and events[j].actor == event.actor
            and describe(events[j], link) == (what, detail)
        ):
            j += 1
        times = f' <span class="c-muted">×{j - i}</span>' if j - i > 1 else ""
        body = what + times
        body += f'<span class="c-timeline-detail">{detail}</span>' if detail else ""
        entries.append(
            c.timeline_entry(
                body, c.time(event.at), person(event.actor) if event.actor else ""
            )
        )
        i = j
    if not entries:
        return c.empty("No changes recorded.")
    note = (
        ""
        if log.recorded
        else '<p class="c-empty">bd recorded no events for this bead; these come from its '
        "timestamps.</p>"
    )
    return c.timeline(entries) + note


def changelog_fragment(log: Changelog, index: Index) -> str:
    """The changelog as the fragment a page loads after it shows, linking from the
    `changelog/` folder to the bead pages beside it."""
    link = Linker(index, "../beads/")
    return timeline(log, link) + link.templates()


def properties(bead: Bead, index: Index, link: Linker) -> str:
    epic = index.epic(bead.id)
    items: list[tuple[str, str]] = [
        ("Status", status_dot(bead.status)),
        (
            "Priority",
            f"{priority_chip(bead.priority)}"
            f'<span class="c-muted">{c.esc(PRIORITY_NAMES.get(bead.priority, ""))}</span>',
        ),
        ("Assignee", person(bead.assignee)),
    ]
    if epic is not None:
        items.append(("Epic", link.bead(epic.id, c.esc(epic.title))))
    if bead.labels:
        items.append(
            ("Labels", "".join(c.chip(label, "muted") for label in bead.labels))
        )
    items.append(("Type", c.esc(bead.issue_type.capitalize())))
    items.append(("Created", c.time(bead.created_at)))
    items.append(("Updated", c.time(bead.updated_at)))
    if bead.closed_at:
        items.append(("Closed", c.time(bead.closed_at)))
    return c.meta(items)


# The fewest sections a bead's page has before it shows a table of contents.
TOC_MINIMUM = 3


def anchor(heading: str) -> str:
    """A section's id from its heading: "Blocked by" is `blocked-by`."""
    return heading.lower().replace(" ", "-")


def details(bead: Bead) -> str:
    """The bead's bookkeeping, one click away: who made it, and Enact's metadata."""
    items: list[tuple[str, str]] = []
    if bead.created_by:
        items.append(("Created by", c.esc(bead.created_by)))
    if bead.owner:
        items.append(("Owner", c.esc(bead.owner)))
    if bead.started_at:
        items.append(("Started", c.time(bead.started_at)))
    items += [(k, f'<span class="c-mono">{c.esc(v)}</span>') for k, v in bead.metadata]
    if not items:
        return ""
    return c.section("", c.disclosure("Details", c.meta(items)))


def bead_page(
    bead: Bead,
    index: Index,
    assets: Assets,
    log: Changelog | None,
) -> str:
    link: Linker = Linker(index, "")

    def render(source: str) -> str:
        return c.prose(markdown.render(source, link.mention))

    eyebrow = (
        f"{c.mono(bead.id)}"
        + (c.chip("Epic", "violet") if bead.issue_type == "epic" else "")
        + status_dot(bead.status)
    )
    head = c.title(bead.title, eyebrow=eyebrow)
    main: list[str] = []
    # (label, anchor, count, depth) for the table of contents, one per section shown.
    entries: list[tuple[str, str, int | None, int]] = []

    def add(heading: str, html: str, count: int | None = None) -> None:
        if not html:
            return
        main.append(html)
        entries.append((heading, anchor(heading), count, 0))

    if bead.closed and bead.close_reason:
        add(
            "Close reason",
            c.section(
                "Close reason", render(bead.close_reason), id_=anchor("Close reason")
            ),
        )
    for heading, source in (
        ("Description", bead.description),
        ("Acceptance criteria", bead.acceptance_criteria),
        ("Design", bead.design),
        ("Notes", bead.notes),
    ):
        if source.strip():
            add(heading, c.section(heading, render(source), id_=anchor(heading)))
    # Child epics are milestones, listed apart from the child tasks they are made of.
    children = index.children.get(bead.id, [])
    milestones = [i for i in children if index.beads[i].issue_type == "epic"]
    subtasks = [i for i in children if index.beads[i].issue_type != "epic"]
    for heading, ids in (
        ("Milestones", milestones),
        ("Subtasks", subtasks),
        ("Blocked by", index.blocked_by.get(bead.id, [])),
        ("Blocking", index.blocking.get(bead.id, [])),
        ("Related", index.related.get(bead.id, [])),
    ):
        add(
            heading,
            related_section(heading, ids, index, link, id_=anchor(heading)),
            len(ids),
        )
    if bead.comments:
        add(
            "Comments",
            c.section(
                "Comments",
                "".join(
                    c.comment(
                        x.author,
                        person(x.author),
                        c.time(x.created_at),
                        markdown.render(x.text, link.mention),
                    )
                    for x in bead.comments
                ),
                count=len(bead.comments),
                id_="comments",
            ),
            len(bead.comments),
        )
    add(
        "Changelog",
        c.section(
            "Changelog",
            (
                timeline(log, link)
                if log is not None
                else c.include(
                    f"../changelog/{bead.id}.html", "No changelog recorded yet.", 4
                )
            ),
            id_="changelog",
        ),
    )
    main.append(details(bead))
    crumbs: list[tuple[str, str | None]] = [("Beads", "../")]
    chain: list[Bead] = []
    epic = index.epic(bead.id)
    while epic is not None and len(chain) < 4 and epic not in chain:
        chain.insert(0, epic)
        epic = index.epic(epic.id)
    crumbs += [(e.id, e.id) for e in chain]
    crumbs.append((bead.id, None))
    content = c.layout("".join(main), properties(bead, index, link))
    # A short page has nothing to jump between, so contents show only from three sections.
    if len(entries) >= TOC_MINIMUM:
        content = c.with_toc(c.toc(entries), content)
    body = head + content
    body += link.templates()
    return c.page(
        title=f"{bead.title} · {bead.id}",
        body=body,
        root="../",
        assets=assets,
        crumbs=crumbs,
        brand=BRAND,
        description=bead.title,
    )


def not_found(assets: Assets, base: str) -> str:
    """The page for an unknown address. A server answers it at any address, so its links
    resolve against `base`, the site's root as an absolute path."""
    return c.page(
        title="Not found · Beryl",
        body=c.title("Not found", sub="No bead has this address.")
        + '<p><a class="c-muted" href="./">Back to beads</a></p>',
        root="",
        assets=assets,
        crumbs=[("Beads", "./")],
        brand=BRAND,
        base=base,
    )
