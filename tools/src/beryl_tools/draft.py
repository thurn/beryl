"""Draft a card from Phase, filling gaps with exactly matching rendered abilities."""

from __future__ import annotations

import difflib
import re
from collections.abc import Callable
from dataclasses import dataclass
from functools import cached_property
from pathlib import Path
from typing import cast

from . import (
    card,
    card_files,
    card_store,
    phase_data,
    phase_draft,
    render_diff,
    set_groups,
    store_cards,
)

Json = card_store.Json
Ability = phase_draft.Ability
Unsupported = phase_draft.Unsupported

SPELL = "Ability::spell()"
KEYWORDS = {
    "FirstStrike": "first strike",
    "DoubleStrike": "double strike",
}

# Oracle paragraph breaks also delimit abilities on permanents. On spells Phase
# may return a builder for each sentence, even within a single paragraph.
SENTENCE: re.Pattern[str] = re.compile(r"(?<=[.!?])\s+|\n+")


def clauses(text: str, context: render_diff.Context) -> tuple[str, ...]:
    normalized = render_diff.normalize(text, context)
    if context.spell:
        return tuple(part for part in SENTENCE.split(normalized) if part)
    return tuple(normalized.splitlines())


@dataclass(frozen=True)
class Clause:
    text: str
    reason: str
    node_kind: str = ""


@dataclass(frozen=True)
class Draft:
    """The expression for the card file and any Oracle text still needing work."""

    expression: str
    unresolved: tuple[Clause, ...] = ()
    source_note: str = ""

    @property
    def complete(self) -> bool:
        return bool(self.expression) and not self.unresolved


@dataclass(frozen=True)
class Candidate:
    text: str
    ability: Ability
    card_name: str = ""


@dataclass(frozen=True)
class PhaseCandidates:
    abilities: tuple[Candidate, ...] = ()
    keywords: frozenset[str] = frozenset()
    rejected: tuple[Clause, ...] = ()
    note: str = ""


def phase_candidates(
    entry: Json | None,
    oracle: render_diff.Face,
    context: render_diff.Context,
    land_types: set[str],
) -> PhaseCandidates:
    if entry is None:
        return PhaseCandidates(note="Phase has no parse for this card.")
    source_text = entry.get("oracle_text") or ""
    if not isinstance(source_text, str) or render_diff.normalize(
        source_text, context
    ) != render_diff.normalize(oracle.text, context):
        return PhaseCandidates(note="The Phase and card store Oracle texts differ.")
    known = store_cards.keywords()
    keywords: set[str] = set()
    for keyword in phase_draft.items(entry.get("keywords", [])):
        if isinstance(keyword, str):
            text = KEYWORDS.get(keyword, keyword.lower())
            if text in known:
                keywords.add(text)
            elif keyword.lower() in known:
                keywords.add(keyword.lower())
    abilities: list[Candidate] = []
    rejected = [
        Clause(
            "",
            f"card.{field}: requires manual review",
            "" if field in {"non_ability_text", "parse_warnings"} else f"card:{field}",
        )
        for field in (
            "non_ability_text",
            "parse_warnings",
            "additional_cost",
            "modal",
            "casting_restrictions",
            "casting_options",
            "strive_cost",
            "solve_condition",
        )
        if entry.get(field)
    ]
    intrinsic = {
        f"Ability::tap_for_mana(ManaSet::one(ManaColor::{color}))"
        for subtype, color in phase_draft.BASIC_LAND_COLORS.items()
        if subtype in land_types
    }
    for field, convert in (
        ("abilities", phase_draft.ability),
        ("triggers", phase_draft.triggered),
        ("replacements", phase_draft.replacement),
        ("static_abilities", None),
    ):
        for raw in phase_draft.items(entry.get(field, [])):
            node = phase_draft.Node(raw, field)
            description = node.take("description")
            try:
                if (
                    description is None
                    and field == "abilities"
                    and intrinsic
                    and phase_draft.ability(raw).builder in intrinsic
                ):
                    continue
                if not isinstance(description, str) or not description:
                    raise Unsupported(f"{field}: missing description")
                if convert is None:
                    mode = node.fields.get("mode")
                    raise Unsupported(
                        f"{field}: unsupported {mode!r}",
                        phase_draft.node_kind("static", mode),
                    )
                abilities.append(Candidate(description, convert(raw)))
            except Unsupported as error:
                rejected.append(
                    Clause(str(description or ""), str(error), error.node_kind)
                )
    return PhaseCandidates(tuple(abilities), frozenset(keywords), tuple(rejected))


