"""`by oracle-setup` and `by oracle-smoke`: XMage and Forge, the rules oracles that scenarios
are checked against (docs/engine-architecture.md, section 17.2).

`oracle-setup` installs Maven and openjdk@21 with Homebrew when they are missing (on Windows,
the Microsoft Build of OpenJDK 21 with winget, and Maven from Apache into the cache), fetches each
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
the full results go to oracles/logs. Forge's AI also plays a few games of the deck, unless
`--no-sim`. With `--lock-wait`, a run that finds another holding the oracles that long skips
every scenario and passes: the fast check's oracle lane for changed cards does that.
"""

from __future__ import annotations

import argparse
import itertools
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
from collections.abc import Iterator, Sequence
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from . import card_store, cards, checks, forge, oracle_selection, paths, runlog, system
from . import card_diverge, xmage, xmage_java

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
def locked(timeout: float | None = None) -> Iterator[None]:
    """One setup, smoke or oracle run at a time: concurrent Maven builds would share one
    repository, and a run writes its XMage tests and their reports into the shared checkout.
    Waits forever, or at most `timeout` seconds, then raises system.LockHeld."""
    with system.file_lock(root() / "oracles.lock", timeout):
        yield


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


# Windows: Maven from Apache, unpacked into the cache; JDK 21 from winget.
MAVEN_VERSION = "3.9.11"
MAVEN_URL = (
    f"https://archive.apache.org/dist/maven/maven-3/{MAVEN_VERSION}/binaries/"
    f"apache-maven-{MAVEN_VERSION}-bin.zip"
)
WINGET_JDK = "Microsoft.OpenJDK.21"


def windows_jdk() -> Path | None:
    """The newest JDK 21 winget installed."""
    homes = sorted(Path(r"C:\Program Files\Microsoft").glob("jdk-21*"))
    return homes[-1] if homes else None


def windows_maven() -> Path:
    return root() / f"apache-maven-{MAVEN_VERSION}"


def ensure_windows_tools(log: Path) -> list[str]:
    """Installs JDK 21 and Maven where missing; returns the ones it installed."""
    installed = []
    if windows_jdk() is None:
        run(
            ["winget", "install", "--exact", "--id", WINGET_JDK, "--silent"]
            + ["--accept-package-agreements", "--accept-source-agreements"],
            log,
        )
        installed.append(WINGET_JDK)
    if not windows_maven().is_dir():
        import io
        import urllib.request
        import zipfile

        with urllib.request.urlopen(MAVEN_URL, timeout=120) as response:
            zipfile.ZipFile(io.BytesIO(response.read())).extractall(root())
        installed.append(f"maven {MAVEN_VERSION}")
    return installed


def ensure_formulas(log: Path) -> list[str]:
    """Installs the missing formulas; returns the ones it installed."""
    if system.WINDOWS:
        return ensure_windows_tools(log)
    if shutil.which("brew") is None:
        raise StepFailed("Homebrew is missing: https://brew.sh")
    missing = [f for f in FORMULAS if brew_prefix(f) is None]
    if missing:
        run(["brew", "install", *missing], log, HOMEBREW_NO_AUTO_UPDATE="1")
    return missing


def java_home() -> Path:
    if system.WINDOWS:
        home = windows_jdk()
        if home is None:
            raise StepFailed("JDK 21 is not installed; run `just oracle-setup`")
        return home
    prefix = brew_prefix("openjdk@21")
    if prefix is None:
        raise StepFailed("openjdk@21 is not installed; run `just oracle-setup`")
    return prefix / "libexec" / "openjdk.jdk" / "Contents" / "Home"


