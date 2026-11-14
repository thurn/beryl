import os
import struct
import time
from pathlib import Path

import pytest

from beryl_tools import rehome, system, warm

# The fixtures write POSIX paths, which `rehome` looks for only off Windows.
pytestmark: pytest.MarkDecorator = pytest.mark.skipif(
    system.WINDOWS, reason="the fixtures write POSIX paths"
)


def encoded(path: str) -> bytes:
    """One path entry as cargo writes it in a `dep-*` file: a kind byte, then the u32 count
    and the bytes, then a byte for its missing checksum."""
    data = path.encode()
    return b"\x01" + struct.pack("<I", len(data)) + data + b"\x00"


def dep_info(*paths: str) -> bytes:
    header = b"\x01\x00\x00\x00\xff\x01" + struct.pack("<I", len(paths))
    return header + b"".join(encoded(p) for p in paths) + b"\x00\x00\x00\x00"


def test_paths_into_another_checkout_point_at_the_new_one(tmp_path: Path) -> None:
    new = tmp_path / "new"
    (new / "rust" / "crates" / "store" / "src").mkdir(parents=True)
    (new / "rust" / "Cargo.toml").write_text("[workspace]")
    (new / "data").mkdir()
    (new / "data" / "card-store.tsv").write_text("cards")
    old = tmp_path / "gone-checkout-with-a-longer-name"
    data = dep_info(
        "src/lib.rs",
        f"{old}/rust/Cargo.toml",
        f"{old}/rust/crates/store/src/../../../../data/card-store.tsv",
        f"{old}/rust/crates/store/src/deleted.rs",
        f"{tmp_path}/.git/worktrees/gone/HEAD",
    )

    assert rehome.rehome_dep_info(data, new) == dep_info(
        "src/lib.rs",
        f"{new}/rust/Cargo.toml",
        f"{new}/rust/crates/store/src/../../../../data/card-store.tsv",
        # A file the new checkout lacks stays missing, so cargo rebuilds what read it.
        f"{old}/rust/crates/store/src/deleted.rs",
        f"{tmp_path}/.git/worktrees/gone/HEAD",
    )
    assert rehome.rehome_dep_info(dep_info("src/lib.rs"), new) is None


def test_a_build_scripts_watched_files_point_at_the_new_checkout(
    tmp_path: Path,
) -> None:
    new = tmp_path / "new"
    (new / "rust" / "app").mkdir(parents=True)
    (new / "rust" / "app" / "app.json").write_text("{}")
    output = "\n".join(
        [
            f"cargo:rerun-if-changed={tmp_path}/old/rust/app/app.json",
            "cargo:rustc-env=COMMIT=abc",
            "",
        ]
    )
    assert rehome.rehome_output(output, new) == output.replace(
        f"{tmp_path}/old", str(new)
    )


def test_a_clone_points_cargos_records_at_its_checkout_and_keeps_their_times(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    first = tmp_path / "first"
    (first / "ui").mkdir(parents=True)
    (first / "ui" / "package-lock.json").write_text("lock")
    dep = Path("rust/target/debug/.fingerprint/store-0123456789abcdef/dep-lib-store")
    (first / dep).parent.mkdir(parents=True)
    (first / dep).write_bytes(dep_info(f"{first}/rust/Cargo.toml"))
    built = time.time() - 3600
    os.utime(first / dep, (built, built))
    warm.save(first)

    second = tmp_path / "second"
    (second / "ui" / "node_modules").mkdir(parents=True)
    (second / "ui" / "package-lock.json").write_text("lock")
    (second / "ui" / "node_modules" / warm.STAMP).write_text(warm.lock_key(second))
    (second / "rust").mkdir()
    (second / "rust" / "Cargo.toml").write_text("[workspace]")
    assert warm.ensure(second) is False

    assert (second / dep).read_bytes() == dep_info(f"{second}/rust/Cargo.toml")
    assert (second / dep).stat().st_mtime == pytest.approx(built)
