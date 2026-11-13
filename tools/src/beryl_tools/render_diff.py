"""`card render [names | --all]`: each card's rendered rules text against its Oracle text (plan
K13, docs/project-50k.md, Authoring tools).

A card's definition is data, and `beryl_card::render` turns it back into English. When the English
differs from the card's Oracle text in the card store, either the definition does something other
than the card says (a rules error, K13's reason to exist) or a template words it differently.

Both texts go through the same normalizations first (`NORMALIZATIONS`, in order), each a named
function that absorbs one way Oracle text varies without changing what the card does. Then each
face's paragraphs (its lines) are compared, and a paragraph that differs is shown as a word diff:
`[-removed-]` for Oracle's words the render lacks and `{+added+}` for the render's own.

`--all` renders every card written in a file and prints the match rate, then the mismatches
grouped by cause, a cause being one changed run of words (`'him' -> 'it'`), so cards that a
single template words wrongly fall in one group.

`card check` runs `check` as its last step (card_check.render): a mismatch fails it.
"""

from __future__ import annotations

import argparse
import difflib
import json
import re
import subprocess
import sys
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from . import card, card_store, paths

Json = card_store.Json


@dataclass(frozen=True)
class Face:
    """One face: its name and its rules text, one paragraph per line."""

    name: str
    text: str
    # Whether it is an instant or a sorcery, by its Oracle type line.
    spell: bool = False


@dataclass(frozen=True)
class Context:
    """What a normalization needs to know of the face being compared."""

    # The names the card refers to itself by, longest first.
    names: tuple[str, ...]
    # Whether the face is an instant or a sorcery, whose text is one spell ability.
    spell: bool = False


def context_of(card_name: str, faces: Iterable[str], spell: bool = False) -> Context:
    """The names `card_name` and its `faces` refer to themselves by: each in full and, for a
    legendary name with a comma, the part before it ("Ana" for "Ana, the Sky Sage").
    """
    names: set[str] = set()
    for name in [card_name, *faces, *card_name.split(" // ")]:
        names.add(name)
        if ", " in name:
            names.add(name.split(", ")[0])
    return Context(tuple(sorted(names, key=len, reverse=True)), spell)


def strip_reminder_text(text: str, context: Context) -> str:
    """Removes reminder text, which is in parentheses and has no game function (CR 207.2a):
    Oracle prints it for some keywords and not others, and the renderer never does. "Flash (You
    may cast this spell any time you could cast an instant.)" becomes "Flash". A line that is nothing but
    reminder text keeps its words: Oracle prints a land's intrinsic mana ability that way
    (CR 305.6), where the renderer writes the ability, so "({T}: Add {U}.)" becomes
    "{T}: Add {U}."."""
    lines = text.split("\n")
    return "\n".join(
        (
            line[1:-1]
            if re.fullmatch(r"\([^()]*\)", line)
            else re.sub(r" ?\([^()]*\)", "", line)
        )
        for line in lines
    )


SELF = "~"

# The words a card's rules text uses for the object it is printed on (CR 201.5), besides its
# name: "this creature", "this spell" and so on, by its card type.
SELF_WORDS: str = (
    r"this (?:artifact|battle|creature|enchantment|land|permanent|planeswalker|spell"
    r"|card|Equipment|Aura|Vehicle|Saga)"
)


def unify_self_reference(text: str, context: Context) -> str:
    """Writes every way the card names itself as `~`. Oracle uses the card's name on older
    cards and on legendary ones, and "this creature" (by type) on cards since 2024; the renderer
    picks one by rule. "Exile Rift Wave." (on a card named Rift Wave) and "Exile this spell." both become "Exile ~." A
    legendary card's short name counts too: "Ana" for "Ana, the Sky Sage"."""
    names = "|".join(re.escape(n) for n in context.names)
    pattern = rf"\b(?:{names})(?![\w'])|\b{SELF_WORDS}\b" if names else SELF_WORDS
    return re.sub(pattern, SELF, text, flags=re.IGNORECASE)


# Typographic variants and their plain form: curly quotes, the minus sign Oracle prints on
# loyalty costs and the en dash, and the non-breaking space.
PLAIN: dict[str, str] = {
    "“": '"',
    "”": '"',
    "‘": "'",
    "’": "'",
    "−": "-",
    "–": "-",
    " ": " ",
}