def mvn(*args: str) -> list[str]:
    """A Maven command line on the cache's repository, batch mode, no transfer progress."""
    maven = str(windows_maven() / "bin" / "mvn.cmd") if system.WINDOWS else "mvn"
    return [
        maven,
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
    # Forward slashes: a properties file reads a backslash as an escape.
    user = (root() / "forge-user").as_posix()
    profile.write_text(f"userDir={user}/data/\ncacheDir={user}/cache/\n", newline="\n")


def ensure_rosetta() -> None:
    """Forge's desktop build wraps its jar with launch4j, whose macOS `windres` is an x86-64
    binary: on Apple silicon it runs only under Rosetta. Fails before the long Maven build.
    """
    if sys.platform != "darwin" or platform.machine() != "arm64" or built(FORGE):
        return
    probe = subprocess.run(["arch", "-x86_64", "/usr/bin/true"], capture_output=True)
    if probe.returncode != 0:
        raise StepFailed(
            "Forge's build needs Rosetta: "
            "softwareupdate --install-rosetta --agree-to-license"
        )


def setup(_args: argparse.Namespace) -> int:
    started = time.monotonic()
    summary: dict[str, object] = {"root": str(root())}
    try:
        with locked():
            ensure_rosetta()
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
    path.write_text("\n".join(header + main) + "\n", newline="\n")
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


# Generated scenarios the dump adds to the cards' own (architecture section 17.2): random
# positions played by random choices, filed under the card "Generated".
GENERATED = 100

# Generated scenarios an oracle fails for a gap in its exporter rather than a difference in the
# rules, by oracle, card and scenario, with the gap: reported as unsupported. An entry whose
# scenario passes when played is stale and fails the run, so the list stays current; a change
# to a generator renames what it covers. Both generators' scenarios may be listed: the random
# ones under the card "Generated", and those generated from a card's definition
# (`beryl_cards::derive`) under their card.
GAPS: Path = paths.REPO / "data" / "oracle-exporter-gaps.json"


def load_gaps(path: Path) -> dict[tuple[str, str, str], str]:
    """The exporter gaps listed in `path`, by oracle, card and scenario."""
    entries = cast(list[dict[str, str]], json.loads(path.read_text()))
    return {(e["oracle"], e["card"], e["scenario"]): e["gap"] for e in entries}


EXPORTER_GAPS: dict[tuple[str, str, str], str] = load_gaps(GAPS)


# What the name of each scenario generated from a card's definition starts with
# (`beryl_cards::derive::PREFIX`): the report marks them `generated`.
DERIVED_PREFIX = "generated "


def mark_generated(result: dict[str, object]) -> None:
    """Marks each scenario of `result` generated from its card's definition as `generated`."""
    for s in cast(list[dict[str, object]], result["scenarios"]):
        if cast(str, s["name"]).startswith(DERIVED_PREFIX):
            s["generated"] = True


def apply_gaps(oracle: str, result: dict[str, object]) -> list[str]:
    """Reports the scenarios of `result` that `EXPORTER_GAPS` lists for `oracle` as unsupported,
    recounting, and returns the stale entries: those whose scenario was played and did not
    fail."""
    scenarios = cast(list[dict[str, object]], result["scenarios"])
    gaps = {
        f"{card} / {name}": why
        for (o, card, name), why in EXPORTER_GAPS.items()
        if o == oracle
    }
    # Stale only among the scenarios played: a run for some cards plays no other card's.
    stale = {f"{s['card']} / {s['name']}" for s in scenarios} & set(gaps)
    for s in scenarios:
        key = f"{s['card']} / {s['name']}"
        why = gaps.get(key)
        if why is None or s["status"] != "fail":
            continue
        stale.discard(key)
        s["status"] = "unsupported"
        s["reason"] = f"exporter gap: {why}"
        s.pop("failures", None)
        s.pop("failure", None)
    for status in ("pass", "fail", "allowlisted", "unsupported"):
        result[status] = sum(1 for s in scenarios if s["status"] == status)
    result["stale_gaps"] = sorted(stale)
    return sorted(stale)


def dump_scenarios(path: Path, log: Path, cards: list[str]) -> list[card_store.Json]:
    """Every card scenario and the generated ones, as `beryl_scenario dump` writes them, and
    those generated for each of `cards` complete from the card store (store_cards.py).
    """
    run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "beryl_cards",
            "--features",
            "scenarios",
            "--bin",
            "beryl_scenario",
            "--",
            "dump",
            str(path),
            "--generated",
            str(GENERATED),
            *(arg for card in cards for arg in ("--card", card)),
        ],
        log,
        paths.RUST,
        CARGO_TARGET_DIR=str(paths.RUN_TARGET),
    )
    return json.loads(path.read_text())


