"""A small, safe Markdown renderer: the subset that bead descriptions and comments use.

Blocks: paragraphs, ATX headings, fenced code, bullet and numbered lists (one level of
nesting by indentation), block quotes and pipe tables. Inline: code spans, bold, italics,
links, bare URLs, and whatever `autolink` recognises (such as bead ids). Every character of
the source is escaped; only the markup this module writes is HTML. A link keeps its target
only if it is http, https or relative."""

from __future__ import annotations

import re
from collections.abc import Callable
from html import escape

# Turns escaped text into HTML with links, for words such as bead ids. It receives text that
# is already escaped and returns HTML.
Autolink = Callable[[str], str]

FENCE: re.Pattern[str] = re.compile(r"^(\s*)(```|~~~)(.*)$")
HEADING: re.Pattern[str] = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
BULLET: re.Pattern[str] = re.compile(r"^(\s*)[-*+]\s+(.*)$")
NUMBERED: re.Pattern[str] = re.compile(r"^(\s*)\d+[.)]\s+(.*)$")
QUOTE: re.Pattern[str] = re.compile(r"^\s*>\s?(.*)$")
TABLE_RULE: re.Pattern[str] = re.compile(
    r"^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$"
)
CODE_SPAN: re.Pattern[str] = re.compile(r"(`+)(.+?)\1")
LINK: re.Pattern[str] = re.compile(r"\[([^\]]+)\]\(([^)\s]+)\)")
URL: re.Pattern[str] = re.compile(r"(?<![\"'=])\bhttps?://[^\s<]+[^\s<.,;:!?)\]'\"]")
BOLD: re.Pattern[str] = re.compile(r"\*\*(.+?)\*\*|__(.+?)__")
ITALIC: re.Pattern[str] = re.compile(
    r"(?<![\w*])\*(?!\s)(.+?)(?<!\s)\*(?![\w*])|(?<!\w)_(?!\s)(.+?)(?<!\s)_(?!\w)"
)
SAFE_TARGET: re.Pattern[str] = re.compile(r"^(https?://|/|\.|#|[\w-]+(/|$))")
# Placeholders that keep code spans and links out of later inline rules. They use a private
# use character, which no source text contains.
MARK = ""


def no_links(text: str) -> str:
    return text


def inline(source: str, autolink: Autolink = no_links) -> str:
    """One line or paragraph of inline Markdown as HTML."""
    kept: list[str] = []

    def keep(html: str) -> str:
        kept.append(html)
        return f"{MARK}{len(kept) - 1}{MARK}"

    text = CODE_SPAN.sub(lambda m: keep(f"<code>{escape(m.group(2))}</code>"), source)

    def a_link(m: re.Match[str]) -> str:
        label, target = m.group(1), m.group(2)
        if not SAFE_TARGET.match(target):
            return keep(escape(m.group(0)))
        return keep(f'<a href="{escape(target)}">{escape(label)}</a>')

    text = LINK.sub(a_link, text)
    text = URL.sub(
        lambda m: keep(f'<a href="{escape(m.group(0))}">{escape(m.group(0))}</a>'),
        text,
    )
    text = escape(text, quote=False)
    text = BOLD.sub(lambda m: f"<strong>{m.group(1) or m.group(2)}</strong>", text)
    text = ITALIC.sub(lambda m: f"<em>{m.group(1) or m.group(2)}</em>", text)
    text = autolink(text)
    return re.sub(f"{MARK}(\\d+){MARK}", lambda m: kept[int(m.group(1))], text)


def cells(line: str) -> list[str]:
    stripped = line.strip()
    if stripped.startswith("|"):
        stripped = stripped[1:]
    if stripped.endswith("|"):
        stripped = stripped[:-1]
    return [c.strip() for c in stripped.split("|")]