def unify_punctuation(text: str, context: Context) -> str:
    """Writes quotes, dashes and spaces one way. Oracle prints a minus sign (U+2212) on loyalty
    costs and curly quotes in some printings, where a template might use '-' and '"': "−3:"
    becomes "-3:". An em dash keeps its spaces trimmed: "Evoke — Exile" becomes "Evoke—Exile".
    """
    for fancy, plain in PLAIN.items():
        text = text.replace(fancy, plain)
    return re.sub(r" *— *", "—", text)


def unify_whitespace(text: str, context: Context) -> str:
    """Collapses runs of spaces and trims each line, and drops empty lines: Oracle has none
    within a face, and the removal of reminder text can leave them. "Flying  \\n\\n" becomes
    "Flying"."""
    lines = (re.sub(r"[ \t]+", " ", line).strip() for line in text.split("\n"))
    return "\n".join(line for line in lines if line)


# The personal pronouns Oracle uses for a named character, and the one a template uses for any
# object. "Her" is "its" before a noun ("her loyalty abilities") and "it" otherwise.
PRONOUNS: list[tuple[str, str]] = [
    (r"\b[Hh]er(?= [a-z])", "its"),
    (r"\b(?:him|her)\b", "it"),
    (r"\bhis\b", "its"),
    (r"\b(?:he|she)\b", "it"),
    (r"\b(?:He|She)\b", "It"),
]


def neuter_pronouns(text: str, context: Context) -> str:
    """Writes a character's pronouns as "it": Oracle calls a legendary planeswalker or creature
    "him" or "her", which no definition records, and the renderer says "it". "Exile Ana, then
    return him to the battlefield under his owner's control" becomes "... return it to the
    battlefield under its owner's control"."""
    for pattern, plain in PRONOUNS:
        text = re.sub(pattern, plain, text)
    return text


def sort_mana_choices(text: str, context: Context) -> str:
    """Puts the mana symbols an ability chooses among in WUBRG order: Oracle lists a land's
    colors in the order of its basic land types, where the renderer uses color order, and the
    choice is the same either way. "Add {G}, {W}, or {U}" becomes "Add {W}, {U}, or {G}".
    """

    def sort(match: re.Match[str]) -> str:
        found = re.findall(r"\{[^}]+\}", match.group(2))
        order = "WUBRGC"
        found.sort(key=lambda m: order.index(m[1]) if m[1] in order else -1)
        joiners = re.findall(r"\}([^{]*)\{", match.group(2))
        out = found[0]
        for joiner, symbol in zip(joiners, found[1:], strict=True):
            out += joiner + symbol
        return match.group(1) + out

    choices = r"(\{[^}]+\}(?:(?:, |,? or )\{[^}]+\})+)"
    return re.sub(rf"(\b[Aa]dd ){choices}", sort, text)


def unify_mana_symbols(text: str, context: Context) -> str:
    """Writes each mana symbol in upper case with its hybrid halves in WUBRG order and pulls it
    apart from the next one by a space, so the word diff compares symbols, not runs of them.
    "{t}" becomes "{T}", "{U/W}" "{W/U}" and "{2}{W}" "{2} {W}"."""

    def symbol(match: re.Match[str]) -> str:
        parts = match.group(1).upper().split("/")
        order = "WUBRGCS"
        parts.sort(key=lambda p: order.index(p) if p in order else -1)
        return "{" + "/".join(parts) + "}"

    text = re.sub(r"\{([^{}]+)\}", symbol, text)
    return re.sub(r"\}(?=\{)", "} ", text)


def lower_sentence_starts(text: str, context: Context) -> str:
    """Lower-cases the first letter of each sentence, which a template may capitalize where
    Oracle continues a sentence, or the other way round: "Draw a card." and "draw a card." are
    the same, and so are "A spell" and "a spell". A capital `~` start is already the same.
    """
    return re.sub(
        r'(^|[.:!?—"]\s*|\n)([A-Z])(?=[a-z]|(?<=A) )',
        lambda m: m.group(1) + m.group(2).lower(),
        text,
    )


# A keyword line: words, optionally with a cost or number, separated by commas, with no
# sentence punctuation. CR 702.1: "the object lists only the name of the ability as a
# “keyword”", some with a cost or number after it.
KEYWORD_PART = r"[a-z][a-z' ]*(?: [{}0-9A-Z/ ]+| [0-9]+| [a-z]+)*"
KEYWORD_LINE: re.Pattern[str] = re.compile(rf"^{KEYWORD_PART}(?:, {KEYWORD_PART})*$")


