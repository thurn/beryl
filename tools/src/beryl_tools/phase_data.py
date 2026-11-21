"""Load the pinned Phase card data at authoring time."""

from __future__ import annotations

import gzip
import hashlib
import json
import tempfile
import urllib.request
import zlib
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
    return cast(
        dict[str, card_store.Json],
        pinned_json(URL, destination, release["sha256"], release["card_data_hash"]),
    )


def read_gaps(pin: Path = PIN) -> set[str] | str:
    """Names Phase reports a gap for in the same release as the drafter input."""
    try:
        release = cast(dict[str, str], json.loads(pin.read_text()))
        card_data_hash = release["card_data_hash"]
        archive = pin.with_name(f"phase-coverage-{card_data_hash}.json.gz")
        coverage = pinned_json(
            archive,
            paths.cache_root() / "phase" / "coverage-data.json",
            release["coverage_sha256"],
            card_data_hash,
        )
        if coverage["card_data_hash"] != card_data_hash:
            raise ValueError("coverage and card data releases differ")
        return {
            str(row["card_name"]).casefold()
            for row in cast(list[card_store.Json], coverage["cards"])
            if row["supported"] is False or row["gap_count"] != 0
        }
    except (OSError, ValueError, KeyError, EOFError, zlib.error) as error:
        return f"Phase coverage unavailable: {error}"


def pinned_json(
    source: str | Path, destination: Path, expected: str, release: str
) -> card_store.Json:
    """Verify and cache JSON from a URL or a local gzip archive."""
    cached = destination.is_file()
    if cached:
        raw: bytes = destination.read_bytes()
    elif isinstance(source, Path):
        with gzip.open(source, "rb") as archive:
            raw = archive.read()
    else:
        with urllib.request.urlopen(source, timeout=15) as response:
            raw = response.read()
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError(
            f"{destination.name} differs from pinned Phase release {release}"
        )
    data = cast(card_store.Json, json.loads(raw))
    if not cached:
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
    return data
