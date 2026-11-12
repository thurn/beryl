"""The file-length check: no source file over 800 lines (docs/project-50k.md, K17).

A file over the limit is on `data/file-length-allowlist.json` with its length, the burn-down
list of splits: a listed file may not grow, and the check fails when one is shorter than its
entry (lower the number) or under the limit (remove the entry). Tests, generated and frozen
code, the Playwright drivers, the expert system and the retired long-think code are exempt.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from . import paths

LIMIT = 800
ALLOWLIST: Path = paths.REPO / "data" / "file-length-allowlist.json"

SCOPE = ("rust/crates", "ui/src", "ui/scripts", "tools/src")
SUFFIXES = {".rs", ".ts", ".tsx", ".css", ".py", ".mjs"}
# Paths from the repository root: a directory (its whole subtree) or a file.
EXEMPT = (
    "ui/src/generated",
    "rust/crates/beryl_cards/src/names.rs",
    "rust/crates/beryl_search/src/frozen",
    "ui/scripts/drive.mjs",
    "rust/crates/beryl_search/src/expert",
    "rust/crates/beryl_search/src/long.rs",
    "rust/crates/beryl_cli/src/longthink",
)
# Tests live in `tests` directories (Rust's `tests/` and `src/tests`, tools/tests) or in
# files named tests.rs.
SKIPPED_DIRECTORIES = {"tests", "node_modules", "target"}


def exempt(rel: str) -> bool:
    parts = rel.split("/")
    return (
        parts[-1] == "tests.rs"
        or bool(SKIPPED_DIRECTORIES.intersection(parts[:-1]))
        or any(rel == e or rel.startswith(e + "/") for e in EXEMPT)
    )


def lengths(root: Path) -> dict[str, int]:
    """The lines of each in-scope source file under `root` over the limit, by path."""
    found: dict[str, int] = {}
    for directory in SCOPE:
        base = root / directory
        for path in sorted(base.rglob("*")) if base.is_dir() else []:
            rel = path.relative_to(root).as_posix()
            if path.suffix in SUFFIXES and path.is_file() and not exempt(rel):
                data = path.read_bytes()
                count = data.count(b"\n") + (
                    1 if data and not data.endswith(b"\n") else 0
                )
                if count > LIMIT:
                    found[rel] = count
    return found


def problems(over: dict[str, int], allowed: dict[str, int]) -> list[str]:
    """Why the files and the allowlist disagree, one line each; none when they agree."""
    list_file = ALLOWLIST.relative_to(paths.REPO)
    found = []
    for path, count in sorted(over.items()):
        if path not in allowed:
            found.append(
                f"{path}: {count} lines, over {LIMIT} and not on the allowlist: split it"
            )
        elif count > allowed[path]:
            found.append(
                f"{path}: {count} lines, up from {allowed[path]} on the allowlist: "
                "a file over the limit may not grow"
            )
    for path, count in sorted(allowed.items()):
        have = over.get(path)
        if have is None:
            found.append(
                f"{path}: no longer over {LIMIT} lines (or gone): remove the entry "
                f"from {list_file}"
            )
        elif have < count:
            found.append(
                f"{path}: {have} lines, below the allowlist's {count}: lower it in {list_file}"
            )
    return found


def check(root: Path, allowlist: Path, shrink: bool = False) -> list[str]:
    """What is wrong with the files under `root` against the `allowlist` file. With `shrink`
    the file is first lowered to what the code has: only down."""
    allowed: dict[str, int] = json.loads(allowlist.read_text())
    over = lengths(root)
    if shrink:
        allowed = {p: min(c, over[p]) for p, c in sorted(allowed.items()) if p in over}
        allowlist.write_text(json.dumps(allowed, indent=2) + "\n")
    return problems(over, allowed)


def step(*args: str) -> int:
    """The `file-length` check; `--shrink` also lowers the allowlist to the files' lengths."""
    found = check(paths.REPO, ALLOWLIST, "--shrink" in args)
    for line in found:
        print(f"file-length: {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
