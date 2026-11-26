"""The `counts`, `fuzz`, `exhaust`, `scale` and `ignite-lint` check families' adapter. The
families are declared in `ignite/checks/rest.toml`.

Run from the repository's root as `python -m beryl_tools.ignite.rest <verb> <family> ...`:

- `list <family>` prints one JSON line per check: `{"check": "<family>:<name>", "inputs": [...]}`.
  The inputs are the paths that check reads in addition to the family's own `inputs`.
- `next <family>` (the exploration families `fuzz` and `exhaust`) prints, in the same format, checks
  that have not been played: `fuzz` reserves a batch of seeds that no earlier batch used, and
  `exhaust` names every card, which Ignite's result cache then filters by what was played on the
  same inputs.
- `run <family> <check>...` runs the checks named and writes one JSON line per check that reached
  a verdict to the file named by `IGNITE_RESULTS`: `{"check", "outcome"}` and, for a failure,
  `message`. A check the tool ended without judging gets no line, so Ignite records it as
  `incomplete`. A run that cannot happen on this machine (the count gate's virtual machine is
  missing, say) writes an `incomplete` line whose message starts with `cannot-run:`.

The families:

- `counts`: `counts:rollouts` is the rollout benchmark's instruction count against its bar, and
  `counts:family/<name>` a family's present cost against its ratio, both in
  `engine/crates/beryl_bench/count-gate.toml`. The count runs in the `beryl-bench` virtual machine
  under Lima or WSL. After `counts:rollouts` a run in the `tip` class (`IGNITE_CLASS`)
  stores the verified count for its verifier and calibration and files `count-creep` and
  `count-lower` tickets through `ignite ticket`. It also closes untouched tickets when the
  count clears them. Ticket operations never change the check's outcome.
- `fuzz`: batches of random games, `fuzz:<target>/<first seed>+<games>`, played by the tests of
  `beryl_search` and `beryl_session` that `just fuzz` runs.
- `exhaust`: `exhaust:<card>/depth<d>-nodes<n>`, every decision sequence from the end positions of
  one card's scenarios (`beryl_cards`' `exhaust` test), and `exhaust:pool/<first seed>+<games>`,
  the same search from positions of random games between decks of every keystone family
  together, over seeds reserved as `fuzz` reserves them.
- `scale`: the Python tests that read every card file, with the card files among their inputs.
- `ignite-lint`: `ignite-lint:fmt` and `ignite-lint:clippy` over the `ignite/` workspace. Its
  tests are the `ignite` family, which runs them under cargo-nextest with no adapter of ours.
  `ignite` is accepted here too, with `ignite:test`, for a main checkout whose manifest predates
  that split (see `LEGACY_IGNITE`).
"""

from __future__ import annotations

import json
import os
import re
import secrets
import subprocess
import sys
import tempfile
import tomllib
from collections.abc import Iterable, Sequence
from pathlib import Path
from typing import Any
from urllib.parse import unquote

from .. import count_trend, paths, system
from . import python as python_adapter
from .script import card_files, card_name, slug

RESULTS_ENV = "IGNITE_RESULTS"
# The class Ignite tells each command it runs in (docs/ignite.md, "Cores, memory and shared
# resources"). Only a run in this one verifies a commit.
CLASS_ENV = "IGNITE_CLASS"
VERIFYING_CLASS = "tip"
FAMILIES = ("counts", "fuzz", "exhaust", "scale", "ignite-lint", "ignite")
EXPLORING = ("fuzz", "exhaust")
# The longest message a result carries.
MESSAGE_LIMIT = 300
# The exit code of a count run that cannot happen here (`reruns.CANNOT_RUN`, which is
# orchestration code and cannot be imported).
CANNOT_RUN = 125
# The ticket client. `IGNITE_BIN` names it when it is not on the path.
IGNITE_ENV = "IGNITE_BIN"

GATE_FILE = Path("engine/crates/beryl_bench/count-gate.toml")
ROLLOUTS = "counts:rollouts"
FAMILY_PREFIX = "counts:family/"

