"""Load the pinned Phase card data at authoring time."""

from __future__ import annotations

import hashlib
import json
import tempfile
import urllib.request
from functools import cache
from pathlib import Path
from typing import cast

from . import card_store, paths

PIN: Path = paths.REPO / "data" / "phase-data.json"
URL = "https://data.phase-rs.dev/card-data.json"


def read(pin: Path = PIN) -> dict[str, card_store.Json] | str:
    """Return the data or a short reason why the pinned release is unavailable."""
    try:
        return load(pin, paths.cache_root() / "phase" / "card-data.json")
    except (OSError, ValueError, KeyError) as error:
        return f"Phase data unavailable: {error}"


@cache
def load(pin: Path, destination: Path) -> dict[str, card_store.Json]:
    release = cast(dict[str, str], json.loads(pin.read_text()))
    expected = release["sha256"]
    download = not destination.is_file()
    if download:
        with urllib.request.urlopen(URL, timeout=15) as response:
            raw: bytes = response.read()
    else:
        raw = destination.read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError(
            f"{destination.name} differs from pinned Phase release {release['card_data_hash']}"
        )
    cards = cast(dict[str, card_store.Json], json.loads(raw))
    if download:
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(
            dir=destination.parent, delete=False
        ) as output:
            temporary = Path(output.name)
            try:
                output.write(raw)
                output.close()
                temporary.replace(destination)
            finally:
                temporary.unlink(missing_ok=True)
    return cards
