"""`model-trial pair`: record the user's listed tickets, mark a block of them, plant a false
premise, title the copies and file each pair as one atomic batch. A pair's row in
`pairs.jsonl` goes `listed` (by `add`), `marked` (by `next`), then `filed` (by `file`).

Filing a pair updates the real ticket (the source arm, which the pool works and lands as
usual) and creates its copy (a root ticket that a worker on the copy arm works from a frozen
commit and never lands). The two arms read the same words, so any planted premise is written
into the source before the copy is made from it.
"""

from __future__ import annotations

import argparse
import secrets
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from . import candidates, copytext, io, manifest
from .manifest import FILED, LISTED, MARKED, SKIPPED, Pair
from .spec import Spec

# Words that would tell either session its ticket is one arm of a trial.
MARKING_WORDS = ("trial", "copy", "sonnet", "opus", "duplicate", "pilot")
# Both arms of a filed pair run before other work, so that a block waits as little as it can
# for its slowest source. A source is real work the pool does anyway, so moving it up costs
# nothing.
FILED_PRIORITY = 1


class PairError(ValueError):
    pass


def new_pair(trial: str, ticket: dict[str, Any], order: int) -> Pair:
    return Pair(
        pair=secrets.token_hex(4),
        trial=trial,
        source=str(ticket["id"]),
        order=order,
        kind=candidates.kind_of(candidates.ticket_text(ticket)),
        previous_allocation=ticket.get("allocation"),
    )


def mark_block(
    pairs: list[Pair],
    block: int,
    count: int,
    limit: int,
    excluded: Callable[[Pair], str | None] = lambda _: None,
) -> tuple[list[Pair], list[Pair]]:
    """Marks the next `count` listed pairs, in list order, as block `block`, never taking the
    trial past `limit` marked or filed pairs. A listed pair whose source can no longer be an
    arm (`excluded` gives the reason) is skipped and the next one taken in its place. Returns
    the pairs it marked and the pairs it skipped."""
    taken = sum(p.state in (MARKED, FILED) for p in pairs)
    room = max(0, min(count, limit - taken))
    chosen: list[Pair] = []
    skipped: list[Pair] = []
    for pair in sorted(pairs, key=lambda p: p.order):
        if len(chosen) >= room:
            break
        if pair.state != LISTED:
            continue
        reason = excluded(pair)
        if reason is not None:
            pair.state, pair.note = SKIPPED, f"skipped by pair next: {reason}"
            skipped.append(pair)
            continue
        pair.state = MARKED
        pair.block = block
        chosen.append(pair)
    return chosen, skipped


def unpark(pair: Pair) -> dict[str, Any]:
    """The update that gives a skipped source back the allocation `pair add` took from it, so
    that the pool works it as the real ticket it is."""
    return {
        "op": "update",
        "id": pair.source,
        "set": {"allocation": pair.previous_allocation},
    }


def plant(
    pair: Pair, source: dict[str, Any], find: str, replace: str, expect: str
) -> None:
    if pair.state not in (LISTED, MARKED):
        raise PairError(f"pair {pair.pair} is {pair.state}: plant before filing")
    if not find or find == replace:
        raise PairError("the planted sentence must differ from the one it replaces")
    count = candidates.ticket_text(source).count(find)
    if count != 1:
        raise PairError(
            f"the sentence appears {count} times in {source['id']}'s text, not exactly once"
        )
    pair.plant = {"find": find, "replace": replace, "expect": expect}


def check_title(title: str, source: dict[str, Any]) -> None:
    """A copy's title says the same request in other words and gives nothing away."""
    words = title.lower()
    if words.strip() == str(source.get("title", "")).lower().strip():
        raise PairError("the copy's title must not be the source's title")
    marked = [w for w in MARKING_WORDS if w in words]
    if marked:
        raise PairError(f"the copy's title must not say {', '.join(marked)}")
    if str(source["id"]).lower() in words:
        raise PairError("the copy's title must not name the source")


