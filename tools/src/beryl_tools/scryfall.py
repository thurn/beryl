"""Scryfall's bulk data files, downloaded into $BERYL_CACHE/scryfall and read a card at a time.

https://api.scryfall.com/bulk-data lists the files by type (`default_cards`, `oracle_cards`,
`rulings`, ...). Each is gzipped JSON Lines: one object per line, so a reader holds one card at
a time rather than the whole file (`default_cards` is about 500 MB unpacked). A downloaded file
is reused until it is a week old or `refresh` asks for a new one; Scryfall regenerates them
daily.

Scryfall's API guidelines, followed here: https://scryfall.com/docs/api
"""

from __future__ import annotations

import gzip
import json
import shutil
import sys
import time
import urllib.error
import urllib.request
from collections.abc import Callable, Iterator
from importlib import metadata
from pathlib import Path
from typing import cast

from . import paths

BULK = "https://api.scryfall.com/bulk-data"
# How long a downloaded bulk file serves before a run fetches a new one.
MAX_AGE = 7 * 24 * 3600
RETRIES = 5

Json = dict[str, object]
# Fetches a URL and returns the decoded JSON response.
Get = Callable[[str], Json]


def user_agent() -> str:
    return f"ProjectBeryl/{metadata.version('beryl-tools')}"


def request(url: str, accept: str = "application/json") -> urllib.request.Request:
    # Scryfall asks every client for a User-Agent naming it and an Accept header.
    return urllib.request.Request(
        url, headers={"User-Agent": user_agent(), "Accept": accept}
    )


def http_get(url: str) -> Json:
    """Fetches `url`, waiting and trying again while Scryfall says it is asked too often."""
    for attempt in range(RETRIES):
        try:
            with urllib.request.urlopen(request(url), timeout=60) as response:
                return cast(Json, json.load(response))
        except urllib.error.HTTPError as e:
            if e.code != 429 or attempt == RETRIES - 1:
                raise
            time.sleep(2.0 * (attempt + 1))
    raise AssertionError("unreachable")


def cache_dir() -> Path:
    path = paths.cache_root() / "scryfall"
    path.mkdir(parents=True, exist_ok=True)
    return path


def bulk_file(kind: str, refresh: bool = False) -> Path:
    """The bulk file of type `kind` (`default_cards`, `rulings`, ...) in the cache, downloaded
    when it is missing, a week old or `refresh` is set."""
    path = cache_dir() / f"{kind}.jsonl.gz"
    if not refresh and path.exists() and time.time() - path.stat().st_mtime < MAX_AGE:
        return path
    listing = cast(list[Json], http_get(BULK)["data"])
    entry = next((e for e in listing if e.get("type") == kind), None)
    if entry is None:
        raise LookupError(f"Scryfall has no bulk file of type {kind}")
    url = str(entry["jsonl_download_uri"])
    print(f"scryfall: downloading {url}", file=sys.stderr)
    partial = path.with_suffix(".partial")
    with urllib.request.urlopen(request(url, "*/*"), timeout=600) as response:
        with partial.open("wb") as out:
            shutil.copyfileobj(response, out, 1 << 20)
    partial.replace(path)
    return path


def objects(path: Path) -> Iterator[Json]:
    """The objects of a bulk file, one per line, gzipped (`.gz`) or not."""
    opened = (
        gzip.open(path, "rt", encoding="utf-8")
        if path.suffix == ".gz"
        else path.open(encoding="utf-8")
    )
    with opened as lines:
        for line in lines:
            if line.strip():
                yield cast(Json, json.loads(line))
