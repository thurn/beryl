"""Building the site: the pages it writes, what they show, and that a rebuild rewrites only
what changed."""

from __future__ import annotations

from pathlib import Path

from beryl_status import build, changelog
from beryl_status.beads import Bead, Index, parse
from beryl_status.changelog import Changelog, Event
from beryl_status.serve import resolve

from tests.conftest import EXPORT, record


def nothing(bead: Bead) -> tuple[bool, Changelog | None]:
    return False, None


def made(tmp_path: Path, lines: list[str] = EXPORT) -> tuple[Path, Index]:
    site = tmp_path / "site"
    found = Index.build(parse(lines))
    build.build(site, found, "v1", nothing)
    return site, found


def test_writes_every_page(tmp_path: Path) -> None:
    site, _ = made(tmp_path)
    assert (site / "index.html").is_file()
    assert (site / "beads" / "by-epic.1.html").is_file()
    assert (site / "sw.js").is_file() and (site / ".nojekyll").is_file()
    assert len(list((site / "assets").glob("cirrus.*.css"))) == 1
    assert '"version": "v1"' in (site / "version.json").read_text()


def test_home_page_shows_each_category(tmp_path: Path) -> None:
    home = (made(tmp_path)[0] / "index.html").read_text()
    for anchor in ("epics", "beads", "automatic", "bug-reports", "deferred"):
        assert f'id="{anchor}"' in home and f'href="#{anchor}"' in home
    for bead in ("by-free", "by-epic", "by-ci", "by-bug", "by-later"):
        assert f'href="beads/{bead}"' in home
    assert "Flaky checks" in home
    assert "by-epic.1" not in home and "by-old" not in home
    assert "1/3" in home  # The epic's closed children over all of them.


def test_bead_page_shows_every_part(tmp_path: Path) -> None:
    page = (made(tmp_path)[0] / "beads" / "by-epic.1.html").read_text()
    assert "First child" in page and "In progress" in page
    assert "session ff7437" in page  # The assignee, shortened.
    assert 'href="by-epic"' in page  # Its epic, in the rail and the breadcrumbs.
    assert "<strong>" in page and "&lt;b&gt;tags" in page  # Markdown, escaped.
    assert 'data-hovercard="hc-by-free"' in page  # A mention, with a hovercard.
    assert "A <em>note</em>" in page  # The comment.
    assert 'data-include="../changelog/by-epic.1.html"' in page  # No current changelog.


def test_blockers_and_subtasks(tmp_path: Path) -> None:
    site, _ = made(tmp_path)
    blocked = (site / "beads" / "by-epic.2.html").read_text()
    assert "Blocked by" in blocked and 'href="by-free"' in blocked
    assert "Close reason" in blocked and "Done in abc123" in blocked
    blocking = (site / "beads" / "by-free.html").read_text()
    assert "Blocking" in blocking
    epic = (site / "beads" / "by-epic.html").read_text()
    assert "Subtasks" in epic and "1 closed" in epic


def test_a_current_changelog_is_inlined(tmp_path: Path) -> None:
    found = Index.build(parse(EXPORT))
    bead = found.beads["by-free"]
    log = Changelog(
        bead.updated_at,
        (
            Event(
                "2026-11-02T00:00:00Z", "Someone", "updated", "", (("priority", "0"),)
            ),
            Event("2026-11-01T12:00:00Z", "Enact", "updated"),
            Event("2026-11-01T11:00:00Z", "Enact", "updated"),
            Event("2026-11-01T10:00:00Z", "Someone", "created"),
        ),
    )
    site = tmp_path / "site"
    build.build(
        site,
        found,
        "v1",
        lambda b: (b.id == "by-free", log if b.id == "by-free" else None),
    )
    page = (site / "beads" / "by-free.html").read_text()
    assert "set priority to" in page and "created this" in page
    assert "2 bookkeeping updates by Enact" in page
    assert "data-include" not in page
    assert (site / "changelog" / "by-free.html").is_file()


def test_a_rebuild_rewrites_only_what_changed(tmp_path: Path) -> None:
    site = tmp_path / "site"
    cache = build.PageCache()
    found = Index.build(parse(EXPORT))
    build.build(site, found, "v1", nothing, cache=cache)
    changed = [
        (
            record("by-free", title="Renamed", priority=0, labels=["tier:opus"])
            if line.startswith('{"_type": "issue", "id": "by-free"')
            else line
        )
        for line in EXPORT
    ]
    again = build.build(site, Index.build(parse(changed)), "v2", nothing, cache=cache)
    # The bead itself, the bead it blocks and the bead that mentions it (which show its
    # title in a row and a hovercard), the home page and version.json.
    assert again.written == 5
    assert "Renamed" in (site / "beads" / "by-epic.2.html").read_text()


def test_a_deleted_bead_loses_its_page(tmp_path: Path) -> None:
    site, _ = made(tmp_path)
    build.build(site, Index.build(parse(EXPORT[:-2])), "v2", nothing)
    assert not (site / "beads" / "by-old.html").exists()


def test_addresses_resolve_like_github_pages(tmp_path: Path) -> None:
    site, _ = made(tmp_path)
    assert resolve(site, "/") == site / "index.html"
    assert resolve(site, "/beads/by-free") == site / "beads" / "by-free.html"
    assert resolve(site, "/beads/by-free?x=1") == site / "beads" / "by-free.html"
    assert resolve(site, "/../secret") is None
    assert resolve(site, "/beads/nothing") is None


def test_changelog_events_parse_and_round_trip() -> None:
    output = (
        '[{"event_type":"updated","actor":"a","created_at":"2026-11-02T00:00:00Z",'
        '"old_value":"{}","new_value":"{\\"priority\\":0,\\"metadata\\":{}}"},'
        '{"event_type":"label_added","actor":"a","created_at":"2026-11-01T00:00:00Z",'
        '"comment":"Added label: tier:opus"}]'
    )
    log = changelog.parse(output, "u")
    assert [e.kind for e in log.events] == ["updated", "label_added"]
    assert log.events[0].fields == (("priority", "0"),)
    assert changelog.from_json(changelog.to_json(log)) == log
    assert not changelog.parse("null", "u").recorded