@dataclass
class Filing:
    """What `pair file` sends for one pair, and what it changed beyond the two arms."""

    body: dict[str, Any]
    copy_index: int
    opened: list[dict[str, Any]]


def build_filing(
    pair: Pair,
    spec: Spec,
    source: dict[str, Any],
    tickets: dict[str, dict[str, Any]],
    start_commit: str,
    then: str | None,
    siblings_parked: dict[str, int | None],
    home_machine: str | None,
) -> Filing:
    """The batch that files `pair`: it opens the source's parked epic (when every other listed
    ticket of that epic is parked by its own allocation, given in `siblings_parked`), updates
    the source, creates the copy and makes `then` wait on both."""
    if not pair.copy_title:
        raise PairError(
            f"pair {pair.pair} ({pair.source}) has no copy title: run `pair title` first"
        )
    sid = str(source["id"])
    ops: list[dict[str, Any]] = []
    opened: list[dict[str, Any]] = []
    parent = tickets.get(str(source.get("parent") or ""))
    if parent is not None:
        changes: dict[str, Any] = {}
        if parent.get("allocation") == 0:
            unparked = sorted(t for t, a in siblings_parked.items() if a != 0)
            if unparked:
                raise PairError(
                    f"{parent['id']} is parked, but its listed tickets "
                    f"{', '.join(unparked)} are not parked on their own: opening it "
                    "would start them"
                )
            changes["allocation"] = 1
        if parent.get("status") == "pending_approval":
            changes["status"] = "open"
        if changes:
            ops.append({"op": "update", "id": str(parent["id"]), "set": changes})
            opened += [
                {
                    "ticket": str(parent["id"]),
                    "field": k,
                    "before": parent.get(k),
                    "after": v,
                }
                for k, v in changes.items()
            ]

    own = candidates.labels(source)
    wanted = [f"provider:{spec.source.provider}", f"effort:{spec.source.effort}"]
    if not candidates.label_values(source, "tier:"):
        wanted.append(f"tier:{spec.source.tier}")
    update: dict[str, Any] = {
        "allocation": pair.previous_allocation,
        "priority": FILED_PRIORITY,
    }
    if source.get("status") == "pending_approval":
        update["status"] = "open"
        opened.append(
            {
                "ticket": sid,
                "field": "status",
                "before": "pending_approval",
                "after": "open",
            }
        )
    if pair.plant is not None:
        for key in ("body", "acceptance", "notes"):
            text = str(source.get(key) or "")
            if pair.plant["find"] in text:
                update[key] = copytext.planted(text, pair.plant)
    ops.append(
        {
            "op": "update",
            "id": sid,
            "set": update,
            "add_labels": [l for l in wanted if l not in own],
        }
    )

    interface = pair.kind == "interface"
    copy_labels = [
        candidates.COPY_LABEL,
        f"trial:pair-{pair.pair}",
        *spec.copy.labels(),
    ]
    pin = candidates.machine_pin(source, tickets) or home_machine
    if pin:
        copy_labels.append(f"machine:{pin}")
    fields: dict[str, Any] = {
        "title": pair.copy_title,
        "body": copytext.body(source, parent, start_commit, pair.plant, interface),
        "acceptance": copytext.acceptance(source, pair.plant),
        "type": source.get("type") or "task",
        "priority": FILED_PRIORITY,
        "labels": copy_labels,
        "status": "open",
    }
    if os_needed := candidates.needs_os(source, tickets):
        fields["needs_os"] = os_needed
    if source.get("estimate_minutes"):
        fields["estimate_minutes"] = source["estimate_minutes"]
    copy_index = len(ops)
    ops.append({"op": "create", "fields": fields})
    if then:
        ops.append({"op": "dep_add", "id": then, "on": sid})
        ops.append({"op": "dep_add", "id": then, "on": f"${copy_index}"})
    body: dict[str, Any] = {"ops": ops}
    if source.get("version") is not None:
        body["if"] = [{"id": sid, "version": source["version"]}]
    return Filing(body=body, copy_index=copy_index, opened=opened)


