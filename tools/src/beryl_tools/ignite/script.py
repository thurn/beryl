"""The script, app and mobile check families' adapter. The families are declared in
`ignite/checks/script.toml`.

Run from the repository's root as `python -m beryl_tools.ignite.script <verb> <family> ...`:

- `list <family> [card...]` prints one JSON line per check: `{"check": "script:<card>/<moment>/
  <seat>", "inputs": ["rust/cards/<crate>/<set>/<card>.rs"]}`. The inputs are the paths that
  check reads in addition to the family's own `inputs` in the manifest. For `script`, naming
  cards (by name or slug; `fixtures` for the UI recordings) lists only theirs.
- `run <family> <check>...` runs the checks named and writes one JSON line per check that reached
  a verdict to the file named by `IGNITE_RESULTS`: `{"check", "outcome"}` and, for a failure,
  `message`. A check the tool ended without judging gets no line, so Ignite records it as
  `incomplete`.

The families:

- `script`: `beryl_cli script-check`, in a debug build (the `script-cli` product), where every
  script the session builds is held to the replay check and the rule invariants. One check per
  card, moment and seat, and one per UI fixture. The moments of a card are what `beryl_cli
  specs-check <card>` lists, which plays them. A check's own inputs are its card's file, the
  file of every card that file names (`used_files`), and the file of every card their
  definitions name, so that editing a card leaves the checks of cards that do not use it as
  they were.
- `app`: `just smoke`, one check, `app:smoke`.
- `mobile`: `cargo check` of the app for the iOS Simulator and for Android, one check per target.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
from collections.abc import Iterable, Sequence
from pathlib import Path
from typing import Any

RESULTS_ENV = "IGNITE_RESULTS"
FAMILIES = ("script", "app", "mobile")
CARDS = "rust/cards"
FIXTURES = "ui/fixtures.json"
# The seats of a card's moment, as `beryl_cli` names them.
SEATS = ("yours", "theirs")
# The longest message a result carries.
MESSAGE_LIMIT = 300
# `just smoke`'s exit code when its build failed: the app was never launched.
CANNOT_RUN = 125

MOBILE_TARGETS = ("aarch64-apple-ios-sim", "aarch64-linux-android")
# The Android toolchain `by mobile-setup` installs (tools/src/beryl_tools/mobile.py, which cannot
# be imported here because it reaches the orchestration code).
ANDROID_HOME: Path = Path.home() / "Library" / "Android" / "sdk"
NDK_VERSION = "28.2.13676358"
MIN_SDK = 24


def slug(name: str) -> str:
    """A card name as `beryl_cli` writes it in a check's id (`specs::slug`): ASCII letters and
    digits lower-cased, runs of spaces and hyphens as one hyphen."""
    out = ""
    for c in name:
        if c.isascii() and c.isalnum():
            out += c.lower()
        elif c in " -" and out and not out.endswith("-"):
            out += "-"
    return out.rstrip("-")


def card_name(text: str) -> str | None:
    """The card named by the generated header of a card file: its first `//!` line holds the name
    and then the mana cost."""
    for line in text.splitlines():
        if line.startswith("//! "):
            return line[4:].split(" {", 1)[0].strip()
        if not line.startswith("//"):
            return None
    return None


def card_files(root: Path) -> dict[str, str]:
    """Each card's slug, mapped to the path of its file from `root`."""
    files: dict[str, str] = {}
    for file in sorted((root / CARDS).glob("*/*/*.rs")):
        name = card_name(file.read_text())
        if name:
            files[slug(name)] = file.relative_to(root).as_posix()
    return files


# How a card's file names another card: a constant of a generated type, `Card`, `Printed` or
# `Token` (`Card::ManaCrypt`); a constant of a names module, `beryl_cards::names` or
# `printed::token` (`names::MANA_CRYPT`); or a string literal that is a card's whole name
# (`"Mana Crypt"`), since a card name is a `&str` a scenario may spell out.
CARD_REFERENCE: re.Pattern[str] = re.compile(
    r"\b(?:Card|Printed|Token)::([A-Z][A-Za-z0-9]*)\b"
    r"|\b(?:names|token)::([A-Z][A-Z0-9_]*)\b"
    r'|"([^"\\\n]{1,80})"'
)

# Where a card file's scenarios start: the generated card list reads them from
# `pub mod scenarios {` (`has_scenarios` in rust/crates/beryl_card_build/src/generate.rs).
SCENARIOS: re.Pattern[str] = re.compile(r"^\s*pub\s+mod\s+scenarios\s*\{", re.MULTILINE)


def letters(name: str) -> str:
    """A card name, a variant or a constant with everything but its letters and digits removed
    and lower-cased, which the three spellings of one card share."""
    return "".join(c.lower() for c in name if c.isascii() and c.isalnum())