def sort_keyword_lines(text: str, context: Context) -> str:
    """Puts each keyword of a run of keyword lines on its own line, sorted. Oracle groups
    keywords on one line, "Flying, vigilance", in its own order, where the renderer writes one
    per line: "Flying, vigilance" and "vigilance\\nflying" become "flying\\nvigilance". It runs
    after `lower_sentence_starts`, so the keywords compare in lower case."""
    out: list[str] = []
    run: list[str] = []
    for line in [*text.split("\n"), ""]:
        if line and KEYWORD_LINE.match(line):
            run += line.split(", ")
            continue
        out += sorted(run)
        run = []
        out.append(line)
    return "\n".join(out[:-1])


def join_spell_paragraphs(text: str, context: Context) -> str:
    """Joins an instant's or sorcery's paragraphs, other than keyword lines, into one: they are
    one spell ability (CR 113.3a), and Oracle breaks them for reading, putting a closing "Draw
    a card." on its own line on recent cards. On a spell, "Return target spell to its owner's
    hand.\nDraw a card." becomes one paragraph."""
    if not context.spell:
        return text
    out: list[str] = []
    for line in text.split("\n"):
        joins = out and not KEYWORD_LINE.match(line) and not KEYWORD_LINE.match(out[-1])
        if joins:
            out[-1] += " " + line
        else:
            out.append(line)
    return "\n".join(out)


Normalization = Callable[[str, Context], str]

# The normalizations, in the order they run.
NORMALIZATIONS: list[Normalization] = [
    strip_reminder_text,
    unify_punctuation,
    unify_whitespace,
    unify_self_reference,
    neuter_pronouns,
    sort_mana_choices,
    unify_mana_symbols,
    lower_sentence_starts,
    join_spell_paragraphs,
    sort_keyword_lines,
]


def normalize(text: str, context: Context) -> str:
    for step in NORMALIZATIONS:
        text = step(text, context)
    return text


def words(paragraph: str) -> list[str]:
    """The paragraph's words, each punctuation mark a word of its own."""
    return re.findall(r"\{[^}]*\}|[\w'+/~-]+|[^\w\s]", paragraph)


def word_diff(oracle: str, rendered: str) -> tuple[str, list[str]]:
    """`oracle` and `rendered` as one line with `[-removed-]` and `{+added+}` runs, and each
    run as a cause: `'removed' -> 'added'`."""
    a, b = words(oracle), words(rendered)
    out: list[str] = []
    causes: list[str] = []
    for op, i1, i2, j1, j2 in difflib.SequenceMatcher(
        a=a, b=b, autojunk=False
    ).get_opcodes():
        if op == "equal":
            out += a[i1:i2]
            continue
        old, new = " ".join(a[i1:i2]), " ".join(b[j1:j2])
        if old:
            out.append(f"[-{old}-]")
        if new:
            out.append(f"{{+{new}+}}")
        causes.append(f"{old!r} -> {new!r}")
    return " ".join(out), causes


@dataclass(frozen=True)
class Diff:
    """A card's comparison: the paragraphs that differ, as lines to print, and their causes."""

    lines: list[str]
    causes: list[str]

    @property
    def matches(self) -> bool:
        return not self.lines


def compare(card_name: str, oracle: list[Face], rendered: list[Face]) -> Diff:
    """Compares each face of `oracle` with the rendered face of its name, paragraph by
    paragraph."""
    by_name = {f.name: f.text for f in rendered}
    lines: list[str] = []
    causes: list[str] = []
    for face in oracle:
        if face.name not in by_name:
            lines.append(f"{face.name}: no rendered face")
            causes.append("no rendered face")
            continue
        context = context_of(card_name, [f.name for f in oracle], face.spell)
        a = normalize(face.text, context).split("\n")
        b = normalize(by_name[face.name], context).split("\n")
        a, b = [p for p in a if p], [p for p in b if p]
        for op, i1, i2, j1, j2 in difflib.SequenceMatcher(
            a=a, b=b, autojunk=False
        ).get_opcodes():
            if op == "equal":
                continue
            olds, news = a[i1:i2], b[j1:j2]
            # Pair the differing paragraphs in order; the rest are missing or extra.
            for k in range(max(len(olds), len(news))):
                old = olds[k] if k < len(olds) else ""
                new = news[k] if k < len(news) else ""
                shown, found = word_diff(old, new)
                lines.append(f"{face.name}: {shown}")
                causes += found
    return Diff(lines, causes)


