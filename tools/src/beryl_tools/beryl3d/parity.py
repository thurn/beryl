"""`just b3d parity`: the parity list, everything the web interface can show with each item's
state in Beryl 3D (docs/beryl-3d.md, 'The parity list').

It prints one line per kind of item, `<kind> <done> / <total>`, followed when the kind is not
complete by the first few items still missing. The items come from:

- the recordings: each fixture of `ui/fixtures.json` and each card spec moment is done when
  every interface test of it passes, from both seats in every desktop and phone window
  (`just b3d test --json` and `--phone-only --json`), no test of it has a fault allowed by
  `data/beryl3d/test-faults.json`, and no key of `data/beryl3d/timeline-parity.json` names it;
- `data/beryl3d/parity.json`, which names a recording for each verb, effect and mark of
  `data/ui-conventions.json`, maps each skill to the job it does for a client, each web screen
  file to a screen of `data/beryl3d/screens.json`, and lists the jobs with the Beryl 3D command
  that does each;
- `data/beryl3d/screens.json`, which lists Beryl 3D's screens and names a fixture for each
  prompt shape;
- `data/beryl3d/conventions/<kind>.json`, which records how Beryl 3D follows each UI
  Conventions entry, or why it cannot;
- the Beryl 3D sweep's log, for the macOS launch test.

`--check` reads only the data files and fails when a skill directory or a web screen file has
no entry in `parity.json`, or a key of it names something that no longer exists. It never fails
for an incomplete list. The sweep runs it as its `parity-list` step.

The command writes no file.
"""

from __future__ import annotations

import argparse
import json
import subprocess
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .. import paths as repo_paths
from ..lanes import BY
from . import paths, sweep
from .command import Command

PARITY: Path = paths.DATA / "parity.json"
SCREENS: Path = paths.DATA / "screens.json"
CONVENTIONS: Path = paths.DATA / "conventions"
TIMELINE_PARITY: Path = paths.DATA / "timeline-parity.json"
TEST_FAULTS: Path = paths.DATA / "test-faults.json"
UI_CONVENTIONS: Path = repo_paths.REPO / "data" / "ui-conventions.json"
FIXTURES: Path = repo_paths.REPO / "ui" / "fixtures.json"
SKILLS: Path = repo_paths.REPO / ".claude" / "skills"
# The section kinds of `data/ui-conventions.json` whose entries Beryl 3D follows, each with a
# file of the same name under [`CONVENTIONS`].
CONVENTION_KINDS: tuple[str, ...] = (
    "exception",
    "settled",
    "no-equivalent",
    "not-seen",
)
# The lists of `data/ui-conventions.json` that `parity.json` maps to recordings, by key.
PLAYED: tuple[str, ...] = ("verbs", "effects", "marks")
# How many missing items a line names before it counts the rest, unless `--full`.
MOST_NAMED = 5
# The width of the kind's column, the longest kind's ("prompt shapes").
KIND_WIDTH = 13
# A test's name: this, then `<recording>/<seat>/<width>x<height>`.
TEST_PREFIX = "b3d:"
# The value of a `parity.json` mapping that says the item looks at no client, or is no screen.
NONE_PREFIX = "none: "
# A `web_screens` value for a web screen Beryl 3D does not have.
MISSING = "missing"

Json = dict[str, Any]


def read_json(path: Path) -> Json:
    return json.loads(path.read_text())


def web_screen_files(root: Path = repo_paths.REPO) -> list[str]:
    """The web interface's screen files, as paths from the repository's root: every
    `ui/src/screens/*.tsx`, and every component of `ui/src/table` whose name ends in
    `View.tsx` or `Opening.tsx` or that is in `ui/src/table/lobby`."""
    table = root / "ui" / "src" / "table"
    found = [
        *(root / "ui" / "src" / "screens").glob("*.tsx"),
        *(
            p
            for p in table.glob("*.tsx")
            if p.name.endswith(("View.tsx", "Opening.tsx"))
        ),
        *(table / "lobby").glob("*.tsx"),
    ]
    return sorted(p.relative_to(root).as_posix() for p in found)


def skill_names(root: Path = SKILLS) -> list[str]:
    return sorted(p.name for p in root.iterdir() if p.is_dir())


