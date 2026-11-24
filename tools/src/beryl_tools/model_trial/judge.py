"""`model-trial judge`: the user's blind page. It serves, on 127.0.0.1 only, the spec's
`judge_pairs` pairs drawn at random with a recorded seed and spread across the kinds. Each
pair shows the ticket text both arms read, the two diffs side by side and each side's images,
as A and B, with A drawn for each pair apart from the reviewer's labels. The user picks A, B
or no preference for quality, initiative and overall, and each pick is appended to
`verdicts.jsonl`.

The page never says which arm wrote which side, in its text or in its HTML source: the sides
are A and B, a pair is addressed by its number on the page, and an image by its side and
position. It follows `efficiency/src/beryl_efficiency/serve.py` for serving a page with the
standard library, and also takes a form `POST` to record a verdict.
"""

from __future__ import annotations

import argparse
import html
import json
import mimetypes
import random
import secrets
from dataclasses import dataclass, field
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from . import blind, manifest, review
from .manifest import Grade, Pair, Verdict
from .spec import Spec

HOST = "127.0.0.1"
PORT = 8766
LOCAL_NAMES: tuple[str, ...] = ("127.0.0.1", "localhost")
CHOICES = {"A": "A", "B": "B", "none": "No preference"}
QUESTIONS = {
    "quality": "Quality",
    "initiative": "Initiative",
    "overall": "Overall",
}


@dataclass
class Item:
    """One pair on the page: its diffs and image files by side, A and B."""

    number: int
    pair: str
    a_arm: str
    ticket: str
    diffs: dict[str, str]
    images: dict[str, list[Path]] = field(default_factory=dict)


def choose(
    found: dict[str, tuple[Pair, dict[str, Grade]]], count: int, seed: int
) -> list[dict[str, str]]:
    """`count` pairs of `found` (every pair that is not void), spread across the kinds: the
    kinds take turns, each giving its next pair in an order drawn from `seed`. Each chosen pair
    gets its own arm for A."""
    rng = random.Random(seed)
    by_kind: dict[str, list[str]] = {}
    for token in sorted(found):
        by_kind.setdefault(found[token][0].kind, []).append(token)
    for tokens in by_kind.values():
        rng.shuffle(tokens)
    chosen: list[str] = []
    kinds = sorted(by_kind)
    while len(chosen) < count and any(by_kind.values()):
        for kind in kinds:
            if by_kind[kind] and len(chosen) < count:
                chosen.append(by_kind[kind].pop())
    return [{"pair": t, "a_arm": rng.choice(review.ARMS)} for t in chosen]


def selection(
    trial: str, spec: Spec, eligible: dict[str, tuple[Pair, dict[str, Grade]]]
) -> list[dict[str, str]]:
    """The pairs the page shows, chosen once and kept in `judging.json`. A selection that has
    fewer than `judge_pairs` grows when more pairs become eligible."""
    path = manifest.trial_dir(trial) / "judging.json"
    kept = json.loads(path.read_text(encoding="utf-8")) if path.is_file() else {}
    seed = int(kept.get("seed") or secrets.randbits(32))
    chosen: list[dict[str, str]] = list(kept.get("pairs") or [])
    if len(chosen) < spec.judge_pairs:
        taken = {c["pair"] for c in chosen}
        rest = {t: v for t, v in eligible.items() if t not in taken}
        chosen += choose(rest, spec.judge_pairs - len(chosen), seed + len(chosen))
        path.write_text(
            json.dumps({"seed": seed, "pairs": chosen}, indent=1) + "\n",
            encoding="utf-8",
        )
    return chosen


# The page.

STYLE = """
body { font: 14px/1.4 -apple-system, system-ui, sans-serif; margin: 0 24px 48px; }
nav a { margin-right: 12px; }
.sides { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
pre { background: #f6f6f6; padding: 8px; overflow: auto; font-size: 12px; max-height: 80vh; }
pre.ticket { white-space: pre-wrap; max-height: 40vh; }
.add { color: #116329; } .del { color: #82071e; } .hunk { color: #6639ba; }
img { max-width: 100%; border: 1px solid #ccc; margin: 4px 0; }
fieldset { display: inline-block; margin-right: 16px; }
"""


def diff_html(diff: str) -> str:
    lines = []
    for line in diff.splitlines():
        kind = (
            "hunk"
            if line.startswith("@@")
            else (
                "add" if line.startswith("+") else "del" if line.startswith("-") else ""
            )
        )
        text = html.escape(line)
        lines.append(f'<span class="{kind}">{text}</span>' if kind else text)
    return "\n".join(lines) or "(no change)"


def document(title: str, body: str) -> str:
    return (
        f"<!doctype html><html><head><meta charset='utf-8'><title>{html.escape(title)}"
        f"</title><style>{STYLE}</style></head><body>{body}</body></html>"
    )


def index_html(items: list[Item], verdicts: dict[str, Verdict]) -> str:
    rows = "".join(
        f"<li><a href='/pair/{i.number}'>Pair {i.number}</a>"
        f"{' (judged)' if i.pair in verdicts else ''}</li>"
        for i in items
    )
    return document("Blind pairs", f"<h1>Blind pairs</h1><ol>{rows}</ol>")


