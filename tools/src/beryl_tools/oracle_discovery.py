"""Report reviewed upstream fixtures that Beryl has enough coverage to adapt or replay."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
from collections.abc import Iterable, Mapping
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import cast

from . import oracle_exports, oracles, paths, system
from .card_store import Json

MANIFEST: Path = paths.REPO / "data" / "oracle-candidates.json"

# Forms whose translation into a Beryl scenario has been reviewed. A manifest
# entry can name a more specific, still unhandled form even when related syntax
# exists. Exporter support for a particular adaptation is checked separately.
SUPPORTED: dict[str, set[str]] = {
    "setup": {"hand", "battlefield", "transformed permanent"},
    "actions": {
        "cast",
        "choose copy",
        "resolve",
        "target creature",
        "target player",
        "choose a triggered ability's target",
        "advance to step",
    },
    "assertions": {
        "life total",
        "zone count by name",
        "controlled count by name",
        "loyalty counters",
        "exact battlefield mana value",
    },
}


@dataclass(frozen=True)
class Source:
    oracle: str
    commit: str
    path: str
    method: str
    lines: str
    fingerprint: str
    helpers: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class Requirement:
    card: str
    face: str | None = None


@dataclass(frozen=True)
class Adaptation:
    scenario: str
    required_assertions: list[Json]


@dataclass(frozen=True)
class Candidate:
    id: str
    sources: list[str]
    cards: list[Requirement]
    setup: list[str]
    actions: list[str]
    assertions: list[str]
    witness: str
    adaptation: Adaptation | None = None


@dataclass(frozen=True)
class Manifest:
    sources: dict[str, Source]
    candidates: list[Candidate]

    @staticmethod
    def load(path: Path) -> Manifest:
        raw = json.loads(path.read_text(encoding="utf-8"))
        return Manifest(
            {key: Source(**source) for key, source in raw["sources"].items()},
            [
                Candidate(
                    **{
                        **entry,
                        "cards": [Requirement(**req) for req in entry["cards"]],
                        "adaptation": (
                            Adaptation(**entry["adaptation"])
                            if "adaptation" in entry
                            else None
                        ),
                    }
                )
                for entry in raw["candidates"]
            ],
        )


@dataclass(frozen=True)
class Registry:
    faces: dict[str, list[str]]
    aliases: dict[str, str]

    @staticmethod
    def from_inventory(entries: Iterable[Mapping[str, object]]) -> Registry:
        faces = {
            cast(str, entry["name"]): cast(list[str], entry["faces"])
            for entry in entries
        }
        aliases = {
            alias: name
            for name, names in faces.items()
            for alias in (*names, " // ".join(names))
        }
        # An actual definition wins if a face elsewhere has the same name.
        aliases.update({name: name for name in faces})
        return Registry(faces, aliases)

    def supports(self, req: Requirement) -> bool:
        return req.card in self.faces and (
            req.face is None or req.face in self.faces[req.card]
        )


@dataclass(frozen=True)
class SourceCheck:
    configured_commit: str
    url: str
    status: str
    reason: str | None = None


def git(checkout: Path, *args: str) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        ["git", "-C", str(checkout), *args], capture_output=True, check=False
    )


class PinnedCheckout:
    def __init__(self, oracle: oracles.Oracle, path: Path) -> None:
        self.oracle = oracle
        self.path = path
        self.available: bool = (path / ".git").exists() and git(
            path, "cat-file", "-e", f"{oracle.commit}^{{commit}}"
        ).returncode == 0
        self.blobs: dict[str, bytes | None] = {}

    def blob(self, path: str) -> bytes | None:
        if path not in self.blobs:
            result = git(self.path, "cat-file", "blob", f"{self.oracle.commit}:{path}")
            self.blobs[path] = result.stdout if result.returncode == 0 else None
        return self.blobs[path]

    def check(self, source: Source) -> SourceCheck:
        url = f"{self.oracle.repo}/blob/{source.commit}/{source.path}#L{source.lines}"
        pin = self.oracle.commit
        if not self.available:
            return SourceCheck(
                pin,
                url,
                "unavailable",
                f"{source.oracle} checkout lacks the configured pin",
            )
        # Hash the whole fixture class, including local constants and helpers, and the
        # explicitly listed supporting files. A change anywhere in them requires review.
        digest = hashlib.sha256()
        for path in sorted({source.path, *source.helpers}):
            content = self.blob(path)
            if content is None:
                return SourceCheck(pin, url, "missing", f"no source at {pin}:{path}")
            digest.update(path.encode())
            digest.update(b"\0")
            digest.update(hashlib.sha256(content).digest())
        if digest.hexdigest() != source.fingerprint:
            return SourceCheck(
                pin, url, "changed", "fixture or helper fingerprint changed"
            )
        return SourceCheck(pin, url, "verified")

    def replay_blocker(self) -> str | None:
        if not self.available:
            return f"{self.oracle.name} checkout lacks the configured pin"
        if oracles.head(self.path) != self.oracle.commit:
            return f"{self.oracle.name} working checkout is not at the configured pin"
        return None


@dataclass(frozen=True)
class Finding:
    candidate: Candidate
    sources: list[dict[str, object]]
    missing_cards: list[str]
    missing_faces: list[Requirement]
    unhandled_forms: dict[str, list[str]]
    status: str
    replay: dict[str, str]


def classify(
    candidate: Candidate,
    registry: Registry,
    sources: dict[str, Source],
    checks: dict[str, SourceCheck],
    replay: dict[str, str] | None = None,
) -> Finding:
    missing_cards = sorted(
        {r.card for r in candidate.cards if r.card not in registry.faces}
    )
    missing_faces = [
        r
        for r in candidate.cards
        if r.card in registry.faces and not registry.supports(r)
    ]
    unhandled = {
        kind: [form for form in getattr(candidate, kind) if form not in supported]
        for kind, supported in SUPPORTED.items()
    }
    if any(checks[key].status != "verified" for key in candidate.sources):
        status = "source review"
    elif missing_cards or missing_faces or any(unhandled.values()):
        status = "blocked"
    elif candidate.adaptation is None:
        status = "eligible"
    elif replay and "replayable" in replay.values():
        status = "replayable"
    else:
        status = "adaptation blocked"
    return Finding(
        candidate,
        [{**asdict(sources[key]), **asdict(checks[key])} for key in candidate.sources],
        missing_cards,
        missing_faces,
        unhandled,
        status,
        replay or {},
    )


def replay_status(exports: oracle_exports.Exports, entry: Json) -> dict[str, str]:
    """Check the normal export, holding any refusal or skipped assertion against replay."""
    name = oracle_exports.label(entry)
    xmage = next(e for e in exports.xmage if f"{e.card} / {e.scenario}" == name)
    forge_refusal = next(
        (e for e in exports.skipped if oracle_exports.label(e) == name), None
    )
    if forge_refusal is not None:
        forge_status = str(forge_refusal["reason"])
    else:
        forge = next(e for e in exports.forge if oracle_exports.label(e) == name)
        expected = sum(
            a["action"] == "Assert" for a in cast(list[Json], entry["actions"])
        )
        actual = sum(s["op"] == "assert" for s in cast(list[Json], forge["steps"]))
        forge_status = (
            "replayable"
            if actual == expected
            else f"checks {actual} of {expected} assertions"
        )
    return {
        "xmage": xmage.unsupported or ", ".join(xmage.skipped) or "replayable",
        "forge": forge_status,
    }


def missing_assertions(entry: Json, adaptation: Adaptation) -> list[Json]:
    """Find reviewed facts absent from the authored actions before the generated snapshot."""
    actions = cast(list[Json], entry["actions"])
    snapshot = cast("int | None", entry.get("snapshot"))
    authored = [a["assertion"] for a in actions[:snapshot] if a["action"] == "Assert"]
    return [fact for fact in adaptation.required_assertions if fact not in authored]


def binary_json(binary: Path, *args: str) -> object:
    result = subprocess.run(
        [str(binary), *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if result.returncode:
        raise oracles.StepFailed(
            result.stderr.strip() or f"beryl_scenario {args[0]} failed"
        )
    return json.loads(result.stdout)


def report(manifest: Manifest, card: str | None = None) -> list[Finding]:
    with tempfile.TemporaryDirectory(prefix="beryl-oracle-candidates-") as tmp:
        log = Path(tmp) / "build.log"
        oracles.run(
            paths.CLI_BUILD, log, paths.ENGINE, CARGO_TARGET_DIR=str(paths.RUN_TARGET)
        )
        binary = system.exe(paths.RUN_TARGET / "debug" / "beryl_scenario")
        registry = Registry.from_inventory(
            cast(list[Json], binary_json(binary, "inventory"))
        )
        canonical = registry.aliases.get(card, card) if card is not None else None
        selected = [
            c
            for c in manifest.candidates
            if card is None
            or any(r.card == canonical or r.face == card for r in c.cards)
        ]
        checkouts = {
            o.name: PinnedCheckout(o, oracles.checkout(o)) for o in oracles.ORACLES
        }
        checks = {
            key: checkouts[manifest.sources[key].oracle].check(manifest.sources[key])
            for key in sorted({key for c in selected for key in c.sources})
        }
        findings = [classify(c, registry, manifest.sources, checks) for c in selected]
        adaptations = {
            a.scenario: a
            for f in findings
            if f.status == "adaptation blocked"
            and (a := f.candidate.adaptation) is not None
        }
        if not adaptations:
            return findings
        dump = cast(list[Json], binary_json(binary, "dump"))
        entries = {
            oracle_exports.label(e): e
            for e in dump
            if oracle_exports.label(e) in adaptations
        }
        exports = oracle_exports.Exports.prepare(
            list(entries.values()), checkouts["xmage"].path
        )
        availability = {name: c.replay_blocker() for name, c in checkouts.items()}

        for index, finding in enumerate(findings):
            adaptation = finding.candidate.adaptation
            if finding.status != "adaptation blocked" or adaptation is None:
                continue
            entry = entries.get(adaptation.scenario)
            if entry is None:
                replay = {
                    name: "reviewed Beryl scenario is missing" for name in checkouts
                }
            elif missing := missing_assertions(entry, adaptation):
                reason = f"reviewed assertions are missing: {json.dumps(missing)}"
                replay = {name: reason for name in checkouts}
            else:
                replay = replay_status(exports, entry)
                replay.update(
                    {name: reason for name, reason in availability.items() if reason}
                )
            findings[index] = classify(
                finding.candidate, registry, manifest.sources, checks, replay
            )
        return findings


def card_diagnostics(card: str) -> None:
    """Print named-card discovery results after the ordinary oracle checks."""
    try:
        manifest = Manifest.load(MANIFEST)
        # Card check already has the canonical name. Unrelated cards need no registry build.
        if not any(r.card == card for c in manifest.candidates for r in c.cards):
            return
        findings = report(manifest, card)
    except (OSError, ValueError, oracles.StepFailed) as e:
        print(f"oracle candidates: report unavailable ({e})")
        return
    for finding in findings:
        print(f"oracle candidate: {finding.candidate.id}: {finding.status}")
        for source in finding.sources:
            if source["reason"]:
                print(f"  {source['method']}: {source['reason']}")
        if finding.missing_cards:
            print(f"  missing cards: {', '.join(finding.missing_cards)}")
        for req in finding.missing_faces:
            print(f"  missing face: {req.face} ({req.card})")
        for kind, forms in finding.unhandled_forms.items():
            if forms:
                print(f"  unhandled {kind}: {', '.join(forms)}")
        for name, status in finding.replay.items():
            print(f"  {name}: {status}")


def run(args: argparse.Namespace) -> int:
    try:
        findings = report(Manifest.load(MANIFEST), args.card)
    except (OSError, ValueError, oracles.StepFailed) as e:
        print(f"oracle-candidates: {e}", file=sys.stderr)
        return 1
    print(json.dumps([asdict(f) for f in findings], indent=2, ensure_ascii=False))
    return 0


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    parser = commands.add_parser(
        "oracle-candidates", help="report upstream fixtures Beryl could adapt or replay"
    )
    parser.add_argument("--card", help="show candidates that require this card or face")
    parser.set_defaults(func=run)