def render(source: str, autolink: Autolink = no_links) -> str:
    """A Markdown document as HTML."""
    lines: list[str] = source.replace("\r\n", "\n").split("\n")
    out: list[str] = []
    i = 0

    def para_end(j: int) -> bool:
        line = lines[j]
        return (
            not line.strip()
            or bool(FENCE.match(line))
            or bool(HEADING.match(line))
            or bool(BULLET.match(line))
            or bool(NUMBERED.match(line))
            or bool(QUOTE.match(line))
        )

    while i < len(lines):
        line = lines[i]
        if not line.strip():
            i += 1
            continue
        if fence := FENCE.match(line):
            marker = fence.group(2)
            language = fence.group(3).strip().split(" ")[0]
            body: list[str] = []
            i += 1
            while i < len(lines) and not lines[i].strip().startswith(marker):
                body.append(lines[i])
                i += 1
            i += 1
            cls = f' class="language-{escape(language)}"' if language else ""
            out.append(f"<pre><code{cls}>{escape(chr(10).join(body))}</code></pre>")
            continue
        if heading := HEADING.match(line):
            # A description's headings sit under the page's own section titles.
            level = min(len(heading.group(1)) + 2, 6)
            out.append(f"<h{level}>{inline(heading.group(2), autolink)}</h{level}>")
            i += 1
            continue
        if QUOTE.match(line):
            quoted: list[str] = []
            while i < len(lines) and (m := QUOTE.match(lines[i])):
                quoted.append(m.group(1))
                i += 1
            out.append(
                f"<blockquote>{render(chr(10).join(quoted), autolink)}</blockquote>"
            )
            continue
        if (
            "|" in line
            and i + 1 < len(lines)
            and TABLE_RULE.match(lines[i + 1])
            and "|" in lines[i + 1]
        ):
            head = cells(line)
            i += 2
            body_rows: list[list[str]] = []
            while i < len(lines) and "|" in lines[i] and lines[i].strip():
                body_rows.append(cells(lines[i]))
                i += 1
            th = "".join(f"<th>{inline(c, autolink)}</th>" for c in head)
            trs = "".join(
                "<tr>" + "".join(f"<td>{inline(c, autolink)}</td>" for c in r) + "</tr>"
                for r in body_rows
            )
            out.append(
                f'<div class="c-table"><table><thead><tr>{th}</tr></thead>'
                f"<tbody>{trs}</tbody></table></div>"
            )
            continue
        if BULLET.match(line) or NUMBERED.match(line):
            html, i = list_block(lines, i, autolink)
            out.append(html)
            continue
        paragraph = [line.strip()]
        i += 1
        while i < len(lines) and not para_end(i):
            paragraph.append(lines[i].strip())
            i += 1
        out.append(f"<p>{inline(' '.join(paragraph), autolink)}</p>")
    return "\n".join(out)


def list_block(lines: list[str], i: int, autolink: Autolink) -> tuple[str, int]:
    """The list starting at line `i`, and the line after it. An item's continuation lines and
    its more deeply indented items belong to it."""
    first = BULLET.match(lines[i]) or NUMBERED.match(lines[i])
    assert first is not None
    indent = len(first.group(1))
    ordered = NUMBERED.match(lines[i]) is not None and BULLET.match(lines[i]) is None
    items: list[str] = []
    while i < len(lines):
        match = BULLET.match(lines[i]) or NUMBERED.match(lines[i])
        if match is None or len(match.group(1)) != indent:
            break
        text = [match.group(2)]
        nested = ""
        i += 1
        while i < len(lines) and lines[i].strip():
            deeper = BULLET.match(lines[i]) or NUMBERED.match(lines[i])
            if deeper is not None and len(deeper.group(1)) > indent:
                nested, i = list_block(lines, i, autolink)
                continue
            if deeper is not None or FENCE.match(lines[i]) or HEADING.match(lines[i]):
                break
            text.append(lines[i].strip())
            i += 1
        items.append(f"<li>{inline(' '.join(text), autolink)}{nested}</li>")
        # A blank line between items of the same list keeps the list going; an item of the
        # other kind (numbered after bullets) starts a new list.
        if i + 1 < len(lines) and not lines[i].strip():
            after = lines[i + 1]
            same = NUMBERED.match(after) if ordered else BULLET.match(after)
            if same is not None and len(same.group(1)) == indent:
                i += 1
    tag = "ol" if ordered else "ul"
    return f"<{tag}>{''.join(items)}</{tag}>", i