# A portable fallback comes from a prelude-only ability on a plain Printed face.
# A local constant, another card, or another face would need context from its
# source file. Keep those examples available to the writer without copying them.
PRINTED: re.Pattern[str] = re.compile(
    r"pub static CARD:\s*CardDef\s*=\s*Printed::\w+\s*\.abilities\(&\[(.*)\]\)\s*\.card\(\);",
    re.S,
)
TOKEN: re.Pattern[str] = re.compile(r'"(?:[^"\\]|\\.)*"|\w+(?:::\w+)*|[^\s]')
SPELL_ABILITY: re.Pattern[str] = re.compile(
    r"Ability::spell\(\)(?:\s*\.target\(&(.+)\))?\s*\.effect\(&(.+)\)", re.S
)
LOCAL: re.Pattern[str] = re.compile(r"\b(?:[A-Z]\w*|[a-z_]+)\b")


def portable_ability(source: str, spell: bool) -> Ability | None:
    prefix, marker, expression = source.partition("pub static CARD:")
    if prefix.strip() != "use crate::prelude::*;" or not marker:
        return None
    found = PRINTED.fullmatch((marker + expression).strip())
    if found is None:
        return None
    expression = found[1].strip().removesuffix(",").strip()
    depth = 0
    for token in TOKEN.findall(expression):
        if token in ("Card", "Printed", "Token", "Prop") or token.startswith(
            ("Card::", "Printed::", "Token::", "Prop::", "Self::", "crate::", "super::")
        ):
            return None
        if LOCAL.fullmatch(token) and (
            token[0].isupper() or token in ("self", "super", "crate")
        ):
            return None
        if token in ("(", "[", "{"):
            depth += 1
        elif token in (")", "]", "}"):
            depth -= 1
        elif token == "," and depth == 0:
            return None
    if depth:
        return None
    if spell:
        matched = SPELL_ABILITY.fullmatch(expression)
        if matched is not None:
            return Ability(SPELL, (matched[2].strip(),), (matched[1] or "").strip())
        return None
    return Ability(expression)


def rendered_candidates(
    files: dict[str, Path], store: Path, renderer: render_diff.Renderer
) -> tuple[Candidate, ...]:
    portable: dict[str, tuple[render_diff.Face, Ability]] = {}
    with card_store.Store(store) as opened:
        for name, record in zip(files, opened.cards(files), strict=True):
            faces = render_diff.oracle_faces(record)
            if len(faces) != 1:
                continue
            if ability := portable_ability(
                card.definition(files[name]), faces[0].spell
            ):
                portable[name] = (faces[0], ability)
    if not portable:
        return ()
    result = renderer(list(portable))
    if isinstance(result, str):
        return ()
    candidates: list[Candidate] = []
    for name, faces in result:
        if name in portable and len(faces) == 1:
            oracle, ability = portable[name]
            if render_diff.compare(name, [oracle], faces).matches:
                candidates.append(Candidate(faces[0].text, ability, name))
    return tuple(candidates)


def definition(record: Json, abilities: list[Ability], keywords: list[str]) -> str:
    face = render_diff.oracle_faces(record)[0]
    expression = f"Printed::{set_groups.camel(face.name)}"
    if keywords:
        flags = [f"Keywords::{keyword}" for keyword in keywords]
        expression += f".keywords({flags[0]}"
        expression += "".join(f".union({flag})" for flag in flags[1:]) + ")"
    if abilities:
        lines = "\n".join(f"    {ability.expression()}," for ability in abilities)
        expression += f".abilities(&[\n{lines}\n])"
    return expression + ".card()"


def without_ability_word(sentence: str) -> str:
    """Phase can omit the ability word when describing a trigger or an effect."""
    prefix, dash, body = sentence.partition("—")
    return body if dash and all(word.isalpha() for word in prefix.split()) else sentence


