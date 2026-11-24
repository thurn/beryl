"""`model-trial report` and `model-trial list`.

`report` prints a trial's table in Markdown, for all pairs and again by kind. Every number
carries the count of pairs behind it. A void pair is listed but counted nowhere else. A rubric
line goes to an arm only when every run of the pair's review picked that arm; a split is a tie.
The cap counts the copies' sessions and the reviewer runs; the source arm's sessions are work
the pool would have paid for anyway, so their cost is printed and left out of the spend.

`report --trial A --trial B` prints the trials side by side, and flags a reviewer model or
rubric version that differs between them, since their rubric counts are then not comparable.
"""

from __future__ import annotations

import argparse
import statistics
import sys
from collections.abc import Callable, Sequence
from dataclasses import dataclass, field
from typing import Any

from . import blind, io, manifest, review
from .manifest import FILED, Grade, Pair, Review, Verdict
from .review import ARMS, RUBRIC
from .spec import Spec, SpecError

KINDS = ("engine", "interface", "tooling", "other")
FAIRNESS = (
    "The copy knew its work would not land, and only the source passed the landing gate "
    "and tip verification and could retry after a rejection, which this trial does not "
    "equalize."
)


@dataclass
class Trial:
    id: str
    spec: Spec
    pairs: list[Pair]
    grades: list[Grade]
    reviews: list[Review]
    verdicts: list[Verdict] = field(default_factory=list)
    # From `io.plan_usage`: `before` and `now`, each a list of readings.
    usage: dict[str, list[dict[str, Any]]] = field(default_factory=dict)


@dataclass
class Result:
    """One graded pair as the report reads it."""

    pair: Pair
    arms: dict[str, Grade]
    void: list[str]
    # Evidence that one arm saw the other, from the grader's scan and the reviewer.
    suspect: list[str]
    # Each rubric line's arm (or `tie`) under the swap rule; None until every run is in.
    picks: dict[str, str] | None
    # Mean defects the reviewer listed per arm over the pair's runs; None until reviewed.
    defects: dict[str, float] | None
    # Per arm, `yes`, `no` or `split` for a planted pair's reviews.
    noticed: dict[str, str] | None

    @property
    def decision(self) -> str | None:
        return (self.pair.contamination or {}).get("decision")


def results(trial: Trial) -> list[Result]:
    pairs = {p.pair: p for p in trial.pairs}
    found: list[Result] = []
    for token, arms in manifest.latest_grades(trial.grades).items():
        pair = pairs.get(token)
        if pair is None:
            continue
        runs = [r for r in trial.reviews if r.pair == token and r.ok]
        reviewed = len(runs) >= trial.spec.reviewer.runs_per_pair and runs
        void = list(arms["source"].void)
        if pair.contamination and pair.contamination.get("decision") == "void":
            void.append(f"voided for contamination: {pair.contamination.get('note')}")
        suspect = [f"{s['what']} ({s['where']})" for s in arms["source"].suspect]
        for run in runs:
            suspect += [
                f"the reviewer (run {run.run}) found both sides share "
                f"{case.get('what', '')} ({case.get('a', '?')} and {case.get('b', '?')})"
                for case in review.shared(run)
            ]
        picks: dict[str, str] | None = None
        defects: dict[str, float] | None = None
        noticed: dict[str, str] | None = None
        if reviewed:
            each = [review.picks(r) for r in runs]
            picks = {
                key: (
                    each[0][key] if all(p[key] == each[0][key] for p in each) else "tie"
                )
                for key in RUBRIC
            }
            counts = [review.defects(r) for r in runs]
            defects = {a: sum(c[a] for c in counts) / len(counts) for a in ARMS}
            marks = [m for m in (review.planted(r) for r in runs) if m is not None]
            if pair.plant is not None and marks:
                noticed = {
                    a: (
                        "yes"
                        if all(m[a] for m in marks)
                        else "no" if not any(m[a] for m in marks) else "split"
                    )
                    for a in ARMS
                }
        found.append(Result(pair, arms, void, suspect, picks, defects, noticed))
    return sorted(found, key=lambda r: r.pair.order)


# Formatting.


def name(spec: Spec, arm: str) -> str:
    return f"{(spec.source if arm == 'source' else spec.copy).tier} ({arm})"


def median_cell(values: Sequence[float | None], unit: str = "") -> tuple[str, float]:
    found = [float(v) for v in values if v is not None]
    if not found:
        return "none (n=0)", 0.0
    m = statistics.median(found)
    shown = f"${m:.2f}" if unit == "$" else f"{m:.1f}"
    return f"{shown} (n={len(found)})", m


def count_cell(hits: int, n: int) -> str:
    return f"{hits} of {n}"


