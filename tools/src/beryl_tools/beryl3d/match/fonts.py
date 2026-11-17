"""Checks that Beryl 3D's bundled fonts are the faces the web interface draws with: the same
family and version in each file's `name` table (fonts.mjs reads them). A difference is a
warning in the snapshot's report, since text is compared as data and a different face would
make every line of it differ."""

from __future__ import annotations

import json
import subprocess
from collections.abc import Mapping
from pathlib import Path

from ... import paths as repo_paths
from .. import paths as beryl3d_paths

HERE: Path = Path(__file__).resolve().parent
BUNDLED: Path = beryl3d_paths.WORKSPACE / "crates" / "beryl_scene" / "fonts"
FONTSOURCE: Path = repo_paths.UI / "node_modules" / "@fontsource-variable"
# Each bundled font and the web's WOFF2 file of the same face: fontsource's Latin subset of its
# variable weight axis, which the web interface's text uses.
PAIRS: tuple[tuple[str, str], ...] = (
    ("Cinzel.ttf", "cinzel/files/cinzel-latin-wght-normal.woff2"),
    ("CrimsonPro.ttf", "crimson-pro/files/crimson-pro-latin-wght-normal.woff2"),
    ("OpenSans.ttf", "open-sans/files/open-sans-latin-wght-normal.woff2"),
)


def names(files: list[Path]) -> dict[str, dict[str, str | None]]:
    """Each file's `name` table family and version, or its `error`, as fonts.mjs prints them."""
    done = subprocess.run(
        ["node", str(HERE / "fonts.mjs"), *map(str, files)],
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:
        failed: dict[str, dict[str, str | None]] = {}
        for file in files:
            failed[str(file)] = {"error": done.stderr.strip() or "fonts.mjs failed"}
        return failed
    return json.loads(done.stdout)


def differences(read: Mapping[str, Mapping[str, str | None]]) -> list[str]:
    """A warning line for each pair of `PAIRS` whose two files differ in family or version, or
    whose file could not be read, given the names `read` from each file."""
    out: list[str] = []
    for ours, theirs in PAIRS:
        bundled = read.get(str(BUNDLED / ours), {"error": "not read"})
        web = read.get(str(FONTSOURCE / theirs), {"error": "not read"})
        for side, file, found in (("Beryl 3D", ours, bundled), ("web", theirs, web)):
            if "error" in found:
                out.append(f"warning: fonts: {side}'s {file}: {found['error']}")
        if "error" in bundled or "error" in web:
            continue
        for field in ("family", "version"):
            if bundled.get(field) != web.get(field):
                out.append(
                    f"warning: fonts: {ours} has {field} {bundled.get(field)!r} but the web's "
                    f"{theirs} has {web.get(field)!r}"
                )
    return out


def check() -> list[str]:
    """The warning lines for every bundled font that is not the web's face."""
    files = [p for ours, theirs in PAIRS for p in (BUNDLED / ours, FONTSOURCE / theirs)]
    return differences(names(files))