def compose(
    record: Json,
    entry: Json | None,
    fallback: Callable[[], tuple[Candidate, ...]],
    source_note: str = "",
) -> Draft:
    faces = render_diff.oracle_faces(record)
    if record.get("layout") != "normal" or len(faces) != 1:
        return Draft(
            "",
            (Clause(f"Layout {record.get('layout')}", "requires a manual definition"),),
        )
    oracle = faces[0]
    name = str(record["name"])
    context = render_diff.context_of(name, [oracle.name], oracle.spell)
    face = cast(list[Json], record["faces"])[0]
    types, _, subtypes = str(face.get("type_line", "")).partition(" — ")
    land_types = set(subtypes.split()) if "Land" in types.split() else set()
    try:
        phase = phase_candidates(entry, oracle, context, land_types)
    except Unsupported as error:
        phase = PhaseCandidates(note=str(error))
    text = clauses(oracle.text, context)
    known = store_cards.keywords()
    keywords: list[str] = []
    covered: list[Ability] = []
    unresolved: list[Clause] = []
    index = 0
    while index < len(text):
        sentence = text[index]
        if sentence in phase.keywords:
            keywords.append(known[sentence])
            index += 1
            continue
        match = match_clause(text[index:], phase.abilities, context, name)
        if match is None:
            match = match_clause(text[index:], fallback(), context, name)
        if match is None:
            plain = without_ability_word(sentence)
            rejection = next(
                (
                    item
                    for item in phase.rejected
                    if sentence in clauses(item.text, context)
                    or plain in clauses(item.text, context)
                ),
                None,
            )
            unresolved.append(
                Clause(sentence, rejection.reason, rejection.node_kind)
                if rejection
                else Clause(sentence, "No matching translated or rendered ability")
            )
            index += 1
        else:
            size, ability = match
            covered.append(ability)
            index += size
    for rejection in phase.rejected:
        if not rejection.text:
            unresolved.append(
                Clause("Unmapped Phase data", rejection.reason, rejection.node_kind)
            )
    if oracle.spell:
        spell_abilities = [ability for ability in covered if ability.builder == SPELL]
        if (
            len(spell_abilities) != len(covered)
            or sum(bool(a.target) for a in covered) > 1
        ):
            unresolved.append(
                Clause(
                    oracle.text, "Spell clauses require manual ability or target wiring"
                )
            )
            covered = []
        elif spell_abilities:
            effects = tuple(
                effect for ability in spell_abilities for effect in ability.effects
            )
            target = next((a.target for a in spell_abilities if a.target), "")
            covered = [Ability(SPELL, effects, target)]
    code = (
        definition(record, covered, keywords) if covered or keywords or not text else ""
    )
    return Draft(code, tuple(unresolved), source_note or phase.note)


def match_clause(
    remaining: tuple[str, ...],
    candidates: tuple[Candidate, ...],
    context: render_diff.Context,
    name: str,
) -> tuple[int, Ability] | None:
    for candidate in candidates:
        if candidate.card_name == name:
            continue
        source_context = (
            render_diff.context_of(
                candidate.card_name, [candidate.card_name], context.spell
            )
            if candidate.card_name
            else context
        )
        parts = clauses(candidate.text, source_context)
        if parts and remaining[: len(parts)] == parts:
            if (candidate.ability.builder == SPELL) == context.spell:
                return len(parts), candidate.ability
    return None


class Drafter:
    """Share the pinned input and rendered fallback across a set of card drafts."""

    def __init__(
        self, sources: card.Sources, renderer: render_diff.Renderer | None = None
    ) -> None:
        self.sources = sources
        self.renderer: render_diff.Renderer = renderer or render_diff.rendered

    @cached_property
    def phase(self) -> dict[str, Json] | str:
        return phase_data.read()

    @cached_property
    def files(self) -> dict[str, Path]:
        return card_files.implemented(self.sources.cards, self.sources.store)

    @cached_property
    def fallback(self) -> tuple[Candidate, ...]:
        return rendered_candidates(self.files, self.sources.store, self.renderer)

    def draft(self, record: Json, *, phase_gap: bool = False) -> Draft:
        if phase_gap:
            return compose(
                record, None, lambda: self.fallback, "Phase reports a parse gap."
            )
        phase = self.phase
        name = str(record["name"])
        if isinstance(phase, str):
            return compose(record, None, lambda: self.fallback, phase)
        return compose(record, phase.get(name.lower()), lambda: self.fallback)


def render_status(record: Json, result: render_diff.Rendered | str) -> str:
    """Show the status of each sentence in the definition currently on disk."""
    if isinstance(result, str):
        return f"Could not render the card file.\n{result}"
    if len(result) != 1:
        return "The renderer did not return a single card."
    name = str(record["name"])
    oracle = render_diff.oracle_faces(record)
    by_name = {face.name: face.text for face in result[0][1]}
    lines = ["Rendered card file"]
    for face in oracle:
        if face.name not in by_name:
            lines.append(f"- {face.name}: no rendered face")
            continue
        context = render_diff.context_of(name, [f.name for f in oracle], face.spell)
        expected = (
            SENTENCE.split(render_diff.normalize(face.text, context))
            if face.text
            else []
        )
        raw = by_name[face.name]
        actual = SENTENCE.split(render_diff.normalize(raw, context)) if raw else []
        for op, i, j, start, stop in difflib.SequenceMatcher(
            a=expected, b=actual, autojunk=False
        ).get_opcodes():
            if op == "equal":
                lines.extend(f"- Match · {sentence}" for sentence in expected[i:j])
                continue
            for offset in range(max(j - i, stop - start)):
                old = expected[i + offset] if i + offset < j else ""
                new = actual[start + offset] if start + offset < stop else ""
                shown, _ = render_diff.word_diff(old, new)
                lines.append(f"- Diff · {shown}")
    if len(lines) == 1:
        lines.append("- Match · no rules text")
    return "\n".join(lines)
