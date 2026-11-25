"""The adapter for the Rust families of `ignite/checks/rust.toml` that cargo-nextest does not
run: clippy, formatting, doctests, the `allocations` binaries and the card-text check.

`python -m beryl_tools.ignite.rust <family> list` prints the family's checks, and
`python -m beryl_tools.ignite.rust <family> run <check>...` runs those named and reports one
result per check (`beryl_tools.ignite.report`). `clippy build` makes the `clippy` build product
with the same flags as the clippy run, so the run compiles nothing the product built.

Every family but card-text has one check per workspace package, `<family>:<package>`, whose
own input is `crate:<package>` (or, for formatting, the package's directory). The package left
to the nightly, `beryl_link_iroh`, is in none of them, as in today's rust lane.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast

from .. import paths
from . import Listed, Result, listed, report

# The card-text family's modules are imported inside its functions. Imported here, they put
# every card module among the inputs of each family this adapter serves (the input lists
# follow the imports made when the adapter loads; tools/tests/test_ignite_inputs.py), so a
# comment edited in card.py reran clippy, the doctests and the `allocations` binaries, and
# rebuilt the workspace's test binaries for them.
if TYPE_CHECKING:
    from .. import card_store, render_diff

# The package the families leave out: its iroh dependency's cold build alone outlasts a check.
LEFT_OUT = "beryl_link_iroh"
SCOPE: list[str] = ["--workspace", "--exclude", LEFT_OUT]

# Clippy's own target directory, relative to `rust/` (`paths.CLIPPY_TARGET` says why).
CLIPPY_TARGET = "target/clippy"

# Clippy with every lint capped at a warning. The workspace denies every warning
# (`[workspace.lints.rust] warnings = "deny"` in engine/Cargo.toml), so without the cap a lint
# fails its crate's unit, cargo keeps no result for it, and the run after the `clippy` product
# would compile that crate again. Capped, every unit succeeds and cargo replays its cached
# warnings on the next run, which reads each workspace package's warning as that package's
# failure. A type error is still an error, and fails the product's build.
# The Windows target sets its own rustflags in .cargo/config.toml, which overrides build.rustflags.
CLIPPY_FLAGS: str = (
    "target.x86_64-pc-windows-msvc.rustflags"
    if sys.platform == "win32"
    else "build.rustflags"
)
CLIPPY: list[str] = [
    "cargo",
    "clippy",
    "--locked",
    *SCOPE,
    "--all-targets",
    "--target-dir",
    CLIPPY_TARGET,
    "--config",
    f'{CLIPPY_FLAGS}=["--cap-lints=warn"]',
]

# Where the `scenario-dump` product leaves `beryl_scenario`, relative to `rust/`: it builds in
# the `test` profile (paths.CLI_BUILD), whose binaries land in target/debug.
SCENARIO_BINARY: Path = Path("target") / "debug" / "beryl_scenario"

# The check of the card-text family that holds allowlist entries naming no card file.
ALLOWLIST_CHECK = "card-text:allowlist"

# A Markdown code fence in a `///` or `//!` doc comment, and the tags after its backticks.
DOC_FENCE: re.Pattern[str] = re.compile(r"^\s*//[/!]\s*```\s*(.*?)\s*$")
# The tags that leave a fenced block a doctest rustdoc compiles (`ignore` and other languages,
# such as `text`, do not).
DOCTEST_TAGS: frozenset[str] = frozenset(
    {"", "rust", "compile_fail", "should_panic", "no_run", "test_harness"}
)

# The line that ends each batch of a package's doctests.
TEST_RESULT: re.Pattern[str] = re.compile(r"^test result: (ok|FAILED)\.")
# rustfmt --check's line naming a file that is not formatted.
RUSTFMT_DIFF: re.Pattern[str] = re.compile(
    r"^Diff in (.+?\.rs)(?::(\d+))?(?::| at line (\d+))"
)


@dataclass(frozen=True)
class Package:
    """A workspace package: its name, its directory relative to the checkout and its targets as
    `cargo metadata` describes them."""

    name: str
    dir: str
    targets: list[dict[str, object]]


def packages() -> list[Package]:
    """The workspace's packages but `LEFT_OUT`, from `cargo metadata`."""
    run = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        cwd=paths.ENGINE,
        capture_output=True,
        text=True,
        check=True,
    )
    return parse_packages(run.stdout, paths.REPO)