def group_rows(spec: Spec, group: list[Result]) -> list[tuple[str, str, str]]:
    """The table of one group of counted pairs, as (line, source cell, copy cell)."""
    n = len(group)
    rows: list[tuple[str, str, str]] = []

    def per_arm(label: str, cell: Callable[[str], str]) -> None:
        rows.append((label, cell("source"), cell("copy")))

    per_arm("Finished", lambda a: count_cell(sum(r.arms[a].finished for r in group), n))
    per_arm(
        "Checks pass / fail / unknown",
        lambda a: " / ".join(
            str(sum(r.arms[a].check == v for r in group))
            for v in ("pass", "fail", "unknown")
        )
        + f" (n={n})",
    )
    for label, attr, unit in (
        ("Median dollars", "dollars", "$"),
        ("Median agent minutes", "agent_minutes", ""),
        ("Median claim-to-close minutes", "claim_to_close_minutes", ""),
    ):
        s_cell, s_m = median_cell(
            [getattr(r.arms["source"], attr) for r in group], unit
        )
        c_cell, c_m = median_cell([getattr(r.arms["copy"], attr) for r in group], unit)
        if s_m:
            c_cell += f", {c_m / s_m:.0%} of the source's"
        rows.append((label, s_cell, c_cell))
    reviewed = [r for r in group if r.defects is not None]
    per_arm(
        "Defects listed per pair",
        lambda a: (
            f"{statistics.mean(r.defects[a] for r in reviewed if r.defects):.1f}"
            if reviewed
            else "none"
        )
        + f" (n={len(reviewed)})",
    )
    interface = [r for r in group if r.pair.kind == "interface"]
    per_arm(
        "Interface pairs with no image",
        lambda a: count_cell(
            sum(not r.arms[a].attachments for r in interface), len(interface)
        ),
    )
    return rows


def rubric_rows(spec: Spec, group: list[Result]) -> list[str]:
    reviewed = [r for r in group if r.picks is not None]
    lines = [
        f"| Rubric line (n={len(reviewed)} reviewed) | {name(spec, 'source')} "
        f"| {name(spec, 'copy')} | tie |",
        "|---|---|---|---|",
    ]
    for key, title in RUBRIC.items():
        counts = [
            sum(bool(r.picks and r.picks[key] == v) for r in reviewed)
            for v in (*ARMS, "tie")
        ]
        lines.append(f"| {title} | " + " | ".join(str(c) for c in counts) + " |")
    return lines


def group_section(spec: Spec, title: str, group: list[Result]) -> list[str]:
    lines = [
        f"## {title} ({len(group)} counted)",
        "",
        f"| | {name(spec, 'source')} | {name(spec, 'copy')} |",
        "|---|---|---|",
    ]
    lines += [f"| {label} | {s} | {c} |" for label, s, c in group_rows(spec, group)]
    return [*lines, "", *rubric_rows(spec, group), ""]


def money(value: float) -> str:
    return f"${value:,.2f}"


def models_line(trial: Trial) -> str:
    seen: dict[str, set[str]] = {a: set() for a in ARMS}
    for p in trial.pairs:
        for arm in ARMS:
            if p.models.get(arm):
                seen[arm].add(p.models[arm])
    spec = trial.spec
    parts = []
    for arm in ARMS:
        a = spec.source if arm == "source" else spec.copy
        ids = ", ".join(sorted(seen[arm])) or "no pair filed yet"
        parts.append(
            f"{arm} `tier:{a.tier}` on {a.provider} at {a.effort} effort ({ids})"
        )
    return "Arms: " + "; ".join(parts) + "."


def reviewer_of(trial: Trial) -> tuple[set[str], set[str]]:
    """The reviewer model IDs and rubric versions of the trial's reviews."""
    return (
        {r.reviewer_model for r in trial.reviews},
        {r.rubric_version for r in trial.reviews},
    )


def header(trial: Trial, found: list[Result]) -> list[str]:
    spec = trial.spec
    filed = sum(p.state == FILED for p in trial.pairs)
    reviewed = sum(r.picks is not None for r in found)
    models, versions = reviewer_of(trial)
    latest = manifest.latest_grades(trial.grades)
    copies = [float(a["copy"].dollars or 0) for a in latest.values()]
    sources = [
        a["source"].dollars for a in latest.values() if a["source"].dollars is not None
    ]
    reviewer_cost = sum((r.cost_usd for r in trial.reviews), 0.0)
    total = sum(copies, 0.0) + reviewer_cost
    lines = [
        f"# Model trial {trial.id}",
        "",
        models_line(trial),
        f"Reviewer: {', '.join(sorted(models)) or 'no review yet'} at {spec.reviewer.effort} "
        f"effort, rubric version {', '.join(sorted(versions)) or 'none yet'}, "
        f"{spec.reviewer.runs_per_pair} runs per pair.",
        f"Pairs: {len(trial.pairs)} listed, {filed} filed, {len(found)} graded, "
        f"{reviewed} reviewed.",
        f"Spend: {money(total)} of the {money(spec.cap_usd)} cap: the copies "
        f"{money(sum(copies, 0.0))} (n={len(copies)}) and the reviewer {money(reviewer_cost)} "
        f"(n={len(trial.reviews)} runs). The source arm's sessions cost "
        f"{money(sum((float(s) for s in sources), 0.0))} (n={len(sources)}), which the cap leaves out.",
    ]
    lines.append("Plan usage: " + usage_text(trial.usage) + ".")
    lines.append(FAIRNESS)
    return [*lines, ""]


