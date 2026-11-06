"""`by oracle-setup` and `by oracle-smoke`: XMage and Forge, the rules oracles that scenarios
are checked against (docs/engine-architecture.md, section 17.2).

`oracle-setup` installs Maven and openjdk@21 with Homebrew when they are missing, fetches each
oracle at the commit pinned below into ~/Library/Caches/beryl/oracles, and builds only the
modules the harnesses need, with JAVA_HOME on JDK 21 and a Maven repository of its own in the
cache. A checkout at its pin whose build stamp matches is left alone, so a second run is a quick
no-op. Logs go to oracles/logs; the command prints a JSON summary on stdout.

`oracle-smoke` runs one existing XMage test class and one Forge `sim` game of the Milestone 1
deck against itself.

`oracle` dumps every card scenario (`beryl_scenario dump`), plays them in XMage, as the JUnit
classes `xmage.py` exports into its checkout and runs with Maven, and in Forge, through the
exporter and harness of `forge.py`, and prints a JSON summary per oracle: each scenario passes,
fails, is on the divergence allowlist (data/oracle-divergences.json: where the oracle is wrong,
with the Comprehensive Rules citation that says so) or is unsupported (outside the exporter's
vocabulary, with the reason). It fails on any failure the allowlist does not explain. Logs and
the full results go to oracles/logs.
"""

from __future__ import annotations

import argparse
import fcntl
import itertools
import json
import os
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from collections.abc import Iterator, Sequence
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

from . import cards, forge, paths, xmage

FORMULAS = ("maven", "openjdk@21")


@dataclass(frozen=True)
class Oracle:
    name: str
    repo: str
    commit: str
    # Maven arguments that build what the harnesses need and install it into the cache's
    # Maven repository, so a harness can then run one module on its own.
    build: tuple[str, ...]


XMAGE = Oracle(
    name="xmage",
    repo="https://github.com/magefree/mage",
    commit="987fd4e9c73e9f113e235f8d8f98628f2d391667",
    # Mage.Tests and the modules it depends on: the engine, every card set, the server, the
    # two-player duel and the AI players.
    build=("-pl", "Mage.Tests", "-am", "install", "-DskipTests"),
)
FORGE = Oracle(
    name="forge",
    repo="https://github.com/Card-Forge/forge",
    commit="61e9442c433711a82258daca3236f936a1d6f70e",
    # The desktop module carries `sim` (forge.view.SimulateMatch) and the test harness; it
    # depends on forge-core, forge-game, forge-ai and forge-gui.
    build=("-pl", "forge-gui-desktop", "-am", "install", "-DskipTests"),
)
ORACLES: tuple[Oracle, ...] = (XMAGE, FORGE)

# The smoke's XMage test class, for a card Beryl implements.
XMAGE_SMOKE_TEST = "EliteSpellbinderTest"


def root() -> Path:
    path = paths.cache_root() / "oracles"
    path.mkdir(parents=True, exist_ok=True)
    return path


def checkout(oracle: Oracle) -> Path:
    return root() / oracle.name


def log_dir() -> Path:
    path = root() / "logs"
    path.mkdir(parents=True, exist_ok=True)
    return path


def stamp_path(oracle: Oracle) -> Path:
    return root() / f"{oracle.name}.built"


def stamp(oracle: Oracle) -> str:
    """What a finished build records: the commit and the build's arguments."""
    return " ".join((oracle.commit, *oracle.build))


@contextmanager
def locked() -> Iterator[None]:
    """One setup or smoke at a time: concurrent Maven builds would share one repository."""
    with open(root() / "oracles.lock", "w") as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


class StepFailed(Exception):
    pass