def recording_of(test: str) -> tuple[str, str]:
    """The recording and seat a test's name names. A card spec moment's recording holds a
    slash itself (`<card>/<moment>`), so the seat and window are split off the end."""
    recording, seat, _window = test.removeprefix(TEST_PREFIX).rsplit("/", 2)
    return recording, seat


def last_json(text: str) -> Json:
    """The last line of `text` that is a JSON object: a `b3d` program's report, after any
    lines a build printed first."""
    for line in reversed(text.splitlines()):
        if line.startswith("{"):
            return json.loads(line)
    raise ValueError("no JSON report in the output")


@dataclass
class Results:
    """What the interface tests found, by recording."""

    # The seats each recording was played from, desktop and phone apart.
    desktop: dict[str, set[str]] = field(default_factory=dict)
    phone: dict[str, set[str]] = field(default_factory=dict)
    # The recordings with a test that failed in either run.
    failed: set[str] = field(default_factory=set)

    def add(self, report: Json, phone: bool) -> None:
        played = self.phone if phone else self.desktop
        for row in report["results"]:
            recording, seat = recording_of(row["test"])
            played.setdefault(recording, set()).add(seat)
            if row["outcome"] != "pass":
                self.failed.add(recording)

    def passes(self, recording: str) -> bool:
        """Whether every test of `recording` passed, and it was played in both layouts."""
        return (
            recording in self.desktop
            and recording in self.phone
            and recording not in self.failed
        )


@dataclass
class Kind:
    """One line of the list."""

    kind: str
    done: int
    total: int
    missing: list[str]
    # What the line says after its count when nothing is missing, or instead of the names.
    note: str = ""

    def line(self, full: bool) -> str:
        head = f"{self.kind:<{KIND_WIDTH}} {self.done:,} / {self.total:,}"
        if self.note:
            return f"{head}   {self.note}"
        if not self.missing:
            return head
        shown = self.missing if full else self.missing[:MOST_NAMED]
        rest = len(self.missing) - len(shown)
        more = f" (+{rest:,} more, --full)" if rest else ""
        return f"{head}   missing: {', '.join(shown)}{more}"

    def json(self) -> Json:
        return {
            "kind": self.kind,
            "done": self.done,
            "total": self.total,
            "missing": self.missing,
            "note": self.note,
        }


@dataclass
class Sources:
    """Everything the list is counted from, read by [`read_sources`] or built by a test."""

    results: Results
    fixtures: list[str]
    parity: Json
    screens: Json
    ui_conventions: Json
    # The entries of each conventions file, by section kind.
    conventions: dict[str, Json]
    # The keys of `timeline-parity.json`, `<recording>/<seat>`.
    timeline_keys: set[str]
    # The tests `test-faults.json` allows faults for.
    listed_faults: set[str]
    sweep_records: list[dict[str, object]]


def not_timed(sources: Sources) -> set[str]:
    """The recordings whose timeline differs from the web interface's for some seat."""
    return {k.rsplit("/", 1)[0] for k in sources.timeline_keys}


def done_recordings(sources: Sources) -> set[str]:
    """The recordings that are done: every test passes, no test of theirs has a listed fault,
    and their timelines agree with the web interface's from both seats."""
    faulted = {recording_of(t)[0] for t in sources.listed_faults}
    blocked = faulted | not_timed(sources)
    return {
        r
        for r in set(sources.results.desktop) | set(sources.results.phone)
        if sources.results.passes(r) and r not in blocked
    }


def spec_moments(sources: Sources) -> list[str]:
    """The card spec moments the tests played: every recording named `<card>/<moment>` but
    the screen recordings (`screen/<name>`)."""
    fixtures = set(sources.fixtures)
    return sorted(
        r
        for r in sources.results.desktop
        if "/" in r and not r.startswith("screen/") and r not in fixtures
    )


def recordings_kind(kind: str, names: Iterable[str], done: set[str]) -> Kind:
    names = list(names)
    missing = [n for n in names if n not in done]
    return Kind(kind, len(names) - len(missing), len(names), missing)


def timelines_kind(sources: Sources) -> Kind:
    """Each fixture and spec moment, seat by seat, whose timeline agrees with the web
    interface's."""
    wanted = set(sources.fixtures) | set(spec_moments(sources))
    keys = sorted(
        f"{r}/{seat}"
        for r, seats in sources.results.desktop.items()
        if r in wanted
        for seat in seats
    )
    missing = [k for k in keys if k in sources.timeline_keys]
    return Kind("timelines", len(keys) - len(missing), len(keys), missing)


