"""The server's restart when its code changes on disk, and the site it answers from until
the new code's first build."""

from __future__ import annotations

from pathlib import Path

from beryl_status.beads import Index, parse
from beryl_status.serve import CodeWatch
from beryl_status.site import Site

from tests.conftest import EXPORT


def test_restarts_once_new_code_settles_and_imports() -> None:
    watch = CodeWatch("old", lambda: True)
    assert watch.step("old") == ""
    # A checkout is still writing files: the first reading of new code waits.
    assert watch.step("new") == ""
    assert watch.step("new") == "restart"


def test_waits_while_the_code_keeps_changing() -> None:
    watch = CodeWatch("old", lambda: True)
    assert [watch.step(d) for d in ("a", "b", "c")] == ["", "", ""]


def test_skips_code_that_does_not_import_until_it_changes() -> None:
    works = False
    watch = CodeWatch("old", lambda: works)
    assert [watch.step("bad") for _ in range(4)] == ["", "broken", "", ""]
    works = True
    assert [watch.step("fixed"), watch.step("fixed")] == ["", "restart"]


def test_back_to_the_running_code_is_no_change() -> None:
    watch = CodeWatch("old", lambda: True)
    assert [watch.step(d) for d in ("new", "old", "old")] == ["", "", ""]


def test_serves_the_stand_in_until_its_own_build(tmp_path: Path) -> None:
    old = tmp_path / "old"
    site = Site(tmp_path, tmp_path / "new", standin=old)
    assert site.serving == old
    site.index = Index.build(parse(EXPORT))
    site.rebuild()
    assert site.serving == tmp_path / "new"
    assert (site.serving / "index.html").is_file()


def test_only_the_first_build_logs(tmp_path: Path) -> None:
    lines: list[str] = []
    site = Site(tmp_path, tmp_path / "site", log=lines.append)
    site.index = Index.build(parse(EXPORT))
    site.rebuild()
    site.rebuild()
    assert len(lines) == 1 and lines[0].startswith("status: built")
