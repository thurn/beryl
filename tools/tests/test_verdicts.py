import base64
import re
from pathlib import Path

from beryl_tools import shots, verdicts

FLAT = bytes([100] * 192)


def sketch(*changes: tuple[int, int]) -> str:
    """An 8x8 RGB fingerprint, flat grey but for (byte, delta) changes."""
    data = bytearray(FLAT)
    for i, delta in changes:
        data[i] += delta
    return base64.b64encode(bytes(data)).decode()


def ledger_raw() -> verdicts.Json:
    return {
        "mockups": {
            "m": {
                "file": "m.png",
                "masks": [],
                "art": {},
                "parts": {
                    "passed": {
                        "rect": [0, 0, 10, 10],
                        "verdict": "pass",
                        "bead": "by-1",
                        "commit": "abc1234",
                        "fingerprint": sketch(),
                        "accepted": ["by-1 residual: painted grain"],
                    },
                    "differing": {
                        "rect": [0, 0, 10, 10],
                        "verdict": "differs",
                        "bead": "by-1",
                        "commit": "abc1234",
                    },
                    "new": [0, 0, 10, 10],
                },
            }
        }
    }


def test_the_region_file_keeps_its_layout() -> None:
    assert verdicts.dumps(verdicts.load()) == shots.REGIONS.read_text()


def test_the_ledger_is_well_formed() -> None:
    for name, spec in verdicts.parts(verdicts.load()).items():
        if "verdict" in spec:
            assert spec["verdict"] in verdicts.VERDICTS, name
            assert re.fullmatch(r"by-[a-z0-9.]+", str(spec["bead"])), name
            assert re.fullmatch(r"[0-9a-f]{7,}\+?", str(spec["commit"])), name
            has_print = "fingerprint" in spec
            assert has_print == (spec["verdict"] == "pass"), name
            if has_print:
                assert verdicts.drift(str(spec["fingerprint"]), sketch()) < 255, name
        for line in spec.get("accepted", []):  # pyre-ignore[16]: JSON
            assert re.match(r"by-[a-z0-9.]+ (residual|conflict): \S", line), name


def test_a_passed_part_is_reported_once_its_fingerprint_moves() -> None:
    ledger = verdicts.parts(ledger_raw())
    still = {"m/passed": sketch((5, 1), (90, -2))}
    assert verdicts.changed_since_pass(ledger, still) == {}
    moved = {"m/passed": sketch((5, 12)), "m/differing": sketch((0, 50))}
    assert verdicts.changed_since_pass(ledger, moved) == {"m/passed": 12}
    assert "m/passed: by-1's pass at abc1234 (drift 12)" in verdicts.report(
        ledger, {**moved, "m/new": sketch()}
    )
    assert verdicts.report(ledger, moved | {"m/new": sketch()}).startswith(
        "ledger: 1 pass, 1 differs, 1 unreviewed"
    )


def test_a_fingerprint_of_another_shape_counts_as_changed() -> None:
    ledger = verdicts.parts(ledger_raw())
    assert verdicts.changed_since_pass(ledger, {"m/passed": "AAAA"}) == {
        "m/passed": 255
    }


def test_recording_verdicts_and_accepted_lines() -> None:
    raw = ledger_raw()
    prints = {"m/new": sketch((3, 9)), "m/passed": sketch((1, 1))}
    verdicts.record(
        raw,
        ["m/new", "m/passed"],
        verdict="pass",
        bead="by-2",
        commit="def5678+",
        fingerprints=prints,
        residuals=["painted grain"],
        conflicts=["the sheet disagrees"],
    )
    parts = verdicts.parts(raw)
    assert parts["m/new"] == {
        "rect": [0, 0, 10, 10],
        "verdict": "pass",
        "bead": "by-2",
        "commit": "def5678+",
        "fingerprint": prints["m/new"],
        "accepted": [
            "by-2 residual: painted grain",
            "by-2 conflict: the sheet disagrees",
        ],
    }
    assert parts["m/passed"]["accepted"] == [
        "by-1 residual: painted grain",
        "by-2 residual: painted grain",
        "by-2 conflict: the sheet disagrees",
    ]
    verdicts.record(
        raw,
        ["m/passed"],
        verdict="differs",
        bead="by-3",
        commit="0123456",
        fingerprints={},
        residuals=[],
        conflicts=[],
    )
    after = verdicts.parts(raw)["m/passed"]
    assert after["verdict"] == "differs" and "fingerprint" not in after
    assert len(after["accepted"]) == 3  # pyre-ignore[6]: JSON


def test_a_packet_carries_the_rubric_images_and_accepted_lines(
    tmp_path: Path,
) -> None:
    ledger = verdicts.parts(ledger_raw())
    [prompt] = verdicts.write_packets(tmp_path, ledger, ["m/passed"], 2)
    assert prompt == tmp_path / "m" / "passed" / "prompt.md"
    text = prompt.read_text()
    assert verdicts.RUBRIC in text
    assert "`VERDICT: pass` or `VERDICT: differs`" in text
    for image in ("side-by-side.png", "onion.png", "diff.png"):
        assert str(tmp_path / "m" / "passed" / image) in text
    assert "10x10 px rectangle at 0,0 of the mockup, shown at 2x." in text
    assert text.endswith(
        "## Accepted (do not raise these again)\n\n" "- by-1 residual: painted grain\n"
    )
    [fresh] = verdicts.write_packets(tmp_path, ledger, ["m/new"], 1)
    assert "None yet." in fresh.read_text()