def mapped_kind(kind: str, mapping: Mapping[str, str], done: set[str]) -> Kind:
    """Items each done when the recording `mapping` names for it is done."""
    missing = [name for name, recording in mapping.items() if recording not in done]
    return Kind(kind, len(mapping) - len(missing), len(mapping), missing)


def screens_kind(sources: Sources, done: set[str]) -> Kind:
    """Beryl 3D's screens marked built whose recording is done, out of those and the web
    screens it has none for."""
    screens = sources.screens["screens"]
    unbuilt = [
        s["name"]
        for s in screens
        if s.get("built") is not True or s.get("recording") not in done
    ]
    absent = [f for f, s in sources.parity["web_screens"].items() if s == MISSING]
    missing = [*unbuilt, *absent]
    total = len(screens) + len(absent)
    return Kind("screens", total - len(missing), total, missing)


def conventions_kind(sources: Sources, done: set[str]) -> Kind:
    """The UI Conventions entries that a conventions file shows Beryl 3D following, in a
    recording that is done. An entry recorded as open is named with its reason."""
    total = 0
    missing: list[str] = []
    for entry in sources.ui_conventions["entries"]:
        if entry["kind"] not in CONVENTION_KINDS:
            continue
        total += 1
        recorded = sources.conventions.get(entry["kind"], {}).get(entry["name"])
        if isinstance(recorded, dict) and recorded.get("recording") in done:
            continue
        if isinstance(recorded, dict) and "open" in recorded:
            missing.append(f"{entry['name']} (open: {recorded['open']})")
        else:
            missing.append(entry["name"])
    return Kind("conventions", total - len(missing), total, missing)


def tools_kind(sources: Sources) -> Kind:
    tools = sources.parity["tools"]
    missing = [t["job"] for t in tools if t["b3d"] is None]
    return Kind("tools", len(tools) - len(missing), len(tools), missing)


def launch_kind(records: list[dict[str, object]]) -> Kind:
    """The macOS launch test, which the sweep's `smoke` step runs in its quick part: passed at
    the last commit whose quick part passed. The log keeps each part's result, not each step's,
    so a failed quick part says nothing of `smoke` and the last pass is reported."""
    commit = sweep.last_passed(records, sweep.QUICK)
    others = "iOS, Android and Windows not required"
    if commit:
        return Kind("launch tests", 1, 1, [], f"macOS pass at {commit[:10]}; {others}")
    return Kind("launch tests", 0, 1, [], f"macOS not run; {others}")


def report(sources: Sources) -> list[Kind]:
    done = done_recordings(sources)
    parity = sources.parity
    return [
        recordings_kind("fixtures", sources.fixtures, done),
        recordings_kind("card specs", spec_moments(sources), done),
        timelines_kind(sources),
        *(mapped_kind(key, parity[key], done) for key in PLAYED),
        mapped_kind("prompt shapes", sources.screens["prompt_shapes"], done),
        screens_kind(sources, done),
        conventions_kind(sources, done),
        tools_kind(sources),
        launch_kind(sources.sweep_records),
    ]