def check_forge(entries: list[dict[str, object]], work: Path) -> dict[str, object]:
    """Plays the scenarios in Forge; the summary of `forge.summarize`."""
    exported, skipped = forge.export(entries, card_store.STORE)
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
        path = reports() / f"TEST-{xmage_java.PACKAGE}.{name}.xml"
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
    faces = xmage_java.Faces.load(card_store.STORE)
    exports = xmage.export_all(entries, faces, xmage_java.xmage_cards(checkout(XMAGE)))
    classes = xmage_java.write(exports, xmage_tests())
    for old in reports().glob(f"TEST-{xmage_java.PACKAGE}.*.xml"):
        old.unlink()
    if classes:
        run(
            mvn(
                "-pl",
                "Mage.Tests",
                f"-Dtest={xmage_java.PACKAGE}.*BerylTest",
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
            key = (xmage_java.class_name(export.card), export.method)
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


# How each scenario status counts in the run log: an allowlisted divergence fails no run.
RUN_LOG_OUTCOMES = {
    "pass": "pass",
    "allowlisted": "pass",
    "fail": "fail",
    "unsupported": "skip",
}


def log_results(
    results: dict[str, dict[str, object]],
    seconds: dict[str, float],
    chosen: dict[str, str] | None = None,
) -> None:
    """Reports each scenario of each oracle to the run log as `oracle:<card>/<scenario>/<engine>`,
    with why its card was chosen when cards were (`oracle_selection`). Neither oracle times a
    scenario, so each engine's seconds are shared evenly among those it played."""
    for engine, result in results.items():
        scenarios = cast(list[dict[str, object]], result["scenarios"])
        played = sum(1 for s in scenarios if s["status"] != "unsupported") or 1
        for s in scenarios:
            outcome = RUN_LOG_OUTCOMES.get(cast(str, s["status"]), "fail")
            runlog.result(
                f"oracle:{s['card']}/{s['name']}/{engine}",
                outcome,
                0.0 if outcome == "skip" else seconds[engine] / played,
                chosen.get(cast(str, s["card"])) if chosen is not None else None,
            )


def skip_all(entries: list[dict[str, object]], why: str) -> None:
    """Reports every scenario of `entries` as skipped in each oracle, saying why."""
    print(f"oracle: skipped: {why}")
    for e in entries:
        for engine in ("xmage", "forge"):
            runlog.result(f"oracle:{e['card']}/{e['name']}/{engine}", "skip", 0.0)


def oracle(args: argparse.Namespace) -> int:
    work = root() / "run"
    work.mkdir(exist_ok=True)
    # Whether the oracles and the scenarios built, so that a failure is the scenarios'.
    built = False
    try:
        require_built()
        # The dump needs no oracle, so it runs before the wait for the lock, into a file of its
        # own: two runs may dump at once.
        with tempfile.TemporaryDirectory(prefix="beryl-oracle-") as dump:
            entries = dump_scenarios(
                Path(dump) / "scenarios.json", log_dir() / "dump.log", args.cards
            )
        # Cards named with --select are a rerun's or a bisection's, narrowing the selection.
        every = sorted({cast(str, e["card"]) for e in entries})
        chosen = (
            oracle_selection.slow_ci(every) if args.select and not args.cards else None
        )
        if chosen is not None:
            print(f"oracle: chosen: {json.dumps(chosen)}")
        wanted = chosen or args.cards
        if wanted:
            entries = [e for e in entries if e["card"] in wanted]
        try:
            with locked(args.lock_wait):
                require_built()  # Again: a setup may have held the lock to rebuild one.
                built = True
                xmage_log = log_dir() / "xmage-oracle.log"
                xmage_log.write_text("")
                started = time.perf_counter()
                results: dict[str, dict[str, object]] = {
                    "xmage": check_xmage(entries, xmage_log)
                }
                seconds = {"xmage": time.perf_counter() - started}
                started = time.perf_counter()
                results["forge"] = check_forge(entries, work)
                seconds["forge"] = time.perf_counter() - started
                if not args.no_sim:
                    # A few AI-versus-AI games of the deck: Forge knows every card of it.
                    sim_log = log_dir() / "forge-sim.log"
                    sim_log.write_text("")
                    results["forge"]["sim"] = sim_forge(sim_log, SIM_GAMES)
                stale = [g for name, r in results.items() for g in apply_gaps(name, r)]
                for r in results.values():
                    mark_generated(r)
                log_results(results, seconds, chosen)
        except system.LockHeld:
            # Another run holds the oracles, as slow CI's oracle lane does for a minute or more.
            skip_all(
                entries, f"another oracle run held the lock for {args.lock_wait:g} s"
            )
            return 0
    except (StepFailed, RuntimeError, subprocess.CalledProcessError) as e:
        print(f"oracle: {e}", file=sys.stderr)
        return 1 if built else checks.CANNOT_RUN
    report = log_dir() / "oracle.json"
    report.write_text(json.dumps(results, indent=2))
    # Only what needs attention on stdout; the full report is in the log.
    summary: dict[str, object] = {
        name: {**result, "scenarios": forge.attention(result)}
        for name, result in results.items()
    }
    summary["report"] = str(report)
    print(json.dumps(summary, indent=2))
    return 1 if stale or card_diverge.failed(results) else 0


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
    p.add_argument(
        "--no-sim", action="store_true", help="skip Forge's AI games of the deck"
    )
    p.add_argument(
        "--select",
        action="store_true",
        help="only the cards slow CI chooses: those changed since origin/master and a "
        "sample, or every card when the exporters or harness changed",
    )
    p.add_argument(
        "--lock-wait",
        type=float,
        metavar="SECONDS",
        help="skip every scenario, passing, when another run holds the oracles this long",
    )
    p.set_defaults(func=oracle)