# A Rust comment, or a literal that may hold `//` or `/*` without starting one. Block comments
# that nest are taken to their first `*/`.
COMMENT_OR_LITERAL: re.Pattern[str] = re.compile(
    r'\bb?r(#*)"[\s\S]*?"\1'  # a raw string
    r'|"(?:\\.|[^"\\])*"'  # a string
    r"|'(?:\\.|[^'\\])'"  # a character
    r"|//[^\n]*|/\*[\s\S]*?\*/"  # a comment
)


def code_only(text: str) -> str:
    """Rust source without its comments, doc comments included. A card a comment names is not
    one the scenarios play, and Ignite keeps this family's listing across a change that edits
    only comments (`listing_ignores_comments` in ignite/checks/script.toml), so the checks'
    inputs must not depend on them."""
    return COMMENT_OR_LITERAL.sub(
        lambda m: " " if m.group(0).startswith("/") else m.group(0), text
    )


def references(text: str) -> set[str]:
    """Every spelling in `text`'s code that may name a card, as `letters` writes it."""
    found = CARD_REFERENCE.findall(code_only(text))
    return {letters("".join(groups)) for groups in found} - {""}


def definition(text: str) -> str:
    """A card file without its scenarios: the part a scenario of another card plays."""
    found = SCENARIOS.search(text)
    return text[: found.start()] if found else text


def used_files(root: Path, own: str, files: dict[str, str]) -> list[str]:
    """The card files a card's scenarios read: its own, the file of every other card its file
    names (a card it steals, a spell that targets it, a permanent on the battlefield), and, so
    that a card those cards' definitions name is read too, every card their definitions name in
    turn. The scenarios of the cards it uses are not followed, because they are not played.
    """
    by_letters = {letters(slug_): path for slug_, path in files.items()}
    used = {files[own]}
    texts = [(root / files[own]).read_text()]
    while texts:
        for reference in references(texts.pop()):
            path = by_letters.get(reference)
            if path and path not in used:
                used.add(path)
                texts.append(definition((root / path).read_text()))
    return sorted(used)


def binary(root: Path) -> Path:
    """The debug `beryl_cli` the `script-cli` product builds."""
    target = Path(os.environ.get("CARGO_TARGET_DIR", root / "rust" / "target"))
    return target / "debug" / ("beryl_cli.exe" if os.name == "nt" else "beryl_cli")


MOMENT: re.Pattern[str] = re.compile(r"^ok\s+(\S+) / (\S+)\s+\(")


