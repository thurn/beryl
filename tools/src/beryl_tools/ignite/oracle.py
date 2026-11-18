"""The oracle check families, `oracle` and `oracle-generated`, which play Beryl's scenarios in
XMage and Forge and compare the results (docs/engine-architecture.md, section 17.2). The families
are declared in `ignite/checks/oracle.toml`.

Run from the repository's root as `python -m beryl_tools.ignite.oracle <verb> <family> ...`:

- `list oracle` prints one JSON line per scenario and engine: `{"check":
  "oracle:Trinket+Mage/Trinket+Mage+finds+nothing/xmage", "digest": "<hex>"}`. The scenarios are
  those `beryl_scenario dump --generated 100` writes, today's regression set: every card's own,
  those generated from card definitions, and the random ones of the first 100 seeds.
- `list oracle-generated` prints nothing, and `next oracle-generated` prints, in the same format, the random scenarios of a batch of seeds
  this machine has never yielded before: `{"check": "oracle-generated:1048577/forge", ...}`.
- `run <family> <check>...` plays the checks named and writes one JSON line per check that
  reached a verdict to the file named by `IGNITE_RESULTS`: `{"check", "outcome"}` and, for a
  failure or a pass with something to say, `message`. A check the oracles ended without judging
  gets no line, so Ignite records it as `incomplete`.

A check's `digest` is the input of its own that Ignite adds to its input hash. It is what
`oracle_memory.py` keys a scenario's remembered outcome on: a hash of the Java method and Forge
JSON the exporters prepared for it (or their reasons for refusing it), the oracles' pins and the
Java harnesses. Here it also covers the engine and the entries of `data/oracle-divergences.json`
and `data/oracle-exporter-gaps.json` that name the scenario for that engine, since they turn a
failure into a pass. Editing an exporter, a card or the rules therefore reruns only the
scenarios whose exported input changes.

The entries of those two files keep their meaning. A divergence that the oracle shows is a pass
whose message quotes the reason; a divergence entry for a scenario that passes is stale and
fails, as does an exporter-gap entry whose scenario passes or exports. An unsupported scenario,
one the exporter cannot express, is a pass whose message says why: Ignite has no skipped
outcome, and it is no evidence of a bug.

Each verb needs the oracles that `just oracle-setup` builds. Without them `list` and `next`
fail, which leaves the family `incomplete`, and `run` reports nothing.

Check names have no spaces, so a card's and a scenario's names are escaped as in a URL's query
(`quote_plus`), with `/` escaped too, since it separates the parts.

The oracle runner functions come from `oracles.py`, which imports no orchestration module at
load time; this module imports neither it nor any other module the switch to Ignite deletes.
"""

from __future__ import annotations

import hashlib
import json
import os
import secrets
import subprocess
import sys
import tempfile
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any, cast
from urllib.parse import quote_plus, unquote_plus

from .. import oracle_exports, oracles, paths, system

RESULTS_ENV = "IGNITE_RESULTS"
FAMILIES = ("oracle", "oracle-generated")
ENGINES = ("xmage", "forge")

# The card the dump files random scenarios under (`beryl_cards::generate::CARD`), and what
# each one's name starts with before its seed.
GENERATED = "Generated"
GENERATED_PREFIX = f"{GENERATED} "

# The seeds `next` tries in one batch. A seed whose script the runner refuses gives no
# scenario, so a batch yields a few fewer.
BATCH = 20

# The first seed a machine explores is a random multiple of this, so two machines explore
# different seeds, all far above the regression set's 1 to 100.
REGION = 1 << 20

# The files whose contents `oracle_memory.side` hashes besides the pins: the Java harnesses,
# one of which (XMage's base class) lives beside the Python tools. Copied here because the
# switch to Ignite deletes `oracle_memory`.
SIDE_GLOBS: tuple[str, ...] = (
    "tools/oracles/**/*.java",
    "tools/src/beryl_tools/BerylScenarioTest.java",
)

# The longest message a result carries.
MESSAGE_LIMIT = 300

# Characters that stay as they are in a check name; `+` and `/` are escaped.
SAFE = "!$&'()*,;:=@"


# --- Names ---


def check_name(family: str, entry: Mapping[str, object], engine: str) -> str:
    """The check that plays `entry` in `engine`."""
    card, name = cast(str, entry["card"]), cast(str, entry["name"])
    if family == "oracle-generated":
        return f"{family}:{name.removeprefix(GENERATED_PREFIX)}/{engine}"
    return (
        f"{family}:{quote_plus(card, safe=SAFE)}/{quote_plus(name, safe=SAFE)}/{engine}"
    )


def parse_name(check: str) -> tuple[str, str, str]:
    """The card, scenario and engine a check names."""
    family, _, rest = check.partition(":")
    parts = rest.split("/")
    if family == "oracle-generated" and len(parts) == 2:
        return GENERATED, f"{GENERATED_PREFIX}{parts[0]}", parts[1]
    if family == "oracle" and len(parts) == 3:
        return unquote_plus(parts[0]), unquote_plus(parts[1]), parts[2]
    raise ValueError(f"not a check of the oracle families: {check!r}")


