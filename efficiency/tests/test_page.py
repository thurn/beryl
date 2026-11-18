"""The page: every page and section is well-formed HTML for the synthetic store and for an
empty one, every number it shows is a number of the query's JSON, and the server answers
`GET` only, on 127.0.0.1 only."""

from __future__ import annotations

import http.client
import json
import threading
from collections.abc import Iterator
from html.parser import HTMLParser
from pathlib import Path

import pytest

from beryl_efficiency import changes, cli, paths, serve, store
from beryl_efficiency.changes import Change
from tests.test_queries import NUMBER, numbers_in, root  # noqa: F401, the fixture

# Elements that have no end tag.
VOID: frozenset[str] = frozenset({"meta", "br", "img", "input", "link", "hr", "circle"})
# Elements whose text is not a figure: not shown, or the range control.
HIDDEN: frozenset[str] = frozenset({"style", "script", "header"})


class Checked(HTMLParser):
    """Parses a page, failing on an end tag that closes anything but the last open element,
    and collects the text a reader sees."""

    def __init__(self) -> None:
        super().__init__()
        self.open: list[str] = []
        self.shown: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag not in VOID:
            self.open.append(tag)

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        pass

    def handle_endtag(self, tag: str) -> None:
        if tag in VOID:
            return
        assert self.open and self.open[-1] == tag, (tag, self.open)
        self.open.pop()

    def handle_data(self, data: str) -> None:
        if not (set(self.open) & HIDDEN):
            self.shown.append(data)


def checked(html: str) -> str:
    """The text a reader sees in `html`, after checking that it is well-formed."""
    parser = Checked()
    parser.feed(html)
    parser.close()
    assert parser.open == [], parser.open
    return " ".join(parser.shown)


# Efficiency changes on the per-ticket dollars and on requests' context, made between the
# fixture's old ticket and its recent ones, so that both trends over all time carry a mark.
LABEL = "Sessions read smaller logs"
CONTEXT_LABEL = "Requests carry less context"
SHA = "c" * 40
COMMITTED = "2026-11-10T12:00:00Z"


@pytest.fixture(autouse=True)
def changes_file(monkeypatch: pytest.MonkeyPatch) -> None:
    """The changes file is this test's, not the project's, whose labels carry digits."""
    listed = [
        Change(LABEL, "bead.dollars", "down", SHA),
        Change(CONTEXT_LABEL, "request.context-tokens", "down", SHA),
        Change(
            "Fewer agent minutes for sonnet",
            "bead.agent-minutes[tier=sonnet]",
            "down",
            None,
        ),
    ]
    monkeypatch.setattr(changes, "load", lambda path=None: listed)
    monkeypatch.setattr(changes, "file_versions", lambda checkout, ref="staging": [])


@pytest.fixture
def marked(root: Path) -> Path:
    """The fixture's store with the change's commit on staging."""
    db = store.open_store(paths.store())
    db.execute(
        "INSERT INTO commits (sha, time, subject) VALUES (?, ?, 'perf: smaller logs')",
        (SHA, COMMITTED),
    )
    db.commit()
    db.close()
    return root


def get(path: str) -> serve.Answer:
    path, _, query = path.partition("?")
    return serve.respond(path, query)


ADDRESSES: list[str] = [
    "/",
    "/?since=all",
    "/?since=24h",
    "/?since=all&tier=opus",
    "/epic/by-e",
    "/epic/by-e?since=all",
    "/bead/by-e.1",
    "/bead/by-e.2",
    "/bead/by-far",
    "/session/t1",
    "/session/t1?bead=by-e.2",
    "/cards",
    "/cards?since=all&by=role",
    "/machine",
    "/ci",
    "/ci?script=check&since=all",
    "/ci?lane=rust",
    "/pushes",
    "/failures",
    "/changes",
    "/findings?since=all",
    "/findings?rule=waiting-on-person&since=all",
    "/metrics",
    "/metrics/bead.dollars?since=all",
    "/metrics/request.context-tokens?since=all",
]
# The five views the page's header leads to, and the sections they open.
VIEWS: list[str] = [
    "/cards",
    "/machine",
    "/ci",
    "/changes",
    "/findings",
    "/pushes",
    "/failures",
    "/metrics",
    "/metrics/bead.dollars",
]


