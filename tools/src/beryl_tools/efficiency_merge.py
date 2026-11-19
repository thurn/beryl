"""The git merge driver for `data/efficiency-changes.json`, which keeps both sides' new entries.

The file is a JSON array that every change made to save time or tokens appends to. Two such
changes in flight both add lines at the array's end, so git's text merge stops on a conflict
there even though the additions are independent. On 2026-11-19 three of about twenty Ignite
rebases (a landing, a sync and another landing) stopped on this file, and each needed a hand
rebase.

This driver merges the three versions as JSON arrays: the entries of the base version stay,
then come the entries ours added, then the entries theirs added, leaving out an entry that
both sides added. A side that edited or removed an entry of the base version is merged only
when the other side left the base entries alone or made the same edit; two different edits
are a conflict, which falls back to git's text merge with conflict markers.

Git runs it from the worktree's top folder as `python3 <this file> %O %A %B %P` (`install`,
which `by` runs before every command, sets that up in the repository's config, and
.gitattributes names the driver). It writes the result into the `%A` file and exits 0, or 1
on a conflict. Standard library only: git runs this file alone, not through `by`'s
environment.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

# The driver's name in .gitattributes and the repository's config.
NAME = "efficiency-changes"
# This file, from the worktree's top folder, where git runs merge drivers.
SCRIPT = "tools/src/beryl_tools/efficiency_merge.py"


class Conflict(Exception):
    """Both sides changed the entries that were already in the file, in different ways."""


def entries(text: str) -> list[object]:
    """The array `text` holds; `ValueError` when it is not a JSON array."""
    value = json.loads(text)
    if not isinstance(value, list):
        raise ValueError("not a JSON array")
    return value


def merge(base: str, ours: str, theirs: str) -> str:
    """The merged text of the file from its three versions' texts, written as the file is
    now. Raises `Conflict` when both sides changed the base entries differently, and
    `ValueError` when a version is not a JSON array."""
    old, mine, other = entries(base), entries(ours), entries(theirs)
    count = len(old)
    kept_mine, kept_other = mine[:count], other[:count]
    if kept_mine != old and kept_other != old and kept_mine != kept_other:
        raise Conflict("an entry that was already in the file")
    merged = kept_mine if kept_mine != old else kept_other
    for added in (mine[count:], other[count:]):
        merged.extend(entry for entry in added if entry not in merged[count:])
    return json.dumps(merged, indent=2, ensure_ascii=False) + "\n"


def main(argv: list[str]) -> int:
    """Git's call: `%O %A %B [%P]`. Writes the merge into the `%A` file and returns 0, or on a
    conflict leaves git's text merge with conflict markers there and returns 1."""
    base, ours, theirs = (Path(a) for a in argv[:3])
    shown = argv[3] if len(argv) > 3 else "efficiency-changes.json"
    try:
        merged = merge(*(p.read_text(encoding="utf-8") for p in (base, ours, theirs)))
    except Conflict as conflict:
        print(f"{NAME}: both sides changed {conflict} in {shown}", file=sys.stderr)
    except ValueError as error:
        print(
            f"{NAME}: {shown} is not a JSON array on one side: {error}", file=sys.stderr
        )
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
    git(f"merge.{NAME}.name", "Efficiency changes (efficiency_merge.py)")
    git(key, driver)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