def parse_packages(metadata: str, checkout: Path) -> list[Package]:
    """The packages in `cargo metadata` JSON, with directories relative to `checkout`."""
    found: list[Package] = []
    for package in cast(list[dict[str, object]], json.loads(metadata)["packages"]):
        name = str(package["name"])
        if name == LEFT_OUT:
            continue
        manifest = Path(str(package["manifest_path"])).parent
        found.append(
            Package(
                name,
                manifest.relative_to(checkout).as_posix(),
                cast(list[dict[str, object]], package.get("targets") or []),
            )
        )
    return sorted(found, key=lambda p: p.name)


def per_package(
    family: str, chosen: Iterable[Package], named: bool = False
) -> list[Listed]:
    """One check per package. `named` also names the package on the listing line, for a
    family whose products Ignite builds per package."""
    return [
        Listed(f"{family}:{p.name}", [f"crate:{p.name}"], p.name if named else "")
        for p in chosen
    ]


def asked(family: str, checks: Iterable[str]) -> list[str]:
    """The names after `<family>:` of the checks named, in order; others are ignored."""
    prefix = f"{family}:"
    return [c.removeprefix(prefix) for c in checks if c.startswith(prefix)]


# Clippy.


def package_name(message: dict[str, object]) -> str:
    """The package a cargo JSON message is about. Its `package_id` is
    `path+file:///.../<dir>#<name>@<version>`, or `...#<version>` when the name is the
    directory's."""
    package_id = str(message.get("package_id") or "")
    location, _, spec = package_id.rpartition("#")
    name, at, _ = spec.partition("@")
    return name if at else location.rstrip("/").rsplit("/", 1)[-1]


def clippy_results(
    output: str,
    wanted: list[str],
    returncode: int,
    *,
    family: str = "clippy",
    directory: str = "engine",
) -> list[Result]:
    """A package warning or error is a failure. A pass needs both an artifact for the
    package and a completed Cargo build, since later targets may still have diagnostics.
    """
    reached: set[str] = set()
    diagnostics: dict[str, list[dict[str, object]]] = {}
    finished = False
    for line in output.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(message, dict):
            continue
        reason = message.get("reason")
        name = package_name(cast(dict[str, object], message))
        if reason == "compiler-artifact":
            reached.add(name)
        elif reason == "build-finished":
            finished = message.get("success") is True
        elif reason == "compiler-message":
            body = cast(dict[str, object], message.get("message") or {})
            if body.get("level") in ("warning", "error"):
                diagnostics.setdefault(name, []).append(body)
    incomplete = f"cargo clippy exited with status {returncode}"
    if returncode != 0:
        incomplete += " before confirming the package result"
    elif not finished:
        incomplete += " without a successful build-finished message"
    else:
        incomplete += " without reporting an artifact for this package"
    results: list[Result] = []
    for name in wanted:
        check = f"{family}:{name}"
        found = diagnostics.get(name)
        if found:
            results.append(diagnostic_result(check, found, directory))
        elif returncode == 0 and finished and name in reached:
            results.append(Result(check, "pass"))
        else:
            results.append(Result(check, "incomplete", incomplete))
    return results


def diagnostic_result(
    check: str, found: list[dict[str, object]], directory: str
) -> Result:
    """A failure naming the first diagnostic's place, with every diagnostic rendered."""
    spans = cast(list[dict[str, object]], found[0].get("spans") or [])
    primary = next((s for s in spans if s.get("is_primary")), None)
    at = (
        f"{directory}/{primary['file_name']}:{primary['line_start']}" if primary else ""
    )
    rendered = "\n".join(
        str(d.get("rendered") or d.get("message")).rstrip() for d in found
    )
    return Result(check, "fail", rendered, at)


