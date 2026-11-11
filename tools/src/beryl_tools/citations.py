"""The citations check: every Comprehensive Rules link in the tree names a rule that exists, and
the text quoted after it is that rule's text.

AGENTS.md has rules code cite the rules with yawgatog links and quotations. Each release
renumbers and rewords rules, which turns a citation into a dead anchor or a quotation the rule no
longer contains. This check fetches the rules page, then holds each link in a comment under
`rust/crates`, `ui/src`, `tools/src`, `data` and `docs` to it:

- the anchor is the rule number without its dots, with an `R` before it;
- the rule exists on the page;
- a quotation that directly follows the link (`: "..."` or ` "..."`, after an optional closing
  parenthesis, joined across comment lines and split at `...` or `. . .`) is text of that rule,
  compared with whitespace collapsed, curly quotes straightened and an inner single quote
  matching the rule's double quote.

It needs the network, so it is no lane of `just check` or `just ci`; run it after a rules
release and after writing a citation. The page is cached under `$BERYL_CACHE` for a day;
`--refresh` fetches it again. A number-only "CR 712.x" with no link is beyond it: after a release
renumbers a rule, grep for those by hand.
"""

from __future__ import annotations

import argparse
import html
import re
import sys
import time
import urllib.request
from pathlib import Path

from . import paths

RULES_URL = "https://yawgatog.com/resources/magic-rules/"
# The page is reused this long before it is fetched again.
MAX_AGE_SECONDS = 24 * 60 * 60
ROOTS = ("rust/crates", "ui/src", "tools/src", "data", "docs")
EXTENSIONS = {".rs", ".ts", ".tsx", ".py", ".md", ".json"}
# Directories under the roots that hold no source of ours.
SKIPPED_DIRECTORIES = {"node_modules", "target", "generated"}
# How many comment lines a quotation may run over.
QUOTATION_LINES = 20

HEADING: re.Pattern[str] = re.compile(
    r"<h[1-6][^>]*\bid=(R[0-9a-z]+)[^>]*>(.*?)</h[1-6]>", re.DOTALL
)
TAG: re.Pattern[str] = re.compile(r"<[^>]*>")
LEADING_NUMBER: re.Pattern[str] = re.compile(r"^\d+(?:\.\d+[a-z]?)?\.?\s*")
LINK: re.Pattern[str] = re.compile(
    r"\[CR (\d+(?:\.\d+[a-z]?)?)\]\(https://yawgatog\.com/resources/magic-rules/#(\w+)\)"
)
COMMENT_PREFIX: re.Pattern[str] = re.compile(r"^\s*(?:///|//!|//|#|/\*\*?|\*(?!\w))\s?")
QUOTATION_START: re.Pattern[str] = re.compile(r'^\)?:?\s*"')
ELLIPSIS: re.Pattern[str] = re.compile(r"\.\.\.|\. \. \.|…")
STRAIGHTENED: dict[int, str] = str.maketrans({"“": '"', "”": '"', "‘": "'", "’": "'"})


def normalize(text: str) -> str:
    """`text` with curly quotes straightened and whitespace collapsed."""
    return " ".join(text.translate(STRAIGHTENED).split())


def parse_rules(page: str) -> dict[str, str]:
    """Each rule's anchor (`R7126a`) to its normalized text, without its number, with the
    rule's double quotes as single ones, as a comment quotes them."""
    rules = {}
    for anchor, body in HEADING.findall(page):
        text = html.unescape(TAG.sub("", body))
        rules[anchor] = normalize(LEADING_NUMBER.sub("", text.strip())).replace(
            '"', "'"
        )
    return rules


def rules_page(refresh: bool) -> str:
    """The rules page, from the cache unless it is a day old or `refresh`."""
    cached = paths.cache_root() / "rules" / "magic-rules.html"
    fresh = cached.is_file() and time.time() - cached.stat().st_mtime < MAX_AGE_SECONDS
    if not fresh or refresh:
        request = urllib.request.Request(
            RULES_URL, headers={"User-Agent": "beryl-tools"}
        )
        with urllib.request.urlopen(request, timeout=60) as response:
            body: bytes = response.read()
        cached.parent.mkdir(parents=True, exist_ok=True)
        cached.write_bytes(body)
    return cached.read_text(encoding="utf-8")


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


def quotation(text: str) -> str | None:
    """The quotation that opens `text` (what follows a link), if one does."""
    if not QUOTATION_START.match(text):
        return None
    body = text[text.index('"') + 1 :]
    close = body.find('"')
    return body if close < 0 else body[:close]


def problems_in(rel: str, lines: list[str], rules: dict[str, str]) -> list[str]:
    """Each `file:line: CR n: problem` for the links in one file's `lines`."""
    found = []
    for index, line in enumerate(lines):
        for link in LINK.finditer(line):
            number, anchor = link.group(1), link.group(2)
            where = f"{rel}:{index + 1}: CR {number}"
            expected = "R" + number.replace(".", "")
            if anchor != expected:
                found.append(f"{where}: the anchor is #{anchor}, not #{expected}")
                continue
            if anchor not in rules:
                found.append(f"{where}: no such rule on the page")
                continue
            commented = comment_text(line) is not None
            text = following_text(lines, index, link.end(), commented)
            quoted = quotation(text)
            if quoted is None:
                continue
            rule = rules[anchor]
            for segment in ELLIPSIS.split(quoted):
                segment = segment.strip()
                if segment and segment not in rule:
                    found.append(f'{where}: the rule does not say "{segment}"')
    return found


def check(root: Path, rules: dict[str, str]) -> list[str]:
    """Each problem with a citation in the files under `root`'s roots."""
    found = []
    for directory in ROOTS:
        base = root / directory
        files = sorted(base.rglob("*")) if base.is_dir() else []
        for path in files:
            rel = path.relative_to(root).as_posix()
            skipped = SKIPPED_DIRECTORIES.intersection(rel.split("/")[:-1])
            if path.suffix not in EXTENSIONS or skipped or not path.is_file():
                continue
            lines = path.read_text(encoding="utf-8").splitlines()
            found.extend(problems_in(rel, lines, rules))
    return found


def run(args: argparse.Namespace) -> int:
    rules = parse_rules(rules_page(args.refresh))
    if not rules:
        print(f"citations: no rules found on {RULES_URL}", file=sys.stderr)
        return 1
    found = check(paths.REPO, rules)
    for line in found:
        print(line)
    print(
        f"citations: {len(found)} problems against {len(rules)} rules", file=sys.stderr
    )
    return 1 if found else 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "citations",
        help="hold every Comprehensive Rules link and quotation to the rules page (network)",
    )
    p.add_argument("--refresh", action="store_true", help="fetch the rules page again")
    p.set_defaults(func=run)
