"""Which tickets can be a trial's source arm, and the reason each other one is left out.
`candidates` prints both lists for the user to pick from, and `pair add` refuses a ticket with
the same code, so the two never disagree. Nothing here picks a ticket.
"""

from __future__ import annotations

import argparse
import re
from collections import Counter
from dataclasses import dataclass, field
from typing import Any

from . import io, manifest
from .spec import Spec

COPY_LABEL = "trial:copy"
# A ticket the hub filed: it has no tier until `pair file` gives it the source arm's.
HUB_LABEL = "hub"
# A review's main output is follow-up tickets, which a copy would file a second time.
REVIEW_LABEL = "review"
# A lead's worker verifies and closes its followers, so a copy would close real tickets.
LEAD: re.Pattern[str] = re.compile(r"\*\*Lead\.\*\*|\bFollowers:")
# `blocked_by` rules (`ignite_core::ready`) that mean waiting on another ticket: its own
# dependency (2), an ancestor's dependency (3), or a `last` ticket's siblings (4).
WAITING_RULES = (2, 3, 4)

# The path prefixes of each kind of ticket, matched in this order. A ticket takes the kind
# most of the paths its text names belong to. A path that starts with a crate's name, such as
# `beryl_rules/src/`, is one under `engine/crates/`.
KIND_PREFIXES: tuple[tuple[str, tuple[str, ...]], ...] = (
    (
        "interface",
        (
            "ui/",
            "client/",
            "engine/crates/beryl_app",
            "engine/crates/beryl_mobile",
            "beryl_app",
            "beryl_mobile",
        ),
    ),
    (
        "tooling",
        (
            "tools/",
            "ignite/",
            "efficiency/",
            "data/",
            "docs/",
            "beryl_tools/",
            "beryl_efficiency/",
        ),
    ),
    ("engine", ("engine/cards/", "engine/crates/", "beryl_")),
)
# Which kind a tie goes to.
TIE_ORDER = ("interface", "engine", "tooling")
PATH: re.Pattern[str] = re.compile(r"[A-Za-z0-9_./-]+")


def kind_of(text: str) -> str:
    """`engine`, `interface`, `tooling` or `other`, by the distinct paths `text` names."""
    seen: set[str] = set()
    counts: Counter[str] = Counter()
    for token in PATH.findall(text):
        token = token.lstrip("./")
        if token in seen:
            continue
        for kind, prefixes in KIND_PREFIXES:
            if token.startswith(prefixes):
                seen.add(token)
                counts[kind] += 1
                break
    if not counts:
        return "other"
    return max(counts, key=lambda k: (counts[k], -TIE_ORDER.index(k)))


def ticket_text(ticket: dict[str, Any]) -> str:
    return "\n".join(str(ticket.get(k) or "") for k in ("body", "acceptance", "notes"))


def labels(ticket: dict[str, Any]) -> list[str]:
    return [str(label) for label in ticket.get("labels") or []]


def label_values(ticket: dict[str, Any], prefix: str) -> list[str]:
    return [l.removeprefix(prefix) for l in labels(ticket) if l.startswith(prefix)]


def lineage(
    ticket: dict[str, Any], tickets: dict[str, dict[str, Any]]
) -> list[dict[str, Any]]:
    """`ticket`, then its parent, and so on up, as far as `tickets` knows them."""
    chain = [ticket]
    while (parent := chain[-1].get("parent")) and str(parent) in tickets:
        chain.append(tickets[str(parent)])
        if len(chain) > 32:
            break
    return chain


def machine_pin(
    ticket: dict[str, Any], tickets: dict[str, dict[str, Any]]
) -> str | None:
    """The nearest `machine:` label on the ticket or an ancestor: where its work must run."""
    for t in lineage(ticket, tickets):
        if pins := label_values(t, "machine:"):
            return pins[0]
    return None


def needs_os(ticket: dict[str, Any], tickets: dict[str, dict[str, Any]]) -> str | None:
    for t in lineage(ticket, tickets):
        if t.get("needs_os"):
            return str(t["needs_os"])
    return None


