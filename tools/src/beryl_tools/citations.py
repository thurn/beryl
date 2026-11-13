"""The citations check: every Comprehensive Rules citation in the tree names a rule that exists in
the release the tree cites, and the text quoted after it is that rule's text.

AGENTS.md has rules code cite the rules with yawgatog links and quotations. Each release
renumbers and rewords rules, which turns a citation into a dead anchor, a quotation the rule no
longer contains, or a bare rule number that silently names another rule. So the tree cites one
release, pinned in `data/rules-version.json`, and this check holds each citation in a file under
`rust/crates`, `rust/cards`, `ui/src`, `tools/src`, `data` and `docs` to that release's text:

- a link's anchor is the rule number without its dots, with an `R` before it. A citation
  may be a link, `[CR 118.12a](https://yawgatog.com/...#R11812a)`, or a reference,
  `[CR <number>]`, whose link a definition in the same file gives
  (`[CR 118.12a]: https://yawgatog.com/...#R11812a`), as Markdown and rustdoc read it;
- the rule exists in the pinned release (for a bare "CR n" too);
- a quotation that directly follows the citation (`: "..."` or ` "..."`, after an optional
  closing parenthesis, joined across comment lines and split at `...` or `. . .`) is text of that
  rule, compared with whitespace collapsed, curly quotes straightened and an inner single quote
  matching the rule's double quote.

The release's text is Wizards' dated `.txt` file, which Wizards keeps hosted after later
releases. It is fetched once per machine into `$BERYL_CACHE/rules` and never changes, so only
the first run needs the network. `just citations migrate <date>` moves the tree to a new
release (`citation_migrate`).

The `names` lane runs it as the `citations` step (`step`), which never fails for want of the
network: slow CI holds the whole tree, fetching the text if it is not cached; the fast check
holds the files changed on the branch and never fetches. With no text to hold them to, the step
logs a skip and passes.
"""

from __future__ import annotations

import argparse
import http.client
import json
import re
import sys
import unicodedata
import urllib.request
from dataclasses import dataclass
from pathlib import Path

from . import paths, runlog

VERSION_FILE = "data/rules-version.json"
ROOTS = ("rust/crates", "rust/cards", "ui/src", "tools/src", "data", "docs")
EXTENSIONS = {".rs", ".ts", ".tsx", ".py", ".md", ".json"}
# Directories under the roots that hold no source of ours.
SKIPPED_DIRECTORIES = {"node_modules", "target", "generated"}
# Files under the roots that cite no rules.
SKIPPED_FILES: set[str] = {VERSION_FILE}
# How many comment lines a quotation may run over.
QUOTATION_LINES = 20
# How long the `citations` step waits for the release's text before it skips.
STEP_FETCH_TIMEOUT = 10

NUMBER = r"\d{1,3}(?:\.\d+[a-z]*)?"
RULE_LINE: re.Pattern[str] = re.compile(rf"^({NUMBER})\.?\s+(\S.*)$")
LINK = r"https://yawgatog\.com/resources/magic-rules/#(\w+)"
# A linked citation (groups 1 and 2: number and anchor), a reference link's definition (groups
# 3 and 4), a reference to one (group 5: number) or a bare citation (group 6: number). A bare
# number needs three digits, so "CR 7" is no citation.
CITATION: re.Pattern[str] = re.compile(
    rf"\[CR ({NUMBER})\]\({LINK}\)"
    rf"|\[CR ({NUMBER})\]: {LINK}"
    rf"|\[CR ({NUMBER})\](?!\(|: https://)"
    r"|(?<!\[)\bCR (\d{3}(?:\.\d+[a-z]*)?)\b"
)
# A Rust doc line that starts with `[CR x]:` but does not define a link. Markdown reads the line
# as a link reference definition, so rustdoc drops the rest of the line from the rendered docs.
DOC_LINE_DEFINITION: re.Pattern[str] = re.compile(
    rf"^\s*(?:///|//!) ?\[CR ({NUMBER})\]:(?!\s*https?://)"
)
COMMENT_PREFIX: re.Pattern[str] = re.compile(r"^\s*(?:///|//!|//|#|/\*\*?|\*(?!\w))\s?")
QUOTATION_START: re.Pattern[str] = re.compile(r'^\)?:?\s*"')
# A bare number is often itself in quotes (a test's message that is only the citation), so its quotation
# needs a colon or a space before it.
BARE_QUOTATION_START: re.Pattern[str] = re.compile(r'^\)?(?::\s*|\s+)"')
ELLIPSIS: re.Pattern[str] = re.compile(r"\.\.\.|\. \. \.|…")
# Wizards' text against the ASCII a comment (and yawgatog's page) spells it in.
TRANSLATED: dict[int, str] = str.maketrans(
    {
        "“": '"',
        "”": '"',
        "‘": "'",
        "’": "'",
        "–": "-",
        "−": "-",
        "—": "--",
        "™": " (tm)",
        "®": " (r)",
    }
)