# --- Digests ---


def side(repo: Path) -> str:
    """A hash of the oracles' side: their pinned commits and the Java harnesses."""
    digest = hashlib.sha256("\0".join(o.commit for o in oracles.ORACLES).encode())
    files = sorted({p for g in SIDE_GLOBS for p in repo.glob(g) if p.is_file()})
    for path in files:
        digest.update(path.relative_to(repo).as_posix().encode() + b"\0")
        digest.update(path.read_bytes())
    return digest.hexdigest()


def allowlisted(repo: Path) -> dict[tuple[str, str], list[dict[str, str]]]:
    """The entries of the divergence and exporter-gap lists, by engine and "card / scenario"."""
    found: dict[tuple[str, str], list[dict[str, str]]] = {}
    for name in ("oracle-divergences.json", "oracle-exporter-gaps.json"):
        entries = cast(
            list[dict[str, str]], json.loads((repo / "data" / name).read_text())
        )
        for e in entries:
            key = (e["oracle"], f"{e['card']} / {e['scenario']}")
            found.setdefault(key, []).append({"list": name, **e})
    return found


def digest(
    side_hash: str,
    export: str,
    engine: str,
    listed: Sequence[Mapping[str, str]],
) -> str:
    """A check's own input: the oracles' side, its scenario's exported input (as
    `oracle_exports.Exports.keys` hashes it), its engine and the list entries naming it.
    """
    text = json.dumps(
        ["ignite-oracle-v1", side_hash, export, engine, list(listed)], sort_keys=True
    )
    return hashlib.sha256(text.encode()).hexdigest()


def lines(
    family: str,
    entries: Sequence[Mapping[str, object]],
    exports: Mapping[str, str],
    repo: Path,
) -> list[dict[str, str]]:
    """The list lines of `entries`, each played in both engines."""
    side_hash = side(repo)
    listed = allowlisted(repo)
    out: list[dict[str, str]] = []
    for entry in entries:
        label = oracle_exports.label(entry)
        for engine in ENGINES:
            out.append(
                {
                    "check": check_name(family, entry, engine),
                    "digest": digest(
                        side_hash,
                        exports[label],
                        engine,
                        listed.get((engine, label), []),
                    ),
                }
            )
    return out


# --- The scenario dump ---


def binary(repo: Path) -> Path:
    """The `beryl_scenario` binary the `scenario-dump` build product makes."""
    return system.exe(repo / "rust" / "target" / "debug" / "beryl_scenario")


def dump(
    repo: Path, generated: int, seed: int | None = None
) -> list[dict[str, object]]:
    """The scenarios `beryl_scenario dump` writes, with the random ones of `generated` seeds
    from `seed` on (from the binary's own first seed when None)."""
    command = [str(binary(repo)), "dump"]
    with tempfile.TemporaryDirectory(prefix="beryl-ignite-oracle-") as work:
        path = Path(work) / "scenarios.json"
        command += [str(path), "--generated", str(generated)]
        if seed is not None:
            command += ["--seed", str(seed)]
        done = subprocess.run(command, capture_output=True, text=True)
        if done.returncode != 0:
            raise RuntimeError(f"{' '.join(command)} failed:\n{done.stderr}")
        return cast(list[dict[str, object]], json.loads(path.read_text()))


def exported(entries: list[dict[str, object]]) -> dict[str, str]:
    """Each entry's exported input, by label; needs the XMage checkout."""
    prepared = oracle_exports.Exports.prepare(entries, oracles.checkout(oracles.XMAGE))
    return prepared.keys()


# --- list and next ---


def list_checks(family: str, repo: Path) -> list[dict[str, str]]:
    if family != "oracle":
        return []  # An exploration family's checks come from `next`.
    oracles.require_built()
    entries = dump(repo, oracles.GENERATED)
    return lines(family, entries, exported(entries), repo)


def state_path() -> Path:
    path = paths.cache_root() / "ignite"
    path.mkdir(parents=True, exist_ok=True)
    return path / "oracle-generated.json"


def take_seeds(state: Path, count: int) -> int:
    """Reserves `count` seeds this machine has never yielded and returns the first. The state
    file records the next free seed; it is written before the seeds are used, so a run that
    stops midway never yields them again."""
    with system.file_lock(state.with_suffix(".lock")):
        start = (
            int(json.loads(state.read_text())["next"])
            if state.is_file()
            else (secrets.randbits(32) + 1) * REGION
        )
        state.write_text(json.dumps({"next": start + count}) + "\n")
    return start


def next_checks(
    family: str, repo: Path, state: Path, count: int = BATCH
) -> list[dict[str, str]]:
    if family != "oracle-generated":
        raise ValueError(
            f"{family} is a regression family: its checks come from `list`"
        )
    oracles.require_built()
    start = take_seeds(state, count)
    entries = [e for e in dump(repo, count, start) if e["card"] == GENERATED]
    return lines(family, entries, exported(entries), repo)


