"""The git merge driver for `data/beryl3d/gates.json`, which reapplies each side's change to a
limit onto the other's.

gates.json holds absolute counts, such as the records a board's playback writes in all. Two
branches that each add work to the same board raise the same lines by different amounts, and
git's text merge stops on a conflict there even though the two additions are independent. On
2026-11-15 one Beryl 3D branch was rebased five times in three hours over the verb branches
that landed before it, and every rebase stopped on gates.json: each time someone took staging's
numbers, added the branch's own increase by hand and ran `just b3d gate` again.

This driver merges the three versions as JSON, key by key:

- a value only one side changed takes that side's value, and a key only one side added or
  removed is added or removed;
- a number both sides changed becomes `base + (ours - base) + (theirs - base)`: both changes;
- anything else both sides changed differently (a string, a list, a key one side removed and
  the other changed) is a conflict, which falls back to git's text merge with conflict markers.

The sum is right for the totals (`total_<counter>`, `handle_allocations`), since independent
work adds up over a playback. For the worst frame (`worst_frame_<counter>`), two changes may
not add up, but `just b3d gate` still holds every board to exactly its limit,
so a wrong number fails the push rather than landing: lower it with `just b3d gate --update`,
or raise it by hand with the old and new numbers in the commit body (rust/beryl3d/AGENTS.md).

Git runs it from the worktree's top folder as `python3 <this file> %O %A %B %P` (`install`,
which `by` runs before every command, sets that up in the repository's config, and
.gitattributes names the driver for gates.json). It writes the result into the `%A` file and
exits 0, or 1 on a conflict. Standard library only: git runs this file alone, not through
`by`'s environment.
"""

from __future__ import annotations

import json
import subprocess
import sys
from decimal import Decimal
from pathlib import Path
from typing import cast

# The driver's name in .gitattributes and the repository's config.
NAME = "b3d-gates"
# This file, from the worktree's top folder, where git runs merge drivers.
SCRIPT = "tools/src/beryl_tools/beryl3d/gates_merge.py"
# A key missing from one version, kept apart from JSON's null.
MISSING = object()

# Any value a JSON file holds; a plain alias, since git may run this file on an older python3.
Json = object


class Conflict(Exception):
    """Both sides changed a value in ways the driver cannot combine."""


def is_number(value: Json) -> bool:
    return isinstance(value, (int, Decimal)) and not isinstance(value, bool)


def combine(base: Json, ours: Json, theirs: Json, where: str) -> Json:
    """The merge of one value as `base`, `ours` and `theirs` hold it, any of which may be
    `MISSING`; `where` names it in a `Conflict`."""
    if ours == theirs:
        return ours
    if ours == base:
        return theirs
    if theirs == base:
        return ours
    if isinstance(ours, dict) and isinstance(theirs, dict):
        old = cast(dict[str, Json], base) if isinstance(base, dict) else {}
        mine = cast(dict[str, Json], ours)
        other = cast(dict[str, Json], theirs)
        merged: dict[str, Json] = {}
        # Our keys in our order, then the keys only theirs has in theirs.
        for key in [*mine, *(k for k in other if k not in mine)]:
            value = combine(
                old.get(key, MISSING),
                mine.get(key, MISSING),
                other.get(key, MISSING),
                f"{where}.{key}" if where else key,
            )
            if value is not MISSING:
                merged[key] = value
        return merged
    if is_number(base) and is_number(ours) and is_number(theirs):
        if all(isinstance(v, int) for v in (base, ours, theirs)):
            return cast(int, ours) + cast(int, theirs) - cast(int, base)
        b, o, t = (Decimal(cast("int | Decimal", v)) for v in (base, ours, theirs))
        return o + t - b
    raise Conflict(where or "the whole file")


def to_json(value: Json) -> Json:
    """`value` with each `Decimal` as the float it writes."""
    if isinstance(value, dict):
        return {k: to_json(v) for k, v in cast(dict[str, Json], value).items()}
    if isinstance(value, list):
        return [to_json(v) for v in cast(list[Json], value)]
    if isinstance(value, Decimal):
        # Exact in decimal, so 0.0436 + 0.001 writes 0.0446 and not 0.044600000000000005.
        return float(value)
    return value


def merge(base: str, ours: str, theirs: str) -> str:
    """The merged text of gates.json from its three versions' texts, written as `just b3d gate
    --update` writes it. Raises `Conflict` naming the first value it cannot merge, and
    `ValueError` when a version is not JSON."""
    old, mine, other = (
        json.loads(t, parse_float=Decimal) for t in (base, ours, theirs)
    )
    return json.dumps(to_json(combine(old, mine, other, "")), indent=2) + "\n"


def main(argv: list[str]) -> int:
    """Git's call: `%O %A %B [%P]`. Writes the merge into the `%A` file and returns 0, or on a
    conflict leaves git's text merge with conflict markers there and returns 1."""
    base, ours, theirs = (Path(a) for a in argv[:3])
    shown = argv[3] if len(argv) > 3 else "gates.json"
    try:
        merged = merge(*(p.read_text(encoding="utf-8") for p in (base, ours, theirs)))
    except Conflict as conflict:
        print(f"{NAME}: both sides changed {conflict} in {shown}", file=sys.stderr)
    except ValueError as error:
        print(f"{NAME}: {shown} is not JSON on one side: {error}", file=sys.stderr)
    else:
        ours.write_text(merged, encoding="utf-8", newline="\n")
        return 0
    labels = [
        "-L",
        f"{shown} (ours)",
        "-L",
        f"{shown} (base)",
        "-L",
        f"{shown} (theirs)",
    ]
    subprocess.run(
        ["git", "merge-file", *labels, str(ours), str(base), str(theirs)], check=False
    )
    return 1


def install(repo: Path) -> None:
    """Names this driver in `repo`'s git config, which every worktree of it shares, unless it
    is there already. Git ignores the `merge=` attribute of a driver its config does not name
    and merges the file as text, so a machine where `by` never ran merges as before."""
    key = f"merge.{NAME}.driver"
    python = "python" if sys.platform == "win32" else "python3"
    driver = f"{python} {SCRIPT} %O %A %B %P"

    def git(*args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", "config", *args],
            cwd=repo,
            capture_output=True,
            text=True,
            check=False,
        )

    if git("--get", key).stdout.strip() == driver:
        return
    git(f"merge.{NAME}.name", "Beryl 3D gate limits (gates_merge.py)")
    git(key, driver)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