def audit(pair: Pair, harm: str, note: str) -> None:
    if pair.plant is None:
        raise PairError(f"pair {pair.pair} has no planted premise to audit")
    if harm != "none" and not (
        harm.startswith("fixed:")
        and manifest.TICKET_ID.fullmatch(harm.removeprefix("fixed:"))
    ):
        raise PairError("--harm is `none` or `fixed:<ticket>`")
    pair.audit = {"harm": harm, "note": note, "at": manifest.now()}


# The commands.


def cmd_add(args: argparse.Namespace, spec: Spec) -> int:
    trial: str = args.trial
    pairs = manifest.read_pairs(trial)
    known = {p.source for p in pairs}
    ids = [t for t in args.tickets if t not in known]
    for t in args.tickets:
        if t in known:
            print(f"{t}: already recorded")
    if not ids:
        return 0
    tickets = io.show(ids)
    ctx = candidates.context(spec, tickets)
    order = max((p.order for p in pairs), default=-1) + 1
    park: list[dict[str, Any]] = []
    refused = 0
    for ticket in tickets:
        pair = new_pair(trial, ticket, order)
        order += 1
        reason = candidates.exclusion(ticket, ctx)
        if reason is not None:
            pair.state, pair.note = SKIPPED, f"refused by pair add: {reason}"
            refused += 1
            print(f"{pair.source}: refused, {reason}")
        else:
            park.append({"op": "update", "id": pair.source, "set": {"allocation": 0}})
            print(f"{pair.source}: listed ({pair.kind}), parked")
        manifest.save_pair(pair)
    if park:
        io.batch({"ops": park})
    return 1 if refused else 0


def exclusions(spec: Spec, pairs: list[Pair]) -> Callable[[Pair], str | None]:
    """Why each pair's source can no longer be an arm, read from the tickets as they are now."""
    tickets = io.with_ancestors(io.show([p.source for p in pairs]))
    ours = {p.source for p in pairs}
    ctx = candidates.Context(
        spec, tickets, io.machine_workers(), candidates.trialed_tickets() - ours
    )
    return lambda p: candidates.exclusion(tickets[p.source], ctx)


def cmd_next(args: argparse.Namespace, spec: Spec) -> int:
    pairs = manifest.read_pairs(args.trial)
    count = args.count if args.count is not None else spec.block_size
    listed = [p for p in pairs if p.state == LISTED]
    excluded = exclusions(spec, listed) if listed else (lambda _: None)
    marked, skipped = mark_block(pairs, args.block, count, spec.pair_limit, excluded)
    for pair in skipped:
        manifest.save_pair(pair)
        print(f"{pair.source}: {pair.note}; its allocation is restored")
    if skipped:
        io.batch({"ops": [unpark(p) for p in skipped]})
    for pair in marked:
        manifest.save_pair(pair)
        print(f"{pair.pair}  {pair.source}  {pair.kind}")
    if not marked:
        print(
            "No listed ticket is left to mark: the list is used up or the trial is full."
        )
    return 0


def _one(args: argparse.Namespace) -> Pair:
    return manifest.find_pair(manifest.read_pairs(args.trial), args.pair)


def cmd_plant(args: argparse.Namespace, spec: Spec) -> int:
    pair = _one(args)
    plant(pair, io.ticket(pair.source), args.find, args.replace_with, args.expect)
    manifest.save_pair(pair)
    print(f"Planted in {pair.source}.")
    return 0


def cmd_title(args: argparse.Namespace, spec: Spec) -> int:
    pair = _one(args)
    if pair.state == FILED:
        raise PairError(f"pair {pair.pair} is already filed")
    check_title(args.title, io.ticket(pair.source))
    pair.copy_title = args.title
    manifest.save_pair(pair)
    return 0