def clippy_build() -> int:
    return subprocess.run(CLIPPY, cwd=paths.ENGINE, check=False).returncode


def clippy_run(
    checks: list[str],
    *,
    command: list[str] = CLIPPY,
    workspace: Path = paths.ENGINE,
    family: str = "clippy",
) -> list[Result]:
    wanted = asked(family, checks)
    if not wanted:
        return []
    run = subprocess.run(
        [*command, "--message-format=json"],
        cwd=workspace,
        stdout=subprocess.PIPE,
        text=True,
        errors="replace",
        check=False,
    )
    results = clippy_results(
        run.stdout,
        wanted,
        run.returncode,
        family=family,
        directory=workspace.relative_to(paths.REPO).as_posix(),
    )
    # Cargo's stderr reaches the runner log directly. Keep the end of its JSON stream
    # when results are missing, and log diagnostics that otherwise only reach JSONL.
    if any(result.outcome == "incomplete" for result in results) and run.stdout.strip():
        print(run.stdout.rstrip()[-4000:], file=sys.stderr)
    for result in results:
        if result.message:
            print(f"{result.check}: {result.message}", file=sys.stderr)
    return results


# Formatting.


def rust_files(package: Package) -> list[Path]:
    """Every Rust file in `package`'s directory. The card crates' files are reached from no
    crate root (a build script writes their `mod` lines), so `cargo fmt` would miss them.
    """
    root = paths.REPO / package.dir
    return sorted(
        p for p in root.rglob("*.rs") if "target" not in p.relative_to(root).parts
    )


def unformatted(output: str) -> dict[Path, str]:
    """Each file `rustfmt --check` printed a diff for, with its first diff's line."""
    found: dict[Path, str] = {}
    for line in output.splitlines():
        match = RUSTFMT_DIFF.match(line)
        if match:
            found.setdefault(
                Path(match.group(1)), match.group(2) or match.group(3) or "1"
            )
    return found


def format_run(checks: list[str]) -> list[Result]:
    by_name = {p.name: p for p in packages()}
    results: list[Result] = []
    for name in asked("rust-format", checks):
        package = by_name.get(name)
        if package is None:
            continue
        files = rust_files(package)
        run = subprocess.run(
            ["rustfmt", "--check", "--edition", "2024", *map(str, files)],
            cwd=paths.ENGINE,
            capture_output=True,
            text=True,
            errors="replace",
            check=False,
        )
        bad = unformatted(run.stdout)
        check = f"rust-format:{name}"
        if run.returncode == 0:
            results.append(Result(check, "pass"))
        elif bad:
            first, line = next(iter(bad.items()))
            where = first.resolve().relative_to(paths.REPO).as_posix()
            listing = "\n".join(
                p.resolve().relative_to(paths.REPO).as_posix() for p in bad
            )
            message = f"not formatted (run `just fmt`):\n{listing}"
            results.append(Result(check, "fail", message, f"{where}:{line}"))
        else:
            results.append(Result(check, "fail", (run.stderr or run.stdout).strip()))
    return results


# Doctests.


def has_doctest(source: str) -> bool:
    """Whether a Rust source file holds a doc-comment code block that rustdoc compiles."""
    opened = False
    for line in source.splitlines():
        fence = DOC_FENCE.match(line)
        if fence is None:
            continue
        if not opened:
            tags = [t.strip() for t in fence.group(1).split(",")]
            if all(t in DOCTEST_TAGS or t.startswith("edition") for t in tags):
                return True
        opened = not opened
    return False


def documented(package: Package) -> bool:
    """Whether `package`'s library holds a doctest."""
    for target in package.targets:
        if not set(cast(list[str], target.get("kind") or [])) & {"lib", "rlib"}:
            continue
        if not target.get("doctest", True):
            continue
        root = Path(str(target["src_path"])).parent
        return any(
            has_doctest(f.read_text(encoding="utf-8", errors="replace"))
            for f in sorted(root.rglob("*.rs"))
        )
    return False


