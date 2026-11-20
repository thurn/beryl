"""`just b3d match snapshot`: both clients' pictures and element records of one moment, in the
target's snapshot directory (target.py), which the other `match` subcommands read.

1. `b3d tree --records` plays the recording to the moment in Beryl 3D and writes `b3d.json`:
   every element with its box, paint order, style in CSS's names and lines of text. It also
   says which view the moment falls in, when that view arrived and whether the table is still
   moving.
2. `b3d-render png` draws the moment at the target's scale as `b3d.png`.
3. The recording is exported as the web interface's fixture (`b3d export --only`), and
   dump.mjs opens the page `?fixture=<name>&at=<n>#/game` (or, for a screen of
   data/beryl3d/screens.json, the page its `web` names) in WebKit on Playwright's paused
   clock. For a moment in the middle of an animation, the page opens with the views Beryl 3D
   had played out, is handed the rest live at the times Beryl 3D was given them, and its clock
   runs as long as Beryl 3D's did since the first of those (clock.py); a moment at rest runs it
   until the stage is idle. It
   writes `web.png` and `web.json`, the same records read from the DOM and its computed styles.
4. `report.txt` names the moment and the files, and warns of any input the two sides do not
   share: the fonts (fonts.py) and the card scans.

Card faces: by default neither side draws a scan. A fixture page draws placeholder faces, and
Beryl 3D, given an empty scan directory (`BERYL3D_SCANS`), draws Beryl's card frame. With
`--art` both read the app's scan cache: Beryl 3D directly, and the page through a route that
serves Scryfall's image paths from it.

A snapshot is reused while its inputs (the target, `--art`, and the sources of both clients)
are unchanged, unless `--fresh`.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import uuid
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from dataclasses import dataclass, replace
from pathlib import Path
from typing import TypeVar

from ... import paths as repo_paths
from ... import shots
from .. import compare
from . import clock, css, fonts, joined, records
from .target import Target, arguments, only_argument, target

# What a `--all` command measures on each target's snapshot.
R = TypeVar("R")

HERE: Path = Path(__file__).resolve().parent
ROLES: Path = repo_paths.REPO / "data" / "beryl3d" / "match" / "roles.json"
SCREENS: Path = repo_paths.REPO / "data" / "beryl3d" / "screens.json"
# The files a snapshot directory holds once this command has run.
FILES = ("web.png", "web.json", "b3d.png", "b3d.json", "report.txt")
# What a change to either client's look or to these tools is made in.
SOURCES = (
    "ui",
    "rust/beryl3d",
    "rust/crates",
    "data/beryl3d",
    "data/design-tokens.json",
    "tools/src/beryl_tools/beryl3d/match",
)
# The app's scan cache, which Beryl 3D's `Scans::app` reads on macOS.
APP_SCANS: Path = Path.home() / "Library" / "Caches" / "ca.thurn.beryl"
# `b3d-render png`'s line naming the file it wrote.
WROTE: re.Pattern[str] = re.compile(r"^wrote (?P<path>\S+\.png)", re.MULTILINE)


# The moment `--all` takes each target at, unless `--at` gives another.
ALL_AT = "end"

# The `cargo build` arguments of the two programs a snapshot runs.
B3D_BUILD = ["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"]
RENDER_BUILD = ["build", "-q", "-p", "beryl_pictures", "--bin", "b3d-render"]


@dataclass(frozen=True)
class Shared:
    """What snapshots taken together share (`shared`): both programs already built, and one
    Vite server of ui/ at `base` with the browser environment `env`. Each snapshot would
    otherwise build both programs and start its own server, which takes seconds."""

    base: str
    env: dict[str, str]
    # Held while a browser page is open, when the caller limits how many pages its snapshots
    # open at once (`measured`'s `pages`); None for no limit beyond the machine's page slots.
    pages: threading.Semaphore | None = None


@contextmanager
def shared(pages: int | None = None) -> Iterator[Shared | None]:
    """Builds both programs and serves ui/ for the duration, for many `ensure` calls; yields
    None after printing why it could not. With `pages`, the snapshots taken with it open at
    most that many browser pages at once."""
    from .. import cli

    if cli.quick_build(B3D_BUILD) != 0 or cli.quick_build(RENDER_BUILD) != 0:
        print("match: b3d or b3d-render did not build", file=sys.stderr)
        yield None
        return
    env = shots.browser_env()
    try:
        with shots.served_ui(env) as url:
            limit = None if pages is None else threading.BoundedSemaphore(pages)
            yield Shared(url, env, limit)
    except shots.UiError as error:
        print(f"match: {error}", file=sys.stderr)
        yield None


def snapshot_arguments(parser: argparse.ArgumentParser) -> None:
    arguments(parser)
    only_argument(parser)
    parser.add_argument(
        "--fresh", action="store_true", help="take it again even if nothing changed"
    )
    parser.add_argument(
        "--art", action="store_true", help="draw the app's card scans on both sides"
    )


def inputs(t: Target, art: bool) -> str:
    """A digest of what the snapshot of `t` depends on: the target, `--art`, the commit and any
    uncommitted change to the sources of either client or these tools."""
    git = ["git", "-C", str(repo_paths.REPO)]
    head = subprocess.run(
        [*git, "rev-parse", "HEAD"], capture_output=True, text=True, check=False
    ).stdout
    diff = subprocess.run(
        [*git, "diff", "HEAD", "--", *SOURCES],
        capture_output=True,
        text=True,
        check=False,
    ).stdout
    digest = hashlib.sha256()
    for part in (repr(t), str(art), head, diff):
        digest.update(part.encode())
        digest.update(b"\0")
    return digest.hexdigest()


def render_png(t: Target, art: bool, out: Path, build: bool = True) -> str | None:
    """Draws the moment with `b3d-render png` and copies it to `out`; None when it worked, else
    why not. Without `art` it reads scans from an empty directory, so every face is drawn. It
    builds `b3d-render` first unless `build` is False.
    """
    from .. import cli

    if build and cli.quick_build(RENDER_BUILD) != 0:
        return "b3d-render did not build"
    with tempfile.TemporaryDirectory(prefix="b3d-match-scans-") as empty:
        env = {**os.environ}
        if not art:
            env["BERYL3D_SCANS"] = empty
        line = [str(cli.linked("b3d-render")), "png", t.recording, *t.common()]
        line += ["--at", t.at]
        if t.only is not None:
            line += ["--only", t.only]
        done = subprocess.run(
            line, capture_output=True, text=True, env=env, check=False
        )
        if done.returncode == cli.MISSING_PACK and cli.record() == 0:
            done = subprocess.run(
                line, capture_output=True, text=True, env=env, check=False
            )
    wrote = WROTE.search(done.stdout)
    if done.returncode != 0 or not wrote:
        return f"b3d-render png failed:\n{done.stdout}{done.stderr}"
    shutil.move(wrote["path"], out)
    return None


def web_page(recording: str) -> dict[str, str]:
    """How the web interface opens the screen the recording `recording` shows: the `web` of its
    screens.json entry, `{"hash", "query"?, "click"?}`, or the game page for a recording that
    is no screen's. The web app picks its screen from the route's hash (`route` in
    ui/src/App.tsx), and some screens, such as settings, open over another screen only when a
    button is pressed."""
    for screen in json.loads(SCREENS.read_text())["screens"]:
        if screen["recording"] == recording and "web" in screen:
            web: dict[str, str] = screen["web"]
            return web
    return {"hash": "/game"}


def page_job(
    t: Target, messages: list[dict[str, object]], at: int, art: bool, out: Path
) -> dict[str, object]:
    """What page.mjs needs to open the web interface's page of `messages` in `t`'s window, with
    the first `at` of its events handed over (`compare.web_at`), beside the fixture file written
    into `out` that the page reads. `run_page` adds the server's address."""
    width, height = t.width_height()
    # Unique among the snapshots one process takes at once on threads (`boxes --all`).
    name = f"b3d-match-{os.getpid()}-{uuid.uuid4().hex[:8]}"
    # page.mjs answers the page's request for the fixture with this file. A file written into
    # ui/public would not do: a server started earlier (`Shared`) watches no files, so it
    # serves only the public files that existed when it started.
    fixture = out / f"{name}.json"
    fixture.write_text(json.dumps({"messages": messages}))
    web = web_page(t.recording)
    query = [
        *(["phone"] if t.layout == "phone" else []),
        *([web["query"]] if "query" in web else []),
        f"fixture={name}",
        f"at={at}",
    ]
    return {
        "route": f"/?{'&'.join(query)}#{web['hash']}",
        "click": web.get("click"),
        "fixture": {"name": name, "file": str(fixture)},
        "width": width,
        "height": height,
        "scale": float(t.scale),
        "roles": json.loads(ROLES.read_text())["roles"],
        "target": t.test_name(),
        "art_dir": str(APP_SCANS / "art") if art else None,
    }