def usage_text(usage: dict[str, list[dict[str, Any]]]) -> str:
    """Each plan window's reading before the first pair was filed and its latest reading."""
    windows: dict[str, dict[str, dict[str, Any]]] = {}
    for key in ("before", "now"):
        for u in usage.get(key, []):
            windows.setdefault(str(u["window"]), {})[key] = u
    parts = []
    for window, found in windows.items():
        shown = [
            f"{found[key]['percent_used']:.0f}% {when} ({found[key]['at']})"
            for key, when in (
                ("before", "before the first pair"),
                ("now", "at the latest reading"),
            )
            if key in found
        ]
        parts.append(f"{window} {', '.join(shown)}")
    return "; ".join(parts) or "no reading in the store"


def planted_section(spec: Spec, found: list[Result]) -> list[str]:
    planted = [r for r in found if r.pair.plant is not None]
    lines = [f"## Planted premises ({len(planted)})", ""]
    for r in planted:
        noticed = r.noticed or {}
        audit = r.pair.audit
        audited = (
            f"the audit found {audit['harm']}: {audit['note']}"
            if audit
            else "not audited yet"
        )
        lines.append(
            f"- {r.pair.source}: {name(spec, 'source')} noticed: "
            f"{noticed.get('source', 'not reviewed')}; {name(spec, 'copy')} noticed: "
            f"{noticed.get('copy', 'not reviewed')}; {audited}."
        )
    return [*lines, ""] if planted else []


def contamination_section(found: list[Result]) -> list[str]:
    suspect = [r for r in found if r.suspect and r.decision is None]
    cleared = [r for r in found if r.decision == "clear"]
    voided = [r for r in found if r.decision == "void"]
    lines = [
        f"## Contamination: {len(suspect)} suspect, {len(cleared)} cleared, "
        f"{len(voided)} voided",
        "",
    ]
    for label, group in (
        ("suspect", suspect),
        ("cleared", cleared),
        ("voided", voided),
    ):
        for r in group:
            note = (r.pair.contamination or {}).get("note")
            lines.append(
                f"- {r.pair.source} / {r.pair.copy}, {label}: "
                + ("; ".join(r.suspect) or "no evidence recorded")
                + (f". Note: {note}" if note else "")
            )
    return [*lines, ""]


def void_section(found: list[Result]) -> list[str]:
    void = [r for r in found if r.void]
    lines = [f"## Void pairs ({len(void)})", ""]
    lines += [f"- {r.pair.source} / {r.pair.copy}: {'; '.join(r.void)}" for r in void]
    return [*lines, ""]


def agreement(trial: Trial, found: list[Result]) -> tuple[int, int]:
    """On how many of the user's judged and reviewed pairs the reviewer's overall pick matched
    his, and how many there are. "No preference" matches a tie."""
    picks = {r.pair.pair: r.picks for r in found if r.picks is not None and not r.void}
    matched = judged = 0
    for v in trial.verdicts:
        reviewer = picks.get(v.pair)
        if reviewer is None:
            continue
        judged += 1
        his = "tie" if v.overall not in ("A", "B") else blind.labels(v.a_arm)[v.overall]
        matched += his == reviewer["overall"]
    return matched, judged


def pair_lines(spec: Spec, found: list[Result]) -> list[str]:
    lines = ["## Pairs", ""]
    for r in found:
        s, c = r.arms["source"], r.arms["copy"]
        state = (
            "void"
            if r.void
            else "suspect" if r.suspect and r.decision is None else "counted"
        )
        overall = r.picks["overall"] if r.picks else "not reviewed"
        lines.append(
            f"- {r.pair.source} / {r.pair.copy} ({r.pair.kind}, {state}): "
            f"source {s.ended}, {money(float(s.dollars or 0))}; copy {c.ended}, "
            f"{money(float(c.dollars or 0))}; overall: {overall}"
        )
    return lines