def doctest_result(output: str, name: str) -> Result | None:
    """The result of one package from the output of `cargo test --doc -p <name>`, or `None` when
    the output holds no `test result:` line. The package prints one `test result:` line for each
    batch of doctests rustdoc compiled (plain tests, then `compile_fail` ones), so it passes only
    if every line is `ok`, and fails if any line is `FAILED`.
    """
    verdicts = [m.group(1) for m in map(TEST_RESULT.match, output.splitlines()) if m]
    if not verdicts:
        return None
    if all(v == "ok" for v in verdicts):
        return Result(f"doctests:{name}", "pass")
    return Result(f"doctests:{name}", "fail", output.strip())


def doctests_run(checks: list[str]) -> list[Result]:
    """Runs each package's doctests in its own `cargo test --doc`. One invocation for several
    packages merged cargo's `Doc-tests <crate>` headers (standard error) with rustdoc's results
    (standard output) in one pipe, where the two streams arrive in no fixed order. Under load a
    header could follow another package's results, so a package's section was empty and its
    check was reported incomplete."""
    results: list[Result] = []
    for name in asked("doctests", checks):
        run = subprocess.run(
            ["cargo", "test", "--locked", "--no-fail-fast", "--doc", "-p", name],
            cwd=paths.ENGINE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
            check=False,
        )
        result = doctest_result(run.stdout, name)
        if result is not None:
            results.append(result)
    return results


# The `allocations` binaries.


ALLOCATIONS = "allocations"


def has_allocations(package: Package) -> bool:
    return any(
        t.get("name") == ALLOCATIONS and "test" in cast(list[str], t.get("kind") or [])
        for t in package.targets
    )


def allocation_binaries(listing: str) -> dict[str, str]:
    """Each package's `allocations` binary from `cargo nextest list --list-type binaries-only
    --message-format json`, by package name."""
    found: dict[str, str] = {}
    parsed = cast(dict[str, dict[str, object]], json.loads(listing))
    for binary in parsed.get("rust-binaries", {}).values():
        entry = cast(dict[str, object], binary)
        if entry.get("binary-name") == ALLOCATIONS:
            package = str(entry.get("binary-id", "")).split("::", 1)[0]
            found[package] = str(entry["binary-path"])
    return found


def allocations_run(checks: list[str]) -> list[Result]:
    wanted = asked(ALLOCATIONS, checks)
    if not wanted:
        return []
    # nextest cannot list or run a harness-less target, but lists its binary: the same build
    # as the `rust-tests` product's, so nothing compiles again.
    listing = subprocess.run(
        ["cargo", "nextest", "list", "--locked", "--cargo-quiet"]
        + ["--list-type", "binaries-only", "--message-format", "json"]
        + [a for name in wanted for a in ("-p", name)],
        cwd=paths.ENGINE,
        capture_output=True,
        text=True,
        check=False,
    )
    if listing.returncode != 0:
        return []
    binaries = allocation_binaries(listing.stdout)
    results: list[Result] = []
    for name in wanted:
        binary = binaries.get(name)
        if binary is None:
            continue
        run = subprocess.run(
            [binary],
            cwd=paths.REPO / next(p.dir for p in packages() if p.name == name),
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
            check=False,
        )
        outcome = "pass" if run.returncode == 0 else "fail"
        message = "" if run.returncode == 0 else run.stdout.strip()
        results.append(Result(f"{ALLOCATIONS}:{name}", outcome, message))
    return results


# Card text.


def card_check(path: Path) -> str:
    """A card file's check: `card-text:<crate>/<set>/<card>`, its path under engine/cards."""
    from .. import card_files

    return "card-text:" + path.relative_to(card_files.CARDS).with_suffix("").as_posix()


