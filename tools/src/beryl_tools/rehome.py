"""Points a cloned `rust/target`'s records of absolute source paths at the checkout it now serves.

Cargo keeps, for each unit it built, the files the build read (the `dep-*` files in
`.fingerprint/`), and for each build script the files it asked to be rerun for (`build/*/output`).
A path inside the unit's own package is kept relative to it, so it holds in any checkout, but a
path outside it is kept absolute. Two such paths reach every workspace crate: clippy reads the
workspace's `rust/Cargo.toml` for each crate it checks, and `beryl_card_store`, which every rules
and card crate depends on, embeds `data/card-store.tsv` with `include_bytes!`. In a new worktree
those absolute paths name the checkout that built the cache, which is gone, and cargo counts a
missing file as changed: the fast check's clippy and test build recompiled the whole workspace
for a change to `beryl_session` alone (rust lane 141 s against its 60 s budget for the whole
check). `rehome` rewrites each such path to the same file in the new checkout, so cargo compares
that file's time, which `warm.restore_sources` has set, with the build it cloned.

A path is rewritten when it lies in a `rust/` folder of another checkout and the same path exists
in the new one; any other path (the toolchain, git's own files) is left alone, and a file that is
gone from the new checkout stays missing, so cargo rebuilds what read it, as it should.
Standard library only, like warm.py.
"""

from __future__ import annotations

import os
import struct
from contextlib import suppress
from pathlib import Path

# What a checkout's paths pass through: `<checkout>/rust/...`.
RUST = f"{os.sep}rust{os.sep}"
# The output lines of a build script that name a file cargo watches.
RERUN = ("cargo:rerun-if-changed=", "cargo::rerun-if-changed=")


def moved(path: str, checkout: Path) -> str | None:
    """`path` in `checkout`, when it is an absolute path into another checkout's `rust/` folder
    whose file or folder `checkout` also has; None otherwise."""
    root, found, rest = path.partition(RUST)
    if not found or not os.path.isabs(path) or Path(root) == checkout:
        return None
    new = f"{checkout}{RUST}{rest}"
    return new if os.path.exists(new) else None


def rehome_dep_info(data: bytes, checkout: Path) -> bytes | None:
    """`data`, a cargo `dep-*` fingerprint file, with each moved path rewritten; None when no
    path moved. Cargo writes each path as a little-endian u32 byte count and then the bytes;
    a candidate counts as a path only when the count before it matches, so the rest of the file
    is copied unread."""
    out = bytearray()
    start = 0
    changed = False
    # A POSIX path starts with its slash; a Windows path a byte before its drive's colon.
    marker, back = (b"/", 0) if os.sep == "/" else (b":\\", 1)
    i = data.find(marker)
    while i >= 0:
        s = i - back
        if s >= start + 4:
            (count,) = struct.unpack_from("<I", data, s - 4)
            if s + count <= len(data):
                with suppress(UnicodeDecodeError):
                    new = moved(data[s : s + count].decode(), checkout)
                    if new is not None:
                        encoded = new.encode()
                        out += data[start : s - 4]
                        out += struct.pack("<I", len(encoded)) + encoded
                        start = s + count
                        changed = True
                        i = data.find(marker, start)
                        continue
        i = data.find(marker, i + 1)
    if not changed:
        return None
    return bytes(out + data[start:])


def rehome_output(text: str, checkout: Path) -> str | None:
    """`text`, a build script's output, with each watched file that moved rewritten; None when
    none moved."""
    lines = text.split("\n")
    changed = False
    for n, line in enumerate(lines):
        for prefix in RERUN:
            if line.startswith(prefix):
                new = moved(line[len(prefix) :], checkout)
                if new is not None:
                    lines[n] = prefix + new
                    changed = True
    return "\n".join(lines) if changed else None


def rewrite(path: Path, data: bytes) -> None:
    """Writes `data` to `path` and keeps its modification time: cargo compares each source
    file's time with its unit's `dep-*` file's."""
    stat = path.stat()
    path.write_bytes(data)
    os.utime(path, ns=(stat.st_atime_ns, stat.st_mtime_ns))


def rehome(target: Path, checkout: Path) -> int:
    """Rewrites the absolute source paths in `target`, freshly cloned into `checkout`, that name
    another checkout; returns how many files it changed."""
    count = 0
    profiles = [*target.glob("*/"), *target.glob("tauri/*/")]
    for profile in profiles:
        for dep in profile.glob(".fingerprint/*/dep-*"):
            with suppress(OSError):
                data = rehome_dep_info(dep.read_bytes(), checkout)
                if data is not None:
                    rewrite(dep, data)
                    count += 1
        for output in profile.glob("build/*/output"):
            with suppress(OSError, UnicodeDecodeError):
                text = rehome_output(output.read_bytes().decode(), checkout)
                if text is not None:
                    rewrite(output, text.encode())
                    count += 1
    return count
