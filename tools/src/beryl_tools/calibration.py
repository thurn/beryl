"""Select a measurement profile by Ignite verifier and check its live environment."""

from __future__ import annotations

import hashlib
import json
import os
import re
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path


class SetupError(ValueError):
    """The measurement has no compatible calibration and cannot yield a verdict."""


def verifier() -> str:
    """The daemon's registered ID, also available from its registration for manual runs."""
    machine = os.environ.get("IGNITE_MACHINE")
    if not machine:
        home = os.environ.get("IGNITE_HOME")
        directory = Path(home) if home else Path.home() / ".config" / "ignite"
        try:
            machine = json.loads((directory / "machine.json").read_text())["machine"]
        except (OSError, ValueError, KeyError, TypeError):
            raise SetupError(
                "no registered Ignite verifier for this measurement"
            ) from None
    if not isinstance(machine, str) or not re.fullmatch(r"[a-zA-Z0-9_-]+", machine):
        raise SetupError("invalid Ignite verifier ID")
    return machine


@dataclass(frozen=True)
class Profile:
    verifier: str
    revision: str
    fixture: str
    runtime: dict[str, str]
    limits: dict[str, object]

    @property
    def digest(self) -> str:
        """Use as a machine-relative listing digest and to partition measurement history."""
        data = (self.verifier, self.revision, self.fixture, self.runtime, self.limits)
        return hashlib.sha256(json.dumps(data, sort_keys=True).encode()).hexdigest()


def select(
    profiles: Mapping[str, object],
    fixture: str,
    observe: Callable[[], dict[str, str]],
) -> Profile:
    """Read the exact verifier's profile from a TOML or JSON mapping.

    Callers identify their fixture, observe their runtime and validate their own limits.
    """
    machine = verifier()
    entry = profiles.get(machine)
    if entry is None:
        raise SetupError(f"no calibration for verifier {machine} and fixture {fixture}")
    if not isinstance(entry, dict):
        raise SetupError(f"invalid calibration for verifier {machine}")
    revision, runtime, limits = (
        entry.get("revision"),
        entry.get("runtime"),
        entry.get("limits"),
    )
    if (
        not isinstance(revision, str)
        or not re.fullmatch(r"[0-9a-f]{40}", revision)
        or not isinstance(runtime, dict)
        or not runtime
        or not all(
            isinstance(k, str) and isinstance(v, str) and v for k, v in runtime.items()
        )
        or not isinstance(limits, dict)
    ):
        raise SetupError(f"invalid calibration for verifier {machine}")
    if entry.get("fixture") != fixture:
        raise SetupError(
            f"calibration for verifier {machine} does not cover fixture {fixture}"
        )
    live = observe()
    if runtime != live:
        changed = ", ".join(
            k
            for k in sorted(runtime.keys() | live.keys())
            if runtime.get(k) != live.get(k)
        )
        raise SetupError(
            f"incompatible calibration for verifier {machine}: {changed}; recalibrate {fixture}"
        )
    return Profile(machine, revision, fixture, live, limits)