def run(
    argv: Sequence[str],
    log: Path,
    cwd: Path | None = None,
    check: bool = True,
    **env: str,
) -> str:
    """Runs a command with its output streaming into `log`, and returns that output. A failure
    raises StepFailed unless `check` is off."""
    started = time.monotonic()
    with log.open("a") as f:
        f.write(f"$ {' '.join(argv)}\n")
        f.flush()
        offset = f.tell()
        code = subprocess.run(
            list(argv),
            cwd=cwd,
            env={**os.environ, **env},
            stdin=subprocess.DEVNULL,
            stdout=f,
            stderr=subprocess.STDOUT,
            check=False,
        ).returncode
    with log.open() as f:
        f.seek(offset)
        output = f.read()
    with log.open("a") as f:
        f.write(f"exit {code} after {time.monotonic() - started:.0f} s\n\n")
    if code != 0 and check:
        tail = "\n".join(output.splitlines()[-30:])
        raise StepFailed(f"{' '.join(argv[:3])} failed; see {log}\n{tail}")
    return output


def brew_prefix(formula: str) -> Path | None:
    result = subprocess.run(
        ["brew", "--prefix", "--installed", formula],
        capture_output=True,
        text=True,
        check=False,
    )
    return Path(result.stdout.strip()) if result.returncode == 0 else None


def ensure_formulas(log: Path) -> list[str]:
    """Installs the missing formulas; returns the ones it installed."""
    if shutil.which("brew") is None:
        raise StepFailed("Homebrew is missing: https://brew.sh")
    missing = [f for f in FORMULAS if brew_prefix(f) is None]
    if missing:
        run(["brew", "install", *missing], log, HOMEBREW_NO_AUTO_UPDATE="1")
    return missing


def java_home() -> Path:
    prefix = brew_prefix("openjdk@21")
    if prefix is None:
        raise StepFailed("openjdk@21 is not installed; run `just oracle-setup`")
    return prefix / "libexec" / "openjdk.jdk" / "Contents" / "Home"


def mvn(*args: str) -> list[str]:
    """A Maven command line on the cache's repository, batch mode, no transfer progress."""
    return [
        "mvn",
        "-B",
        "--no-transfer-progress",
        f"-Dmaven.repo.local={root() / 'm2'}",
        *args,
    ]


def head(path: Path) -> str | None:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=path,
        capture_output=True,
        text=True,
        check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def fetch(oracle: Oracle, log: Path) -> bool:
    """Puts the checkout at its pinned commit with a depth-one fetch. Returns whether it had
    to fetch."""
    path = checkout(oracle)
    if (path / ".git").is_dir() and head(path) == oracle.commit:
        return False
    if not (path / ".git").is_dir():
        if path.exists():
            shutil.rmtree(path)
        run(["git", "init", "--quiet", str(path)], log)
    run(
        ["git", "fetch", "--quiet", "--depth", "1", oracle.repo, oracle.commit],
        log,
        path,
    )
    run(["git", "checkout", "--quiet", "--force", "--detach", oracle.commit], log, path)
    stamp_path(oracle).unlink(missing_ok=True)
    return True


def built(oracle: Oracle) -> bool:
    marker = stamp_path(oracle)
    return marker.is_file() and marker.read_text().strip() == stamp(oracle)


def build(oracle: Oracle, log: Path) -> bool:
    """Builds the oracle unless its stamp matches. Returns whether it built."""
    if built(oracle):
        return False
    run(mvn(*oracle.build), log, checkout(oracle), JAVA_HOME=str(java_home()))
    stamp_path(oracle).write_text(stamp(oracle) + "\n")
    return True


def configure_forge() -> None:
    """Points Forge's user and cache directories (preferences, decks, logs, downloaded data)
    into the cache instead of ~/Library/Application Support/Forge. Forge reads this file from
    its assets directory: the working directory for a build without "git" in its version, so
    `sim` runs in forge-gui (GuiDesktop.getAssetsDir)."""
    profile = checkout(FORGE) / "forge-gui" / "forge.profile.properties"
    user = root() / "forge-user"
    profile.write_text(f"userDir={user / 'data'}/\ncacheDir={user / 'cache'}/\n")