def with_section(path: str) -> str:
    return path + ("&" if "?" in path else "?") + "section=1"


@pytest.mark.parametrize("path", ADDRESSES)
def test_every_number_on_a_page_is_a_number_of_its_json(
    marked: Path, path: str
) -> None:
    as_json = path.replace("?", ".json?", 1) if "?" in path else path + ".json"
    found = json.loads(get(as_json).body)
    have = numbers_in(found)
    for address in (path, with_section(path)):
        answer = get(address)
        assert answer.status == 200 and answer.kind.startswith("text/html")
        shown = checked(answer.body.decode())
        for token in NUMBER.findall(shown):
            assert any(abs(float(token) - n) < 1e-9 for n in have), (token, address)


def test_a_sections_address_returns_only_that_section(root: Path) -> None:
    whole = get("/epic/by-e").body.decode()
    alone = get("/epic/by-e?section=1").body.decode()
    assert whole.startswith("<!doctype html>") and "<header>" in whole
    assert alone.startswith('<section class="q-epic">') and alone.endswith("</section>")
    assert "<header>" not in alone and "<script>" not in alone
    # Each epic line of the summary opens its epic's section in the summary's range.
    summary = get("/?since=all").body.decode()
    assert 'data-src="/epic/by-e?since=all&amp;section=1"' in summary
    assert (
        'data-command="just efficiency epic by-e --since all"'
        in get("/epic/by-e?since=all&section=1").body.decode()
    )


