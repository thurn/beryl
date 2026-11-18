"""The browser check families: `ui-test`, `shots`, `ui-smoke`, `motion` and `ui-counts`, which
play the UI headless against recorded game fixtures. The families are declared in
`ignite/checks/browser.toml`.

Run from the repository's root as `python -m beryl_tools.ignite.browser <verb> <family> ...`:

- `list <family>` prints one JSON line per check: `{"check": "ui-test:<title>", "inputs":
  [...]}`. The inputs are the paths that check reads, in addition to the family's own `inputs`
  in the manifest.
- `run <family> <check>...` runs the checks named and appends one JSON line per check that
  reached a verdict to the file named by `IGNITE_RESULTS`: `{"check", "outcome"}` and, for a
  failure, `message` and, where one is known, `at` (`ui/src/stage/phone.ts:212`). A check the
  run ended without judging (the fixtures failed to record, Vite never served the UI, a script
  crashed) gets no line, so Ignite records it as `incomplete`.

The checks, by family:

- `ui-test:<name>`: a check of ui/scripts/kept.mjs by its title, of verbs.mjs
  (`ui-test:verb:<verb>/<seat>/<layout>`) or of shapes.mjs (`ui-test:shape:...`). Each script
  prints its own checks with `--list` and reports each result through `recordResult`
  (ui/scripts/capture.mjs), which writes Ignite's line when `IGNITE_RESULTS` is set. A check's
  own inputs are the modules its script imports, directly or not, and the files they read.
- `ui-smoke:<title>`: the kept checks slow CI plays today (`suites.SMOKE_TITLES`).
- `shots:<name>`: one shot of ui/shots.json, which already lists each phone shot once per
  phone size (ui/scripts/shots.mjs).
- `motion:<verb>/<layout>`: the stepped capture (ui/scripts/stepped.mjs) of each verb of
  ui/verbs.json, held to its values by `motion_measure`, as `by motion` holds it.
- `ui-counts:<fixture>`: the work the UI does to play a fixture of data/ui-counts.json in each
  of its layouts (ui/scripts/counts.mjs), held to the limits there within their tolerance.

Each run records the fixtures it plays with `beryl_cli record-fixture` (the `cli` build
product) into a folder of its own and serves ui/ with Vite (the `ui-deps` product). It holds
no browser slot (ui/scripts/slots.mjs): Ignite holds the `browser-pages` resource for it. The
serving and recording are those of shots.py, copied here because an adapter must not import
the orchestration (`checks`, `warm`) that shots.py does.
"""

from __future__ import annotations

import base64
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from collections.abc import Iterable, Iterator, Sequence
from contextlib import contextmanager
from pathlib import Path
from typing import Any

from .. import conventions, motion_measure, paths, suites, system

RESULTS_ENV = "IGNITE_RESULTS"
# The file of check names, one a line, that the scripts run exactly (selection.py's CHOSEN_ENV).
CHOSEN_ENV = "BERYL_CHECKS"
FAMILIES = ("ui-test", "shots", "ui-smoke", "motion", "ui-counts")

KEPT_SCRIPT = "ui/scripts/kept.mjs"
VERB_SCRIPT: str = suites.VERB_SCRIPT
SHAPE_SCRIPT: str = suites.SHAPE_SCRIPT
SHOTS_SCRIPT = "ui/scripts/shots.mjs"
COUNTS_SCRIPT = "ui/scripts/counts.mjs"
SHOTS_FILE = "ui/shots.json"
FIXTURES_FILE = "ui/fixtures.json"
COUNTS_FILE = "data/ui-counts.json"
# What each ui-test script names its checks with, before Ignite's `ui-test:` prefix: a kept
# check's run-log id already has the prefix, a verb's and a shape's do not.
SCRIPT_PREFIX: dict[str, str] = {
    KEPT_SCRIPT: "ui-test:",
    VERB_SCRIPT: "",
    SHAPE_SCRIPT: "",
}
# The data files each script reads that it does not import, beside those `script_inputs` finds.
SCRIPT_DATA: dict[str, list[str]] = {
    KEPT_SCRIPT: [FIXTURES_FILE],
    VERB_SCRIPT: [suites.VERBS, SHOTS_FILE],
    SHAPE_SCRIPT: [suites.SHAPES, SHOTS_FILE],
}
# The metrics of a ui-counts play held to their limits (ui_counts.py GATED_METRICS).
GATED_METRICS = ("renders", "mutations", "styled", "laidOut")
# Real seconds after which Vite is taken to be hung: far above any start on a loaded machine.
SERVER_TIMEOUT = 300.0
MESSAGE_LIMIT = 300


