"""The static files of cirrus, written into a site under content-hashed names so that a browser
and the service worker can keep them forever: a changed file gets a new name."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from importlib import resources
from pathlib import Path

# The files a page links, by the name the code uses. The service worker is not hashed: a
# browser looks for it at a fixed address, beside the site's root page.
HASHED: tuple[str, ...] = ("cirrus.css", "cirrus.js")
WORKER = "sw.js"


def source(name: str) -> bytes:
    """A static file's bytes. `cirrus.css` is the files of `static/css/` joined in the order
    of their names (tokens, layout, components, motion), so the page loads one stylesheet
    while each source file stays a readable length."""
    static = resources.files("cirrus").joinpath("static")
    if name == "cirrus.css":
        parts = sorted(
            (p for p in static.joinpath("css").iterdir() if p.name.endswith(".css")),
            key=lambda p: p.name,
        )
        return b"\n".join(p.read_bytes() for p in parts)
    return static.joinpath(name).read_bytes()


def hashed_name(name: str, data: bytes) -> str:
    """`cirrus.css` as `cirrus.<10 hex digits>.css`; the service worker recognises the form."""
    stem, dot, extension = name.rpartition(".")
    digest = hashlib.sha256(data).hexdigest()[:10]
    return f"{stem}.{digest}{dot}{extension}"


@dataclass(frozen=True)
class Assets:
    """Where each static file lives in the site, relative to its root."""

    paths: dict[str, str]

    @staticmethod
    def load() -> Assets:
        return Assets(
            {name: f"assets/{hashed_name(name, source(name))}" for name in HASHED}
        )

    def write(self, site: Path) -> list[Path]:
        """Writes every static file that is missing, and returns what it wrote."""
        written: list[Path] = []
        for name, relative in [*self.paths.items(), (WORKER, WORKER)]:
            target = site / relative
            data = source(name)
            if target.exists() and target.read_bytes() == data:
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            written.append(target)
        return written

    def url(self, name: str, root: str) -> str:
        return root + self.paths[name]