def test_the_json_address_is_the_commands_json(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    assert cli.main(["bead", "by-e.1", "--no-ingest", "--json"]) == 0
    printed = json.loads(capsys.readouterr().out)
    assert json.loads(get("/bead/by-e.1.json").body) == printed


def test_the_bead_strip_is_as_long_as_its_wall_minutes(root: Path) -> None:
    found = json.loads(get("/bead/by-e.1.json").body)
    strip = found["strip"]
    assert strip and strip[0]["at"] == 0
    assert sum(s["minutes"] for s in strip) == pytest.approx(
        found["wall_minutes"], abs=0.05
    )
    names = {s["name"] for s in strip}
    assert "model" in names and any("test" in n or "cargo" in n for n in names), names
    # One mark per request of the ticket, in order.
    contexts = found["contexts"]
    assert len(contexts) == found["requests"]
    assert [c["at"] for c in contexts] == sorted(c["at"] for c in contexts)


def test_the_trends_are_medians_by_day_or_hour(root: Path) -> None:
    by_day = json.loads(get("/.json?since=all").body)["dollars"]["trend"]
    assert [(p["at"][:10], p["count"]) for p in by_day][-1][1] == 2
    by_hour = json.loads(get("/.json?since=24h").body)
    assert all(len(p["at"]) == 16 for p in by_hour["dollars"]["trend"])


@pytest.mark.parametrize(
    "path",
    ["/", "/?since=all", "/epic/by-none", "/bead/by-none", "/?section=1", *VIEWS],
)
def test_the_empty_store_renders_every_page(path: str) -> None:
    answer = get(path)
    assert answer.status == 200
    checked(answer.body.decode())


def test_bad_addresses_are_refused(root: Path) -> None:
    for path, status in (
        ("/ingest", 404),
        ("/sql", 404),
        ("/nothing", 404),
        ("/session/none", 404),
        ("/?since=never", 400),
        ("/?drop=1", 400),
        ("/time", 404),  # No page for it yet; its JSON is served.
        ("/findings?rule=none", 400),
    ):
        with pytest.raises(serve.Refused) as e:
            get(path)
        assert e.value.status == status, path
    assert get("/time.json?by=tier").status == 200


@pytest.fixture
def running(root: Path) -> Iterator[serve.HTTPServer]:
    found = serve.server(0, no_ingest=True)
    thread = threading.Thread(target=found.serve_forever, daemon=True)
    thread.start()
    yield found
    found.shutdown()
    found.server_close()


def fetch(
    found: serve.HTTPServer, method: str, path: str, host: str | None = None
) -> tuple[int, bytes]:
    connection = http.client.HTTPConnection("127.0.0.1", found.server_address[1])
    headers = {"Host": host} if host else {}
    connection.request(method, path, headers=headers)
    response = connection.getresponse()
    body = response.read()
    connection.close()
    return response.status, body


def test_the_server_answers_get_only_on_the_loopback(
    running: serve.HTTPServer,
) -> None:
    assert running.server_address[0] == "127.0.0.1"
    status, body = fetch(running, "GET", "/bead/by-e.1")
    assert status == 200 and b"by-e.1" in body
    for method in ("POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"):
        assert fetch(running, method, "/")[0] == 405, method
    # A page on another site reaching this port through its own name is refused.
    assert fetch(running, "GET", "/", host="attacker.example:80")[0] == 403
    assert fetch(running, "GET", "/nothing")[0] == 404


def test_a_trend_with_a_change_on_it_carries_the_changes_mark(marked: Path) -> None:
    """The summary's dollars trend and the context metric's trend mark their changes with
    the label and medians; a trend with no change on it has no mark."""
    found = json.loads(get("/.json?since=all").body)
    marks = found["dollars"]["marks"]
    assert [m["label"] for m in marks] == [LABEL] and marks[0]["time"] == COMMITTED
    assert found["agent_minutes"]["marks"][0]["time"] is None  # Not on staging yet.
    for address, label in (
        ("/?since=all", LABEL),
        ("/metrics/request.context-tokens?since=all&section=1", CONTEXT_LABEL),
    ):
        shown = get(address).body.decode()
        assert shown.count('class="mark"') == 1, address
        assert f"<title>{label}: " in shown, address
    assert (
        'class="mark"' not in get("/metrics/bead.agent-minutes?since=all").body.decode()
    )


def test_every_view_leads_to_the_sections_it_opens(marked: Path) -> None:
    """The header leads to the five views; a view's metrics are trends open from the start,
    a change opens its metric's trend, a finding its items, and a check lane its families.
    """
    home = get("/").body.decode()
    for name in ("cards", "machine", "ci", "changes", "findings"):
        assert f'href="/{name}?since=7d"' in home or f'href="/{name}"' in home, name
    cards = get("/cards").body.decode()
    assert (
        '<details class="line" open data-src="/metrics/card.dollars?since=7d&amp;section=1">'
        in cards
    )
    changed = get("/changes").body.decode()
    assert 'data-src="/metrics/bead.dollars?since=all&amp;section=1"' in changed
    assert (
        'data-src="/metrics/bead.agent-minutes?since=all&amp;where=tier%3Asonnet'
        in changed
    )
    ci = get("/ci?since=all").body.decode()
    assert 'data-src="/pushes?since=all&amp;section=1"' in ci
    assert 'data-src="/failures?since=all&amp;section=1"' in ci


def test_an_answer_is_kept_until_the_next_ingest(root: Path) -> None:
    """The server computes an address once per ingest: asking again answers from memory,
    and an ingest landing makes `warm` compute every address asked for again."""
    answers = serve.Answers()
    computed: list[str] = []

    def compute() -> serve.Answer:
        computed.append("/")
        return serve.answer_of("/", "")

    first = answers.get(("/", ""), compute)
    assert answers.get(("/", ""), compute) is first and computed == ["/"]
    answers.warm()  # Nothing new: nothing computed.
    assert computed == ["/"]
    db = store.open_store(paths.store())
    db.execute(
        "INSERT INTO ingests (started, host, wall_seconds, cpu_seconds, bytes_read, "
        "rows, skipped) VALUES ('2026-11-15T20:00:00+00:00', 'here', 1, 1, 0, 0, 0)"
    )
    db.commit()
    db.close()
    answers.warm()
    assert len(answers.found) == 1 and answers.found[("/", "")] is not first
    assert answers.get(("/", ""), compute) is answers.found[("/", "")]


def test_the_status_says_when_the_store_was_updated(running: serve.HTTPServer) -> None:
    status, body = fetch(running, "GET", "/status.json")
    found = json.loads(body)
    assert status == 200 and found["updating"] is False
    page_version = fetch(running, "GET", "/")[1].decode()
    assert f'data-version="{"" if found["version"] is None else found["version"]}"' in (
        page_version
    )
