"""`ui-shots`: screenshots of UI states without opening a window. It records the game fixtures
ui/fixtures.json defines with `beryl_cli record-fixture`, serves the UI with Vite, captures the
routes listed in ui/shots.json with headless Playwright (ui/scripts/shots.mjs) and prints the
paths. Playwright runs the engine of the app's webview: WebKit on macOS, Chromium on Windows.
Pages run in a plain browser on the fixture transport (ui/src/ipc.ts).

`ui-test` plays the game screen through a fixture the same way, dragging, clicking and hovering
as a player would (ui/scripts/interactions.mjs), and then the stage's verbs on the stage's own
clock (ui/scripts/verbs.mjs, the checks named `verb:...`). It records its fixtures into the cache
instead, where its pages fetch them, so it runs alongside `ui-shots` (slow CI's `shots` and
`ui-test` lanes).

`ui-live` serves live games instead: `beryl_cli serve` runs the app's game commands, Vite serves
the UI against it (`?live=`, src/ipc.ts), and ui/scripts/drive.mjs holds a headless page
open on it that an agent drives one step at a time over HTTP, until interrupted.

Screenshots go to the cache (`ui-shots/<checkout>/`), never into the tree; the browser is
installed there on first use.
"""

from __future__ import annotations

import argparse
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from . import checks, paths, selection, system, warm

SERVER_TIMEOUT = 60.0
# The game fixtures' definitions, and where they are recorded to: gitignored, served by Vite.
FIXTURES: Path = paths.UI / "fixtures.json"
RECORDED: Path = paths.UI / "public" / "fixtures"


class UiError(Exception):
    """Serving the UI or installing the browser failed; the message says why."""


def out_dir() -> Path:
    """This checkout's screenshots: a fresh folder per run, so nothing stale is left there."""
    return paths.cache_root() / "ui-shots" / paths.REPO.name


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def port_free(port: int) -> bool:
    """Whether nothing listens on `port`. Like Node's servers, it may reuse a port a closed
    connection still holds."""
    with socket.socket() as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            s.bind(("127.0.0.1", port))
        except OSError:
            return False
        return True


def wait_for_server(url: str, server: subprocess.Popen[bytes]) -> bool:
    """Polls until Vite answers, it exits, or the timeout passes."""
    deadline = time.monotonic() + SERVER_TIMEOUT
    while time.monotonic() < deadline and server.poll() is None:
        try:
            with urllib.request.urlopen(url, timeout=2):
                return True
        except (urllib.error.URLError, OSError):
            time.sleep(0.1)
    return False


# The engine of the app's webview: WKWebView's on macOS, WebView2's on Windows.
BROWSER = "chromium" if system.WINDOWS else "webkit"


def browser_env(browser: str = BROWSER) -> dict[str, str]:
    """The environment for the capture scripts, with `browser` (by default the app webview's
    engine) installed in the cache for Playwright."""
    warm.ensure()
    env = {
        **os.environ,
        "PLAYWRIGHT_BROWSERS_PATH": str(paths.cache_root() / "playwright"),
        "BERYL_BROWSER": browser,
    }
    # A no-op once it is there.
    install = subprocess.run(
        [system.npm_bin("playwright"), "install", browser],
        cwd=paths.UI,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        check=False,
    )
    if install.returncode != 0:
        raise UiError(
            install.stdout
            + install.stderr
            + f"installing Playwright's {browser} failed"
        )
    return env


def record_fixtures(into: Path = RECORDED, only: list[str] | None = None) -> int:
    """Plays the fixtures' seeded games (those named in `only`, or all) and records them into
    `into`, fresh each run."""
    shutil.rmtree(into, ignore_errors=True)
    return subprocess.run(
        ["cargo", "run", "--quiet", "--package", "beryl_cli", "--"]
        + ["record-fixture", str(FIXTURES), str(into)]
        + (only or []),
        cwd=paths.RUST,
        env=paths.run_env(),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        check=False,
    ).returncode


