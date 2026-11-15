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

from beryl_efficiency import cli, serve
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


def get(path: str) -> serve.Answer:
    path, _, query = path.partition("?")
    return serve.respond(path, query, no_ingest=True)


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
]


def with_section(path: str) -> str:
    return path + ("&" if "?" in path else "?") + "section=1"


@pytest.mark.parametrize("path", ADDRESSES)
def test_every_number_on_a_page_is_a_number_of_its_json(root: Path, path: str) -> None:
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
    # One mark per request of the bead, in order.
    contexts = found["contexts"]
    assert len(contexts) == found["requests"]
    assert [c["at"] for c in contexts] == sorted(c["at"] for c in contexts)


def test_the_trends_are_medians_by_day_or_hour(root: Path) -> None:
    by_day = json.loads(get("/.json?since=all").body)["dollars"]["trend"]
    assert [(p["at"][:10], p["count"]) for p in by_day][-1][1] == 2
    by_hour = json.loads(get("/.json?since=24h").body)
    assert all(len(p["at"]) == 16 for p in by_hour["dollars"]["trend"])


@pytest.mark.parametrize(
    "path", ["/", "/?since=all", "/epic/by-none", "/bead/by-none", "/?section=1"]
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
