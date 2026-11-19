"""Ignite's source-rule and repository checks: the `commit`, `source`, `markdown` and
`repository` families of `ignite/checks/rules.toml`.

Two commands, both run from the checkout's root:

- `list <family>` prints one JSON line per check of the family:
  `{"check": "source:file-length", "inputs": ["rust/crates/**", ...]}`. `inputs` are the
  paths and patterns that check reads beyond the family's own `inputs`. The commit check adds
  a `digest` of the commit subjects since `IGNITE_BASE`, which it reads and no tree ID
  covers, so Ignite reuses its pass only for the same subjects.
- `run <family> <check>...` runs the checks named, with or without the family prefix, and
  writes one JSON line per check to the file named by `IGNITE_RESULTS` (to standard output
  when it is unset): `{"check": ..., "outcome": "pass" | "fail" | "incomplete", "at": ...,
  "message": ...}`. It exits 1 when a check failed and 2 for a check it does not know.

A check reports `incomplete`, never a pass, when it cannot run: the Comprehensive Rules text
or the Scryfall rulings it needs are not in the machine's cache, `just` is older than the
justfile's format needs, or git cannot answer. Nothing here downloads anything.

Each check wraps the function that implements it in a domain module (`file_length.check`,
`two_player.check`, ...), so the rule lives in one place.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import uuid
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path

from .. import (
    board_door,
    card_diverge,
    card_names,
    check_inputs,
    citations,
    conventions,
    design_tokens,
    discarded_push,
    file_length,
    justfile_format,
    paths,
    plan_references,
    rule_ownership,
    rulings,
    scryfall,
    two_player,
)

SELF = "tools/src/beryl_tools/ignite/rules.py"
RULES_VERSION = "data/rules-version.json"
COMMIT: re.Pattern[str] = re.compile(
    r"^(feat|fix|perf|refactor|test|docs|build|ci|chore|style|revert)"
    r"(\([a-z0-9-]+\))?!?: \S"
)
COMMIT_MAX = 60
# The files that are scanned for conflict markers, by suffix.
TEXT_SUFFIXES = (".py", ".rs", ".md", ".toml", ".json", ".ts", ".tsx", ".css", ".html")
MARKER: re.Pattern[str] = re.compile(r"^(<{7}|>{7}|={7})( |$)")
# How many problem lines a result quotes.
QUOTED = 20
LOCATION: re.Pattern[str] = re.compile(r"^(\S+?:\d+)(?::|\s|$)")


class Incomplete(Exception):
    """The check could not run, so it has no verdict."""


Run = Callable[[Path], list[str]]


@dataclass(frozen=True)
class Check:
    name: str
    family: str
    run: Run
    # What the check reads beyond the family's own inputs.
    inputs: tuple[str, ...]

    @property
    def full(self) -> str:
        return f"{self.family}:{self.name}"


def module(name: str) -> str:
    return f"tools/src/beryl_tools/{name}.py"


def tree(directories: Sequence[str]) -> list[str]:
    return [f"{d}/**" for d in directories]


def git(root: Path, *args: str) -> str:
    try:
        out = subprocess.run(
            ["git", "-C", str(root), *args], capture_output=True, text=True
        )
    except OSError as error:
        raise Incomplete(f"git did not run: {error}") from error
    if out.returncode != 0:
        raise Incomplete(f"git {' '.join(args)}: {out.stderr.strip()}")
    return out.stdout


def tracked(root: Path) -> list[str]:
    """Every file git knows or would add, relative to `root`."""
    return git(root, "ls-files", "-co", "--exclude-standard").splitlines()


def markdown_files(root: Path) -> list[str]:
    """Every Markdown file but the agent skills' (whose YAML front matter mdformat would
    rewrite)."""
    return [
        p
        for p in tracked(root)
        if p.endswith(".md") and not p.startswith(".agents/") and (root / p).is_file()
    ]


# The checks.


def subject_problems(rows: Sequence[tuple[str, str]]) -> list[str]:
    """What is wrong with commit subjects, given as (short hash, subject)."""
    found = []
    for sha, subject in rows:
        if not COMMIT.match(subject):
            found.append(f"{sha} {subject!r}: not a Conventional Commit (`fix: ...`)")
        elif len(subject) > COMMIT_MAX:
            found.append(f"{sha} {subject!r}: over {COMMIT_MAX} characters")
    return found


def commit_subject(root: Path) -> list[str]:
    """The subjects of the commits since `IGNITE_BASE`, where the branch left `staging`."""
    base = os.environ.get("IGNITE_BASE")
    if not base:
        raise Incomplete("IGNITE_BASE names no commit to count from")
    log = git(root, "log", "--no-merges", "--format=%h%x09%s", f"{base}..HEAD")
    rows = [tuple(row.split("\t", 1)) for row in log.splitlines() if "\t" in row]
    return subject_problems([(sha, subject) for sha, subject in rows])


def commit_digest(root: Path) -> str:
    """A hash of what `commit_subject` reads: the subjects since `IGNITE_BASE`. Without a base,
    or when git cannot answer, a digest no other listing shares, so no pass is reused.
    """
    base = os.environ.get("IGNITE_BASE")
    try:
        if not base:
            raise Incomplete("no IGNITE_BASE")
        log = git(root, "log", "--no-merges", "--format=%s", f"{base}..HEAD")
    except Incomplete:
        return f"unknown:{uuid.uuid4().hex}"
    return hashlib.sha256(log.encode()).hexdigest()


# What a check reads that no file holds, by check name (see `commit_digest`).
DIGESTS: dict[str, Callable[[Path], str]] = {"commit-subject": commit_digest}


def conflict_markers_in(root: Path, files: Sequence[str]) -> list[str]:
    found = []
    for rel in files:
        path = root / rel
        if not path.is_file() or not rel.endswith(TEXT_SUFFIXES):
            continue
        for number, line in enumerate(path.read_text(errors="replace").splitlines(), 1):
            if MARKER.match(line):
                found.append(f"{rel}:{number}: conflict marker")
    return found


def conflict_markers(root: Path) -> list[str]:
    return conflict_markers_in(
        root, [p for p in tracked(root) if not p.endswith(".md")]
    )


def markdown_conflict_markers(root: Path) -> list[str]:
    return conflict_markers_in(root, markdown_files(root))


def markdown_format(root: Path) -> list[str]:
    files = markdown_files(root)
    if not files:
        return []
    out = subprocess.run(
        [sys.executable, "-m", "mdformat", "--check", *files],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if out.returncode == 0:
        return []
    if "No module named" in out.stderr:
        raise Incomplete(out.stderr.strip())
    lines = [line for line in (out.stdout + out.stderr).splitlines() if line]
    return [f"mdformat: {line}" for line in lines] + ["mdformat: run `just fmt`"]


def justfile_format_check(root: Path) -> list[str]:
    try:
        said = subprocess.run(["just", "--version"], capture_output=True, text=True)
    except OSError as error:
        raise Incomplete(f"just did not run: {error}") from error
    have = justfile_format.version(said.stdout)
    if have is None or have < justfile_format.MINIMUM:
        minimum = ".".join(map(str, justfile_format.MINIMUM))
        raise Incomplete(
            f"just {said.stdout.strip() or 'has no version'} is older than {minimum}: "
            "run `cargo install just --locked`"
        )
    command = ["just", "--fmt", "--check", "--unstable"]
    out = subprocess.run(command, cwd=root, capture_output=True, text=True)
    if out.returncode == 0:
        return []
    lines = [line for line in (out.stdout + out.stderr).splitlines() if line]
    return [f"justfile: {line}" for line in lines] + [
        "justfile: not formatted as `just --fmt` writes it"
    ]


def pinned_rules(root: Path) -> dict[str, str]:
    """The pinned Comprehensive Rules, from the cache; incomplete when they are not there."""
    rules, missing = citations.step_rules(citations.pinned(root), fetch=False)
    if rules is None:
        raise Incomplete(missing)
    return rules


def citations_in(root: Path, markdown: bool) -> list[str]:
    only = [
        rel
        for rel, _ in citations.source_files(root)
        if rel.endswith(".md") == markdown
    ]
    if not only:
        return []
    return citations.check(root, pinned_rules(root), only)


def citations_code(root: Path) -> list[str]:
    return citations_in(root, markdown=False)


def citations_docs(root: Path) -> list[str]:
    return citations_in(root, markdown=True)


def rule_ownership_check(root: Path) -> list[str]:
    return rule_ownership.check(root, pinned_rules(root))


def divergences(root: Path) -> list[str]:
    entries = json.loads((root / "data" / "oracle-divergences.json").read_text("utf-8"))
    return card_diverge.problems(entries, pinned_rules(root))


def rulings_check(root: Path) -> list[str]:
    bulk = scryfall.cache_dir() / "rulings.jsonl.gz"
    if not bulk.is_file():
        raise Incomplete(f"{bulk} is not cached")
    library = root / "rust" / "crates" / "beryl_cards" / "src" / "library"
    try:
        return rulings.audit(
            root / "data" / "rulings-ledger.json", bulk, library=library
        )
    except (ValueError, OSError) as error:
        return [str(error)]


def card_names_check(root: Path) -> list[str]:
    allowlist = root / "data" / "card-names-allowlist.json"
    return card_names.check(root, allowlist, card_names.current_index())


def check_inputs_check(root: Path) -> list[str]:
    try:
        return check_inputs.check(root, root / allowlist("check-inputs"), tracked(root))
    except RuntimeError as error:
        raise Incomplete(str(error)) from error


def file_length_check(root: Path) -> list[str]:
    return file_length.check(root, root / "data" / "file-length-allowlist.json")


def conventions_check(root: Path) -> list[str]:
    try:
        if not conventions.stale(conventions.DATA, conventions.PAGE):
            return []
    except ValueError as error:
        return [str(error)]
    page, data = conventions.PAGE, conventions.DATA
    return [
        f"{page.name} is stale: edit {data.name}, run `just conventions` and commit both"
    ]


def design_tokens_check(root: Path) -> list[str]:
    try:
        if not design_tokens.stale(design_tokens.DATA, design_tokens.STYLESHEET):
            return []
    except (ValueError, KeyError) as error:
        return [str(error)]
    sheet, data = design_tokens.STYLESHEET, design_tokens.DATA
    return [
        f"{sheet.name} is stale: edit {data.name}, run `just design-tokens` and commit both"
    ]


MARKDOWN = ["*.md", "**/*.md"]
CITING: tuple[str, ...] = citations.ROOTS
CITED_CODE: list[str] = [
    *tree([r for r in CITING if r != "docs"]),
    *[
        f"docs/{star}*{ext}"
        for ext in sorted(citations.EXTENSIONS - {".md"})
        for star in ("", "**/")
    ],
]
CITED_DOCS: list[str] = [f"{r}/**/*.md" for r in CITING]
RULES_TREE: list[str] = tree(two_player.RULES)


def allowlist(name: str) -> str:
    return f"data/{name}-allowlist.json"


CARD_NAME_TREE: list[str] = tree(card_names.SCOPE)
CARD_NAME_MODULES: list[str] = [
    module(m) for m in ("card_names", "cards", "card_files", "card_store", "set_groups")
]

CARD_NAME_INPUTS: list[str] = [
    *CARD_NAME_TREE,
    "rust/crates/**",
    "rust/cards/**",
    "docs/sample-decks/**",
    "data/card-store.tsv",
    allowlist("card-names"),
    *CARD_NAME_MODULES,
]

CHECKS: tuple[Check, ...] = (
    Check("commit-subject", "commit", commit_subject, ()),
    Check(
        "conflict-markers",
        "source",
        conflict_markers,
        tuple(f"**/*{s}" for s in TEXT_SUFFIXES if s != ".md"),
    ),
    Check(
        "file-length",
        "source",
        file_length_check,
        (
            *tree(file_length.SCOPE),
            allowlist("file-length"),
            module("file_length"),
        ),
    ),
    Check(
        "check-inputs",
        "source",
        check_inputs_check,
        # A literal in a Rust file may name a path anywhere the Rust code reads from, and the
        # check reads the manifests and the Cargo workspaces. Of docs/, the Rust code reads
        # only the sample decks and the incident reports, so a change to the rest of the
        # documentation leaves this check alone.
        (
            "rust/**",
            "ui/**",
            "data/**",
            "docs/sample-decks/**",
            "docs/incidents/**",
            "ignite/checks/**",
            allowlist("check-inputs"),
            module("check_inputs"),
        ),
    ),
    Check(
        "card-names",
        "source",
        card_names_check,
        tuple(CARD_NAME_INPUTS),
    ),
    Check(
        "two-player",
        "source",
        lambda root: two_player.check(root),
        (*RULES_TREE, module("two_player")),
    ),
    Check(
        "board-door",
        "source",
        lambda root: board_door.check(root),
        (*RULES_TREE, module("board_door"), module("two_player")),
    ),
    Check(
        "discarded-push",
        "source",
        lambda root: discarded_push.check(root),
        (*tree(discarded_push.SCANNED), module("discarded_push"), module("two_player")),
    ),
    Check(
        "plan-references",
        "source",
        lambda root: plan_references.check(root),
        (*tree(plan_references.SCANNED), module("plan_references")),
    ),
    Check(
        "citations",
        "source",
        citations_code,
        (*CITED_CODE, RULES_VERSION, module("citations")),
    ),
    Check(
        "rule-ownership",
        "source",
        rule_ownership_check,
        (
            *tree(rule_ownership.SOURCE_ROOTS),
            rule_ownership.ALLOWLIST,
            RULES_VERSION,
            module("rule_ownership"),
            module("citations"),
        ),
    ),
    Check(
        "divergences",
        "source",
        divergences,
        (
            "data/oracle-divergences.json",
            RULES_VERSION,
            module("card_diverge"),
            module("citations"),
        ),
    ),
    Check(
        "rulings",
        "source",
        rulings_check,
        (
            "data/rulings-ledger.json",
            "data/card-store.tsv",
            "rust/cards/**",
            "rust/crates/beryl_cards/src/library/**",
            "data/set-groups.json",
            "rust/crates/beryl_card/src/**",
            module("rulings"),
        ),
    ),
    Check(
        "markdown-format", "markdown", markdown_format, (*MARKDOWN, ".mdformat.toml")
    ),
    Check(
        "markdown-conflict-markers",
        "markdown",
        markdown_conflict_markers,
        tuple(MARKDOWN),
    ),
    Check(
        "citations-docs",
        "markdown",
        citations_docs,
        (*CITED_DOCS, RULES_VERSION, module("citations")),
    ),
    Check(
        "justfile-format",
        "repository",
        justfile_format_check,
        ("justfile", module("justfile_format")),
    ),
    Check(
        "conventions",
        "repository",
        conventions_check,
        (
            "data/ui-conventions.json",
            "data/arena-motion.json",
            "docs/ui-conventions.md",
            module("conventions"),
        ),
    ),
    Check(
        "design-tokens",
        "repository",
        design_tokens_check,
        (
            "data/design-tokens.json",
            "ui/src/design/tokens.css",
            module("design_tokens"),
        ),
    ),
)


def family_checks(family: str) -> list[Check]:
    return [c for c in CHECKS if c.family == family]


def matching(root: Path, patterns: Sequence[str]) -> list[str]:
    """`patterns` without the wildcard patterns that match no tracked file. Ignite refuses a
    check whose input pattern matches nothing, and a check that scans every file of some
    extensions under several roots names patterns such as `docs/*.py` that match nothing today.
    A file added later where such a pattern would match is covered once the pattern is listed
    again, which any change to the check's other inputs causes."""
    kept: list[str] = []
    for pattern in patterns:
        plain_tree = pattern.endswith("/**") and "*" not in pattern[: -len("/**")]
        if "*" not in pattern or plain_tree:
            kept.append(pattern)
        elif git(
            root, "ls-files", "-co", "--exclude-standard", "--", f":(glob){pattern}"
        ):
            kept.append(pattern)
    return kept