def table(trial: Trial) -> str:
    found = results(trial)
    counted = [r for r in found if not r.void]
    lines = header(trial, found)
    lines += group_section(trial.spec, "All pairs", counted)
    for kind in KINDS:
        group = [r for r in counted if r.pair.kind == kind]
        if group:
            lines += group_section(trial.spec, f"{kind.capitalize()} pairs", group)
    lines += planted_section(trial.spec, found)
    lines += contamination_section(found)
    lines += void_section(found)
    if trial.verdicts:
        matched, judged = agreement(trial, found)
        lines += [
            f"The reviewer's overall pick matched the user's on {matched} of {judged} "
            "judged pairs.",
            "",
        ]
    lines += pair_lines(trial.spec, found)
    return "\n".join(lines).rstrip() + "\n"


def compare(trials: list[Trial]) -> str:
    """The trials side by side: each one's arms and reviewer, a flag for each reviewer model
    or rubric version that differs, then the all-pairs lines in one column per trial."""
    lines = ["# Model trials " + ", ".join(t.id for t in trials), ""]
    for t in trials:
        models, versions = reviewer_of(t)
        lines += [
            f"## {t.id}",
            "",
            models_line(t),
            f"Reviewer: {', '.join(sorted(models)) or 'none yet'}; rubric version "
            f"{', '.join(sorted(versions)) or 'none yet'}.",
            "",
        ]
    for label, index in (("reviewer model", 0), ("rubric version", 1)):
        seen = {t.id: reviewer_of(t)[index] for t in trials}
        if len({frozenset(v) for v in seen.values() if v}) > 1:
            each = "; ".join(f"{k}: {', '.join(sorted(v))}" for k, v in seen.items())
            lines.append(
                f"**The {label} differs** ({each}), so the rubric counts are not comparable."
            )
    lines += [
        "",
        "| | " + " | ".join(t.id for t in trials) + " |",
        "|---|" + "---|" * len(trials),
    ]
    columns: list[dict[str, str]] = []
    for t in trials:
        counted = [r for r in results(t) if not r.void]
        cells = {"Pairs counted": str(len(counted))}
        for label, s, c in group_rows(t.spec, counted):
            cells[label] = f"source {s}; copy {c}"
        reviewed = [r for r in counted if r.picks is not None]
        for key, title in RUBRIC.items():
            cells[title] = " / ".join(
                f"{sum(bool(r.picks and r.picks[key] == v) for r in reviewed)} {v}"
                for v in (*ARMS, "tie")
            )
        columns.append(cells)
    for label in columns[0]:
        lines.append(
            f"| {label} | " + " | ".join(c.get(label, "") for c in columns) + " |"
        )
    return "\n".join(lines).rstrip() + "\n"


# The commands.


def load(trial: str) -> Trial:
    from . import load_spec

    pairs = manifest.read_pairs(trial)
    first = min((p.filed_at for p in pairs if p.filed_at), default=None)
    return Trial(
        id=trial,
        spec=load_spec(trial),
        pairs=pairs,
        grades=manifest.read_grades(trial),
        reviews=manifest.read_reviews(trial),
        verdicts=manifest.read_verdicts(trial),
        usage=io.plan_usage(first) if first else {},
    )


def cmd_report(args: argparse.Namespace) -> int:
    ids: list[str] = list(dict.fromkeys(args.trial))
    try:
        trials = [load(t) for t in ids]
    except SpecError as error:
        print(f"model-trial: a trial spec: {error}", file=sys.stderr)
        return 2
    text = table(trials[0]) if len(trials) == 1 else compare(trials)
    print(text, end="")
    if args.comment:
        io.comment(args.comment, text)
    return 0


def cmd_list(args: argparse.Namespace) -> int:
    from . import load_spec

    root = manifest.root()
    found = (
        sorted(p.name for p in root.iterdir() if p.is_dir()) if root.is_dir() else []
    )
    if not found:
        print("No trial has records on this machine.")
    for trial in found:
        if not manifest.TICKET_ID.fullmatch(trial):
            continue
        pairs = manifest.read_pairs(trial)
        grades = manifest.read_grades(trial)
        reviews = manifest.read_reviews(trial)
        latest = manifest.latest_grades(grades)
        reviewed = {r.pair for r in reviews if r.ok}
        try:
            spec = load_spec(trial)
            arms = f"{spec.source.tier} against {spec.copy.tier}"
            cap = money(spec.cap_usd)
        except (SpecError, io.IgniteError) as error:
            arms, cap = f"spec unreadable ({error})", "?"
        print(
            f"{trial}  {arms}  {sum(p.state == FILED for p in pairs)} filed, "
            f"{len(latest)} graded, {len(reviewed & set(latest))} reviewed, "
            f"{money(review.spent(grades, reviews))} of {cap}"
        )
    return 0