@dataclass(frozen=True)
class Version:
    """A rules release: the date it takes effect and the URL of its text."""

    effective: str
    url: str

    @staticmethod
    def for_date(effective: str) -> Version:
        """The release effective on `effective` (YYYY-MM-DD), at Wizards' usual URL."""
        compact = effective.replace("-", "")
        return Version(
            effective,
            f"https://media.wizards.com/{effective[:4]}/downloads/"
            f"MagicCompRules%20{compact}.txt",
        )


@dataclass(frozen=True)
class Citation:
    """One citation in a file: where its number (and a link's anchor) sit on its line, and the
    quotation after it."""

    line: int
    number: str
    number_span: tuple[int, int]
    anchor: str | None
    anchor_span: tuple[int, int] | None
    quotation: str | None
    # "link", "definition" (of a reference link), "reference" or "bare".
    kind: str = "link"


def anchor_of(number: str) -> str:
    """The yawgatog anchor and rules key of rule `number`: `712.6a` is `R7126a`."""
    return "R" + number.replace(".", "")


def normalize(text: str) -> str:
    """`text` in ASCII punctuation, without accents, with whitespace collapsed."""
    decomposed = unicodedata.normalize("NFKD", text.translate(TRANSLATED))
    stripped = "".join(c for c in decomposed if not unicodedata.combining(c))
    return " ".join(stripped.split())


def parse_rules(text: str) -> dict[str, str]:
    """Each rule's anchor (`R7126a`) to its normalized text (with its examples and further
    paragraphs, without its number, and with the rule's double quotes as single ones, as a
    comment quotes them), from a release's `.txt`."""
    lines = text.lstrip("﻿").splitlines()
    # The glossary's numbered senses would read as rules; it opens at its last heading (the
    # first is the contents').
    headings = [i for i, line in enumerate(lines) if line.strip() == "Glossary"]
    end = headings[-1] if headings else len(lines)
    raw: dict[str, list[str]] = {}
    current: list[str] | None = None
    for line in lines[:end]:
        match = RULE_LINE.match(line)
        if match:
            # The contents name each section once before its rules: the later line wins.
            current = [match.group(2)]
            raw[anchor_of(match.group(1))] = current
        elif current is not None and line.strip():
            current.append(line)
    return {
        anchor: normalize(" ".join(parts)).replace('"', "'")
        for anchor, parts in raw.items()
    }


def pinned(root: Path) -> Version:
    """The release the tree at `root` cites."""
    data = json.loads((root / VERSION_FILE).read_text(encoding="utf-8"))
    return Version(str(data["effective"]), str(data["url"]))


def pin(root: Path, version: Version) -> None:
    """Make `version` the release the tree at `root` cites."""
    text = json.dumps({"effective": version.effective, "url": version.url}, indent=2)
    (root / VERSION_FILE).write_text(text + "\n", encoding="utf-8")


