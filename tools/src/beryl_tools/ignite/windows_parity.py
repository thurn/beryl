"""Validate Windows check enrollment and the evidence from a qualification run."""

from __future__ import annotations

import argparse
import json
import tomllib
from collections import Counter
from pathlib import Path
from typing import Any

from ..paths import REPO

# The browser families were retired with the web client. Keep their exclusions
# explicit in case one returns. The Simulator build still runs on macOS.
WINDOWS_EXCLUSIONS: frozenset[str] = frozenset(
    {
        "native-ios",
        "ui-test",
        "ui-smoke",
        "shots",
        "motion",
        "ui-counts",
        "b3d-web-drift",
    }
)


def windows_inventory(root: Path) -> tuple[set[str], list[str]]:
    """Compare every current declaration with the configured Windows capabilities."""
    config = tomllib.loads((root / "ignite.toml").read_text(encoding="utf-8"))
    cygnus = config["machine"]["cygnus"]
    enabled = set(cygnus.get("families", []))
    resources = cygnus.get("resources", {})
    families = {
        name: declaration
        for manifest in (root / config["repository"]["checks"]).glob("*.toml")
        for name, declaration in tomllib.loads(manifest.read_text(encoding="utf-8"))
        .get("family", {})
        .items()
    }
    expected = families.keys() - WINDOWS_EXCLUSIONS
    gaps = [
        f"{name}: missing Windows enrollment" for name in sorted(expected - enabled)
    ]
    gaps.extend(
        f"{name}: unexpected Windows enrollment" for name in sorted(enabled - expected)
    )
    if cygnus["os"] != "windows":
        gaps.append("Cygnus: expected a Windows verifier")
    if families.get("native-ios", {}).get("platforms") != ["macos"]:
        gaps.append("native-ios: expected macOS Simulator coverage")
    for name in sorted(expected):
        family = families[name]
        platforms = family["platforms"]
        if platforms != "any" and "windows" not in platforms:
            gaps.append(f"{name}: declaration excludes Windows")
        for resource in family.get("resources", []):
            if resources.get(resource, 0) < 1:
                gaps.append(f"{name}: Cygnus has no {resource} resource")
    return expected, gaps


def evidence_gaps(
    families: set[str], evidence: dict[str, Any], revision: str, verifier: str
) -> list[str]:
    """Require a fresh pass for every independently listed check or idle sample."""
    listings = evidence["listings"]
    gaps = [
        f"{family}: no listing or idle sample"
        for family in sorted(families - listings.keys())
    ]
    expected: set[str] = set()
    for family, checks in sorted(listings.items()):
        if family not in families or not checks:
            gaps.append(f"{family}: unexpected or empty listing")
        for check in checks:
            prefix, _, name = check.partition(":")
            if prefix != family or not name or check in expected:
                gaps.append(f"{family}: invalid or duplicate listed check {check}")
            expected.add(check)

    reported: Counter[str] = Counter()
    for result in evidence["results"]:
        check = result["check"]
        reported[check] += 1
        if result.get("outcome") != "pass" or result.get("cached") is not False:
            gaps.append(f"{check}: no fresh passing verdict")
        if (
            result.get("machine") != verifier
            or result.get("os") != "windows"
            or result.get("revision") != revision
        ):
            gaps.append(f"{check}: verdict is from a different verifier or revision")
    gaps.extend(
        f"{check}: missing verdict" for check in sorted(expected - reported.keys())
    )
    gaps.extend(
        f"{check}: unexpected verdict" for check in sorted(reported.keys() - expected)
    )
    gaps.extend(
        f"{check}: duplicate verdict"
        for check, count in sorted(reported.items())
        if count > 1
    )
    return gaps


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--verifier", required=True)
    args = parser.parse_args()
    families, gaps = windows_inventory(REPO)
    evidence = json.loads(args.evidence.read_text(encoding="utf-8"))
    gaps.extend(evidence_gaps(families, evidence, args.revision, args.verifier))
    if gaps:
        print("\n".join(gaps))
        return 1
    print(
        f"{len(evidence['results'])} fresh Cygnus verdicts across {len(families)} families"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