def oracle_faces(record: Json) -> list[Face]:
    """The card's faces from the card store, each with its Oracle text."""
    return [
        Face(
            str(f["name"]),
            str(f.get("oracle_text", "")),
            bool(re.search(r"\b(?:Instant|Sorcery)\b", str(f.get("type_line", "")))),
        )
        for f in cast(list[Json], record["faces"])
    ]


Rendered = list[tuple[str, list[Face]]]


def render_command(names: list[str]) -> list[str]:
    """The command that prints each named card's rendered faces as a line of JSON, or every
    card written in a file's when `names` is empty. A double-faced card may be named as the
    store names it, "Front // Back"; its line names it by its front face, as Beryl does.
    """
    return [
        "cargo",
        "run",
        "--quiet",
        "-p",
        "beryl_cards",
        "--features",
        "scenarios",
        "--bin",
        "beryl_scenario",
        "--",
        "render",
        "--json",
        *names,
    ]


def parse(result: subprocess.CompletedProcess[str]) -> Rendered | str:
    """What `render_command` printed: each card's Beryl name and rendered faces, in the order
    named; else the error it printed."""
    if result.returncode != 0:
        return (
            result.stderr or result.stdout
        ).strip() or "beryl_scenario render failed"
    out: Rendered = []
    for line in result.stdout.splitlines():
        entry = cast(Json, json.loads(line))
        faces = [
            Face(str(f["name"]), str(f["text"]))
            for f in cast(list[Json], entry["faces"])
        ]
        out.append((str(entry["name"]), faces))
    return out


def rendered(names: list[str]) -> Rendered | str:
    """Runs `render_command` on `names` and parses what it printed."""
    return parse(
        subprocess.run(
            render_command(names),
            cwd=paths.RUST,
            env=paths.run_env(),
            capture_output=True,
            text=True,
            check=False,
        )
    )


# What renders cards for `run`: `rendered`, which tests replace.
Renderer = Callable[[list[str]], Rendered | str]


def report(name: str, faces: list[Face], diff: Diff) -> str:
    """A card's rendered text, then `match` or its differing paragraphs."""
    text = "\n".join(f"{f.name}\n{f.text}" for f in faces)
    verdict = "match" if diff.matches else "mismatch:\n" + "\n".join(diff.lines)
    return f"== {name}\n{text}\n-- {verdict}"


def run(
    args: argparse.Namespace,
    sources: card.Sources = card.Sources(),
    render: Renderer = rendered,
) -> int:
    with card_store.Store(sources.store) as store:
        records: list[Json] = []
        for asked in cast(list[str], args.names):
            found = card.find(asked, store)
            if isinstance(found, str):
                print(f"card render: {found}", file=sys.stderr)
                return 1
            records.append(found)
        if not args.all and not records:
            print("card render: name a card or pass --all", file=sys.stderr)
            return 1
        texts = render([str(r["name"]) for r in records])
        if isinstance(texts, str):
            print(f"card render: {texts}", file=sys.stderr)
            return 1
        if args.all:
            records = [cast(Json, store.card(n)) for n, _ in texts]
    diffs: dict[str, Diff] = {}
    for record, (_, faces) in zip(records, texts, strict=True):
        name = str(record["name"])
        diffs[name] = compare(name, oracle_faces(record), faces)
        if not args.all:
            print(report(name, faces, diffs[name]))
    matched = sum(d.matches for d in diffs.values())
    if args.all:
        print(f"{matched} of {len(diffs)} cards match their Oracle text")
        groups: dict[str, list[str]] = {}
        for name, diff in diffs.items():
            for cause in dict.fromkeys(diff.causes):
                groups.setdefault(cause, []).append(name)
        for cause, names in sorted(groups.items(), key=lambda g: (-len(g[1]), g[0])):
            print(f"{cause}: {', '.join(names)}")
        for name, diff in diffs.items():
            if not diff.matches:
                print(f"== {name}\n" + "\n".join(diff.lines))
    return 0 if matched == len(diffs) else 1


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = commands.add_parser(
        "render",
        help="the card's rendered rules text and its diff against its Oracle text",
    )
    p.add_argument("names", nargs="*", help="the cards' names or slugs")
    p.add_argument(
        "--all",
        action="store_true",
        help="every card written in a file: the match rate and the mismatches by cause",
    )
    p.set_defaults(func=run)