def cmd_audit(args: argparse.Namespace, spec: Spec) -> int:
    pair = _one(args)
    audit(pair, args.harm, args.note)
    manifest.save_pair(pair)
    return 0


def cmd_contamination(args: argparse.Namespace, spec: Spec) -> int:
    pair = _one(args)
    pair.contamination = {
        "decision": "void" if args.void else "clear",
        "note": args.note,
        "at": manifest.now(),
    }
    manifest.save_pair(pair)
    return 0


def cmd_file(args: argparse.Namespace, spec: Spec) -> int:
    trial: str = args.trial
    pairs = manifest.read_pairs(trial)
    marked = [p for p in pairs if p.state == MARKED]
    if not marked:
        print("No marked pair to file.")
        return 0
    # Skip a source that can no longer be an arm before asking for titles, so that a skipped
    # pair needs none.
    excluded = exclusions(spec, marked)
    skipped = [p for p in marked if excluded(p) is not None]
    for pair in skipped:
        pair.state, pair.note = SKIPPED, f"skipped by pair file: {excluded(pair)}"
        manifest.save_pair(pair)
        print(f"{pair.source}: {pair.note}; its allocation is restored")
    if skipped:
        io.batch({"ops": [unpark(p) for p in skipped]})
    marked = [p for p in marked if p.state == MARKED]
    untitled = [p.source for p in marked if not p.copy_title]
    if untitled:
        raise PairError(
            f"no copy title for {', '.join(untitled)}: run `pair title` first"
        )
    models = {
        "source": io.model_ids(spec.source.provider).get(spec.source.tier, ""),
        "copy": io.model_ids(spec.copy.provider).get(spec.copy.tier, ""),
    }
    home = io.this_machine()
    failed = len(skipped)
    for pair in marked:
        source = io.ticket(pair.source)
        tickets = io.with_ancestors([source])
        ctx = candidates.Context(
            spec, tickets, io.machine_workers(), candidates.trialed_tickets(pair.source)
        )
        reason = candidates.exclusion(source, ctx)
        if reason is not None:
            pair.state, pair.note = SKIPPED, f"skipped by pair file: {reason}"
            manifest.save_pair(pair)
            io.batch({"ops": [unpark(pair)]})
            print(f"{pair.source}: {pair.note}; its allocation is restored")
            failed += 1
            continue
        parent = str(source.get("parent") or "")
        sibling_ids = [
            p.source
            for p in pairs
            if p.state in (LISTED, MARKED) and p.source != pair.source
        ]
        siblings = {
            str(t["id"]): t.get("allocation")
            for t in io.show(sibling_ids)
            if str(t.get("parent") or "") == parent and parent
        }
        start = io.staging_tip()
        filing = build_filing(
            pair, spec, source, tickets, start, args.then, siblings, home
        )
        reply = io.batch(filing.body)
        results = reply.get("results", []) if isinstance(reply, dict) else []
        pair.copy = str(results[filing.copy_index]["id"])
        pair.state, pair.start_commit, pair.models = FILED, start, models
        pair.opened, pair.filed_at = filing.opened, manifest.now()
        pair.copied_attachments = copy_attachments(trial, pair)
        manifest.save_pair(pair)
        print(f"{pair.source} -> copy {pair.copy}, from {start[:12]}")
    return 1 if failed else 0


def copy_attachments(trial: str, pair: Pair) -> list[str]:
    """Puts the source's images on the copy under the same names, since both arms' text
    embeds them by name."""
    copy = pair.copy
    assert copy is not None
    names: list[str] = []
    folder = manifest.trial_dir(trial) / pair.pair / "source-attachments"
    for item in io.attachments(pair.source):
        name = str(item["name"])
        path = io.get_attachment(pair.source, name, folder / name)
        io.attach(copy, path, name, item.get("caption"))
        names.append(name)
    return names
