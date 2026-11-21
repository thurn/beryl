"""List a set's first printings and group its unwritten cards by what they need."""

from __future__ import annotations

import argparse
import json
import shlex
import sys
import tempfile
from collections.abc import Iterable, Iterator
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Literal, cast

from . import (
    card,
    card_store,
    catalogs,
    census,
    draft,
    nearest,
    paths,
    phase_data,
    render_diff,
    store_cards,
    system,
    vocabulary,
)

Json = card_store.Json
TIERS: tuple[str, ...] = ("store", "drafted", "composed", "vocabulary", "mechanism")
# The multi-face shapes documented by beryl_card::definition::card, plus normal cards.
LAYOUTS: frozenset[str] = frozenset(
    ("normal", "transform", "modal_dfc", "split", "adventure")
)
# The vocabulary names these keyword abilities by the builders that express them.
KEYWORD_BUILDERS = {
    "enchant": "Ability::spell",
    "protection": "Modification::AddProtection",
}
# The census names the event behind this Scryfall keyword action.
CENSUS_ALIASES = {"trigger:BecomeMonstrous": "Monstrosity"}
# Lower quartile of the best matches between clauses on different implemented cards
# (251 clauses at calibration). Match words and word pairs, with inflections folded.
NEAR = 1 / 6
BATCH_SIZE = 20


@dataclass(frozen=True)
class Group:
    kind: Literal["phase", "sentence"]
    label: str


@dataclass
class Plan:
    implemented: list[str] = field(default_factory=list)
    tiers: dict[str, list[str]] = field(
        default_factory=lambda: {tier: [] for tier in TIERS}
    )
    vocabulary: dict[Group, dict[str, list[str]]] = field(default_factory=dict)
    mechanisms: dict[str, list[str]] = field(default_factory=dict)

    def groups(self) -> list[tuple[Group, dict[str, list[str]]]]:
        return sorted(
            self.vocabulary.items(),
            key=lambda pair: (-len(pair[1]), pair[0].label, pair[0].kind),
        )

    def packet(self, code: str, audit: dict[str, int]) -> Json:
        return {
            "set": code,
            "implemented": self.implemented,
            "tiers": self.tiers,
            "vocabulary": [
                {
                    **asdict(group),
                    "sentence": next(iter(sentences.values()))[0],
                    "cards": list(sentences),
                    "sentences_by_card": sentences,
                }
                for group, sentences in self.groups()
            ],
            "mechanisms": self.mechanisms,
            "draft_audit": audit,
        }


def first_printings(code: str, store: Path) -> list[Json]:
    with card_store.Store(store) as opened:
        return [
            record
            for kind, record in opened
            if kind == "card" and record.get("first_set") == code
        ]


def read_object(path: Path) -> Json:
    try:
        return cast(Json, json.loads(path.read_text(encoding="utf-8")))
    except FileNotFoundError:
        return {}


def rules_clauses(record: Json) -> Iterator[str]:
    faces = render_diff.oracle_faces(record)
    names = [face.name for face in faces]
    for face in faces:
        context = render_diff.context_of(str(record["name"]), names, face.spell)
        yield from draft.clauses(face.text, context)


def keyword_key(name: str) -> str:
    return "".join(name.casefold().split()).replace("_", "")


def missing_keywords(cache: Path, sources: card.Sources) -> set[str]:
    # Use the census's aggregate kind names. The planner reads no Forge card script.
    entries = {entry.name for entry in vocabulary.entries(sources.vocabulary)}
    known = {keyword_key(word) for word in store_cards.keywords()}
    known.update(keyword_key(entry.rsplit("::", 1)[-1]) for entry in entries)
    known.update(key for key, entry in KEYWORD_BUILDERS.items() if entry in entries)
    known.update(
        keyword_key(kind.partition(":")[2])
        for kind, entry in census.coverage().items()
        if entry in entries
    )
    catalog = cast(dict[str, list[str]], read_object(catalogs.CATALOGS)["catalogs"])
    missing = {keyword_key(name) for name in catalog["keyword-abilities"]}
    report = read_object(cache / "census.json")
    for row in cast(list[Json], report.get("uncovered", [])):
        kind = str(row["kind"])
        category, _, name = kind.partition(":")
        if kind in CENSUS_ALIASES:
            missing.add(keyword_key(CENSUS_ALIASES[kind]))
        elif category in ("keyword", "effect"):
            missing.add(keyword_key(name))
    return missing - known


