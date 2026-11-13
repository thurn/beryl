"""`just citations shorten <paths>`: rewrite each linked citation in a Rust doc comment as a
reference, with the links gathered as definitions at the end of the comment.

A linked citation, `[CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)`, puts
a 56-character URL in the middle of a sentence. Rustdoc and Markdown read the reference form,
`[CR 118.12a]`, the same way when the same doc comment defines the link:

    /// ... unless you discard a basic land card ([CR 118.12a]): "This means ..."
    ///
    /// [CR 118.12a]: https://yawgatog.com/resources/magic-rules/#R11812a

The citations check (`citations.py`) holds both forms to the pinned rules release.
"""

from __future__ import annotations

import argparse
import re
import textwrap
from pathlib import Path

from . import paths

LINKED: re.Pattern[str] = re.compile(
    r"\[CR ([\d.a-z]+)\]\((https://yawgatog\.com/resources/magic-rules/#\w+)\)"
)
DOC: re.Pattern[str] = re.compile(r"^(\s*)(///|//!)( ?)(.*)$")
# rustfmt's line width, which the comments are wrapped to.
WIDTH = 100
# A doc comment line that is no prose to rewrap: a list item, a heading, a code fence or a
# link definition.
NOT_PROSE: re.Pattern[str] = re.compile(
    r"^\s*([-*+] |\d+\. |#|```|\[[^\]]+\]: https?://)"
)
# A reference at the start of a doc line, which Markdown would read as a link definition.
LINE_START_REFERENCE: re.Pattern[str] = re.compile(
    r"^(\s*(?:///|//!) ?)\[CR ([\d.a-z]+)\](?!\(|: ?https?://)"
)
# The space in a reference, which a line never breaks at.
# The space before a reference is kept too, so that wrapping never starts a line with one.
REFERENCE_SPACE: re.Pattern[str] = re.compile(r"(?: )?\[CR (?=[\d.a-z]+\])")
UNBROKEN = "\x00"


def rewrap(block: list[str], prefix: str, changed: set[int]) -> list[str]:
    """`block`, one doc comment's lines that each start with `prefix`, with each paragraph
    that holds a line of `changed` wrapped again to [`WIDTH`] columns. A paragraph with a
    list, a heading, a code fence or a link definition in it is left as it is."""
    out: list[str] = []
    paragraph: list[int] = []

    def flush() -> None:
        texts = [block[i][len(prefix) :].strip() for i in paragraph]
        if changed.intersection(paragraph) and not any(
            NOT_PROSE.match(t) for t in texts
        ):
            out.extend(
                f"{prefix} {line.replace(UNBROKEN, ' ')}"
                for line in textwrap.wrap(
                    REFERENCE_SPACE.sub(
                        lambda m: m.group(0).replace(" ", UNBROKEN),
                        " ".join(texts),
                    ),
                    width=WIDTH - len(prefix) - 1,
                    break_long_words=False,
                    break_on_hyphens=False,
                )
            )
        else:
            out.extend(block[i] for i in paragraph)
        paragraph.clear()

    for i, line in enumerate(block):
        if line.strip() == prefix.strip():
            flush()
            out.append(line)
        else:
            paragraph.append(i)
    flush()
    return out


def shorten(text: str) -> str:
    """`text`, a Rust source, with each doc comment's linked citations as references."""
    lines = text.split("\n")
    out: list[str] = []
    i = 0
    while i < len(lines):
        match = DOC.match(lines[i])
        if not match:
            out.append(lines[i])
            i += 1
            continue
        indent, marker = match.group(1), match.group(2)
        block: list[str] = []
        while i < len(lines):
            line = DOC.match(lines[i])
            if not line or line.group(1) != indent or line.group(2) != marker:
                break
            block.append(lines[i])
            i += 1
        links: dict[str, str] = {}

        def reference(m: re.Match[str]) -> str:
            links.setdefault(m.group(1), m.group(2))
            return f"[CR {m.group(1)}]"

        shortened = [LINKED.sub(reference, line) for line in block]
        changed = {i for i, line in enumerate(block) if shortened[i] != line}
        block = rewrap(shortened, f"{indent}{marker}", changed)
        # A reference that still starts a line stays an inline link.
        block = [
            LINE_START_REFERENCE.sub(
                lambda m: (
                    f"{m.group(1)}[CR {m.group(2)}]({links[m.group(2)]})"
                    if m.group(2) in links
                    else m.group(0)
                ),
                line,
            )
            for line in block
        ]
        defined = {
            m.group(1)
            for line in block
            for m in re.finditer(r"\[CR ([\d.a-z]+)\]: https://", line)
        }
        used = {
            m.group(1)
            for line in block
            for m in re.finditer(r"\[CR ([\d.a-z]+)\](?!\(|: ?https?://)", line)
        }
        new = [(n, url) for n, url in links.items() if n not in defined and n in used]
        if new:
            prefix = f"{indent}{marker}"
            if block[-1].strip() != marker and not re.search(
                r"\[CR [\d.a-z]+\]: https://", block[-1]
            ):
                block.append(prefix)
            block.extend(f"{prefix} [CR {n}]: {url}" for n, url in new)
        out.extend(block)
    return "\n".join(out)


def run(args: argparse.Namespace) -> int:
    changed = 0
    for name in args.paths:
        for path in (
            sorted(Path(name).rglob("*.rs")) if Path(name).is_dir() else [Path(name)]
        ):
            text = path.read_text(encoding="utf-8")
            new = shorten(text)
            if new != text:
                path.write_text(new, encoding="utf-8")
                changed += 1
    print(f"citations shorten: {changed} files rewritten")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "shorten",
        help="rewrite linked citations in Rust doc comments as references",
    )
    p.add_argument(
        "paths",
        nargs="*",
        default=[str(paths.REPO / "rust" / "cards")],
        help="files or directories (default: rust/cards)",
    )
    p.set_defaults(func=run)
