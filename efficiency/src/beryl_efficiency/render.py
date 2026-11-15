"""Results and their text rendering. A query returns a `Result`: plain data for `--json` and
the page, the lines the command prints, and the queries that drill into it."""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, field


@dataclass
class Result:
    query: str
    data: dict[str, object]
    lines: list[str] = field(default_factory=list)
    more: list[str] = field(default_factory=list)

    def json(self) -> dict[str, object]:
        return {"query": self.query, **self.data, "more": self.more}


def cell(value: object) -> str:
    """A value as a table shows it: unknown is a dash, never zero."""
    if value is None:
        return "-"
    if isinstance(value, float):
        return f"{value:.2f}"
    return str(value)


def table(headers: Sequence[str], rows: Sequence[Sequence[object]]) -> list[str]:
    """`rows` under `headers` in columns two spaces apart, numbers right-aligned."""
    shown = [[cell(v) for v in row] for row in rows]
    widths: list[int] = [
        max([len(h), *(len(r[i]) for r in shown)]) for i, h in enumerate(headers)
    ]
    numeric: list[bool] = [
        bool(rows) and all(isinstance(r[i], (int, float)) or r[i] is None for r in rows)
        for i in range(len(headers))
    ]

    def line(values: Sequence[str]) -> str:
        return "  ".join(
            v.rjust(w) if n else v.ljust(w) for v, w, n in zip(values, widths, numeric)
        ).rstrip()

    return [line(list(headers)), *(line(r) for r in shown)]


def text(result: Result) -> str:
    lines = list(result.lines)
    if result.more:
        lines.append("")
        lines.append("more: " + " | ".join(result.more))
    return "\n".join(lines)