def setup(_args: argparse.Namespace) -> int:
    started = time.monotonic()
    summary: dict[str, object] = {"root": str(root())}
    try:
        with locked():
            summary["installed"] = ensure_formulas(log_dir() / "brew.log")
            summary["java_home"] = str(java_home())
            for oracle in ORACLES:
                log = log_dir() / f"{oracle.name}-setup.log"
                log.write_text("")
                step_started = time.monotonic()
                fetched = fetch(oracle, log)
                built = build(oracle, log)
                summary[oracle.name] = {
                    "commit": oracle.commit,
                    "path": str(checkout(oracle)),
                    "fetched": fetched,
                    "built": built,
                    "seconds": round(time.monotonic() - step_started),
                    "log": str(log),
                }
            configure_forge()
    except StepFailed as e:
        print(f"oracle-setup: {e}", file=sys.stderr)
        summary["error"] = str(e).splitlines()[0]
        print(json.dumps(summary, indent=2))
        return 1
    summary["seconds"] = round(time.monotonic() - started)
    print(json.dumps(summary, indent=2))
    return 0


def require_built(oracles: Sequence[Oracle] = ORACLES) -> None:
    stale = [o.name for o in oracles if not built(o)]
    if stale:
        raise StepFailed(
            f"{', '.join(stale)} not built at its pin; run `just oracle-setup`"
        )


def smoke_xmage(log: Path) -> str:
    run(
        mvn(
            "-pl",
            "Mage.Tests",
            f"-Dtest={XMAGE_SMOKE_TEST}",
            "-Dxmage.dataCollectors.printGameLogs=false",
            "test",
        ),
        log,
        checkout(XMAGE),
        JAVA_HOME=str(java_home()),
    )
    return XMAGE_SMOKE_TEST


def forge_deck(path: Path) -> Path:
    """The Milestone 1 main deck as a Forge .dck file: its `<count> <name>` lines up to the
    blank line before the sideboard, which is .dck's own line format."""
    lines = cards.DECK.read_text().splitlines()
    main = list(itertools.takewhile(str.strip, lines))
    header = ["[metadata]", "Name=Beryl M1 WU Control", "[Main]"]
    path.write_text("\n".join(header + main) + "\n")
    return path


def forge_jar() -> Path:
    target = checkout(FORGE) / "forge-gui-desktop" / "target"
    jars = list(target.glob("forge-gui-desktop-*-jar-with-dependencies.jar"))
    if not jars:
        raise StepFailed(f"no Forge desktop jar in {target}; run `just oracle-setup`")
    # A new pin builds a new version beside the old jar, and versions do not sort as text
    # (2.0.100 before 2.0.99): the newest jar is the one the last build wrote.
    return max(jars, key=lambda jar: jar.stat().st_mtime)


def sim_forge(log: Path, games: int) -> list[str]:
    """Forge's AI plays `games` games of the Milestone 1 deck against itself with `sim`;
    returns each game's result line."""
    deck = forge_deck(root() / "beryl-m1.dck")
    output = run(
        [
            str(java_home() / "bin" / "java"),
            "-Djava.awt.headless=true",
            "-jar",
            str(forge_jar()),
            "sim",
            # `sim` looks decks up by file name in the -D directory.
            "-D",
            str(deck.parent),
            "-d",
            deck.name,
            deck.name,
            "-n",
            str(games),
            "-s",
            "1",
            "-q",
        ],
        log,
        checkout(FORGE) / "forge-gui",
    )
    # SimulateMatch prints "Game Result: Game 1 ended in ... ms. <player> has won!" or
    # "Game Result: Game 1 ended in a Draw! ...", and exits 0 even when a deck fails to load.
    results = [l.strip() for l in output.splitlines() if l.startswith("Game Result:")]
    if len(results) != games:
        raise StepFailed(
            f"Forge sim finished {len(results)} of {games} games; see {log}"
        )
    return results


def smoke_forge(log: Path) -> str:
    return sim_forge(log, 1)[-1]