def listing(family: str, root: Path) -> list[dict[str, str | list[str]]]:
    lines: list[dict[str, str | list[str]]] = []
    for c in family_checks(family):
        line: dict[str, str | list[str]] = {
            "check": c.full,
            "inputs": [SELF, *matching(root, c.inputs)],
        }
        if c.name in DIGESTS:
            line["digest"] = DIGESTS[c.name](root)
        lines.append(line)
    return lines


def result(check: Check, root: Path) -> dict[str, object]:
    """One check's result line."""
    try:
        found = check.run(root)
    except Incomplete as reason:
        return {"check": check.full, "outcome": "incomplete", "message": str(reason)}
    if not found:
        return {"check": check.full, "outcome": "pass"}
    quoted = found[:QUOTED]
    if len(found) > QUOTED:
        quoted.append(f"(+{len(found) - QUOTED} more)")
    line: dict[str, object] = {
        "check": check.full,
        "outcome": "fail",
        "message": "\n".join(quoted),
    }
    where = LOCATION.match(found[0])
    if where:
        line["at"] = where.group(1)
    return line


def find(family: str, names: Sequence[str]) -> list[Check]:
    known = {c.name: c for c in family_checks(family)}
    chosen = []
    for name in names:
        short = name.removeprefix(f"{family}:")
        if short not in known:
            raise KeyError(name)
        chosen.append(known[short])
    return chosen


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser(prog="beryl_tools.ignite.rules")
    sub = parser.add_subparsers(dest="verb", required=True)
    for verb in ("list", "run"):
        p = sub.add_parser(verb)
        p.add_argument("family", choices=sorted({c.family for c in CHECKS}))
        if verb == "run":
            p.add_argument("checks", nargs="+")
    args = parser.parse_args(argv)
    if args.verb == "list":
        for entry in listing(args.family, paths.REPO):
            print(json.dumps(entry))
        return 0
    try:
        chosen = find(args.family, args.checks)
    except KeyError as unknown:
        print(f"no check named {unknown.args[0]} in {args.family}", file=sys.stderr)
        return 2
    lines = [result(check, paths.REPO) for check in chosen]
    text = "".join(json.dumps(line) + "\n" for line in lines)
    target = os.environ.get("IGNITE_RESULTS")
    if target:
        with open(target, "a", encoding="utf-8") as results:
            results.write(text)
    else:
        sys.stdout.write(text)
    return 1 if any(line["outcome"] == "fail" for line in lines) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