@contextmanager
def served_ui(
    env: dict[str, str], port: int | None = None, preview: bool = False
) -> Iterator[str]:
    """Serves ui/ with Vite on `port`, or a free one, for the duration; yields its base URL.
    `preview` serves the production build in ui/dist (`vite preview`) instead of the dev server.
    """
    port = port or free_port()
    url = f"http://127.0.0.1:{port}/"
    mode = ["preview", "--strictPort"] if preview else []
    with tempfile.TemporaryFile() as log:
        server = subprocess.Popen(
            [system.npm_bin("vite"), *mode, "--host", "127.0.0.1", "--port", str(port)],
            cwd=paths.UI,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            if not wait_for_server(url, server):
                log.seek(0)
                raise UiError(
                    log.read().decode(errors="replace") + f"Vite did not serve {url}"
                )
            yield url
        finally:
            system.terminate(server)
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()


def fixture_views(name: str) -> int:
    """Prints fixture `name`'s views, each with the index `&at=` takes, recorded into the
    cache for the purpose (ui/scripts/fixture-views.mjs)."""
    into = paths.cache_root() / "ui-views" / paths.REPO.name / name
    code = record_fixtures(into, [name])
    if code != 0:
        print(f"ui-shots: recording {name} failed", file=sys.stderr)
        return code
    return subprocess.run(
        ["node", "scripts/fixture-views.mjs", str(into / f"{name}.json")],
        cwd=paths.UI,
        stdin=subprocess.DEVNULL,
        check=False,
    ).returncode


def ui_shots(args: argparse.Namespace) -> int:
    if args.views:
        return fixture_views(args.views)
    chooser = selection.Chooser("shots") if args.select else None
    if chooser and chooser.choice.idle():
        print("shots: the change reaches no shot")
        return 0
    out = out_dir()
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    try:
        env = browser_env()
        # Recording builds and plays the fixtures; failing that, no shot was taken.
        code = checks.CANNOT_RUN if record_fixtures() else 0
        only = list(args.only)
        if code == 0 and chooser:
            only = chooser.run([RECORDED], only)
        if code == 0 and (only or not chooser):
            with served_ui(env) as url:
                code = subprocess.run(
                    ["node", "scripts/shots.mjs", url, str(out), "shots.json"] + only,
                    cwd=paths.UI,
                    env=env,
                    stdin=subprocess.DEVNULL,
                    check=False,
                ).returncode
    except UiError as e:
        print(f"ui-shots: {e}", file=sys.stderr)
        code = checks.CANNOT_RUN
    finally:
        if chooser:
            chooser.label()
    if code != 0:
        print("ui-shots: FAILED", file=sys.stderr)
    return code


# The fixtures ui/scripts/interactions.mjs plays.
INTERACTION_FIXTURES = [
    "greedy-turn-9",
    "opening-bottom",
    "opponent-turn",
    "combat-declare",
    "combat-double-block",
    "combat-blockers",
    "choice-scry",
    "choice-look",
    "choice-jace",
    "choice-pay",
    "choice-trigger-order",
    "hand-pair",
    "cast-oculus",
    "cast-exile-choice",
    "cast-kicker",
    "cast-evoke",
    "teferi-timing",
    "pay-default",
    "pay-other",
    "exiled-beneath",
    "attached-across",
    "exile-cast",
    "arrow-spell",
    "arrow-ability",
    "arrow-player",
    "arrow-stacked",
    "counterspell-targets",
    "reprieve-target",
    "marks",
    "marks-damage",
    "tokens",
    "mana-floating",
    "stack-x",
    "menu-choices",
    "walker-low",
    "namesake-target",
    "pay-life",
    "crowded",
    "piles-crowded",
    "target-up-to-one",
    "target-own",
    "attack-past-walker",
    "choice-explore",
    "choice-loot",
    "choice-untap",
    "choice-search",
    "choice-thirst",
    "choice-cleanup",
    "choice-teferi",
    "target-graveyard",
    "attack-token",
    "crew",
    "waterbend-tap",
    "game-over",
]


def ui_test(args: argparse.Namespace) -> int:
    recorded = paths.cache_root() / "ui-test" / paths.REPO.name
    chooser = selection.Chooser("ui-test") if args.select else None
    if chooser and chooser.choice.idle():
        print("ui-test: the change reaches no check")
        return 0
    try:
        env = browser_env()
        if record_fixtures(recorded, ui_test_fixtures()):
            code = checks.CANNOT_RUN
        else:
            code = interactions(env, recorded, list(args.only), chooser)
    except UiError as e:
        print(f"ui-test: {e}", file=sys.stderr)
        code = checks.CANNOT_RUN
    finally:
        if chooser:
            chooser.label()
    if code != 0:
        print("ui-test: FAILED", file=sys.stderr)
    return code


def ui_test_fixtures() -> list[str]:
    """The fixtures `ui-test` records: interactions.mjs' and the verb suite's."""
    return list(dict.fromkeys(INTERACTION_FIXTURES + selection.verb_fixtures()))


def interactions(
    env: dict[str, str],
    recorded: Path,
    only: list[str],
    chooser: selection.Chooser | None,
) -> int:
    """Plays the checks of interactions.mjs and then verbs.mjs: those `only` names, or with
    `chooser`, those it chose, which the scripts match exactly so that a rerun's filters narrow
    them. A script none of the names is for does not run (selection.scripts). Fails if either
    script does."""
    with tempfile.TemporaryDirectory(prefix="beryl-ui-test-") as tmp:
        names = only
        if chooser:
            names = chooser.run([recorded], only)
            if not names:
                return 0
            if not only:
                chosen = Path(tmp) / "chosen.txt"
                chosen.write_text("".join(f"{t}\n" for t in names))
                env = {**env, selection.CHOSEN_ENV: str(chosen)}
        code = 0
        with served_ui(env) as url:
            for script, own in selection.scripts(names).items():
                ran = subprocess.run(
                    ["node", str(paths.REPO / script), url, str(recorded)]
                    + (own if only else []),
                    cwd=paths.UI,
                    env=env,
                    stdin=subprocess.DEVNULL,
                    check=False,
                )
                code = code or ran.returncode
        return code


def ui_live(args: argparse.Namespace) -> int:
    out = (
        paths.cache_root()
        / "ui-live"
        / paths.REPO.name
        / time.strftime("%Y%m%d-%H%M%S")
    )
    out.mkdir(parents=True)
    profile = ["--release"] if args.release else []
    built = subprocess.run(
        ["cargo", "build", "--quiet", "--package", "beryl_cli", *profile],
        cwd=paths.RUST,
        env=paths.run_env(),
        stdin=subprocess.DEVNULL,
        check=False,
    )
    if built.returncode != 0:
        print("ui-live: building beryl_cli failed", file=sys.stderr)
        return built.returncode
    binary = system.exe(
        paths.RUN_TARGET / ("release" if args.release else "debug") / "beryl_cli"
    )
    # Fixture routes (`?fixture=`) reach rare states; record them as ui-shots does.
    if record_fixtures() != 0:
        print("ui-live: recording the fixtures failed", file=sys.stderr)
        return 1
    if args.port and not port_free(args.port):
        print(f"ui-live: port {args.port} is in use", file=sys.stderr)
        return 1
    game_port, drive_port = free_port(), args.port or free_port()
    children: list[subprocess.Popen[bytes]] = []
    # A plain SIGTERM would end Python without the cleanup below, orphaning the children.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    try:
        env = browser_env()
        children.append(
            subprocess.Popen(
                [str(binary), "serve", "--port", str(game_port)],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
            )
        )
        with served_ui(env) as url:
            app = f"{url}?live=http://127.0.0.1:{game_port}#/"
            children.append(
                subprocess.Popen(
                    ["node", "scripts/drive.mjs", str(drive_port), app, str(out)]
                    + [str(args.width), str(args.height)],
                    cwd=paths.UI,
                    env=env,
                    stdin=subprocess.DEVNULL,
                )
            )
            drive = f"http://127.0.0.1:{drive_port}/"
            if not wait_for_server(drive, children[-1]):
                raise UiError(f"the driver did not answer at {drive}")
            print(f"ui-live: app {app}", flush=True)
            print(f"ui-live: drive {drive} (POST a Playwright snippet)", flush=True)
            print(f"ui-live: screenshots {out}", flush=True)
            while all(c.poll() is None for c in children):
                time.sleep(0.5)
            raise UiError("the game server or the driver exited")
    except UiError as e:
        print(f"ui-live: {e}", file=sys.stderr)
        return 1
    finally:
        for c in children:
            system.terminate(c)


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("ui-shots", help="screenshot UI states headless (ui/shots.json)")
    p.add_argument("only", nargs="*", help="shot names; all of them by default")
    p.add_argument(
        "--select",
        action="store_true",
        help="only the shots the branch's change can break, as many as fit (selection.py)",
    )
    p.add_argument(
        "--views",
        metavar="FIXTURE",
        help="list a fixture's views with the index each route's &at= takes",
    )
    p.set_defaults(func=ui_shots)
    p = sub.add_parser(
        "ui-test", help="play the game screen headless through a fixture"
    )
    p.add_argument(
        "only",
        nargs="*",
        help="run only the checks whose titles contain one of these (any case); a verb "
        "check by its name, verb:<verb>/<seat>/<layout>, or the start of it",
    )
    p.add_argument(
        "--select",
        action="store_true",
        help="only the checks the branch's change can break, as many as fit (selection.py)",
    )
    p.set_defaults(func=ui_test)
    p = sub.add_parser(
        "ui-live",
        help="live games in a headless page an agent drives over HTTP, until interrupted",
    )
    p.add_argument(
        "--port", type=int, default=0, help="the driver's port; any free one"
    )
    p.add_argument("--width", type=int, default=1600)
    p.add_argument("--height", type=int, default=900)
    p.add_argument(
        "--release", action="store_true", help="optimized agents (`just dev` is debug)"
    )
    p.set_defaults(func=ui_live)
