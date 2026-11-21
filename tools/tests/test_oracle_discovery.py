"""Classification uses reviewed source, runtime cards and complete assertion exports."""

import hashlib
import subprocess
from dataclasses import replace
from pathlib import Path

import pytest

from beryl_tools import oracle_discovery as discovery
from beryl_tools import oracle_exports, oracles, xmage_java
from beryl_tools.card_store import Json


def registry(*names: str) -> discovery.Registry:
    return discovery.Registry.from_inventory(
        [{"name": name, "faces": [name]} for name in names]
    )


def verified(manifest: discovery.Manifest) -> dict[str, discovery.SourceCheck]:
    return {
        key: discovery.SourceCheck(source.commit, "", "verified")
        for key, source in manifest.sources.items()
    }


def test_a_candidate_becomes_eligible_when_its_last_card_is_implemented() -> None:
    manifest = discovery.Manifest.load(discovery.MANIFEST)
    candidate = next(
        c for c in manifest.candidates if c.id.endswith("testDamageSmallCreature")
    )
    before = discovery.classify(
        candidate,
        registry("Lightning Bolt", "Mountain"),
        manifest.sources,
        verified(manifest),
    )
    after = discovery.classify(
        candidate,
        registry("Lightning Bolt", "Mountain", "Sejiri Merfolk"),
        manifest.sources,
        verified(manifest),
    )
    assert before.missing_cards == ["Sejiri Merfolk"]
    assert before.status == "blocked"
    assert after.missing_cards == []
    assert after.status == "eligible"


def test_a_present_card_can_still_need_a_face_or_unhandled_forms() -> None:
    manifest = discovery.Manifest.load(discovery.MANIFEST)
    candidate = next(
        c for c in manifest.candidates if c.id.endswith("testCopyTransformedDFC")
    )
    prowler = "Kessig Prowler"
    faces = [prowler]
    cards: list[Json] = [
        {"name": "Rite of Replication", "faces": ["Rite of Replication"]},
        {"name": "Tropical Island", "faces": ["Tropical Island"]},
        {"name": prowler, "faces": faces},
    ]
    before = discovery.classify(
        candidate,
        discovery.Registry.from_inventory(cards),
        manifest.sources,
        verified(manifest),
    )
    faces.append("Sinuous Predator")
    implemented = discovery.Registry.from_inventory(cards)
    after = discovery.classify(
        candidate, implemented, manifest.sources, verified(manifest)
    )
    assert before.missing_cards == after.missing_cards == []
    assert before.missing_faces == [discovery.Requirement(prowler, "Sinuous Predator")]
    assert after.missing_faces == []
    assert implemented.aliases["Sinuous Predator"] == prowler
    assert implemented.aliases["Kessig Prowler // Sinuous Predator"] == prowler
    assert after.status == "blocked"
    assert after.unhandled_forms["actions"]
    assert after.unhandled_forms["assertions"]


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