def blockers(record: Json, escalation: Json, missing: set[str]) -> list[str]:
    reasons: list[str] = []
    if mechanic := escalation.get("mechanic"):
        rule = escalation.get("rule")
        reasons.append(f"{mechanic} ({rule})" if rule else str(mechanic))
    layout = str(record.get("layout"))
    if layout not in LAYOUTS:
        reasons.append(f"Layout {layout}")
    for keyword in cast(list[str], record.get("keywords", [])):
        if keyword_key(keyword) in missing:
            reasons.append(f"Keyword or action {keyword}")
    return reasons


def vocabulary_groups(
    unresolved: Iterable[draft.Clause], examples: list[set[str]]
) -> dict[Group, list[str]]:
    groups: dict[Group, list[str]] = {}
    clauses = tuple(unresolved)
    for clause in clauses:
        if clause.node_kind:
            groups.setdefault(Group("phase", clause.node_kind), []).append(clause.text)
    if groups:
        return groups
    for clause in clauses:
        if clause.text == "Unmapped Phase data":
            continue
        words = nearest.words(clause.text, stemmed=True)
        if words and not any(
            nearest.overlap(words, sample) >= NEAR for sample in examples
        ):
            groups[Group("sentence", clause.text)] = [clause.text]
    return groups


def classify(
    records: list[Json],
    drafter: draft.Drafter,
    escalations: dict[str, Json],
    missing: set[str],
    phase_gaps: set[str],
) -> Plan:
    result = Plan()
    with card_store.Store(drafter.sources.store) as store:
        examples = [
            nearest.words(sentence, stemmed=True)
            for implemented in store.cards(drafter.files)
            for sentence in rules_clauses(implemented)
        ]
    for record in records:
        name = str(record["name"])
        if name in drafter.files:
            result.implemented.append(name)
            continue
        reasons = blockers(record, escalations.get(name, {}), missing)
        if reasons:
            result.tiers["mechanism"].append(name)
            result.mechanisms[name] = reasons
            continue
        if store_cards.complete(record) is not None:
            result.tiers["store"].append(name)
            continue
        proposal = drafter.draft(record, phase_gap=name.casefold() in phase_gaps)
        if proposal.complete:
            result.tiers["drafted"].append(name)
            continue
        unresolved = proposal.unresolved
        if record.get("layout") != "normal":
            unresolved = tuple(draft.Clause(text, "") for text in rules_clauses(record))
        groups = vocabulary_groups(unresolved, examples)
        if groups:
            result.tiers["vocabulary"].append(name)
            for group, sentences in groups.items():
                result.vocabulary.setdefault(group, {})[name] = sentences
        else:
            result.tiers["composed"].append(name)
    return result


def draft_audit(records: list[Json], cache: Path) -> dict[str, int]:
    names = {str(record["name"]) for record in records}
    audit = read_object(cache / "phase" / "beryl-draft-results.json")
    checks: dict[str, Json] = {}
    for cohort in cast(dict[str, Json], audit.get("cohorts", {})).values():
        checks.update(cast(dict[str, Json], cohort.get("cards", {})))
    results = [checks[name] for name in names & checks.keys()]
    return {
        "checked": len(results),
        **{
            key: sum(row.get(key) is True for row in results)
            for key in ("drafted", "compiled", "exact")
        },
    }


def bead_lines(code: str, plan: Plan) -> Iterator[str]:
    def create(tier: str, title: str, description: str) -> str:
        return shlex.join(
            [
                "tk",
                "create",
                "--title",
                title,
                "--description",
                description,
                "--labels",
                tier,
            ]
        )

    for group, cards in plan.groups():
        examples = " ".join(
            f"{name} ({' / '.join(sentences)})" for name, sentences in cards.items()
        )
        yield create(
            "tier:opus",
            f"{code}: cover {group.label}",
            "Review the missing clause against Beryl's vocabulary and the Comprehensive Rules. "
            f"Some cards also have other unresolved clauses. {examples}",
        )
    for tier, label in (("drafted", "tier:light"), ("composed", "tier:sonnet")):
        names = plan.tiers[tier]
        for index in range(0, len(names), BATCH_SIZE):
            cards = ", ".join(names[index : index + BATCH_SIZE])
            yield create(
                label,
                f"{code}: write {tier} cards {index // BATCH_SIZE + 1}",
                f"Follow the card-writing skill for each card in this batch. {cards}",
            )