# --- run ---


def clip(text: str) -> str:
    text = " ".join(text.split())
    return text if len(text) <= MESSAGE_LIMIT else text[: MESSAGE_LIMIT - 1] + "…"


def selected(
    family: str, names: Sequence[str], repo: Path
) -> tuple[list[dict[str, object]], dict[str, set[str]]]:
    """The dumped scenarios the checks name, and the engines each label is named for."""
    wanted: dict[str, set[str]] = {}
    for name in names:
        card, scenario, engine = parse_name(name)
        wanted.setdefault(f"{card} / {scenario}", set()).add(engine)
    if family == "oracle-generated":
        seeds = [
            int(label.removeprefix(f"{GENERATED} / {GENERATED_PREFIX}"))
            for label in wanted
        ]
        entries = dump(repo, max(seeds) - min(seeds) + 1, min(seeds))
    else:
        entries = dump(repo, oracles.GENERATED)
    return [e for e in entries if oracle_exports.label(e) in wanted], wanted


def verdict(scenario: Mapping[str, object], stale_gap: bool) -> dict[str, str]:
    """The result line's outcome and message for one engine's report of a scenario."""
    status = scenario["status"]
    if stale_gap:
        return {
            "outcome": "fail",
            "message": "the scenario no longer fails here, so its entry in "
            "data/oracle-exporter-gaps.json is stale",
        }
    if status == "pass":
        if scenario.get("stale_allowlist"):
            return {
                "outcome": "fail",
                "message": "the scenario passes, so its entry in "
                "data/oracle-divergences.json is stale",
            }
        return {"outcome": "pass"}
    if status == "allowlisted":
        return {
            "outcome": "pass",
            "message": clip(
                f"known divergence ({scenario.get('cr', '')}): {scenario['reason']}"
            ),
        }
    if status == "unsupported":
        return {
            "outcome": "pass",
            "message": clip(f"unsupported: {scenario['reason']}"),
        }
    failure = scenario.get("failure") or scenario.get("failures") or "failed"
    if isinstance(failure, list):
        failure = "; ".join(str(f) for f in cast(list[object], failure))
    return {"outcome": "fail", "message": clip(str(failure))}


def report(
    family: str,
    engine: str,
    result: dict[str, object],
    wanted: Mapping[str, set[str]],
) -> list[dict[str, str]]:
    """The result lines of one engine's summary, for the checks named. Exporter gaps are
    applied first, as `by oracle` applies them."""
    stale = set(oracles.apply_gaps(engine, result))
    out: list[dict[str, str]] = []
    for s in cast(list[dict[str, object]], result["scenarios"]):
        label = oracle_exports.label(s)
        if engine in wanted.get(label, set()):
            out.append(
                {"check": check_name(family, s, engine), **verdict(s, label in stale)}
            )
    return out


def append(results: Path, found: Sequence[Mapping[str, str]]) -> None:
    with results.open("a") as out:
        for line in found:
            out.write(json.dumps(line) + "\n")


def run_checks(family: str, names: Sequence[str], repo: Path, results: Path) -> None:
    """Plays the checks named, one engine at a time, holding the oracles' lock: `by oracle`
    shares the checkouts until the switch. An engine that fails to run reports nothing.
    """
    oracles.require_built()
    entries, wanted = selected(family, names, repo)
    if not entries:
        return
    exports = oracle_exports.Exports.prepare(entries, oracles.checkout(oracles.XMAGE))
    engines = {engine for want in wanted.values() for engine in want}
    work = oracles.root() / "ignite"
    work.mkdir(exist_ok=True)
    with oracles.locked():
        for engine in ENGINES:
            if engine not in engines:
                continue
            try:
                if engine == "xmage":
                    log = oracles.log_dir() / "ignite-xmage.log"
                    log.write_text("")
                    result = oracles.check_xmage(exports, log)
                else:
                    result = oracles.check_forge(exports, work)
            except (
                oracles.StepFailed,
                RuntimeError,
                subprocess.CalledProcessError,
            ) as e:
                print(f"oracle: {engine}: {e}", file=sys.stderr)
                continue
            append(results, report(family, engine, result, wanted))


def main(argv: Sequence[str]) -> int:
    repo = Path.cwd()
    verbs = ("list", "next", "run")
    if len(argv) < 2 or argv[0] not in verbs or argv[1] not in FAMILIES:
        print(__doc__, file=sys.stderr)
        return 1
    verb, family, rest = argv[0], argv[1], list(argv[2:])
    try:
        if verb in ("list", "next"):
            found = (
                list_checks(family, repo)
                if verb == "list"
                else next_checks(family, repo, state_path())
            )
            for line in found:
                print(json.dumps(line))
            return 0
        results = os.environ.get(RESULTS_ENV)
        if not results:
            print(f"{RESULTS_ENV} is not set", file=sys.stderr)
            return 1
        run_checks(family, rest, repo, Path(results))
    except (oracles.StepFailed, RuntimeError, ValueError) as e:
        print(f"oracle: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