def test_fingerprinting_uses_the_pinned_files_and_includes_external_helpers(
    tmp_path: Path,
) -> None:
    git(tmp_path, "init", "--quiet")
    fixture = tmp_path / "Fixture.java"
    helper = tmp_path / "Helper.java"
    fixture.write_text("class Fixture { void test() {} }\n")
    helper.write_text("class Helper {}\n")
    git(tmp_path, "add", ".")
    git(
        tmp_path,
        "-c",
        "user.name=Beryl",
        "-c",
        "user.email=beryl@example.com",
        "commit",
        "--no-gpg-sign",
        "-qm",
        "fixture",
    )
    first_pin = git(tmp_path, "rev-parse", "HEAD")
    digest = hashlib.sha256()
    for file in (fixture, helper):
        digest.update(
            file.name.encode() + b"\0" + hashlib.sha256(file.read_bytes()).digest()
        )
    source = discovery.Source(
        "xmage", first_pin, fixture.name, "test", "1", digest.hexdigest(), [helper.name]
    )
    oracle = replace(oracles.XMAGE, commit=first_pin)
    checkout = discovery.PinnedCheckout(oracle, tmp_path)
    helper.write_text("class Helper { int changed; }\n")
    assert checkout.check(source).status == "verified"
    git(tmp_path, "add", ".")
    git(
        tmp_path,
        "-c",
        "user.name=Beryl",
        "-c",
        "user.email=beryl@example.com",
        "commit",
        "--no-gpg-sign",
        "-qm",
        "helper",
    )
    next_pin = git(tmp_path, "rev-parse", "HEAD")
    checkout = discovery.PinnedCheckout(replace(oracle, commit=next_pin), tmp_path)
    assert checkout.check(source).status == "changed"
    assert checkout.check(replace(source, path="Missing.java")).status == "missing"
    assert (
        discovery.PinnedCheckout(oracle, tmp_path / "absent").check(source).status
        == "unavailable"
    )
    assert (
        discovery.PinnedCheckout(replace(oracle, commit="f" * 40), tmp_path)
        .check(source)
        .status
        == "unavailable"
    )


def test_replay_requires_each_reviewed_fact_and_no_oracle_assertion_skips() -> None:
    manifest = discovery.Manifest.load(discovery.MANIFEST)
    candidate = next(c for c in manifest.candidates if c.adaptation is not None)
    adaptation = candidate.adaptation
    assert adaptation is not None
    original, copy = adaptation.required_assertions
    actions = [{"action": "Assert", "assertion": fact} for fact in (original, copy)]
    entry: Json = {
        "card": "Clever Impersonator",
        "name": "Clever Impersonator copies transformed Jace",
        "actions": actions,
        "snapshot": 1,
    }
    assert discovery.missing_assertions(entry, adaptation) == [copy]
    entry["snapshot"] = 2
    assert not discovery.missing_assertions(entry, adaptation)

    xmage = xmage_java.Export(
        str(entry["card"]),
        str(entry["name"]),
        "test",
        skipped=["ManaValue: in the middle of a ChooseUpTo decision"],
    )
    forge: Json = {
        "card": entry["card"],
        "name": entry["name"],
        "steps": [{"op": "assert", "assertion": original}],
    }
    exports = oracle_exports.Exports([xmage], [forge], [])
    assert discovery.replay_status(exports, entry) == {
        "xmage": "ManaValue: in the middle of a ChooseUpTo decision",
        "forge": "checks 1 of 2 assertions",
    }
    xmage.skipped.clear()
    exports.forge.clear()
    exports.skipped.append({**forge, "reason": "fixture unavailable"})
    assert discovery.replay_status(exports, entry) == {
        "xmage": "replayable",
        "forge": "fixture unavailable",
    }


def test_report_filters_a_back_face_and_reports_a_missing_checkout(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(oracles, "checkout", lambda oracle: tmp_path / oracle.name)
    monkeypatch.setattr(oracles, "run", lambda *_args, **_kwargs: "")

    def inventory(_binary: Path, command: str) -> list[Json]:
        assert command == "inventory"
        return [
            {"name": "Island", "faces": ["Island"]},
            {"name": "Clever Impersonator", "faces": ["Clever Impersonator"]},
            {
                "name": "Jace, Vryn's Prodigy",
                "faces": ["Jace, Vryn's Prodigy", "Jace, Telepath Unbound"],
            },
        ]

    monkeypatch.setattr(discovery, "binary_json", inventory)
    manifest = discovery.Manifest.load(discovery.MANIFEST)
    [finding] = discovery.report(manifest, "Jace, Telepath Unbound")
    assert finding.candidate.id == "clever-impersonator-copies-transformed-jace"
    assert finding.status == "source review"
    assert not finding.missing_cards
    assert not finding.missing_faces
    assert [source["status"] for source in finding.sources] == ["unavailable"] * 2
    assert all(source["reason"] for source in finding.sources)