def card_text_list() -> list[Listed]:
    from .. import card_files

    checks = [
        Listed(card_check(path), [path.relative_to(paths.REPO).as_posix()])
        for path in card_files.files()
    ]
    return [*checks, Listed(ALLOWLIST_CHECK)]


def card_text_results(
    named: dict[str, str],
    diffs: dict[str, render_diff.Diff],
    allowlist: card_store.Json,
    escalations: card_store.Json,
    wanted: list[str],
) -> list[Result]:
    """Each wanted check's result, where `named` maps each card file's check to its card's name,
    `diffs` holds each rendered card's comparison with its Oracle text, and the allowlist and
    escalations are `card_text`'s. A file whose card the store lacks or that did not render
    fails; `ALLOWLIST_CHECK` fails on entries naming no card file."""
    from .. import card_text

    results: list[Result] = []
    for check in wanted:
        if check == ALLOWLIST_CHECK:
            stale = {n: e for n, e in allowlist.items() if n not in diffs}
            found = card_text.problems({}, stale, escalations)
        elif check not in named:
            found = [f"{check}: the card store has no card for this file"]
        elif named[check] not in diffs:
            found = [f"{named[check]} did not render"]
        else:
            name = named[check]
            entry = {name: allowlist[name]} if name in allowlist else {}
            found = card_text.problems({name: diffs[name]}, entry, escalations)
        results.append(Result(check, "fail" if found else "pass", "\n".join(found)))
    return results


def card_text_run(checks: list[str]) -> list[Result]:
    from .. import card_files, card_store, card_text, render_diff, system

    wanted = [c for c in checks if c.startswith("card-text:")]
    if not wanted:
        return []
    texts = render_diff.rendered([], system.exe(paths.ENGINE / SCENARIO_BINARY))
    if isinstance(texts, str):
        print(f"card-text: {texts}", file=sys.stderr)
        return []
    diffs: dict[str, render_diff.Diff] = {}
    named: dict[str, str] = {}
    with card_store.Store(card_store.STORE) as store:
        for name, faces in texts:
            record = cast(card_store.Json, store.card(name))
            canonical = str(record["name"])
            diffs[canonical] = render_diff.compare(
                canonical, render_diff.oracle_faces(record), faces
            )
        for path in card_files.files():
            # A file is named for its card's front face; the store's record has the full name
            # ("Front // Back") that the comparison above is keyed by.
            card = card_files.name_of(path, store)
            record = store.card(card) if card is not None else None
            if record:
                named[card_check(path)] = str(record["name"])
    allowlist = card_text.load(card_text.ALLOWLIST)
    escalations = card_text.load(card_text.ESCALATIONS)
    return card_text_results(named, diffs, allowlist, escalations, wanted)


# The command line.


LISTS: dict[str, Callable[[], list[Listed]]] = {
    "clippy": lambda: per_package("clippy", packages()),
    "rust-format": lambda: [
        Listed(f"rust-format:{p.name}", [p.dir]) for p in packages()
    ],
    "doctests": lambda: per_package("doctests", filter(documented, packages())),
    ALLOCATIONS: lambda: per_package(
        ALLOCATIONS, filter(has_allocations, packages()), named=True
    ),
    "card-text": card_text_list,
}

RUNS: dict[str, Callable[[list[str]], list[Result]]] = {
    "clippy": clippy_run,
    "rust-format": format_run,
    "doctests": doctests_run,
    ALLOCATIONS: allocations_run,
    "card-text": card_text_run,
}


def main(args: list[str]) -> int:
    match args:
        case ["clippy", "build"]:
            return clippy_build()
        case [family, "list"] if family in LISTS:
            listed(LISTS[family]())
            return 0
        case [family, "run", *checks] if family in RUNS:
            results = RUNS[family](checks)
            report(results)
            return int(any(r.outcome == "fail" for r in results))
        case _:
            families = ", ".join(LISTS)
            print(
                f"usage: rust.py <family> list | <family> run <check>... | clippy build\n"
                f"families: {families}",
                file=sys.stderr,
            )
            return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