def smoke(args: argparse.Namespace) -> int:
    summary: dict[str, object] = {}
    failed = False
    with locked():
        try:
            require_built()
        except StepFailed as e:
            print(f"oracle-smoke: {e}", file=sys.stderr)
            return 1
        for name, step in (("xmage", smoke_xmage), ("forge", smoke_forge)):
            if args.only and name != args.only:
                continue
            log = log_dir() / f"{name}-smoke.log"
            log.write_text("")
            started = time.monotonic()
            result: dict[str, object] = {"ok": True}
            try:
                result["result"] = step(log)
            except StepFailed as e:
                print(f"oracle-smoke: {e}", file=sys.stderr)
                result["ok"] = False
                failed = True
            result["seconds"] = round(time.monotonic() - started)
            result["log"] = str(log)
            summary[name] = result
    print(json.dumps(summary, indent=2))
    return 1 if failed else 0


# Games of the deck mirror `oracle` has Forge's AI play.
SIM_GAMES = 3

DIVERGENCES: Path = paths.REPO / "data" / "oracle-divergences.json"


def dump_scenarios(path: Path, log: Path) -> list[dict[str, object]]:
    """Every card scenario, as `beryl_scenario dump` writes them."""
    run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "beryl_cards",
            "--bin",
            "beryl_scenario",
            "--",
            "dump",
            str(path),
        ],
        log,
        paths.RUST,
    )
    return json.loads(path.read_text())


def check_forge(entries: list[dict[str, object]], work: Path) -> dict[str, object]:
    """Plays the scenarios in Forge; the summary of `forge.summarize`."""
    exported, skipped = forge.export(entries, paths.REPO / "data" / "cards.json")
    scenarios = work / "forge-scenarios.json"
    scenarios.write_text(json.dumps(exported, indent=1))
    classes = root() / "forge-harness"
    forge.compile_harness(java_home(), forge_jar(), classes)
    results = forge.play(
        java_home(),
        forge_jar(),
        classes,
        checkout(FORGE) / "forge-gui",
        scenarios,
        work / "forge-results.json",
        log_dir() / "forge-oracle.log",
    )
    return forge.summarize(results, skipped, forge.divergences(DIVERGENCES, "forge"))


def xmage_tests() -> Path:
    """Where the exported test classes go in the XMage checkout."""
    return checkout(XMAGE) / "Mage.Tests/src/test/java/org/mage/test/beryl"


def reports() -> Path:
    return checkout(XMAGE) / "Mage.Tests" / "target" / "surefire-reports"


def surefire_results(classes: Sequence[str]) -> dict[tuple[str, str], str | None]:
    """Each test's outcome from Surefire's reports, by class and method: None for a pass, else
    the failure's message."""
    outcomes: dict[tuple[str, str], str | None] = {}
    for name in classes:
        path = reports() / f"TEST-{xmage.PACKAGE}.{name}.xml"
        if not path.is_file():
            continue
        for case in ET.parse(path).getroot().iter("testcase"):
            problem = next(
                (c for c in case if c.tag in ("failure", "error", "skipped")), None
            )
            message = None
            if problem is not None:
                text = (problem.get("message") or "").strip()
                message = text.splitlines()[0][:300] if text else problem.tag
            outcomes[(name, case.get("name", ""))] = message
    return outcomes