def run_list(args: argparse.Namespace, sources: card.Sources = card.Sources()) -> int:
    records = first_printings(args.code.lower(), sources.store)
    if not records:
        print(f"No first printings for set {args.code!r}", file=sys.stderr)
        return 1
    files = draft.Drafter(sources).files
    escalations = read_object(sources.escalations)
    for record in records:
        name = str(record["name"])
        status: list[str] = []
        if name in files:
            status.append("implemented")
        if store_cards.complete(record) is not None:
            status.append("store")
        if name in escalations:
            status.append("escalated")
        print(f"{name}\t{', '.join(status) or 'unwritten'}")
    return 0


def planning_render(names: list[str]) -> render_diff.Rendered | str:
    """Use a local renderer when available so planning does not start a Rust build."""
    binary = system.exe(paths.RUN_TARGET / "debug" / "beryl_scenario")
    result = (
        render_diff.rendered(names, binary)
        if binary.is_file()
        else "No local renderer. Run `just card render --all` to enable rendered examples."
    )
    if isinstance(result, str):
        print(f"Rendered draft examples unavailable: {result}", file=sys.stderr)
    return result


def run_plan(args: argparse.Namespace, sources: card.Sources = card.Sources()) -> int:
    code = args.code.lower()
    records = first_printings(code, sources.store)
    if not records:
        print(f"No first printings for set {args.code!r}", file=sys.stderr)
        return 1
    cache = paths.cache_root()
    drafter = draft.Drafter(sources, planning_render)
    if isinstance(drafter.phase, str):
        print(drafter.phase, file=sys.stderr)
        gaps: set[str] = set()
    else:
        coverage = phase_data.read_gaps()
        if isinstance(coverage, str):
            print(coverage, file=sys.stderr)
        gaps = set() if isinstance(coverage, str) else coverage
    if not (cache / "census.json").is_file():
        print(
            "Run `just census` to include its keyword and action gaps.", file=sys.stderr
        )
    escalations = cast(dict[str, Json], read_object(sources.escalations))
    plan = classify(
        records, drafter, escalations, missing_keywords(cache, sources), gaps
    )
    audit = draft_audit(records, cache)
    destination = cache / f"set-plan-{code}.json"
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=cache, delete=False
    ) as output:
        temporary = Path(output.name)
        try:
            json.dump(plan.packet(code, audit), output, ensure_ascii=False, indent=2)
            output.write("\n")
            output.close()
            temporary.replace(destination)
        finally:
            temporary.unlink(missing_ok=True)

    summary = sys.stderr if args.beads else sys.stdout
    print(
        f"{code} · {len(records)} first printings\n\nTier          Cards", file=summary
    )
    print(f"{'implemented':14}{len(plan.implemented):5}", file=summary)
    for tier in TIERS:
        print(f"{tier:14}{len(plan.tiers[tier]):5}", file=summary)
    print(
        f"\nPrior draft audit in this set · {audit['checked']} checked, "
        f"{audit['drafted']} complete, {audit['compiled']} compiled, {audit['exact']} rendered exactly",
        file=summary,
    )
    if plan.vocabulary:
        print("\nLargest vocabulary groups", file=summary)
        for group, cards in plan.groups()[:5]:
            print(f"{len(cards):5}  {group.label}", file=summary)
    print(f"\nWrote {destination}", file=summary)
    if args.beads:
        for line in bead_lines(code, plan):
            print(line)
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    commands = sub.add_parser(
        "set", help="list or plan a set's first printings"
    ).add_subparsers(dest="set_command", required=True)
    listing = commands.add_parser(
        "list", help="list each card and its implementation status"
    )
    listing.add_argument("code")
    listing.set_defaults(func=run_list)
    planner = commands.add_parser("plan", help="sort unwritten cards into tiers")
    planner.add_argument("code")
    planner.add_argument(
        "--tickets",
        dest="beads",
        action="store_true",
        help="print draft ticket commands",
    )
    planner.add_argument(
        "--beads", dest="beads", action="store_true", help=argparse.SUPPRESS
    )
    planner.set_defaults(func=run_plan)