class UiError(Exception):
    """Serving the UI, recording the fixtures or installing the browser failed."""


# --- What a script reads ---

# A module a script imports, or a file it reads relative to itself
# (`new URL("../verbs.json", import.meta.url)`).
SCRIPT_REFERENCE: re.Pattern[str] = re.compile(
    r"""(?:\bfrom\s+|\bimport\s*\(\s*|\bimport\s+|new URL\(\s*)["'](\.{1,2}/[^"']+)["']"""
)


def script_inputs(script: str, root: Path) -> list[str]:
    """The paths, from `root`, that the script `script` reads: itself, every module of the
    repository it imports directly or not, and the files they open relative to themselves.
    """
    seen: set[str] = set()
    todo = [script]
    while todo:
        name = todo.pop()
        file = root / name
        if name in seen or not file.is_file():
            continue
        seen.add(name)
        if file.suffix not in (".mjs", ".js"):
            continue
        for found in SCRIPT_REFERENCE.finditer(file.read_text()):
            target = os.path.normpath(Path(name).parent / found.group(1))
            todo.append(Path(target).as_posix())
    return sorted(seen)


def check_inputs(script: str, root: Path) -> list[str]:
    return sorted({*script_inputs(script, root), *SCRIPT_DATA.get(script, [])})


# --- Recording, serving and the browser (shots.py's, without the orchestration) ---


def browser_env(browser: str | None = None) -> dict[str, str]:
    """The environment for the capture scripts, with `browser` (by default the app webview's
    engine on this platform) installed for Playwright in the machine's cache."""
    browser = browser or ("chromium" if system.WINDOWS else "webkit")
    env = {
        **os.environ,
        "PLAYWRIGHT_BROWSERS_PATH": str(paths.cache_root() / "playwright"),
        "BERYL_BROWSER": browser,
    }
    # Ignite holds the machine's browser pages, so no run takes ui/scripts/slots.mjs' slots.
    env.pop("BERYL_BROWSER_SLOTS_DIR", None)
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
            f"{install.stdout}{install.stderr}installing Playwright's {browser} failed"
        )
    return env


