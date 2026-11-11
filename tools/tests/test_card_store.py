import gzip
import io
import json
import os
import time
from pathlib import Path
from typing import cast

import pytest

from beryl_tools import card_store, scryfall


def entry(kind: card_store.Kind, key: str, **value: object) -> bytes:
    return card_store.line(kind, key, dict(value))


def store_of(tmp_path: Path, entries: list[bytes]) -> Path:
    path = tmp_path / "store.tsv"
    card_store.write(path, entries)
    return path


def test_every_key_is_found_by_seeking(tmp_path: Path) -> None:
    # Names that sort as prefixes of one another, non-ASCII names, and a token name that
    # several records share.
    names = ["A", "A B", "Ab", "Æther", "Zz", *(f"Card {i:03}" for i in range(300))]
    entries = [entry("card", n, name=n, scryfall_id=n) for n in names]
    entries += [entry("token", "A", name="A", scryfall_id=f"t{i}") for i in range(3)]
    path = store_of(tmp_path, entries)
    lines = path.read_bytes().splitlines()
    assert lines == sorted(lines)
    with card_store.Store(path) as store:
        for name in names:
            assert store.card(name) == {"name": name, "scryfall_id": name}
        assert [t["scryfall_id"] for t in store.tokens("A")] == [
            "t0",
            "t1",
            "t2",
        ]
        for missing in ("", "0", "A ", "Card 0005", "Zzz", "\U0010ffff"):
            assert store.card(missing) is None
        assert store.tokens("Ab") == []
        assert len(list(store)) == len(names) + 3


def test_an_alias_answers_with_its_record(tmp_path: Path) -> None:
    path = store_of(
        tmp_path,
        [
            entry("card", "Fire // Ice", name="Fire // Ice", scryfall_id="f"),
            entry("card", "Fire", alias="Fire // Ice", scryfall_id="f"),
            entry("token", "X // Y", name="X // Y", scryfall_id="1"),
            entry("token", "X // Y", name="X // Y", scryfall_id="2"),
            entry("token", "X", alias="X // Y", scryfall_id="2"),
        ],
    )
    with card_store.Store(path) as store:
        assert store.card("Fire") == store.card("Fire // Ice")
        assert [t["scryfall_id"] for t in store.tokens("X")] == ["2"]
        # Iteration skips aliases.
        assert [r["scryfall_id"] for _, r in store] == ["f", "1", "2"]


def test_the_committed_store_is_sorted_and_its_links_resolve() -> None:
    """What every reader's seek relies on, of data/card-store.tsv itself: a hand edit or a merge
    of two refreshes can break it where no lookup of an implemented card would notice.
    """
    keys: list[bytes] = []
    values: dict[bytes, list[card_store.Json]] = {}
    with card_store.STORE.open("rb") as lines:
        for raw in lines:
            key, tab, value = raw.partition(b"\t")
            assert tab and raw.endswith(b"\n"), raw[:80]
            keys.append(key)
            values.setdefault(key, []).append(json.loads(value))
    assert keys == sorted(keys)

    def records(kind: bytes, name: object) -> list[card_store.Json]:
        found = values.get(kind + b":" + str(name).encode(), [])
        return [v for v in found if "alias" not in v]

    for key, found in values.items():
        kind, _, name = key.partition(b":")
        if kind == b"card" and records(kind, name.decode()):
            # A card's key is its alone: no second card, and no alias beside it.
            assert len(found) == 1, key
        for value in found:
            if "alias" in value:
                targets = records(kind, value["alias"])
                assert [t for t in targets if t["scryfall_id"] == value["scryfall_id"]]
            for link in cast(list[card_store.Json], value.get("tokens", [])):
                made = records(b"token", link["name"])
                assert [t for t in made if t["scryfall_id"] == link["scryfall_id"]], key


def test_a_fresh_bulk_file_is_reused_without_asking_scryfall(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path))

    def offline(url: str) -> scryfall.Json:
        raise AssertionError(f"asked {url}")

    monkeypatch.setattr(scryfall, "http_get", offline)
    cached = tmp_path / "scryfall" / "rulings.jsonl.gz"
    cached.parent.mkdir()
    with gzip.open(cached, "wt") as f:
        f.write('{"a": 1}\n\n{"a": 2}\n')
    assert scryfall.bulk_file("rulings") == cached
    assert [o["a"] for o in scryfall.objects(cached)] == [1, 2]


def test_a_stale_bulk_file_is_downloaded_again(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path))
    cached = tmp_path / "scryfall" / "rulings.jsonl.gz"
    cached.parent.mkdir()
    cached.write_bytes(b"old")
    week_ago = time.time() - scryfall.MAX_AGE - 1
    os.utime(cached, (week_ago, week_ago))
    listing = {
        "data": [
            {"type": "default_cards", "jsonl_download_uri": "https://d/default"},
            {"type": "rulings", "jsonl_download_uri": "https://d/rulings"},
        ]
    }
    monkeypatch.setattr(scryfall, "http_get", lambda url: listing)
    asked: list[str] = []

    def urlopen(request: object, timeout: float) -> io.BytesIO:
        asked.append(request.full_url)  # type: ignore[attr-defined]
        return io.BytesIO(gzip.compress(json.dumps({"a": 3}).encode()))

    monkeypatch.setattr(scryfall.urllib.request, "urlopen", urlopen)
    assert scryfall.bulk_file("rulings") == cached
    assert asked == ["https://d/rulings"]
    assert [o["a"] for o in scryfall.objects(cached)] == [3]
    with pytest.raises(LookupError, match="oracle_cards"):
        scryfall.bulk_file("oracle_cards", refresh=True)
