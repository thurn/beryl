"""Reading the recordings pack without `b3d`, and finding the card names its recordings spell."""

from __future__ import annotations

import json
import zlib
from pathlib import Path

import pytest

from beryl_tools.beryl3d import pack


def varint(value: int) -> bytes:
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def string(data: bytes) -> bytes:
    return varint(len(data)) + data


def deflate(text: str) -> bytes:
    compressor = zlib.compressobj(6, zlib.DEFLATED, -15)
    return compressor.compress(text.encode()) + compressor.flush()


def encode(entries: list[tuple[str, int, str | None]]) -> bytes:
    """A pack as postcard writes `beryl_inspect::pack::Contents`; `None` is a recording that
    failed, written with why."""
    out = varint(pack.VERSION) + varint(len(entries))
    for name, seat, messages in entries:
        out += string(name.encode()) + bytes([seat]) + varint(300)
        if messages is None:
            out += varint(1) + string(b"it panicked")
        else:
            out += varint(0) + string(deflate(messages))
    return out


def test_a_pack_reads_back_each_recording_and_seat() -> None:
    long = json.dumps([{"name": "Island"}] * 200)
    data = encode(
        [("a/b", 0, '[{"name":"Counterspell"}]'), ("a/b", 1, long), ("c", 0, None)]
    )
    entries = pack.read(data)
    assert [(e.name, e.seat) for e in entries] == [("a/b", 0), ("a/b", 1), ("c", 0)]
    assert entries[0].messages() == '[{"name":"Counterspell"}]'
    assert entries[1].messages() == long
    assert entries[2].deflated is None and entries[2].messages() == ""


def test_a_pack_of_another_version_or_cut_short_is_refused() -> None:
    data = encode([("a", 0, "[]")])
    with pytest.raises(ValueError, match="version"):
        pack.read(varint(pack.VERSION + 1) + data[1:])
    with pytest.raises(ValueError, match="ends inside"):
        pack.read(data[:-2])


def test_a_name_counts_only_as_a_whole_string() -> None:
    text = '{"name":"Island","text":"Islandwalk","alt":"Fork"}'
    assert pack.names_in(text, ["Island", "Fork", "Counterspell"]) == {"Island", "Fork"}


def test_seen_names_are_kept_per_recording_and_reread_when_messages_change(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(pack.paths, "cache", lambda: tmp_path)
    names = ["Island", "Fork"]
    first = pack.read(
        encode([("a", 0, '["Island"]'), ("a", 1, '["Fork"]'), ("b", 0, "[]")])
    )
    assert pack.names_seen(first, names) == {"a": {"Island", "Fork"}, "b": set()}
    kept = json.loads((tmp_path / pack.SEEN_KEPT).read_text())
    assert sorted(kept.values()) == [[], ["Fork"], ["Island"]]
    # Changed messages are read again.
    second = pack.read(encode([("a", 0, '["Island", "Fork"]'), ("b", 0, '["Island"]')]))
    assert pack.names_seen(second, names) == {"a": {"Island", "Fork"}, "b": {"Island"}}
    # A new card name reads every recording again, since a kept answer never looked for it.
    assert pack.names_seen(first, [*names, "Counterspell"])["a"] == {"Island", "Fork"}
    # A kept answer is reused: change what one key says, and the recording reports it.
    key = next(k for k, v in kept.items() if v == ["Island"])
    kept[key] = ["Fork"]
    (tmp_path / pack.SEEN_KEPT).write_text(json.dumps(kept))
    assert pack.names_seen(first, names)["a"] == {"Fork"}