def pair_html(item: Item, total: int, verdict: Verdict | None) -> str:
    nav = "<nav><a href='/'>All pairs</a>"
    if item.number > 1:
        nav += f"<a href='/pair/{item.number - 1}'>Previous</a>"
    if item.number < total:
        nav += f"<a href='/pair/{item.number + 1}'>Next</a>"
    nav += "</nav>"
    sides = ""
    for side in ("A", "B"):
        images = "".join(
            f"<img src='/image/{item.number}/{side}/{k + 1}' alt='Side {side}, picture {k + 1}'>"
            for k in range(len(item.images.get(side, [])))
        )
        sides += (
            f"<div><h2>{side}</h2>{images}"
            f"<pre>{diff_html(item.diffs.get(side, ''))}</pre></div>"
        )
    fields = ""
    for key, label in QUESTIONS.items():
        current = getattr(verdict, key, None) if verdict else None
        options = "".join(
            f"<label><input type='radio' name='{key}' value='{value}' required"
            f"{' checked' if current == value else ''}> {text}</label> "
            for value, text in CHOICES.items()
        )
        fields += f"<fieldset><legend>{label}</legend>{options}</fieldset>"
    form = (
        f"<form method='post' action='/pair/{item.number}'>{fields}"
        "<button type='submit'>Save</button></form>"
    )
    body = (
        f"{nav}<h1>Pair {item.number} of {total}</h1>"
        f"<pre class='ticket'>{html.escape(item.ticket)}</pre>{form}"
        f"<div class='sides'>{sides}</div>"
    )
    return document(f"Pair {item.number}", body)


def verdict_of(item: Item, trial: str, form: dict[str, list[str]]) -> Verdict | None:
    picks = {k: (form.get(k) or [""])[0] for k in QUESTIONS}
    if not all(v in CHOICES for v in picks.values()):
        return None
    return Verdict(
        pair=item.pair,
        trial=trial,
        a_arm=item.a_arm,
        quality=picks["quality"],
        initiative=picks["initiative"],
        overall=picks["overall"],
    )


# Serving it.


def build_items(trial: str, spec: Spec) -> list[Item]:
    pairs = {p.pair: p for p in manifest.read_pairs(trial)}
    eligible = {
        token: (pairs[token], arms)
        for token, arms in manifest.latest_grades(manifest.read_grades(trial)).items()
        if token in pairs and not review.is_void(pairs[token], arms)
    }
    items: list[Item] = []
    for chosen in selection(trial, spec, eligible):
        pair, arms = eligible.get(chosen["pair"], (None, None))
        if pair is None or arms is None:
            continue
        material = review.gather(pair, arms)
        labels = blind.labels(chosen["a_arm"])
        items.append(
            Item(
                number=len(items) + 1,
                pair=pair.pair,
                a_arm=chosen["a_arm"],
                ticket=material.ticket,
                diffs={s: material.sides[a].diff or "" for s, a in labels.items()},
                images={s: material.sides[a].images for s, a in labels.items()},
            )
        )
    return items


class Handler(BaseHTTPRequestHandler):
    trial: str = ""
    items: list[Item] = []

    def send(self, status: int, kind: str, body: bytes) -> None:
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def local(self) -> bool:
        host = (self.headers.get("Host") or "").rsplit(":", 1)[0]
        if host not in LOCAL_NAMES:
            self.send(HTTPStatus.FORBIDDEN, "text/plain", b"local only")
            return False
        return True

    def item(self, number: str) -> Item | None:
        return next((i for i in self.items if str(i.number) == number), None)

    def do_GET(self) -> None:  # noqa: N802, the name http.server calls
        if not self.local():
            return
        parts = [p for p in urlsplit(self.path).path.split("/") if p]
        verdicts = {v.pair: v for v in manifest.read_verdicts(self.trial)}
        if not parts:
            page = index_html(self.items, verdicts)
            self.send(HTTPStatus.OK, "text/html; charset=utf-8", page.encode())
            return
        found = self.item(parts[1]) if len(parts) > 1 else None
        if parts[0] == "pair" and found and len(parts) == 2:
            page = pair_html(found, len(self.items), verdicts.get(found.pair))
            self.send(HTTPStatus.OK, "text/html; charset=utf-8", page.encode())
            return
        if parts[0] == "image" and found and len(parts) == 4 and parts[3].isdigit():
            images = found.images.get(parts[2], [])
            k = int(parts[3]) - 1
            if 0 <= k < len(images) and images[k].is_file():
                kind = (
                    mimetypes.guess_type(images[k].name)[0]
                    or "application/octet-stream"
                )
                self.send(HTTPStatus.OK, kind, images[k].read_bytes())
                return
        self.send(HTTPStatus.NOT_FOUND, "text/plain", b"not found")

    def do_POST(self) -> None:  # noqa: N802
        if not self.local():
            return
        parts = [p for p in urlsplit(self.path).path.split("/") if p]
        found = self.item(parts[1]) if len(parts) == 2 and parts[0] == "pair" else None
        length = int(self.headers.get("Content-Length") or 0)
        form = parse_qs(self.rfile.read(min(length, 10_000)).decode("utf-8", "replace"))
        verdict = verdict_of(found, self.trial, form) if found else None
        if found is None or verdict is None:
            self.send(
                HTTPStatus.BAD_REQUEST,
                "text/plain",
                b"pick A, B or no preference for each",
            )
            return
        manifest.append_verdict(verdict)
        following = found.number + 1 if found.number < len(self.items) else None
        self.send_response(HTTPStatus.SEE_OTHER)
        self.send_header("Location", f"/pair/{following}" if following else "/")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, format: str, *args: object) -> None:
        pass


def cmd_judge(args: argparse.Namespace, spec: Spec) -> int:
    trial: str = args.trial
    items = build_items(trial, spec)
    if not items:
        print("No graded pair that is not void is ready to judge.")
        return 0

    class Bound(Handler):
        pass

    Bound.trial, Bound.items = trial, items
    server = ThreadingHTTPServer((HOST, int(args.port)), Bound)
    print(
        f"model-trial judge: http://{HOST}:{server.server_address[1]}/ ({len(items)} pairs)",
        flush=True,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0