# --- fuzz ---

# The searches of `just fuzz`: each is a name, the cargo arguments that select its tests and the
# games it plays in a batch. A game of the shrinking searches costs a second or more, so they
# play fewer; the batch's seeds are reserved for the widest.
FUZZ_TARGETS: dict[str, tuple[list[str], int]] = {
    "games": (["-p", "beryl_search", "--test", "all", "games::"], 500),
    "families": (["-p", "beryl_search", "--test", "all", "families::"], 500),
    "shrink": (["-p", "beryl_search", "--test", "all", "shrink::"], 100),
    "session": (["-p", "beryl_session", "--test", "session", "fuzz::"], 100),
}
# Seeds reserved per batch. A multiple of 4, since the engine fuzzer plays every fourth game in
# the large layout and a batch must start on the same phase as any other.
WIDTH = 500
# The first seed a machine plays is a random multiple of this, so two machines play different
# seeds, all far above the 0 to a few thousand of the checks that run on every landing.
REGION = 1 << 20
# Optimized, with the invariants `just fuzz` keeps on.
RELEASE_CHECKS = {
    "CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS": "true",
    "CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS": "true",
}

# --- exhaust ---

# The bounds `just exhaust` uses by default.
EXHAUST_DEPTH = 6
EXHAUST_NODES = 20000


# Pool games a batch searches from. One game's search takes well under a second, so a batch is
# a few minutes of one core.
POOL_GAMES = 256


def exhaust_suffix() -> str:
    return f"depth{EXHAUST_DEPTH}-nodes{EXHAUST_NODES}"


# --- scale ---

# The tests that read card and vocabulary files (`lanes.CARD_READERS`, copied because an adapter
# must not import orchestration code), as paths from `tools/`.
CARD_READERS = (
    "tests/test_scale.py",
    "tests/test_card_names.py",
    "tests/test_card.py",
    "tests/test_specs.py",
)
# What those tests read besides the modules they import.
SCALE_INPUTS = [
    "engine/cards",
    "engine/crates/beryl_card",
    "engine/crates/beryl_card_build",
    "engine/crates/beryl_card_store",
    "engine/crates/beryl_cards",
    "data/card-store.tsv",
    "data/set-groups.json",
]

# --- ignite-lint ---