@dataclass
class Context:
    spec: Spec
    # Every known ticket by ID, so that ancestors' pins can be read.
    tickets: dict[str, dict[str, Any]]
    # Machines whose pool takes tickets.
    workers: set[str]
    # Tickets already in a trial (in any trial's `pairs.jsonl`), other than the one asking.
    trialed: set[str] = field(default_factory=set)


def exclusion(ticket: dict[str, Any], ctx: Context) -> str | None:
    """Why `ticket` cannot be the source arm under `ctx.spec`, or None when it can."""
    tid = str(ticket["id"])
    own = labels(ticket)
    source = ctx.spec.source
    if COPY_LABEL in own or tid in ctx.trialed:
        return "already in a trial"
    if ticket.get("type") == "epic":
        return "an epic"
    if ticket.get("status") not in ("open", "pending_approval"):
        return f"not open ({ticket.get('status')})"
    if ticket.get("assignee") or ticket.get("session"):
        return f"claimed by {ticket.get('assignee') or ticket.get('session')}"
    waiting = sorted(
        {
            str(b["id"])
            for b in ticket.get("blocked_by") or []
            if b.get("rule") in WAITING_RULES
        }
    )
    if waiting:
        return f"waiting on {', '.join(waiting)}"
    tiers = label_values(ticket, "tier:")
    if not tiers and HUB_LABEL not in own:
        return "no tier label"
    if tiers and tiers != [source.tier]:
        return f"tier:{', tier:'.join(tiers)}, not tier:{source.tier}"
    for key, wanted in (("provider", source.provider), ("effort", source.effort)):
        pins = label_values(ticket, f"{key}:")
        if pins and pins != [wanted]:
            return f"{key}:{pins[0]} conflicts with the source arm's {key}:{wanted}"
    if REVIEW_LABEL in own:
        return "a review: its main output is tickets, which a copy would file twice"
    if LEAD.search(ticket_text(ticket)):
        return "a lead whose worker closes its followers, which a copy would close too"
    pin = machine_pin(ticket, ctx.tickets)
    if pin is not None and pin not in ctx.workers:
        return f"pinned to machine:{pin}, which has no worker"
    return None


def trialed_tickets(exclude: str | None = None) -> set[str]:
    """Every source and copy recorded in any trial's records."""
    found: set[str] = set()
    base = manifest.root()
    if not base.is_dir():
        return found
    for path in sorted(base.iterdir()):
        if not path.is_dir() or not manifest.TICKET_ID.fullmatch(path.name):
            continue
        for pair in manifest.read_pairs(path.name):
            if pair.state != manifest.SKIPPED:
                found.update(t for t in (pair.source, pair.copy) if t)
    if exclude:
        found.discard(exclude)
    return found


def context(spec: Spec, tickets: list[dict[str, Any]]) -> Context:
    return Context(
        spec=spec,
        tickets=io.with_ancestors(tickets),
        workers=io.machine_workers(),
        trialed=trialed_tickets(),
    )


def run(args: argparse.Namespace, spec: Spec) -> int:
    tickets = io.open_tickets()
    ctx: Context = context(spec, tickets)
    epics: list[str] = args.epic or []
    wanted: list[str] = args.label or []

    def chosen(t: dict[str, Any]) -> bool:
        ancestry = {str(a["id"]) for a in lineage(t, ctx.tickets)[1:]}
        return (not epics or bool(ancestry & set(epics))) and all(
            l in labels(t) for l in wanted
        )

    eligible: list[str] = []
    excluded: list[str] = []
    for t in sorted((t for t in tickets if chosen(t)), key=lambda t: str(t["id"])):
        if t.get("type") == "epic":
            continue
        reason = exclusion(t, ctx)
        kind = kind_of(ticket_text(t))
        line = f"{t['id']:<12} {kind:<9} {t.get('status', ''):<16} {t.get('title', '')}"
        if reason is None:
            eligible.append(line)
        else:
            excluded.append(f"{line}\n{'':<12} left out: {reason}")
    print(f"Eligible as the source arm ({len(eligible)}):")
    print("\n".join(eligible) or "  none")
    print(f"\nLeft out ({len(excluded)}):")
    print("\n".join(excluded) or "  none")
    listed = set(spec.tickets)
    out = [line for line in excluded if line.split()[0] in listed]
    if out:
        print("\nListed in the spec but left out:")
        print("\n".join(out))
    return 0