def cached_text(version: Version) -> Path:
    """Where the text of `version` is cached once it has been fetched."""
    return paths.cache_root() / "rules" / f"{version.effective}.txt"


def rules_text(version: Version, timeout: float = 60) -> str:
    """The text of `version`, from the cache once it has been fetched."""
    cached = cached_text(version)
    if not cached.is_file():
        request = urllib.request.Request(
            version.url, headers={"User-Agent": "beryl-tools"}
        )
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body: bytes = response.read()
        cached.parent.mkdir(parents=True, exist_ok=True)
        cached.write_bytes(body)
    return cached.read_text(encoding="utf-8")


def load_rules(version: Version) -> dict[str, str]:
    """The rules of `version`, or exit when its text holds none."""
    rules = parse_rules(rules_text(version))
    if not rules:
        sys.exit(f"citations: no rules found in {version.url}")
    return rules


def step_rules(version: Version, fetch: bool) -> tuple[dict[str, str] | None, str]:
    """The rules of `version` for the `citations` step, or None and why they are missing: the
    cached text, else, with `fetch`, the text fetched with a short timeout. A failed fetch is
    no failure of the step; a cached text with no rules in it is (`load_rules`)."""
    if not cached_text(version).is_file():
        if not fetch:
            return None, f"the {version.effective} rules are not cached"
        try:
            rules_text(version, timeout=STEP_FETCH_TIMEOUT)
        except (OSError, http.client.HTTPException) as e:
            return None, f"the {version.effective} rules could not be fetched: {e}"
    return load_rules(version), ""


def comment_text(line: str) -> str | None:
    """`line` without its comment marker, or None when it is no comment line."""
    match = COMMENT_PREFIX.match(line)
    return line[match.end() :] if match else None


def following_text(lines: list[str], index: int, end: int, commented: bool) -> str:
    """The text after column `end` of `lines[index]`, continued over the following lines of
    the same comment (or paragraph), joined with spaces."""
    parts = [lines[index][end:]]
    for line in lines[index + 1 : index + 1 + QUOTATION_LINES]:
        text = comment_text(line) if commented else line
        if text is None or not text.strip():
            break
        parts.append(text)
    return normalize(" ".join(parts))


def quotation(text: str, linked: bool = True) -> str | None:
    """The quotation that opens `text` (what follows a citation), if one does."""
    start = QUOTATION_START if linked else BARE_QUOTATION_START
    if not start.match(text):
        return None
    body = text[text.index('"') + 1 :]
    close = body.find('"')
    return body if close < 0 else body[:close]


def segments(quoted: str) -> list[str]:
    """The pieces of a quotation between its ellipses."""
    return [s.strip() for s in ELLIPSIS.split(quoted) if s.strip()]


def citations_in(lines: list[str]) -> list[Citation]:
    """Every citation in one file's `lines`."""
    found = []
    for index, line in enumerate(lines):
        commented = comment_text(line) is not None
        for match in CITATION.finditer(line):
            kind, group = next(
                (kind, group)
                for kind, group in (
                    ("link", 1),
                    ("definition", 3),
                    ("reference", 5),
                    ("bare", 6),
                )
                if match.group(group) is not None
            )
            anchored = kind in ("link", "definition")
            text = following_text(lines, index, match.end(), commented)
            found.append(
                Citation(
                    line=index,
                    number=match.group(group),
                    number_span=match.span(group),
                    anchor=match.group(group + 1) if anchored else None,
                    anchor_span=match.span(group + 1) if anchored else None,
                    quotation=(
                        None
                        if kind == "definition"
                        else quotation(text, kind != "bare")
                    ),
                    kind=kind,
                )
            )
    return found