def problems(
    parity: Json,
    screens: Json,
    ui_conventions: Json,
    conventions: Mapping[str, Json],
    skills: list[str],
    web_files: list[str],
) -> list[str]:
    """What `--check` fails for: each skill directory and web screen file `parity.json` does
    not map, each key that names something that no longer exists, and each value that names
    no job or screen."""
    found: list[str] = []
    jobs = {t["job"] for t in parity["tools"]}
    screen_names = {s["name"] for s in screens["screens"]}

    def compare(what: str, have: Iterable[str], want: Iterable[str]) -> None:
        have, want = set(have), set(want)
        found.extend(f"{what}: {n} has no entry" for n in sorted(want - have))
        found.extend(f"{what}: {n} no longer exists" for n in sorted(have - want))

    compare("parity.json skills", parity["skills"], skills)
    for skill, job in parity["skills"].items():
        if job not in jobs and not job.startswith(NONE_PREFIX):
            found.append(
                f"parity.json skills: {skill} maps to {job!r}, which is no job"
            )
    for tool in parity["tools"]:
        for skill in tool["skills"]:
            if skill not in skills:
                found.append(
                    f"parity.json tools: {tool['job']} names no skill {skill!r}"
                )
    compare("parity.json web_screens", parity["web_screens"], web_files)
    for path, screen in parity["web_screens"].items():
        if screen not in screen_names | {MISSING} and not screen.startswith(
            NONE_PREFIX
        ):
            found.append(
                f"parity.json web_screens: {path} maps to {screen!r}, no screen"
            )
    for key in PLAYED:
        names = [item["name"] for item in ui_conventions[key]]
        compare(f"parity.json {key}", parity[key], names)
    for kind in CONVENTION_KINDS:
        names = [e["name"] for e in ui_conventions["entries"] if e["kind"] == kind]
        entries = conventions.get(kind, {})
        found.extend(
            f"conventions/{kind}.json: {name} no longer exists"
            for name in sorted(set(entries) - set(names))
        )
    return found


def read_conventions() -> dict[str, Json]:
    return {
        kind: read_json(CONVENTIONS / f"{kind}.json")["entries"]
        for kind in CONVENTION_KINDS
    }


def run_tests(phone: bool) -> Json:
    """`just b3d test --json`, or its phone windows', as the `test` command runs it."""
    command = [*BY, "b3d", "test", *(["--phone-only"] if phone else []), "--json"]
    done = subprocess.run(command, stdout=subprocess.PIPE, text=True, check=False)
    return last_json(done.stdout)


def read_results(folder: Path | None) -> Results:
    """The interface tests' results: from `desktop.json` and `phone.json` in `folder`, the two
    `b3d test --json` outputs, or else from running both."""
    results = Results()
    for phone, name in ((False, "desktop.json"), (True, "phone.json")):
        if folder is None:
            results.add(run_tests(phone), phone)
        else:
            results.add(last_json((folder / name).read_text()), phone)
    return results


def read_sources(results: Results) -> Sources:
    fixtures = [f["name"] for f in read_json(FIXTURES)["fixtures"]]
    faults = read_json(TEST_FAULTS)["tests"] if TEST_FAULTS.exists() else {}
    timeline = read_json(TIMELINE_PARITY)["recordings"]
    return Sources(
        results=results,
        fixtures=fixtures,
        parity=read_json(PARITY),
        screens=read_json(SCREENS),
        ui_conventions=read_json(UI_CONVENTIONS),
        conventions=read_conventions(),
        timeline_keys=set(timeline),
        listed_faults=set(faults),
        sweep_records=sweep.read_log(),
    )


def run_check() -> int:
    found = problems(
        read_json(PARITY),
        read_json(SCREENS),
        read_json(UI_CONVENTIONS),
        read_conventions(),
        skill_names(),
        web_screen_files(),
    )
    for line in found:
        print(line)
    if found:
        print(f"{len(found)} problems; map each in data/beryl3d/parity.json.")
        return 1
    print("data/beryl3d/parity.json maps every skill and web screen file.")
    return 0


def run_parity(args: argparse.Namespace) -> int:
    if args.check:
        return run_check()
    kinds = report(read_sources(read_results(args.results)))
    if args.json:
        print(json.dumps({"kinds": [k.json() for k in kinds]}))
    else:
        for kind in kinds:
            print(kind.line(args.full))
    return 0


def parity_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--full", action="store_true", help="name every missing item of each line"
    )
    parser.add_argument("--json", action="store_true", help="print the whole report")
    parser.add_argument(
        "--check",
        action="store_true",
        help="only check that parity.json maps every skill and web screen file",
    )
    parser.add_argument(
        "--results",
        type=Path,
        help="read the tests' results from desktop.json and phone.json here, the outputs of"
        " `b3d test --json` and `b3d test --phone-only --json`, instead of running them",
    )


COMMANDS: tuple[Command, ...] = (
    Command(
        "parity",
        "the parity list: what of the web interface Beryl 3D shows, and what it lacks",
        (
            ("just b3d parity", "count every kind, running the interface tests"),
            (
                "just b3d parity --check",
                "check that parity.json maps every skill and screen",
            ),
        ),
        run_parity,
        parity_arguments,
    ),
)