IGNITE_COMMANDS: dict[str, list[str]] = {
    "fmt": ["cargo", "fmt", "--check"],
    "clippy": ["cargo", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"],
}
IGNITE_DIR = "ignite"
# Ignite's daemon reads the check manifests from the main checkout, which lags the trees it runs
# checks in. A checkout whose manifest predates the `ignite-lint` family still declares `ignite`
# as a family of this adapter, with `ignite:test` running `cargo test`. These keep that family
# working in a tree that holds this file until no checkout's manifest declares it.
LEGACY_IGNITE = "ignite"
LEGACY_IGNITE_COMMANDS: dict[str, list[str]] = {
    **IGNITE_COMMANDS,
    "test": ["cargo", "test", "--locked", "-q"],
}


# --- list and next ---


def gate_families(root: Path) -> list[str]:
    """The families with a bar in the count gate's file, in its order."""
    return list(tomllib.loads((root / GATE_FILE).read_text()).get("family", {}))


def list_counts(root: Path) -> list[dict[str, Any]]:
    from .. import calibration, count_gate

    checks = [
        {"check": name, "inputs": []}
        for name in [
            ROLLOUTS,
            *(f"{FAMILY_PREFIX}{name}" for name in gate_families(root)),
        ]
    ]
    try:
        digest = count_gate.load_gate(root).profile.digest
    except calibration.SetupError as error:
        # A machine-relative listing without a digest is incomplete. Keep the actual names
        # visible on every verifier, even if setup prevents it from measuring any of them.
        print(f"counts setup: {error}", file=sys.stderr)
        return checks
    return [{**check, "digest": digest} for check in checks]


def card_names(root: Path) -> dict[str, str]:
    """Each card file's slug mapped to the card's name, which the exhaustive search takes."""
    names: dict[str, str] = {}
    for file in sorted((root / "engine" / "cards").glob("*/*/*.rs")):
        name = card_name(file.read_text())
        if name:
            names[slug(name)] = name
    return names


def scale_checks(root: Path) -> list[dict[str, Any]]:
    """The card readers' tests, each with the inputs `pytest` gives it and the card files.
    A collection error raises, which leaves the family `incomplete`."""
    project = next(p for p in python_adapter.PROJECTS if p.folder == "tools")
    folder = root / project.folder
    files = python_adapter.project_files(folder)
    graph = python_adapter.import_graph(files)
    out: list[dict[str, Any]] = []
    for node in python_adapter.collect(project, root):
        test_file = node.split("::", 1)[0]
        if test_file not in CARD_READERS:
            continue
        inputs = python_adapter.test_inputs(test_file, folder, files, graph, root)
        out.append(
            {"check": f"scale:{project.folder}/{node}", "inputs": inputs + SCALE_INPUTS}
        )
    return out


def list_checks(family: str, root: Path) -> list[dict[str, Any]]:
    if family == "counts":
        return list_counts(root)
    if family == "scale":
        return scale_checks(root)
    if family == "ignite-lint":
        return [
            {"check": f"ignite-lint:{name}", "inputs": []} for name in IGNITE_COMMANDS
        ]
    if family == LEGACY_IGNITE:
        return [
            {"check": f"ignite:{name}", "inputs": []} for name in LEGACY_IGNITE_COMMANDS
        ]
    # An exploration family's checks come from `next`.
    return []


def take_seeds(state: Path, count: int) -> int:
    """Reserves `count` seeds this machine has never yielded and returns the first. The state
    file records the next free seed; it is written before the seeds are used, so a run that
    stops midway never yields them again."""
    state.parent.mkdir(parents=True, exist_ok=True)
    with system.file_lock(state.with_suffix(".lock")):
        start = (
            int(json.loads(state.read_text())["next"])
            if state.is_file()
            else (secrets.randbits(32) + 1) * REGION
        )
        state.write_text(json.dumps({"next": start + count}) + "\n")
    return start


def fuzz_state() -> Path:
    return paths.cache_root() / "ignite" / "fuzz.json"


def next_fuzz(state: Path) -> list[dict[str, Any]]:
    """One batch: a check for each search, over the seeds the batch reserved."""
    first = take_seeds(state, WIDTH)
    return [
        {"check": f"fuzz:{target}/{first}+{games}", "inputs": []}
        for target, (_, games) in FUZZ_TARGETS.items()
    ]


def next_exhaust(root: Path, state: Path) -> list[dict[str, Any]]:
    """A check for each card, with the card's file as its own input, and one batch of pool
    games over seeds no earlier batch used."""
    files = card_files(root)
    first = take_seeds(state, POOL_GAMES)
    return [
        {"check": f"exhaust:{card}/{exhaust_suffix()}", "inputs": [path]}
        for card, path in files.items()
    ] + [{"check": f"exhaust:pool/{first}+{POOL_GAMES}", "inputs": []}]


def next_checks(family: str, root: Path) -> list[dict[str, Any]]:
    if family == "fuzz":
        return next_fuzz(fuzz_state())
    if family == "exhaust":
        return next_exhaust(root, fuzz_state())
    raise ValueError(f"{family} is a regression family: its checks come from `list`")


# --- run ---


def clip(text: str) -> str:
    text = " ".join(text.split())
    return text if len(text) <= MESSAGE_LIMIT else text[: MESSAGE_LIMIT - 1] + "…"


def tail(output: str, lines: int = 12) -> str:
    return clip(" ".join(output.strip().splitlines()[-lines:]))


def append(results: Path, lines: Iterable[dict[str, Any]]) -> None:
    with results.open("a") as out:
        for line in lines:
            out.write(json.dumps(line) + "\n")


def cannot_run(check: str, why: str) -> dict[str, Any]:
    """The line of a check that cannot run on this machine, also said on the error output so
    that the saved output says why."""
    print(f"cannot-run: {why}", file=sys.stderr)
    return {
        "check": check,
        "outcome": "incomplete",
        "message": f"cannot-run: {clip(why)}",
    }


def cargo(
    args: Sequence[str], root: Path, env: dict[str, str] | None = None
) -> tuple[int, str]:
    done = subprocess.run(
        ["cargo", *args],
        cwd=root / "engine",
        env={**os.environ, **(env or {})},
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        check=False,
    )
    return done.returncode, done.stdout + done.stderr


def judged(code: int, output: str, what: str) -> dict[str, Any] | None:
    """Cargo's verdict: 0 passed, 101 means a test failed or the build did; any other code is a
    failure to run it."""
    if code == 0:
        return {"outcome": "pass"}
    if code != 101:
        return None
    findings = [
        line.strip()
        for line in output.splitlines()
        if "seed" in line or "panicked" in line or line.startswith("test ")
    ]
    return {
        "outcome": "fail",
        "message": clip(" ".join(findings[-6:]) or tail(output) or what),
    }


# The path of the ticket client.
def ignite_command() -> str:
    return os.environ.get(IGNITE_ENV, "ignite")


class IgniteTickets:
    """The ticket store of `count_trend`, over the `ignite ticket` commands."""

    def call(self, *args: str) -> tuple[int, str]:
        try:
            done = subprocess.run(
                [ignite_command(), "ticket", *args],
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                check=False,
            )
        except OSError as error:
            return 127, str(error)
        return done.returncode, done.stdout

    def json_reply(self, *args: str) -> Any:
        code, out = self.call(*args, "--json")
        if code != 0:
            return None
        try:
            return json.loads(out)
        except ValueError:
            return None

    def list_issues(self, *filters: str, include_closed: bool = False) -> object:
        extra = ["--all"] if include_closed else []
        found = self.json_reply("list", *filters, *extra)
        if isinstance(found, dict):
            found = found.get("tickets", found.get("results", []))
        return found if found is not None else []

    def add_comment(self, issue: str, comment: str) -> tuple[int, str]:
        return self.call("comment", issue, "--", comment)

    def create_issue(
        self, *, title: str, body: str, priority: str, labels: str
    ) -> tuple[int, str]:
        with tempfile.TemporaryDirectory() as scratch:
            body_file = Path(scratch) / "body.txt"
            body_file.write_text(body)
            return self.call(
                "create",
                title,
                "-p",
                f"P{priority}",
                "--labels",
                labels,
                "--body-file",
                str(body_file),
                # Filed by a check, so it stays on the machine that measured it.
                "--automatic",
            )

    def batch(self, request: dict[str, object]) -> Any:
        with tempfile.TemporaryDirectory() as scratch:
            batch_file = Path(scratch) / "batch.json"
            batch_file.write_text(json.dumps(request))
            return self.json_reply("batch", str(batch_file))

    def inspect_issues(
        self, *issues: str
    ) -> tuple[int, list[dict[str, object]]] | None:
        # An empty batch reads the hub sequence without changing any tickets.
        before = self.batch({"ops": []})
        if not isinstance(before, dict) or not isinstance(before.get("seq"), int):
            return None
        found = self.json_reply("show", *issues, "--comments", "--history")
        details = found if isinstance(found, list) else [found]
        return before["seq"], [ticket for ticket in details if isinstance(ticket, dict)]

    def close_issues(self, sequence: int, reasons: dict[str, str]) -> bool:
        # A comment advances the hub sequence but leaves the ticket version alone.
        result = self.batch(
            {
                "if": [{"seq": sequence}],
                "ops": [
                    {"op": "close", "id": issue, "reason": reason}
                    for issue, reason in reasons.items()
                ],
            }
        )
        return isinstance(result, dict) and isinstance(result.get("results"), list)


def git(root: Path, *args: str) -> str:
    done = subprocess.run(
        ["git", *args], cwd=root, capture_output=True, text=True, check=False
    )
    return done.stdout.strip() if done.returncode == 0 else ""


def verified_commit(root: Path) -> str | None:
    """The commit whose count may be stored: HEAD of a tree without edits, when this run is
    in the class that verifies commits. A count measured on anything else is only compared.
    """
    if os.environ.get(CLASS_ENV) != VERIFYING_CLASS:
        return None
    if git(root, "status", "--porcelain", "--untracked-files=no"):
        return None
    return git(root, "rev-parse", "HEAD") or None


# The count gate command line, which tests replace.
def count_gate_command(*args: str) -> list[str]:
    return [sys.executable, "-m", "beryl_tools.count_gate", "count-gate", *args]


def run_gate(args: Sequence[str], root: Path) -> tuple[int, str]:
    done = subprocess.run(
        count_gate_command(*args),
        cwd=root,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        check=False,
    )
    return done.returncode, done.stdout + done.stderr


def trend(root: Path, found: dict[str, Any]) -> None:
    """Stores the verified commit's count and updates tickets the trend calls for. A failure here
    never changes the check's outcome."""
    try:
        for line in count_trend.report_found(
            verified_commit(root), found, IgniteTickets()
        ):
            print(line, file=sys.stderr)
    except Exception as error:  # noqa: BLE001 - best effort by design
        print(f"count trend: {error}", file=sys.stderr)


def run_rollouts(root: Path) -> dict[str, Any]:
    with tempfile.TemporaryDirectory() as scratch:
        measure = Path(scratch) / "measure.json"
        code, output = run_gate(["--measure", str(measure)], root)
        print(output, file=sys.stderr)
        if code == CANNOT_RUN:
            return cannot_run(ROLLOUTS, tail(output))
        if not measure.is_file():
            return {}
        found = json.loads(measure.read_text())
    trend(root, found)
    result: dict[str, Any] = {"outcome": "pass" if found["ok"] else "fail"}
    if not found["ok"]:
        result["message"] = clip(str(found["message"]))
    return result


# The line `count_gate.family_gate` prints for each family it judged.
VERDICT: re.Pattern[str] = re.compile(
    r"^bench: verdict (\S+): (pass|fail)$", re.MULTILINE
)


def parse_family_verdicts(names: Sequence[str], output: str) -> list[dict[str, Any]]:
    verdicts = dict(VERDICT.findall(output))
    results: list[dict[str, Any]] = []
    for name in names:
        verdict = verdicts.get(name)
        if verdict == "pass":
            results.append({"check": f"{FAMILY_PREFIX}{name}", "outcome": "pass"})
        elif verdict == "fail":
            said = [
                t for t in output.splitlines() if t.startswith(f"bench: family {name}:")
            ]
            results.append(
                {
                    "check": f"{FAMILY_PREFIX}{name}",
                    "outcome": "fail",
                    "message": (
                        clip(said[-1].removeprefix("bench: ")) if said else tail(output)
                    ),
                }
            )
    return results


def run_gate_families(names: Sequence[str], root: Path) -> list[dict[str, Any]]:
    """The results of the families `names`, counted in one run of the gate: one build and one
    turn at the VM instead of one per family, which put a whole check run past its hang limit.
    A family the gate did not judge gets no line, so Ignite records it as `incomplete`.
    """
    arguments = [argument for name in names for argument in ("--family", name)]
    code, output = run_gate(arguments, root)
    print(output, file=sys.stderr)
    if code == CANNOT_RUN:
        return [cannot_run(f"{FAMILY_PREFIX}{name}", tail(output)) for name in names]
    return parse_family_verdicts(names, output)


def run_counts(names: Sequence[str], root: Path, results: Path) -> None:
    include_rollouts = ROLLOUTS in names
    families = [
        name.removeprefix(FAMILY_PREFIX)
        for name in names
        if name.startswith(FAMILY_PREFIX)
    ]
    if include_rollouts and families:
        with tempfile.TemporaryDirectory() as scratch:
            measure = Path(scratch) / "measure.json"
            arguments = [
                "--measure",
                str(measure),
                *(argument for name in families for argument in ("--family", name)),
            ]
            code, output = run_gate(arguments, root)
            print(output, file=sys.stderr)
            if code == CANNOT_RUN:
                reason = tail(output)
                append(
                    results,
                    [
                        cannot_run(ROLLOUTS, reason),
                        *(
                            cannot_run(f"{FAMILY_PREFIX}{name}", reason)
                            for name in families
                        ),
                    ],
                )
                return
            batch: list[dict[str, Any]] = []
            if measure.is_file():
                found = json.loads(measure.read_text())
                trend(root, found)
                result: dict[str, Any] = {"outcome": "pass" if found["ok"] else "fail"}
                if not found["ok"]:
                    result["message"] = clip(str(found["message"]))
                batch.append({"check": ROLLOUTS, **result})
            batch.extend(parse_family_verdicts(families, output))
            if batch:
                append(results, batch)
        return
    if include_rollouts:
        result = run_rollouts(root)
        if result:
            append(results, [{"check": ROLLOUTS, **result}])
    if families:
        append(results, run_gate_families(families, root))


def parse_fuzz(name: str) -> tuple[str, int, int]:
    """(target, first seed, games) of `fuzz:<target>/<first>+<games>`."""
    match = re.fullmatch(r"fuzz:(\w+)/(\d+)\+(\d+)", name)
    if not match or match[1] not in FUZZ_TARGETS:
        raise ValueError(f"not a fuzz check: {name}")
    return match[1], int(match[2]), int(match[3])


def run_fuzz(names: Sequence[str], root: Path, results: Path) -> None:
    for name in names:
        target, first, games = parse_fuzz(name)
        selected, _ = FUZZ_TARGETS[target]
        code, output = cargo(
            [
                "test",
                "--release",
                "--locked",
                *selected,
                "--",
                "--nocapture",
                "--include-ignored",
            ],
            root,
            {
                **RELEASE_CHECKS,
                "BERYL_FUZZ_GAMES": str(games),
                "BERYL_FUZZ_FIRST_SEED": str(first),
            },
        )
        result = judged(code, output, "the search failed")
        if result:
            append(results, [{"check": name, **result}])


def parse_exhaust(name: str) -> tuple[str, int, int]:
    """(card slug, depth, nodes) of `exhaust:<card>/depth<d>-nodes<n>`."""
    match = re.fullmatch(r"exhaust:([^/]+)/depth(\d+)-nodes(\d+)", name)
    if not match:
        raise ValueError(f"not an exhaust check: {name}")
    return unquote(match[1]), int(match[2]), int(match[3])


def parse_pool(name: str) -> tuple[int, int] | None:
    """(first seed, games) of `exhaust:pool/<first>+<games>`, or None for a card's check."""
    match = re.fullmatch(r"exhaust:pool/(\d+)\+(\d+)", name)
    return (int(match[1]), int(match[2])) if match else None


def exhaust_test(test: str, env: dict[str, str], root: Path) -> tuple[int, str]:
    """Runs the `exhaust` test binary's tests named `test`, ignored ones included, at the
    default bounds and with the environment `env` adds."""
    return cargo(
        [
            "test",
            "--release",
            "--locked",
            "-p",
            "beryl_cards",
            "--test",
            "exhaust",
            test,
            "--",
            "--nocapture",
            "--include-ignored",
        ],
        root,
        {
            **RELEASE_CHECKS,
            "BERYL_EXHAUST_DEPTH": str(EXHAUST_DEPTH),
            "BERYL_EXHAUST_NODES": str(EXHAUST_NODES),
            **env,
        },
    )


def run_exhaust(names: Sequence[str], root: Path, results: Path) -> None:
    cards = card_names(root)
    for name in names:
        pool = parse_pool(name)
        if pool:
            first, games = pool
            code, output = exhaust_test(
                "pool_positions", {"BERYL_EXHAUST_POOL": f"{first}+{games}"}, root
            )
        else:
            card, depth, nodes = parse_exhaust(name)
            if card not in cards:
                continue
            code, output = exhaust_test(
                "every_scenario",
                {
                    "BERYL_EXHAUST_CARD": cards[card],
                    "BERYL_EXHAUST_DEPTH": str(depth),
                    "BERYL_EXHAUST_NODES": str(nodes),
                },
                root,
            )
        result = judged(code, output, "the search failed")
        if result:
            append(results, [{"check": name, **result}])


def renamed(lines: Iterable[str]) -> list[dict[str, Any]]:
    """The `pytest:` result lines of python.py's plugin, named for this family."""
    out: list[dict[str, Any]] = []
    for text in lines:
        line = json.loads(text)
        line["check"] = "scale:" + line["check"].removeprefix("pytest:")
        out.append(line)
    return out


def run_scale(names: Sequence[str], root: Path, results: Path) -> None:
    project = next(p for p in python_adapter.PROJECTS if p.folder == "tools")
    nodes = [n.removeprefix("scale:").partition("/")[2] for n in names]
    with tempfile.TemporaryDirectory() as scratch:
        scratch_results = Path(scratch) / "results.jsonl"
        python_adapter.run_pytest(project, nodes, root, scratch_results)
        if scratch_results.is_file():
            append(results, renamed(scratch_results.read_text().splitlines()))


def run_ignite(names: Sequence[str], root: Path, results: Path) -> None:
    for name in names:
        family, _, short = name.partition(":")
        commands = (
            LEGACY_IGNITE_COMMANDS if family == LEGACY_IGNITE else IGNITE_COMMANDS
        )
        command = commands.get(short)
        if command is None:
            continue
        done = subprocess.run(
            command,
            cwd=root / IGNITE_DIR,
            # Incremental compilation saves nothing for a check that runs once per input hash,
            # and on Windows rustc fails to finalize its session directory ("Access is
            # denied", os error 5) when an indexer or virus scanner holds it, which made
            # clippy fail on cygnus alone.
            env={**os.environ, "CARGO_INCREMENTAL": "0"},
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
        )
        output = done.stdout + done.stderr
        if done.returncode == 0:
            result: dict[str, Any] = {"outcome": "pass"}
        elif done.returncode in (1, 101):
            # `cargo fmt --check` exits 1 when it would reformat, and cargo exits 101 when the
            # build or a test failed; clippy's lints and rustc's errors exit 101 as well.
            result = {"outcome": "fail", "message": tail(output) or "failed"}
        else:
            continue
        append(results, [{"check": name, **result}])


def run_checks(family: str, names: Sequence[str], root: Path, results: Path) -> None:
    runners = {
        "counts": run_counts,
        "fuzz": run_fuzz,
        "exhaust": run_exhaust,
        "scale": run_scale,
        "ignite-lint": run_ignite,
        LEGACY_IGNITE: run_ignite,
    }
    runners[family](names, root, results)


def main(argv: Sequence[str]) -> int:
    root = Path.cwd()
    verbs = ("list", "next", "run")
    if len(argv) < 2 or argv[0] not in verbs or argv[1] not in FAMILIES:
        print(__doc__, file=sys.stderr)
        return 1
    verb, family, rest = argv[0], argv[1], list(argv[2:])
    if verb == "list":
        for line in list_checks(family, root):
            print(json.dumps(line))
        return 0
    if verb == "next":
        if family not in EXPLORING:
            print(f"{family} has no `next`: it is a regression family", file=sys.stderr)
            return 1
        for line in next_checks(family, root):
            print(json.dumps(line))
        return 0
    results = os.environ.get(RESULTS_ENV)
    if not results:
        print(f"{RESULTS_ENV} is not set", file=sys.stderr)
        return 1
    run_checks(family, rest, root, Path(results))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