def problems_in(rel: str, lines: list[str], rules: dict[str, str]) -> list[str]:
    """Each `file:line: CR n: problem` for the citations in one file's `lines`."""
    found = []
    for index, line in enumerate(lines):
        match = DOC_LINE_DEFINITION.match(line)
        if match:
            found.append(
                f"{rel}:{index + 1}: CR {match.group(1)}: a doc line that starts with "
                f"`[CR {match.group(1)}]:` is read as a link definition, so rustdoc drops "
                "the rest of the line; write the citation as an inline link there"
            )
    cited = citations_in(lines)
    defined = {c.number for c in cited if c.kind == "definition"}
    for citation in cited:
        number = citation.number
        where = f"{rel}:{citation.line + 1}: CR {number}"
        expected = anchor_of(number)
        if citation.kind == "reference" and number not in defined:
            found.append(
                f"{where}: no link definition `[CR {number}]: "
                f"https://yawgatog.com/resources/magic-rules/#{expected}` in the file"
            )
            continue
        if citation.anchor is not None and citation.anchor != expected:
            found.append(f"{where}: the anchor is #{citation.anchor}, not #{expected}")
            continue
        if expected not in rules:
            found.append(f"{where}: no such rule")
            continue
        if citation.quotation is None:
            continue
        for segment in segments(citation.quotation):
            if segment not in rules[expected]:
                found.append(f'{where}: the rule does not say "{segment}"')
    return found


def cites(rel: str) -> bool:
    """Whether the file at `rel` (relative to the root) is one that may cite the rules."""
    parts = rel.split("/")
    return (
        rel.startswith(tuple(f"{r}/" for r in ROOTS))
        and Path(rel).suffix in EXTENSIONS
        and not SKIPPED_DIRECTORIES.intersection(parts[:-1])
        and rel not in SKIPPED_FILES
    )


def source_files(root: Path) -> list[tuple[str, Path]]:
    """Each file under `root`'s roots that may cite the rules, with its relative path."""
    found = []
    for directory in ROOTS:
        base = root / directory
        files = sorted(base.rglob("*")) if base.is_dir() else []
        for path in files:
            rel = path.relative_to(root).as_posix()
            if cites(rel) and path.is_file():
                found.append((rel, path))
    return found


def check(
    root: Path, rules: dict[str, str], only: list[str] | None = None
) -> list[str]:
    """Each problem with a citation in the files under `root`'s roots, or in those of them at
    the relative paths `only`."""
    files = (
        source_files(root)
        if only is None
        else [
            (rel, root / rel) for rel in only if cites(rel) and (root / rel).is_file()
        ]
    )
    found = []
    for rel, path in files:
        lines = path.read_text(encoding="utf-8").splitlines()
        found.extend(problems_in(rel, lines, rules))
    return found


def run(args: argparse.Namespace) -> int:
    version = pinned(paths.REPO)
    rules = load_rules(version)
    found = check(paths.REPO, rules)
    for line in found:
        print(line)
    print(
        f"citations: {len(found)} problems against the {version.effective} rules",
        file=sys.stderr,
    )
    return 1 if found else 0


def step(*args: str) -> int:
    """The `citations` step: `--fetch` to fetch the release's text when it is not cached, then
    the files to hold (relative to the repository), or none for every file."""
    fetch = bool(args) and args[0] == "--fetch"
    only = list(args[1:] if fetch else args) or None
    version = pinned(paths.REPO)
    rules, missing = step_rules(version, fetch)
    if rules is None:
        print(f"citations: skipped: {missing}")
        runlog.result("citations", "skip", 0.0)
        return 0
    found = check(paths.REPO, rules, only)
    for line in found:
        print(f"citations: {line}")
    return 1 if found else 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    from . import citation_migrate, citation_shorten

    p = sub.add_parser(
        "citations",
        help="hold every Comprehensive Rules citation to the pinned rules release",
    )
    p.set_defaults(func=run)
    actions = p.add_subparsers(dest="action")
    citation_migrate.register(actions)
    citation_shorten.register(actions)