def run_page(
    script: str, job: dict[str, object], out: Path, serving: Shared | None
) -> tuple[list[str], str | None]:
    """Runs the page script `script` (dump.mjs or motion.mjs) on `job` (`page_job`), from the
    server `serving` names or from one started for it, and deletes the job's files afterwards.
    Returns the lines it printed (page errors, scans missing) and, if it failed, why."""
    job_file: Path = out / f"{script}-job.json"
    fixture = job["fixture"]
    assert isinstance(fixture, dict)

    def dump(base: str, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
        job_file.write_text(json.dumps({**job, "base": base}))
        return subprocess.run(
            ["node", str(HERE / script), str(job_file)],
            cwd=repo_paths.UI,
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
        )

    try:
        if serving is not None and serving.pages is not None:
            with serving.pages:
                done = dump(serving.base, serving.env)
        elif serving is not None:
            done = dump(serving.base, serving.env)
        else:
            env = shots.browser_env()
            with shots.served_ui(env) as url:
                done = dump(url, env)
    except shots.UiError as error:
        return [], str(error)
    finally:
        Path(str(fixture["file"])).unlink(missing_ok=True)
        job_file.unlink(missing_ok=True)
    lines = done.stdout.splitlines()
    if done.returncode != 0:
        return lines, f"{script} failed:\n{done.stdout}{done.stderr}"
    return lines, None


def dump_web(
    t: Target,
    messages: list[dict[str, object]],
    moment: dict[str, object],
    art: bool,
    out: Path,
    serving: Shared | None = None,
    only: int | None = None,
) -> tuple[list[str], str | None]:
    """Shoots the web interface's page of `messages` at `moment` (`b3d.json`'s) into `out`'s
    `web.png` and `web.json` with dump.mjs. With `only`, the index of a web element in the
    records, the picture shows that element alone. Returns `run_page`'s lines and failure.
    """
    at = compare.web_at(messages, int(str(moment["view"])))
    given = moment.get("view_given_ms")
    views_given = moment.get("views_given_ms")
    offset: float | None = None
    advances: list[list[float]] = []
    notes: list[str] = []
    if moment.get("animating") and isinstance(given, int):
        now = float(str(moment["t_ms"]))
        base = float(given)
        if isinstance(views_given, list):
            base, at, advances, notes = clock.handover(
                messages,
                [float(g) for g in views_given],
                clock.script_ends(t.recording, t.seat, serving is None),
                now,
                now,
                base,
            )
        offset = now - base
    job = page_job(t, messages, at, art, out)
    job |= {
        "offset_ms": offset,
        "advances": advances,
        "png": str(out / "web.png"),
        "json": str(out / "web.json"),
        "only": only,
    }
    lines, failure = run_page("dump.mjs", job, out, serving)
    lines = [note.removeprefix("note: ") for note in notes] + lines
    if failure is None:
        # dump.mjs writes the layered properties as WebKit's text; the records hold structures.
        written = out / "web.json"
        document = json.loads(written.read_text())
        for element in document["elements"]:
            element["style"] = css.normalized(element["style"])
        written.write_text(json.dumps(document, indent=1) + "\n")
    return lines, failure


def report(
    t: Target,
    moment: dict[str, object],
    warnings: list[str],
    out: Path,
    final: Path | None = None,
) -> str:
    """The snapshot's report: how to take it again, the moment, the warnings and the files.
    It reads the records in `out` and names the files as they will be in `final` (default
    `out`), where a snapshot taken in a staging directory ends up."""
    final = final or out
    web = json.loads((out / "web.json").read_text())
    b3d = json.loads((out / "b3d.json").read_text())
    state = "at rest"
    given = moment.get("view_given_ms")
    if moment.get("animating") and isinstance(given, int):
        # dump_web runs the page's clock this long after the view arrives.
        state = (
            f"animating, {int(str(moment['t_ms'])) - given}ms after the view arrived"
        )
    command = f"just b3d match snapshot {t.recording} --at {t.at} " + " ".join(
        t.options()
    )
    if t.only is not None:
        command += f" --only '{t.only}'"
    lines = [
        command,
        f"target {t.test_name()} ({t.layout}), view {moment['view']} of {moment['views']}, "
        f"t={moment['t_ms']}ms, {state}",
        f"web: {len(web['elements'])} elements, Beryl 3D: {len(b3d['elements'])} elements",
        *warnings,
        *(str(final / name) for name in FILES),
    ]
    return "\n".join(lines) + "\n"


def take(
    t: Target, out: Path, art: bool, serving: Shared | None
) -> tuple[dict[str, object], list[str]] | None:
    """Writes both sides' records and pictures of `t` into `out`; the moment `b3d.json`
    describes and the lines dump.mjs printed, or None after printing why it could not.
    """
    build = serving is None
    printed = compare.b3d(
        ["tree", t.recording, *t.common(), "--at", t.at, "--records"], build
    )
    if printed is None:
        return None
    printout = json.loads(printed.strip().splitlines()[-1])
    moment = {
        key: printout[key]
        for key in (
            "view",
            "views",
            "t_ms",
            "view_given_ms",
            "views_given_ms",
            "animating",
        )
        if key in printout
    }
    (out / "b3d.json").write_text(
        json.dumps(
            {
                "target": t.test_name(),
                "scale": float(t.scale),
                "moment": moment,
                "elements": printout["elements"],
            },
            indent=1,
        )
        + "\n"
    )
    failed = render_png(t, art, out / "b3d.png", build)
    if failed:
        print(f"match snapshot: {failed}", file=sys.stderr)
        return None

    messages = exported(t, build)
    if messages is None:
        return None
    printed_lines, failed = dump_web(t, messages, moment, art, out, serving)
    if failed:
        print(f"match snapshot: {failed}", file=sys.stderr)
        return None
    return moment, printed_lines


def exported(t: Target, build: bool) -> list[dict[str, object]] | None:
    """The recording's messages from `t`'s seat, as the web interface's fixture holds them
    (`b3d export --only`), or None after `b3d` printed why not."""
    work = Path(tempfile.mkdtemp(prefix="b3d-match-"))
    try:
        if compare.b3d(["export", str(work), "--only", t.recording], build) is None:
            return None
        path = work / f"{t.recording.replace('/', '__')}.{t.seat}.json"
        messages: list[dict[str, object]] = json.loads(path.read_text())
        return messages
    finally:
        shutil.rmtree(work, ignore_errors=True)


def web_pair(directory: Path, locator: str) -> tuple[int | None, str]:
    """The index of the web element paired with the Beryl 3D element `locator` names in the
    snapshot in `directory`, or None and why there is none. As in `b3d tree`, a locator is the
    start of a tree line, so the element whose key equals it wins, then the only one whose key
    starts with it."""
    b3d = json.loads((directory / "b3d.json").read_text())["elements"]
    keys = [str(element.get("key") or "") for element in b3d]
    found = [at for at, key in enumerate(keys) if key == locator]
    if not found:
        found = [at for at, key in enumerate(keys) if key.startswith(locator)]
    if len(found) != 1:
        some = ", ".join(keys[at] for at in found[:5])
        return None, (
            f"--only: '{locator}' names {len(found)} Beryl 3D elements"
            + (f" ({some})" if some else "; `just b3d tree --at <moment>` lists them")
        )
    pairs = joined.ensure(directory)["pairs"]
    assert isinstance(pairs, list)
    for pair in pairs:
        if joined.index(pair, "b3d") == found[0]:
            return joined.index(pair, "web"), ""
    return None, (
        f"--only: no web element is paired with {keys[found[0]]}; "
        "`just b3d match boxes` lists the unpaired elements"
    )


def take_only(
    t: Target, whole: Path, out: Path, art: bool, serving: Shared | None
) -> tuple[dict[str, object], list[str]] | None:
    """As `take`, for a target with `--only`: the records are those of the whole window's
    snapshot `whole`, and both pictures show only the element `t.only` names and its web pair,
    on black."""
    index, why = web_pair(whole, str(t.only))
    if index is None:
        print(f"match snapshot: {why}", file=sys.stderr)
        return None
    b3d = json.loads((whole / "b3d.json").read_text())
    (out / "b3d.json").write_text(json.dumps(b3d, indent=1) + "\n")
    failed = render_png(t, art, out / "b3d.png", serving is None)
    if failed:
        print(f"match snapshot: {failed}", file=sys.stderr)
        return None
    messages = exported(t, serving is None)
    if messages is None:
        return None
    moment = b3d["moment"]
    printed_lines, failed = dump_web(t, messages, moment, art, out, serving, index)
    if failed:
        print(f"match snapshot: {failed}", file=sys.stderr)
        return None
    return moment, printed_lines


def ensure(
    t: Target, fresh: bool = False, art: bool = False, serving: Shared | None = None
) -> Path | None:
    """The snapshot directory of `t`, taken now unless an up-to-date one exists; None after
    printing why it could not be taken. The other `match` subcommands start from this. Given
    `serving` (`shared`), it neither builds nor starts a server of its own.
    """
    final = t.directory()
    digest = inputs(t, art)
    stamp = final / "inputs.txt"
    complete = all((final / name).exists() for name in FILES)
    if not fresh and complete and stamp.exists() and stamp.read_text() == digest:
        return final
    # The snapshot is written to a directory of its own and renamed into place when complete,
    # so a reader never sees half of one and an interrupted run leaves the old one intact.
    out = final.parent / f".{final.name}.{os.getpid()}-{uuid.uuid4().hex[:8]}"
    out.mkdir()
    try:
        if not complete_into(
            t, out, final, whole_args=(fresh, art, serving), digest=digest
        ):
            return None
        trash = final.parent / f".{final.name}.old-{uuid.uuid4().hex[:8]}"
        if final.exists():
            final.rename(trash)
        out.rename(final)
        shutil.rmtree(trash, ignore_errors=True)
        return final
    finally:
        shutil.rmtree(out, ignore_errors=True)


def complete_into(
    t: Target,
    out: Path,
    final: Path,
    whole_args: tuple[bool, bool, Shared | None],
    digest: str,
) -> bool:
    """Takes the snapshot of `t` into the empty directory `out`, with its report naming the files
    as they will be in `final`; False after printing why it could not."""
    fresh, art, serving = whole_args
    if t.only is None:
        took = take(t, out, art, serving)
    else:
        whole = ensure(replace(t, only=None), fresh, art, serving)
        took = None if whole is None else take_only(t, whole, out, art, serving)
    if took is None:
        return False
    moment, printed_lines = took

    for side in ("web", "b3d"):
        broken = records.faults(json.loads((out / f"{side}.json").read_text()))
        if broken:
            listed = "\n  ".join(broken[:10])
            print(
                f"match snapshot: {side}.json breaks the record contract:\n  {listed}",
                file=sys.stderr,
            )
            return False
    warnings = fonts.check()
    missing = [line for line in printed_lines if line.startswith("scan missing:")]
    if art:
        if missing:
            warnings.append(
                f"warning: scans: {len(missing)} card images are missing from the app's scan "
                f"cache ({APP_SCANS / 'art'}): the web shows none and Beryl 3D its card frame"
            )
    else:
        warnings.append(
            "note: scans: neither side draws card scans (the web its placeholder faces, Beryl "
            "3D its card frame); --art draws the app's scan cache on both"
        )
    warnings += [
        f"warning: web {line}" for line in printed_lines if line not in missing
    ]
    (out / "report.txt").write_text(report(t, moment, warnings, out, final))
    (out / "inputs.txt").write_text(digest)
    return True


def test_targets(prefix: str, at: str) -> list[Target] | None:
    """Every desktop and phone test target whose name starts with `prefix`, at `at`, as
    `just b3d test --list` names them; None after printing why they could not be listed.
    """
    names: list[str] = []
    for phone in ([], ["--phone-only"]):
        printed = compare.b3d(["test", *([prefix] if prefix else []), "--list", *phone])
        if printed is None:
            return None
        names += printed.split()
    return [
        target(
            argparse.Namespace(
                recording=name, at=at, seat=None, layout=None, size=None, scale=None
            )
        )
        for name in names
    ]


def every(
    recording: str | None,
    at: str | None,
    fresh: bool,
    art: bool,
    measure: Callable[[Target, Path], R],
) -> Iterator[tuple[Target, R | None]] | None:
    """`measure` run on the snapshot of every test target whose name starts with `recording`
    (all of them when it is None), at `at` or `ALL_AT`, yielding each target with what it
    measured, or None where the snapshot failed, in the order they finish; None after printing
    why the targets could not be listed. `match boxes --all` and `match gaps --all` are this.
    """
    prefix = recording or ""
    if prefix and not prefix.startswith("b3d:"):
        prefix = f"b3d:{prefix}"
    targets = test_targets(prefix, at or ALL_AT)
    if targets is None:
        return None
    return measured(targets, fresh, art, measure)


def measured(
    targets: list[Target],
    fresh: bool,
    art: bool,
    measure: Callable[[Target, Path], R],
    pages: int | None = None,
) -> Iterator[tuple[Target, R | None]]:
    """`measure` run on the snapshot of each of `targets`, yielding each target with what it
    measured, or None where the snapshot failed, in the order they finish. With `pages`, at
    most that many browser pages are open at once."""
    # Each target's snapshot runs three programs and a browser page, so the targets run on as
    # many threads as the machine's cores; the pages also wait for the machine's browser page
    # slots (ui/scripts/slots.mjs).
    workers = max(1, int(os.environ.get("IGNITE_CORES") or os.cpu_count() or 1))
    with shared(pages) as given:
        if given is None:
            return
        serving: Shared = given

        def one(t: Target) -> R | None:
            directory = ensure(t, fresh, art, serving)
            return None if directory is None else measure(t, directory)

        with concurrent.futures.ThreadPoolExecutor(workers) as pool:
            jobs = {pool.submit(one, t): t for t in targets}
            for job in concurrent.futures.as_completed(jobs):
                yield jobs[job], job.result()


def run(args: argparse.Namespace) -> int:
    t = target(args)
    out = ensure(t, fresh=args.fresh, art=args.art)
    if out is None:
        return 1
    print((out / "report.txt").read_text(), end="")
    return 0
