"""The registered verifier's native and Callgrind profiles for the frame gates."""

from __future__ import annotations

import json
import platform
import subprocess
from pathlib import Path
from typing import Literal

from .. import calibration, paths

PROFILES: Path = paths.REPO / "data" / "client" / "frame-calibration.json"
Mode = Literal["native", "instructions"]


def registered(mode: Mode) -> calibration.Profile:
    try:
        profiles = json.loads(PROFILES.read_text(encoding="utf-8"))[mode]
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise calibration.SetupError(
            f"could not read frame profiles: {error}"
        ) from error
    return calibration.registered(profiles, f"client-frame-{mode}/v1")


def native_runtime() -> dict[str, str]:
    """The host and pinned Rust compiler that build the native frame gate."""
    try:
        rustc = subprocess.check_output(
            ["rustc", "--version"], cwd=paths.REPO, text=True
        ).strip()
    except (OSError, subprocess.CalledProcessError) as error:
        raise calibration.SetupError(
            f"could not identify the native Rust compiler: {error}"
        ) from error
    return {
        "backend": "native",
        "os": platform.system(),
        "arch": platform.machine().lower(),
        "rustc": rustc,
    }


def native() -> calibration.Profile:
    profile = registered("native")
    calibration.require_runtime(profile, native_runtime())
    return profile
