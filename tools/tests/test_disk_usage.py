"""The disk report counts the live paths and physical extents once."""

from pathlib import Path
import subprocess
import sys

import pytest

from beryl_tools import disk_usage as disk


def test_git_worktree_list_does_not_repeat_the_main_checkout(tmp_path: Path) -> None:
    checkout = tmp_path / "beryl"
    managed = checkout / ".worktrees"
    managed_tree = managed / "tree-01"
    legacy = tmp_path / "beryl-fixes"
    linked = legacy / "old-fix"
    for path in (managed_tree, linked, checkout / "rust"):
        path.mkdir(parents=True)

    roots = disk.disjoint(
        disk.checkout_roots(
            checkout, managed, [checkout, managed_tree, linked], tmp_path
        )
    )

    assert {(root.area, root.path) for root in roots} == {
        (disk.MAIN, (checkout / "rust").resolve()),
        (disk.WORKTREES, managed_tree.resolve()),
        (disk.CHECKOUTS, legacy.resolve()),
    }


def test_edited_clones_share_blocks_and_hard_links_share_an_inode(
    tmp_path: Path,
) -> None:
    if sys.platform != "darwin":
        pytest.skip("needs APFS physical block maps")
    original = tmp_path / "original"
    clone = tmp_path / "clone"
    link = tmp_path / "hard-link"
    original.write_bytes(b"a" * (1024 * 1024))
    subprocess.run(["cp", "-c", str(original), str(clone)], check=True)
    with clone.open("r+b") as file:
        file.seek(128 * 1024)
        file.write(b"b" * 4096)
    link.hardlink_to(original)

    report = disk.measure(
        [
            disk.Root("original", "original", original),
            disk.Root("clone", "clone", clone),
            disk.Root("clone", "hard link", link),
        ]
    )

    allocation = original.stat().st_blocks * 512
    assert report.total.unmapped == report.total.unreadable == 0
    assert report.total.allocated == allocation + clone.stat().st_blocks * 512
    assert allocation < report.total.physical < report.total.allocated
    assert report.areas["clone"].physical == report.total.physical
    assert report.areas["original"].physical == allocation
    assert report.areas["original"].exclusive == 0
    assert report.areas["clone"].exclusive == report.total.physical - allocation