def check_xmage(entries: list[dict[str, object]], log: Path) -> dict[str, object]:
    """Exports the scenarios as XMage tests and runs them with Maven: one status per scenario
    (pass, fail, allowlisted, unsupported) and their counts, as `forge.summarize` reports.
    """
    faces = xmage.Faces.load(cards.OUTPUT)
    exports = xmage.export_all(entries, faces, xmage.xmage_cards(checkout(XMAGE)))
    classes = xmage.write(exports, xmage_tests())
    for old in reports().glob(f"TEST-{xmage.PACKAGE}.*.xml"):
        old.unlink()
    if classes:
        run(
            mvn(
                "-pl",
                "Mage.Tests",
                f"-Dtest={xmage.PACKAGE}.*BerylTest",
                "-Dsurefire.failIfNoSpecifiedTests=false",
                "-Dxmage.dataCollectors.printGameLogs=false",
                "test",
            ),
            log,
            checkout(XMAGE),
            check=False,
            JAVA_HOME=str(java_home()),
        )
    outcomes = surefire_results(classes)
    if classes and not outcomes:
        errors = [line for line in log.read_text().splitlines() if "[ERROR]" in line]
        detail = "\n".join(errors[:6])
        raise StepFailed(f"XMage ran no Beryl tests; see {log}\n{detail}")
    allowed = forge.divergences(DIVERGENCES, "xmage")
    scenarios: list[dict[str, object]] = []
    for export in exports:
        entry: dict[str, object] = {"card": export.card, "name": export.scenario}
        divergence = allowed.get(f"{export.card} / {export.scenario}")
        if export.unsupported is not None:
            entry["status"] = "unsupported"
            entry["reason"] = export.unsupported
        else:
            key = (xmage.class_name(export.card), export.method)
            entry["test"] = f"{key[0]}.{key[1]}"
            failure = outcomes.get(key, "no result: the test did not compile or run")
            if failure is None:
                entry["status"] = "pass"
                if divergence is not None:
                    entry["stale_allowlist"] = True
            elif divergence is not None:
                entry["status"] = "allowlisted"
                entry["cr"] = divergence.cr
                entry["reason"] = divergence.reason
                entry["failure"] = failure
            else:
                entry["status"] = "fail"
                entry["failure"] = failure
        if export.skipped:
            entry["skipped"] = export.skipped
        scenarios.append(entry)
    counts = {
        status: sum(1 for e in scenarios if e["status"] == status)
        for status in ("pass", "fail", "allowlisted", "unsupported")
    }
    stale = sum(1 for e in scenarios if e.get("stale_allowlist"))
    return {**counts, "stale_allowlist": stale, "scenarios": scenarios, "log": str(log)}


def oracle(args: argparse.Namespace) -> int:
    work = root() / "run"
    work.mkdir(exist_ok=True)
    try:
        with locked():
            require_built()
            entries = dump_scenarios(work / "scenarios.json", log_dir() / "dump.log")
            if args.cards:
                entries = [e for e in entries if e["card"] in args.cards]
            xmage_log = log_dir() / "xmage-oracle.log"
            xmage_log.write_text("")
            results: dict[str, dict[str, object]] = {
                "xmage": check_xmage(entries, xmage_log),
                "forge": check_forge(entries, work),
            }
            # A few AI-versus-AI games of the deck: Forge knows every card of it.
            sim_log = log_dir() / "forge-sim.log"
            sim_log.write_text("")
            results["forge"]["sim"] = sim_forge(sim_log, SIM_GAMES)
    except (StepFailed, RuntimeError, subprocess.CalledProcessError) as e:
        print(f"oracle: {e}", file=sys.stderr)
        return 1
    report = log_dir() / "oracle.json"
    report.write_text(json.dumps(results, indent=2))
    # Only what needs attention on stdout; the full report is in the log.
    summary: dict[str, object] = {
        name: {**result, "scenarios": forge.attention(result)}
        for name, result in results.items()
    }
    summary["report"] = str(report)
    print(json.dumps(summary, indent=2))
    return 1 if any(r["fail"] for r in results.values()) else 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "oracle-setup", help="install, fetch and build XMage and Forge in the cache"
    )
    p.set_defaults(func=setup)
    p = sub.add_parser(
        "oracle-smoke", help="run one XMage test class and one Forge sim game"
    )
    p.add_argument("only", nargs="?", choices=[o.name for o in ORACLES])
    p.set_defaults(func=smoke)
    p = sub.add_parser(
        "oracle",
        help="check every card scenario against the oracles, with a JSON summary",
    )
    p.add_argument("cards", nargs="*", help="only these cards' scenarios")
    p.set_defaults(func=oracle)
