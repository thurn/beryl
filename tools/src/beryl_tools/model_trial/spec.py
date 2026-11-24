"""A model trial's spec: the two arms, the block size, the budget, the reviewer and the ordered
list of source tickets. It is a fenced JSON block under the heading "## Trial spec" in the
trial epic's notes, so that it travels with the trial's tickets. Every number the trial uses
comes from here, never from the code.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path

HEADING = "## Trial spec"
FENCE: re.Pattern[str] = re.compile(r"```json\s*\n(.*?)\n```", re.DOTALL)


class SpecError(ValueError):
    """A spec the tool cannot run on. The message names the field."""


@dataclass(frozen=True)
class Arm:
    """The ticket labels that route one arm's work: `tier:<tier>`, `provider:<provider>` and
    `effort:<effort>`."""

    tier: str
    provider: str
    effort: str

    def labels(self) -> list[str]:
        return [
            f"tier:{self.tier}",
            f"provider:{self.provider}",
            f"effort:{self.effort}",
        ]


@dataclass(frozen=True)
class Reviewer:
    """`model` is a key of `[pool.models.<provider>]` in `ignite.toml`."""

    model: str = "opus"
    effort: str = "high"
    runs_per_pair: int = 2


@dataclass(frozen=True)
class Spec:
    source: Arm
    copy: Arm
    cap_usd: float
    tickets: tuple[str, ...]
    block_size: int = 6
    # Zero means "every listed ticket".
    max_pairs: int = 0
    block_reserve_usd: float = 0.0
    planted_per_block: int = 0
    reviewer: Reviewer = Reviewer()
    judge_pairs: int = 10

    @property
    def pair_limit(self) -> int:
        return self.max_pairs or len(self.tickets)


def _object(value: object, field: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise SpecError(f"`{field}` must be a JSON object")
    return {str(k): v for k, v in value.items()}


def _text(data: dict[str, object], key: str, field: str) -> str:
    value = data.get(key)
    if not isinstance(value, str) or not value:
        raise SpecError(f"`{field}.{key}` must be a non-empty string")
    return value


def _number(
    data: dict[str, object], key: str, default: float | None, *, whole: bool = False
) -> float:
    value = data.get(key, default)
    if value is None:
        raise SpecError(f"`{key}` is required")
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise SpecError(f"`{key}` must be a number")
    if value < 0 or (whole and int(value) != value):
        raise SpecError(
            f"`{key}` must be a {'whole ' if whole else ''}number of at least 0"
        )
    return float(value)


def _arm(data: dict[str, object], name: str) -> Arm:
    arm = _object(data.get(name), f"arms.{name}")
    field = f"arms.{name}"
    return Arm(
        _text(arm, "tier", field),
        _text(arm, "provider", field),
        _text(arm, "effort", field),
    )


def parse(value: object, *, draft: bool = False) -> Spec:
    """The spec in `value` (the decoded JSON), with defaults for the optional fields. A `draft`
    spec, written before the user has picked the tickets, may leave `tickets` out."""
    data = _object(value, "spec")
    arms = _object(data.get("arms"), "arms")
    tickets = data.get("tickets", [] if draft else None)
    if (
        not isinstance(tickets, list)
        or (not tickets and not draft)
        or not all(isinstance(t, str) and t for t in tickets)
    ):
        raise SpecError("`tickets` must be a non-empty list of ticket IDs")
    if len(set(tickets)) != len(tickets):
        raise SpecError("`tickets` names a ticket twice")
    reviewer = _object(data.get("reviewer", {}), "reviewer")
    default = Reviewer()
    block_size = int(_number(data, "block_size", Spec.block_size, whole=True))
    if block_size < 1:
        raise SpecError("`block_size` must be at least 1")
    return Spec(
        source=_arm(arms, "source"),
        copy=_arm(arms, "copy"),
        cap_usd=_number(data, "cap_usd", None),
        tickets=tuple(str(t) for t in tickets),
        block_size=block_size,
        max_pairs=int(_number(data, "max_pairs", 0, whole=True)),
        block_reserve_usd=_number(data, "block_reserve_usd", 0.0),
        planted_per_block=int(_number(data, "planted_per_block", 0, whole=True)),
        reviewer=Reviewer(
            model=str(reviewer.get("model", default.model)),
            effort=str(reviewer.get("effort", default.effort)),
            runs_per_pair=int(
                _number(reviewer, "runs_per_pair", default.runs_per_pair, whole=True)
            ),
        ),
        judge_pairs=int(_number(data, "judge_pairs", Spec.judge_pairs, whole=True)),
    )


def from_notes(notes: str) -> Spec:
    """The spec in a trial epic's notes: the first ```json block after "## Trial spec"."""
    start = notes.find(HEADING)
    if start < 0:
        raise SpecError(f'the epic\'s notes have no "{HEADING}" heading')
    found = FENCE.search(notes, start)
    if found is None:
        raise SpecError(f'no ```json block follows "{HEADING}"')
    try:
        value = json.loads(found.group(1))
    except json.JSONDecodeError as error:
        raise SpecError(f"the spec is not valid JSON: {error}") from error
    return parse(value)


def from_file(path: Path, *, draft: bool = False) -> Spec:
    """The spec in a JSON file, as `new` and `candidates --spec` read it."""
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise SpecError(f"cannot read {path}: {error.strerror}") from error
    except json.JSONDecodeError as error:
        raise SpecError(f"{path} is not valid JSON: {error}") from error
    return parse(value, draft=draft)