def record_fixtures(into: Path, only: Sequence[str] = ()) -> None:
    """Plays the seeded games of ui/fixtures.json (those named in `only`, or all) and records
    them into `into` with the `beryl_cli` the `cli` build product made."""
    shutil.rmtree(into, ignore_errors=True)
    cli = system.exe(paths.RUN_TARGET / "debug" / "beryl_cli")
    done = subprocess.run(
        [str(cli), "record-fixture", str(paths.REPO / FIXTURES_FILE), str(into)]
        + list(only),
        cwd=paths.RUST,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:
        raise UiError(f"{done.stdout}{done.stderr}recording the fixtures failed")


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def wait_for_server(url: str, server: subprocess.Popen[bytes]) -> bool:
    """Polls until Vite answers, it exits, or `SERVER_TIMEOUT` passes."""
    end = time.monotonic() + SERVER_TIMEOUT
    while time.monotonic() < end and server.poll() is None:
        try:
            with urllib.request.urlopen(url, timeout=2):
                return True
        except (urllib.error.URLError, OSError):
            time.sleep(0.1)
    return False


@contextmanager
def served_ui(env: dict[str, str]) -> Iterator[str]:
    """Serves ui/ with Vite on a free port for the duration, playing the tree as it was when the
    server started (`BERYL_UI_FIXED`, ui/vite.config.ts); yields its base URL."""
    env = {**env, "BERYL_UI_FIXED": "1", "BERYL_PARENT_PID": str(os.getpid())}
    url = f"http://127.0.0.1:{free_port()}/"
    port = url.rsplit(":", 1)[1].strip("/")
    with tempfile.TemporaryFile() as log:
        server = subprocess.Popen(
            [system.npm_bin("vite"), "--host", "127.0.0.1", "--port", port],
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
                    log.read().decode(errors="replace")
                    + f"Vite did not serve {url} in {SERVER_TIMEOUT:.0f} s"
                )
            yield url
        finally:
            system.terminate(server)
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()


def node(args: Sequence[str], env: dict[str, str] | None = None) -> int:
    """Runs a script of ui/scripts in ui/, its output passed through for the run's log."""
    return subprocess.run(
        ["node", *args],
        cwd=paths.UI,
        env=env,
        stdin=subprocess.DEVNULL,
        check=False,
    ).returncode


def node_lines(args: Sequence[str]) -> list[dict[str, Any]]:
    """The JSON lines a script's `--list` prints. A script that fails raises, which leaves the
    family `incomplete`."""
    done = subprocess.run(
        ["node", *args],
        cwd=paths.UI,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:
        raise UiError(f"{done.stdout}{done.stderr}`node {' '.join(args)}` failed")
    return [
        json.loads(line) for line in done.stdout.splitlines() if line.startswith("{")
    ]


# --- list ---


def ui_test_fixtures() -> list[str]:
    """The fixtures `ui-test` records: kept.mjs', and the verb and shape suites'."""
    fixtures = suites.verb_fixtures() + suites.shape_fixtures()
    return list(dict.fromkeys(suites.KEPT_FIXTURES + fixtures))


def ui_test_checks(root: Path) -> list[dict[str, Any]]:
    """Every check of the three ui-test scripts. kept.mjs' checks derive their pages from the
    recorded fixtures as they register, so listing them records those first."""
    out: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="ignite-ui-test-list-") as tmp:
        recorded = Path(tmp) / "fixtures"
        record_fixtures(recorded, suites.KEPT_FIXTURES)
        listed = {
            KEPT_SCRIPT: node_lines([str(root / KEPT_SCRIPT), "--list", str(recorded)]),
            VERB_SCRIPT: node_lines([str(root / VERB_SCRIPT), "--list"]),
            SHAPE_SCRIPT: node_lines([str(root / SHAPE_SCRIPT), "--list"]),
        }
    for script, lines in listed.items():
        inputs = check_inputs(script, root)
        prefix = SCRIPT_PREFIX[script]
        for line in lines:
            name = str(line["check"]).removeprefix(prefix)
            out.append({"check": f"ui-test:{name}", "inputs": inputs})
    return out


def counts_gate(root: Path) -> dict[str, Any]:
    return json.loads((root / COUNTS_FILE).read_text())


def list_checks(family: str, root: Path) -> list[dict[str, Any]]:
    """The family's checks with the inputs of their own."""
    if family == "ui-test":
        return ui_test_checks(root)
    if family == "ui-smoke":
        inputs = check_inputs(KEPT_SCRIPT, root)
        return [
            {"check": f"ui-smoke:{t}", "inputs": inputs} for t in suites.SMOKE_TITLES
        ]
    if family == "shots":
        lines = node_lines([str(root / SHOTS_SCRIPT), "--list", str(root / SHOTS_FILE)])
        return [{"check": str(line["check"]), "inputs": []} for line in lines]
    if family == "motion":
        verbs = suites.verb_suite().get("verbs")
        return [
            {"check": f"motion:{verb}/{layout}", "inputs": []}
            for layout in motion_measure.LAYOUTS
            for verb in (verbs if isinstance(verbs, list) else [])
        ]
    if family == "ui-counts":
        return [
            {"check": f"ui-counts:{fixture}", "inputs": []}
            for fixture in counts_gate(root)["fixtures"]
        ]
    raise ValueError(f"unknown family {family!r}")


# --- run ---


def clip(text: str) -> str:
    text = " ".join(text.split())
    return text if len(text) <= MESSAGE_LIMIT else text[: MESSAGE_LIMIT - 1] + "…"


def append(results: Path, lines: Iterable[dict[str, Any]]) -> None:
    with results.open("a") as out:
        for line in lines:
            out.write(json.dumps(line) + "\n")


def reported(file: Path) -> list[dict[str, Any]]:
    """The lines a script wrote to its own results file; none if it wrote none."""
    if not file.exists():
        return []
    return [json.loads(line) for line in file.read_text().splitlines() if line.strip()]


# The inline source map Vite's dev server ends a module it transformed with.
INLINE_MAP = "//# sourceMappingURL=data:application/json;base64,"
BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
# A place the scripts wrote in the UI's source, as a page ran it: `ui/src/<path>:<line>`.
SERVED_PLACE: re.Pattern[str] = re.compile(r"\bui/(src/[^\s:()?]+):(\d+)")


def vlq(segment: str) -> list[int]:
    """The numbers of one segment of a source map's `mappings` (base64 VLQ)."""
    out: list[int] = []
    value = shift = 0
    for char in segment:
        digit = BASE64.index(char)
        value += (digit & 31) << shift
        if digit & 32:
            shift += 5
            continue
        out.append(-(value >> 1) if value & 1 else value >> 1)
        value = shift = 0
    return out


def original_lines(mappings: str) -> list[int | None]:
    """For each line of a generated module (from 0), the line of its source (from 0) that its
    first mapped segment comes from; None for a line with none."""
    lines: list[int | None] = []
    source_line = 0
    for generated in mappings.split(";"):
        first: int | None = None
        for segment in filter(None, generated.split(",")):
            numbers = vlq(segment)
            if len(numbers) >= 4:
                source_line += numbers[2]
                if first is None:
                    first = source_line
        lines.append(first)
    return lines


def source_line(module: str, line: int) -> int | None:
    """The line of the source file that line `line` (from 1) of `module`, a module's text as the
    dev server served it, was made from; None if the module has no inline map for it."""
    at = module.rfind(INLINE_MAP)
    if at < 0:
        return None
    encoded = module[at + len(INLINE_MAP) :].split()[0]
    mapping = json.loads(base64.b64decode(encoded))
    lines = original_lines(str(mapping.get("mappings", "")))
    found = lines[line - 1] if 0 < line <= len(lines) else None
    return None if found is None else found + 1


def in_source(result: dict[str, Any], url: str) -> dict[str, Any]:
    """`result` with each place in the UI's source it names moved from the line of the module
    Vite served the page (which its transform shifts) to the line of the file itself, asked of
    the dev server at `url` while it still serves. A place it cannot map stays as it was.
    """
    modules: dict[str, str | None] = {}

    def mapped(found: re.Match[str]) -> str:
        path, line = found.group(1), int(found.group(2))
        if path not in modules:
            try:
                with urllib.request.urlopen(url + path, timeout=10) as reply:
                    modules[path] = reply.read().decode(errors="replace")
            except (urllib.error.URLError, OSError):
                modules[path] = None
        module = modules[path]
        real = source_line(module, line) if module is not None else None
        return f"ui/{path}:{real or line}"

    return {
        key: SERVED_PLACE.sub(mapped, value) if key in ("at", "message") else value
        for key, value in result.items()
    }


def play_scripts(
    asked: dict[str, list[str]],
    recorded: Path,
    env: dict[str, str],
    tmp: Path,
) -> dict[str, dict[str, Any]]:
    """Plays each script of `asked` on exactly the checks named for it (by the names the script
    gives them), on a UI served once; each result line by its script's name for the check.
    Each script writes to a results file of its own, so a line for a check nobody asked for
    (none should come) never reaches Ignite's."""
    found: dict[str, dict[str, Any]] = {}
    with served_ui(env) as url:
        for script, names in asked.items():
            if not names:
                continue
            chosen = tmp / f"{Path(script).stem}-chosen.txt"
            chosen.write_text("".join(f"{n}\n" for n in names))
            own = tmp / f"{Path(script).stem}-results.jsonl"
            node(
                [str(paths.REPO / script), url, str(recorded)],
                {**env, CHOSEN_ENV: str(chosen), RESULTS_ENV: str(own)},
            )
            for line in reported(own):
                found[str(line["check"])] = in_source(line, url)
    return found


def run_ui_test(family: str, names: Sequence[str], results: Path) -> None:
    """Runs ui-test's checks, or ui-smoke's (kept checks under another name)."""
    asked: dict[str, list[str]] = {KEPT_SCRIPT: [], VERB_SCRIPT: [], SHAPE_SCRIPT: []}
    script_name: dict[str, str] = {}  # The script's name for each check asked.
    for name in names:
        bare = name.removeprefix(f"{family}:")
        if bare.startswith(suites.VERB_PREFIX):
            script = VERB_SCRIPT
        elif bare.startswith(suites.SHAPE_PREFIX):
            script = SHAPE_SCRIPT
        else:
            script = KEPT_SCRIPT
        # kept.mjs' chosen titles are bare; its results carry the `ui-test:` prefix.
        asked[script].append(bare)
        script_name[name] = f"{SCRIPT_PREFIX[script]}{bare}"
    env = browser_env()
    with tempfile.TemporaryDirectory(prefix=f"ignite-{family}-") as tmp:
        recorded = Path(tmp) / "fixtures"
        record_fixtures(recorded, ui_test_fixtures())
        found = play_scripts(asked, recorded, env, Path(tmp))
    append(
        results,
        (
            {**found[script_name[name]], "check": name}
            for name in names
            if script_name[name] in found
        ),
    )


def run_shots(names: Sequence[str], results: Path) -> None:
    """Takes the shots named. The screenshots go to the cache, as `by ui-shots` puts them."""
    out = paths.cache_root() / "ui-shots" / paths.REPO.name
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    env = browser_env()
    wanted = [n.removeprefix("shots:") for n in names]
    with tempfile.TemporaryDirectory(prefix="ignite-shots-") as tmp:
        # Shots open fixtures by URL from what Vite serves.
        record_fixtures(paths.UI / "public" / "fixtures")
        own = Path(tmp) / "results.jsonl"
        with served_ui(env) as url:
            node(
                [
                    str(paths.REPO / SHOTS_SCRIPT),
                    url,
                    str(out),
                    SHOTS_FILE.removeprefix("ui/"),
                ]
                + wanted,
                {**env, RESULTS_ENV: str(own)},
            )
            found = {str(line["check"]): in_source(line, url) for line in reported(own)}
    append(results, (found[n] for n in names if n in found))


def run_motion(names: Sequence[str], results: Path) -> None:
    """Captures the verb suite in stepped stage time and holds each verb named to its values.
    The moments of `--more` are played only when a verb asked for is played by them alone.
    """
    data = conventions.load()
    reference = motion_measure.motion()
    asked = [n.removeprefix("motion:").rsplit("/", 1) for n in names]
    more = any(verb in motion_measure.MORE_ONLY for verb, _ in asked)
    env = browser_env()
    out = paths.cache_root() / "motion" / paths.REPO.name
    shutil.rmtree(out / "frames", ignore_errors=True)
    recorded = out / "fixtures"
    record_fixtures(recorded, suites.verb_fixtures())
    with served_ui(env) as url:
        code = node(
            [str(paths.REPO / motion_measure.CAPTURE_SCRIPT), url, str(recorded)]
            + [str(out / "frames")]
            + (["--more"] if more else []),
            env,
        )
    if code != 0:
        raise UiError("the stepped capture failed")
    measured = motion_measure.measure_capture(data, reference, out / "frames")
    exempt = motion_measure.exempted(data)
    lines: list[dict[str, Any]] = []
    for name, (verb, layout) in zip(names, asked):
        mine = [
            vs
            for v, step, vs in measured
            if v == verb and step.startswith(f"{layout}/")
        ]
        if not mine and verb in exempt:
            lines.append(
                {
                    "check": name,
                    "outcome": "pass",
                    "message": f"exempt (exception {exempt[verb]}), not played",
                }
            )
        elif not mine:
            message = f"{layout}: the stage played no {verb} to capture"
            lines.append({"check": name, "outcome": "fail", "message": message})
        else:
            bad = [v.line() for vs in mine for v in vs if v.failed]
            lines.append(
                {"check": name, "outcome": "fail", "message": clip("; ".join(bad))}
                if bad
                else {"check": name, "outcome": "pass"}
            )
    append(results, lines)


def judge_counts(
    fixture: str,
    gate: dict[str, Any],
    measured: dict[str, dict[str, int]],
) -> dict[str, Any] | None:
    """The verdict on one fixture's counts in every layout of the gate: a fail for a count that
    rose more than the tolerance above its limit, or a layout with no limit; None if a layout
    was not measured, which leaves the check incomplete."""
    tolerance = float(gate["tolerance_percent"])
    over: list[str] = []
    for layout in gate["layouts"]:
        key = f"{fixture}/{layout}"
        counts = measured.get(key)
        limits = gate["limits"].get(key)
        if counts is None:
            return None
        if limits is None:
            over.append(f"{key}: no limits in {COUNTS_FILE}")
            continue
        for metric in GATED_METRICS:
            limit, count = int(limits[metric]), int(counts[metric])
            change = 100.0 * (count - limit) / limit if limit else 0.0
            if change > tolerance:
                over.append(
                    f"{key} {metric}: {count:,} against {limit:,} ({change:+.1f}%),"
                    f" over the {tolerance}% tolerance"
                )
    if over:
        return {"outcome": "fail", "message": clip("; ".join(over))}
    return {"outcome": "pass"}


def run_counts(names: Sequence[str], root: Path, results: Path) -> None:
    """Plays the fixtures named in every layout of data/ui-counts.json and holds each fixture's
    counts to its limits there."""
    gate = counts_gate(root)
    fixtures = [n.removeprefix("ui-counts:") for n in names]
    # Only Chromium reports the style and layout counts (ui/scripts/counts.mjs).
    env = browser_env("chromium")
    with tempfile.TemporaryDirectory(prefix="ignite-ui-counts-") as tmp:
        recorded = Path(tmp) / "fixtures"
        record_fixtures(recorded, fixtures)
        out = Path(tmp) / "counts.json"
        with served_ui(env) as url:
            node(
                [str(paths.REPO / COUNTS_SCRIPT), url, str(recorded)]
                + ["--json", str(out), "--layouts", ",".join(gate["layouts"])]
                + fixtures,
                env,
            )
        measured = json.loads(out.read_text()) if out.exists() else {}
    lines: list[dict[str, Any]] = []
    for name, fixture in zip(names, fixtures):
        verdict = judge_counts(fixture, gate, measured)
        if verdict is not None:
            lines.append({"check": name, **verdict})
    append(results, lines)


def run_checks(family: str, names: Sequence[str], root: Path, results: Path) -> None:
    if family in ("ui-test", "ui-smoke"):
        run_ui_test(family, names, results)
    elif family == "shots":
        run_shots(names, results)
    elif family == "motion":
        run_motion(names, results)
    elif family == "ui-counts":
        run_counts(names, root, results)
    else:
        raise ValueError(f"unknown family {family!r}")


def main(argv: Sequence[str]) -> int:
    root = Path.cwd()
    if len(argv) < 2 or argv[0] not in ("list", "run") or argv[1] not in FAMILIES:
        print(__doc__, file=sys.stderr)
        return 1
    verb, family, rest = argv[0], argv[1], list(argv[2:])
    try:
        if verb == "list":
            for line in list_checks(family, root):
                print(json.dumps(line))
            return 0
        results = os.environ.get(RESULTS_ENV)
        if not results:
            print(f"{RESULTS_ENV} is not set", file=sys.stderr)
            return 1
        if rest:
            run_checks(family, rest, root, Path(results))
        return 0
    except UiError as e:
        print(f"{family}: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