def moments(root: Path, cards: Sequence[str]) -> list[tuple[str, str]]:
    """The (card slug, moment) pairs `specs-check` lists for `cards`, every card if none. A
    failure to run raises, which leaves the family `incomplete`."""
    done = subprocess.run(
        [str(binary(root)), "specs-check", *cards],
        cwd=root / "rust",
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        raise RuntimeError(f"specs-check failed:\n{done.stdout}{done.stderr}")
    return [
        (m[1], m[2]) for line in done.stdout.splitlines() if (m := MOMENT.match(line))
    ]


def fixture_checks(root: Path) -> list[dict[str, Any]]:
    """One check for each UI fixture's recording, from the seat its human sits in.

    A fixture plays whole decks, so any card file can change what it records: each fixture
    check reads every card file (`CARDS`, a directory whose git tree ID covers them all).
    """
    fixtures = json.loads((root / FIXTURES).read_text())["fixtures"]
    out: list[dict[str, Any]] = []
    for fixture in fixtures:
        # A manual table is seen from seat 0; a game from the seat its `until` stop names.
        seat = re.search(r"seat (\d+)", fixture.get("until") or "")
        view = SEATS[1] if seat and seat[1] != "0" else SEATS[0]
        out.append(
            {
                "check": f"script:fixtures/{fixture['name']}/{view}",
                "inputs": [FIXTURES, CARDS],
            }
        )
    return out


def script_checks(root: Path, cards: Sequence[str]) -> list[dict[str, Any]]:
    files = card_files(root)
    wanted = [slug(c) for c in cards]
    unknown = [c for c in wanted if c != "fixtures" and c not in files]
    if unknown:
        raise ValueError(f"no card file for {', '.join(unknown)}")
    named = [c for c in wanted if c != "fixtures"]
    out: list[dict[str, Any]] = []
    if named or not wanted:
        for card, moment in moments(root, named):
            for seat in SEATS:
                out.append(
                    {
                        "check": f"script:{card}/{moment}/{seat}",
                        "inputs": used_files(root, card, files),
                    }
                )
    if not wanted or "fixtures" in wanted:
        out += fixture_checks(root)
    return out


def list_checks(family: str, root: Path, cards: Sequence[str]) -> list[dict[str, Any]]:
    if family == "script":
        return script_checks(root, cards)
    if family == "app":
        return [{"check": "app:smoke", "inputs": []}]
    return [{"check": f"mobile:{t}", "inputs": []} for t in MOBILE_TARGETS]


# --- run ---


def clip(text: str) -> str:
    text = " ".join(text.split())
    return text if len(text) <= MESSAGE_LIMIT else text[: MESSAGE_LIMIT - 1] + "…"


def append(results: Path, lines: Iterable[dict[str, Any]]) -> None:
    with results.open("a") as out:
        for line in lines:
            out.write(json.dumps(line) + "\n")


def failures(output: str) -> dict[str, str]:
    """The panic text of each check `script-check` printed as `FAILED: <check>` followed by
    indented lines."""
    found: dict[str, list[str]] = {}
    current: list[str] | None = None
    for line in output.splitlines():
        if line.startswith("FAILED: "):
            current = found.setdefault(line[len("FAILED: ") :], [])
        elif current is not None and line.startswith("    "):
            current.append(line.strip())
        else:
            current = None
    return {check: clip(" ".join(lines)) for check, lines in found.items()}


def run_script(names: Sequence[str], root: Path, results: Path) -> None:
    """Runs the checks named through `script-check`, which writes its own result lines to a
    scratch file; they are copied on without its `seconds`, a failure with its panic text.
    """
    with tempfile.TemporaryDirectory() as scratch:
        scratch_results = Path(scratch) / "results.jsonl"
        done = subprocess.run(
            [
                str(binary(root)),
                "script-check",
                str(root / FIXTURES),
                *[n.removeprefix("script:") for n in names],
            ],
            cwd=root / "rust",
            env={**os.environ, "BERYL_CHECK_RESULTS": str(scratch_results)},
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
        )
        panics = failures(done.stdout)
        if not scratch_results.exists():
            return
        lines: list[dict[str, Any]] = []
        for text in scratch_results.read_text().splitlines():
            line = json.loads(text)
            result: dict[str, Any] = {
                "check": line["check"],
                "outcome": line["outcome"],
            }
            if line["outcome"] == "fail":
                result["message"] = panics.get(line["check"], "failed")
            lines.append(result)
        append(results, lines)


def run_app(root: Path) -> dict[str, Any] | None:
    done = subprocess.run(
        ["just", "smoke"], cwd=root, capture_output=True, text=True, check=False
    )
    if done.returncode == 0:
        return {"outcome": "pass"}
    if done.returncode == CANNOT_RUN:
        return None
    return {
        "outcome": "fail",
        "message": clip((done.stdout + done.stderr)[-2000:]) or "smoke failed",
    }


def ndk_env() -> dict[str, str]:
    """The environment for plain cargo building C for Android with the NDK's clang."""
    prebuilt = ANDROID_HOME / "ndk" / NDK_VERSION / "toolchains" / "llvm" / "prebuilt"
    clang = prebuilt / "darwin-x86_64" / "bin" / f"aarch64-linux-android{MIN_SDK}-clang"
    return {
        **os.environ,
        "CC_aarch64_linux_android": str(clang),
        "AR_aarch64_linux_android": str(clang.with_name("llvm-ar")),
    }


def run_mobile(target: str, root: Path) -> dict[str, Any] | None:
    # From ui/, where Xcode's and Gradle's build scripts run cargo, so this resolves the
    # toolchain the way the phone builds do.
    done = subprocess.run(
        [
            "cargo",
            "check",
            "--locked",
            "--manifest-path",
            str(root / "rust" / "Cargo.toml"),
            "-p",
            "beryl_app",
            "--target",
            target,
        ],
        cwd=root / "ui",
        env=ndk_env(),
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode == 0:
        return {"outcome": "pass"}
    # Cargo exits 101 when compiling failed; another code is a failure to run it.
    if done.returncode != 101:
        return None
    return {"outcome": "fail", "message": clip(done.stderr[-2000:])}


def run_checks(family: str, names: Sequence[str], root: Path, results: Path) -> None:
    if family == "script":
        run_script(names, root, results)
        return
    for name in names:
        result = (
            run_app(root)
            if family == "app"
            else run_mobile(name.removeprefix("mobile:"), root)
        )
        if result is not None:
            append(results, [{"check": name, **result}])


def main(argv: Sequence[str]) -> int:
    root = Path.cwd()
    if len(argv) < 2 or argv[0] not in ("list", "run") or argv[1] not in FAMILIES:
        print(__doc__, file=sys.stderr)
        return 1
    verb, family, rest = argv[0], argv[1], list(argv[2:])
    if verb == "list":
        for line in list_checks(family, root, rest):
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
